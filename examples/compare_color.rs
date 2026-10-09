//! Mean linear-color measurements on independently exported image patches.
use anyhow::{Result, ensure};
use rawpuppy::input::SensorImage;
use std::path::PathBuf;
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
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    ensure!(paths.len() == 2, "Pass two float exports");
    let a = SensorImage::open(&paths[0])?;
    let b = SensorImage::open(&paths[1])?;
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
    Ok(())
}
