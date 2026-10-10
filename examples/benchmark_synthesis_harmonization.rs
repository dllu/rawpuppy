//! Compare unscreened and screened boundary matching on saved model contexts.
use anyhow::{Result, ensure};
use clap::Parser;
use rawpuppy::synthesis::harmonization::harmonize;
use rawpuppy::{color, export, models, pipeline::Rendered};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Parser)]
struct Args {
    input: PathBuf,
    generated: PathBuf,
    mask: PathBuf,
    output: PathBuf,
    /// Penalize changes to generated colors; zero gives a harmonic correction.
    #[arg(long, default_value_t = 0.)]
    screening: f64,
    #[arg(long, default_value_t = 1024)]
    max_iterations: usize,
}

fn load(path: &Path) -> Result<Rendered> {
    if path.extension().is_some_and(|v| v == "exr") {
        let decoded = exr::prelude::read_first_rgba_layer_from_file(
            path,
            |size, _| -> Result<Rendered> {
                let count = rawpuppy::input::pixel_count(size.width(), size.height(), 1)?;
                let mut pixels = Vec::new();
                pixels.try_reserve_exact(count)?;
                pixels.resize(count, [0.; 4]);
                Ok(Rendered {
                    width: size.width(),
                    height: size.height(),
                    pixels,
                })
            },
            |image: &mut Result<Rendered>, p, (r, g, b, a): (f32, f32, f32, f32)| {
                if let Ok(image) = image {
                    image.pixels[p.y() * image.width + p.x()] = [r, g, b, a];
                }
            },
        )?;
        return decoded.layer_data.channel_data.pixels;
    }
    let image = image::ImageReader::open(path)?.decode()?.to_rgba8();
    let pixels = image
        .pixels()
        .map(|p| {
            [
                color::srgb_decode(p[0] as f32 / 255.),
                color::srgb_decode(p[1] as f32 / 255.),
                color::srgb_decode(p[2] as f32 / 255.),
                p[3] as f32 / 255.,
            ]
        })
        .collect();
    Ok(Rendered {
        width: image.width() as usize,
        height: image.height() as usize,
        pixels,
    })
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(!args.output.exists(), "Choose a fresh output directory");
    let paths = [&args.input, &args.generated, &args.mask];
    let before: Vec<_> = paths
        .iter()
        .map(|p| models::sha256(p))
        .collect::<Result<_>>()?;
    let input = load(&args.input)?;
    let generated = load(&args.generated)?;
    let mask = image::ImageReader::open(&args.mask)?.decode()?.to_luma8();
    ensure!(
        (mask.width() as usize, mask.height() as usize) == (input.width, input.height),
        "Mask dimensions differ"
    );
    let started = Instant::now();
    let (output, solve) = harmonize(
        &input,
        &generated,
        mask.as_raw(),
        args.screening,
        args.max_iterations,
    )?;
    let seconds = started.elapsed().as_secs_f64();
    let mut outside = 0;
    let mut boundary_correction = 0f32;
    for (i, ((a, b), g)) in output
        .pixels
        .iter()
        .zip(&input.pixels)
        .zip(&generated.pixels)
        .enumerate()
    {
        if mask.as_raw()[i] == 0 {
            ensure!(a == b, "Unselected sample changed");
            outside += 1;
        }
        for c in 0..3 {
            boundary_correction = boundary_correction.max((a[c] - g[c]).abs());
        }
    }
    std::fs::create_dir(&args.output)?;
    export::write(
        &args.output.join("harmonized.exr"),
        &output,
        color::OutputSpace::LinearSrgb,
        false,
    )?;
    export::write(
        &args.output.join("harmonized.png"),
        &output,
        color::OutputSpace::Srgb,
        false,
    )?;
    let after: Vec<_> = paths
        .iter()
        .map(|p| models::sha256(p))
        .collect::<Result<_>>()?;
    ensure!(before == after, "Input files changed");
    let record = serde_json::json!({"scope":"Bounded saved-output comparison of harmonic and screened background matching; application generation uses unscreened boundary_poisson_v1 for new painted layers. These values are not a perceptual quality score.","input_paths":paths,"input_sha256":before,"input_files_unchanged":true,"dimensions":[input.width,input.height],"solve":solve,"seconds":seconds,"unselected_pixels_exact":outside,"maximum_rgb_correction":boundary_correction,"output_float_sha256":format!("{:x}",Sha256::digest(bytemuck::cast_slice(&output.pixels)))});
    std::fs::write(
        args.output.join("receipt.json"),
        serde_json::to_string_pretty(&record)? + "\n",
    )?;
    println!("{record}");
    Ok(())
}
