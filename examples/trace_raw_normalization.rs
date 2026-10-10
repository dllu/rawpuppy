//! Read-only RAW normalization diagnostics and an explicitly counterfactual clipping export.
use anyhow::{Result, ensure};
use clap::Parser;
use rawpuppy::{
    color::OutputSpace,
    edits::{Edits, ToneMapper},
    export,
    input::SensorImage,
    models,
    pipeline::{Pipeline, Rendered},
};
use rayon::prelude::*;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    input: PathBuf,
    output: PathBuf,
    #[arg(long, default_value_t = 1800)]
    max_edge: usize,
    /// Independently developed RGB files whose signed-value statistics should be recorded.
    #[arg(long)]
    reference: Vec<PathBuf>,
}

#[derive(Serialize)]
struct Channel {
    samples: usize,
    negative_samples: usize,
    minimum: f32,
    maximum: f32,
    mean: f64,
    mean_after_zero_clipping: f64,
}

fn rgb_statistics(
    width: usize,
    height: usize,
    pixels: impl Iterator<Item = [f32; 3]>,
) -> serde_json::Value {
    let count = (width * height) as f64;
    let mut minimum = [f32::INFINITY; 3];
    let mut negative = [0usize; 3];
    let mut sum = [0f64; 3];
    for pixel in pixels {
        for c in 0..3 {
            minimum[c] = minimum[c].min(pixel[c]);
            negative[c] += usize::from(pixel[c] < 0.);
            sum[c] += f64::from(pixel[c]);
        }
    }
    serde_json::json!({
        "dimensions": [width, height],
        "minimum_rgb": minimum,
        "mean_rgb": sum.map(|v| v / count),
        "negative_fraction_rgb": negative.map(|v| v as f64 / count),
    })
}

fn rendered_statistics(image: &Rendered) -> serde_json::Value {
    rgb_statistics(
        image.width,
        image.height,
        image.pixels.iter().map(|p| [p[0], p[1], p[2]]),
    )
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(!args.output.exists(), "Choose a new output directory");
    ensure!(args.max_edge > 0, "Maximum edge must be positive");
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    let input_hash = models::sha256(&args.input)?;
    let mut source = SensorImage::open(&args.input)?;
    ensure!(
        source.cpp == 1 && source.cfa.is_some(),
        "Expected an RGB mosaic RAW"
    );
    let cfa = source.cfa.as_ref().unwrap();
    let mut channels: [Channel; 3] = std::array::from_fn(|_| Channel {
        samples: 0,
        negative_samples: 0,
        minimum: f32::INFINITY,
        maximum: f32::NEG_INFINITY,
        mean: 0.,
        mean_after_zero_clipping: 0.,
    });
    for y in source.origin[1]..source.origin[1] + source.active[1] {
        for x in source.origin[0]..source.origin[0] + source.active[0] {
            let value = source.data[y * source.metadata.sensor_width + x];
            let channel = &mut channels[cfa.color_at(y, x)];
            channel.samples += 1;
            channel.negative_samples += usize::from(value < 0.);
            channel.minimum = channel.minimum.min(value);
            channel.maximum = channel.maximum.max(value);
            channel.mean += f64::from(value);
            channel.mean_after_zero_clipping += f64::from(value.max(0.));
        }
    }
    for channel in &mut channels {
        ensure!(channel.samples > 0, "Missing mosaic color channel");
        channel.mean /= channel.samples as f64;
        channel.mean_after_zero_clipping /= channel.samples as f64;
    }
    let mut edits = Edits::default();
    edits.tone.mapper = ToneMapper::Linear;
    let signed = Pipeline::compile(&source, &edits)?.render(Some(args.max_edge))?;
    let signed_statistics = rendered_statistics(&signed);
    std::fs::create_dir(&args.output)?;
    export::write(
        &args.output.join("signed.exr"),
        &signed,
        OutputSpace::LinearSrgb,
        false,
    )?;
    drop(signed);
    // Counterfactual only: mutate this diagnostic's owned sensor allocation.
    // Production reconstruction continues to preserve signed sensor noise.
    source.data.par_iter_mut().for_each(|v| *v = v.max(0.));
    let clipped = Pipeline::compile(&source, &edits)?.render(Some(args.max_edge))?;
    let clipped_statistics = rendered_statistics(&clipped);
    export::write(
        &args.output.join("zero-clipped.exr"),
        &clipped,
        OutputSpace::LinearSrgb,
        false,
    )?;
    ensure!(
        models::sha256(&args.input)? == input_hash,
        "Input bytes changed"
    );
    let mut references = Vec::new();
    for path in &args.reference {
        let image = SensorImage::open(path)?;
        ensure!(
            image.cpp == 3 && image.cfa.is_none(),
            "Reference must be developed RGB"
        );
        references.push(serde_json::json!({
            "file": path.file_name().map(|name| name.to_string_lossy()),
            "sha256": models::sha256(path)?,
            "statistics": rgb_statistics(image.metadata.width, image.metadata.height, image.data.as_chunks::<3>().0.iter().copied()),
        }));
    }
    let receipt = serde_json::json!({
        "scope": "signed sensor normalization and diagnostic-only zero clipping; not a production clipping policy",
        "input_sha256": input_hash,
        "input_unchanged": true,
        "metadata": source.metadata,
        "sensor_origin": source.origin,
        "active_size": source.active,
        "normalized_channels": channels,
        "signed": signed_statistics,
        "zero_clipped_sensor": clipped_statistics,
        "references": references,
        "edits": edits,
    });
    std::fs::write(
        args.output.join("receipt.json"),
        serde_json::to_string_pretty(&receipt)? + "\n",
    )?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}
