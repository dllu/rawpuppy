use rawpuppy::{
    edits::{Edits, ToneMapper},
    input::SensorImage,
    pipeline::Pipeline,
    sidecar,
};

fn camera(rgb: [f32; 3]) -> SensorImage {
    let mut source = SensorImage::from_rgb(17, 19, vec![0.; 17 * 19 * 3]).unwrap();
    let cfa = rawler::CFA::new("RGGB");
    source.data = (0..17 * 19)
        .map(|i| rgb[cfa.color_at(i / 17, i % 17)])
        .collect();
    source.cpp = 1;
    source.cfa = Some(cfa);
    source.raw_integer = true;
    source.metadata.as_shot = [2., 1., 1.5];
    source
}

#[test]
fn recovery_preserves_intact_channels_and_camera_lower_bounds() {
    for (input, expected) in [
        ([0.6, 0.995, 0.8], [1.2; 3]),
        ([1.; 3], [2.; 3]),
        ([1., 0.1, 0.1], [2., 0.1, 0.15]),
    ] {
        let source = camera(input);
        let original = source.data.clone();
        let mut edits = Edits::for_image(&source);
        edits.tone.mapper = ToneMapper::Linear;
        let output = Pipeline::compile(&source, &edits).unwrap().sample([0.5; 2]);
        for c in 0..3 {
            assert!(
                (output[c] - expected[c]).abs() < 2e-6,
                "{input:?}: {output:?}"
            );
        }
        assert_eq!(output[3], 1.);
        assert_eq!(source.data, original);
    }
}

#[test]
fn clipping_coverage_blends_continuously_into_the_fully_clipped_estimate() {
    let rgb = [0.995; 3];
    let white = [2., 1., 1.5];
    let mut previous = rawpuppy::highlights::recover(rgb, [1., 1., 0.], white);
    assert_eq!(previous[2], rgb[2]);
    for i in 1..=100 {
        let output = rawpuppy::highlights::recover(rgb, [1., 1., i as f32 / 100.], white);
        for c in 0..3 {
            assert!(output[c] >= rgb[c]);
            assert!(
                (output[c] - previous[c]).abs() < 0.02,
                "Discontinuous coverage: {previous:?} -> {output:?}"
            );
        }
        previous = output;
    }
}

#[test]
fn recovery_keeps_valid_raw_and_signed_hdr_exact_and_legacy_recipes_compatible() {
    let source = camera([0.2, 0.3, 0.4]);
    let mut edits = Edits::for_image(&source);
    edits.tone.mapper = ToneMapper::Linear;
    edits.scene.exposure = 3.;
    let recovered = Pipeline::compile(&source, &edits).unwrap().sample([0.5; 2]);
    edits.raw.recover_highlights = false;
    assert_eq!(
        recovered,
        Pipeline::compile(&source, &edits).unwrap().sample([0.5; 2])
    );
    let hdr = SensorImage::from_rgb(
        2,
        2,
        vec![
            -0.1, 2.5, 1.4, -0.1, 2.5, 1.4, -0.1, 2.5, 1.4, -0.1, 2.5, 1.4,
        ],
    )
    .unwrap();
    let before = Pipeline::compile(&hdr, &edits).unwrap().sample([0.5; 2]);
    edits.raw.recover_highlights = true;
    assert_eq!(
        before,
        Pipeline::compile(&hdr, &edits).unwrap().sample([0.5; 2])
    );
    let directory = tempfile::tempdir().unwrap();
    let integer_hdr = camera([1.2, -0.1, 2.4]);
    edits.raw.recover_highlights = false;
    let original_hdr = Pipeline::compile(&integer_hdr, &edits)
        .unwrap()
        .sample([0.5; 2]);
    edits.raw.recover_highlights = true;
    assert_eq!(
        original_hdr,
        Pipeline::compile(&integer_hdr, &edits)
            .unwrap()
            .sample([0.5; 2])
    );
    let path = directory.path().join("test.rawpuppy.xmp");
    sidecar::save(&path, &edits).unwrap();
    assert_eq!(sidecar::load(&path).unwrap(), edits);
    let legacy: Edits =
        serde_json::from_str(include_str!("data/legacy-synthesis-recipe.json")).unwrap();
    assert!(!legacy.raw.recover_highlights);
    assert!(
        serde_json::to_value(&legacy.raw)
            .unwrap()
            .get("recover_highlights")
            .is_none()
    );
    assert_eq!(
        rawpuppy::synthesis::recipe_hash(&legacy).unwrap(),
        legacy.display.synthesis[0].recipe_sha256
    );
}
