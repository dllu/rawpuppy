#![cfg(feature = "raw-ml")]
use rawpuppy::{input::SensorImage, ml_runtime::InferenceDevice, raw_ml::BayerModel};

fn model() -> BayerModel {
    let directory = std::env::var_os("RAWPUPPY_TEST_RAWNIND_GRAPH")
        .expect("Set RAWPUPPY_TEST_RAWNIND_GRAPH to the verified prepared graph directory");
    BayerModel::open(std::path::Path::new(&directory), InferenceDevice::Auto).unwrap()
}

fn sensor(width: usize, height: usize, pattern: &str) -> SensorImage {
    let mut source = SensorImage::from_rgb(width, height, vec![0.; width * height * 3]).unwrap();
    let cfa = rawler::CFA::new(pattern);
    source.data = (0..width * height)
        .map(|index| {
            let x = index % width;
            let y = index / width;
            let rgb = [
                0.05 + 0.25 * x as f32 / width as f32,
                0.18 + 0.01 * y as f32 / height as f32,
                0.25,
            ];
            rgb[cfa.color_at(y, x)]
        })
        .collect();
    source.cfa = Some(cfa);
    source.cpp = 1;
    source
}

#[test]
#[ignore = "requires the verified external RawNIND graph"]
fn learned_reconstruction_handles_bayer_phases_large_width_and_preserves_photometry() {
    let model = model();
    let mut phase_pixels = Vec::new();
    for pattern in ["RGGB", "BGGR", "GRBG", "GBRG"] {
        let source = sensor(100_003, 5, pattern);
        let original = source.data.clone();
        let patch = model
            .reconstruct_patch(&source, [99_999, 1], [4, 3])
            .unwrap();
        assert_eq!(patch.size, [4, 3]);
        assert_eq!(patch.origin, [99_999, 1]);
        assert_eq!(source.data, original);
        let mut input_sum = 0f64;
        let mut output_sum = 0f64;
        for y in 0..3 {
            for x in 0..4 {
                let channel = source.cfa.as_ref().unwrap().color_at(y + 1, x + 99_999);
                input_sum += source.data[(y + 1) * 100_003 + x + 99_999] as f64;
                output_sum += patch.pixels[y * 4 + x][channel] as f64;
            }
        }
        assert!((input_sum - output_sum).abs() < 2e-6);
        phase_pixels.push(
            patch
                .pixels
                .iter()
                .copied()
                .reduce(|a, b| std::array::from_fn(|c| a[c] + b[c]))
                .unwrap()
                .map(|v| v / 12.),
        );
    }
    // Constant-neighborhood camera colors should not swap when the sensor's
    // Bayer phase changes. Allow the model's subpixel reconstruction variation.
    for pixel in &phase_pixels[1..] {
        for c in 0..3 {
            assert!(
                (pixel[c] - phase_pixels[0][c]).abs() < 0.01,
                "{phase_pixels:?}"
            );
        }
    }
    let incomplete = sensor(1, 1, "RGGB");
    assert!(
        model
            .reconstruct_patch(&incomplete, [0, 0], [1, 1])
            .is_err()
    );
    let mut malformed = sensor(3, 3, "RGGB");
    malformed.data.pop();
    assert!(model.reconstruct_patch(&malformed, [0, 0], [2, 2]).is_err());
}

#[test]
#[ignore = "requires the verified external RawNIND graph"]
fn overlapping_contexts_agree_before_local_gain_matching() {
    let model = model();
    let source = sensor(600, 600, "GRBG");
    let first = model
        .reconstruct_patch(&source, [280, 281], [71, 59])
        .unwrap();
    let second = model
        .reconstruct_patch(&source, [312, 310], [39, 30])
        .unwrap();
    assert_ne!(first.context_origin, second.context_origin);
    for y in 0..30 {
        for x in 0..39 {
            let a = first.pixels[(y + 29) * 71 + x + 32];
            let b = second.pixels[y * 39 + x];
            for c in 0..3 {
                let aligned_gain = a[c] * second.gain / first.gain;
                assert!(
                    (aligned_gain - b[c]).abs() < 3e-5,
                    "ROI changed model structure at {x},{y},{c}: {aligned_gain} / {}",
                    b[c]
                );
            }
        }
    }
}

#[test]
#[ignore = "requires the verified external RawNIND graph"]
fn tiled_reconstruction_matches_single_context_and_ignores_sensor_margins() {
    use rawpuppy::{
        edits::{Edits, Reconstruction},
        pipeline::Pipeline,
    };
    let model = model();
    let mut source = sensor(79, 83, "BGGR");
    source.origin = [7, 9];
    source.active = [61, 63];
    source.metadata.width = 61;
    source.metadata.height = 63;
    for y in 0..83 {
        for x in 0..79 {
            if !(7..68).contains(&x) || !(9..72).contains(&y) {
                source.data[y * 79 + x] = 9.;
            }
        }
    }
    let original = source.data.clone();
    let tiled = model
        .reconstruct_image(&source, false, 32, |_, _| {})
        .unwrap();
    let one = model
        .reconstruct_image(&source, false, 1024, |_, _| {})
        .unwrap();
    let max = tiled
        .data
        .iter()
        .zip(&one.data)
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    assert!(
        max < 3e-6,
        "Tile seams or pooling/photometry mismatch: {max}"
    );
    assert_eq!(source.data, original);
    let mut edits = Edits::default();
    edits.raw.reconstruction = Reconstruction::RawNindV1;
    let rendered = Pipeline::compile(&tiled, &edits)
        .unwrap()
        .render(None)
        .unwrap();
    assert_eq!((rendered.width, rendered.height), (61, 63));
    assert!(rendered.pixels.iter().all(|p| p[3] == 1.));
    assert!(tiled.data.iter().all(|v| v.abs() < 1.));
}

#[test]
#[ignore = "requires the verified RawNIND graph installed in the standard model cache"]
fn renderer_composes_learned_camera_rgb_and_releases_retired_sources() {
    use rawpuppy::{
        edits::{Edits, Reconstruction, ToneMapper},
        pipeline::Pipeline,
        render::{Backend, Renderer},
    };
    use std::sync::Arc;
    let model = model();
    let source = Arc::new(sensor(96, 98, "GBRG"));
    let original = source.data.clone();
    let retired = Arc::downgrade(&source);
    let prepared = model
        .reconstruct_image(&source, false, 1024, |_, _| {})
        .unwrap();
    let mut edits = Edits::default();
    edits.raw.reconstruction = Reconstruction::RawNindV1;
    edits.tone.mapper = ToneMapper::Linear;
    let mut renderer = Renderer::new(Backend::Cpu);
    for exposure in [0., 0.5, -0.2] {
        edits.scene.exposure = exposure;
        let actual = renderer.render(source.clone(), &edits, Some(32)).unwrap();
        let expected = Pipeline::compile(&prepared, &edits)
            .unwrap()
            .render(Some(32))
            .unwrap();
        for (a, b) in actual.pixels.iter().zip(expected.pixels) {
            for c in 0..4 {
                assert!((a[c] - b[c]).abs() < 2e-6);
            }
        }
    }
    assert_eq!(source.data, original);
    drop(source);
    assert!(retired.upgrade().is_some());
    let next = Arc::new(SensorImage::from_rgb(3, 3, vec![0.2; 27]).unwrap());
    renderer.render(next, &Edits::default(), None).unwrap();
    assert!(retired.upgrade().is_none());
}
