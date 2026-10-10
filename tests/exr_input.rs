use exr::prelude::*;
use rawpuppy::{export, input};

#[test]
fn exr_display_window_places_half_hdr_from_the_first_rgb_layer() {
    let directory = tempfile::tempdir().unwrap();
    let display_origin = Vec2(10, -10);
    for offset in [Vec2(2, 1), Vec2(-2, -1)] {
        let path = directory
            .path()
            .join(format!("offset-{}-{}.exr", offset.x(), offset.y()));
        let mut auxiliary = LayerAttributes::named("depth");
        auxiliary.layer_position = display_origin;
        let depth = Layer::new(
            (2, 2),
            auxiliary,
            Encoding::FAST_LOSSLESS,
            SpecificChannels::build()
                .with_channel("Z")
                .with_pixel_fn(|_| (42f32,)),
        );
        let mut attributes = LayerAttributes::named("photograph");
        attributes.layer_position = display_origin + offset;
        let rgb = Layer::new(
            (6, 4),
            attributes,
            Encoding::FAST_LOSSLESS,
            SpecificChannels::rgba(|p: Vec2<usize>| {
                (
                    f16::from_f32(1. + p.x() as f32 * 0.5),
                    f16::from_f32(-0.125 * p.y() as f32),
                    f16::from_f32(0.625 + (p.y() * 6 + p.x()) as f32 / 16.),
                    f16::from_f32(p.x() as f32 / 8.),
                )
            }),
        );
        let mut attributes = ImageAttributes::new(IntegerBounds::new(display_origin, (7, 6)));
        attributes.chromaticities = Some(export::SRGB_CHROMATICITIES);
        Image::empty(attributes)
            .with_layer(depth)
            .with_layer(rgb)
            .write()
            .to_file(&path)
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        let image = input::SensorImage::open(&path).unwrap();
        assert_eq!((image.metadata.width, image.metadata.height), (7, 6));
        assert_eq!((image.origin, image.active), ([0, 0], [7, 6]));
        assert_eq!(input::color_revision(&path).unwrap(), 0);
        let mut expected = vec![[0f32; 3]; 7 * 6];
        for y in 0..4 {
            for x in 0..6 {
                let dx = x + offset.x();
                let dy = y + offset.y();
                if (0..7).contains(&dx) && (0..6).contains(&dy) {
                    expected[dy as usize * 7 + dx as usize] = [
                        1. + x as f32 * 0.5,
                        -0.125 * y as f32,
                        0.625 + (y * 6 + x) as f32 / 16.,
                    ];
                }
            }
        }
        for (actual, expected) in image.data.as_chunks::<3>().0.iter().zip(expected) {
            assert!(
                actual
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| (a - b).abs() < 0.000008),
                "Windowed EXR mismatch {actual:?} vs {expected:?}"
            );
        }
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
}

#[test]
fn exr_distant_legal_windows_clip_pixels_outside_the_display() {
    let directory = tempfile::tempdir().unwrap();
    // OpenEXR's reference bounds are narrower than the signed storage type.
    let boundary = i32::MAX / 2 - 4;
    for (index, (display, data)) in [(-boundary, boundary), (boundary, -boundary)]
        .into_iter()
        .enumerate()
    {
        let path = directory.path().join(format!("far-window-{index}.exr"));
        let mut attributes = LayerAttributes::named("far photograph");
        attributes.layer_position = Vec2(data, data);
        let image = Image::new(
            ImageAttributes::new(IntegerBounds::new(Vec2(display, display), (2, 2))),
            Layer::new(
                (2, 2),
                attributes,
                Encoding::FAST_LOSSLESS,
                SpecificChannels::rgb(|_| (1.5f32, -0.125f32, 0.625f32)),
            ),
        );
        image.write().to_file(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let image = input::SensorImage::open(&path).unwrap();
        assert_eq!((image.metadata.width, image.metadata.height), (2, 2));
        assert_eq!(image.data, vec![0f32; 12]);
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
}

#[test]
fn exr_nonfinite_rgb_is_rejected_without_changing_the_source() {
    let directory = tempfile::tempdir().unwrap();
    for (index, invalid) in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY]
        .into_iter()
        .enumerate()
    {
        let path = directory.path().join(format!("invalid-{index}.exr"));
        Image::from_channels(
            (2, 1),
            SpecificChannels::rgb(move |_| (invalid, 0.18f32, 0.4f32)),
        )
        .write()
        .to_file(&path)
        .unwrap();
        let original = std::fs::read(&path).unwrap();
        let error = input::SensorImage::open(&path)
            .err()
            .expect("Nonfinite EXR pixels must fail to decode");
        assert!(error.to_string().contains("finite"), "{error:#}");
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
}

#[test]
fn exr_without_rgb_channels_reports_an_error_for_input_and_revision() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("depth-only.exr");
    Image::from_channels(
        (2, 1),
        SpecificChannels::build()
            .with_channel("Z")
            .with_pixel_fn(|_| (42f32,)),
    )
    .write()
    .to_file(&path)
    .unwrap();
    let original = std::fs::read(&path).unwrap();
    assert!(input::SensorImage::open(&path).is_err());
    assert!(input::color_revision(&path).is_err());
    assert_eq!(std::fs::read(path).unwrap(), original);
}
