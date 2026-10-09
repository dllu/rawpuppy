//! Real-model corner conformance on an owned copy, retaining a reproducible receipt.
#[cfg(feature = "moebius")]
fn main() -> anyhow::Result<()> {
    use anyhow::ensure;
    use clap::Parser;
    use rawpuppy::{
        edits::Edits,
        input::SensorImage,
        models,
        moebius::Sampling,
        render::{Backend, Renderer},
        sidecar,
    };
    use std::{path::PathBuf, sync::Arc, time::Instant};
    #[derive(Parser)]
    struct Args {
        input: PathBuf,
        output: PathBuf,
        #[arg(long, default_value_t = 0.01)]
        rotation: f32,
        #[arg(long, default_value_t = 20)]
        steps: usize,
        #[arg(long, default_value_t = 1.)]
        scale: f32,
    }
    let args = Args::parse();
    ensure!(!args.output.exists(), "Choose a new output directory");
    let settings = Sampling {
        steps: args.steps,
        ..Default::default()
    };
    settings.validate()?;
    let source_hash = models::sha256(&args.input)?;
    std::fs::create_dir_all(&args.output)?;
    let original = args
        .output
        .join("photo")
        .with_extension(args.input.extension().unwrap_or_default());
    std::fs::copy(&args.input, &original)?;
    ensure!(
        models::sha256(&original)? == source_hash,
        "Input copy differs"
    );
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    tch::set_num_threads(8);
    tch::set_num_interop_threads(1);
    let source = Arc::new(SensorImage::open(&original)?);
    let mut edits = Edits::for_image(&source);
    edits.lens = Default::default();
    edits.geometry.rotation = args.rotation;
    edits.geometry.scale = args.scale;
    let mut renderer = Renderer::new(Backend::Cpu);
    renderer.set_document(original.clone());
    let (w, h) = renderer.dimensions(source.clone(), &edits, None)?;
    let edges = [
        ([0., 0., 1., 1. / h as f32], w, 1),
        ([0., 1. - 1. / h as f32, 1., 1. / h as f32], w, 1),
        ([0., 0., 1. / w as f32, 1.], 1, h),
        ([1. - 1. / w as f32, 0., 1. / w as f32, 1.], 1, h),
    ];
    let mut before = Vec::new();
    for (region, ew, eh) in edges {
        before.push(renderer.render_region(source.clone(), &edits, region, ew, eh)?);
    }
    let missing: usize = before
        .iter()
        .map(|edge| edge.pixels.iter().filter(|p| p[3] < 1.).count())
        .sum();
    ensure!(
        missing > 0,
        "This geometry has no missing perimeter samples"
    );
    let regions = renderer.gap_contexts(source.clone(), &edits)?;
    ensure!(
        !regions.is_empty(),
        "Corner planning missed the perimeter gaps"
    );
    let start = Instant::now();
    let mut times = Vec::new();
    let mut skipped = 0;
    for region in regions {
        let started = Instant::now();
        let fill =
            renderer.generate_fill(source.clone(), &edits, region, vec![], true, &settings)?;
        times.push(started.elapsed().as_secs_f64());
        if let Some(fill) = fill {
            edits.display.synthesis.push(fill);
        } else {
            skipped += 1;
        }
    }
    sidecar::save(&sidecar::path_for(&original), &edits)?;
    let loaded = sidecar::load(&sidecar::path_for(&original))?;
    ensure!(loaded == edits, "Generated recipe did not roundtrip");
    let mut reload = Renderer::new(Backend::Cpu);
    reload.set_document(original.clone());
    let mut filled = 0;
    let mut preserved = 0;
    let mut min_filled_rgb = f32::INFINITY;
    let mut max_filled_rgb = f32::NEG_INFINITY;
    for ((region, ew, eh), baseline) in edges.into_iter().zip(before) {
        let after = reload.render_region(source.clone(), &loaded, region, ew, eh)?;
        for (a, b) in after.pixels.iter().zip(baseline.pixels) {
            ensure!(a.iter().all(|v| v.is_finite()), "Nonfinite generated edge");
            if b[3] == 1. {
                ensure!(*a == b, "Unselected original perimeter sample changed");
                preserved += 1;
            } else {
                ensure!(
                    a[3] == 1.,
                    "Generated perimeter sample is still transparent"
                );
                filled += 1;
                for v in &a[..3] {
                    min_filled_rgb = min_filled_rgb.min(*v);
                    max_filled_rgb = max_filled_rgb.max(*v);
                }
            }
        }
    }
    ensure!(filled == missing, "A missing perimeter sample was skipped");
    ensure!(
        models::sha256(&args.input)? == source_hash && models::sha256(&original)? == source_hash,
        "Original bytes changed"
    );
    let graph = models::cache_dir()?.join("models/moebius/torchscript/manifest.json");
    let receipt = serde_json::json!({
        "purpose": "real-model output-perimeter conformance; not a perceptual quality benchmark",
        "build_profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "dimensions": [w, h], "rotation": args.rotation, "scale": args.scale, "settings": settings,
        "skipped_completed_contexts": skipped,
        "input_sha256": source_hash,
        "cuda_available": tch::Cuda::is_available(),
        "graph_manifest": serde_json::from_slice::<serde_json::Value>(&std::fs::read(graph)?)?,
        "generated_layers": loaded.display.synthesis,
        "generation_seconds_per_context": times, "total_seconds_after_planning": start.elapsed().as_secs_f64(),
        "filled_perimeter_samples": filled, "exactly_preserved_opaque_perimeter_samples": preserved,
        "filled_rgb_range": [min_filled_rgb, max_filled_rgb], "save_reload_verified": true,
    });
    std::fs::write(
        args.output.join("verification.json"),
        serde_json::to_string_pretty(&receipt)? + "\n",
    )?;
    println!(
        "Filled {filled} perimeter samples; preserved {preserved}; {}",
        args.output.display()
    );
    Ok(())
}

#[cfg(not(feature = "moebius"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("Build this example with --features moebius and matching LibTorch 2.13")
}
