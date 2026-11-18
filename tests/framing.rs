use rawpuppy::{
    color,
    crop::{self, Constraint},
    edits::{CameraIntrinsics, Edits, GeometryEdits, GuideAxis, PerspectiveGuide},
    geometry::Geometry,
    input::SensorImage,
    perspective::{self, Mode},
};

#[test]
fn physical_camera_calibration_respects_portrait_orientation() {
    let mut image = SensorImage::from_rgb(4, 3, vec![0.2; 36]).unwrap();
    image.metadata.make = "Fujifilm".into();
    image.metadata.model = "GFX 100S".into();
    image.metadata.focal_length_mm = Some(23.);
    let camera = perspective::camera_for_image(&image).unwrap();
    assert_eq!(camera.sensor_mm, [43.8, 32.9]);
    let landscape = perspective::camera_fov(camera, 11648, 8736, 1.).unwrap();
    let portrait = perspective::camera_fov(camera, 8736, 11648, 1.).unwrap();
    assert!((landscape - (2. * (43.8f32 / 46.).atan()).to_degrees()).abs() < 1e-5);
    assert!(portrait < landscape);
    assert!(perspective::camera_fov(camera, 11648, 8736, 0.9).unwrap() < landscape);
    assert!(
        perspective::camera_fov(
            CameraIntrinsics {
                focal_length_mm: 0.,
                ..camera
            },
            4,
            3,
            1.
        )
        .is_none()
    );
    assert_eq!(Edits::for_image(&image).geometry.camera, Some(camera));
}

#[test]
fn two_guides_per_axis_rectify_an_independent_camera_and_ignore_excluded_lines() {
    for (width, height) in [(600, 400), (400, 600)] {
        let truth = GeometryEdits {
            pitch: 17.,
            yaw: 13.,
            rotation: -4.,
            ..Default::default()
        };
        let original = Geometry::compile(&truth, width, height).unwrap();
        let mut guides: Vec<_> = [
            (GuideAxis::Vertical, [0.25, 0.1], [0.25, 0.9]),
            (GuideAxis::Vertical, [0.75, 0.1], [0.75, 0.9]),
            (GuideAxis::Horizontal, [0.1, 0.2], [0.9, 0.2]),
            (GuideAxis::Horizontal, [0.1, 0.8], [0.9, 0.8]),
        ]
        .map(|(axis, a, b)| PerspectiveGuide {
            axis,
            start: original.map(a, 1).unwrap(),
            end: original.map(b, 1).unwrap(),
            enabled: true,
        })
        .to_vec();
        guides.push(PerspectiveGuide {
            start: [0.1, 0.1],
            end: [0.9, 0.9],
            axis: GuideAxis::Vertical,
            enabled: false,
        });
        let report = perspective::fit_guides(
            &guides,
            [width, height],
            &GeometryEdits::default(),
            Mode::Both,
        )
        .unwrap();
        let solution = report
            .solution
            .unwrap_or_else(|| panic!("{}", report.reason));
        assert!((solution.pitch - truth.pitch).abs() < 0.001, "{solution:?}");
        assert!((solution.yaw - truth.yaw).abs() < 0.001, "{solution:?}");
        let inverse = color::inverse(
            Geometry::compile(&solution, width, height)
                .unwrap()
                .homography,
        )
        .unwrap();
        for guide in guides.iter().filter(|g| g.enabled) {
            let p = [guide.start, guide.end].map(|p| {
                let p = color::apply(inverse, [p[0], p[1], 1.]);
                [p[0] / p[2], p[1] / p[2]]
            });
            let axis = if guide.axis == GuideAxis::Vertical {
                0
            } else {
                1
            };
            assert!((p[1][axis] - p[0][axis]).abs() < 1e-5);
        }
        let duplicate = vec![guides[0].clone(); 2];
        assert!(
            perspective::fit_guides(
                &duplicate,
                [width, height],
                &GeometryEdits::default(),
                Mode::Vertical
            )
            .unwrap()
            .solution
            .is_none()
        );
    }
}

#[test]
fn largest_crop_retains_full_identity_and_approaches_the_rotated_square_optimum() {
    let image = SensorImage::from_rgb(32, 32, vec![0.2; 32 * 32 * 3]).unwrap();
    let mut edits = Edits::default();
    assert_eq!(
        crop::largest(&image, &edits, Constraint::LargestArea).unwrap(),
        [0., 0., 1., 1.]
    );
    edits.geometry.rotation = 45.;
    let free = crop::largest(&image, &edits, Constraint::LargestArea).unwrap();
    assert!(
        free[2] * free[3] > 0.49 && free[2] * free[3] <= 0.50001,
        "{free:?}"
    );
    let square = crop::largest(&image, &edits, Constraint::Aspect(1.)).unwrap();
    assert!(square[2] * square[3] > 0.49);
    assert!((square[2] - square[3]).abs() < 1e-6);
}

#[test]
fn auto_crops_preserve_requested_aspects_and_exclude_corrected_lens_gaps() {
    for (width, height) in [(64, 48), (48, 64)] {
        let image = SensorImage::from_rgb(width, height, vec![0.2; width * height * 3]).unwrap();
        let mut edits = Edits::default();
        edits.geometry.pitch = 18.;
        edits.geometry.yaw = -12.;
        edits.geometry.rotation = 7.;
        edits.geometry.distortion = [0.12, -0.02];
        edits.geometry.chromatic_aberration = [0.005, -0.004];
        let largest = crop::largest(&image, &edits, Constraint::LargestArea).unwrap();
        for constraint in [
            Constraint::LargestArea,
            Constraint::Aspect(1.),
            Constraint::Aspect(3. / 2.),
            Constraint::Aspect(16. / 9.),
        ] {
            let c = crop::largest(&image, &edits, constraint).unwrap();
            assert!(c[2] * c[3] <= largest[2] * largest[3] + 1e-6);
            if let Constraint::Aspect(r) = constraint {
                assert!((c[2] * width as f32 / (c[3] * height as f32) - r).abs() < 1e-5);
            }
            edits.geometry.crop = c;
            let geometry = Geometry::compile(&edits.geometry, width, height).unwrap();
            for y in 0..=96 {
                for x in 0..=96 {
                    for channel in 0..3 {
                        let p = geometry
                            .map([x as f32 / 96., y as f32 / 96.], channel)
                            .unwrap();
                        assert!(
                            p.iter().all(|v| (0. ..=1.).contains(v)),
                            "Gap in crop {c:?} at {p:?}"
                        );
                    }
                }
            }
        }
        edits.geometry = GeometryEdits::default();
        for ratio in [1e-4, 1e4] {
            let c = crop::largest(&image, &edits, Constraint::Aspect(ratio)).unwrap();
            assert!((c[2] * width as f32 / (c[3] * height as f32) / ratio - 1.).abs() < 1e-5);
        }
    }
}

#[test]
fn editable_evidence_roundtrips_without_invalidating_generated_pixels() {
    let mut edits = Edits::default();
    let hash = rawpuppy::synthesis::recipe_hash(&edits).unwrap();
    edits.geometry.camera = Some(CameraIntrinsics {
        focal_length_mm: 23.,
        sensor_mm: [43.8, 32.9],
    });
    edits.geometry.crop_aspect = Some(1.5);
    edits.geometry.guides.push(PerspectiveGuide {
        start: [0.2, 0.1],
        end: [0.3, 0.9],
        axis: GuideAxis::Vertical,
        enabled: false,
    });
    edits.validate().unwrap();
    assert_eq!(rawpuppy::synthesis::recipe_hash(&edits).unwrap(), hash);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("photo.raf.rawpuppy.xmp");
    rawpuppy::sidecar::save(&path, &edits).unwrap();
    assert_eq!(rawpuppy::sidecar::load(&path).unwrap(), edits);
}

#[test]
fn applied_perspective_and_auto_fit_can_crop_outside_the_original_frame() {
    let image = SensorImage::from_rgb(64, 48, vec![0.2; 64 * 48 * 3]).unwrap();
    let edits = Edits::default();
    let mut corrected = edits.geometry.clone();
    corrected.scale = 1.5;
    corrected.shift = [0.125, 0.25];
    let expected = [-0.375, -0.5, 1.5, 1.5];
    let compare = |actual: [f32; 4], expected: [f32; 4]| {
        for (a, b) in actual.into_iter().zip(expected) {
            assert!((a - b).abs() < 1e-6, "{actual:?} vs {expected:?}");
        }
    };
    crop::after_perspective(&image, &edits, &mut corrected, None).unwrap();
    compare(corrected.crop, expected);

    let mut with_crop = edits.clone();
    with_crop.geometry.crop = [0.2, 0.3, 0.7, 0.6];
    let mut manual_crop = corrected.clone();
    manual_crop.crop = with_crop.geometry.crop;
    crop::after_perspective(&image, &with_crop, &mut manual_crop, None).unwrap();
    assert_eq!(manual_crop.crop, with_crop.geometry.crop);

    let mut current = edits.clone();
    current.geometry = corrected.clone();
    let constraint = Constraint::Aspect(2.);
    let manual_fit = crop::largest(&image, &current, constraint).unwrap();
    crop::after_perspective(&image, &edits, &mut corrected, Some(constraint)).unwrap();
    assert_eq!(corrected.crop, manual_fit);
    assert_eq!(corrected.crop_aspect, Some(2.));
    assert!(corrected.crop[0] < 0. && corrected.crop[1] < 0.);
    current.geometry = corrected;
    current.validate().unwrap();
    let rendered = rawpuppy::pipeline::Pipeline::compile(&image, &current)
        .unwrap()
        .render(None)
        .unwrap();
    assert!(rendered.width.abs_diff(rendered.height * 2) <= 1);
    assert!(rendered.pixels.iter().all(|p| p[3] == 1.));
}

#[test]
#[ignore = "uses the local DSCF0027 RAF, or RAWPUPPY_TEST_LONDON"]
fn london_roof_is_reachable_above_the_original_frame() {
    let path = std::env::var_os("RAWPUPPY_TEST_LONDON")
        .unwrap_or_else(|| ".local-fixtures/DSCF0027.raf".into());
    let image = SensorImage::open(path.as_ref()).unwrap();
    let previous = Edits::for_image(&image);
    // Preserve the reported pose so this test isolates the crop from detection.
    let mut geometry = GeometryEdits {
        pitch: -11.142_692,
        yaw: -0.487_296_88,
        rotation: -0.730_587_5,
        field_of_view: 97.434_42,
        shift: [0.004_907_664, -0.115_215_376],
        ..previous.geometry.clone()
    };
    crop::after_perspective(&image, &previous, &mut geometry, None).unwrap();
    let mut expanded = previous;
    expanded.geometry = geometry;
    let pipeline = rawpuppy::pipeline::Pipeline::compile(&image, &expanded).unwrap();
    let forward = rawpuppy::color::inverse(pipeline.geometry.homography).unwrap();
    // The top of the left rooftop pavilion in the lens-corrected source.
    let corrected = rawpuppy::color::apply(forward, [0.29, 0.052, 1.]);
    let roof = [corrected[0] / corrected[2], corrected[1] / corrected[2]];
    assert!(
        roof[1] < 0.,
        "Roof was already inside the old canvas: {roof:?}"
    );
    let [x, y, w, h] = expanded.geometry.crop;
    let uv = [(roof[0] - x) / w, (roof[1] - y) / h];
    assert!(uv.into_iter().all(|v| (0. ..=1.).contains(&v)), "{uv:?}");
    assert_eq!(pipeline.sample(uv)[3], 1.);
}
