use rawpuppy::{
    edits::Edits,
    pipeline::Rendered,
    synthesis::{self, GeneratedFill, Layers, MaskDab},
};

#[test]
fn generated_layer_roundtrips_and_preserves_every_unpainted_pixel() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("photo.raw");
    std::fs::write(&original, b"immutable original").unwrap();
    let mut layers = Layers::new(original.clone());
    let mut generated = Rendered {
        width: 16,
        height: 16,
        pixels: vec![[0.7, 0.2, 0.1, 1.]; 256],
    };
    generated.pixels[0] = [10., 5., 3., 0.];
    let (asset, hash) = layers.store(&generated).unwrap();
    let mut edits = Edits::default();
    let fill = GeneratedFill {
        region: [0., 0., 1., 1.],
        dabs: vec![MaskDab {
            center: [0.5, 0.5],
            radius: 0.2,
        }],
        fill_gaps: false,
        steps: 10,
        seed: 0,
        asset,
        sha256: hash,
        source_sha256: layers.source_hash().unwrap().into(),
        source_color_revision: 0,
        recipe_sha256: synthesis::recipe_hash(&edits).unwrap(),
        model: "moebius-scene-2026-v1".into(),
    };
    edits.display.synthesis.push(fill);
    let sidecar = rawpuppy::sidecar::path_for(&original);
    rawpuppy::sidecar::save(&sidecar, &edits).unwrap();
    let loaded = rawpuppy::sidecar::load(&sidecar).unwrap();
    assert_eq!(loaded, edits);
    let mut raster = Rendered {
        width: 16,
        height: 16,
        pixels: vec![[0.18; 4]; 256],
    };
    let mut reloaded = Layers::new(original.clone());
    reloaded
        .apply(&loaded, &mut raster, [0., 0., 1., 1.])
        .unwrap();
    assert!((raster.pixels[8 * 16 + 8][0] - 0.7).abs() < 1e-6);
    for y in 0..16 {
        for x in 0..16 {
            let dx = (x as f32 + 0.5) / 16. - 0.5;
            let dy = (y as f32 + 0.5) / 16. - 0.5;
            if dx * dx + dy * dy > 0.04 {
                assert_eq!(raster.pixels[y * 16 + x], [0.18; 4]);
            }
        }
    }
    assert_eq!(std::fs::read(original).unwrap(), b"immutable original");
}

#[test]
fn stale_or_corrupt_synthesis_is_rejected_without_changing_output() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("photo.raw");
    std::fs::write(&original, b"source").unwrap();
    let mut layers = Layers::new(original.clone());
    let layer = Rendered {
        width: 1,
        height: 1,
        pixels: vec![[1.; 4]],
    };
    let (asset, hash) = layers.store(&layer).unwrap();
    let mut edits = Edits::default();
    edits.display.synthesis.push(GeneratedFill {
        region: [0., 0., 1., 1.],
        dabs: vec![MaskDab {
            center: [0.5; 2],
            radius: 1.,
        }],
        fill_gaps: false,
        steps: 10,
        seed: 0,
        asset: asset.clone(),
        sha256: hash,
        source_sha256: layers.source_hash().unwrap().into(),
        source_color_revision: 0,
        recipe_sha256: synthesis::recipe_hash(&edits).unwrap(),
        model: "moebius-scene-2026-v1".into(),
    });
    let mut raster = Rendered {
        width: 1,
        height: 1,
        pixels: vec![[0.18; 4]],
    };
    edits.scene.exposure = 1.;
    assert!(layers.apply(&edits, &mut raster, [0., 0., 1., 1.]).is_err());
    assert_eq!(raster.pixels[0], [0.18; 4]);
    edits.scene.exposure = 0.;
    std::fs::write(
        synthesis::asset_directory(&original).join(asset),
        b"corrupt",
    )
    .unwrap();
    let mut reload = Layers::new(original);
    assert!(reload.apply(&edits, &mut raster, [0., 0., 1., 1.]).is_err());
    assert_eq!(raster.pixels[0], [0.18; 4]);
}

#[test]
fn context_shape_remains_square_on_portrait_and_extremely_wide_images() {
    for (w, h) in [(8736, 11648), (100_003, 17)] {
        let region = synthesis::brush_context(
            &[MaskDab {
                center: [0.25, 0.75],
                radius: 0.03,
            }],
            w,
            h,
        )
        .unwrap();
        assert!((region[2] * w as f32 - region[3] * h as f32).abs() < 0.002);
    }
}

#[test]
fn corner_fill_uses_output_resolution_membership_and_preserves_opaque_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("photo.raw");
    std::fs::write(&original, b"source").unwrap();
    let mut layers = Layers::new(original);
    // Coarse inference context is opaque; the output has a gap narrower than
    // one context texel. Filtering the coarse inference mask left alpha slivers.
    let context = Rendered {
        width: 2,
        height: 2,
        pixels: vec![[0.7, 0.2, 0.1, 1.]; 4],
    };
    let (asset, hash) = layers.store(&context).unwrap();
    let mut edits = Edits::default();
    edits.display.synthesis.push(GeneratedFill {
        region: [0., 0., 1., 1.],
        dabs: vec![],
        fill_gaps: true,
        steps: 10,
        seed: 0,
        asset,
        sha256: hash,
        source_sha256: layers.source_hash().unwrap().into(),
        source_color_revision: 0,
        recipe_sha256: synthesis::recipe_hash(&edits).unwrap(),
        model: "moebius-scene-2026-v1".into(),
    });
    let mut raster = Rendered {
        width: 101,
        height: 101,
        pixels: vec![[0.18, 0.18, 0.18, 1.]; 101 * 101],
    };
    raster.pixels[50 * 101 + 50] = [0.; 4];
    layers.apply(&edits, &mut raster, [0., 0., 1., 1.]).unwrap();
    for (i, pixel) in raster.pixels.iter().enumerate() {
        if i == 50 * 101 + 50 {
            assert_eq!(*pixel, [0.7, 0.2, 0.1, 1.]);
        } else {
            assert_eq!(*pixel, [0.18, 0.18, 0.18, 1.]);
        }
    }
}

#[test]
fn corner_context_keeps_square_shape_and_available_photographic_context() {
    let mut probe = Rendered {
        width: 128,
        height: 128,
        pixels: vec![[1.; 4]; 128 * 128],
    };
    for y in 0..12 {
        for x in 0..48 {
            probe.pixels[y * 128 + x][3] = 0.;
        }
    }
    let regions = synthesis::gap_contexts(&probe, 8736, 11648);
    assert_eq!(regions.len(), 1);
    let region = regions[0];
    assert_eq!(region[..2], [0., 0.]);
    assert!((region[2] * 8736. - region[3] * 11648.).abs() < 0.002);
    assert!(region[2] >= 48. / 128. && region[3] >= 12. / 128.);
    assert!(region[0] + region[2] <= 1. && region[1] + region[3] <= 1.);
}
