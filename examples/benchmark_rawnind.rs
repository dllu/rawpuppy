//! Native joint reconstruction measurements; requires prepared CC BY RawNIND weights.
#[cfg(feature = "raw-ml")]
fn main() -> anyhow::Result<()> {
    use anyhow::ensure;
    use clap::Parser;
    use rawpuppy::{
        color, input::SensorImage, ml_runtime::InferenceDevice, models, raw_ml::BayerModel,
    };
    use std::{io::Write, path::PathBuf, time::Instant};

    #[derive(Parser)]
    struct Args {
        input: PathBuf,
        output: PathBuf,
        #[arg(long)]
        graph: PathBuf,
        #[arg(long, value_enum, default_value = "auto")]
        device: InferenceDevice,
        #[arg(long, num_args = 4, default_values_t = [0usize, 0, 512, 512])]
        crop: Vec<usize>,
        #[arg(long, default_value_t = 2)]
        iterations: usize,
    }
    let args = Args::parse();
    ensure!(!args.output.exists(), "Choose a new output directory");
    ensure!(args.iterations > 0, "Iterations must be positive");
    let [x, y, width, height] = args.crop.as_slice() else {
        anyhow::bail!("Provide x, y, width, height");
    };
    tch::set_num_threads(8);
    tch::set_num_interop_threads(1);
    let source = SensorImage::open(&args.input)?;
    let started = Instant::now();
    let model = BayerModel::open(&args.graph, args.device)?;
    let load_seconds = started.elapsed().as_secs_f64();
    let graph_manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(args.graph.join("manifest.json"))?)?;
    let gain = std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            if i == j {
                source.metadata.as_shot[i]
            } else {
                0.
            }
        })
    });
    let to_working = color::multiply(source.metadata.camera_to_working, gain);
    let mut record = serde_json::json!({
        "source_sha256": models::sha256(&args.input)?,
        "metadata": source.metadata,
        "crop_xywh": args.crop,
        "model_manifest": graph_manifest,
        "device": format!("{:?}", model.device()),
        "runtime": "native Rust / LibTorch 2.13",
        "load_seconds": load_seconds,
        "camera_to_linear_srgb_with_as_shot": to_working,
        "runs": [],
    });
    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir(&args.output)?;
    println!("load_seconds={load_seconds:.3}");
    for iteration in 0..args.iterations {
        let started = Instant::now();
        let patch = model.reconstruct_patch(&source, [*x, *y], [*width, *height])?;
        let seconds = started.elapsed().as_secs_f64();
        let camera_path = args.output.join(format!("{iteration}-camera.f32"));
        let mut file = std::io::BufWriter::new(std::fs::File::create_new(&camera_path)?);
        let mut preview = image::RgbImage::new((*width).try_into()?, (*height).try_into()?);
        for (camera, encoded) in patch.pixels.iter().zip(preview.pixels_mut()) {
            for component in camera {
                file.write_all(&component.to_le_bytes())?;
            }
            let formed = color::agx(color::apply(to_working, *camera));
            for c in 0..3 {
                encoded[c] = (color::srgb_encode(formed[c]).clamp(0., 1.) * 255.).round() as u8;
            }
        }
        file.flush()?;
        let preview_path = args.output.join(format!("{iteration}-preview.png"));
        preview.save(&preview_path)?;
        let run = serde_json::json!({
            "iteration": iteration,
            "seconds": seconds,
            "gain_matched_to_observed_cfa_samples": patch.gain,
            "context_origin": patch.context_origin,
            "context_size": patch.context_size,
            "camera_sha256": models::sha256(&camera_path)?,
            "preview_sha256": models::sha256(&preview_path)?,
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

#[cfg(not(feature = "raw-ml"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("Build this example with --features raw-ml and matching LibTorch 2.13")
}
