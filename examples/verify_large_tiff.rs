//! Opt-in large TIFF export check: decode selected strips without a full-file allocation.
use anyhow::{Result, ensure};
use clap::Parser;
use rawpuppy::{color::OutputSpace, export, input, pipeline::Rendered};
use rayon::prelude::*;
use std::{io::Read, path::PathBuf, time::Instant};
use tiff::{decoder::DecodingResult, tags::Tag};

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
            .is_some_and(|e| { e.eq_ignore_ascii_case("tif") || e.eq_ignore_ascii_case("tiff") }),
        "Choose a TIFF output file"
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

    let mut file = std::fs::File::open(&args.output)?;
    let file_bytes = file.metadata()?.len();
    let mut header = [0; 8];
    file.read_exact(&mut header)?;
    let version = if &header[..2] == b"II" {
        u16::from_le_bytes([header[2], header[3]])
    } else {
        ensure!(&header[..2] == b"MM", "Invalid TIFF byte order");
        u16::from_be_bytes([header[2], header[3]])
    };
    let mut decoder = tiff::decoder::Decoder::new(std::fs::File::open(&args.output)?)?;
    ensure!(decoder.dimensions()? == (args.width, args.height));
    ensure!(decoder.get_tag_u16_vec(Tag::ExtraSamples)? == [2]);
    let icc = decoder.get_tag_u8_vec(Tag::IccProfile)?;
    lcms2::Profile::new_icc(&icc)?;
    let mut expected_icc = export::profile(OutputSpace::LinearSrgb)?.icc()?;
    ensure!(
        icc.len() == expected_icc.len(),
        "Unexpected ICC profile size"
    );
    // Profile creation timestamps may differ after a long export.
    expected_icc[24..36].copy_from_slice(&icc[24..36]);
    ensure!(icc == expected_icc, "Incorrect output ICC profile");
    let offsets = decoder.get_tag_u64_vec(Tag::StripOffsets)?;
    let rows = decoder.get_tag_u32(Tag::RowsPerStrip)?;
    let strips = decoder.strip_count()?;
    let mut selected = vec![0, strips / 2, strips - 1];
    selected.sort_unstable();
    selected.dedup();
    for &strip in &selected {
        let DecodingResult::U16(values) = decoder.read_chunk(strip)? else {
            anyhow::bail!("Expected RGBA16 strip");
        };
        let first = strip as usize * rows as usize * args.width as usize;
        let expected_pixels = (rows as usize * args.width as usize).min(count - first);
        ensure!(values.len() == expected_pixels * 4);
        for (i, actual) in values.as_chunks::<4>().0.iter().enumerate() {
            let expected = pixel(first + i).map(|v| (v * 65535. + 0.5) as u16);
            ensure!(
                actual[..3]
                    .iter()
                    .zip(&expected[..3])
                    .all(|(a, b)| a.abs_diff(*b) <= 1)
                    && actual[3] == expected[3],
                "Incorrect pixel in strip {strip}"
            );
        }
    }
    if file_bytes > u64::from(u32::MAX) {
        ensure!(version == 43, "Large export must use BigTIFF");
    } else {
        ensure!(version == 42 || version == 43, "Invalid TIFF version");
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "width": args.width,
            "height": args.height,
            "file_bytes": file_bytes,
            "tiff_version": version,
            "export_seconds": export_seconds,
            "strips": strips,
            "rows_per_strip": rows,
            "last_strip_offset": offsets.last(),
            "verified_strips": selected,
            "icc_matches_linear_srgb": true,
            "straight_alpha": true,
            "source_pixels_unchanged": true
        }))?
    );
    Ok(())
}
