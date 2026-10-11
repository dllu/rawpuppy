//! Evaluate a corpus of owned RAW copies. Reference exports are used only by the
//! independent scoring script, never passed to the production detector.
use anyhow::{Context, Result};
use rawpuppy::{
    color::OutputSpace,
    edits::Edits,
    export,
    input::SensorImage,
    perspective::{self, Mode},
    render::{Backend, Renderer},
};
use std::{path::PathBuf, sync::Arc};

fn main() -> Result<()> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().context("Supply the owned corpus directory")?);
    let output = PathBuf::from(args.next().context("Supply a separate result directory")?);
    let split = args.next().map(|v| v.to_string_lossy().into_owned());
    std::fs::create_dir_all(&output)?;
    let mut renderer = Renderer::new(Backend::Auto);
    #[cfg(feature = "sam3")]
    let model = rawpuppy::sam3::Model::open(
        &rawpuppy::sam3::directory()?,
        rawpuppy::ml_runtime::InferenceDevice::Auto,
    )?;
    let mut paths = std::fs::read_dir(&root)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("case.json").is_file())
        .collect::<Vec<_>>();
    paths.sort();
    for directory in paths {
        let case: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("case.json"))?)?;
        if split
            .as_ref()
            .is_some_and(|split| case["split"].as_str() != Some(split))
        {
            continue;
        }
        let source = Arc::new(SensorImage::open(
            &directory.join(case["input"].as_str().context("Missing input")?),
        )?);
        let edits = Edits::for_image(&source);
        let out = output.join(directory.file_name().unwrap());
        std::fs::create_dir_all(&out)?;
        std::fs::write(out.join("initial.json"), serde_json::to_vec_pretty(&edits)?)?;
        std::fs::write(
            out.join("metadata.json"),
            serde_json::to_vec_pretty(&source.metadata)?,
        )?;
        let preview = perspective::preview(&mut renderer, source.clone(), &edits)?;
        export::write(&out.join("analysis.png"), &preview, OutputSpace::Srgb, true)?;
        #[cfg(feature = "sam3")]
        let mask = {
            let mask = model.mask(&preview)?;
            image::GrayImage::from_raw(
                mask.width as u32,
                mask.height as u32,
                mask.values
                    .iter()
                    .map(|v| (v * 255.).round() as u8)
                    .collect(),
            )
            .unwrap()
            .save(out.join("mask.png"))?;
            Some(mask)
        };
        #[cfg(not(feature = "sam3"))]
        let mask: Option<perspective::Mask> = None;
        for (label, mask) in [("classical", None), ("semantic", mask.as_ref())] {
            if label == "semantic" && mask.is_none() {
                continue;
            }
            for (name, mode) in [("vertical", Mode::Vertical), ("both", Mode::Both)] {
                let report = perspective::analyze(&preview, mask, &edits.geometry, mode)?;
                std::fs::write(
                    out.join(format!("{label}-{name}.json")),
                    serde_json::to_vec_pretty(&report)?,
                )?;
                println!(
                    "{} {label}-{name}: {}",
                    directory.file_name().unwrap().to_string_lossy(),
                    report.reason
                );
                if let Some(geometry) = report.solution {
                    let mut corrected = edits.clone();
                    corrected.geometry = geometry;
                    let image = renderer.render(source.clone(), &corrected, Some(1440))?;
                    export::write(
                        &out.join(format!("{label}-{name}.jpg")),
                        &image,
                        OutputSpace::Srgb,
                        true,
                    )?;
                }
            }
        }
    }
    Ok(())
}
