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
        feather: 0.,
        steps: 10,
        seed: 0,
        asset,
        sha256: hash,
        source_sha256: layers.source_hash().unwrap().into(),
        source_color_revision: 0,
        recipe_sha256: synthesis::recipe_hash(&edits).unwrap(),
        sampling: Default::default(),
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
fn saved_sampling_parameters_preserve_legacy_recipes_and_record_actual_settings() {
    use rawpuppy::synthesis::SamplingParameters;
    let old: Edits =
        serde_json::from_str(include_str!("data/legacy-synthesis-recipe.json")).unwrap();
    let fill = &old.display.synthesis[0];
    assert_eq!(fill.feather, 0.);
    assert!(serde_json::to_value(fill).unwrap().get("feather").is_none());
    assert_eq!(fill.sampling, SamplingParameters::default());
    assert_eq!(fill.sampling.strength, 0.99);
    assert!(
        serde_json::to_value(fill)
            .unwrap()
            .get("sampling")
            .is_none()
    );
    assert_eq!(synthesis::recipe_hash(&old).unwrap(), fill.recipe_sha256);
    let mut current = fill.clone();
    current.sampling = SamplingParameters {
        guidance: 3.,
        strength: 1.,
        noise_offset: 0.1,
    };
    let value = serde_json::to_value(&current).unwrap();
    assert_eq!(value["sampling"]["strength"], 1.);
    assert_eq!(
        serde_json::from_value::<GeneratedFill>(value).unwrap(),
        current
    );
    current.validate().unwrap();
    for feather in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
        current.feather = feather;
        assert!(current.validate().is_err());
    }
    current.feather = 0.15;
    assert_eq!(
        serde_json::to_value(&current).unwrap()["feather"].as_f64(),
        Some(f64::from(current.feather))
    );
    current.validate().unwrap();
    current.sampling.strength = 0.01;
    assert!(current.validate().is_err());
    current.sampling.strength = 1.;
    current.sampling.guidance = f64::INFINITY;
    assert!(current.validate().is_err());
    current.sampling.guidance = 2.;
    current.sampling.noise_offset = 2.;
    assert!(current.validate().is_err());
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
        feather: 0.,
        steps: 10,
        seed: 0,
        asset: asset.clone(),
        sha256: hash,
        source_sha256: layers.source_hash().unwrap().into(),
        source_color_revision: 0,
        recipe_sha256: synthesis::recipe_hash(&edits).unwrap(),
        sampling: Default::default(),
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
fn a_later_bad_asset_leaves_the_entire_raster_unchanged_and_offscreen_assets_are_culled() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("photo.raw");
    std::fs::write(&original, b"immutable").unwrap();
    let mut store = Layers::new(original.clone());
    let layer = Rendered {
        width: 2,
        height: 2,
        pixels: vec![[0.8, 0.2, 0.1, 1.]; 4],
    };
    let (asset, hash) = store.store(&layer).unwrap();
    let mut edits = Edits::default();
    let valid = GeneratedFill {
        region: [0., 0., 1., 1.],
        dabs: vec![MaskDab {
            center: [0.5; 2],
            radius: 1.,
        }],
        fill_gaps: false,
        feather: 0.,
        steps: 10,
        seed: 0,
        asset,
        sha256: hash,
        source_sha256: store.source_hash().unwrap().into(),
        source_color_revision: 0,
        recipe_sha256: synthesis::recipe_hash(&edits).unwrap(),
        sampling: Default::default(),
        model: "moebius-scene-2026-v1".into(),
    };
    let missing_hash = "0".repeat(64);
    let mut missing = valid.clone();
    missing.asset = format!("{missing_hash}.exr");
    missing.sha256 = missing_hash;
    edits.display.synthesis = vec![valid, missing];
    let baseline = vec![[0.18, 0.3, 0.5, 1.]; 16];
    let mut raster = Rendered {
        width: 4,
        height: 4,
        pixels: baseline.clone(),
    };
    let mut reload = Layers::new(original.clone());
    assert!(reload.apply(&edits, &mut raster, [0., 0., 1., 1.]).is_err());
    assert_eq!(
        raster.pixels, baseline,
        "First layer changed pixels before the later load failed"
    );
    // A corrupt but hash-matching asset reaches decode validation, not just checksum rejection.
    let bytes = b"not an EXR";
    use sha2::Digest;
    let hash = format!("{:x}", sha2::Sha256::digest(bytes));
    let asset = format!("{hash}.exr");
    std::fs::write(synthesis::asset_directory(&original).join(&asset), bytes).unwrap();
    edits.display.synthesis[1].asset = asset;
    edits.display.synthesis[1].sha256 = hash;
    assert!(reload.apply(&edits, &mut raster, [0., 0., 1., 1.]).is_err());
    assert_eq!(raster.pixels, baseline);
    edits.display.synthesis[1].region = [2., 2., 1., 1.];
    reload.apply(&edits, &mut raster, [0., 0., 1., 1.]).unwrap();
    assert!(raster.pixels.iter().all(|p| {
        p.iter()
            .zip([0.8, 0.2, 0.1, 1.])
            .all(|(a, b)| (*a - b).abs() < 1e-6)
    }));
    assert_eq!(std::fs::read(original).unwrap(), b"immutable");
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
fn empty_padding_is_model_context_but_not_a_canvas_target() {
    let region = [-1., -1., 3., 3.];
    let image = Rendered {
        width: 16,
        height: 16,
        pixels: (0..16 * 16)
            .map(|i| {
                let uv = [
                    -1. + 3. * ((i % 16) as f32 + 0.5) / 16.,
                    -1. + 3. * ((i / 16) as f32 + 0.5) / 16.,
                ];
                let alpha = if uv.iter().all(|v| (0.0..=1.0).contains(v)) {
                    1.
                } else {
                    0.
                };
                [0.18, 0.18, 0.18, alpha]
            })
            .collect(),
    };
    let model_mask = synthesis::context_mask(&image, region, &[], true, 1.);
    assert!(
        model_mask.contains(&1.),
        "Empty padding must remain unknown to the model"
    );
    let mut targets = model_mask.clone();
    synthesis::clip_mask_to_canvas(&mut targets, [16, 16], region).unwrap();
    assert!(
        targets.iter().all(|v| *v == 0.),
        "Covered photo still has targets in its empty padding"
    );
    assert!(model_mask.contains(&1.));
    // Preserve fractional selection weights inside the canvas.
    let mut fractional = vec![0.25; 64];
    synthesis::clip_mask_to_canvas(&mut fractional, [8, 8], [0., 0., 2., 2.]).unwrap();
    for (i, v) in fractional.iter().enumerate() {
        assert_eq!(*v, if i % 8 < 4 && i / 8 < 4 { 0.25 } else { 0. });
    }
}

#[test]
fn resampling_an_opaque_fill_keeps_every_output_sample_exactly_opaque() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("photo.raw");
    std::fs::write(&original, b"immutable source").unwrap();
    let mut layers = Layers::new(original);
    let (asset, sha256) = layers
        .store(&Rendered {
            width: 2,
            height: 2,
            pixels: vec![[0.7, 0.2, 0.1, 1.]; 4],
        })
        .unwrap();
    let mut edits = Edits::default();
    edits.display.synthesis.push(GeneratedFill {
        region: [0., 0., 1., 1.],
        dabs: vec![],
        fill_gaps: true,
        feather: 0.,
        steps: 20,
        seed: 0,
        asset,
        sha256,
        source_sha256: layers.source_hash().unwrap().into(),
        source_color_revision: 0,
        recipe_sha256: synthesis::recipe_hash(&edits).unwrap(),
        sampling: Default::default(),
        model: "moebius-scene-2026-v1".into(),
    });
    let mut output = Rendered {
        width: 127,
        height: 113,
        pixels: vec![[0.; 4]; 127 * 113],
    };
    layers.apply(&edits, &mut output, [0., 0., 1., 1.]).unwrap();
    for pixel in output.pixels {
        assert_eq!(
            pixel[3], 1.,
            "Opaque fill became partially transparent after interpolation"
        );
        for (actual, expected) in pixel[..3].iter().zip([0.7, 0.2, 0.1]) {
            assert!((*actual - expected).abs() < 1e-6);
        }
    }
}

#[cfg(feature = "moebius")]
#[test]
fn a_completed_overlapping_corner_skips_inference_and_creates_no_asset() {
    use rawpuppy::{
        input::SensorImage,
        moebius::Sampling,
        render::{Backend, Renderer},
    };
    use std::sync::Arc;
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("photo.raw");
    std::fs::write(&original, b"immutable source").unwrap();
    let source =
        Arc::new(SensorImage::from_rgb(100_003, 17, vec![0.18; 100_003 * 17 * 3]).unwrap());
    let mut edits = Edits::default();
    edits.geometry.scale = 0.99;
    let mut renderer = Renderer::new(Backend::Cpu);
    renderer.set_document(original.clone());
    let regions = renderer.gap_contexts(source.clone(), &edits).unwrap();
    assert_eq!(regions.len(), 4);
    let mut layers = Layers::new(original.clone());
    let (asset, sha256) = layers
        .store(&Rendered {
            width: 2,
            height: 2,
            pixels: vec![[0.7, 0.2, 0.1, 1.]; 4],
        })
        .unwrap();
    edits.display.synthesis.push(GeneratedFill {
        region: regions[0],
        dabs: vec![],
        fill_gaps: true,
        feather: 0.,
        steps: 20,
        seed: 0,
        asset,
        sha256,
        source_sha256: layers.source_hash().unwrap().into(),
        source_color_revision: 0,
        recipe_sha256: synthesis::recipe_hash(&edits).unwrap(),
        sampling: Default::default(),
        model: "moebius-scene-2026-v1".into(),
    });
    // The first top-left fill also covers the bottom-left output gap, while
    // the model crop still contains unknown padding outside the narrow photo.
    let context = renderer
        .render_region(source.clone(), &edits, regions[2], 512, 512)
        .unwrap();
    let model_mask = synthesis::context_mask(&context, regions[2], &[], true, 17. / 100_003.);
    assert!(model_mask.contains(&1.));
    let before = std::fs::read_dir(synthesis::asset_directory(&original))
        .unwrap()
        .count();
    assert!(
        renderer
            .generate_fill(
                source.clone(),
                &edits,
                regions[2],
                vec![],
                true,
                &Sampling::default()
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(
        std::fs::read_dir(synthesis::asset_directory(&original))
            .unwrap()
            .count(),
        before
    );
    assert!(!rawpuppy::sidecar::path_for(&original).exists());
    assert_eq!(std::fs::read(original).unwrap(), b"immutable source");
    assert!(
        renderer
            .generate_fill(
                source,
                &edits,
                regions[2],
                vec![],
                true,
                &Sampling {
                    steps: 1,
                    ..Default::default()
                }
            )
            .is_err()
    );
}

#[test]
fn edge_brush_context_uses_available_canvas_and_covers_the_selection() {
    for (width, height) in [(11648, 8736), (8736, 11648), (100_003, 17)] {
        for center in [[0.01, 0.01], [0.99, 0.01], [0.01, 0.99], [0.99, 0.99]] {
            let dab = MaskDab {
                center,
                radius: 0.03,
            };
            let region =
                synthesis::brush_context(std::slice::from_ref(&dab), width, height).unwrap();
            for axis in 0..2 {
                if region[axis + 2] <= 1. {
                    assert!(
                        region[axis] >= 0. && region[axis] + region[axis + 2] <= 1. + 1e-6,
                        "Context wastes available canvas at {center:?}: {region:?}"
                    );
                }
            }
            assert!((region[2] * width as f32 - region[3] * height as f32).abs() < 0.002);
            // Check the physical brush boundary, clipping only at the canvas.
            // This also covers contexts taller than an extremely wide photo.
            for angle in 0..360 {
                let angle = (angle as f32).to_radians();
                let point = [
                    center[0] + dab.radius * angle.cos(),
                    center[1] + dab.radius * width as f32 / height as f32 * angle.sin(),
                ];
                if point.iter().all(|v| (0.0..=1.0).contains(v)) {
                    for axis in 0..2 {
                        assert!(point[axis] >= region[axis] - 1e-6);
                        assert!(point[axis] <= region[axis] + region[axis + 2] + 1e-6);
                    }
                }
            }
        }
    }
}

#[test]
fn regeneration_replans_brushes_and_current_gaps_after_an_aspect_change() {
    let dabs = vec![MaskDab {
        center: [0.98, 0.04],
        radius: 0.03,
    }];
    let hash = "0".repeat(64);
    let brush = GeneratedFill {
        region: synthesis::brush_context(&dabs, 4096, 2048).unwrap(),
        dabs: dabs.clone(),
        fill_gaps: false,
        feather: 0.,
        steps: 20,
        seed: 0,
        asset: format!("{hash}.exr"),
        sha256: hash.clone(),
        source_sha256: hash.clone(),
        source_color_revision: 0,
        recipe_sha256: hash,
        sampling: Default::default(),
        model: "moebius-scene-2026-v1".into(),
    };
    let mut corner = brush.clone();
    corner.region = [0., 0., 0.25, 0.5];
    corner.dabs.clear();
    corner.fill_gaps = true;
    let old = vec![corner.clone(), brush.clone(), corner];
    let snapshot = old.clone();
    // The changed geometry now has one different corner; old corner layers
    // must not duplicate the current fill or retain their obsolete region.
    let gaps = [[0.75, 0.875, 0.25, 0.125]];
    let current = synthesis::regeneration_contexts(&old, 2048, 4096, &gaps).unwrap();
    assert_eq!(current.len(), 2);
    assert!(current[0].fill_gaps);
    assert_eq!(current[0].region, gaps[0]);
    assert!(current[0].dabs.is_empty());
    assert!(!current[1].fill_gaps);
    assert_eq!(current[1].dabs, dabs);
    assert_ne!(current[1].region, brush.region);
    assert!((current[1].region[2] * 2048. - current[1].region[3] * 4096.).abs() < 0.002);
    assert_eq!(old, snapshot, "Planning mutated saved layers");
    // Cropping away every gap should still regenerate painted selections.
    let current = synthesis::regeneration_contexts(&old, 2048, 4096, &[]).unwrap();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].dabs, dabs);
    // With only corner fills left, a gap-free canvas requires no generation.
    assert!(
        synthesis::regeneration_contexts(&old[..1], 2048, 4096, &[])
            .unwrap()
            .is_empty()
    );
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
        feather: 0.,
        steps: 10,
        seed: 0,
        asset,
        sha256: hash,
        source_sha256: layers.source_hash().unwrap().into(),
        source_color_revision: 0,
        recipe_sha256: synthesis::recipe_hash(&edits).unwrap(),
        sampling: Default::default(),
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
    let regions = synthesis::gap_contexts(8736, 11648, |uv| {
        let x = ((uv[0] * probe.width as f32) as usize).min(probe.width - 1);
        let y = ((uv[1] * probe.height as f32) as usize).min(probe.height - 1);
        probe.pixels[y * probe.width + x][3]
    });
    assert_eq!(regions.len(), 1);
    let region = regions[0];
    assert_eq!(region[..2], [0., 0.]);
    assert!((region[2] * 8736. - region[3] * 11648.).abs() < 0.002);
    assert!(region[2] >= 48. / 128. && region[3] >= 12. / 128.);
    assert!(region[0] + region[2] <= 1. && region[1] + region[3] <= 1.);
}

#[test]
fn narrow_rotated_corners_are_detected_at_output_resolution() {
    let (width, height) = (11648, 8736);
    let geometry = rawpuppy::geometry::Geometry::compile(
        &rawpuppy::edits::GeometryEdits {
            rotation: 0.01,
            ..Default::default()
        },
        width,
        height,
    )
    .unwrap();
    let alpha = |uv| {
        if geometry
            .map(uv, 1)
            .is_some_and(|p| p.iter().all(|v| (0.0..=1.0).contains(v)))
        {
            1.
        } else {
            0.
        }
    };
    let probe = Rendered {
        width: 128,
        height: 128,
        pixels: (0..128 * 128)
            .map(|i| {
                [
                    0.,
                    0.,
                    0.,
                    alpha([
                        ((i % 128) as f32 + 0.5) / 128.,
                        ((i / 128) as f32 + 0.5) / 128.,
                    ]),
                ]
            })
            .collect(),
    };
    assert!(probe.pixels.iter().all(|p| p[3] == 1.));
    let mut samples = 0;
    let regions = synthesis::gap_contexts(width, height, |uv| {
        samples += 1;
        alpha(uv)
    });
    assert!(samples <= 128 * 128 + 2 * (width + height));
    assert!(!regions.is_empty(), "Coarse probe missed real output gaps");
    let mut missing = 0;
    for x in 0..width {
        for y in [0, height - 1] {
            let uv = [
                (x as f32 + 0.5) / width as f32,
                (y as f32 + 0.5) / height as f32,
            ];
            if alpha(uv) < 1. {
                missing += 1;
                assert!(regions.iter().any(|r| uv[0] >= r[0]
                    && uv[0] <= r[0] + r[2]
                    && uv[1] >= r[1]
                    && uv[1] <= r[1] + r[3]));
            }
        }
    }
    assert!(missing > 0);
    for region in regions {
        let context = Rendered {
            width: 512,
            height: 512,
            pixels: (0..512 * 512)
                .map(|i| {
                    let uv = [
                        region[0] + region[2] * ((i % 512) as f32 + 0.5) / 512.,
                        region[1] + region[3] * ((i / 512) as f32 + 0.5) / 512.,
                    ];
                    [0.18, 0.18, 0.18, alpha(uv)]
                })
                .collect(),
        };
        let mut mask =
            synthesis::context_mask(&context, region, &[], true, height as f32 / width as f32);
        assert!(
            mask.iter().all(|v| *v == 0.),
            "This case must also miss the gap at inference resolution"
        );
        synthesis::cover_boundary_gaps(&mut mask, [512, 512], region, [width, height], alpha)
            .unwrap();
        assert!(mask.contains(&1.));
        assert!(
            (0..64).any(|y| (0..64).any(|x| mask[y * 8 * 512 + x * 8] == 1.)),
            "Nearest latent downsampling lost the thin mask"
        );
    }
}

#[test]
fn gap_planning_and_mask_coverage_respect_existing_saved_fills() {
    use rawpuppy::{
        input::SensorImage,
        render::{Backend, Renderer},
    };
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("photo.raw");
    std::fs::write(&original, b"immutable source").unwrap();
    let source = Arc::new(SensorImage::from_rgb(128, 96, vec![0.18; 128 * 96 * 3]).unwrap());
    let mut edits = Edits::default();
    edits.geometry.rotation = 10.;
    let mut renderer = Renderer::new(Backend::Cpu);
    renderer.set_document(original.clone());
    assert!(
        !renderer
            .gap_contexts(source.clone(), &edits)
            .unwrap()
            .is_empty()
    );
    let mut layers = Layers::new(original.clone());
    let (asset, sha256) = layers
        .store(&Rendered {
            width: 2,
            height: 2,
            pixels: vec![[0.7, 0.2, 0.1, 1.]; 4],
        })
        .unwrap();
    edits.display.synthesis.push(GeneratedFill {
        region: [0., 0., 1., 1.],
        dabs: vec![],
        fill_gaps: true,
        feather: 0.,
        steps: 20,
        seed: 0,
        asset,
        sha256,
        source_sha256: layers.source_hash().unwrap().into(),
        source_color_revision: 0,
        recipe_sha256: synthesis::recipe_hash(&edits).unwrap(),
        sampling: Default::default(),
        model: "moebius-scene-2026-v1".into(),
    });
    assert!(
        renderer
            .gap_contexts(source.clone(), &edits)
            .unwrap()
            .is_empty()
    );
    let mut mask = vec![0.; 512 * 512];
    renderer
        .cover_gap_mask(source, &edits, [0., 0., 1., 1.], &mut mask, [512, 512])
        .unwrap();
    assert!(mask.iter().all(|v| *v == 0.));
    assert_eq!(std::fs::read(original).unwrap(), b"immutable source");
}

#[test]
#[ignore = "requires an immutable real photograph via RAWPUPPY_TEST_GAP_SOURCE"]
fn real_photo_narrow_corner_planning_and_model_masks() {
    use rawpuppy::{
        input::SensorImage,
        pipeline::Pipeline,
        render::{Backend, Renderer},
    };
    use std::sync::Arc;
    let path = std::path::PathBuf::from(
        std::env::var_os("RAWPUPPY_TEST_GAP_SOURCE").expect("Set RAWPUPPY_TEST_GAP_SOURCE"),
    );
    let hash = rawpuppy::models::sha256(&path).unwrap();
    let source = Arc::new(SensorImage::open(&path).unwrap());
    let mut edits = Edits::for_image(&source);
    edits.lens = Default::default();
    edits.tone.mapper = rawpuppy::edits::ToneMapper::Linear;
    edits.geometry.rotation = 0.01;
    let pipeline = Pipeline::compile(&source, &edits).unwrap();
    let (w, h) = pipeline.dimensions(None);
    assert!(w.max(h) > 8000, "Use a high-resolution photograph");
    let coarse = pipeline.render_region([0., 0., 1., 1.], 128, 128).unwrap();
    assert!(coarse.pixels.iter().all(|p| p[3] == 1.));
    let mut renderer = Renderer::new(Backend::Cpu);
    let started = std::time::Instant::now();
    let regions = renderer.gap_contexts(source.clone(), &edits).unwrap();
    eprintln!(
        "{w}x{h}: {} contexts, planning {:?}",
        regions.len(),
        started.elapsed()
    );
    assert!(!regions.is_empty());
    for region in regions {
        let context = renderer
            .render_region(source.clone(), &edits, region, 512, 512)
            .unwrap();
        let mut mask = synthesis::context_mask(&context, region, &[], true, h as f32 / w as f32);
        assert!(mask.iter().all(|v| *v == 0.));
        renderer
            .cover_gap_mask(source.clone(), &edits, region, &mut mask, [512, 512])
            .unwrap();
        assert!((0..64).any(|y| (0..64).any(|x| mask[y * 8 * 512 + x * 8] == 1.)));
    }
    assert_eq!(rawpuppy::models::sha256(&path).unwrap(), hash);
}

#[test]
fn inward_blending_preserves_unpainted_hdr_and_matches_cropped_viewports() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("photo.raw");
    std::fs::write(&original, b"immutable feather control").unwrap();
    let mut layers = Layers::new(original.clone());
    let generated_value = [0.4, 0.6, 1.25, 1.];
    let generated = Rendered {
        width: 8,
        height: 8,
        pixels: vec![generated_value; 64],
    };
    let (asset, sha256) = layers.store(&generated).unwrap();
    let mut edits = Edits::default();
    let fill = GeneratedFill {
        region: [0., 0., 1., 1.],
        dabs: vec![MaskDab {
            center: [0.5, 0.5],
            radius: 0.25,
        }],
        fill_gaps: false,
        feather: 0.25,
        steps: 20,
        seed: 0,
        sampling: Default::default(),
        asset,
        sha256,
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
    let base = [2., -0.125, 0.625, 1.];
    let mut full = Rendered {
        width: 64,
        height: 33,
        pixels: vec![base; 64 * 33],
    };
    let mut fresh = Layers::new(original.clone());
    fresh.apply(&loaded, &mut full, [0., 0., 1., 1.]).unwrap();
    // At y=0.5, x=47.5/64 is 1/128 inside the radius; the blend's
    // smoothstep coordinate is 1/8, giving the independently known 11/256.
    let boundary = full.pixels[16 * 64 + 47];
    for c in 0..3 {
        assert!(
            (boundary[c] - (base[c] + (generated_value[c] - base[c]) * 11. / 256.)).abs()
                < 0.000001
        );
    }
    assert_eq!(full.pixels[16 * 64 + 48], base);
    for (actual, expected) in full.pixels[16 * 64 + 32].iter().zip(generated_value) {
        assert!((actual - expected).abs() < 0.000001);
    }
    for y in 0..33 {
        for x in 0..64 {
            let dx = (x as f32 + 0.5) / 64. - 0.5;
            let dy = ((y as f32 + 0.5) / 33. - 0.5) * 33. / 64.;
            if dx * dx + dy * dy > 0.25 * 0.25 {
                assert_eq!(full.pixels[y * 64 + x], base);
            }
            assert_eq!(full.pixels[y * 64 + x][3], 1.);
        }
    }
    let mut cropped = Rendered {
        width: 32,
        height: 33,
        pixels: vec![base; 32 * 33],
    };
    fresh
        .apply(&loaded, &mut cropped, [0.25, 0., 0.5, 1.])
        .unwrap();
    for y in 0..33 {
        for x in 0..32 {
            assert_eq!(cropped.pixels[y * 32 + x], full.pixels[y * 64 + x + 16]);
        }
    }
    assert_eq!(
        std::fs::read(original).unwrap(),
        b"immutable feather control"
    );
}

#[test]
fn inward_blending_never_reduces_geometric_gap_coverage() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("photo.raw");
    std::fs::write(&original, b"immutable gap control").unwrap();
    let mut layers = Layers::new(original.clone());
    let value = [0.4, 0.6, 1.25, 1.];
    let generated = Rendered {
        width: 8,
        height: 8,
        pixels: vec![value; 64],
    };
    let (asset, sha256) = layers.store(&generated).unwrap();
    let mut edits = Edits::default();
    edits.display.synthesis.push(GeneratedFill {
        region: [0., 0., 1., 1.],
        dabs: vec![MaskDab {
            center: [0.5, 0.5],
            radius: 0.25,
        }],
        fill_gaps: true,
        feather: 1.,
        steps: 20,
        seed: 0,
        sampling: Default::default(),
        asset,
        sha256,
        source_sha256: layers.source_hash().unwrap().into(),
        source_color_revision: 0,
        recipe_sha256: synthesis::recipe_hash(&Edits::default()).unwrap(),
        model: "moebius-scene-2026-v1".into(),
    });
    let base = [2., -0.125, 0.625, 1.];
    let mut raster = Rendered {
        width: 64,
        height: 33,
        pixels: vec![base; 64 * 33],
    };
    for (x, y, alpha) in [(2, 2, 0.), (47, 16, 0.25)] {
        raster.pixels[y * 64 + x][3] = alpha;
    }
    layers.apply(&edits, &mut raster, [0., 0., 1., 1.]).unwrap();
    for (x, y) in [(2, 2), (47, 16)] {
        for (actual, expected) in raster.pixels[y * 64 + x].iter().zip(value) {
            assert!((actual - expected).abs() < 0.000001);
        }
    }
    assert_eq!(raster.pixels[0], base);
    assert_eq!(std::fs::read(original).unwrap(), b"immutable gap control");
}
