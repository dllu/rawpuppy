use rawpuppy::{
    calibration::{self, Illuminant},
    color,
    edits::{Edits, ToneMapper},
    input::SensorImage,
    pipeline::Pipeline,
};

fn close(a: [f32; 3], b: [f32; 3], tolerance: f32) {
    for i in 0..3 {
        assert!((a[i] - b[i]).abs() <= tolerance, "{a:?} differs from {b:?}");
    }
}
fn source(rgb: [f32; 3]) -> SensorImage {
    SensorImage::from_rgb(64, 48, (0..64 * 48).flat_map(|_| rgb).collect()).unwrap()
}
#[test]
fn illuminant_swatch_and_picker_use_linear_rgb_without_double_transfer_encoding() {
    let linear = [0.8, 0.5, 0.2];
    let xy = calibration::chromaticity(color::apply(color::SRGB_TO_XYZ, linear)).unwrap();
    let picked = calibration::swatch(xy).unwrap();
    close(picked, [1., 0.625, 0.25], 1e-6);
    let restored = calibration::chromaticity(color::apply(color::SRGB_TO_XYZ, picked)).unwrap();
    for i in 0..2 {
        assert!((restored[i] - xy[i]).abs() < 1e-7);
    }
}
#[test]
fn cat16_adapts_white_and_preserves_linear_signed_hdr_values() {
    let image = source([0.2; 3]);
    for illuminant in [
        Illuminant::Daylight { kelvin: 6504. },
        Illuminant::BlackBody { kelvin: 2856. },
        Illuminant::Custom { xy: [0.38, 0.36] },
    ] {
        let mut edits = Edits::default();
        edits.scene.illuminant = illuminant;
        let xy = illuminant.xy(&image.metadata).unwrap();
        let rgb = color::apply(
            color::inverse(color::SRGB_TO_XYZ).unwrap(),
            calibration::xyz(xy).unwrap(),
        );
        let matrix = calibration::matrix(&image.metadata, &edits.scene).unwrap();
        close(color::apply(matrix, rgb), [1.; 3], 2e-6);
        close(color::apply(matrix, rgb.map(|v| v * 8.)), [8.; 3], 2e-5);
        close(
            color::apply(matrix, rgb.map(|v| v * (-0.3))),
            [-0.3; 3],
            1e-6,
        );
    }
}
#[test]
fn spectral_illuminants_match_cie_reference_whites_and_temperature_direction() {
    let image = source([0.2; 3]);
    for (illuminant, expected) in [
        (Illuminant::BlackBody { kelvin: 2856. }, [0.44757, 0.40745]),
        (Illuminant::Daylight { kelvin: 6504. }, calibration::D65),
    ] {
        let xy = illuminant.xy(&image.metadata).unwrap();
        for i in 0..2 {
            assert!((xy[i] - expected[i]).abs() < 0.0002, "{xy:?}");
        }
        assert!(
            (calibration::temperature(xy).unwrap()
                - match illuminant {
                    Illuminant::BlackBody { kelvin } | Illuminant::Daylight { kelvin } => kelvin,
                    _ => unreachable!(),
                })
            .abs()
                < 12.
        );
    }
    let corrected = |kelvin| {
        let mut edits = Edits::default();
        edits.scene.illuminant = Illuminant::BlackBody { kelvin };
        color::apply(
            calibration::matrix(&image.metadata, &edits.scene).unwrap(),
            [0.2; 3],
        )
    };
    let warm_source = corrected(2856.);
    let cool_source = corrected(9000.);
    assert!(warm_source[2] > warm_source[0]);
    assert!(cool_source[0] > cool_source[2]);
}
#[test]
fn as_shot_uses_unbalanced_sensor_profile_and_neutral_luminance() {
    let mut image = source([0.2; 3]);
    let gains = [2., 1., 1.5];
    image.metadata.as_shot = gains;
    // Physical camera neutral: camera response to the chosen scene illuminant.
    let reference = calibration::xyz([0.43, 0.39]).unwrap();
    let xyz_to_cam = [[0.7, 0.1, 0.05], [0.05, 0.9, 0.05], [0.02, 0.1, 0.8]];
    let response = color::apply(xyz_to_cam, reference);
    image.metadata.as_shot = response.map(|v| response[1] / v);
    image.metadata.camera_to_xyz = Some(
        color::inverse(xyz_to_cam)
            .unwrap()
            .map(|row| row.map(|v| v * response[1])),
    );
    let mut edits = Edits::default();
    edits.scene.illuminant = Illuminant::Camera;
    close(
        color::apply(
            calibration::matrix(&image.metadata, &edits.scene).unwrap(),
            image.metadata.as_shot.map(|v| 1. / v),
        ),
        [1.; 3],
        2e-6,
    );
    let xy = Illuminant::Camera.xy(&image.metadata).unwrap();
    assert!((xy[0] - 0.43).abs() < 1e-6 && (xy[1] - 0.39).abs() < 1e-6);
}
#[test]
fn historical_recipes_serialize_and_render_without_new_fields_or_changes() {
    let json = include_str!("data/legacy-calibration-recipe.json");
    let mut edits: Edits = serde_json::from_str(json).unwrap();
    assert!(edits.scene.illuminant.is_legacy());
    assert_eq!(
        serde_json::to_string_pretty(&edits).unwrap(),
        json.replace("\r\n", "\n").trim()
    );
    edits.lens = Default::default();
    let mut image = source([0.2; 3]);
    image.metadata.as_shot = [2., 1., 1.5];
    let expected = color::multiply(
        edits.scene.mixer,
        color::multiply(
            image.metadata.camera_to_working,
            [[2. * 1.04, 0., 0.], [0., 0.97, 0.], [0., 0., 1.5 * 1.12]],
        ),
    );
    assert_eq!(
        Pipeline::compile(&image, &edits).unwrap().calibration,
        expected
    );
}
#[test]
fn neutral_patch_is_measured_before_tone_exposure_mixer_and_current_illuminant() {
    let warm = [0.6, 0.3, 0.1];
    let cool = [0.1, 0.3, 0.6];
    let image = SensorImage::from_rgb(
        64,
        48,
        (0..64 * 48)
            .flat_map(|i| if i % 64 < 32 { warm } else { cool })
            .collect(),
    )
    .unwrap();
    let mut edits = Edits::default();
    let region = [0.05, 0.05, 0.35, 0.8];
    let sample = calibration::estimate(&image, &edits, region).unwrap();
    let expected = calibration::chromaticity(color::apply(color::SRGB_TO_XYZ, warm)).unwrap();
    assert_eq!(sample.xy, expected);
    edits.scene.exposure = 8.;
    edits.scene.mixer = [[3., 0., 0.], [0., 0.5, 0.], [0., 0., 0.1]];
    edits.scene.calibration = [4., 1., 0.25];
    edits.scene.illuminant = Illuminant::BlackBody { kelvin: 1000. };
    edits.tone.mapper = ToneMapper::Agx;
    edits.tone.saturation = 0.;
    assert_eq!(
        calibration::estimate(&image, &edits, region).unwrap().xy,
        sample.xy
    );
    let whole = calibration::estimate(&image, &edits, [0., 0., 1., 1.]).unwrap();
    assert_ne!(whole.xy, sample.xy);
    edits.geometry.crop = [0., 0., 0.4, 1.];
    assert_eq!(
        calibration::estimate(&image, &edits, [0., 0., 1., 1.])
            .unwrap()
            .xy,
        sample.xy
    );
}
#[test]
fn estimator_rejects_empty_dark_clipped_and_invalid_samples() {
    let edits = Edits::default();
    assert!(calibration::estimate(&source([0.; 3]), &edits, [0., 0., 1., 1.]).is_err());
    let mut clipped = source([1.; 3]);
    clipped.raw_integer = true;
    assert!(calibration::estimate(&clipped, &edits, [0., 0., 1., 1.]).is_err());
    assert!(calibration::estimate(&source([0.2; 3]), &edits, [0., 0., 0., 1.]).is_err());
    assert!(Illuminant::Custom { xy: [0.8, 0.8] }.validate().is_err());
    assert!(Illuminant::Daylight { kelvin: 3000. }.validate().is_err());
    assert!(
        Illuminant::BlackBody { kelvin: f32::NAN }
            .validate()
            .is_err()
    );
}
