use rawpuppy::{
    edits::{Edits, LensEdits, LensMode, ToneMapper},
    geometry::Geometry,
    input::SensorImage,
    lens::{Correction, LensProfile, RadialTable},
    pipeline::Pipeline,
};

fn profile(width: usize, height: usize, percent: f32) -> LensProfile {
    let radius_pixels = (width as f32).hypot(height as f32) * 0.5;
    LensProfile {
        distortion: Some(RadialTable {
            radius_pixels,
            knots: vec![[0.5, percent], [1.1, percent]],
        }),
        vignette: Some(RadialTable {
            radius_pixels,
            knots: vec![[0.5, 50.], [1.1, 50.]],
        }),
        ..LensProfile::default()
    }
}

#[test]
fn old_saved_fill_hashes_survive_new_camera_correction_fields() {
    let edits: Edits =
        serde_json::from_str(include_str!("data/legacy-synthesis-recipe.json")).unwrap();
    assert_eq!(edits.lens.mode, LensMode::Off);
    assert!(serde_json::to_value(&edits).unwrap().get("lens").is_none());
    let hash = rawpuppy::synthesis::recipe_hash(&edits).unwrap();
    assert!(!edits.display.synthesis.is_empty());
    for fill in &edits.display.synthesis {
        assert_eq!(hash, fill.recipe_sha256);
    }
}

#[test]
fn embedded_map_and_scene_gain_have_known_coordinates_and_values() {
    let profile = profile(20, 20, 10.);
    let lens = LensEdits {
        mode: LensMode::EmbeddedV1,
        vignette: false,
        auto_frame: false,
        ..LensEdits::default()
    };
    let mut map = Geometry::compile(&Edits::default().geometry, 20, 20).unwrap();
    map.lens = Correction::compile(Some(&profile), &lens, 20, 20).unwrap();
    let point = map.map([1., 0.5], 1).unwrap();
    assert!((point[0] - 1.05).abs() < 1e-6);
    assert!((point[1] - 0.5).abs() < 1e-6);
    let mut source = SensorImage::from_rgb(20, 20, vec![0.1; 20 * 20 * 3]).unwrap();
    source.metadata.lens_profile = Some(profile);
    let mut edits = Edits::for_image(&source);
    edits.lens.distortion = false;
    edits.lens.auto_frame = false;
    edits.tone.mapper = ToneMapper::Linear;
    let pipe = Pipeline::compile(&source, &edits).unwrap();
    let pixel = pipe.sample([0.75, 0.75]);
    assert!((pixel[0] - 0.2).abs() < 0.00002, "{pixel:?}");
    assert_eq!(pixel[3], 1.);
    assert_eq!(source.data[0], 0.1);
    let plain = Pipeline::compile(&source, &Edits::default())
        .unwrap()
        .geometry;
    assert!(!plain.lens.distortion && !plain.lens.vignette);
}

#[test]
fn camera_auto_frame_keeps_barrel_and_pincushion_boundaries_inside_the_sensor() {
    for percent in [-10., 10.] {
        let profile = profile(4001, 3001, percent);
        let lens = LensEdits {
            mode: LensMode::EmbeddedV1,
            vignette: false,
            ..LensEdits::default()
        };
        let mut map = Geometry::compile(&Edits::default().geometry, 4001, 3001).unwrap();
        map.lens = Correction::compile(Some(&profile), &lens, 4001, 3001).unwrap();
        for i in 0..=256 {
            let t = i as f32 / 256.;
            for uv in [[0., t], [1., t], [t, 0.], [t, 1.]] {
                let point = map.map(uv, 1).unwrap();
                assert!(
                    point.iter().all(|v| *v >= 0. && *v <= 1.),
                    "{percent}: {uv:?} -> {point:?}"
                );
            }
        }
    }
}

#[test]
fn gfx_curve_knots_and_invalid_transmission_are_checked() {
    let profile: LensProfile =
        serde_json::from_str(include_str!("data/gfx-lens-profile.json")).unwrap();
    let lens = LensEdits {
        mode: LensMode::EmbeddedV1,
        auto_frame: false,
        ..LensEdits::default()
    };
    let table = Correction::compile(Some(&profile), &lens, 11648, 8736).unwrap();
    for [radius, percent] in &profile.distortion.as_ref().unwrap().knots {
        assert!((table.lookup(radius * radius, 0) - (1. + percent * 0.01)).abs() < 0.000003);
    }
    for [radius, transmission] in &profile.vignette.as_ref().unwrap().knots {
        assert!((table.lookup(radius * radius, 1) - 100. / transmission).abs() < 0.0003);
    }
    assert_eq!(table.lookup(0., 1), 1.);
    let mut bad = profile;
    bad.vignette.as_mut().unwrap().knots[0][1] = 0.;
    assert!(Correction::compile(Some(&bad), &lens, 11648, 8736).is_err());
    assert!(Correction::compile(None, &lens, 11648, 8736).is_err());
}
