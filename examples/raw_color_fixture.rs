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
    /// Use an existing camera's equivalent normalized calibration and as-shot gains.
    #[arg(long)]
    like_camera: Option<PathBuf>,
    /// Directory containing constant-darktable.exr and noisy-darktable.exr controls.
    #[arg(long)]
    reference_dir: Option<PathBuf>,
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
    let mut writer = BufWriter::new(std::fs::File::create_new(path)?);
    let mut encoder = TiffEncoder::new(&mut writer)?;
    let mut image = encoder.new_image::<Gray16>(WIDTH as u32, HEIGHT as u32)?;
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
    directory.write_tag(Tag::Unknown(50720), &[WIDTH as u32, HEIGHT as u32][..])?;
    directory.write_tag(
        Tag::Unknown(50829),
        &[0u32, 0, HEIGHT as u32, WIDTH as u32][..],
    )?;
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
    let neutral = gains.map(|g| Rational {
        n: (1_000_000. / g).round() as u32,
        d: 1_000_000,
    });
    directory.write_tag(Tag::Unknown(50728), &neutral[..])?;
    let mut state = 0x92d68ca2u32;
    let values: Vec<_> = (0..WIDTH * HEIGHT)
        .map(|i| {
            let (x, y) = (i % WIDTH, i / WIDTH);
            let channel = [0, 1, 1, 2][(y % 2) * 2 + x % 2];
            let rgb = COLORS[(y / BLOCK) * 3 + x / BLOCK];
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            // Equal-magnitude signed samples preserve the chosen mean in expectation.
            let noise = if noisy {
                if state & 1 == 0 { -224. } else { 224. }
            } else {
                0.
            };
            (f32::from(BLACK) + rgb[channel] * f32::from(WHITE - BLACK) + noise)
                .round()
                .clamp(0., 65535.) as u16
        })
        .collect();
    image.write_data(&values)?;
    writer.flush()?;
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(!args.output.exists(), "Choose a new output directory");
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    let (camera_matrix, gains, source_hash) = if let Some(path) = &args.like_camera {
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
    std::fs::create_dir(&args.output)?;
    let mut records = Vec::new();
    for noisy in [false, true] {
        let name = if noisy { "noisy" } else { "constant" };
        let path = args.output.join(format!("{name}.dng"));
        write_dng(&path, noisy, xyz_to_camera, gains)?;
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
                    && reference.metadata.width == WIDTH
                    && reference.metadata.height == HEIGHT,
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
            let x0 = (i % 3) * BLOCK + 32;
            let y0 = (i / 3) * BLOCK + 32;
            let mut mean = [0f64; 3];
            let mut camera_sum = [0f64; 3];
            let mut clipped_camera_sum = [0f64; 3];
            let mut camera_count = [0usize; 3];
            let mut reference_sum = [0f64; 3];
            for y in y0..y0 + BLOCK - 64 {
                for x in x0..x0 + BLOCK - 64 {
                    for c in 0..3 {
                        mean[c] += f64::from(output.pixels[y * WIDTH + x][c]);
                        if let Some(reference) = &reference {
                            reference_sum[c] += f64::from(reference.data[(y * WIDTH + x) * 3 + c]);
                        }
                    }
                    let c = source.cfa.as_ref().unwrap().color_at(y, x);
                    camera_sum[c] += f64::from(source.data[y * WIDTH + x]);
                    clipped_camera_sum[c] += f64::from(source.data[y * WIDTH + x].max(0.));
                    camera_count[c] += 1;
                }
            }
            let measured_camera_mean: [f32; 3] =
                std::array::from_fn(|c| (camera_sum[c] / camera_count[c] as f64) as f32);
            let expected_working_mean = color::apply(
                source.metadata.camera_to_working,
                std::array::from_fn(|c| measured_camera_mean[c] * source.metadata.as_shot[c]),
            );
            let count = ((BLOCK - 64) * (BLOCK - 64)) as f64;
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
            patches.push(serde_json::json!({ "patch":i,"input_camera_rgb":color,"measured_camera_mean":measured_camera_mean,"clipped_camera_mean":clipped_camera_mean,"expected_working_mean":expected_working_mean,"rawpuppy_mean":mean,"reference_mean":reference.as_ref().map(|_| reference_sum.map(|v| v / count)),"inferred_reference_camera_mean":inferred_reference_camera_mean,"region":[x0,y0,BLOCK-64,BLOCK-64]}));
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
    let receipt = serde_json::json!({"scope":"synthetic Bayer DNG controls; equivalent normalized camera matrix, not a copy of the camera's original profile","camera_source_sha256":source_hash,"camera_to_working":camera_matrix,"as_shot":gains,"xyz_to_camera":xyz_to_camera,"black":BLACK,"white":WHITE,"block_size":BLOCK,"fixtures":records});
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
