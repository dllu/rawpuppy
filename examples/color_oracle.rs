//! Reproducible linear-sRGB fixtures and output for independent color-transform comparisons.
use anyhow::Result;
use clap::Parser;
use rawpuppy::{
    color::{self, OutputSpace},
    export,
    pipeline::Rendered,
};
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    directory: PathBuf,
    #[arg(long)]
    reference: Vec<PathBuf>,
    /// Compare references with the unchanged input instead of the tone-mapped output.
    #[arg(long)]
    identity: bool,
    #[arg(long)]
    legacy: bool,
    /// Save independent, in-Rec.2020 oracle samples for regression tests.
    #[arg(long)]
    golden: Option<PathBuf>,
    /// Additional rows of deterministic, independent Rec.2020 probes.
    #[arg(long, default_value_t = 0)]
    random_rows: usize,
    #[arg(long)]
    lattice: Option<PathBuf>,
    #[arg(long, default_value_t = 97)]
    grid: usize,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let directory = args.directory;
    let lattice = args
        .lattice
        .as_ref()
        .map(|p| {
            std::fs::read(p).map(|b| {
                b.as_chunks::<4>()
                    .0
                    .iter()
                    .map(|v| f32::from_le_bytes(*v))
                    .collect::<Vec<_>>()
            })
        })
        .transpose()?;
    if let Some(data) = &lattice {
        anyhow::ensure!(
            args.grid >= 2 && data.len() == args.grid * args.grid * args.grid * 3,
            "Alternate lattice dimensions differ"
        );
    }
    std::fs::create_dir_all(&directory)?;
    let colors = [
        [1., 1., 1.],
        [1., 0., 0.],
        [0., 1., 0.],
        [0., 0., 1.],
        [1., 1., 0.],
        [0., 1., 1.],
        [1., 0., 1.],
        [1., 0.1, 0.01],
        [1., 0.38, 0.26],
        [0.12, 0.42, 1.],
        [-0.02, 0.25, 0.1],
        [-1., 0.2, 2.],
    ];
    let width = 257;
    let mut pixels: Vec<_> = colors
        .iter()
        .flat_map(|rgb| {
            (0..width).map(move |x| {
                let ev = -14. + 28. * x as f32 / (width - 1) as f32;
                let gain = 0.18 * 2f32.powf(ev);
                [rgb[0] * gain, rgb[1] * gain, rgb[2] * gain, 1.]
            })
        })
        .collect();
    let mut state = 0x9e3779b97f4a7c15u64;
    for _ in 0..args.random_rows * width {
        let rgb = std::array::from_fn(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let u = 0.2 + 0.65 * ((state >> 40) as f64 / ((1u64 << 24) - 1) as f64);
            if u <= 0.5 {
                (0.18 / 1024. * ((u * 2. * 1025f64.log2()).exp2() - 1.)) as f32
            } else {
                (0.18 * ((u - 0.5) * 32.).exp2()) as f32
            }
        });
        let rgb = color::apply(rawpuppy::agx::TO_SRGB, rgb);
        pixels.push([rgb[0], rgb[1], rgb[2], 1.]);
    }
    let input = Rendered {
        width,
        height: colors.len() + args.random_rows,
        pixels,
    };
    export::write(
        &directory.join("input.exr"),
        &input,
        OutputSpace::LinearSrgb,
        true,
    )?;
    let output = Rendered {
        width,
        height: input.height,
        pixels: input
            .pixels
            .iter()
            .map(|p| {
                let rgb = if args.legacy {
                    color::agx_legacy([p[0], p[1], p[2]])
                } else if let Some(data) = &lattice {
                    rawpuppy::agx::map_lattice([p[0], p[1], p[2]], data, args.grid)
                } else {
                    color::agx([p[0], p[1], p[2]])
                };
                [rgb[0], rgb[1], rgb[2], p[3]]
            })
            .collect(),
    };
    export::write(
        &directory.join("rawpuppy.exr"),
        &output,
        OutputSpace::LinearSrgb,
        true,
    )?;
    let samples: Vec<_> = input
        .pixels
        .iter()
        .zip(&output.pixels)
        .map(|(a, b)| (*a, *b))
        .collect();
    std::fs::write(
        directory.join("samples.json"),
        serde_json::to_vec_pretty(&samples)?,
    )?;
    println!("{} float samples → {}", samples.len(), directory.display());
    for reference in args.reference {
        let decoded = image::ImageReader::open(&reference)?.decode()?.to_rgb32f();
        anyhow::ensure!(
            decoded.width() as usize == width && decoded.height() as usize == input.height,
            "Oracle dimensions differ"
        );
        let baseline = if args.identity { &input } else { &output };
        let mut max = 0f32;
        let mut sum = 0f64;
        let mut row_max = vec![0f32; input.height];
        for (i, (p, actual)) in baseline.pixels.iter().zip(decoded.pixels()).enumerate() {
            for (expected, actual) in p[..3].iter().zip(actual.0) {
                let error = (*expected - actual).abs();
                max = max.max(error);
                sum += error as f64;
                row_max[i / width] = row_max[i / width].max(error);
            }
        }
        println!(
            "{}: max_abs={max:.8}, mean_abs={:.8}, middle_gray={:?}",
            reference.display(),
            sum / (baseline.pixels.len() * 3) as f64,
            decoded.get_pixel(128, 0).0
        );
        println!("row_max={row_max:?}");
        if let Some(path) = &args.golden {
            let cases: Vec<_> = input
                .pixels
                .iter()
                .zip(decoded.pixels())
                .enumerate()
                .filter(|(i, _)| {
                    (i / width < 11 || i / width >= 12)
                        && (i % width % 7 == 0 || i % width == 128 || i % width == 256)
                })
                .map(|(_, (p, reference))| ([p[0], p[1], p[2]], reference.0))
                .collect();
            std::fs::write(path, serde_json::to_vec_pretty(&cases)?)?;
        }
    }
    Ok(())
}
