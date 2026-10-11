//! Run the native detector on owned validation copies; keep previews/masks/line overlays.
use anyhow::Result;
use rawpuppy::{
    color,
    edits::Edits,
    input::SensorImage,
    perspective::{self, Mode},
    render::{Backend, Renderer},
};
use std::{path::PathBuf, sync::Arc, time::Instant};

fn main() -> Result<()> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    let mut renderer = Renderer::new(Backend::Auto);
    #[cfg(feature = "sam3")]
    let model = rawpuppy::sam3::Model::open(
        &rawpuppy::sam3::directory()?,
        rawpuppy::ml_runtime::InferenceDevice::Auto,
    )?;
    #[cfg(feature = "sam3")]
    println!("SAM 3 inference device: {:?}", model.device());
    for arg in std::env::args_os().skip(1) {
        let directory = PathBuf::from(arg);
        let source = Arc::new(SensorImage::open(&directory.join("input.raf"))?);
        let edits = Edits::for_image(&source);
        let start = Instant::now();
        let preview = perspective::preview(&mut renderer, source.clone(), &edits)?;
        let rgb: Vec<u8> = preview
            .pixels
            .iter()
            .flat_map(|p| {
                [p[0], p[1], p[2]]
                    .map(|v| (color::srgb_encode(v).clamp(0., 1.) * 255.).round() as u8)
            })
            .collect();
        let base =
            image::RgbImage::from_raw(preview.width as u32, preview.height as u32, rgb).unwrap();
        base.save(directory.join("analysis.png"))?;
        #[cfg(feature = "sam3")]
        let mask = {
            let mask = model.mask(&preview)?;
            let bytes: Vec<u8> = mask
                .values
                .iter()
                .map(|v| (v * 255.).round() as u8)
                .collect();
            image::GrayImage::from_raw(mask.width as u32, mask.height as u32, bytes)
                .unwrap()
                .save(directory.join("mask.png"))?;
            Some(mask)
        };
        #[cfg(not(feature = "sam3"))]
        let mask: Option<perspective::Mask> = None;
        for (name, mask) in [("classical", None), ("semantic", mask.as_ref())] {
            if name == "semantic" && mask.is_none() {
                continue;
            }
            for (mode_name, mode) in [("vertical", Mode::Vertical), ("both", Mode::Both)] {
                let report = perspective::analyze(&preview, mask, &edits.geometry, mode)?;
                let stem = format!("{name}-{mode_name}");
                std::fs::write(
                    directory.join(format!("{stem}.json")),
                    serde_json::to_vec_pretty(&report)?,
                )?;
                let mut overlay = base.clone();
                for (index, s) in report.segments.iter().enumerate() {
                    let coordinate = |p: [f64; 2]| {
                        [
                            (p[0] + 0.5) * preview.width as f64,
                            p[1] * preview.width as f64 + preview.height as f64 * 0.5,
                        ]
                    };
                    let a = coordinate(s.start);
                    let b = coordinate(s.end);
                    let steps = ((b[0] - a[0]).hypot(b[1] - a[1]).ceil() as usize).max(1);
                    let color = if report.vertical_inliers.contains(&index) {
                        [40, 255, 80]
                    } else if report.horizontal_inliers.contains(&index) {
                        [70, 180, 255]
                    } else {
                        [255, 80, 70]
                    };
                    for i in 0..=steps {
                        let t = i as f64 / steps as f64;
                        let x = (a[0] * (1. - t) + b[0] * t).round() as u32;
                        let y = (a[1] * (1. - t) + b[1] * t).round() as u32;
                        if x < overlay.width() && y < overlay.height() {
                            overlay.put_pixel(x, y, image::Rgb(color));
                        }
                    }
                }
                overlay.save(directory.join(format!("{stem}-lines.png")))?;
                println!(
                    "{} {stem}: {} segments, {} vertical, {} horizontal; {}",
                    directory.display(),
                    report.segments.len(),
                    report.vertical_inliers.len(),
                    report.horizontal_inliers.len(),
                    report.reason
                );
                if let Some(g) = report.solution {
                    let mut corrected = edits.clone();
                    corrected.geometry = g;
                    let rendered = renderer.render(source.clone(), &corrected, Some(1440))?;
                    rawpuppy::export::write(
                        &directory.join(format!("{stem}.png")),
                        &rendered,
                        color::OutputSpace::Srgb,
                        true,
                    )?;
                }
            }
        }
        println!("Elapsed {:.3}s", start.elapsed().as_secs_f64());
    }
    Ok(())
}
