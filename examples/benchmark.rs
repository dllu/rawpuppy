//! Headless end-to-end preview measurements; retains the source and GPU session between edits.
use anyhow::Result;
use clap::Parser;
use rawpuppy::{
    edits::Edits,
    input::SensorImage,
    render::{Backend, CudaMemoryMode, Renderer},
};
use std::{path::PathBuf, sync::Arc, time::Instant};

#[derive(Parser)]
struct Args {
    input: PathBuf,
    #[arg(long, value_enum, default_value = "auto")]
    backend: Backend,
    #[arg(long, default_value_t = 1800)]
    max_edge: usize,
    #[arg(long, default_value_t = 6)]
    iterations: usize,
    #[arg(long)]
    no_camera_corrections: bool,
    #[arg(long, value_enum, default_value = "auto")]
    cuda_memory: CudaMemoryMode,
}
fn main() -> Result<()> {
    #[cfg(feature = "cuda")]
    if std::env::var_os("RUST_MIN_STACK").is_none() {
        // SAFETY: process startup precedes all thread/runtime initialization.
        unsafe {
            std::env::set_var("RUST_MIN_STACK", "33554432");
        }
    }
    let args = Args::parse();
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    let start = Instant::now();
    let source = Arc::new(SensorImage::open(&args.input)?);
    println!("decode_ms={:.2}", start.elapsed().as_secs_f64() * 1000.);
    let mut renderer = Renderer::with_cuda_memory(args.backend, args.cuda_memory);
    let mut edits = if args.no_camera_corrections {
        Edits::default()
    } else {
        Edits::for_image(&source)
    };
    let mut times = Vec::new();
    for i in 0..args.iterations {
        edits.scene.exposure = i as f32 * 0.1;
        let start = Instant::now();
        let image = renderer.render(source.clone(), &edits, Some(args.max_edge))?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.;
        let checksum: f64 = image.pixels.iter().step_by(1009).map(|p| p[0] as f64).sum();
        println!(
            "iteration={i} render_ms={elapsed:.3} checksum={checksum:.6} backend={}",
            renderer.label()
        );
        if i > 0 {
            times.push(elapsed);
        }
    }
    if !times.is_empty() {
        println!(
            "warm_mean_ms={:.3}",
            times.iter().sum::<f64>() / times.len() as f64
        );
    }
    Ok(())
}
