//! Opt-in generated-layer persistence check across decoder allocation limits.
use anyhow::{Result, ensure};
use clap::Parser;
use rawpuppy::{
    edits::Edits,
    input, models,
    pipeline::Rendered,
    sidecar,
    synthesis::{self, GeneratedFill, Layers, MaskDab},
};
use std::{path::PathBuf, time::Instant};

#[derive(Parser)]
struct Args {
    output: PathBuf,
    #[arg(long, default_value_t = 100_003)]
    width: usize,
    #[arg(long, default_value_t = 336)]
    height: usize,
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(!args.output.exists(), "Choose a new output directory");
    let count = input::pixel_count(args.width, args.height, 1)?;
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    std::fs::create_dir(&args.output)?;
    let original = args.output.join("photo.raw");
    std::fs::write(&original, b"owned synthetic original")?;
    let original_hash = models::sha256(&original)?;
    let value = [1.5, -0.125, 0.625, 1.];
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(count)?;
    pixels.resize(count, value);
    let generated = Rendered {
        width: args.width,
        height: args.height,
        pixels,
    };
    let mut layers = Layers::new(original.clone());
    let started = Instant::now();
    let (asset, hash) = layers.store(&generated)?;
    let store_seconds = started.elapsed().as_secs_f64();
    ensure!(
        generated.pixels.iter().all(|p| *p == value),
        "Store changed source pixels"
    );
    let mut edits = Edits::default();
    let fill = GeneratedFill {
        region: [0., 0., 1., 1.],
        dabs: vec![MaskDab {
            center: [0.5; 2],
            radius: 1.,
        }],
        fill_gaps: false,
        feather: 0.,
        steps: 2,
        seed: 0,
        asset,
        sha256: hash,
        source_sha256: original_hash.clone(),
        source_color_revision: 0,
        recipe_sha256: synthesis::recipe_hash(&edits)?,
        sampling: Default::default(),
        model: "moebius-scene-2026-v1".into(),
    };
    edits.display.synthesis.push(fill);
    let recipe = sidecar::path_for(&original);
    sidecar::save(&recipe, &edits)?;
    let recipe_bytes = std::fs::read(&recipe)?;
    drop(generated);
    drop(layers);
    let mut output_pixels = Vec::new();
    output_pixels.try_reserve_exact(count)?;
    output_pixels.resize(count, [0.18, 0.18, 0.18, 1.]);
    let mut output = Rendered {
        width: args.width,
        height: args.height,
        pixels: output_pixels,
    };
    let loaded = sidecar::load(&recipe)?;
    let mut fresh = Layers::new(original.clone());
    let started = Instant::now();
    fresh.apply(&loaded, &mut output, [0., 0., 1., 1.])?;
    let reload_and_apply_seconds = started.elapsed().as_secs_f64();
    let mut max_error = 0f32;
    for pixel in &output.pixels {
        for c in 0..4 {
            max_error = max_error.max((pixel[c] - value[c]).abs());
        }
    }
    ensure!(
        max_error < 0.000001,
        "Layer composition changed constant signed/HDR values: {max_error}"
    );
    ensure!(
        models::sha256(&original)? == original_hash,
        "Original bytes changed"
    );
    ensure!(
        std::fs::read(&recipe)? == recipe_bytes,
        "Saved recipe changed"
    );
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"width":args.width,"height":args.height,"pixels":count,"decoded_rgba_bytes":count*16,"store_seconds":store_seconds,"reload_and_apply_seconds":reload_and_apply_seconds,"max_absolute_error":max_error,"source_pixels_unchanged":true,"original_unchanged":true,"recipe_unchanged":true})
        )?
    );
    Ok(())
}
