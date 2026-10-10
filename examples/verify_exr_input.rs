//! Opt-in EXR input allocation and signed/HDR preservation measurement.
use anyhow::{Result, ensure};
use clap::Parser;
use exr::prelude::*;
use rawpuppy::{input::SensorImage, models};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Instant};

#[derive(Parser)]
struct Args {
    input: PathBuf,
    #[arg(long)]
    generate: bool,
    #[arg(long, default_value_t = 100_003)]
    width: usize,
    #[arg(long, default_value_t = 400)]
    height: usize,
    #[arg(long, required_unless_present = "generate")]
    receipt: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    let value = [1.5f32, -0.125, 0.625];
    if args.generate {
        rawpuppy::input::pixel_count(args.width, args.height, 3)?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args.input)?;
        let mut image = Image::from_channels(
            (args.width, args.height),
            SpecificChannels::rgba(move |_| (value[0], value[1], value[2], 1f32)),
        );
        image.attributes.chromaticities = Some(rawpuppy::export::SRGB_CHROMATICITIES);
        let mut output = std::io::BufWriter::new(file);
        image.write().to_buffered(&mut output)?;
        std::io::Write::flush(&mut output)?;
        println!("Generated {} × {} EXR fixture", args.width, args.height);
        return Ok(());
    }
    let receipt = args.receipt.expect("Clap requires a receipt when decoding");
    ensure!(!receipt.exists(), "Choose a new receipt path");
    let input_hash = models::sha256(&args.input)?;
    let started = Instant::now();
    let image = SensorImage::open(&args.input)?;
    let decode_seconds = started.elapsed().as_secs_f64();
    let mut max_error = 0f32;
    for pixel in image.data.as_chunks::<3>().0 {
        for c in 0..3 {
            ensure!(pixel[c].is_finite(), "Nonfinite EXR input");
            max_error = max_error.max((pixel[c] - value[c]).abs());
        }
    }
    ensure!(
        max_error < 0.000002,
        "Signed/HDR values changed: {max_error}"
    );
    ensure!(models::sha256(&args.input)? == input_hash, "Input changed");
    let record = serde_json::json!({
        "build_profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "dimensions":[image.metadata.width,image.metadata.height],
        "decoded_rgb_bytes":image.data.len()*4,
        "decode_seconds":decode_seconds,
        "max_absolute_error":max_error,
        "data_sha256":format!("{:x}",Sha256::digest(bytemuck::cast_slice(&image.data))),
        "input_sha256":input_hash,
        "input_unchanged":true,
        "color_revision":image.metadata.color_revision,
        "scope":"All decoded components of an owned constant signed/HDR RGBA EXR fixture; process RSS is measured separately, not by this receipt."
    });
    std::fs::write(receipt, serde_json::to_string_pretty(&record)? + "\n")?;
    println!("{record}");
    Ok(())
}
