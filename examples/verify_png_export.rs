//! Opt-in PNG export measurement with bounded row decoding and full pixel verification.
use anyhow::{Result, ensure};
use clap::Parser;
use rawpuppy::{color::OutputSpace, export, input, pipeline::Rendered};
use rayon::prelude::*;
use std::{io::BufReader, path::PathBuf, time::Instant};

#[derive(Parser)]
struct Args {
    output: PathBuf,
    #[arg(long, default_value_t = 100_003)]
    width: u32,
    #[arg(long, default_value_t = 3)]
    height: u32,
}

fn pixel(index: usize) -> [f32; 4] {
    let t = (index % 257) as f32 / 256.;
    [t, 0.25, 1. - t, (index % 3) as f32 / 2.]
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(!args.output.exists(), "Choose a new output file");
    ensure!(
        args.output
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("png")),
        "Choose a PNG output file"
    );
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    let count = input::pixel_count(args.width as usize, args.height as usize, 1)?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(count)?;
    pixels.resize(count, [0.; 4]);
    pixels
        .par_iter_mut()
        .enumerate()
        .for_each(|(i, p)| *p = pixel(i));
    let image = Rendered {
        width: args.width as usize,
        height: args.height as usize,
        pixels,
    };
    let started = Instant::now();
    export::write(&args.output, &image, OutputSpace::LinearSrgb, false)?;
    let export_seconds = started.elapsed().as_secs_f64();
    ensure!(
        image
            .pixels
            .par_iter()
            .enumerate()
            .all(|(i, p)| *p == pixel(i)),
        "Export changed source pixels"
    );
    drop(image);

    let file = std::fs::File::open(&args.output)?;
    let file_bytes = file.metadata()?.len();
    let mut decoder = png::Decoder::new(BufReader::new(file));
    decoder.set_limits(png::Limits { bytes: usize::MAX });
    let mut reader = decoder.read_info()?;
    let info = reader.info();
    ensure!(info.width == args.width && info.height == args.height);
    ensure!(info.bit_depth == png::BitDepth::Sixteen && info.color_type == png::ColorType::Rgba);
    let icc = info
        .icc_profile
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing ICC profile"))?;
    lcms2::Profile::new_icc(icc)?;
    let mut expected_icc = export::profile(OutputSpace::LinearSrgb)?.icc()?;
    ensure!(
        icc.len() == expected_icc.len(),
        "Unexpected ICC profile size"
    );
    expected_icc[24..36].copy_from_slice(&icc[24..36]);
    ensure!(icc.as_ref() == expected_icc, "Incorrect output ICC profile");
    let mut rows = 0;
    let mut max_rgb_error = 0;
    while let Some(row) = reader.next_row()? {
        let data = row.data();
        ensure!(data.len() == args.width as usize * 8);
        for (x, bytes) in data.as_chunks::<8>().0.iter().enumerate() {
            let expected = pixel(rows * args.width as usize + x).map(|v| (v * 65535. + 0.5) as u16);
            for c in 0..4 {
                let actual = u16::from_be_bytes([bytes[c * 2], bytes[c * 2 + 1]]);
                let error = actual.abs_diff(expected[c]);
                if c == 3 {
                    ensure!(error == 0, "Incorrect alpha at ({x}, {rows})");
                } else {
                    max_rgb_error = max_rgb_error.max(error);
                    ensure!(error <= 1, "Incorrect RGB at ({x}, {rows})");
                }
            }
        }
        rows += 1;
    }
    ensure!(rows == args.height as usize);
    reader.finish()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "width": args.width,
            "height": args.height,
            "file_bytes": file_bytes,
            "export_seconds": export_seconds,
            "verified_pixels": count,
            "max_rgb_error_u16": max_rgb_error,
            "alpha_exact": true,
            "icc_matches_linear_srgb": true,
            "source_pixels_unchanged": true,
            "png_finalization_valid": true
        }))?
    );
    Ok(())
}
