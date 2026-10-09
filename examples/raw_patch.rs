//! Sensor-coordinate RAW patches for independent reconstruction comparisons.
use anyhow::{Result, ensure};
use clap::Parser;
use rawpuppy::{color, edits::RawEdits, input::SensorImage, models};
use std::{io::Write, path::PathBuf};

#[derive(Parser)]
struct Args {
    input: PathBuf,
    output: PathBuf,
    #[arg(long, num_args = 4, default_values_t = [0usize, 0, 512, 512])]
    crop: Vec<usize>,
    #[arg(long, default_value_t = 0.)]
    denoise: f32,
    #[arg(long)]
    hot_pixels: bool,
}

fn write_floats(path: &std::path::Path, values: &[f32]) -> Result<()> {
    let mut file = std::io::BufWriter::new(std::fs::File::create_new(path)?);
    for value in values {
        file.write_all(&value.to_le_bytes())?;
    }
    file.flush()?;
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(!args.output.exists(), "Choose a new output directory");
    let source = SensorImage::open(&args.input)?;
    let cfa = source
        .cfa
        .as_ref()
        .filter(|c| c.width == 2 && c.height == 2);
    let cfa = cfa.ok_or_else(|| anyhow::anyhow!("This comparison requires RGB Bayer RAW"))?;
    let [requested_x, requested_y, width, height] = args.crop.as_slice() else {
        anyhow::bail!("Provide x, y, width, height");
    };
    ensure!(
        *width > 0 && *height > 0 && width % 32 == 0 && height % 32 == 0,
        "Comparison patches must have nonzero dimensions divisible by 32"
    );
    ensure!(
        args.denoise.is_finite() && args.denoise >= 0.,
        "Invalid sensor denoise standard deviation"
    );
    // Align the comparison context to RGGB without changing the decoded source.
    let (rx, ry) = (0..2)
        .flat_map(|y| (0..2).map(move |x| (x, y)))
        .find(|&(x, y)| cfa.color_at(y, x) == 0)
        .ok_or_else(|| anyhow::anyhow!("Missing red CFA sample"))?;
    let x = requested_x
        .checked_add((rx + 2 - requested_x % 2) % 2)
        .ok_or_else(|| anyhow::anyhow!("Patch origin overflow"))?;
    let y = requested_y
        .checked_add((ry + 2 - requested_y % 2) % 2)
        .ok_or_else(|| anyhow::anyhow!("Patch origin overflow"))?;
    ensure!(
        cfa.color_at(y, x + 1) == 1
            && cfa.color_at(y + 1, x) == 1
            && cfa.color_at(y + 1, x + 1) == 2,
        "Unsupported Bayer layout"
    );
    ensure!(
        x.checked_add(*width)
            .is_some_and(|v| v <= source.metadata.sensor_width)
            && y.checked_add(*height)
                .is_some_and(|v| v <= source.metadata.sensor_height),
        "Patch is outside the original sensor"
    );
    let count = rawpuppy::input::pixel_count(*width, *height, 1)?;
    let mut mosaic = Vec::new();
    let mut reconstructed = Vec::new();
    mosaic.try_reserve_exact(count)?;
    reconstructed.try_reserve_exact(
        count
            .checked_mul(3)
            .ok_or_else(|| anyhow::anyhow!("Patch size overflow"))?,
    )?;
    let edits = RawEdits {
        hot_pixels: args.hot_pixels,
        denoise: args.denoise,
        ..Default::default()
    };
    for yy in y..y + height {
        for xx in x..x + width {
            mosaic.push(source.data[yy * source.metadata.sensor_width + xx]);
            reconstructed.extend(source.reconstruct(xx as isize, yy as isize, &edits));
        }
    }
    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir(&args.output)?;
    write_floats(&args.output.join("mosaic.f32"), &mosaic)?;
    write_floats(&args.output.join("mhc.f32"), &reconstructed)?;
    let gain = std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            if i == j {
                source.metadata.as_shot[i]
            } else {
                0.
            }
        })
    });
    let to_working = color::multiply(source.metadata.camera_to_working, gain);
    let mut preview = image::RgbImage::new((*width).try_into()?, (*height).try_into()?);
    for (camera, encoded) in reconstructed
        .as_chunks::<3>()
        .0
        .iter()
        .zip(preview.pixels_mut())
    {
        let formed = color::agx(color::apply(to_working, *camera));
        for c in 0..3 {
            encoded[c] = (color::srgb_encode(formed[c]).clamp(0., 1.) * 255.).round() as u8;
        }
    }
    preview.save(args.output.join("preview.png"))?;
    let record = serde_json::json!({
        "source_sha256": models::sha256(&args.input)?,
        "metadata": source.metadata,
        "crop_xywh": [x, y, width, height],
        "requested_crop_xywh": args.crop,
        "orientation": "unrotated sensor coordinates; red at patch top-left",
        "data": "little-endian float32; row-major mosaic and interleaved camera RGB",
        "packing": ["R top-left", "G top-right", "G bottom-left", "B bottom-right"],
        "raw_edits": edits,
        "camera_to_linear_srgb_with_as_shot": to_working,
        "mosaic_sha256": models::sha256(&args.output.join("mosaic.f32"))?,
        "mhc_sha256": models::sha256(&args.output.join("mhc.f32"))?,
    });
    std::fs::write(
        args.output.join("metadata.json"),
        serde_json::to_string_pretty(&record)? + "\n",
    )?;
    println!("{record}");
    Ok(())
}
