use rawpuppy::{
    color,
    edits::{Edits, ToneMapper},
    export,
    input::SensorImage,
    pipeline::{Pipeline, Rendered},
};

struct RgbHalf;
impl tiff::encoder::colortype::ColorType for RgbHalf {
    // TIFF stores binary16 samples using their 16-bit representations.
    type Inner = u16;
    const TIFF_VALUE: tiff::tags::PhotometricInterpretation =
        tiff::tags::PhotometricInterpretation::RGB;
    const BITS_PER_SAMPLE: &'static [u16] = &[16; 3];
    const SAMPLE_FORMAT: &'static [tiff::tags::SampleFormat] =
        &[tiff::tags::SampleFormat::IEEEFP; 3];
    fn horizontal_predict(_: &[u16], _: &mut Vec<u16>) {
        unreachable!()
    }
}

fn write_float_tiff(path: &std::path::Path, bits: u16, values: &[f32], icc: Option<&[u8]>) {
    fn encode<C: tiff::encoder::colortype::ColorType>(
        path: &std::path::Path,
        values: &[C::Inner],
        icc: Option<&[u8]>,
    ) where
        [C::Inner]: tiff::encoder::TiffValue,
    {
        let mut encoder =
            tiff::encoder::TiffEncoder::new(std::fs::File::create(path).unwrap()).unwrap();
        let mut image = encoder
            .new_image::<C>((values.len() / 3) as u32, 1)
            .unwrap();
        if let Some(icc) = icc {
            image
                .encoder()
                .write_tag(tiff::tags::Tag::IccProfile, icc)
                .unwrap();
        }
        image.write_data(values).unwrap();
    }
    match bits {
        16 => encode::<RgbHalf>(
            path,
            &values
                .iter()
                .map(|v| half::f16::from_f32(*v).to_bits())
                .collect::<Vec<_>>(),
            icc,
        ),
        32 => encode::<tiff::encoder::colortype::RGB32Float>(path, values, icc),
        64 => encode::<tiff::encoder::colortype::RGB64Float>(
            path,
            &values.iter().map(|v| f64::from(*v)).collect::<Vec<_>>(),
            icc,
        ),
        _ => unreachable!(),
    }
}

#[test]
fn float_tiff_hdr_survives_import_exposure_and_exr_export() {
    let directory = tempfile::tempdir().unwrap();
    let values = vec![-0.125, 0.25, 2., 4., 0.5, 0.0625, 0.18, 0.18, 0.18];
    for bits in [16, 32, 64] {
        let path = directory.path().join(format!("linear-{bits}.tiff"));
        write_float_tiff(&path, bits, &values, None);
        let original = std::fs::read(&path).unwrap();
        let source = SensorImage::open(&path).unwrap();
        let expected: Vec<_> = values
            .iter()
            .map(|v| {
                if bits == 16 {
                    half::f16::from_f32(*v).to_f32()
                } else {
                    *v
                }
            })
            .collect();
        assert_eq!(source.metadata.bits, bits as usize);
        assert_eq!(source.data, expected);
        assert!(!source.raw_integer);
        let mut edits = Edits::for_image(&source);
        edits.tone.mapper = ToneMapper::Linear;
        edits.scene.exposure = 1.;
        let output = Pipeline::compile(&source, &edits)
            .unwrap()
            .render(None)
            .unwrap();
        for (pixel, reference) in output.pixels.iter().zip(expected.as_chunks::<3>().0) {
            for c in 0..3 {
                assert!((pixel[c] - reference[c] * 2.).abs() < 0.000003);
            }
            assert_eq!(pixel[3], 1.);
        }
        let exported = directory.path().join(format!("edited-{bits}.exr"));
        export::write(&exported, &output, color::OutputSpace::LinearSrgb, false).unwrap();
        let reloaded = SensorImage::open(&exported).unwrap();
        for (actual, reference) in reloaded.data.iter().zip(&expected) {
            assert!((actual - reference * 2.).abs() < 0.000003);
        }
        let recipe = rawpuppy::sidecar::path_for(&path);
        rawpuppy::sidecar::save(&recipe, &edits).unwrap();
        let saved_recipe = std::fs::read(&recipe).unwrap();
        let cli_export = directory.path().join(format!("cli-{bits}.exr"));
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_rawpuppy"))
            .args(["--backend", "cpu", "export"])
            .arg(&path)
            .arg(&cli_export)
            .args(["--color-space", "linear-srgb"])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let cli = SensorImage::open(&cli_export).unwrap();
        assert_eq!(cli.data, reloaded.data);
        assert_eq!(std::fs::read(recipe).unwrap(), saved_recipe);
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
}

#[test]
fn float_tiff_rejects_nonfinite_or_unrepresentable_working_values() {
    let directory = tempfile::tempdir().unwrap();
    for bits in [16, 32, 64] {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let path = directory.path().join("invalid.tiff");
            write_float_tiff(&path, bits, &[value, 0.25, 0.5], None);
            let original = std::fs::read(&path).unwrap();
            let error = SensorImage::open(&path).err().unwrap();
            assert!(error.to_string().contains("finite"));
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }
    }
    let path = directory.path().join("overflow.tiff");
    let mut encoder =
        tiff::encoder::TiffEncoder::new(std::fs::File::create(&path).unwrap()).unwrap();
    encoder
        .write_image::<tiff::encoder::colortype::RGB64Float>(1, 1, &[f64::MAX, 0.25, 0.5])
        .unwrap();
    drop(encoder);
    assert!(
        SensorImage::open(path.as_path())
            .err()
            .unwrap()
            .to_string()
            .contains("finite")
    );
}

#[test]
fn grayscale_float_tiff_expands_linear_hdr_without_a_transfer_curve() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("gray-hdr.tiff");
    let values = [-0.125f64, 0.25, 2., 0.18];
    let mut encoder =
        tiff::encoder::TiffEncoder::new(std::fs::File::create(&path).unwrap()).unwrap();
    encoder
        .write_image::<tiff::encoder::colortype::Gray64Float>(4, 1, &values)
        .unwrap();
    drop(encoder);
    let source = SensorImage::open(&path).unwrap();
    assert_eq!(source.metadata.bits, 64);
    for (pixel, value) in source.data.as_chunks::<3>().0.iter().zip(values) {
        assert_eq!(*pixel, [value as f32; 3]);
    }
}

#[test]
fn wide_gamut_float_tiff_icc_retains_signed_values_and_highlights() {
    let directory = tempfile::tempdir().unwrap();
    let xy = |x, y| lcms2::CIExyY { x, y, Y: 1. };
    let primaries = lcms2::CIExyYTRIPLE {
        Red: xy(0.708, 0.292),
        Green: xy(0.170, 0.797),
        Blue: xy(0.131, 0.046),
    };
    let curve = lcms2::ToneCurve::new(1.);
    let icc = lcms2::Profile::new_rgb(&xy(0.3127, 0.329), &primaries, &[&curve; 3])
        .unwrap()
        .icc()
        .unwrap();
    let values = [0., 1., 0., 2., 0.25, -0.125, 0.18, 0.18, 0.18];
    let matrix = color::multiply(
        color::inverse(color::SRGB_TO_XYZ).unwrap(),
        color::REC2020_TO_XYZ,
    );
    for bits in [16, 32, 64] {
        let path = directory.path().join(format!("rec2020-{bits}.tiff"));
        write_float_tiff(&path, bits, &values, Some(&icc));
        let source = SensorImage::open(&path).unwrap();
        for (p, actual) in values
            .as_chunks::<3>()
            .0
            .iter()
            .zip(source.data.as_chunks::<3>().0)
        {
            let p = p.map(|v| {
                if bits == 16 {
                    half::f16::from_f32(v).to_f32()
                } else {
                    v
                }
            });
            let expected = color::apply(matrix, p);
            for c in 0..3 {
                assert!(
                    (actual[c] - expected[c]).abs() < 0.0002,
                    "{bits}-bit ICC: {actual:?} vs {expected:?}"
                );
            }
        }
        assert!(source.data.iter().any(|v| *v < 0.));
        assert!(source.data.iter().any(|v| *v > 1.));
    }
}

#[test]
fn grayscale_icc_inputs_preserve_their_declared_tone_curve() {
    use image::ImageEncoder;
    let directory = tempfile::tempdir().unwrap();
    let curve = lcms2::ToneCurve::new(2.2);
    let white = lcms2::CIExyY {
        x: 0.3457,
        y: 0.3585,
        Y: 1.,
    };
    let profile = lcms2::Profile::new_gray(&white, &curve)
        .unwrap()
        .icc()
        .unwrap();
    let width = 100_003u32;
    let samples: Vec<_> = (0..width)
        .map(|i| [0u16, 16384, 32768, 65535][i as usize % 4])
        .collect();
    for extension in ["png", "tiff"] {
        let path = directory.path().join(format!("gray.{extension}"));
        let file = std::fs::File::create(&path).unwrap();
        if extension == "png" {
            let mut encoder = image::codecs::png::PngEncoder::new(file);
            encoder.set_icc_profile(profile.clone()).unwrap();
            encoder
                .write_image(
                    bytemuck::cast_slice(&samples),
                    width,
                    1,
                    image::ExtendedColorType::L16,
                )
                .unwrap();
        } else {
            let mut encoder = tiff::encoder::TiffEncoder::new(file).unwrap();
            let mut image = encoder
                .new_image::<tiff::encoder::colortype::Gray16>(width, 1)
                .unwrap();
            image
                .encoder()
                .write_tag(tiff::tags::Tag::IccProfile, profile.as_slice())
                .unwrap();
            image.write_data(&samples).unwrap();
        }
        let original = std::fs::read(&path).unwrap();
        let decoded = SensorImage::open(&path).unwrap();
        assert_eq!(
            (decoded.metadata.width, decoded.metadata.height),
            (width as usize, 1)
        );
        for (sample, pixel) in samples.iter().zip(decoded.data.as_chunks::<3>().0) {
            let expected = (*sample as f32 / 65535.).powf(2.2);
            assert!(
                pixel.iter().all(|v| (*v - expected).abs() < 0.00003),
                "{extension} gray profile mismatch: {pixel:?} vs {expected}"
            );
        }
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
    let invalid = directory.path().join("color-with-gray-icc.png");
    let mut encoder = image::codecs::png::PngEncoder::new(std::fs::File::create(&invalid).unwrap());
    encoder.set_icc_profile(profile).unwrap();
    encoder
        .write_image(&[200, 40, 80], 1, 1, image::ExtendedColorType::Rgb8)
        .unwrap();
    assert!(
        SensorImage::open(&invalid)
            .err()
            .unwrap()
            .to_string()
            .contains("grayscale raster")
    );
}

#[test]
fn primary_matrices_and_white_adaptation_match_standard_colorimetry() {
    let srgb = color::primaries_to_xyz([[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]], [0.3127, 0.329])
        .unwrap();
    let rec = color::primaries_to_xyz(
        [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]],
        [0.3127, 0.329],
    )
    .unwrap();
    for (a, b) in srgb
        .into_iter()
        .flatten()
        .zip(color::SRGB_TO_XYZ.into_iter().flatten())
    {
        assert!((a - b).abs() < 0.000002);
    }
    for (a, b) in rec
        .into_iter()
        .flatten()
        .zip(color::REC2020_TO_XYZ.into_iter().flatten())
    {
        assert!((a - b).abs() < 0.000002);
    }
    let prophoto = color::rgb_primaries_to_working(
        [[0.7347, 0.2653], [0.1596, 0.8404], [0.0366, 0.0001]],
        [0.3457, 0.3585],
    )
    .unwrap();
    for v in color::apply(prophoto, [0.18; 3]) {
        assert!((v - 0.18).abs() < 0.000002);
    }
}

#[test]
fn tagged_hdr_exr_import_preserves_wide_gamut_and_unbounded_values() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("wide.exr");
    let values = [
        [0.18; 4],
        [2., 0.3, -0.01, 1.],
        [0.1, 0.9, 0.2, 1.],
        [0.4, 0.01, 3., 1.],
    ];
    let image = Rendered {
        width: 2,
        height: 2,
        pixels: values.to_vec(),
    };
    let chroma = exr::meta::attribute::Chromaticities {
        red: exr::math::Vec2(0.708, 0.292),
        green: exr::math::Vec2(0.170, 0.797),
        blue: exr::math::Vec2(0.131, 0.046),
        white: exr::math::Vec2(0.3127, 0.329),
    };
    export::write_linear_exr(
        &image,
        std::io::BufWriter::new(std::fs::File::create(&path).unwrap()),
        chroma,
    )
    .unwrap();
    let source = SensorImage::open(&path).unwrap();
    let m = color::multiply(
        color::inverse(color::SRGB_TO_XYZ).unwrap(),
        color::REC2020_TO_XYZ,
    );
    for (p, actual) in values.iter().zip(source.data.as_chunks::<3>().0.iter()) {
        let expected = color::apply(m, [p[0], p[1], p[2]]);
        for c in 0..3 {
            assert!((expected[c] - actual[c]).abs() < 0.000008);
        }
    }
    assert!(source.data.iter().any(|v| *v < 0.));
    assert!(source.data.iter().any(|v| *v > 1.));
    assert_eq!(source.metadata.color_revision, 1);
    let mut layers = rawpuppy::synthesis::Layers::new(path.clone());
    let (asset, sha256) = layers.store(&image).unwrap();
    let mut edits = Edits::default();
    let fill = rawpuppy::synthesis::GeneratedFill {
        region: [0., 0., 1., 1.],
        dabs: vec![rawpuppy::synthesis::MaskDab {
            center: [0.5; 2],
            radius: 1.,
        }],
        fill_gaps: false,
        steps: 10,
        seed: 0,
        asset,
        sha256,
        source_sha256: layers.source_hash().unwrap().into(),
        source_color_revision: 0,
        recipe_sha256: rawpuppy::synthesis::recipe_hash(&edits).unwrap(),
        sampling: Default::default(),
        model: "moebius-scene-2026-v1".into(),
    };
    edits.display.synthesis.push(fill);
    let mut raster = Rendered {
        width: 2,
        height: 2,
        pixels: vec![[0.18; 4]; 4],
    };
    assert!(layers.apply(&edits, &mut raster, [0., 0., 1., 1.]).is_err());
    assert_eq!(raster.pixels, vec![[0.18; 4]; 4]);
    edits.display.synthesis[0].source_color_revision = 1;
    layers.apply(&edits, &mut raster, [0., 0., 1., 1.]).unwrap();
    let exported = directory.path().join("linear.exr");
    export::write(&exported, &image, color::OutputSpace::LinearSrgb, false).unwrap();
    let metadata = exr::meta::MetaData::read_from_file(exported, false).unwrap();
    assert_eq!(
        metadata.headers[0].shared_attributes.chromaticities,
        Some(export::SRGB_CHROMATICITIES)
    );
}

#[test]
fn photographic_agx_preserves_gray_and_matches_independent_oracle_samples() {
    for c in color::agx([0.18; 3]) {
        assert!((c - 0.18).abs() < 0.000003);
    }
    let cases: Vec<([f32; 3], [f32; 3])> =
        serde_json::from_str(include_str!("data/agx-oracle.json")).unwrap();
    let mut sum = 0f64;
    for (input, reference) in &cases {
        let actual = color::agx(*input);
        for c in 0..3 {
            let error = (actual[c] - reference[c]).abs();
            assert!(
                error < 0.01,
                "AgX oracle mismatch {input:?}: {actual:?} vs {reference:?}"
            );
            sum += error as f64;
        }
    }
    assert!(sum / ((cases.len() * 3) as f64) < 0.0008);
}

#[test]
fn old_agx_recipes_keep_their_original_transform() {
    let edits: Edits =
        serde_json::from_str(r#"{"tone":{"mapper":"agx","saturation":1.0}}"#).unwrap();
    assert_eq!(edits.tone.mapper, ToneMapper::Agx);
    let source = SensorImage::from_rgb(2, 2, vec![0.18; 12]).unwrap();
    let actual = Pipeline::compile(&source, &edits).unwrap().sample([0.5; 2]);
    for (actual, expected) in actual[..3].iter().zip([0.21446736, 0.21453233, 0.21453686]) {
        assert!((*actual - expected).abs() < 0.000002);
    }
    assert_eq!(Edits::default().tone.mapper, ToneMapper::AgxSdr);
    assert!(
        serde_json::to_string(&Edits::default())
            .unwrap()
            .contains("agx_sdr_v1")
    );
}
