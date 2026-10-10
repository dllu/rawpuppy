#![cfg(feature = "gpu")]
use rawpuppy::{
    edits::{Edits, Retouch, RetouchMode},
    gpu::{Backend, CudaMemoryMode, GpuRenderer},
    input::SensorImage,
    pipeline::Pipeline,
};
use std::sync::Arc;

#[test]
#[ignore = "requires a working Vulkan/Metal/CUDA compute device"]
fn imported_float_hdr_keeps_signed_values_through_gpu_and_exr_export() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hdr64.tiff");
    let (width, height) = (19u32, 23u32);
    let values: Vec<_> = (0..width * height)
        .flat_map(|i| {
            let x = f64::from(i % width) / f64::from(width);
            let y = f64::from(i / width) / f64::from(height);
            [x * 3. - 0.5, y * 2. + 0.125, (x + y) * 4.]
        })
        .collect();
    let mut encoder =
        tiff::encoder::TiffEncoder::new(std::fs::File::create(&path).unwrap()).unwrap();
    let mut image = encoder
        .new_image::<tiff::encoder::colortype::RGB64Float>(width, height)
        .unwrap();
    image
        .encoder()
        .write_tag(tiff::tags::Tag::Orientation, 6u16)
        .unwrap();
    image.write_data(&values).unwrap();
    drop(encoder);
    let original_file = std::fs::read(&path).unwrap();
    let source = SensorImage::open(&path).unwrap();
    assert_eq!(source.orientation, rawler::Orientation::Rotate90);
    assert_eq!(
        (source.metadata.width, source.metadata.height),
        (height as usize, width as usize)
    );
    let source = Arc::new(source);
    let original_data = source.data.clone();
    let mut edits = Edits::for_image(&source);
    edits.tone.mapper = rawpuppy::edits::ToneMapper::Linear;
    edits.scene.exposure = 1.;
    let expected = Pipeline::compile(&source, &edits)
        .unwrap()
        .render(None)
        .unwrap();
    let backend = if std::env::var_os("RAWPUPPY_TEST_CUDA").is_some() {
        Backend::Cuda
    } else {
        Backend::Auto
    };
    let mut gpu = GpuRenderer::new(backend).unwrap();
    let actual = gpu.render(source.clone(), &edits, None).unwrap();
    eprintln!(
        "HDR TIFF → {} → EXR: {} × {}",
        gpu.name(),
        actual.width,
        actual.height
    );
    assert_eq!(
        (actual.width, actual.height),
        (expected.width, expected.height)
    );
    assert!(actual.pixels.iter().any(|p| p[0] < 0.));
    assert!(actual.pixels.iter().any(|p| p[2] > 4.));
    for (a, b) in actual.pixels.iter().zip(&expected.pixels) {
        assert!(a.iter().zip(b).all(|(a, b)| (*a - b).abs() < 0.00001));
        assert_eq!(a[3], 1.);
    }
    let exported = directory.path().join("gpu.exr");
    rawpuppy::export::write(
        &exported,
        &actual,
        rawpuppy::color::OutputSpace::LinearSrgb,
        false,
    )
    .unwrap();
    let reloaded = SensorImage::open(&exported).unwrap();
    for (a, b) in reloaded
        .data
        .as_chunks::<3>()
        .0
        .iter()
        .zip(&expected.pixels)
    {
        assert!(a.iter().zip(&b[..3]).all(|(a, b)| (*a - b).abs() < 0.00001));
    }
    assert_eq!(source.data, original_data);
    assert_eq!(std::fs::read(path).unwrap(), original_file);
}

#[cfg(feature = "cuda")]
#[test]
#[ignore = "requires a working CUDA device"]
fn automatic_selection_prefers_working_cuda_over_native_gpu() {
    let mut gpu = GpuRenderer::new(Backend::Auto).unwrap();
    assert!(
        gpu.name().starts_with("cuda"),
        "Working CUDA was bypassed: {}",
        gpu.name()
    );
    let source = Arc::new(SensorImage::from_rgb(17, 13, vec![0.2; 17 * 13 * 3]).unwrap());
    let mut edits = Edits::default();
    edits.tone.mapper = rawpuppy::edits::ToneMapper::Linear;
    let output = gpu.render(source.clone(), &edits, None).unwrap();
    assert!(
        output
            .pixels
            .iter()
            .all(|p| (p[0] - 0.2).abs() < 1e-6 && p[3] == 1.)
    );
    assert!(source.data.iter().all(|v| *v == 0.2));
}

#[test]
#[ignore = "requires a working Vulkan/Metal/CUDA compute device"]
fn sensor_highlight_recovery_matches_cpu_across_orientations() {
    let backend = if std::env::var("RAWPUPPY_TEST_CUDA").is_ok() {
        Backend::Cuda
    } else {
        Backend::Auto
    };
    let mut gpu = GpuRenderer::new(backend).unwrap();
    for orientation in [1, 6, 8] {
        let mut source = SensorImage::from_rgb(37, 41, vec![0.; 37 * 41 * 3]).unwrap();
        let cfa = rawler::CFA::new("RGGB");
        source.data = (0..37 * 41)
            .map(|i| {
                let x = i % 37;
                let rgb = if x < 12 {
                    [0.2, 0.3, 0.4]
                } else if x < 26 {
                    [0.6, 0.995, 0.8]
                } else {
                    [1.; 3]
                };
                rgb[cfa.color_at(i / 37, x)]
            })
            .collect();
        source.cpp = 1;
        source.cfa = Some(cfa);
        source.raw_integer = true;
        source.metadata.as_shot = [2., 1., 1.5];
        source.orientation = rawler::Orientation::from_u16(orientation);
        if orientation != 1 {
            source.metadata.width = 41;
            source.metadata.height = 37;
        }
        let source = Arc::new(source);
        let original = source.data.clone();
        let mut edits = Edits::for_image(&source);
        edits.geometry.chromatic_aberration = [0.001, -0.001];
        for mapper in [
            rawpuppy::edits::ToneMapper::Linear,
            rawpuppy::edits::ToneMapper::AgxSdr,
        ] {
            edits.tone.mapper = mapper;
            let expected = Pipeline::compile(&source, &edits)
                .unwrap()
                .render(None)
                .unwrap();
            let actual = gpu.render(source.clone(), &edits, None).unwrap();
            let max = actual
                .pixels
                .iter()
                .zip(&expected.pixels)
                .flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| (a - b).abs()))
                .fold(0f32, f32::max);
            if max >= 0.0003 {
                let (i, (a, b)) = actual
                    .pixels
                    .iter()
                    .zip(&expected.pixels)
                    .enumerate()
                    .max_by(|(_, (a, b)), (_, (c, e))| {
                        let ab = a
                            .iter()
                            .zip(b.iter())
                            .map(|(x, y)| (x - y).abs())
                            .fold(0f32, f32::max);
                        let ce = c
                            .iter()
                            .zip(e.iter())
                            .map(|(x, y)| (x - y).abs())
                            .fold(0f32, f32::max);
                        ab.total_cmp(&ce)
                    })
                    .unwrap();
                eprintln!(
                    "orientation={orientation}, mapper={mapper:?}, pixel={i}: GPU {a:?}, CPU {b:?}"
                );
            }
            assert!(max < 0.0003, "Recovery CPU/GPU mismatch: {max}");
        }
        assert_eq!(source.data, original);
    }
}

#[test]
#[ignore = "requires a working native Vulkan or Metal compute device"]
fn repeated_native_renderers_share_the_device_and_retire_their_own_sources() {
    let backend = if cfg!(target_os = "macos") {
        Backend::Metal
    } else {
        Backend::Vulkan
    };
    let a = Arc::new(SensorImage::from_rgb(13, 11, vec![0.2; 13 * 11 * 3]).unwrap());
    let b = Arc::new(SensorImage::from_rgb(17, 9, vec![0.7; 17 * 9 * 3]).unwrap());
    let retired = Arc::downgrade(&a);
    let mut edits = Edits::default();
    edits.tone.mapper = rawpuppy::edits::ToneMapper::Linear;
    // Auto initializes the same native wgpu runtime in a non-CUDA build.
    let mut automatic = GpuRenderer::new(Backend::Auto).unwrap();
    let mut first = GpuRenderer::new(backend).unwrap();
    let mut second = GpuRenderer::new(backend).unwrap();
    for (renderer, source, value) in [
        (&mut automatic, b.clone(), 0.7),
        (&mut first, a.clone(), 0.2),
        (&mut second, b.clone(), 0.7),
    ] {
        let output = renderer.render(source, &edits, None).unwrap();
        assert!(
            output
                .pixels
                .iter()
                .all(|p| p[..3].iter().all(|v| (*v - value).abs() < 1e-6) && p[3] == 1.)
        );
    }
    drop(a);
    assert!(retired.upgrade().is_some());
    drop(first);
    assert!(retired.upgrade().is_none());
    let conflicting = if backend == Backend::Metal {
        Backend::Vulkan
    } else {
        Backend::Metal
    };
    assert!(GpuRenderer::new(conflicting).is_err());
    assert!(
        second
            .render(b.clone(), &edits, None)
            .unwrap()
            .pixels
            .iter()
            .all(|p| (p[0] - 0.7).abs() < 1e-6)
    );
    assert!(b.data.iter().all(|v| *v == 0.7));
}

#[test]
#[ignore = "requires a working Vulkan/Metal/CUDA compute device"]
fn composed_gpu_matches_cpu_for_rgb_bayer_orientation_and_local_edits() {
    let backend = if std::env::var("RAWPUPPY_TEST_CUDA").is_ok() {
        Backend::Cuda
    } else {
        Backend::Auto
    };
    check_backend(backend, CudaMemoryMode::Auto);
}

#[cfg(feature = "cuda")]
#[test]
#[ignore = "requires a working CUDA device"]
fn copied_cuda_matches_cpu_for_the_same_composed_cases() {
    check_backend(Backend::Cuda, CudaMemoryMode::Copy);
}

#[cfg(feature = "cuda")]
#[test]
#[ignore = "requires coherent integrated CUDA system memory"]
fn system_memory_rebuilds_cleanup_and_releases_retired_sources() {
    let mut gpu = GpuRenderer::with_cuda_memory(Backend::Cuda, CudaMemoryMode::System).unwrap();
    let mut image = SensorImage::from_rgb(37, 41, vec![0.1; 37 * 41 * 3]).unwrap();
    image.cpp = 1;
    image.cfa = Some(rawler::CFA::new("RGGB"));
    image.data = (0..37 * 41)
        .map(|i| 0.1 + (i % 31) as f32 * 0.0003)
        .collect();
    image.data[400] = 0.95;
    let image = Arc::new(image);
    let original = image.data.clone();
    let retired = Arc::downgrade(&image);
    for (hot_pixels, denoise) in [
        (false, 0.),
        (true, 0.),
        (true, 0.01),
        (false, 0.),
        (true, 0.),
    ] {
        let mut edits = Edits::default();
        edits.raw.hot_pixels = hot_pixels;
        edits.raw.denoise = denoise;
        edits.tone.mapper = rawpuppy::edits::ToneMapper::Linear;
        let expected = Pipeline::compile(&image, &edits)
            .unwrap()
            .render(None)
            .unwrap();
        let actual = gpu.render(image.clone(), &edits, None).unwrap();
        for (a, b) in actual.pixels.iter().zip(expected.pixels) {
            assert!(a.iter().zip(b).all(|(a, b)| (*a - b).abs() < 0.0003));
        }
        assert_eq!(image.data, original);
    }
    drop(image);
    assert!(
        retired.upgrade().is_some(),
        "The session must retain its current immutable source"
    );
    let replacement = Arc::new(SensorImage::from_rgb(3, 7, vec![0.2; 3 * 7 * 3]).unwrap());
    gpu.render(replacement, &Edits::default(), None).unwrap();
    assert!(
        retired.upgrade().is_none(),
        "Retired input remained allocated after replacement"
    );
}

fn check_backend(backend: Backend, memory: CudaMemoryMode) {
    let mut gpu = GpuRenderer::with_cuda_memory(backend, memory).unwrap();
    for mosaic in [false, true] {
        for orientation in 1..=8 {
            let (width, height) = (19, 23);
            let data: Vec<f32> = (0..width * height)
                .flat_map(|i| {
                    let x = (i % width) as f32 / width as f32;
                    let y = (i / width) as f32 / height as f32;
                    [x * 0.6 + 0.01, y * 0.8 + 0.02, (x + y) * 0.5 + 0.03]
                })
                .collect();
            let mut image = SensorImage::from_rgb(width, height, data).unwrap();
            if mosaic {
                let pattern = ["RGGB", "BGGR", "GRBG", "GBRG"][(orientation - 1) % 4];
                let cfa = rawler::CFA::new(pattern);
                image.data = image
                    .data
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .enumerate()
                    .map(|(i, p)| p[cfa.color_at(i / width, i % width)])
                    .collect();
                image.cpp = 1;
                image.cfa = Some(cfa);
            }
            image.orientation = rawler::Orientation::from_u16(orientation as u16);
            if image.orientation.to_flips().0 {
                image.metadata.width = height;
                image.metadata.height = width;
            }
            if orientation % 2 == 0 {
                let radius_pixels = (width as f32).hypot(height as f32) * 0.5;
                image.metadata.lens_profile = Some(rawpuppy::lens::LensProfile {
                    distortion: Some(rawpuppy::lens::RadialTable {
                        radius_pixels,
                        knots: vec![[0.4, -2.], [1.1, -5.]],
                    }),
                    vignette: Some(rawpuppy::lens::RadialTable {
                        radius_pixels,
                        knots: vec![[0.4, 80.], [1.1, 55.]],
                    }),
                    red_ca: Some(rawpuppy::lens::RadialTable {
                        radius_pixels,
                        knots: vec![[0.4, 0.01], [1.1, -0.02]],
                    }),
                    blue_ca: Some(rawpuppy::lens::RadialTable {
                        radius_pixels,
                        knots: vec![[0.4, -0.015], [1.1, 0.025]],
                    }),
                });
                if orientation % 4 == 0 {
                    let profile = image.metadata.lens_profile.as_mut().unwrap();
                    profile.distortion = None;
                    profile.vignette = None;
                }
            }
            let image = Arc::new(image);
            let original = image.data.clone();
            let mut edits = Edits::for_image(&image);
            if orientation == 6 {
                // Historical recipes still use the common camera map without CA.
                edits.lens.chromatic_aberration = false;
            }
            edits.raw.hot_pixels = true;
            edits.raw.denoise = 0.012;
            edits.geometry.pitch = 4.;
            edits.geometry.yaw = -5.;
            edits.geometry.rotation = 6.;
            edits.geometry.distortion = [-0.05, 0.02];
            edits.geometry.chromatic_aberration = if orientation % 4 == 0 {
                [0.; 2]
            } else {
                [0.008, -0.006]
            };
            edits.geometry.crop = [0.05, 0.02, 0.9, 0.95];
            edits.scene.exposure = 0.8;
            edits.scene.calibration = [1.2, 0.9, 1.1];
            edits.scene.vignette = [0.3, -0.2];
            edits.scene.graduated.exposure = -0.5;
            edits.scene.graduated.angle = 25.;
            edits.display.curve = vec![[0., 0.], [0.2, 0.1], [0.8, 0.9], [1., 1.]];
            edits.display.split_strength = 0.2;
            edits.display.shadows = [0.5, 0.7, 1.];
            edits.display.highlights = [1., 0.9, 0.7];
            for (i, mode) in [RetouchMode::Clone, RetouchMode::Heal]
                .into_iter()
                .enumerate()
            {
                edits.display.retouch.push(Retouch {
                    source: [0.3, 0.4],
                    target: [0.6, 0.6 + i as f32 * 0.1],
                    radius: 0.1,
                    feather: 0.4,
                    opacity: 0.8,
                    mode,
                });
            }
            for tone in [
                rawpuppy::edits::ToneMapper::AgxSdr,
                rawpuppy::edits::ToneMapper::Agx,
                rawpuppy::edits::ToneMapper::Linear,
            ] {
                edits.tone.mapper = tone;
                let reference = Pipeline::compile(&image, &edits)
                    .unwrap()
                    .render(None)
                    .unwrap();
                let actual = gpu.render(image.clone(), &edits, None).unwrap();
                assert_eq!(
                    (reference.width, reference.height),
                    (actual.width, actual.height)
                );
                let mut largest = 0f32;
                for (i, (a, b)) in actual.pixels.iter().zip(&reference.pixels).enumerate() {
                    for c in 0..4 {
                        assert!(a[c].is_finite(), "Nonfinite GPU output at {i}:{c}");
                        largest = largest.max((a[c] - b[c]).abs());
                    }
                }
                assert!(
                    largest < 0.0003,
                    "GPU mismatch {largest} with mosaic={mosaic}, orientation={orientation}"
                );
            }
            assert_eq!(
                image.data, original,
                "GPU rendering modified the original sensor"
            );
        }
    }
    let wide = Arc::new(SensorImage::from_rgb(100_003, 2, vec![0.18; 100_003 * 2 * 3]).unwrap());
    let mut edits = Edits::default();
    edits.tone.mapper = rawpuppy::edits::ToneMapper::Linear;
    let actual = gpu.render(wide, &edits, None).unwrap();
    assert_eq!(actual.width, 100_003);
    assert!(
        actual
            .pixels
            .iter()
            .all(|p| (p[0] - 0.18).abs() < 1e-6 && p[3] == 1.)
    );
}
