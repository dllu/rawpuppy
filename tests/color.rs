use rawpuppy::{
    color,
    edits::{Edits, ToneMapper},
    export,
    input::SensorImage,
    pipeline::{Pipeline, Rendered},
};

#[test]
fn primary_matrices_and_white_adaptation_match_standard_colorimetry() {
    let srgb = color::primaries_to_xyz([[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]], [0.3127, 0.329])
        .unwrap();
    let rec = color::primaries_to_xyz(
        [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]],
        [0.3127, 0.329],
    )
    .unwrap();
    for (a, b) in srgb
        .into_iter()
        .flatten()
        .zip(color::SRGB_TO_XYZ.into_iter().flatten())
    {
        assert!((a - b).abs() < 0.000002);
    }
    for (a, b) in rec
        .into_iter()
        .flatten()
        .zip(color::REC2020_TO_XYZ.into_iter().flatten())
    {
        assert!((a - b).abs() < 0.000002);
    }
    let prophoto = color::rgb_primaries_to_working(
        [[0.7347, 0.2653], [0.1596, 0.8404], [0.0366, 0.0001]],
        [0.3457, 0.3585],
    )
    .unwrap();
    for v in color::apply(prophoto, [0.18; 3]) {
        assert!((v - 0.18).abs() < 0.000002);
    }
}

#[test]
fn tagged_hdr_exr_import_preserves_wide_gamut_and_unbounded_values() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("wide.exr");
    let values = [
        [0.18; 4],
        [2., 0.3, -0.01, 1.],
        [0.1, 0.9, 0.2, 1.],
        [0.4, 0.01, 3., 1.],
    ];
    let image = Rendered {
        width: 2,
        height: 2,
        pixels: values.to_vec(),
    };
    let chroma = exr::meta::attribute::Chromaticities {
        red: exr::math::Vec2(0.708, 0.292),
        green: exr::math::Vec2(0.170, 0.797),
        blue: exr::math::Vec2(0.131, 0.046),
        white: exr::math::Vec2(0.3127, 0.329),
    };
    export::write_linear_exr(
        &image,
        std::io::BufWriter::new(std::fs::File::create(&path).unwrap()),
        chroma,
    )
    .unwrap();
    let source = SensorImage::open(&path).unwrap();
    let m = color::multiply(
        color::inverse(color::SRGB_TO_XYZ).unwrap(),
        color::REC2020_TO_XYZ,
    );
    for (p, actual) in values.iter().zip(source.data.as_chunks::<3>().0.iter()) {
        let expected = color::apply(m, [p[0], p[1], p[2]]);
        for c in 0..3 {
            assert!((expected[c] - actual[c]).abs() < 0.000008);
        }
    }
    assert!(source.data.iter().any(|v| *v < 0.));
    assert!(source.data.iter().any(|v| *v > 1.));
    assert_eq!(source.metadata.color_revision, 1);
    let mut layers = rawpuppy::synthesis::Layers::new(path.clone());
    let (asset, sha256) = layers.store(&image).unwrap();
    let mut edits = Edits::default();
    let fill = rawpuppy::synthesis::GeneratedFill {
        region: [0., 0., 1., 1.],
        dabs: vec![rawpuppy::synthesis::MaskDab {
            center: [0.5; 2],
            radius: 1.,
        }],
        fill_gaps: false,
        steps: 10,
        seed: 0,
        asset,
        sha256,
        source_sha256: layers.source_hash().unwrap().into(),
        source_color_revision: 0,
        recipe_sha256: rawpuppy::synthesis::recipe_hash(&edits).unwrap(),
        sampling: Default::default(),
        model: "moebius-scene-2026-v1".into(),
    };
    edits.display.synthesis.push(fill);
    let mut raster = Rendered {
        width: 2,
        height: 2,
        pixels: vec![[0.18; 4]; 4],
    };
    assert!(layers.apply(&edits, &mut raster, [0., 0., 1., 1.]).is_err());
    assert_eq!(raster.pixels, vec![[0.18; 4]; 4]);
    edits.display.synthesis[0].source_color_revision = 1;
    layers.apply(&edits, &mut raster, [0., 0., 1., 1.]).unwrap();
    let exported = directory.path().join("linear.exr");
    export::write(&exported, &image, color::OutputSpace::LinearSrgb, false).unwrap();
    let metadata = exr::meta::MetaData::read_from_file(exported, false).unwrap();
    assert_eq!(
        metadata.headers[0].shared_attributes.chromaticities,
        Some(export::SRGB_CHROMATICITIES)
    );
}

#[test]
fn photographic_agx_preserves_gray_and_matches_independent_oracle_samples() {
    for c in color::agx([0.18; 3]) {
        assert!((c - 0.18).abs() < 0.000003);
    }
    let cases: Vec<([f32; 3], [f32; 3])> =
        serde_json::from_str(include_str!("data/agx-oracle.json")).unwrap();
    let mut sum = 0f64;
    for (input, reference) in &cases {
        let actual = color::agx(*input);
        for c in 0..3 {
            let error = (actual[c] - reference[c]).abs();
            assert!(
                error < 0.01,
                "AgX oracle mismatch {input:?}: {actual:?} vs {reference:?}"
            );
            sum += error as f64;
        }
    }
    assert!(sum / ((cases.len() * 3) as f64) < 0.0008);
}

#[test]
fn old_agx_recipes_keep_their_original_transform() {
    let edits: Edits =
        serde_json::from_str(r#"{"tone":{"mapper":"agx","saturation":1.0}}"#).unwrap();
    assert_eq!(edits.tone.mapper, ToneMapper::Agx);
    let source = SensorImage::from_rgb(2, 2, vec![0.18; 12]).unwrap();
    let actual = Pipeline::compile(&source, &edits).unwrap().sample([0.5; 2]);
    for (actual, expected) in actual[..3].iter().zip([0.21446736, 0.21453233, 0.21453686]) {
        assert!((*actual - expected).abs() < 0.000002);
    }
    assert_eq!(Edits::default().tone.mapper, ToneMapper::AgxSdr);
    assert!(
        serde_json::to_string(&Edits::default())
            .unwrap()
            .contains("agx_sdr_v1")
    );
}
