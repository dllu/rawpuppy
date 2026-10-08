#![cfg(feature = "gpu")]
use rawpuppy::{
    edits::{Edits, Retouch, RetouchMode},
    gpu::{Backend, GpuRenderer},
    input::SensorImage,
    pipeline::Pipeline,
};
use std::sync::Arc;

#[test]
#[ignore = "requires a working Vulkan/Metal/CUDA compute device"]
fn composed_gpu_matches_cpu_for_rgb_bayer_orientation_and_local_edits() {
    let backend = if std::env::var("RAWPUPPY_TEST_CUDA").is_ok() {
        Backend::Cuda
    } else {
        Backend::Auto
    };
    let mut gpu = GpuRenderer::new(backend).unwrap();
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
            let image = Arc::new(image);
            let mut edits = Edits::default();
            edits.raw.hot_pixels = true;
            edits.raw.denoise = 0.012;
            edits.geometry.pitch = 4.;
            edits.geometry.yaw = -5.;
            edits.geometry.rotation = 6.;
            edits.geometry.distortion = [-0.05, 0.02];
            edits.geometry.chromatic_aberration = [0.008, -0.006];
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
