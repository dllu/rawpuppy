//! Native, reproducible 512-pixel image/mask comparisons without an editor recipe.
#[cfg(feature = "moebius")]
fn main() -> anyhow::Result<()> {
    use anyhow::ensure;
    use clap::Parser;
    use rawpuppy::{
        color, models,
        moebius::{InferenceDevice, Moebius, Sampling},
        pipeline::Rendered,
    };
    use sha2::{Digest, Sha256};
    use std::{path::PathBuf, time::Instant};

    #[derive(Parser)]
    struct Args {
        #[arg(long)]
        models: PathBuf,
        #[arg(long)]
        image: PathBuf,
        #[arg(
            long,
            required_unless_present = "saved_fill",
            conflicts_with = "saved_fill"
        )]
        mask: Option<PathBuf>,
        /// Render this saved painted fill's exact float context from its original/sidecar.
        #[arg(long)]
        saved_fill: Option<usize>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, value_enum, default_value = "cuda")]
        device: InferenceDevice,
        #[arg(long, default_value_t = 20)]
        steps: usize,
        #[arg(long, default_value_t = 42)]
        seed: i64,
        #[arg(long, default_value_t = 1.)]
        strength: f64,
        #[arg(long, default_value_t = 2)]
        iterations: usize,
        /// Grow the model mask in pixels; commit through the declared target only.
        #[arg(long, default_value_t = 0)]
        mask_padding: usize,
        /// Explicitly widen the committed comparison selection, separately from inference padding.
        #[arg(long, default_value_t = 0)]
        target_padding: usize,
        /// Compare generated float context with a saved EXR asset before final mask composition.
        #[arg(long)]
        reference_context: Option<PathBuf>,
    }
    let args = Args::parse();
    let settings = Sampling {
        steps: args.steps,
        seed: args.seed,
        strength: args.strength,
        ..Default::default()
    };
    settings.validate()?;
    ensure!(args.iterations > 0, "Iterations must be positive");
    ensure!(args.mask_padding <= 512, "Mask padding exceeds the context");
    ensure!(
        args.target_padding <= 512,
        "Target padding exceeds the context"
    );
    ensure!(!args.output.exists(), "Choose a new output directory");
    #[cfg(feature = "cuda")]
    if std::env::var_os("RUST_MIN_STACK").is_none() {
        // SAFETY: startup, before any worker/runtime is created.
        unsafe { std::env::set_var("RUST_MIN_STACK", "33554432") };
    }
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    tch::set_num_threads(8);
    tch::set_num_interop_threads(1);
    let source_hash = models::sha256(&args.image)?;
    let (input, original_mask, saved_context) = if let Some(index) = args.saved_fill {
        use rawpuppy::{
            input::SensorImage,
            render::{Backend, Renderer},
            sidecar, synthesis,
        };
        let recipe_path = sidecar::path_for(&args.image);
        let recipe_hash = models::sha256(&recipe_path)?;
        let mut edits = sidecar::load(&recipe_path)?;
        let fill = edits
            .display
            .synthesis
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Saved fill index does not exist"))?;
        ensure!(
            !fill.fill_gaps,
            "Saved-context comparison currently requires a painted fill"
        );
        ensure!(
            fill.source_sha256 == source_hash
                && fill.recipe_sha256 == synthesis::recipe_hash(&edits)?,
            "Saved fill source/recipe is stale"
        );
        let source = std::sync::Arc::new(SensorImage::open(&args.image)?);
        ensure!(
            fill.source_color_revision == source.metadata.color_revision,
            "Saved fill color interpretation is stale"
        );
        edits.display.synthesis.truncate(index);
        let mut renderer = Renderer::new(Backend::Auto);
        renderer.set_document(args.image.clone());
        let (w, h) = renderer.dimensions(source.clone(), &edits, None)?;
        let input = renderer.render_region(source, &edits, fill.region, 512, 512)?;
        let mask =
            synthesis::context_mask(&input, fill.region, &fill.dabs, false, h as f32 / w as f32);
        ensure!(
            models::sha256(&recipe_path)? == recipe_hash,
            "Saved recipe changed while rendering"
        );
        (
            input,
            mask,
            Some(
                serde_json::json!({"fill":fill,"sidecar_sha256":recipe_hash,"render_backend":renderer.label(),"canvas_dimensions":[w,h]}),
            ),
        )
    } else {
        let original = image::open(&args.image)?.into_rgb8();
        let mask_image = image::open(args.mask.as_ref().unwrap())?.into_luma8();
        ensure!(
            original.dimensions() == (512, 512) && mask_image.dimensions() == (512, 512),
            "Provide matching 512×512 sRGB input and mask PNGs"
        );
        let mask: Vec<f32> = mask_image.pixels().map(|p| p[0] as f32 / 255.).collect();
        let input = Rendered {
            width: 512,
            height: 512,
            pixels: original
                .pixels()
                .map(|p| {
                    [
                        color::srgb_decode(p[0] as f32 / 255.),
                        color::srgb_decode(p[1] as f32 / 255.),
                        color::srgb_decode(p[2] as f32 / 255.),
                        1.,
                    ]
                })
                .collect(),
        };
        (input, mask, None)
    };
    let mask = if args.target_padding == 0 {
        original_mask.clone()
    } else {
        dilate_mask(&original_mask, args.target_padding)
    };
    ensure!(mask.iter().any(|v| *v > 0.), "The mask is empty");
    let original = encode(&input);
    let inference_mask = if args.mask_padding == 0 {
        mask.clone()
    } else {
        dilate_mask(&mask, args.mask_padding)
    };
    let started = Instant::now();
    let model = Moebius::open(&args.models, args.device)?;
    let load_seconds = started.elapsed().as_secs_f64();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(args.models.join("manifest.json"))?)?;
    let mut record = serde_json::json!({
        "purpose": "native bounded-context image/mask comparison",
        "model": "Moebius scene checkpoint",
        "model_manifest": manifest,
        "input_color": if saved_context.is_some() { "exact saved-context display-linear working RGB, without an input PNG quantization round trip" } else { "sRGB encoded PNG, decoded to display-linear sRGB" },
        "image_sha256": source_hash,
        "mask_sha256": args.mask.as_ref().map(|p| models::sha256(p)).transpose()?,
        "saved_context":saved_context,
        "model_mask_padding_pixels": args.mask_padding,
        "target_mask_padding_pixels":args.target_padding,
        "composition_mask": "explicit target selection, without inference padding",
        "input_size": [512, 512],
        "input_pixels_sha256":format!("{:x}",Sha256::digest(bytemuck::cast_slice(&input.pixels))),
        "settings": settings,
        "dtype": "float32",
        "runtime": "native Rust / LibTorch 2.13",
        "device": format!("{:?}", model.device()),
        "load_seconds": load_seconds,
        "runs": [],
    });
    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir(&args.output)?;
    let reference = args
        .reference_context
        .as_ref()
        .map(|p| -> anyhow::Result<Vec<[f32; 4]>> {
            let decoded = exr::prelude::read_first_rgba_layer_from_file(
                p,
                |size, _| -> anyhow::Result<Vec<[f32; 4]>> {
                    ensure!(
                        (size.width(), size.height()) == (512, 512),
                        "Reference must be a 512×512 float context"
                    );
                    let mut pixels = Vec::new();
                    pixels.try_reserve_exact(512 * 512)?;
                    pixels.resize(512 * 512, [0.; 4]);
                    Ok(pixels)
                },
                |image: &mut anyhow::Result<Vec<[f32; 4]>>,
                 position,
                 (r, g, b, a): (f32, f32, f32, f32)| {
                    if let Ok(image) = image {
                        image[position.y() * 512 + position.x()] = [r, g, b, a];
                    }
                },
            )?;
            let pixels = decoded.layer_data.channel_data.pixels?;
            ensure!(
                pixels.iter().flatten().all(|v| v.is_finite()),
                "Reference must contain finite float pixels"
            );
            Ok(pixels)
        })
        .transpose()?;
    record["reference_context_sha256"] = args
        .reference_context
        .as_ref()
        .map(|p| models::sha256(p))
        .transpose()?
        .into();
    let context_path = args.output.join("input-context.exr");
    rawpuppy::export::write(&context_path, &input, color::OutputSpace::LinearSrgb, false)?;
    original.save(args.output.join("input.png"))?;
    image::GrayImage::from_raw(
        512,
        512,
        original_mask
            .iter()
            .map(|v| (v * 255.).round() as u8)
            .collect(),
    )
    .unwrap()
    .save(args.output.join("original-mask.png"))?;
    image::GrayImage::from_raw(
        512,
        512,
        mask.iter().map(|v| (v * 255.).round() as u8).collect(),
    )
    .unwrap()
    .save(args.output.join("target-mask.png"))?;
    record["input_context_sha256"] = models::sha256(&context_path)?.into();
    record["target_mask_sha256"] = models::sha256(&args.output.join("target-mask.png"))?.into();
    let inference_mask_path = args.output.join("inference-mask.png");
    image::GrayImage::from_raw(
        512,
        512,
        inference_mask
            .iter()
            .map(|v| (v * 255.).round() as u8)
            .collect(),
    )
    .unwrap()
    .save(&inference_mask_path)?;
    record["inference_mask_sha256"] = models::sha256(&inference_mask_path)?.into();
    println!("load_seconds={load_seconds:.3}");
    for iteration in 0..args.iterations {
        let started = Instant::now();
        let mut generated = model.inpaint_512(&input, &inference_mask, &settings)?;
        if record["saved_context"]["fill"]["harmonization"].as_str() == Some("boundary_poisson_v1")
        {
            rawpuppy::synthesis::harmonization::match_background_v1(
                &input,
                &mut generated,
                &inference_mask,
            )?;
        }
        let seconds = started.elapsed().as_secs_f64();
        let context_pixels_sha256 = format!(
            "{:x}",
            Sha256::digest(bytemuck::cast_slice(&generated.pixels))
        );
        let reference_comparison=reference.as_ref().map(|pixels| {
            let mut maximum=0f32;let mut sum=0f64;let mut changed=0;
            for (a,b) in generated.pixels.iter().zip(pixels){changed+=usize::from(a!=b);for c in 0..4 {let error=(a[c]-b[c]).abs();maximum=maximum.max(error);sum+=f64::from(error);}}
            serde_json::json!({"max_absolute_rgba_error":maximum,"mean_absolute_rgba_error":sum/(512.*512.*4.),"changed_pixels":changed})
        });
        let context_output = args.output.join(format!("{iteration}-context.exr"));
        rawpuppy::export::write(
            &context_output,
            &generated,
            color::OutputSpace::LinearSrgb,
            false,
        )?;
        // Retain the actual inference composition separately. It already preserves
        // pixels outside the expanded model mask, and is not the raw VAE output.
        let mut inferred_hash = None;
        if args.mask_padding > 0 {
            let inferred_path = args.output.join(format!("{iteration}-inferred.png"));
            encode(&generated).save(&inferred_path)?;
            inferred_hash = Some(models::sha256(&inferred_path)?);
            for ((pixel, original), selection) in
                generated.pixels.iter_mut().zip(&input.pixels).zip(&mask)
            {
                for c in 0..4 {
                    pixel[c] = original[c] + selection * (pixel[c] - original[c]);
                }
            }
        }
        ensure!(
            generated
                .pixels
                .iter()
                .zip(&input.pixels)
                .zip(&mask)
                .all(|((after, before), selection)| *selection > 0. || after == before),
            "Unselected float context pixels changed"
        );
        let output = encode(&generated);
        let outside_max = output
            .pixels()
            .zip(original.pixels())
            .zip(&mask)
            .filter(|(_, m)| **m == 0.)
            .flat_map(|((after, before), _)| (0..3).map(move |c| after[c].abs_diff(before[c])))
            .max();
        ensure!(outside_max.unwrap_or(0) == 0, "Unmasked pixels changed");
        let path = args.output.join(format!("{iteration}-composed.png"));
        output.save(&path)?;
        let run = serde_json::json!({
            "iteration": iteration,
            "seconds": seconds,
            "output_sha256": models::sha256(&path)?,
            "context_output_sha256": models::sha256(&context_output)?,
            "context_output_pixels_sha256":context_pixels_sha256,
            "outside_composed_float_samples_exact":true,
            "reference_context_comparison":reference_comparison,
            "inferred_output_sha256": inferred_hash,
            "outside_composed_max_absolute_8bit_channel_change": outside_max,
        });
        println!("{run}");
        record["runs"].as_array_mut().unwrap().push(run);
        std::fs::write(
            args.output.join("measurement.json"),
            serde_json::to_string_pretty(&record)? + "\n",
        )?;
    }
    ensure!(
        models::sha256(&args.image)? == source_hash,
        "Input image changed"
    );
    if args.saved_fill.is_some() {
        ensure!(
            models::sha256(&rawpuppy::sidecar::path_for(&args.image))?
                == record["saved_context"]["sidecar_sha256"].as_str().unwrap(),
            "Saved recipe changed during sampling"
        );
    }
    Ok(())
}

#[cfg(feature = "moebius")]
fn encode(rendered: &rawpuppy::pipeline::Rendered) -> image::RgbImage {
    let mut output = image::RgbImage::new(512, 512);
    for (encoded, pixel) in output.pixels_mut().zip(&rendered.pixels) {
        for c in 0..3 {
            encoded[c] =
                (rawpuppy::color::srgb_encode(pixel[c]).clamp(0., 1.) * 255.).round() as u8;
        }
    }
    output
}

/// Square binary dilation matching the FLUX benchmark's PIL MaxFilter for binary masks.
#[cfg(feature = "moebius")]
fn dilate_mask(mask: &[f32], padding: usize) -> Vec<f32> {
    const SIDE: usize = 512;
    let mut horizontal = vec![0.; SIDE * SIDE];
    for y in 0..SIDE {
        for x in 0..SIDE {
            if (x.saturating_sub(padding)..=(x + padding).min(SIDE - 1))
                .any(|i| mask[y * SIDE + i] > 0.)
            {
                horizontal[y * SIDE + x] = 1.;
            }
        }
    }
    let mut expanded = vec![0.; SIDE * SIDE];
    for y in 0..SIDE {
        for x in 0..SIDE {
            if (y.saturating_sub(padding)..=(y + padding).min(SIDE - 1))
                .any(|i| horizontal[i * SIDE + x] > 0.)
            {
                expanded[y * SIDE + x] = 1.;
            }
        }
    }
    expanded
}

#[cfg(not(feature = "moebius"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("Build this example with --features moebius and matching LibTorch 2.13")
}
