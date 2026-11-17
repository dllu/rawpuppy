use super::{AgxEdits, BLACK_EV, Curve, WHITE_EV, rec2020_in_gamut, relative_ev};
use crate::{color, edits::Edits, input::SensorImage, pipeline::Pipeline};
use anyhow::{Result, ensure};
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pick {
    White,
    Black,
    Both,
    PivotExposure,
    PivotAndOutput,
}

pub(crate) fn estimate(
    source: &SensorImage,
    edits: &Edits,
    region: [f32; 4],
    pick: Pick,
) -> Result<AgxEdits> {
    let mut agx = edits.tone.agx;
    let mut scene = edits.clone();
    scene.tone.mapper = crate::edits::ToneMapper::Linear;
    scene.display.retouch.clear();
    let pipeline = Pipeline::compile(source, &scene)?;
    let [u, v, width, height] = region;
    let columns = (width * pipeline.geometry.width as f32).max(1.);
    let rows = (height * pipeline.geometry.height as f32).max(1.);
    let scale = (640. / columns.max(rows)).min(1.);
    let columns = (columns * scale).round().max(1.) as usize;
    let rows = (rows * scale).round().max(1.) as usize;
    let input = agx.primaries.matrices()?.0;
    let pivot = matches!(pick, Pick::PivotExposure | Pick::PivotAndOutput);
    let mut values: Vec<_> = (0..rows * columns)
        .into_par_iter()
        .filter_map(|i| {
            let uv = [
                u + (i % columns) as f32 * width / columns as f32 + 0.5 * width / columns as f32,
                v + (i / columns) as f32 * height / rows as f32 + 0.5 * height / rows as f32,
            ];
            let scene = rec2020_in_gamut(pipeline.sample_scene(uv)?);
            let value = if pivot {
                color::apply(color::REC2020_TO_XYZ, scene)[1]
            } else {
                // A bright saturated color can clip before its luminance reaches white.
                color::apply(input, scene).into_iter().fold(0., f32::max)
            };
            value.is_finite().then_some(value.max(0.))
        })
        .collect();
    ensure!(
        !values.is_empty(),
        "The selection contains no photograph pixels"
    );
    let ev = |value: f32| relative_ev(value.max(f32::MIN_POSITIVE));
    if pivot {
        let mean = values.iter().map(|&v| f64::from(v)).sum::<f64>() / values.len() as f64;
        agx.pivot_ev = ev(mean as f32);
        agx.clamp_pivot();
        if pick == Pick::PivotAndOutput {
            let before_look = AgxEdits {
                slope: 1.,
                lift: 0.,
                brightness: 1.,
                ..edits.tone.agx
            };
            agx.pivot_output = Curve::new(&before_look)
                .at(agx.pivot_ev)
                .clamp(agx.target_black + 0.001, agx.target_white - 0.001);
        }
    } else {
        // Trim a small tail on large crops to avoid choosing a hot pixel or
        // a patch of reconstruction noise as the endpoint.
        let tail = values.len() / 1000;
        let last = values.len() - 1;
        let scale = 1. + agx.range_margin;
        let percentile =
            |values: &mut [f32], index| *values.select_nth_unstable_by(index, f32::total_cmp).1;
        if matches!(pick, Pick::White | Pick::Both) {
            agx.white_ev = (ev(percentile(&mut values, last - tail)).max(0.) * scale)
                .clamp(*WHITE_EV.start(), *WHITE_EV.end());
        }
        if matches!(pick, Pick::Black | Pick::Both) {
            agx.black_ev = (ev(percentile(&mut values, tail)).min(0.) * scale)
                .clamp(*BLACK_EV.start(), *BLACK_EV.end());
        }
        agx.clamp_pivot();
    }
    Ok(agx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_levels_use_scene_exposure_and_cropped_area_independently_of_display() {
        let input = [0., 0.18 / 32., 0.18, 0.72, 16.];
        let rgb = input.into_iter().flat_map(|v| [v; 3]).collect();
        let source = SensorImage::from_rgb(5, 1, rgb).unwrap();
        let mut edits = Edits::default();
        edits.scene.exposure = 1.;
        edits.geometry.crop = [0.2, 0., 0.6, 1.];
        edits.tone.agx.range_margin = 0.;
        edits.tone.agx.lift = 0.09;
        edits.tone.saturation = 0.;
        edits.display.curve = vec![[0., 0.], [0.5, 0.75], [1., 1.]];

        let full = [0., 0., 1., 1.];
        let agx = estimate(&source, &edits, full, Pick::Both).unwrap();
        assert!((agx.black_ev + 4.).abs() < 1e-4, "{}", agx.black_ev);
        assert!((agx.white_ev - 3.).abs() < 1e-4, "{}", agx.white_ev);
        assert_eq!(agx.lift, edits.tone.agx.lift);
        let black = estimate(&source, &edits, full, Pick::Black).unwrap();
        assert_eq!(black.black_ev, agx.black_ev);
        assert_eq!(black.white_ev, edits.tone.agx.white_ev);
        let white = estimate(&source, &edits, full, Pick::White).unwrap();
        assert_eq!(white.white_ev, agx.white_ev);
        assert_eq!(white.black_ev, edits.tone.agx.black_ev);

        let selected = estimate(
            &source,
            &edits,
            [1. / 3., 0., 1. / 3., 1.],
            Pick::PivotExposure,
        )
        .unwrap();
        assert!(
            (selected.pivot_ev - 1.).abs() < 1e-4,
            "{}",
            selected.pivot_ev
        );
        assert_eq!(selected.pivot_output, edits.tone.agx.pivot_output);
    }

    #[test]
    fn geometric_gaps_do_not_become_black_samples() {
        let source = SensorImage::from_rgb(60, 40, vec![0.18 / 8.; 60 * 40 * 3]).unwrap();
        let mut edits = Edits::default();
        edits.geometry.rotation = 40.;
        let agx = estimate(&source, &edits, [0., 0., 1., 1.], Pick::Both).unwrap();
        assert!((agx.black_ev + 3.3).abs() < 1e-5);
        assert_eq!(agx.white_ev, *WHITE_EV.start());
    }
}
