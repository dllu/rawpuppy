//! Full-size saved synthesis rendering and all-strip color/alpha export verification.
use anyhow::{Result, ensure};
use clap::Parser;
use rawpuppy::{
    color::{self, OutputSpace},
    export,
    input::SensorImage,
    models,
    render::{Backend, Renderer},
    sidecar,
};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc, time::Instant};
use tiff::{decoder::DecodingResult, tags::Tag};

#[derive(Parser)]
struct Args {
    input: PathBuf,
    output: PathBuf,
    #[arg(long, value_enum, default_value = "cuda")]
    backend: Backend,
    #[arg(long, value_enum, default_value = "display-p3")]
    color_space: OutputSpace,
    /// Require the renderer to compose saved layers in its GPU kernel.
    #[arg(long)]
    require_fusion: bool,
    /// Compare every output component with the existing CPU composition of the same GPU base.
    #[arg(long)]
    compare_cpu_composition: bool,
}

fn main() -> Result<()> {
    #[cfg(feature = "cuda")]
    if std::env::var_os("RUST_MIN_STACK").is_none() {
        // SAFETY: startup before threads or runtimes are created.
        unsafe { std::env::set_var("RUST_MIN_STACK", "33554432") };
    }
    let args = Args::parse();
    ensure!(!args.output.exists(), "Choose a fresh output directory");
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    let original_hash = models::sha256(&args.input)?;
    let recipe = sidecar::path_for(&args.input);
    let recipe_hash = models::sha256(&recipe)?;
    let edits = sidecar::load(&recipe)?;
    ensure!(
        !edits.display.synthesis.is_empty(),
        "Provide saved synthesis layers"
    );
    ensure!(
        edits
            .display
            .synthesis
            .iter()
            .all(|fill| !fill.fill_gaps && !fill.dabs.is_empty()),
        "This control verifies painted layers; use the corner probe for geometric fills"
    );
    let source = Arc::new(SensorImage::open(&args.input)?);
    let sensor_hash = Sha256::digest(bytemuck::cast_slice(&source.data));
    let mut renderer = Renderer::new(args.backend);
    renderer.set_document(args.input.clone());
    let (width, height) = renderer.dimensions(source.clone(), &edits, None)?;
    let started = Instant::now();
    let base = renderer.render_before_synthesis(
        source.clone(),
        &edits,
        [0., 0., 1., 1.],
        width,
        height,
    )?;
    let base_seconds = started.elapsed().as_secs_f64();
    let started = Instant::now();
    let rendered = renderer.render(source.clone(), &edits, None)?;
    let saved_layer_render_seconds = started.elapsed().as_secs_f64();
    let layers_on_gpu = renderer.layers_on_gpu();
    ensure!(
        !args.require_fusion || layers_on_gpu,
        "Saved layers fell back to CPU composition"
    );
    let cpu_composition_comparison = if args.compare_cpu_composition {
        let mut expected = rawpuppy::pipeline::Rendered {
            width,
            height,
            pixels: base.pixels.clone(),
        };
        rawpuppy::synthesis::Layers::new(args.input.clone()).apply(
            &edits,
            &mut expected,
            [0., 0., 1., 1.],
        )?;
        let mut max = 0f32;
        let mut sum = 0f64;
        let mut changed = 0usize;
        for (a, b) in rendered.pixels.iter().zip(&expected.pixels) {
            changed += usize::from(a != b);
            for c in 0..4 {
                let error = (a[c] - b[c]).abs();
                max = max.max(error);
                sum += f64::from(error);
            }
        }
        ensure!(max < 0.0003, "Fused/CPU composition differs by {max}");
        Some(
            serde_json::json!({"max_absolute_rgba_error":max,"mean_absolute_rgba_error":sum/(rendered.pixels.len()*4) as f64,"changed_pixels":changed,"cpu_composition_float_sha256":format!("{:x}",Sha256::digest(bytemuck::cast_slice(&expected.pixels)))}),
        )
    } else {
        None
    };
    ensure!(
        (rendered.width, rendered.height) == (width, height),
        "Full dimensions changed"
    );
    let aspect = height as f32 / width as f32;
    let mut changed = 0usize;
    for (i, (a, b)) in rendered.pixels.iter().zip(&base.pixels).enumerate() {
        ensure!(
            a.iter().all(|v| v.is_finite()),
            "Nonfinite full-size output"
        );
        ensure!(a[3] == b[3], "Painted synthesis changed coverage");
        if a != b {
            changed += 1;
            let uv = [
                ((i % width) as f32 + 0.5) / width as f32,
                ((i / width) as f32 + 0.5) / height as f32,
            ];
            ensure!(
                edits
                    .display
                    .synthesis
                    .iter()
                    .any(|fill| fill.dabs.iter().any(|dab| {
                        let dx = uv[0] - dab.center[0];
                        let dy = (uv[1] - dab.center[1]) * aspect;
                        dx * dx + dy * dy <= dab.radius * dab.radius
                    })),
                "Changed full-size sample outside paint at {i}"
            );
        }
    }
    ensure!(changed > 0, "No saved generated pixels were applied");
    drop(base);
    let rendered_hash = Sha256::digest(bytemuck::cast_slice(&rendered.pixels));
    std::fs::create_dir(&args.output)?;
    let output = args.output.join("full.tiff");
    let started = Instant::now();
    export::write(&output, &rendered, args.color_space, false)?;
    let export_seconds = started.elapsed().as_secs_f64();
    let mut decoder = tiff::decoder::Decoder::new(std::fs::File::open(&output)?)?;
    ensure!(decoder.dimensions()? == (u32::try_from(width)?, u32::try_from(height)?));
    ensure!(
        decoder.get_tag_u16_vec(Tag::ExtraSamples)? == [2],
        "Export must tag straight alpha"
    );
    let icc = decoder.get_tag_u8_vec(Tag::IccProfile)?;
    lcms2::Profile::new_icc(&icc)?;
    let mut expected_icc = export::profile(args.color_space)?.icc()?;
    ensure!(icc.len() == expected_icc.len(), "Incorrect ICC size");
    expected_icc[24..36].copy_from_slice(&icc[24..36]);
    ensure!(icc == expected_icc, "Incorrect output color profile");
    let rows = decoder.get_tag_u32(Tag::RowsPerStrip)? as usize;
    let strips = decoder.strip_count()?;
    let matrix = args.color_space.matrix();
    let mut verified = 0usize;
    let started = Instant::now();
    for strip in 0..strips {
        let DecodingResult::U16(samples) = decoder.read_chunk(strip)? else {
            anyhow::bail!("Expected RGBA16 strips")
        };
        let first = strip as usize * rows * width;
        ensure!(
            samples.len() == (rows * width).min(rendered.pixels.len() - first) * 4,
            "Incorrect strip length"
        );
        for (i, actual) in samples.as_chunks::<4>().0.iter().enumerate() {
            let p = rendered.pixels[first + i];
            let rgb = color::apply(matrix, [p[0], p[1], p[2]]);
            let expected = [
                args.color_space.encode(rgb[0]),
                args.color_space.encode(rgb[1]),
                args.color_space.encode(rgb[2]),
                p[3],
            ]
            .map(|v| (v.clamp(0., 1.) * 65535. + 0.5) as u16);
            ensure!(
                *actual == expected,
                "Exported sample differs at {}",
                first + i
            );
            verified += 1;
        }
    }
    let decode_verify_seconds = started.elapsed().as_secs_f64();
    ensure!(
        verified == width * height,
        "Export verification skipped pixels"
    );
    ensure!(
        Sha256::digest(bytemuck::cast_slice(&rendered.pixels)) == rendered_hash,
        "Export changed rendered pixels"
    );
    ensure!(
        Sha256::digest(bytemuck::cast_slice(&source.data)) == sensor_hash,
        "Rendering changed sensor values"
    );
    ensure!(
        models::sha256(&args.input)? == original_hash && models::sha256(&recipe)? == recipe_hash,
        "Original or recipe changed"
    );
    let record = serde_json::json!({"scope":"Full saved painted-layer render and every RGBA16 TIFF sample/profile check on one owned source; numerical export conformance, not physical display colorimetry","layers_on_gpu":layers_on_gpu,"cpu_composition_comparison":cpu_composition_comparison,"dimensions":[width,height],"pixels":verified,"all_rgba16_components_verified":verified*4,"changed_painted_pixels":changed,"unpainted_pixels_exact":true,"alpha_exact":true,"source_and_recipe_unchanged":true,"sensor_and_rendered_hashes_unchanged":true,"input_sha256":original_hash,"recipe_sha256":recipe_hash,"rendered_sha256":format!("{:x}",rendered_hash),"color_space":args.color_space,"icc_matches":true,"straight_alpha":true,"base_render_seconds":base_seconds,"saved_layer_render_seconds":saved_layer_render_seconds,"export_seconds":export_seconds,"decode_verify_seconds":decode_verify_seconds,"backend":renderer.label(),"file_bytes":output.metadata()?.len(),"strips":strips});
    std::fs::write(
        args.output.join("receipt.json"),
        serde_json::to_string_pretty(&record)? + "\n",
    )?;
    println!("{record}");
    Ok(())
}
