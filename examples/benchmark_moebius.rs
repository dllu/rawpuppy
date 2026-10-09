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
    use std::{path::PathBuf, time::Instant};

    #[derive(Parser)]
    struct Args {
        #[arg(long)]
        models: PathBuf,
        #[arg(long)]
        image: PathBuf,
        #[arg(long)]
        mask: PathBuf,
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
        /// Grow the model mask in pixels; commit through the original mask only.
        #[arg(long, default_value_t = 0)]
        mask_padding: usize,
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
    ensure!(!args.output.exists(), "Choose a new output directory");
    let original = image::open(&args.image)?.into_rgb8();
    let mask_image = image::open(&args.mask)?.into_luma8();
    ensure!(
        original.dimensions() == (512, 512) && mask_image.dimensions() == (512, 512),
        "Provide matching 512×512 sRGB input and mask PNGs"
    );
    let mask: Vec<f32> = mask_image.pixels().map(|p| p[0] as f32 / 255.).collect();
    ensure!(mask.iter().any(|v| *v > 0.), "The mask is empty");
    let inference_mask = if args.mask_padding == 0 {
        mask.clone()
    } else {
        dilate_mask(&mask, args.mask_padding)
    };
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
    tch::set_num_threads(8);
    tch::set_num_interop_threads(1);
    let started = Instant::now();
    let model = Moebius::open(&args.models, args.device)?;
    let load_seconds = started.elapsed().as_secs_f64();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(args.models.join("manifest.json"))?)?;
    let mut record = serde_json::json!({
        "purpose": "native bounded-context image/mask comparison",
        "model": "Moebius scene checkpoint",
        "model_manifest": manifest,
        "input_color": "sRGB encoded PNG, decoded to display-linear sRGB",
        "image_sha256": models::sha256(&args.image)?,
        "mask_sha256": models::sha256(&args.mask)?,
        "model_mask_padding_pixels": args.mask_padding,
        "composition_mask": "original mask, without inference padding",
        "input_size": [512, 512],
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
        let seconds = started.elapsed().as_secs_f64();
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
