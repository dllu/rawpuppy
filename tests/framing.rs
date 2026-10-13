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
