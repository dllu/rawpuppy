//! Native, reproducible 512-pixel image/mask comparisons without an editor recipe.
#[cfg(feature = "moebius")]
fn main() -> anyhow::Result<()> {
    use anyhow::ensure;
    use clap::Parser;
    use rawpuppy::{
        color, models,
        moebius::{InferenceDevice, Moebius, Sampling},
        pipeline::Rendered,
    };
    use std::{path::PathBuf, time::Instant};

    #[derive(Parser)]
    struct Args {
        #[arg(long)]
        models: PathBuf,
        #[arg(long)]
        image: PathBuf,
        #[arg(long)]
        mask: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, value_enum, default_value = "cuda")]
        device: InferenceDevice,
        #[arg(long, default_value_t = 20)]
        steps: usize,
        #[arg(long, default_value_t = 42)]
        seed: i64,
        #[arg(long, default_value_t = 1.)]
        strength: f64,
        #[arg(long, default_value_t = 2)]
        iterations: usize,
    }
    let args = Args::parse();
    let settings = Sampling {
        steps: args.steps,
        seed: args.seed,
        strength: args.strength,
        ..Default::default()
    };
    settings.validate()?;
    ensure!(args.iterations > 0, "Iterations must be positive");
    ensure!(!args.output.exists(), "Choose a new output directory");
    let original = image::open(&args.image)?.into_rgb8();
    let mask_image = image::open(&args.mask)?.into_luma8();
    ensure!(
        original.dimensions() == (512, 512) && mask_image.dimensions() == (512, 512),
        "Provide matching 512×512 sRGB input and mask PNGs"
    );
    let mask: Vec<f32> = mask_image.pixels().map(|p| p[0] as f32 / 255.).collect();
    ensure!(mask.iter().any(|v| *v > 0.), "The mask is empty");
    let input = Rendered {
        width: 512,
        height: 512,
        pixels: original
            .pixels()
            .map(|p| {
                [
                    color::srgb_decode(p[0] as f32 / 255.),
                    color::srgb_decode(p[1] as f32 / 255.),
                    color::srgb_decode(p[2] as f32 / 255.),
                    1.,
                ]
            })
            .collect(),
    };
    tch::set_num_threads(8);
    tch::set_num_interop_threads(1);
    let started = Instant::now();
    let model = Moebius::open(&args.models, args.device)?;
    let load_seconds = started.elapsed().as_secs_f64();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(args.models.join("manifest.json"))?)?;
    let mut record = serde_json::json!({
        "purpose": "native bounded-context image/mask comparison",
        "model": "Moebius scene checkpoint",
        "model_manifest": manifest,
        "input_color": "sRGB encoded PNG, decoded to display-linear sRGB",
        "image_sha256": models::sha256(&args.image)?,
        "mask_sha256": models::sha256(&args.mask)?,
        "input_size": [512, 512],
        "settings": settings,
        "dtype": "float32",
        "runtime": "native Rust / LibTorch 2.13",
        "device": format!("{:?}", model.device()),
        "load_seconds": load_seconds,
        "runs": [],
    });
    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir(&args.output)?;
    println!("load_seconds={load_seconds:.3}");
    for iteration in 0..args.iterations {
        let started = Instant::now();
        let generated = model.inpaint_512(&input, &mask, &settings)?;
        let seconds = started.elapsed().as_secs_f64();
        let mut output = image::RgbImage::new(512, 512);
        for (encoded, pixel) in output.pixels_mut().zip(&generated.pixels) {
            for c in 0..3 {
                encoded[c] = (color::srgb_encode(pixel[c]).clamp(0., 1.) * 255.).round() as u8;
            }
        }
        let outside_max = output
            .pixels()
            .zip(original.pixels())
            .zip(&mask)
            .filter(|(_, m)| **m == 0.)
            .flat_map(|((after, before), _)| (0..3).map(move |c| after[c].abs_diff(before[c])))
            .max();
        ensure!(outside_max.unwrap_or(0) == 0, "Unmasked pixels changed");
        let path = args.output.join(format!("{iteration}-composed.png"));
        output.save(&path)?;
        let run = serde_json::json!({
            "iteration": iteration,
            "seconds": seconds,
            "output_sha256": models::sha256(&path)?,
            "outside_composed_max_absolute_8bit_channel_change": outside_max,
        });
        println!("{run}");
        record["runs"].as_array_mut().unwrap().push(run);
        std::fs::write(
            args.output.join("measurement.json"),
            serde_json::to_string_pretty(&record)? + "\n",
        )?;
    }
    Ok(())
}

#[cfg(not(feature = "moebius"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("Build this example with --features moebius and matching LibTorch 2.13")
}
