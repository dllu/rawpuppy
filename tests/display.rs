use rawpuppy::{
    color::OutputSpace,
    display::{self, Desktop, Encoder, Icc, Request},
    export,
    pipeline::Rendered,
};

#[cfg(target_os = "linux")]
#[test]
#[ignore = "run only in an owned managed Wayland compositor session"]
fn managed_wayland_uses_compositor_matching_and_rejects_double_icc_conversion() {
    assert_eq!(std::env::var("RAWPUPPY_TEST_MANAGED_WAYLAND").unwrap(), "1");
    let request = Request {
        desktop: Desktop::Wayland,
        ..Default::default()
    };
    let resolved = display::discover(&request).unwrap();
    assert!(resolved.icc.is_none());
    assert!(resolved.label.contains("compositor"));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("monitor.icc");
    std::fs::write(
        &path,
        export::profile(OutputSpace::Srgb).unwrap().icc().unwrap(),
    )
    .unwrap();
    let custom = Request {
        custom: Some(path),
        ..request
    };
    let error = display::discover(&custom).unwrap_err();
    assert!(error.to_string().contains("twice"));
}

#[test]
fn rgba_display_conversion_matches_rgb_reference_and_retains_alpha() {
    let bytes = export::profile(OutputSpace::DisplayP3)
        .unwrap()
        .icc()
        .unwrap();
    let profile = Icc::from_bytes(bytes.clone()).unwrap();
    let image = Rendered {
        width: 4,
        height: 1,
        pixels: vec![
            [0.18, 0.2, 0.5, 0.],
            [0.02, 0.1, 0.2, 0.25],
            [1., 0.1, 0.01, 0.75],
            [4., -0.2, 0.6, 1.],
        ],
    };
    let source = export::profile(OutputSpace::LinearSrgb).unwrap();
    let destination = lcms2::Profile::new_icc(&bytes).unwrap();
    let reference: lcms2::Transform<[f32; 3], [u8; 3]> = lcms2::Transform::new(
        &source,
        lcms2::PixelFormat::RGB_FLT,
        &destination,
        lcms2::PixelFormat::RGB_8,
        lcms2::Intent::RelativeColorimetric,
    )
    .unwrap();
    let mut rgb = [[0u8; 3]; 4];
    reference.transform_pixels(
        &image
            .pixels
            .iter()
            .map(|p| [p[0], p[1], p[2]])
            .collect::<Vec<_>>(),
        &mut rgb,
    );
    let mut encoder = Encoder::default();
    let result = encoder.encode(&image, Some(&profile)).unwrap();
    for (i, pixel) in result.as_chunks::<4>().0.iter().enumerate() {
        assert_eq!(&pixel[..3], &rgb[i]);
        assert_eq!(pixel[3], [0, 64, 191, 255][i]);
    }
    assert_eq!(result, encoder.encode(&image, Some(&profile)).unwrap());
    assert_eq!(
        encoder.encode(&image, None).unwrap(),
        export::rgba8(&image, OutputSpace::Srgb)
    );
}

#[test]
fn changed_custom_profile_is_detected_without_changing_photo_recipe() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("monitor.icc");
    std::fs::write(
        &path,
        export::profile(OutputSpace::Srgb).unwrap().icc().unwrap(),
    )
    .unwrap();
    let request = Request {
        desktop: Desktop::Other,
        monitor: None,
        custom: Some(path.clone()),
    };
    let first = display::discover(&request).unwrap();
    std::fs::write(
        &path,
        export::profile(OutputSpace::DisplayP3)
            .unwrap()
            .icc()
            .unwrap(),
    )
    .unwrap();
    let second = display::discover(&request).unwrap();
    assert_ne!(first.icc.unwrap().digest, second.icc.unwrap().digest);
    std::fs::write(&path, b"invalid profile").unwrap();
    assert!(display::discover(&request).is_err());
}

#[test]
fn monitor_selection_geometry_supports_negative_origins_and_large_coordinates() {
    assert_eq!(
        display::overlap([-3840, 0, 3840, 2160], [-3000, 100, 1200, 900]),
        1200 * 900
    );
    assert_eq!(display::overlap([0, 0, 100, 100], [100, 0, 100, 100]), 0);
    assert_eq!(
        display::overlap(
            [i32::MAX - 5000, 0, 4000, 3000],
            [i32::MAX - 4000, 0, 4000, 3000]
        ),
        3000 * 3000
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "only run with an owned Xvfb display and RAWPUPPY_TEST_ISOLATED_DISPLAY=1"]
fn x11_monitor_zero_profile_is_independent_of_randr_primary_selection() {
    use rawpuppy::display::Monitor;
    use x11rb::{
        connection::Connection,
        protocol::{
            randr::{ConnectionExt as _, MonitorInfo},
            xinerama::ConnectionExt as _,
            xproto::{AtomEnum, ConnectionExt as _},
        },
        wrapper::ConnectionExt as _,
    };
    assert_eq!(
        std::env::var("RAWPUPPY_TEST_ISOLATED_DISPLAY").as_deref(),
        Ok("1")
    );
    let (connection, screen) = x11rb::connect(None).unwrap();
    let s = &connection.setup().roots[screen];
    let root = s.root;
    let width = s.width_in_pixels / 2;
    let height = s.height_in_pixels;
    let output = connection
        .randr_get_screen_resources_current(root)
        .unwrap()
        .reply()
        .unwrap()
        .outputs[0];
    for (name, x, outputs, primary) in [
        (b"LEFT".as_slice(), 0, vec![output], false),
        (b"RIGHT".as_slice(), width as i16, vec![], false),
    ] {
        let atom = connection
            .intern_atom(false, name)
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        connection
            .randr_set_monitor(
                root,
                MonitorInfo {
                    name: atom,
                    primary,
                    automatic: false,
                    x,
                    y: 0,
                    width,
                    height,
                    width_in_millimeters: 200,
                    height_in_millimeters: 250,
                    outputs,
                },
            )
            .unwrap()
            .check()
            .unwrap();
    }
    let monitors = connection
        .xinerama_query_screens()
        .unwrap()
        .reply()
        .unwrap()
        .screen_info;
    assert_eq!(
        monitors.len(),
        2,
        "Virtual monitor setup did not expose Xinerama indexing"
    );
    let profiles = [
        export::profile(OutputSpace::DisplayP3)
            .unwrap()
            .icc()
            .unwrap(),
        export::profile(OutputSpace::AdobeRgb)
            .unwrap()
            .icc()
            .unwrap(),
    ];
    for (name, bytes) in [
        ("_ICC_PROFILE", &profiles[0]),
        ("_ICC_PROFILE_1", &profiles[1]),
    ] {
        let atom = connection
            .intern_atom(false, name.as_bytes())
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        connection
            .change_property8(
                x11rb::protocol::xproto::PropMode::REPLACE,
                root,
                atom,
                AtomEnum::CARDINAL,
                bytes,
            )
            .unwrap()
            .check()
            .unwrap();
    }
    let mut encoder = Encoder::default();
    let image = Rendered {
        width: 2,
        height: 1,
        pixels: vec![[0.8, 0.2, 0.01, 1.], [0.02, 0.1, 0.7, 0.5]],
    };
    let original = image.pixels.clone();
    let mut encoded = Vec::new();
    for i in [0, 1, 0] {
        let m = &monitors[i];
        let request = Request {
            desktop: Desktop::X11,
            custom: None,
            monitor: Some(Monitor {
                name: Some(if i == 0 { "LEFT" } else { "RIGHT" }.into()),
                rect: [
                    m.x_org.into(),
                    m.y_org.into(),
                    m.width.into(),
                    m.height.into(),
                ],
            }),
        };
        let resolved = display::discover(&request).unwrap();
        assert_eq!(
            resolved.icc.as_ref().map(|p| p.digest),
            Some(Icc::from_bytes(profiles[i].clone()).unwrap().digest),
            "Selected profile for Xinerama monitor {i} differs from its atom"
        );
        let converted = encoder.encode(&image, resolved.icc.as_ref()).unwrap();
        let expected = Encoder::default()
            .encode(&image, Some(&Icc::from_bytes(profiles[i].clone()).unwrap()))
            .unwrap();
        assert_eq!(
            converted, expected,
            "Moving between monitor profiles reused a stale transform"
        );
        encoded.push(converted);
    }
    assert_ne!(encoded[0], encoded[1]);
    assert_eq!(encoded[0], encoded[2]);
    assert_eq!(image.pixels, original);
    for name in [b"LEFT".as_slice(), b"RIGHT".as_slice()] {
        let atom = connection
            .intern_atom(true, name)
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        connection
            .randr_delete_monitor(root, atom)
            .unwrap()
            .check()
            .unwrap();
    }
    for name in [b"_ICC_PROFILE".as_slice(), b"_ICC_PROFILE_1".as_slice()] {
        let atom = connection
            .intern_atom(true, name)
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        connection
            .delete_property(root, atom)
            .unwrap()
            .check()
            .unwrap();
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "only run with an owned Xvfb display and RAWPUPPY_TEST_ISOLATED_DISPLAY=1"]
fn x11_profile_discovery_tracks_root_property_updates() {
    use rawpuppy::display::Monitor;
    use x11rb::{
        connection::Connection,
        protocol::xproto::{AtomEnum, ConnectionExt},
        wrapper::ConnectionExt as _,
    };
    assert_eq!(
        std::env::var("RAWPUPPY_TEST_ISOLATED_DISPLAY").as_deref(),
        Ok("1")
    );
    let (connection, screen) = x11rb::connect(None).unwrap();
    let root = connection.setup().roots[screen].root;
    let atom = connection
        .intern_atom(false, b"_ICC_PROFILE")
        .unwrap()
        .reply()
        .unwrap()
        .atom;
    let request = Request {
        desktop: Desktop::X11,
        monitor: Some(Monitor {
            name: Some("screen".into()),
            rect: [0, 0, 1440, 960],
        }),
        custom: None,
    };
    let first = export::profile(OutputSpace::Srgb).unwrap().icc().unwrap();
    connection
        .change_property8(
            x11rb::protocol::xproto::PropMode::REPLACE,
            root,
            atom,
            AtomEnum::CARDINAL,
            &first,
        )
        .unwrap()
        .check()
        .unwrap();
    assert_eq!(
        display::discover(&request).unwrap().icc.unwrap(),
        Icc::from_bytes(first).unwrap()
    );
    let second = export::profile(OutputSpace::DisplayP3)
        .unwrap()
        .icc()
        .unwrap();
    connection
        .change_property8(
            x11rb::protocol::xproto::PropMode::REPLACE,
            root,
            atom,
            AtomEnum::CARDINAL,
            &second,
        )
        .unwrap()
        .check()
        .unwrap();
    assert_eq!(
        display::discover(&request).unwrap().icc.unwrap(),
        Icc::from_bytes(second).unwrap()
    );
    connection
        .delete_property(root, atom)
        .unwrap()
        .check()
        .unwrap();
    assert!(display::discover(&request).unwrap().icc.is_none());
}
