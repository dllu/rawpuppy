use image::ImageDecoder;
use rawpuppy::{
    color::{self, OutputSpace},
    edits::{Edits, Retouch, RetouchMode, ToneMapper},
    export,
    geometry::Geometry,
    input::{SensorImage, pixel_count},
    pipeline::Pipeline,
    sidecar,
};

fn close(a: f32, b: f32, tolerance: f32) {
    assert!((a - b).abs() <= tolerance, "{a} != {b}");
}
fn linear_edits() -> Edits {
    let mut e = Edits::default();
    e.tone.mapper = ToneMapper::Linear;
    e
}
fn bayer(width: usize, height: usize, pattern: &str, rgb: [f32; 3]) -> SensorImage {
    let mut image = SensorImage::from_rgb(width, height, vec![0.; width * height * 3]).unwrap();
    let cfa = rawler::CFA::new(pattern);
    image.data = (0..width * height)
        .map(|i| rgb[cfa.color_at(i / width, i % width)])
        .collect();
    image.cpp = 1;
    image.cfa = Some(cfa);
    image
}

#[test]
fn sensor_reconstruction_preserves_constants_in_every_bayer_layout() {
    for pattern in ["RGGB", "BGGR", "GRBG", "GBRG"] {
        let image = bayer(17, 19, pattern, [0.2, 0.4, 0.7]);
        for y in 0..19 {
            for x in 0..17 {
                let rgb = image.reconstruct(x, y, &Default::default());
                for i in 0..3 {
                    close(rgb[i], [0.2, 0.4, 0.7][i], 2e-7);
                }
            }
        }
    }
}

#[test]
fn hot_pixel_correction_preserves_source_and_removes_isolated_outlier() {
    let mut image = bayer(13, 13, "RGGB", [0.1; 3]);
    image.data[6 * 13 + 6] = 1.;
    let e = rawpuppy::edits::RawEdits {
        hot_pixels: true,
        denoise: 0.,
        ..Default::default()
    };
    for v in image.reconstruct(6, 6, &e) {
        close(v, 0.1, 1e-6);
    }
    assert_eq!(image.data[6 * 13 + 6], 1.);
}

#[test]
fn reconstruction_methods_keep_legacy_json_and_require_matching_sources() {
    use rawpuppy::edits::Reconstruction;
    let raw: rawpuppy::edits::RawEdits =
        serde_json::from_str(r#"{"hot_pixels":false,"denoise":0.0}"#).unwrap();
    assert_eq!(
        serde_json::to_string(&raw).unwrap(),
        r#"{"hot_pixels":false,"denoise":0.0}"#
    );
    let source = bayer(16, 16, "RGGB", [0.2; 3]);
    let mut edits = Edits::default();
    edits.raw.reconstruction = Reconstruction::RawNindV1;
    assert!(Pipeline::compile(&source, &edits).is_err());
    let roundtrip: Edits = serde_json::from_str(&serde_json::to_string(&edits).unwrap()).unwrap();
    assert_eq!(roundtrip.raw.reconstruction, Reconstruction::RawNindV1);
}

#[test]
fn no_16_bit_dimension_or_pixel_index_limit() {
    let width = 100_003;
    let image = SensorImage::from_rgb(width, 2, vec![0.18; width * 2 * 3]).unwrap();
    let edits = linear_edits();
    let p = Pipeline::compile(&image, &edits).unwrap();
    let rendered = p.render(None).unwrap();
    assert_eq!((rendered.width, rendered.height), (width, 2));
    close(rendered.pixels[width - 1][0], 0.18, 1e-6);
    assert!(pixel_count(usize::MAX, 2, 3).is_err());
}

#[test]
fn pinhole_identity_crop_and_rotation_have_known_coordinates() {
    let mut e = Edits::default();
    let g = Geometry::compile(&e.geometry, 1000, 500).unwrap();
    for uv in [[0., 0.], [1., 1.], [0.3, 0.7]] {
        let p = g.map(uv, 1).unwrap();
        for i in 0..2 {
            close(p[i], uv[i], 1e-6);
        }
    }
    e.geometry.crop = [0.2, 0.3, 0.4, 0.5];
    let g = Geometry::compile(&e.geometry, 1000, 500).unwrap();
    assert_eq!((g.width, g.height), (400, 250));
    let center = g.map([0.5, 0.5], 1).unwrap();
    close(center[0], 0.4, 1e-6);
    close(center[1], 0.55, 1e-6);
    e.geometry.crop = [0., 0., 1., 1.];
    e.geometry.rotation = 180.;
    let g = Geometry::compile(&e.geometry, 1000, 500).unwrap();
    let p = g.map([0.1, 0.2], 1).unwrap();
    close(p[0], 0.9, 1e-6);
    close(p[1], 0.8, 1e-6);
}

#[test]
fn camera_orientation_uses_all_eight_exif_transforms() {
    let mut image =
        SensorImage::from_rgb(3, 2, (0..6).flat_map(|i| [i as f32; 3]).collect()).unwrap();
    let expected = [0., 2., 5., 3., 0., 3., 5., 2.];
    for (i, expect) in expected.into_iter().enumerate() {
        image.orientation = rawler::Orientation::from_u16(i as u16 + 1);
        let (t, _, _) = image.orientation.to_flips();
        let (w, h) = if t { (2., 3.) } else { (3., 2.) };
        let rgb = image
            .sample([0.5 / w, 0.5 / h], &Default::default())
            .unwrap();
        close(rgb[0], expect, 1e-6);
    }
}

#[test]
fn exposure_preserves_unbounded_scene_highlights() {
    let image = SensorImage::from_rgb(2, 2, vec![0.8; 12]).unwrap();
    let mut e = linear_edits();
    e.scene.exposure = 3.;
    let p = Pipeline::compile(&image, &e).unwrap().sample([0.5; 2]);
    close(p[0], 6.4, 1e-5);
    assert_eq!(p[3], 1.);
    e.geometry.rotation = 45.;
    assert_eq!(
        Pipeline::compile(&image, &e).unwrap().sample([0., 0.])[3],
        0.
    );
}

#[test]
fn viewport_actual_pixels_matches_full_resolution_without_an_intermediate() {
    let source = SensorImage::from_rgb(
        16,
        12,
        (0..192).flat_map(|i| [i as f32 / 192.; 3]).collect(),
    )
    .unwrap();
    let e = linear_edits();
    let p = Pipeline::compile(&source, &e).unwrap();
    let full = p.render(None).unwrap();
    let view = p
        .render_region([4. / 16., 3. / 12., 8. / 16., 6. / 12.], 8, 6)
        .unwrap();
    for y in 0..6 {
        for x in 0..8 {
            for c in 0..4 {
                close(
                    view.pixels[y * 8 + x][c],
                    full.pixels[(y + 3) * 16 + x + 4][c],
                    1e-6,
                );
            }
        }
    }
}

#[test]
fn agx_is_finite_neutral_and_compresses_extreme_intensities() {
    let mut prev = 0.;
    for i in -30..=30 {
        let p = color::agx([2f32.powi(i); 3]);
        assert!(p.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
        close(p[0], p[1], 0.002);
        close(p[1], p[2], 0.002);
        assert!(p[0] >= prev - 0.0001);
        prev = p[0];
    }
    assert!(color::agx([-1., 0., 1.]).iter().all(|x| x.is_finite()));
}

#[test]
fn monotone_curve_has_no_overshoot_and_srgb_roundtrips() {
    let curve = color::curve_lut(&[[0., 0.], [0.1, 0.7], [0.9, 0.8], [1., 1.]], 4096);
    assert!(curve.windows(2).all(|p| p[1] >= p[0]));
    for i in 0..1024 {
        let v = i as f32 / 1023.;
        close(color::srgb_decode(color::srgb_encode(v)), v, 2e-6);
    }
}

#[test]
fn clone_replaces_target_after_tone_mapping_without_changing_source() {
    let image =
        SensorImage::from_rgb(8, 2, (0..16).flat_map(|i| [i as f32 / 16.; 3]).collect()).unwrap();
    let mut edits = linear_edits();
    edits.display.retouch.push(Retouch {
        source: [0.1875, 0.25],
        target: [0.8125, 0.25],
        radius: 0.15,
        feather: 0.5,
        opacity: 1.,
        mode: RetouchMode::Clone,
    });
    let p = Pipeline::compile(&image, &edits)
        .unwrap()
        .sample([0.8125, 0.25]);
    close(p[0], 1. / 16., 1e-6);
    assert_eq!(image.data[6 * 3], 6. / 16.);
}

#[test]
fn xmp_roundtrip_is_namespaced_atomic_and_darktable_independent() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("photo.raf");
    let darktable = dir.path().join("photo.raf.xmp");
    std::fs::write(&darktable, b"keep me").unwrap();
    let path = sidecar::path_for(&original);
    let mut edits = Edits::default();
    edits.scene.exposure = -2.;
    sidecar::save(&path, &edits).unwrap();
    assert_eq!(sidecar::load(&path).unwrap(), edits);
    assert_eq!(std::fs::read(&darktable).unwrap(), b"keep me");
    let xml = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        xml.replace(sidecar::NAMESPACE, "https://example.com/impostor"),
    )
    .unwrap();
    assert!(sidecar::load(&path).is_err());
    edits.version = 999;
    assert!(sidecar::save(&path, &edits).is_err());
}

#[test]
fn invalid_recipes_are_rejected_before_processing() {
    let mut e = Edits::default();
    e.geometry.crop[2] = 0.;
    assert!(e.validate().is_err());
    e = Edits::default();
    e.scene.exposure = f32::NAN;
    assert!(e.validate().is_err());
    e = Edits::default();
    e.display.curve = vec![[0., 0.], [0.5, 1.], [1., 0.8]];
    assert!(e.validate().is_err());
}

#[test]
fn exported_files_have_valid_color_profiles_and_no_clobber() {
    let dir = tempfile::tempdir().unwrap();
    let image = SensorImage::from_rgb(3, 2, vec![0.18; 18]).unwrap();
    let e = linear_edits();
    let rendered = Pipeline::compile(&image, &e).unwrap().render(None).unwrap();
    for space in [
        OutputSpace::Srgb,
        OutputSpace::DisplayP3,
        OutputSpace::AdobeRgb,
        OutputSpace::Rec2020,
    ] {
        for ext in ["png", "tiff", "jpg"] {
            let path = dir.path().join(format!("{space:?}.{ext}"));
            export::write(&path, &rendered, space, false).unwrap();
            let icc = if ext == "tiff" {
                let mut decoder =
                    tiff::decoder::Decoder::new(std::fs::File::open(&path).unwrap()).unwrap();
                assert_eq!(decoder.dimensions().unwrap(), (3, 2));
                decoder.get_tag_u8_vec(tiff::tags::Tag::IccProfile).unwrap()
            } else {
                let mut decoder = image::ImageReader::open(&path)
                    .unwrap()
                    .into_decoder()
                    .unwrap();
                assert_eq!(decoder.dimensions(), (3, 2));
                decoder.icc_profile().unwrap().unwrap()
            };
            assert!(lcms2::Profile::new_icc(&icc).is_ok());
            assert!(export::write(&path, &rendered, space, false).is_err());
            let reloaded = SensorImage::open(&path).unwrap();
            close(
                reloaded.data[0],
                0.18,
                if ext == "jpg" { 0.005 } else { 0.001 },
            );
        }
    }
}

#[test]
fn exr_roundtrips_hdr_values_and_alpha() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hdr.exr");
    let rendered = rawpuppy::pipeline::Rendered {
        width: 2,
        height: 1,
        pixels: vec![[4., 2., -0.1, 1.], [1., 0., 0., 0.]],
    };
    export::write(&path, &rendered, OutputSpace::LinearSrgb, false).unwrap();
    let decoded = image::open(&path).unwrap().to_rgba32f();
    for (got, expected) in decoded
        .as_raw()
        .iter()
        .zip(rendered.pixels.iter().flatten())
    {
        close(*got, *expected, 1e-6);
    }
}

#[test]
fn tiff_exports_identify_and_preserve_straight_alpha() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("alpha.tiff");
    let image = rawpuppy::pipeline::Rendered {
        width: 3,
        height: 1,
        pixels: vec![
            [0.8, 0.2, 0.05, 0.],
            [0.25, 0.5, 0.75, 0.25],
            [0.9, 0.1, 0.2, 1.],
        ],
    };
    export::write(&path, &image, OutputSpace::LinearSrgb, false).unwrap();
    let mut decoder = tiff::decoder::Decoder::new(std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(
        decoder
            .get_tag_u16_vec(tiff::tags::Tag::ExtraSamples)
            .unwrap(),
        vec![tiff::tags::ExtraSamples::UnassociatedAlpha.to_u16()]
    );
    assert!(
        lcms2::Profile::new_icc(&decoder.get_tag_u8_vec(tiff::tags::Tag::IccProfile).unwrap())
            .is_ok()
    );
    let tiff::decoder::DecodingResult::U16(values) = decoder.read_image().unwrap() else {
        panic!("RGBA16 TIFF expected");
    };
    for (value, reference) in values.iter().zip(image.pixels.iter().flatten()) {
        assert!((*value as f32 / 65535. - reference).abs() < 1.0 / 65535. + 1e-7);
    }
    let decoded = image::ImageReader::open(path)
        .unwrap()
        .decode()
        .unwrap()
        .to_rgba16();
    assert_eq!(decoded.as_raw(), &values);
}
