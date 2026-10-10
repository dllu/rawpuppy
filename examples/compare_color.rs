//! Mean linear-color measurements on independently exported image patches.
use anyhow::{Result, ensure};
use clap::Parser;
use rawpuppy::input::SensorImage;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[arg(num_args = 2, required = true)]
    paths: Vec<PathBuf>,
    /// Also compare every decoded working-RGB component, requiring equal dimensions.
    #[arg(long)]
    pixels: bool,
}
fn mean(image: &SensorImage, uv: [f32; 4]) -> [f64; 3] {
    let w = image.metadata.width;
    let h = image.metadata.height;
    let x0 = (uv[0] * w as f32) as usize;
    let y0 = (uv[1] * h as f32) as usize;
    let x1 = ((uv[0] + uv[2]) * w as f32) as usize;
    let y1 = ((uv[1] + uv[3]) * h as f32) as usize;
    let mut total = [0.; 3];
    let mut n = 0.;
    for y in y0..y1.min(h) {
        for x in x0..x1.min(w) {
            for (sum, value) in total
                .iter_mut()
                .zip(&image.data[(y * w + x) * 3..(y * w + x) * 3 + 3])
            {
                *sum += *value as f64;
            }
            n += 1.;
        }
    }
    total.map(|v| v / n)
}
fn main() -> Result<()> {
    let args = Args::parse();
    let a = SensorImage::open(&args.paths[0])?;
    let b = SensorImage::open(&args.paths[1])?;
    ensure!(a.cpp == 3 && b.cpp == 3, "RGB exports required");
    println!(
        "dimensions: {}x{} versus {}x{}",
        a.metadata.width, a.metadata.height, b.metadata.width, b.metadata.height
    );
    for uv in [
        [0., 0., 1., 1.],
        [0.1, 0.1, 0.1, 0.1],
        [0.4, 0.2, 0.1, 0.1],
        [0.4, 0.4, 0.1, 0.1],
        [0.3, 0.7, 0.1, 0.1],
        [0.7, 0.7, 0.1, 0.1],
    ] {
        let aa = mean(&a, uv);
        let bb = mean(&b, uv);
        println!(
            "region={uv:?} mean_a={aa:?} mean_b={bb:?} ratio={:?}",
            std::array::from_fn::<_, 3, _>(|i| aa[i] / bb[i])
        );
    }
    if args.pixels {
        ensure!(
            (a.metadata.width, a.metadata.height) == (b.metadata.width, b.metadata.height),
            "All-pixel comparison requires matching dimensions"
        );
        ensure!(a.data.len() == b.data.len(), "Different component counts");
        let mut changed = 0usize;
        let mut maximum = 0f64;
        let mut sum = 0f64;
        let mut squared = 0f64;
        for (a, b) in a.data.iter().zip(&b.data) {
            let difference = (f64::from(*a) - f64::from(*b)).abs();
            changed += usize::from(a != b);
            maximum = maximum.max(difference);
            sum += difference;
            squared += difference * difference;
        }
        let count = a.data.len();
        println!(
            "{}",
            serde_json::json!({"scope":"Every decoded working-linear RGB component; no alpha, perceptual or physical-colorimetry claim","dimensions":[a.metadata.width,a.metadata.height],"components":count,"changed_components":changed,"maximum_absolute_difference":maximum,"mean_absolute_difference":sum/count as f64,"rms_difference":(squared/count as f64).sqrt(),"a_rgb_sha256":format!("{:x}",Sha256::digest(bytemuck::cast_slice(&a.data))),"b_rgb_sha256":format!("{:x}",Sha256::digest(bytemuck::cast_slice(&b.data)))})
        );
    }
    Ok(())
}
