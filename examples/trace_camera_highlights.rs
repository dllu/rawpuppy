//! Read-only camera-channel and formation diagnostics on a bounded sample grid.
use anyhow::{Result, ensure};
use clap::Parser;
use rawpuppy::{
    color,
    edits::{Edits, ToneMapper},
    input::SensorImage,
    models,
    pipeline::Pipeline,
};
use std::{collections::HashMap, path::PathBuf};

#[derive(Parser)]
struct Args {
    input: PathBuf,
    output: PathBuf,
}
fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(!args.output.exists(), "Choose a new output file");
    let hash = models::sha256(&args.input)?;
    let source = SensorImage::open(&args.input)?;
    let cfa = source
        .cfa
        .as_ref()
        .filter(|_| source.cpp == 1)
        .ok_or_else(|| anyhow::anyhow!("RAW mosaic required"))?;
    let mut histograms: [HashMap<u32, usize>; 3] = std::array::from_fn(|_| HashMap::new());
    for y in source.origin[1]..source.origin[1] + source.active[1] {
        for x in source.origin[0]..source.origin[0] + source.active[0] {
            let channel = cfa.color_at(y, x);
            let value = source.data[y * source.metadata.sensor_width + x];
            *histograms[channel].entry(value.to_bits()).or_default() += 1;
        }
    }
    let peaks: Vec<_> = histograms
        .into_iter()
        .map(|h| {
            let mut values: Vec<_> = h
                .into_iter()
                .map(|(bits, count)| (f32::from_bits(bits), count))
                .collect();
            values.sort_by_key(|a| (std::cmp::Reverse(a.1), a.0.to_bits()));
            let common_bright: Vec<_> = values
                .iter()
                .filter(|(v, _)| *v > 0.5)
                .take(12)
                .copied()
                .collect();
            let max = values
                .iter()
                .map(|(v, _)| *v)
                .fold(f32::NEG_INFINITY, f32::max);
            serde_json::json!({"maximum":max,"most_common_bright_levels":common_bright})
        })
        .collect();
    let mut defaults = Edits::for_image(&source);
    defaults.raw.recover_highlights = false;
    let display = Pipeline::compile(&source, &defaults)?;
    let mut linear_edits = defaults.clone();
    linear_edits.tone.mapper = ToneMapper::Linear;
    let linear = Pipeline::compile(&source, &linear_edits)?;
    let mut samples = Vec::new();
    for y in 1..20 {
        for x in 1..20 {
            let uv = [x as f32 / 20., y as f32 / 20.];
            let camera: [f32; 3] = std::array::from_fn(|channel| {
                let p = linear.geometry.map(uv, channel).unwrap();
                source.sample(p, &defaults.raw).unwrap()[channel]
            });
            let scene = linear.sample(uv);
            let formed = display.sample(uv);
            let mut sample = serde_json::json!({"uv":uv,"camera":camera,"white_balanced_camera":std::array::from_fn::<_,3,_>(|c|camera[c]*source.metadata.as_shot[c]),"scene_linear_rgb":scene,"formed_linear_rgb":formed});
            if color::luminance([scene[0], scene[1], scene[2]]) > 0.7 {
                sample["highlight"] = true.into();
            }
            samples.push(sample);
        }
    }
    ensure!(models::sha256(&args.input)? == hash, "Input changed");
    let record = serde_json::json!({"scope":"read-only sensor and color-stage diagnostics; no reconstruction or quality claim","input_sha256":hash,"metadata":source.metadata,"channel_histogram_peaks":peaks,"samples":samples});
    std::fs::write(&args.output, serde_json::to_string_pretty(&record)? + "\n")?;
    println!(
        "{}",
        serde_json::json!({"histogram_peaks":peaks,"bright_grid_samples":samples.iter().filter(|s|s["highlight"]==true).count()})
    );
    Ok(())
}
