use rawpuppy::{
    config::{Config, Module, ModuleDefaults},
    edits::Edits,
    input::SensorImage,
    sidecar,
};

fn image() -> SensorImage {
    SensorImage::from_rgb(8, 8, vec![0.2; 8 * 8 * 3]).unwrap()
}

#[test]
fn preferences_are_created_roundtrip_and_merge_partial_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rawpuppy/config.toml");
    let mut config = Config::load_from(&path).unwrap();
    assert!(path.is_file());
    assert_eq!(config.ui.font_size, 18.);
    let edits = config.defaults.for_image(&image());
    assert_eq!(
        [
            edits.scene.exposure,
            edits.tone.saturation,
            edits.detail.sharpen
        ],
        [0.7, 1.05, 0.2]
    );
    config.ui.font_size = 22.;
    config.ui.sidebar_width = 480.;
    config.defaults.light.exposure = -0.25;
    config.defaults.sensor.recover_highlights = Some(false);
    config.save_to(&path).unwrap();
    assert_eq!(Config::load_from(&path).unwrap(), config);

    std::fs::write(
        &path,
        "[ui]\nfont_size = 20\n[defaults.light]\nexposure = 1.2\n[defaults.detail]\nlocal_contrast = 0.1\n",
    )
    .unwrap();
    let partial = Config::load_from(&path).unwrap();
    assert_eq!(partial.ui.font_size, 20.);
    assert_eq!(partial.ui.sidebar_width, 420.);
    assert_eq!(partial.defaults.light.exposure, 1.2);
    assert_eq!(partial.defaults.detail.sharpen, 0.2);
    assert_eq!(partial.defaults.detail.local_contrast, 0.1);
    assert_eq!(partial.defaults.color.saturation, 1.05);
}

#[test]
fn invalid_config_is_reported_without_replacing_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    for contents in [
        "[ui]\nfont_size = 0\n",
        "[ui]\nfont_size = nan\n",
        "[ui]\nsidebar_width = 20000\n",
        "[ui]\nfont_szie = 18\n",
        "[defaults.light]\nexposure = 40\n",
        "[defaults.color]\nsaturation = -1\n",
        "[defaults.detail]\nsharpen = 2\n",
        "[defaults.sensor]\ndenoise = inf\n",
        "[ui",
    ] {
        std::fs::write(&path, contents).unwrap();
        assert!(Config::load_from(&path).is_err(), "{contents}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
    }
    let original = Config::default();
    original.save_to(&path).unwrap();
    let mut invalid = original.clone();
    invalid.defaults.detail.sharpen = -1.;
    assert!(invalid.save_to(&path).is_err());
    assert_eq!(Config::load_from(&path).unwrap(), original);
}

#[test]
fn captured_defaults_exclude_photo_specific_edits_and_existing_sidecars_win() {
    let image = image();
    let mut defaults = ModuleDefaults::default();
    let baseline = defaults.for_image(&image);
    let mut edited = baseline.clone();
    edited.scene.exposure = 1.25;
    edited.scene.vignette = [0.5, 0.1];
    edited.scene.graduated.exposure = -1.;
    edited.scene.calibration = [1.1, 1., 0.9];
    edited.geometry.rotation = 15.;
    edited.geometry.crop = [0.1, 0.1, 0.8, 0.8];
    edited.tone.saturation = 1.1;
    edited.detail.sharpen = 0.3;
    edited.raw.hot_pixels = true;
    for module in [Module::Light, Module::Color, Module::Detail, Module::Sensor] {
        defaults.capture(module, &edited);
    }
    let next = defaults.for_image(&image);
    assert_eq!(next.scene.exposure, 1.25);
    assert_eq!(next.scene.vignette, [0.5, 0.1]);
    assert_eq!(next.tone.saturation, 1.1);
    assert_eq!(next.detail.sharpen, 0.3);
    assert!(next.raw.hot_pixels);
    assert_eq!(next.geometry, baseline.geometry);
    assert_eq!(next.scene.calibration, baseline.scene.calibration);
    assert_eq!(next.scene.graduated, baseline.scene.graduated);
    assert_eq!(next.display, baseline.display);

    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("photo.png");
    assert_eq!(
        sidecar::load_for_default(&original, next.clone()).unwrap(),
        next
    );
    // Neutral/historical recipes must not inherit today's new-photo defaults.
    let historical = Edits::default();
    sidecar::save(&sidecar::path_for(&original), &historical).unwrap();
    assert_eq!(
        sidecar::load_for_default(&original, next).unwrap(),
        historical
    );

    let mut integer_raw = image;
    integer_raw.raw_integer = true;
    assert!(
        ModuleDefaults::default()
            .for_image(&integer_raw)
            .raw
            .recover_highlights
    );
    assert!(!defaults.for_image(&integer_raw).raw.recover_highlights);
}
