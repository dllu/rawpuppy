use rawpuppy::{
    edits::GeometryEdits,
    geometry::Geometry,
    perspective::{self, Mask, Mode, Segment},
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

fn family(
    point: [f64; 3],
    count: usize,
    length: f64,
    x_range: [f64; 2],
    noise: f64,
) -> Vec<Segment> {
    (0..count)
        .map(|i| {
            let x = x_range[0] + (x_range[1] - x_range[0]) * (i % 8) as f64 / 7.;
            let y = -0.23 + (i / 8) as f64 * 0.08;
            let d = [point[0] - x * point[2], point[1] - y * point[2]];
            let norm = d[0].hypot(d[1]);
            Segment {
                start: [x, y],
                end: [x + d[0] / norm * length, y + d[1] / norm * length],
                straightness: noise,
            }
        })
        .collect()
}

fn lean(solution: &GeometryEdits, segments: &[Segment], width: usize, height: usize) -> f64 {
    let inverse = rawpuppy::color::inverse(
        Geometry::compile(solution, width, height)
            .unwrap()
            .homography,
    )
    .unwrap();
    let aspect = height as f64 / width as f64;
    segments
        .iter()
        .map(|s| {
            let [a, b] = [s.start, s.end].map(|p| {
                let q = rawpuppy::color::apply(
                    inverse,
                    [(p[0] + 0.5) as f32, (p[1] / aspect + 0.5) as f32, 1.],
                );
                [
                    q[0] as f64 / q[2] as f64,
                    q[1] as f64 / q[2] as f64 * aspect,
                ]
            });
            (b[0] - a[0]).abs().atan2((b[1] - a[1]).abs()).to_degrees()
        })
        .fold(0., f64::max)
}

#[test]
fn steep_receding_facade_edges_cannot_outvote_the_actual_vertical_direction() {
    // Two independent homogeneous vanishing families, including a dominant
    // receding direction that looks vertical in a steep telephoto view.
    let vertical = family([0.007, 0.907, -0.421], 24, 0.14, [-0.4, 0.4], 0.2);
    let mut segments = vertical.clone();
    segments.extend(family([0.56, -0.789, -0.251], 48, 0.18, [-0.45, -0.1], 0.2));
    let base = GeometryEdits {
        field_of_view: 20.94,
        ..Default::default()
    };
    let report = perspective::fit_segments(segments, [960, 1440], &base, Mode::Vertical).unwrap();
    let solution = report
        .solution
        .unwrap_or_else(|| panic!("{}", report.reason));
    assert!(
        lean(&solution, &vertical, 960, 1440) < 0.001,
        "{solution:?}"
    );
}

#[test]
fn long_precise_edges_determine_the_fit_amid_short_noisy_window_fragments() {
    let accurate = family([0.014, 0.907, -0.419], 16, 0.24, [-0.4, 0.4], 0.15);
    let mut segments = accurate.clone();
    segments.extend(family([0.014, 0.907, -0.379], 80, 0.027, [-0.4, 0.4], 0.4));
    let base = GeometryEdits {
        field_of_view: 96.,
        ..Default::default()
    };
    let report = perspective::fit_segments(segments, [1440, 1080], &base, Mode::Vertical).unwrap();
    let solution = report
        .solution
        .unwrap_or_else(|| panic!("{}", report.reason));
    assert!(
        lean(&solution, &accurate, 1440, 1080) < 0.08,
        "{solution:?}"
    );
}

#[test]
fn analysis_is_invariant_to_exposure_colour_and_display_looks() {
    use rawpuppy::{
        edits::Edits,
        input::SensorImage,
        render::{Backend, Renderer},
    };
    use std::sync::Arc;
    let source = Arc::new(
        SensorImage::from_rgb(
            128,
            96,
            (0..128 * 96)
                .flat_map(|i| {
                    let x = (i % 128) as f32 / 128.;
                    let y = (i / 128) as f32 / 96.;
                    [x * x * 0.02, y * y * 0.01, 0.003]
                })
                .collect(),
        )
        .unwrap(),
    );
    let original = source.data.clone();
    let mut renderer = Renderer::new(Backend::Cpu);
    let mut edits = Edits::default();
    let before = perspective::preview(&mut renderer, source.clone(), &edits).unwrap();
    edits.scene.exposure = 8.;
    edits.scene.calibration = [3., 0.5, 0.3];
    edits.scene.vignette = [2., -1.];
    edits.scene.illuminant = rawpuppy::calibration::Illuminant::BlackBody { kelvin: 1800. };
    edits.tone.saturation = 0.;
    edits.display.curve = vec![[0., 0.], [0.4, 0.9], [1., 1.]];
    let after = perspective::preview(&mut renderer, source.clone(), &edits).unwrap();
    assert_eq!(before.pixels, after.pixels);
    assert_eq!(source.data, original);
}

#[test]
fn interrupted_window_columns_supply_a_precise_long_baseline() {
    let truth = GeometryEdits {
        pitch: -12.4,
        rotation: 0.8,
        field_of_view: 96.,
        ..Default::default()
    };
    let camera = Geometry::compile(&truth, 800, 640).unwrap();
    let mut image = canvas();
    for x in [0.12, 0.25, 0.38, 0.51, 0.64, 0.77, 0.9] {
        for row in 0..9 {
            let y = 0.05 + row as f32 * 0.1;
            line(
                &mut image,
                camera.map([x, y], 1).unwrap(),
                camera.map([x, y + 0.055], 1).unwrap(),
            );
        }
    }
    let report = perspective::analyze(
        &image,
        None,
        &GeometryEdits {
            field_of_view: 96.,
            ..Default::default()
        },
        Mode::Vertical,
    )
    .unwrap();
    let solution = report
        .solution
        .unwrap_or_else(|| panic!("{}", report.reason));
    assert!((solution.pitch - truth.pitch).abs() < 0.15, "{solution:?}");
    assert!(
        (solution.rotation - truth.rotation).abs() < 0.08,
        "{solution:?}"
    );
    assert!(
        report
            .segments
            .iter()
            .filter(|s| (s.end[1] - s.start[1]).abs() > 0.4)
            .count()
            >= 4,
        "Columns were not recovered from their separate jambs"
    );
}

#[test]
fn real_architectural_edge_regressions_agree_with_independent_reference_axes() {
    #[derive(serde::Deserialize)]
    struct Case {
        id: String,
        analysis_size: [usize; 2],
        geometry: GeometryEdits,
        segments: Vec<SegmentFixture>,
        reference_axis_segments: Vec<SegmentFixture>,
    }
    #[derive(serde::Deserialize)]
    struct SegmentFixture {
        start: [f64; 2],
        end: [f64; 2],
        straightness: f64,
    }
    impl From<SegmentFixture> for Segment {
        fn from(s: SegmentFixture) -> Self {
            Self {
                start: s.start,
                end: s.end,
                straightness: s.straightness,
            }
        }
    }
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("data/perspective-reference-lines.json")).unwrap();
    for case in cases {
        let report = perspective::fit_segments(
            case.segments.into_iter().map(Into::into).collect(),
            case.analysis_size,
            &case.geometry,
            Mode::Vertical,
        )
        .unwrap();
        let solution = report
            .solution
            .unwrap_or_else(|| panic!("{}: {}", case.id, report.reason));
        let reference: Vec<Segment> = case
            .reference_axis_segments
            .into_iter()
            .map(Into::into)
            .collect();
        // Export alignment is an independent, approximate oracle (not perfect
        // gravity truth). This catches the previous 6–26 degree wrong-axis fits.
        let discrepancy = lean(
            &solution,
            &reference,
            case.analysis_size[0],
            case.analysis_size[1],
        );
        assert!(
            discrepancy < 0.85,
            "{}: reference-axis discrepancy {discrepancy}°",
            case.id
        );
    }
}

#[cfg(feature = "gpu")]
#[test]
#[ignore = "requires Vulkan/Metal/CUDA compute"]
fn antialiased_linear_analysis_matches_cpu_on_the_selected_compute_backend() {
    use rawpuppy::{
        edits::Edits,
        input::SensorImage,
        render::{Backend, Renderer},
    };
    use std::sync::Arc;
    let source = Arc::new(
        SensorImage::from_rgb(
            127,
            93,
            (0..127 * 93)
                .flat_map(|i| {
                    let x = (i % 127) as f32 / 127.;
                    let y = (i / 127) as f32 / 93.;
                    [x * 2.5 - 0.12, y * 2. - 0.15, 0.4 + x * y]
                })
                .collect(),
        )
        .unwrap(),
    );
    let mut edits = Edits::default();
    edits.geometry.distortion = [0.04, -0.01];
    let cpu =
        perspective::preview(&mut Renderer::new(Backend::Cpu), source.clone(), &edits).unwrap();
    let backend = if std::env::var_os("RAWPUPPY_TEST_CUDA").is_some() {
        Backend::Cuda
    } else {
        Backend::Vulkan
    };
    let device = perspective::preview(&mut Renderer::new(backend), source, &edits).unwrap();
    assert_eq!((cpu.width, cpu.height), (device.width, device.height));
    for (a, b) in cpu.pixels.iter().zip(device.pixels) {
        for i in 0..4 {
            assert!((a[i] - b[i]).abs() < 0.0002, "{a:?} vs {b:?}");
        }
    }
}
