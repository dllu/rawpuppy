//! Reproduce the README comparison from a local copy of DSCF0027 (The Standard, London).
//! The manual recipe is specific to that photograph, checked against its cooked reference.
use anyhow::{Context, Result};
use rawpuppy::{
    calibration::{self, Illuminant},
    color::OutputSpace,
    edits::{Edits, ToneMapper},
    export,
    input::SensorImage,
    perspective::{Engine, MaskProvider, Mode},
    render::{Backend, Renderer},
    sidecar,
};
use std::{path::PathBuf, sync::Arc};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let input = PathBuf::from(args.next().context("Supply a local RAW copy")?);
    let output = PathBuf::from(args.next().context("Supply an owned output directory")?);
    std::fs::create_dir_all(&output)?;
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    let source = Arc::new(SensorImage::open(&input)?);
    let mut renderer = Renderer::new(Backend::Auto);
    renderer.set_document(input.clone());
    let mut before = Edits::for_image(&source);
    before.scene.illuminant = Illuminant::Legacy;
    before.raw.hot_pixels = true;
    before.tone.mapper = ToneMapper::Linear;
    let mut after = before.clone();
    after.tone.mapper = ToneMapper::AgxSdr;
    after.scene.exposure = 2.1;
    after.tone.saturation = 1.04;
    let report = Engine::default().run(
        &mut renderer,
        source.clone(),
        &after,
        Mode::Both,
        MaskProvider::Auto,
    )?;
    eprintln!(
        "Automatic perspective: {} · {}",
        report.mask_provider, report.reason
    );
    after.geometry = report.solution.context("No reliable two-axis correction")?;
    // Preserve this photograph's chosen composition. Pose is estimated by the
    // same general detector as the editor; reference exports never enter it.
    after.geometry.scale = 1.0821165;
    after.geometry.shift = [0.01581575, -0.3030224];
    after.geometry.crop = [0., 0., 1., 1.];
    let sample = calibration::estimate(&source, &after, [0., 0., 1., 1.])?;
    after.scene.illuminant = Illuminant::Custom { xy: sample.xy };
    for (name, edits) in [("before", &before), ("after", &after)] {
        std::fs::write(
            output.join(format!("{name}.json")),
            serde_json::to_vec_pretty(edits)?,
        )?;
        sidecar::save(&output.join(format!("{name}.rawpuppy.xmp")), edits)?;
        let rendered = renderer.render(source.clone(), edits, None)?;
        let rgb = image::RgbImage::from_raw(
            rendered.width as u32,
            rendered.height as u32,
            rendered
                .pixels
                .iter()
                .flat_map(|p| {
                    [p[0], p[1], p[2]].map(|v| {
                        (rawpuppy::color::srgb_encode(v).clamp(0., 1.) * 255.).round() as u8
                    })
                })
                .collect(),
        )
        .unwrap();
        let rgb = image::imageops::resize(
            &rgb,
            1200,
            (1200. * rendered.height as f64 / rendered.width as f64).round() as u32,
            image::imageops::FilterType::Lanczos3,
        );
        let preview = rawpuppy::pipeline::Rendered {
            width: rgb.width() as usize,
            height: rgb.height() as usize,
            pixels: rgb
                .pixels()
                .map(|p| {
                    [
                        rawpuppy::color::srgb_decode(p[0] as f32 / 255.),
                        rawpuppy::color::srgb_decode(p[1] as f32 / 255.),
                        rawpuppy::color::srgb_decode(p[2] as f32 / 255.),
                        1.,
                    ]
                })
                .collect(),
        };
        export::write(
            &output.join(format!("{name}.jpg")),
            &preview,
            OutputSpace::Srgb,
            true,
        )?;
    }
    eprintln!(
        "{} × {} source; {}",
        source.metadata.width,
        source.metadata.height,
        renderer.label()
    );
    Ok(())
}
