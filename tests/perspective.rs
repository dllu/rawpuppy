use rawpuppy::{
    edits::GeometryEdits,
    geometry::Geometry,
    perspective::{self, Mask, Mode},
    pipeline::Rendered,
};

fn canvas() -> Rendered {
    Rendered {
        width: 800,
        height: 640,
        pixels: vec![[0.025, 0.025, 0.025, 1.]; 800 * 640],
    }
}
fn line(image: &mut Rendered, a: [f32; 2], b: [f32; 2]) {
    let w = image.width as f32;
    let h = image.height as f32;
    let a = [a[0] * w, a[1] * h];
    let b = [b[0] * w, b[1] * h];
    let d = [b[0] - a[0], b[1] - a[1]];
    let length = d[0] * d[0] + d[1] * d[1];
    for y in 0..image.height {
        for x in 0..image.width {
            let p = [x as f32 + 0.5 - a[0], y as f32 + 0.5 - a[1]];
            let t = ((p[0] * d[0] + p[1] * d[1]) / length).clamp(0., 1.);
            let distance = (p[0] - t * d[0]).hypot(p[1] - t * d[1]);
            let weight = (2. - distance).clamp(0., 1.);
            if weight > 0. {
                let v = 0.025 + 0.8 * weight;
                image.pixels[y * image.width + x] = [v, v, v, 1.];
            }
        }
    }
}
fn facade(g: &GeometryEdits) -> Rendered {
    facade_size(g, 800, 640)
}
fn facade_size(g: &GeometryEdits, width: usize, height: usize) -> Rendered {
    let mut image = Rendered {
        width,
        height,
        pixels: vec![[0.025, 0.025, 0.025, 1.]; width * height],
    };
    let geometry = Geometry::compile(g, image.width, image.height).unwrap();
    for x in [0.13, 0.25, 0.38, 0.51, 0.64, 0.77, 0.89] {
        line(
            &mut image,
            geometry.map([x, -0.2], 1).unwrap(),
            geometry.map([x, 1.2], 1).unwrap(),
        );
    }
    for y in [0.08, 0.23, 0.38, 0.53, 0.68, 0.83, 0.97] {
        line(
            &mut image,
            geometry.map([-0.2, y], 1).unwrap(),
            geometry.map([1.2, y], 1).unwrap(),
        );
    }
    image
}
#[test]
fn recovers_tilt_and_roll_with_infinite_horizontal_vanishing_point() {
    let original = GeometryEdits {
        pitch: 14.,
        rotation: 5.,
        ..Default::default()
    };
    let image = facade(&original);
    let report =
        perspective::analyze(&image, None, &GeometryEdits::default(), Mode::Vertical).unwrap();
    let solution = report
        .solution
        .as_ref()
        .unwrap_or_else(|| panic!("{}; {} segments", report.reason, report.segments.len()));
    assert!(
        (solution.pitch - original.pitch).abs() < 0.3,
        "{solution:?}"
    );
    assert!(
        (solution.rotation - original.rotation).abs() < 0.3,
        "{solution:?}"
    );
    assert!(solution.yaw.abs() < 1e-6);
    let geometry = Geometry::compile(solution, 800, 640).unwrap();
    let center = geometry.map([0.5, 0.5], 1).unwrap();
    assert!(
        (center[0] - 0.5).abs() < 1e-6 && (center[1] - 0.5).abs() < 1e-6,
        "Center moved: {center:?}"
    );
}
#[test]
fn recovers_a_two_axis_pinhole_camera_and_handles_portrait_coordinates() {
    for truth in [
        GeometryEdits {
            yaw: 18.,
            pitch: 12.,
            rotation: -3.,
            ..Default::default()
        },
        GeometryEdits::default(),
    ] {
        for (width, height) in [(800, 640), (640, 800)] {
            let image = facade_size(&truth, width, height);
            let report =
                perspective::analyze(&image, None, &GeometryEdits::default(), Mode::Both).unwrap();
            let solution = report
                .solution
                .as_ref()
                .unwrap_or_else(|| panic!("{}; {} segments", report.reason, report.segments.len()));
            assert!((solution.pitch - truth.pitch).abs() < 0.5, "{solution:?}");
            assert!((solution.yaw - truth.yaw).abs() < 0.5, "{solution:?}");
            assert!(
                (solution.rotation - truth.rotation).abs() < 0.3,
                "{solution:?}"
            );
        }
    }
}
#[test]
fn circular_arcs_and_disconnected_clutter_do_not_force_a_correction() {
    let mut image = canvas();
    for (x, p) in image.pixels.iter_mut().enumerate() {
        let u = (x % 800) as f32 + 0.5 - 400.;
        let v = (x / 800) as f32 + 0.5 - 320.;
        let r = u.hypot(v);
        if [90., 140., 200., 260.]
            .iter()
            .any(|radius| (r - radius).abs() < 1.5)
        {
            *p = [0.8, 0.8, 0.8, 1.];
        }
    }
    let report =
        perspective::analyze(&image, None, &GeometryEdits::default(), Mode::Vertical).unwrap();
    assert!(
        report.solution.is_none(),
        "Arc tangents became architecture: {report:?}"
    );
    assert!(
        report.segments.len() < 4,
        "Curves passed the straightness checks"
    );
    let blank = canvas();
    assert!(
        perspective::analyze(&blank, None, &GeometryEdits::default(), Mode::Vertical)
            .unwrap()
            .solution
            .is_none()
    );
}
#[test]
fn masks_select_evidence_without_creating_edges_and_invalid_masks_are_rejected() {
    let image = facade(&GeometryEdits {
        pitch: 10.,
        ..Default::default()
    });
    let mask = Mask {
        width: 800,
        height: 640,
        values: vec![0.; 800 * 640],
    };
    let report = perspective::analyze(
        &image,
        Some(&mask),
        &GeometryEdits::default(),
        Mode::Vertical,
    )
    .unwrap();
    assert!(report.solution.is_none() && report.segments.is_empty());
    let mask = Mask {
        width: 800,
        height: 640,
        values: vec![1.; 800 * 640],
    };
    assert!(
        perspective::analyze(
            &canvas(),
            Some(&mask),
            &GeometryEdits::default(),
            Mode::Vertical
        )
        .unwrap()
        .segments
        .is_empty()
    );
    let mask = Mask {
        width: 800,
        height: 640,
        values: vec![f32::NAN; 800 * 640],
    };
    assert!(
        perspective::analyze(
            &image,
            Some(&mask),
            &GeometryEdits::default(),
            Mode::Vertical
        )
        .is_err()
    );
}
#[test]
fn old_recipes_keep_identical_serialized_geometry_until_a_shift_is_applied() {
    let geometry = GeometryEdits::default();
    let json = serde_json::to_string(&geometry).unwrap();
    assert!(!json.contains("shift"));
    assert_eq!(
        serde_json::from_str::<GeometryEdits>(&json).unwrap(),
        geometry
    );
    let shifted = GeometryEdits {
        shift: [0.1, -0.2],
        ..geometry
    };
    let json = serde_json::to_string(&shifted).unwrap();
    assert_eq!(
        serde_json::from_str::<GeometryEdits>(&json).unwrap(),
        shifted
    );
}

#[test]
fn camera_intrinsics_follow_the_equivalent_diagonal_and_oriented_aspect() {
    let landscape = perspective::horizontal_fov(16., 11648, 8736, 1.).unwrap();
    let portrait = perspective::horizontal_fov(16., 8736, 11648, 1.).unwrap();
    assert!((landscape - 94.5).abs() < 0.1);
    assert!((portrait - 78.1).abs() < 0.1);
    assert!(perspective::horizontal_fov(16., 11648, 8736, 0.9).unwrap() < landscape);
    assert!(perspective::horizontal_fov(f32::NAN, 800, 600, 1.).is_none());
    assert!(perspective::horizontal_fov(0., 800, 600, 1.).is_none());
}

#[test]
fn transparency_boundaries_are_not_architectural_evidence() {
    let mut image = canvas();
    image.pixels.fill([0.; 4]);
    for start in [75, 275, 475, 675] {
        for y in 60..560 {
            for x in start..start + 75 {
                image.pixels[y * 800 + x] = [0.8, 0.8, 0.8, 1.];
            }
        }
    }
    let report =
        perspective::analyze(&image, None, &GeometryEdits::default(), Mode::Vertical).unwrap();
    assert!(report.segments.is_empty() && report.solution.is_none());
}
