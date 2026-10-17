use rawpuppy::{
    edits::{DetailEdits, Edits},
    input::SensorImage,
    pipeline::Pipeline,
    render::{Backend, Renderer},
    sidecar, synthesis,
};
use std::sync::Arc;

#[test]
fn detail_roundtrips_without_invalidating_saved_fills_or_changing_legacy_recipes() {
    let mut edits = Edits::default();
    let json = serde_json::to_string(&edits).unwrap();
    assert!(!json.contains("\"detail\""));
    let hash = synthesis::recipe_hash(&edits).unwrap();
    edits.detail = DetailEdits {
        sharpen: 0.2,
        local_contrast: 0.25,
    };
    assert_eq!(synthesis::recipe_hash(&edits).unwrap(), hash);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.rawpuppy.xmp");
    sidecar::save(&path, &edits).unwrap();
    assert_eq!(sidecar::load(&path).unwrap(), edits);
    for value in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
        edits.detail.sharpen = value;
        assert!(edits.validate().is_err());
    }
    let image = SensorImage::from_rgb(16, 16, vec![0.2; 16 * 16 * 3]).unwrap();
    assert_eq!(Edits::for_image(&image).detail.sharpen, 0.2);
}

fn source() -> Arc<SensorImage> {
    Arc::new(
        SensorImage::from_rgb(
            157,
            93,
            (0..157 * 93)
                .flat_map(|i| {
                    let x = (i % 157) as f32;
                    let y = (i / 157) as f32;
                    let l = 0.25 + 0.04 * (x * 0.5).sin() + 0.02 * (y * 0.9).sin();
                    [l, l * 0.8, l * 0.6]
                })
                .collect(),
        )
        .unwrap(),
    )
}

#[test]
fn final_detail_matches_pipeline_and_streamed_export_strips() {
    let image = source();
    let mut edits = Edits {
        detail: DetailEdits {
            sharpen: 0.2,
            local_contrast: 0.35,
        },
        ..Default::default()
    };
    edits.geometry.rotation = 0.3;
    let expected = Pipeline::compile(&image, &edits)
        .unwrap()
        .render(None)
        .unwrap();
    let mut renderer = Renderer::new(Backend::Cpu);
    for (y, height) in [(0, 31), (31, 62)] {
        let strip = renderer
            .render_region(
                image.clone(),
                &edits,
                [0., y as f32 / 93., 1., height as f32 / 93.],
                157,
                height,
            )
            .unwrap();
        for (a, b) in strip.pixels.iter().zip(&expected.pixels[y * 157..]) {
            for c in 0..4 {
                assert!((a[c] - b[c]).abs() < 5e-6, "{a:?} vs {b:?}");
            }
        }
    }
}

#[test]
fn generated_pixels_receive_the_same_final_detail_as_the_photo() {
    use rawpuppy::{
        edits::ToneMapper,
        pipeline::Rendered,
        synthesis::{GeneratedFill, Layers, MaskDab},
    };
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("photo.raw");
    std::fs::write(&original, b"immutable original").unwrap();
    let image = source();
    let mut layers = Layers::new(original.clone());
    let (asset, sha256) = layers
        .store(&Rendered {
            width: 157,
            height: 93,
            pixels: image
                .data
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| [p[0], p[1], p[2], 1.])
                .collect(),
        })
        .unwrap();
    let mut edits = Edits::default();
    edits.tone.mapper = ToneMapper::Linear;
    edits.display.synthesis.push(GeneratedFill {
        region: [0., 0., 1., 1.],
        dabs: vec![MaskDab {
            center: [0.5; 2],
            radius: 1.,
        }],
        fill_gaps: false,
        feather: 0.,
        harmonization: Default::default(),
        steps: 10,
        seed: 0,
        sampling: Default::default(),
        asset,
        sha256,
        source_sha256: layers.source_hash().unwrap().into(),
        source_color_revision: 0,
        recipe_sha256: synthesis::recipe_hash(&edits).unwrap(),
        model: "moebius-scene-2026-v1".into(),
    });
    let mut renderer = Renderer::new(Backend::Cpu);
    renderer.set_document(original.clone());
    let before = renderer.render(image.clone(), &edits, None).unwrap();
    edits.detail = DetailEdits {
        sharpen: 0.2,
        local_contrast: 0.4,
    };
    let actual = renderer.render(image.clone(), &edits, None).unwrap();
    assert!(
        actual
            .pixels
            .iter()
            .zip(&before.pixels)
            .any(|(a, b)| (a[0] - b[0]).abs() > 0.001)
    );
    edits.display.synthesis.clear();
    let photographic = Pipeline::compile(&image, &edits)
        .unwrap()
        .render(None)
        .unwrap();
    for (a, b) in actual.pixels.iter().zip(&photographic.pixels) {
        for c in 0..4 {
            assert!((a[c] - b[c]).abs() < 3e-6, "{a:?} vs {b:?}");
        }
    }
    assert_eq!(std::fs::read(original).unwrap(), b"immutable original");
}

#[cfg(feature = "gpu")]
#[test]
#[ignore = "requires a Vulkan/Metal/CUDA compute device"]
fn spatial_detail_matches_cpu_after_native_gpu_composition() {
    let image = source();
    let backend = if std::env::var_os("RAWPUPPY_TEST_CUDA").is_some() {
        Backend::Cuda
    } else {
        Backend::Vulkan
    };
    let mut edits = Edits {
        detail: DetailEdits {
            sharpen: 0.2,
            local_contrast: 0.4,
        },
        ..Default::default()
    };
    edits.geometry.rotation = 0.6;
    edits.geometry.pitch = -4.;
    let cpu = Pipeline::compile(&image, &edits)
        .unwrap()
        .render(None)
        .unwrap();
    let mut renderer = Renderer::new(backend);
    let actual = renderer.render(image.clone(), &edits, None).unwrap();
    let mut gpu = rawpuppy::gpu::GpuRenderer::new(backend).unwrap();
    let direct = gpu.render(image, &edits, None).unwrap();
    for output in [actual, direct] {
        for (a, b) in output.pixels.iter().zip(&cpu.pixels) {
            for c in 0..4 {
                assert!((a[c] - b[c]).abs() < 0.00003, "{backend:?}: {a:?} vs {b:?}");
            }
        }
    }
}

#[test]
fn a_valid_subpixel_crop_remains_a_one_pixel_image_with_detail_enabled() {
    let image = source();
    let mut edits = Edits::for_image(&image);
    edits.geometry.crop = [0.3, 0.3, 0.001, 0.001];
    let actual = Renderer::new(Backend::Cpu)
        .render(image, &edits, None)
        .unwrap();
    assert_eq!([actual.width, actual.height], [1, 1]);
    assert!(actual.pixels[0].iter().all(|v| v.is_finite()));
}
