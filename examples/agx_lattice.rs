//! Prepare independent RGB probes and pack observed oracle output; no oracle source is copied.
use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use rawpuppy::{
    color::{self, OutputSpace},
    export,
    pipeline::Rendered,
};
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Prepare {
        output: PathBuf,
        #[arg(long, default_value_t = 97)]
        grid: usize,
        /// Write true Rec.2020 samples with EXR chromaticity metadata.
        #[arg(long)]
        tagged: bool,
    },
    Pack {
        input: PathBuf,
        output: PathBuf,
        #[arg(long, default_value_t = 97)]
        grid: usize,
    },
}
fn value(u: f64) -> f32 {
    if u <= 0.5 {
        (0.18 / 1024. * ((u * 2. * 1025f64.log2()).exp2() - 1.)) as f32
    } else {
        (0.18 * ((u - 0.5) * 32.).exp2()) as f32
    }
}
fn main() -> Result<()> {
    match Args::parse().command {
        Command::Prepare {
            output,
            grid,
            tagged,
        } => {
            ensure!(
                grid >= 3 && grid % 2 == 1,
                "Use an odd grid size of at least three"
            );
            let m = color::multiply(color::inverse(color::SRGB_TO_XYZ)?, color::REC2020_TO_XYZ);
            let mut pixels = Vec::with_capacity(grid * grid * grid);
            for b in 0..grid {
                for g in 0..grid {
                    for r in 0..grid {
                        let rgb = [r, g, b].map(|v| value(v as f64 / (grid - 1) as f64));
                        let rgb = if tagged { rgb } else { color::apply(m, rgb) };
                        pixels.push([rgb[0], rgb[1], rgb[2], 1.]);
                    }
                }
            }
            let image = Rendered {
                width: grid * grid,
                height: grid,
                pixels,
            };
            if tagged {
                let chroma = exr::meta::attribute::Chromaticities {
                    red: exr::math::Vec2(0.708, 0.292),
                    green: exr::math::Vec2(0.170, 0.797),
                    blue: exr::math::Vec2(0.131, 0.046),
                    white: exr::math::Vec2(0.3127, 0.329),
                };
                export::write_linear_exr(
                    &image,
                    std::io::BufWriter::new(std::fs::File::create(output)?),
                    chroma,
                )?;
            } else {
                export::write(&output, &image, OutputSpace::LinearSrgb, false)?;
            }
        }
        Command::Pack {
            input,
            output,
            grid,
        } => {
            let image = image::ImageReader::open(input)?.decode()?.to_rgb32f();
            ensure!(
                image.width() as usize == grid * grid && image.height() as usize == grid,
                "Lattice dimensions differ"
            );
            let data = image.into_raw();
            ensure!(
                data.iter().all(|v| v.is_finite()),
                "Nonfinite oracle output"
            );
            println!(
                "Observed linear Rec.2020 range: {} .. {}",
                data.iter().copied().fold(f32::INFINITY, f32::min),
                data.iter().copied().fold(f32::NEG_INFINITY, f32::max)
            );
            let bytes: Vec<_> = data.into_iter().flat_map(|v| v.to_le_bytes()).collect();
            std::fs::write(output, bytes)?;
        }
    }
    Ok(())
}
