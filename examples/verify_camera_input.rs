//! Real-camera decode, framing and composed CPU/GPU checks on a read-only input.
use anyhow::{Result, ensure};
use clap::Parser;
use rawpuppy::{
    edits::Edits,
    input::SensorImage,
    models,
    render::{Backend, Renderer},
};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc, time::Instant};

#[derive(Parser)]
struct Args {
    input: PathBuf,
    output: PathBuf,
    #[arg(long, value_enum, num_args = 1.., default_values = ["cuda", "vulkan"])]
    backend: Vec<Backend>,
    #[arg(long)]
    expected_orientation: Option<u16>,
}

fn main() -> Result<()> {
    #[cfg(feature = "cuda")]
    if std::env::var_os("RUST_MIN_STACK").is_none() {
        // SAFETY: process startup precedes thread/runtime initialization.
        unsafe { std::env::set_var("RUST_MIN_STACK", "33554432") };
    }
    let args = Args::parse();
    ensure!(!args.output.exists(), "Choose a new output directory");
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    let input_hash = models::sha256(&args.input)?;
    let started = Instant::now();
    let source = Arc::new(SensorImage::open(&args.input)?);
    let decode_seconds = started.elapsed().as_secs_f64();
    let sensor_hash = Sha256::digest(bytemuck::cast_slice(&source.data));
    if let Some(expected) = args.expected_orientation {
        ensure!(
            source.orientation.to_u16() == expected,
            "EXIF orientation mismatch"
        );
    }
    let defaults = Edits::for_image(&source);
    let mut cpu = Renderer::new(Backend::Cpu);
    let (w, h) = cpu.dimensions(source.clone(), &defaults, None)?;
    let mut missing = 0;
    let mut perimeter_samples = 0;
    for (region, ew, eh) in [
        ([0., 0., 1., 1. / h as f32], w, 1),
        ([0., 1. - 1. / h as f32, 1., 1. / h as f32], w, 1),
        ([0., 0., 1. / w as f32, 1.], 1, h),
        ([1. - 1. / w as f32, 0., 1. / w as f32, 1.], 1, h),
    ] {
        let edge = cpu.render_region(source.clone(), &defaults, region, ew, eh)?;
        ensure!(
            edge.pixels.iter().flatten().all(|v| v.is_finite()),
            "Nonfinite perimeter"
        );
        missing += edge.pixels.iter().filter(|p| p[3] < 1.).count();
        perimeter_samples += edge.pixels.len();
    }
    std::fs::create_dir_all(&args.output)?;
    let mut edited = defaults.clone();
    edited.geometry.crop = [0.15, 0.1, 0.7, 0.75];
    edited.geometry.rotation = 3.;
    edited.geometry.yaw = 2.;
    edited.geometry.pitch = -1.;
    edited.geometry.distortion = [-0.01, 0.005];
    edited.geometry.chromatic_aberration = [0.0004, -0.0003];
    edited.scene.exposure = 0.7;
    edited.scene.calibration = [1.05, 0.95, 1.];
    edited.scene.graduated.exposure = 0.5;
    edited.scene.graduated.angle = 35.;
    edited.display.curve = vec![[0., 0.], [0.5, 0.6], [1., 1.]];
    edited.display.split_strength = 0.15;
    edited.display.shadows = [0.9, 0.95, 1.1];
    edited.display.highlights = [1.1, 1.02, 0.85];
    let mut comparisons = Vec::new();
    for (name, edits) in [("camera-defaults", &defaults), ("composed-edit", &edited)] {
        let reference = cpu.render(source.clone(), edits, Some(512))?;
        ensure!(
            reference.pixels.iter().flatten().all(|v| v.is_finite()),
            "Nonfinite CPU preview"
        );
        rawpuppy::export::write(
            &args.output.join(format!("{name}.png")),
            &reference,
            rawpuppy::color::OutputSpace::Srgb,
            false,
        )?;
        for backend in &args.backend {
            let mut renderer = Renderer::new(*backend);
            let started = Instant::now();
            let actual = renderer.render(source.clone(), edits, Some(512))?;
            let seconds = started.elapsed().as_secs_f64();
            ensure!(
                actual.width == reference.width && actual.height == reference.height,
                "Preview size mismatch"
            );
            ensure!(
                actual.pixels.iter().flatten().all(|v| v.is_finite()),
                "Nonfinite GPU preview"
            );
            let mut max = 0f32;
            let mut sum = 0f64;
            let mut alpha_mismatches = 0;
            for (a, b) in actual.pixels.iter().zip(&reference.pixels) {
                alpha_mismatches += usize::from(a[3] != b[3]);
                for c in 0..3 {
                    let error = (a[c] - b[c]).abs();
                    max = max.max(error);
                    sum += error as f64;
                }
            }
            let mean = sum / (3 * actual.pixels.len()) as f64;
            let mut warm_seconds = Vec::new();
            for i in 0..3 {
                let mut changed = edits.clone();
                changed.scene.exposure += i as f32 * 0.1;
                let started = Instant::now();
                let preview = renderer.render(source.clone(), &changed, Some(1800))?;
                let seconds = started.elapsed().as_secs_f64();
                ensure!(
                    preview.pixels.iter().flatten().all(|v| v.is_finite()),
                    "Nonfinite large preview"
                );
                if i > 0 {
                    warm_seconds.push(seconds);
                }
            }
            comparisons.push(serde_json::json!({
                "recipe": name, "backend": format!("{backend:?}"), "device_label": renderer.label(),
                "preview_dimensions": [actual.width,actual.height], "cold_render_seconds": seconds,
                "max_absolute_rgb_error": max, "mean_absolute_rgb_error": mean,
                "alpha_mismatches": alpha_mismatches, "warm_1800_preview_seconds": warm_seconds,
                "passed": max <= 0.003 && mean <= 0.00003 && alpha_mismatches == 0,
            }));
        }
    }
    ensure!(
        models::sha256(&args.input)? == input_hash,
        "Input bytes changed"
    );
    ensure!(
        Sha256::digest(bytemuck::cast_slice(&source.data)) == sensor_hash,
        "Sensor allocation changed"
    );
    let receipt = serde_json::json!({
        "scope": "real-camera decode, default full-resolution perimeter and composed preview CPU/GPU agreement; not an independent lens/color oracle",
        "build_profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "input_sha256": input_hash, "sensor_unchanged": true, "input_unchanged": true,
        "metadata": source.metadata, "orientation": source.orientation.to_u16(),
        "decode_seconds": decode_seconds, "output_dimensions": [w,h],
        "default_perimeter_samples": perimeter_samples, "default_missing_perimeter_samples": missing,
        "perimeter_count_convention": "four full-resolution edge strips; corner points counted twice",
        "default_edits": defaults, "composed_edits": edited,
        "tolerances": {"max_absolute_rgb":0.003,"mean_absolute_rgb":0.00003,"alpha_mismatches":0},
        "comparisons": comparisons,
    });
    std::fs::write(
        args.output.join("receipt.json"),
        serde_json::to_string_pretty(&receipt)? + "\n",
    )?;
    println!(
        "{}",
        serde_json::json!({"lens":source.metadata.lens_model,"orientation":source.orientation.to_u16(),"missing_perimeter_samples":missing,"comparisons":comparisons})
    );
    ensure!(
        missing == 0,
        "Default framing left transparent perimeter samples"
    );
    ensure!(
        source.metadata.lens_profile_error.is_none(),
        "Embedded lens metadata failed to parse"
    );
    ensure!(
        comparisons.iter().all(|c| c["passed"] == true),
        "CPU/GPU preview mismatch; inspect receipt"
    );
    Ok(())
}
