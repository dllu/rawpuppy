//! Opt-in full-size JPEG memory/color regression probe with immutable float source.
use anyhow::{Result, ensure};
use clap::Parser;
use rawpuppy::{color::OutputSpace, export, input, models, pipeline::Rendered};
use rayon::prelude::*;
use std::{path::PathBuf, time::Instant};

#[derive(Parser)]
struct Args {
    output: PathBuf,
    #[arg(long, default_value_t = 8736)]
    width: usize,
    #[arg(long, default_value_t = 11648)]
    height: usize,
    #[arg(long, value_enum, default_value = "srgb")]
    color_space: OutputSpace,
}
fn pixel(i: usize) -> [f32; 4] {
    let t = (i % 257) as f32 / 256.;
    [t * 1.5 - 0.125, 0.25, 1. - t, (i % 3) as f32 / 2.]
}
fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(!args.output.exists(), "Choose a fresh output file");
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    let count = input::pixel_count(args.width, args.height, 1)?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(count)?;
    pixels.resize(count, [0.; 4]);
    pixels
        .par_iter_mut()
        .enumerate()
        .for_each(|(i, p)| *p = pixel(i));
    let image = Rendered {
        width: args.width,
        height: args.height,
        pixels,
    };
    let started = Instant::now();
    export::write(&args.output, &image, args.color_space, false)?;
    let export_seconds = started.elapsed().as_secs_f64();
    ensure!(
        image
            .pixels
            .par_iter()
            .enumerate()
            .all(|(i, p)| *p == pixel(i)),
        "JPEG export changed source floats"
    );
    println!(
        "{}",
        serde_json::json!({"scope":"generated signed/HDR/alpha float source; JPEG output byte identity compared separately, not perceptual quality","dimensions":[args.width,args.height],"pixels":count,"file_bytes":args.output.metadata()?.len(),"output_sha256":models::sha256(&args.output)?,"export_seconds":export_seconds,"color_space":args.color_space,"source_pixels_unchanged":true})
    );
    Ok(())
}
