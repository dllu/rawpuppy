//! Synthetic DNG color/noise controls for independent RAW pipeline comparisons.
//! This product includes DNG technology under license by Adobe.
use anyhow::{Result, ensure};
use clap::Parser;
use rawpuppy::{
    color,
    edits::{Edits, ToneMapper},
    export,
    input::SensorImage,
    models,
    pipeline::Pipeline,
};
use std::{
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};
use tiff::{
    encoder::{Rational, SRational, TiffEncoder, colortype::Gray16},
    tags::Tag,
};

#[derive(Parser)]
struct Args {
    output: PathBuf,
    /// Fixture width; the nine patch regions scale with the image dimensions.
    #[arg(long, default_value_t = WIDTH)]
    width: usize,
    #[arg(long, default_value_t = HEIGHT)]
    height: usize,
    /// Use an existing camera's equivalent normalized calibration and as-shot gains.
    #[arg(long)]
    like_camera: Option<PathBuf>,
    /// Directory containing constant-darktable.exr and noisy-darktable.exr controls.
    #[arg(long)]
    reference_dir: Option<PathBuf>,
    /// Add a distinct Standard-A matrix and select its known warm neutral.
    #[arg(long)]
    dual_warm: bool,
    /// Add a third, custom-xy calibration with a distinct matrix and neutral.
    #[arg(long, conflicts_with = "custom_spectrum")]
    triple: bool,
    /// Declare an equal-energy spectrum for the first calibration illuminant.
    #[arg(long, conflicts_with_all=["triple","dual_warm"])]
    custom_spectrum: bool,
    /// Encode the selected white balance using AsShotWhiteXY instead of neutral.
    #[arg(long)]
    white_xy: bool,
}

const BLOCK: usize = 256;
const WIDTH: usize = BLOCK * 3;
const HEIGHT: usize = BLOCK * 3;
const BLACK: u16 = 512;
const WHITE: u16 = 15360;
const COLORS: [[f32; 3]; 9] = [
    [0.; 3],
    [0.005; 3],
    [0.02; 3],
    [0.18; 3],
    [0.18, 0.04, 0.02],
    [0.02, 0.18, 0.04],
    [0.04, 0.02, 0.18],
    [-0.01, 0.005, -0.005],
    [0.3; 3],
];

fn write_dng(
    path: &Path,
    noisy: bool,
    xyz_to_camera: color::Matrix,
    gains: [f32; 3],
) -> Result<()> {
    write_dng_with_secondary(path, noisy, xyz_to_camera, gains, None)
}

fn write_dng_with_secondary(
    path: &Path,
    noisy: bool,
    xyz_to_camera: color::Matrix,
    gains: [f32; 3],
    secondary: Option<color::Matrix>,
) -> Result<()> {
    write_dng_extended(path, noisy, xyz_to_camera, gains, secondary, None)
}

struct ExtraTags {
    analog: [f32; 3],
    camera: color::Matrix,
    forward: Option<color::Matrix>,
    signature_match: bool,
    byte_signature: bool,
    third: Option<(color::Matrix, [f32; 2])>,
    custom_first: Option<Vec<u8>>,
    white_xy: Option<[f32; 2]>,
    neutral_with_xy: bool,
}

struct Undefined<'a>(&'a [u8]);
impl tiff::encoder::TiffValue for Undefined<'_> {
    const BYTE_LEN: u8 = 1;
    const FIELD_TYPE: tiff::tags::Type = tiff::tags::Type::UNDEFINED;
    fn count(&self) -> usize {
        self.0.len()
    }
    fn data(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(self.0)
    }
}

fn xy_illuminant(xy: [f32; 2]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(0u16.to_ne_bytes());
    for v in xy {
        bytes.extend(((v * 1_000_000.).round() as u32).to_ne_bytes());
        bytes.extend(1_000_000u32.to_ne_bytes());
    }
    bytes
}

fn equal_energy_illuminant() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(1u16.to_ne_bytes());
    for v in [2u32, 360, 1, 470, 1, 1, 1, 1, 1] {
        bytes.extend(v.to_ne_bytes());
    }
    bytes
}

fn write_dng_extended(
    path: &Path,
    noisy: bool,
    xyz_to_camera: color::Matrix,
    gains: [f32; 3],
    secondary: Option<color::Matrix>,
    extra: Option<&ExtraTags>,
) -> Result<()> {
    write_dng_sized(
        path,
        noisy,
        xyz_to_camera,
        gains,
        secondary,
        extra,
        FixtureLayout {
            dimensions: [WIDTH, HEIGHT],
            hot_pixel: None,
        },
    )
}

struct FixtureLayout {
    dimensions: [usize; 2],
    hot_pixel: Option<[usize; 2]>,
}

fn write_dng_sized(
    path: &Path,
    noisy: bool,
    xyz_to_camera: color::Matrix,
    gains: [f32; 3],
    secondary: Option<color::Matrix>,
    extra: Option<&ExtraTags>,
    layout: FixtureLayout,
) -> Result<()> {
    let [width, height] = layout.dimensions;
    let count = rawpuppy::input::pixel_count(width, height, 1)?;
    let (w, h) = (u32::try_from(width)?, u32::try_from(height)?);
    let mut writer = BufWriter::new(std::fs::File::create_new(path)?);
    let mut encoder = TiffEncoder::new(&mut writer)?;
    let mut image = encoder.new_image::<Gray16>(w, h)?;
    let directory = image.encoder();
    directory.write_tag(Tag::Make, "Rawpuppy")?;
    directory.write_tag(Tag::Model, "Controlled Bayer Fixture")?;
    directory.write_tag(Tag::Orientation, 1u16)?;
    directory.write_tag(Tag::PhotometricInterpretation, 32803u16)?;
    directory.write_tag(Tag::Unknown(50706), &[1u8, 4, 0, 0][..])?; // DNGVersion
    directory.write_tag(Tag::Unknown(50707), &[1u8, 1, 0, 0][..])?; // DNGBackwardVersion
    directory.write_tag(Tag::Unknown(50708), "Rawpuppy Controlled Bayer Fixture")?;
    directory.write_tag(Tag::Unknown(33421), &[2u16, 2][..])?;
    directory.write_tag(Tag::Unknown(33422), &[0u8, 1, 1, 2][..])?;
    directory.write_tag(Tag::Unknown(50710), &[0u8, 1, 2][..])?;
    directory.write_tag(Tag::Unknown(50711), 1u16)?;
    directory.write_tag(Tag::Unknown(50713), &[1u16, 1][..])?;
    directory.write_tag(
        Tag::Unknown(50714),
        Rational {
            n: u32::from(BLACK),
            d: 1,
        },
    )?;
    directory.write_tag(Tag::Unknown(50717), u32::from(WHITE))?;
    directory.write_tag(Tag::Unknown(50719), &[0u32, 0][..])?;
    directory.write_tag(Tag::Unknown(50720), &[w, h][..])?;
    directory.write_tag(Tag::Unknown(50829), &[0u32, 0, h, w][..])?;
    directory.write_tag(Tag::Unknown(50778), 21u16)?; // D65
    let matrix: Vec<_> = xyz_to_camera
        .into_iter()
        .flatten()
        .map(|v| SRational {
            n: (v * 1_000_000.).round() as i32,
            d: 1_000_000,
        })
        .collect();
    directory.write_tag(Tag::Unknown(50721), matrix.as_slice())?;
    if let Some(secondary) = secondary {
        let matrix: Vec<_> = secondary
            .into_iter()
            .flatten()
            .map(|v| SRational {
                n: (v * 1_000_000.).round() as i32,
                d: 1_000_000,
            })
            .collect();
        directory.write_tag(Tag::Unknown(50722), matrix.as_slice())?;
        directory.write_tag(Tag::Unknown(50779), 17u16)?; // Standard A
    }
    let neutral = gains.map(|g| Rational {
        n: (1_000_000. / g).round() as u32,
        d: 1_000_000,
    });
    if let Some(xy) = extra.and_then(|e| e.white_xy) {
        let values = xy.map(|v| Rational {
            n: (f64::from(v) * 100_000_000.).round() as u32,
            d: 100_000_000,
        });
        directory.write_tag(Tag::Unknown(50729), &values[..])?;
        if extra.is_some_and(|e| e.neutral_with_xy) {
            directory.write_tag(Tag::Unknown(50728), &neutral[..])?;
        }
    } else {
        directory.write_tag(Tag::Unknown(50728), &neutral[..])?;
    }
    if let Some(extra) = extra {
        if extra.third.is_some() || extra.custom_first.is_some() {
            directory.write_tag(Tag::Unknown(50706), &[1u8, 6, 0, 0][..])?;
        }
        if let Some(data) = &extra.custom_first {
            directory.write_tag(Tag::Unknown(50778), 255u16)?;
            directory.write_tag(Tag::Unknown(52533), Undefined(data))?;
        }
        if let Some((cm, white)) = extra.third {
            directory.write_tag(Tag::Unknown(52529), 255u16)?;
            directory.write_tag(Tag::Unknown(52535), Undefined(&xy_illuminant(white)))?;
            let matrix: Vec<_> = cm
                .into_iter()
                .flatten()
                .map(|v| SRational {
                    n: (v * 1_000_000.).round() as i32,
                    d: 1_000_000,
                })
                .collect();
            directory.write_tag(Tag::Unknown(52531), matrix.as_slice())?;
        }
        let analog = extra.analog.map(|v| Rational {
            n: (v * 1_000_000.).round() as u32,
            d: 1_000_000,
        });
        directory.write_tag(Tag::Unknown(50727), &analog[..])?;
        let camera: Vec<_> = extra
            .camera
            .into_iter()
            .flatten()
            .map(|v| SRational {
                n: (v * 1_000_000.).round() as i32,
                d: 1_000_000,
            })
            .collect();
        directory.write_tag(Tag::Unknown(50723), camera.as_slice())?;
        if extra.byte_signature {
            directory.write_tag(Tag::Unknown(50931), &b"rawpuppy-test-camera\0"[..])?;
            directory.write_tag(
                Tag::Unknown(50932),
                if extra.signature_match {
                    &b"rawpuppy-test-camera\0"[..]
                } else {
                    &b"other-reference-camera\0"[..]
                },
            )?;
        } else {
            directory.write_tag(Tag::Unknown(50931), "rawpuppy-test-camera")?;
            directory.write_tag(
                Tag::Unknown(50932),
                if extra.signature_match {
                    "rawpuppy-test-camera"
                } else {
                    "other-reference-camera"
                },
            )?;
        }
        if let Some(forward) = extra.forward {
            let fm: Vec<_> = forward
                .into_iter()
                .flatten()
                .map(|v| SRational {
                    n: (v * 1_000_000.).round() as i32,
                    d: 1_000_000,
                })
                .collect();
            directory.write_tag(Tag::Unknown(50964), fm.as_slice())?;
        }
    }
    let mut state = 0x92d68ca2u32;
    let mut values = Vec::new();
    values.try_reserve_exact(count)?;
    values.extend((0..count).map(|i| {
        let (x, y) = (i % width, i / width);
        let channel = [0, 1, 1, 2][(y % 2) * 2 + x % 2];
        let rgb = COLORS[(y * 3 / height) * 3 + x * 3 / width];
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        // Equal-magnitude signed samples preserve the chosen mean in expectation.
        let noise = if noisy {
            if state & 1 == 0 { -224. } else { 224. }
        } else {
            0.
        };
        if layout.hot_pixel == Some([x, y]) {
            WHITE
        } else {
            (f32::from(BLACK) + rgb[channel] * f32::from(WHITE - BLACK) + noise)
                .round()
                .clamp(0., 65535.) as u16
        }
    }));
    image.write_data(&values)?;
    writer.flush()?;
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(!args.output.exists(), "Choose a new output directory");
    ensure!(
        args.width >= 18 && args.height >= 18,
        "The 3 × 3 Bayer controls need at least 18 pixels per fixture dimension"
    );
    let (width, height) = (args.width, args.height);
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    let (camera_matrix, mut gains, source_hash) = if let Some(path) = &args.like_camera {
        let hash = models::sha256(path)?;
        let source = SensorImage::open(path)?;
        (
            source.metadata.camera_to_working,
            source.metadata.as_shot,
            Some(hash),
        )
    } else {
        (color::IDENTITY, [1.8, 1., 2.4], None)
    };
    let xyz_to_camera = color::multiply(
        color::inverse(camera_matrix)?,
        color::inverse(color::SRGB_TO_XYZ)?,
    );
    let secondary = (args.dual_warm || args.triple).then(|| {
        color::multiply(
            [[1.10, 0.02, -0.03], [0., 1., 0.], [-0.04, 0.02, 0.90]],
            xyz_to_camera,
        )
    });
    if let Some(warm_matrix) = secondary {
        let neutral = color::apply(warm_matrix, [1.09850, 1., 0.35585]);
        gains = neutral.map(|v| neutral[1] / v);
    }
    let mut extended_tags = None;
    if args.triple || args.custom_spectrum {
        let third = args.triple.then(|| {
            (
                color::multiply(
                    [[0.92, 0.04, 0.02], [0.03, 1.08, -0.02], [0.01, -0.03, 1.12]],
                    xyz_to_camera,
                ),
                [0.38, 0.40],
            )
        });
        let neutral = if let Some((matrix, [x, y])) = third {
            color::apply(matrix, [x / y, 1., (1. - x - y) / y])
        } else {
            color::apply(xyz_to_camera, [1.; 3])
        };
        gains = neutral.map(|v| neutral[1] / v);
        extended_tags = Some(ExtraTags {
            analog: [1.; 3],
            camera: color::IDENTITY,
            forward: None,
            signature_match: true,
            byte_signature: false,
            third,
            custom_first: args.custom_spectrum.then(equal_energy_illuminant),
            white_xy: None,
            neutral_with_xy: false,
        });
    }
    if args.white_xy {
        let xyz = if args.triple {
            [0.95, 1., 0.55]
        } else if args.custom_spectrum {
            [1.; 3]
        } else {
            [1.09850, 1., 0.35585]
        };
        let sum: f32 = xyz.iter().sum();
        let xy = [xyz[0] / sum, xyz[1] / sum];
        let matrix = extended_tags
            .as_ref()
            .and_then(|e| e.third.map(|p| p.0))
            .or(secondary)
            .unwrap_or(xyz_to_camera);
        let neutral = color::apply(matrix, xyz);
        gains = neutral.map(|v| neutral[1] / v);
        if let Some(tags) = &mut extended_tags {
            tags.white_xy = Some(xy);
        } else {
            extended_tags = Some(ExtraTags {
                analog: [1.; 3],
                camera: color::IDENTITY,
                forward: None,
                signature_match: true,
                byte_signature: false,
                third: None,
                custom_first: None,
                white_xy: Some(xy),
                neutral_with_xy: false,
            });
        }
    }
    std::fs::create_dir(&args.output)?;
    let mut records = Vec::new();
    for noisy in [false, true] {
        let name = if noisy { "noisy" } else { "constant" };
        let path = args.output.join(format!("{name}.dng"));
        if [width, height] != [WIDTH, HEIGHT] {
            write_dng_sized(
                &path,
                noisy,
                xyz_to_camera,
                gains,
                secondary,
                extended_tags.as_ref(),
                FixtureLayout {
                    dimensions: [width, height],
                    hot_pixel: None,
                },
            )?;
        } else if let Some(extra) = &extended_tags {
            write_dng_extended(&path, noisy, xyz_to_camera, gains, secondary, Some(extra))?;
        } else if secondary.is_some() {
            write_dng_with_secondary(&path, noisy, xyz_to_camera, gains, secondary)?;
        } else {
            write_dng(&path, noisy, xyz_to_camera, gains)?;
        }
        let hash = models::sha256(&path)?;
        let source = SensorImage::open(&path)?;
        let mut edits = Edits::default();
        edits.tone.mapper = ToneMapper::Linear;
        let output = Pipeline::compile(&source, &edits)?.render(None)?;
        let reference = args
            .reference_dir
            .as_ref()
            .map(|directory| SensorImage::open(&directory.join(format!("{name}-darktable.exr"))))
            .transpose()?;
        if let Some(reference) = &reference {
            ensure!(
                reference.cpp == 3
                    && reference.metadata.width == width
                    && reference.metadata.height == height,
                "Reference dimensions or channels differ"
            );
        }
        export::write(
            &args.output.join(format!("{name}-rawpuppy.exr")),
            &output,
            color::OutputSpace::LinearSrgb,
            false,
        )?;
        let mut patches = Vec::new();
        for (i, color) in COLORS.iter().enumerate() {
            let bounds = |cell: usize, length: usize| {
                let start = (cell * length).div_ceil(3);
                let end = ((cell + 1) * length).div_ceil(3);
                let margin = ((end - start) / 4).clamp(2, 32);
                (start + margin, end - margin)
            };
            let (x0, x1) = bounds(i % 3, width);
            let (y0, y1) = bounds(i / 3, height);
            let mut mean = [0f64; 3];
            let mut camera_sum = [0f64; 3];
            let mut clipped_camera_sum = [0f64; 3];
            let mut camera_count = [0usize; 3];
            let mut reference_sum = [0f64; 3];
            for y in y0..y1 {
                for x in x0..x1 {
                    for c in 0..3 {
                        mean[c] += f64::from(output.pixels[y * width + x][c]);
                        if let Some(reference) = &reference {
                            reference_sum[c] += f64::from(reference.data[(y * width + x) * 3 + c]);
                        }
                    }
                    let c = source.cfa.as_ref().unwrap().color_at(y, x);
                    camera_sum[c] += f64::from(source.data[y * width + x]);
                    clipped_camera_sum[c] += f64::from(source.data[y * width + x].max(0.));
                    camera_count[c] += 1;
                }
            }
            let measured_camera_mean: [f32; 3] =
                std::array::from_fn(|c| (camera_sum[c] / camera_count[c] as f64) as f32);
            let expected_working_mean = color::apply(
                source.metadata.camera_to_working,
                std::array::from_fn(|c| measured_camera_mean[c] * source.metadata.as_shot[c]),
            );
            let count = ((x1 - x0) * (y1 - y0)) as f64;
            let mean = mean.map(|v| v / count);
            if !noisy {
                ensure!(
                    mean.iter()
                        .zip(expected_working_mean)
                        .all(|(a, b)| (*a - f64::from(b)).abs() < 0.000003),
                    "Constant DNG patch {i} disagrees with its analytic calibration"
                );
            }
            let clipped_camera_mean: [f32; 3] =
                std::array::from_fn(|c| (clipped_camera_sum[c] / camera_count[c] as f64) as f32);
            let inferred_reference_camera_mean = reference.as_ref().map(|_| {
                color::apply(
                    color::inverse(source.metadata.camera_to_working).unwrap(),
                    reference_sum.map(|v| (v / count) as f32),
                )
                .into_iter()
                .zip(source.metadata.as_shot)
                .map(|(v, g)| v / g)
                .collect::<Vec<_>>()
            });
            patches.push(serde_json::json!({ "patch":i,"input_camera_rgb":color,"measured_camera_mean":measured_camera_mean,"clipped_camera_mean":clipped_camera_mean,"expected_working_mean":expected_working_mean,"rawpuppy_mean":mean,"reference_mean":reference.as_ref().map(|_| reference_sum.map(|v| v / count)),"inferred_reference_camera_mean":inferred_reference_camera_mean,"region":[x0,y0,x1-x0,y1-y0]}));
        }
        ensure!(
            models::sha256(&path)? == hash,
            "Fixture bytes changed while reading"
        );
        records.push(serde_json::json!({"fixture":name,"sha256":hash,"metadata":source.metadata,"patches":patches}));
    }
    if let (Some(path), Some(hash)) = (&args.like_camera, &source_hash) {
        ensure!(
            models::sha256(path)? == *hash,
            "Camera source bytes changed"
        );
    }
    let receipt = serde_json::json!({"scope":"synthetic Bayer DNG controls; equivalent normalized camera matrix, not a copy of the camera's original profile","camera_source_sha256":source_hash,"camera_to_working":camera_matrix,"as_shot":gains,"xyz_to_camera":xyz_to_camera,"black":BLACK,"white":WHITE,"dimensions":[width,height],"block_size":([width,height]==[WIDTH,HEIGHT]).then_some(BLOCK),"fixtures":records});
    std::fs::write(
        args.output.join("receipt.json"),
        serde_json::to_string_pretty(&receipt)? + "\n",
    )?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: [usize; 2] = [100_003, 24];
    const HOT: [usize; 2] = [70_000, 12];

    fn wide_raw_control(path: &Path) -> SensorImage {
        write_dng_sized(
            path,
            false,
            color::inverse(color::SRGB_TO_XYZ).unwrap(),
            [1.; 3],
            None,
            None,
            FixtureLayout {
                dimensions: WIDE,
                hot_pixel: Some(HOT),
            },
        )
        .unwrap();
        let source = SensorImage::open(path).unwrap();
        assert_eq!(
            [source.metadata.sensor_width, source.metadata.sensor_height],
            WIDE
        );
        assert_eq!([source.metadata.width, source.metadata.height], WIDE);
        assert_eq!(source.cfa.as_ref().unwrap().name, "RGGB");
        assert_eq!(source.cpp, 1);
        assert!(source.raw_integer);
        assert_eq!(source.data[HOT[1] * WIDE[0] + HOT[0]], 1.);
        source
    }

    fn wide_row_region() -> [f32; 4] {
        [0., HOT[1] as f32 / WIDE[1] as f32, 1., 1. / WIDE[1] as f32]
    }

    #[test]
    fn wide_dng_decodes_and_corrects_a_hot_pixel_beyond_16_bit_coordinates() {
        use sha2::Digest;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wide.dng");
        let source = wide_raw_control(&path);
        let input_hash = models::sha256(&path).unwrap();
        let sensor_hash = sha2::Sha256::digest(bytemuck::cast_slice(&source.data));
        let mut edits = Edits::default();
        edits.tone.mapper = ToneMapper::Linear;
        let uncorrected = source.reconstruct(HOT[0] as isize, HOT[1] as isize, &edits.raw);
        assert_eq!(uncorrected[0], 1.);
        edits.raw.hot_pixels = true;
        let row = Pipeline::compile(&source, &edits)
            .unwrap()
            .render_region(wide_row_region(), WIDE[0], 1)
            .unwrap();
        let mut checked = 0;
        for (x, pixel) in row.pixels.iter().enumerate() {
            let patch = x * 3 / WIDE[0];
            let start = (patch * WIDE[0]).div_ceil(3);
            let end = ((patch + 1) * WIDE[0]).div_ceil(3);
            // MHC and bilinear interpolation can mix colors at patch boundaries.
            if x < start + 3 || x + 3 >= end {
                continue;
            }
            let expected = COLORS[3 + patch].map(|v| {
                ((f32::from(BLACK) + v * f32::from(WHITE - BLACK)).round() - f32::from(BLACK))
                    / f32::from(WHITE - BLACK)
            });
            for c in 0..3 {
                assert!(
                    (pixel[c] - expected[c]).abs() < 0.000005,
                    "Wide RAW mismatch at x={x}: {pixel:?} vs {expected:?}"
                );
            }
            assert_eq!(pixel[3], 1.);
            checked += 1;
        }
        assert_eq!(checked, WIDE[0] - 18);
        assert_eq!(models::sha256(&path).unwrap(), input_hash);
        assert_eq!(
            sha2::Sha256::digest(bytemuck::cast_slice(&source.data)),
            sensor_hash
        );
    }

    #[cfg(feature = "gpu")]
    #[test]
    #[ignore = "requires a working Vulkan/Metal/CUDA compute device"]
    fn wide_dng_gpu_preserves_raw_coordinates_and_hot_pixel_cleanup() {
        use rawpuppy::gpu::{Backend, CudaMemoryMode, GpuRenderer};
        use sha2::Digest;
        use std::sync::Arc;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wide-gpu.dng");
        let source = Arc::new(wide_raw_control(&path));
        let input_hash = models::sha256(&path).unwrap();
        let sensor_hash = sha2::Sha256::digest(bytemuck::cast_slice(&source.data));
        let mut edits = Edits::default();
        edits.tone.mapper = ToneMapper::Linear;
        edits.raw.hot_pixels = true;
        let expected = Pipeline::compile(&source, &edits)
            .unwrap()
            .render_region(wide_row_region(), WIDE[0], 1)
            .unwrap();
        let modes = if std::env::var_os("RAWPUPPY_TEST_CUDA").is_some() {
            vec![
                (Backend::Cuda, CudaMemoryMode::Copy),
                (Backend::Cuda, CudaMemoryMode::System),
            ]
        } else {
            vec![(
                if cfg!(target_os = "macos") {
                    Backend::Metal
                } else {
                    Backend::Vulkan
                },
                CudaMemoryMode::Auto,
            )]
        };
        for (backend, memory) in modes {
            let mut gpu = GpuRenderer::with_cuda_memory(backend, memory).unwrap();
            let actual = gpu
                .render_region(source.clone(), &edits, wide_row_region(), WIDE[0], 1)
                .unwrap();
            assert_eq!((actual.width, actual.height), (WIDE[0], 1));
            assert_eq!(actual.pixels.len(), expected.pixels.len());
            let mut max_error = 0f32;
            for (a, b) in actual.pixels.iter().zip(&expected.pixels) {
                assert!(a.iter().all(|v| v.is_finite()));
                for c in 0..3 {
                    max_error = max_error.max((a[c] - b[c]).abs());
                }
                assert_eq!(a[3], b[3]);
            }
            assert!(max_error < 0.0003, "{} mismatch {max_error}", gpu.name());
            assert!((actual.pixels[HOT[0]][0] - 0.02).abs() < 0.0001);
            eprintln!(
                "{}: 100003-pixel RAW row and hot pixel at x=70000 passed; max CPU error {max_error:.8}",
                gpu.name()
            );
        }
        assert_eq!(models::sha256(&path).unwrap(), input_hash);
        assert_eq!(
            sha2::Sha256::digest(bytemuck::cast_slice(&source.data)),
            sensor_hash
        );
    }

    #[test]
    fn as_shot_white_xy_uses_the_selected_profile_for_camera_neutral() {
        let directory = tempfile::tempdir().unwrap();
        let daylight = color::inverse(color::SRGB_TO_XYZ).unwrap();
        let warm = color::multiply(
            [[1.10, 0.02, -0.03], [0., 1., 0.], [-0.04, 0.02, 0.90]],
            daylight,
        );
        let third = color::multiply(
            [[0.92, 0.04, 0.02], [0.03, 1.08, -0.02], [0.01, -0.03, 1.12]],
            daylight,
        );
        for (name, white, cm, third_tag) in [
            ("warm", [1.09850, 1., 0.35585], warm, None),
            ("third", [0.95, 1., 0.55], third, Some((third, [0.38, 0.4]))),
        ] {
            let sum: f32 = white.iter().sum();
            let xy = [white[0] / sum, white[1] / sum];
            let extra = ExtraTags {
                analog: [1.; 3],
                camera: color::IDENTITY,
                forward: None,
                signature_match: true,
                byte_signature: false,
                third: third_tag,
                custom_first: None,
                white_xy: Some(xy),
                neutral_with_xy: false,
            };
            let path = directory.path().join(format!("xy-{name}.dng"));
            write_dng_extended(&path, false, daylight, [1.; 3], Some(warm), Some(&extra)).unwrap();
            let hash = models::sha256(&path).unwrap();
            let source = SensorImage::open(&path).unwrap();
            let neutral = color::apply(cm, white);
            let expected_gains = neutral.map(|v| neutral[1] / v);
            assert!(
                source
                    .metadata
                    .as_shot
                    .iter()
                    .zip(expected_gains)
                    .all(|(a, b)| (*a - b).abs() < 0.00005),
                "{name}: {:?} vs {expected_gains:?}",
                source.metadata.as_shot
            );
            let mut expected = color::multiply(
                cm,
                color::multiply(
                    color::adapt_white([0.95047, 1., 1.08883], white).unwrap(),
                    color::SRGB_TO_XYZ,
                ),
            );
            for row in &mut expected {
                let sum: f32 = row.iter().sum();
                for v in row {
                    *v /= sum;
                }
            }
            let expected = color::inverse(expected).unwrap();
            assert!(
                source
                    .metadata
                    .camera_to_working
                    .into_iter()
                    .flatten()
                    .zip(expected.into_iter().flatten())
                    .all(|(a, b)| (a - b).abs() < 0.00005)
            );
            assert_eq!(source.metadata.color_revision, 3);
            assert_eq!(rawpuppy::input::color_revision(&path).unwrap(), 3);
            assert_eq!(models::sha256(&path).unwrap(), hash);
            let mut layers = rawpuppy::synthesis::Layers::new(path.clone());
            let image = rawpuppy::pipeline::Rendered {
                width: 1,
                height: 1,
                pixels: vec![[0.18, 0.25, 0.3, 1.]],
            };
            let (asset, sha256) = layers.store(&image).unwrap();
            let mut edits = Edits::default();
            edits
                .display
                .synthesis
                .push(rawpuppy::synthesis::GeneratedFill {
                    region: [0., 0., 1., 1.],
                    dabs: vec![rawpuppy::synthesis::MaskDab {
                        center: [0.5; 2],
                        radius: 1.,
                    }],
                    fill_gaps: false,
                    steps: 2,
                    seed: 0,
                    asset,
                    sha256,
                    source_sha256: layers.source_hash().unwrap().into(),
                    source_color_revision: 2,
                    recipe_sha256: rawpuppy::synthesis::recipe_hash(&edits).unwrap(),
                    sampling: Default::default(),
                    model: "moebius-scene-2026-v1".into(),
                });
            let mut frame = image;
            assert!(layers.apply(&edits, &mut frame, [0., 0., 1., 1.]).is_err());
            edits.display.synthesis[0].source_color_revision = 3;
            edits.validate().unwrap();
            layers.apply(&edits, &mut frame, [0., 0., 1., 1.]).unwrap();
        }
        let invalid = directory.path().join("invalid-xy.dng");
        let extra = ExtraTags {
            analog: [1.; 3],
            camera: color::IDENTITY,
            forward: None,
            signature_match: true,
            byte_signature: false,
            third: None,
            custom_first: None,
            white_xy: Some([0., 0.4]),
            neutral_with_xy: false,
        };
        write_dng_extended(&invalid, false, daylight, [1.; 3], None, Some(&extra)).unwrap();
        assert!(
            SensorImage::open(&invalid)
                .err()
                .unwrap()
                .to_string()
                .contains("as-shot white point")
        );
        let conflict = directory.path().join("conflicting-white-balance.dng");
        let extra = ExtraTags {
            white_xy: Some([0.44757, 0.40745]),
            neutral_with_xy: true,
            ..extra
        };
        write_dng_extended(&conflict, false, daylight, [1.; 3], None, Some(&extra)).unwrap();
        let hash = models::sha256(&conflict).unwrap();
        assert!(
            SensorImage::open(&conflict)
                .err()
                .unwrap()
                .to_string()
                .contains("not both")
        );
        assert_eq!(models::sha256(&conflict).unwrap(), hash);
    }

    #[test]
    fn custom_and_third_illuminant_dngs_use_the_declared_calibration() {
        let directory = tempfile::tempdir().unwrap();
        let daylight = color::inverse(color::SRGB_TO_XYZ).unwrap();
        let warm = color::multiply(
            [[1.10, 0.02, -0.03], [0., 1., 0.], [-0.04, 0.02, 0.90]],
            daylight,
        );
        let third = color::multiply(
            [[0.92, 0.04, 0.02], [0.03, 1.08, -0.02], [0.01, -0.03, 1.12]],
            daylight,
        );
        for (name, target, cm, secondary, extra) in [
            (
                "custom-xy",
                [0.36 / 0.42, 1., 0.22 / 0.42],
                daylight,
                None,
                ExtraTags {
                    analog: [1.; 3],
                    camera: color::IDENTITY,
                    forward: None,
                    signature_match: true,
                    byte_signature: false,
                    third: None,
                    custom_first: Some(xy_illuminant([0.36, 0.42])),
                    white_xy: None,
                    neutral_with_xy: false,
                },
            ),
            (
                "custom-spectrum",
                [1.; 3],
                daylight,
                None,
                ExtraTags {
                    analog: [1.; 3],
                    camera: color::IDENTITY,
                    forward: None,
                    signature_match: true,
                    byte_signature: false,
                    third: None,
                    custom_first: Some(equal_energy_illuminant()),
                    white_xy: None,
                    neutral_with_xy: false,
                },
            ),
            (
                "third",
                [0.38 / 0.40, 1., 0.22 / 0.40],
                third,
                Some(warm),
                ExtraTags {
                    analog: [1.; 3],
                    camera: color::IDENTITY,
                    forward: None,
                    signature_match: true,
                    byte_signature: false,
                    third: Some((third, [0.38, 0.40])),
                    custom_first: None,
                    white_xy: None,
                    neutral_with_xy: false,
                },
            ),
        ] {
            let neutral = color::apply(cm, target);
            let gains = neutral.map(|v| neutral[1] / v);
            let path = directory.path().join(format!("{name}.dng"));
            write_dng_extended(&path, false, daylight, gains, secondary, Some(&extra)).unwrap();
            let hash = models::sha256(&path).unwrap();
            let source = SensorImage::open(&path).unwrap();
            let mut expected = color::multiply(
                cm,
                color::multiply(
                    color::adapt_white([0.95047, 1., 1.08883], target).unwrap(),
                    color::SRGB_TO_XYZ,
                ),
            );
            for row in &mut expected {
                let sum: f32 = row.iter().sum();
                for v in row {
                    *v /= sum;
                }
            }
            let expected = color::inverse(expected).unwrap();
            let max = source
                .metadata
                .camera_to_working
                .into_iter()
                .flatten()
                .zip(expected.into_iter().flatten())
                .map(|(a, b)| (a - b).abs())
                .fold(0., f32::max);
            assert!(max < 0.00004, "{name} matrix mismatch {max}");
            assert_eq!(source.metadata.color_revision, 2);
            assert_eq!(rawpuppy::input::color_revision(&path).unwrap(), 2);
            let mut layers = rawpuppy::synthesis::Layers::new(path.clone());
            let image = rawpuppy::pipeline::Rendered {
                width: 1,
                height: 1,
                pixels: vec![[0.18, 0.25, 0.3, 1.]],
            };
            let (asset, sha256) = layers.store(&image).unwrap();
            let mut edits = Edits::default();
            let fill = rawpuppy::synthesis::GeneratedFill {
                region: [0., 0., 1., 1.],
                dabs: vec![rawpuppy::synthesis::MaskDab {
                    center: [0.5; 2],
                    radius: 1.,
                }],
                fill_gaps: false,
                steps: 2,
                seed: 0,
                asset,
                sha256,
                source_sha256: layers.source_hash().unwrap().into(),
                source_color_revision: 1,
                recipe_sha256: rawpuppy::synthesis::recipe_hash(&edits).unwrap(),
                sampling: Default::default(),
                model: "moebius-scene-2026-v1".into(),
            };
            edits.display.synthesis.push(fill);
            let mut frame = image;
            assert!(layers.apply(&edits, &mut frame, [0., 0., 1., 1.]).is_err());
            edits.display.synthesis[0].source_color_revision = 2;
            edits.validate().unwrap();
            layers.apply(&edits, &mut frame, [0., 0., 1., 1.]).unwrap();
            assert_eq!(models::sha256(path.as_path()).unwrap(), hash);
        }
    }

    #[test]
    fn dng_analog_camera_and_forward_tags_follow_the_linear_transform() {
        let directory = tempfile::tempdir().unwrap();
        let cm = color::inverse(color::SRGB_TO_XYZ).unwrap();
        let analog = [1.2, 1., 0.8];
        let camera = [[1., 0.03, -0.03], [0.01, 0.99, 0.], [0., 0.02, 0.98]];
        let ab: color::Matrix =
            std::array::from_fn(|i| std::array::from_fn(|j| if i == j { analog[i] } else { 0. }));
        let forward = color::multiply(
            color::adapt_white(
                color::apply(color::SRGB_TO_XYZ, [1.; 3]),
                [0.96422, 1., 0.82521],
            )
            .unwrap(),
            color::SRGB_TO_XYZ,
        );
        for byte_signature in [false, true] {
            for signature_match in [false, true] {
                for fm in [None, Some(forward)] {
                    let extra = ExtraTags {
                        analog,
                        camera,
                        forward: fm,
                        signature_match,
                        byte_signature,
                        third: None,
                        custom_first: None,
                        white_xy: None,
                        neutral_with_xy: false,
                    };
                    let reference_to_camera = color::multiply(
                        ab,
                        if signature_match {
                            camera
                        } else {
                            color::IDENTITY
                        },
                    );
                    let xyz_to_camera = color::multiply(reference_to_camera, cm);
                    let neutral = color::apply(xyz_to_camera, [0.95047, 1., 1.08883]);
                    let gains = neutral.map(|v| neutral[1] / v);
                    let path = directory.path().join(format!(
                        "extra-{byte_signature}-{signature_match}-{}.dng",
                        fm.is_some()
                    ));
                    write_dng_extended(&path, false, cm, gains, None, Some(&extra)).unwrap();
                    let source = SensorImage::open(&path).unwrap();
                    let mut expected = color::multiply(xyz_to_camera, color::SRGB_TO_XYZ);
                    for row in &mut expected {
                        let sum: f32 = row.iter().sum();
                        for v in row {
                            *v /= sum;
                        }
                    }
                    let mut expected = color::inverse(expected).unwrap();
                    if fm.is_some() {
                        let n = gains.map(|v| 1. / v);
                        let reference_inverse = color::inverse(reference_to_camera).unwrap();
                        let reference_n = color::apply(reference_inverse, n);
                        let scale: color::Matrix = std::array::from_fn(|i| {
                            std::array::from_fn(|j| if i == j { 1. / reference_n[i] } else { 0. })
                        });
                        let neutral: color::Matrix = std::array::from_fn(|i| {
                            std::array::from_fn(|j| if i == j { n[i] } else { 0. })
                        });
                        expected =
                            color::multiply(scale, color::multiply(reference_inverse, neutral));
                        let white = color::apply(source.metadata.camera_to_working, [1.; 3]);
                        assert!(white.iter().all(|v| (*v - 1.).abs() < 0.000005));
                    }
                    let max = source
                        .metadata
                        .camera_to_working
                        .into_iter()
                        .flatten()
                        .zip(expected.into_iter().flatten())
                        .map(|(a, b)| (a - b).abs())
                        .fold(0., f32::max);
                    assert!(
                        max < 0.00003,
                        "Extra calibration difference {max}, signature={signature_match}, forward={}",
                        fm.is_some()
                    );
                    assert_eq!(source.metadata.color_revision, 1);
                    assert_eq!(rawpuppy::input::color_revision(&path).unwrap(), 1);
                }
            }
        }
    }

    #[test]
    fn dual_illuminant_dng_selects_warm_and_daylight_and_versions_its_color() {
        let directory = tempfile::tempdir().unwrap();
        let daylight = color::inverse(color::SRGB_TO_XYZ).unwrap();
        let warm = color::multiply(
            [[1.10, 0.02, -0.03], [0., 1., 0.], [-0.04, 0.02, 0.90]],
            daylight,
        );
        for (name, white, expected_matrix) in [
            ("warm", [1.09850, 1., 0.35585], warm),
            ("daylight", [0.95047, 1., 1.08883], daylight),
        ] {
            let neutral = color::apply(expected_matrix, white);
            let gains = neutral.map(|v| neutral[1] / v);
            let path = directory.path().join(format!("{name}.dng"));
            write_dng_with_secondary(&path, false, daylight, gains, Some(warm)).unwrap();
            let source = SensorImage::open(&path).unwrap();
            let mut expected = color::multiply(
                expected_matrix,
                color::multiply(
                    color::adapt_white([0.95047, 1., 1.08883], white).unwrap(),
                    color::SRGB_TO_XYZ,
                ),
            );
            for row in &mut expected {
                let sum: f32 = row.iter().sum();
                for v in row {
                    *v /= sum;
                }
            }
            let expected = color::inverse(expected).unwrap();
            let max = source
                .metadata
                .camera_to_working
                .into_iter()
                .flatten()
                .zip(expected.into_iter().flatten())
                .map(|(a, b)| (a - b).abs())
                .fold(0., f32::max);
            assert!(max < 0.00003, "{name} calibration difference {max}");
            assert_eq!(source.metadata.color_revision, 1);
            assert_eq!(rawpuppy::input::color_revision(&path).unwrap(), 1);
            let mut edits = Edits::default();
            edits.tone.mapper = ToneMapper::Linear;
            let mut layers = rawpuppy::synthesis::Layers::new(path.clone());
            let pixel = rawpuppy::pipeline::Rendered {
                width: 1,
                height: 1,
                pixels: vec![[0.18, 0.25, 0.3, 1.]],
            };
            let (asset, sha256) = layers.store(&pixel).unwrap();
            let fill = rawpuppy::synthesis::GeneratedFill {
                region: [0., 0., 1., 1.],
                dabs: vec![rawpuppy::synthesis::MaskDab {
                    center: [0.5; 2],
                    radius: 1.,
                }],
                fill_gaps: false,
                steps: 2,
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
            let mut rendered = pixel;
            assert!(
                layers
                    .apply(&edits, &mut rendered, [0., 0., 1., 1.])
                    .is_err()
            );
            edits.display.synthesis[0].source_color_revision = 1;
            layers
                .apply(&edits, &mut rendered, [0., 0., 1., 1.])
                .unwrap();
        }
    }

    #[test]
    fn dng_camera_metadata_preserves_signed_samples_and_calibrates_bayer_patches() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("constant.dng");
        let gains = [1.8, 1., 2.4];
        write_dng(
            &path,
            false,
            color::inverse(color::SRGB_TO_XYZ).unwrap(),
            gains,
        )
        .unwrap();
        let hash = models::sha256(&path).unwrap();
        let source = SensorImage::open(&path).unwrap();
        assert_eq!(
            (source.metadata.width, source.metadata.height),
            (WIDTH, HEIGHT)
        );
        assert_eq!(source.cfa.as_ref().unwrap().name, "RGGB");
        assert_eq!(source.cpp, 1);
        assert!(source.raw_integer);
        assert!(source.data.iter().any(|v| *v < 0.));
        for (actual, expected) in source.metadata.as_shot.iter().zip(gains) {
            assert!((*actual - expected).abs() < 0.000005);
        }
        let mut edits = Edits::default();
        edits.tone.mapper = ToneMapper::Linear;
        let pipeline = Pipeline::compile(&source, &edits).unwrap();
        for (i, rgb) in COLORS.iter().enumerate() {
            let x = i % 3 * BLOCK + BLOCK / 2;
            let y = i / 3 * BLOCK + BLOCK / 2;
            let pixel = pipeline.sample([
                (x as f32 + 0.5) / WIDTH as f32,
                (y as f32 + 0.5) / HEIGHT as f32,
            ]);
            for c in 0..3 {
                let normalized = ((f32::from(BLACK) + rgb[c] * f32::from(WHITE - BLACK)).round()
                    - f32::from(BLACK))
                    / f32::from(WHITE - BLACK);
                assert!(
                    (pixel[c] - normalized * gains[c]).abs() < 0.000005,
                    "patch {i}, channel {c}: {pixel:?}"
                );
            }
            assert_eq!(pixel[3], 1.);
        }
        assert_eq!(models::sha256(&path).unwrap(), hash);
    }
}
