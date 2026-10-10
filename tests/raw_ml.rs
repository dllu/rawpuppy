#![cfg(feature = "raw-ml")]
use rawpuppy::{input::SensorImage, ml_runtime::InferenceDevice, raw_ml::BayerModel};

fn model() -> BayerModel {
    let directory = std::env::var_os("RAWPUPPY_TEST_RAWNIND_GRAPH")
        .expect("Set RAWPUPPY_TEST_RAWNIND_GRAPH to the verified prepared graph directory");
    BayerModel::open(std::path::Path::new(&directory), InferenceDevice::Auto).unwrap()
}

#[test]
#[ignore = "requires the verified external RawNIND graph"]
fn learned_cache_retains_original_clipping_instead_of_guessing_from_model_values() {
    use rawpuppy::{
        edits::{Edits, Reconstruction, ToneMapper},
        pipeline::Pipeline,
    };
    let model = model();
    for clipped in [false, true] {
        let mut original = sensor(37, 41, "RGGB");
        original.raw_integer = true;
        original.metadata.as_shot = [2., 1., 1.5];
        let cfa = original.cfa.as_ref().unwrap();
        let rgb = if clipped {
            [0.6, 0.995, 0.8]
        } else {
            [0.2, 0.3, 0.4]
        };
        for i in 0..original.data.len() {
            original.data[i] = rgb[cfa.color_at(i / 37, i % 37)];
        }
        let before = original.data.clone();
        let mut prepared = model
            .reconstruct_image(&original, false, 1024, |_, _| {})
            .unwrap();
        assert_eq!(
            prepared.clipping_mask([0.5; 2], &Default::default()),
            if clipped { 2 } else { 0 }
        );
        assert_eq!(original.data, before);
        // Probe arbitrary learned RGB independently of its original clipping.
        // The model and RGB numerics are checked by the other real-graph tests.
        let value = if clipped {
            [0.6, 0.995, 0.8]
        } else {
            [1.2, -0.1, 2.4]
        };
        for pixel in prepared.data[..37 * 41 * 3].as_chunks_mut::<3>().0 {
            pixel.copy_from_slice(&value);
        }
        let prepared = std::sync::Arc::new(prepared);
        let mut edits = Edits::for_image(&prepared);
        edits.raw.reconstruction = Reconstruction::RawNindV1;
        edits.tone.mapper = ToneMapper::Linear;
        let recovered = Pipeline::compile(&prepared, &edits)
            .unwrap()
            .sample([0.5; 2]);
        #[cfg(feature = "gpu")]
        if std::env::var("RAWPUPPY_TEST_RAWNIND_GPU").as_deref() == Ok("1") {
            let backend = if std::env::var("RAWPUPPY_TEST_CUDA").is_ok() {
                rawpuppy::render::Backend::Cuda
            } else {
                rawpuppy::render::Backend::Vulkan
            };
            let expected = Pipeline::compile(&prepared, &edits)
                .unwrap()
                .render(None)
                .unwrap();
            let mut gpu = rawpuppy::gpu::GpuRenderer::new(backend).unwrap();
            let actual = gpu.render(prepared.clone(), &edits, None).unwrap();
            for (a, b) in actual.pixels.iter().zip(&expected.pixels) {
                assert!(a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.0003));
            }
        }
        if clipped {
            assert!(recovered[..3].iter().all(|v| (*v - 1.2).abs() < 2e-6));
        } else {
            edits.raw.recover_highlights = false;
            assert_eq!(
                recovered,
                Pipeline::compile(&prepared, &edits)
                    .unwrap()
                    .sample([0.5; 2])
            );
        }
    }
}

#[test]
#[ignore = "requires the verified external RawNIND graph"]
fn selected_native_device_agrees_with_cpu_on_signed_and_above_white_input() {
    let directory = std::env::var_os("RAWPUPPY_TEST_RAWNIND_GRAPH")
        .expect("Set RAWPUPPY_TEST_RAWNIND_GRAPH to the verified prepared graph directory");
    let cpu = BayerModel::open(std::path::Path::new(&directory), InferenceDevice::Cpu).unwrap();
    let selected = model();
    eprintln!(
        "selected native RAW inference device: {:?}",
        selected.device()
    );
    if std::env::var("RAWPUPPY_TEST_REQUIRE_MPS").as_deref() == Ok("1") {
        assert_eq!(
            selected.device(),
            tch::Device::Mps,
            "MPS must execute this check"
        );
    }
    let mut maximum = 0f32;
    for pattern in ["RGGB", "BGGR", "GRBG", "GBRG"] {
        let mut source = sensor(109, 111, pattern);
        let cfa = source.cfa.as_ref().unwrap();
        for y in 0..111 {
            for x in 0..109 {
                let rgb = [
                    -0.08 + 0.6 * x as f32 / 108.,
                    0.1 + 0.4 * y as f32 / 110.,
                    1.2 + 0.4 * ((x + 3 * y) as f32 * 0.07).sin(),
                ];
                source.data[y * 109 + x] = rgb[cfa.color_at(y, x)];
            }
        }
        assert!(source.data.iter().any(|v| *v < 0.));
        assert!(source.data.iter().any(|v| *v > 1.));
        let original = source.data.clone();
        let expected = cpu.reconstruct_patch(&source, [17, 19], [31, 33]).unwrap();
        let actual = selected
            .reconstruct_patch(&source, [17, 19], [31, 33])
            .unwrap();
        assert_eq!(actual.size, expected.size);
        assert_eq!(actual.origin, expected.origin);
        assert_eq!(source.data, original);
        for (a, b) in actual.pixels.iter().zip(&expected.pixels) {
            for channel in 0..3 {
                assert!(a[channel].is_finite());
                maximum = maximum.max((a[channel] - b[channel]).abs());
            }
        }
    }
    eprintln!("maximum camera-linear CPU/device difference: {maximum}");
    assert!(maximum < 3e-5, "Native device diverged from CPU: {maximum}");
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

#[test]
#[ignore = "requires the verified external RawNIND graph"]
fn joint_preparation_cancels_at_a_tile_boundary_without_altering_the_source() {
    let model = model();
    let source = sensor(96, 98, "RGGB");
    let original = source.data.clone();
    let mut completed = 0;
    let error = model
        .reconstruct_image_controlled(&source, false, 32, |done, _| {
            completed = done;
            done < 1
        })
        .err()
        .expect("Cancelled job must not return a partial image");
    assert!(error.to_string().contains("cancelled"));
    assert_eq!(completed, 1);
    assert_eq!(source.data, original);
}

#[test]
#[ignore = "requires the verified RawNIND graph installed in the standard model cache"]
fn preview_preparation_is_nonblocking_and_never_commits_superseded_results() {
    use rawpuppy::{
        edits::{Edits, Reconstruction},
        render::{Backend, Renderer},
    };
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    let source = Arc::new(sensor(192, 194, "GBRG"));
    let mut edits = Edits::default();
    edits.raw.reconstruction = Reconstruction::RawNindV1;
    let mut renderer = Renderer::new(Backend::Cpu);
    let (_, preparing) = renderer
        .render_preview_region(source.clone(), &edits, [0., 0., 1., 1.], 32, 32)
        .unwrap();
    assert!(preparing.is_some());
    assert!(renderer.reconstruction_pending());
    let replacement = Arc::new(sensor(96, 98, "BGGR"));
    let standard = Edits::default();
    let (_, preparing) = renderer
        .render_preview_region(replacement.clone(), &standard, [0., 0., 1., 1.], 32, 32)
        .unwrap();
    assert!(preparing.is_none());
    assert!(!renderer.reconstruction_pending());
    let start = Instant::now();
    loop {
        let (_, preparing) = renderer
            .render_preview_region(replacement.clone(), &edits, [0., 0., 1., 1.], 32, 32)
            .unwrap();
        if preparing.is_none() {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(10));
    }
    let expected = renderer
        .render(replacement.clone(), &edits, Some(32))
        .unwrap();
    let (actual, progress) = renderer
        .render_preview_region(
            replacement,
            &edits,
            [0., 0., 1., 1.],
            expected.width,
            expected.height,
        )
        .unwrap();
    assert!(progress.is_none());
    assert_eq!(actual.pixels, expected.pixels);
}
