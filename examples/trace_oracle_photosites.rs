//! Fit effective color-transform columns from an independent full-size photosite debug export.
use anyhow::{Result, ensure};
use clap::Parser;
use rawpuppy::{input::SensorImage, models};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    input: PathBuf,
    reference: PathBuf,
    output: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(!args.output.exists(), "Choose a fresh receipt path");
    let source_hash = models::sha256(&args.input)?;
    let reference_hash = models::sha256(&args.reference)?;
    let source = SensorImage::open(&args.input)?;
    let developed = SensorImage::open(&args.reference)?;
    ensure!(
        source.cpp == 1 && source.cfa.is_some(),
        "Expected a mosaic RAW"
    );
    ensure!(
        source.orientation.to_u16() == 1,
        "Use an unrotated reference with sensor-aligned photosites"
    );
    ensure!(
        developed.cpp == 3 && developed.cfa.is_none(),
        "Expected developed RGB"
    );
    ensure!(
        [developed.metadata.width, developed.metadata.height] == source.active,
        "Reference must match active dimensions without resizing"
    );
    let cfa = source.cfa.as_ref().unwrap();
    ensure!(
        cfa.width == 2 && cfa.height == 2,
        "This control requires Bayer photosites"
    );
    let (width, height) = (source.active[0], source.active[1]);
    let mut numerator = [[0f64; 3]; 3];
    let mut denominator = [0f64; 3];
    let mut fitted_samples = [0usize; 3];
    let mut negative_samples = [0usize; 3];
    let mut negative_zero_rgb = [0usize; 3];
    let mut negative_maximum_rgb = [0f32; 3];
    let mut signed_prediction_error = [0f64; 3];
    let mut signed_prediction_maximum = [0f64; 3];
    let mut clipped_prediction_error = [0f64; 3];
    let mut count = [0usize; 3];
    for y in 0..height {
        let sy = y + source.origin[1];
        for x in 0..width {
            let sx = x + source.origin[0];
            let channel = cfa.color_at(sy, sx);
            let value = source.data[sy * source.metadata.sensor_width + sx];
            let rgb = &developed.data[(y * width + x) * 3..(y * width + x + 1) * 3];
            let balanced = f64::from(value) * f64::from(source.metadata.as_shot[channel]);
            count[channel] += 1;
            if value >= 0.001 {
                denominator[channel] += balanced * balanced;
                fitted_samples[channel] += 1;
                for c in 0..3 {
                    numerator[channel][c] += balanced * f64::from(rgb[c]);
                }
            }
            if value < 0. {
                negative_samples[channel] += 1;
                negative_zero_rgb[channel] += usize::from(rgb.iter().all(|v| *v == 0.));
                for v in rgb {
                    negative_maximum_rgb[channel] = negative_maximum_rgb[channel].max(v.abs());
                }
            }
            for (c, actual) in rgb.iter().enumerate() {
                let coefficient = f64::from(source.metadata.camera_to_working[c][channel]);
                let signed_error = (f64::from(*actual) - coefficient * balanced).abs();
                signed_prediction_error[channel] += signed_error;
                signed_prediction_maximum[channel] =
                    signed_prediction_maximum[channel].max(signed_error);
                clipped_prediction_error[channel] +=
                    (f64::from(*actual) - coefficient * balanced.max(0.)).abs();
            }
        }
    }
    ensure!(
        denominator.iter().all(|v| *v > 0.),
        "No positive signal for a color column"
    );
    let fitted_columns: [[f64; 3]; 3] =
        std::array::from_fn(|channel| numerator[channel].map(|v| v / denominator[channel]));
    let mut fitted_error = [0f64; 3];
    let mut fitted_max_error = [0f64; 3];
    for y in 0..height {
        let sy = y + source.origin[1];
        for x in 0..width {
            let sx = x + source.origin[0];
            let channel = cfa.color_at(sy, sx);
            let balanced = f64::from(source.data[sy * source.metadata.sensor_width + sx].max(0.))
                * f64::from(source.metadata.as_shot[channel]);
            for (c, coefficient) in fitted_columns[channel].iter().enumerate() {
                let error = (f64::from(developed.data[(y * width + x) * 3 + c])
                    - coefficient * balanced)
                    .abs();
                fitted_error[channel] += error;
                fitted_max_error[channel] = fitted_max_error[channel].max(error);
            }
        }
    }
    let declared_columns: [[f64; 3]; 3] = std::array::from_fn(|channel| {
        std::array::from_fn(|c| f64::from(source.metadata.camera_to_working[c][channel]))
    });
    ensure!(
        models::sha256(&args.input)? == source_hash
            && models::sha256(&args.reference)? == reference_hash,
        "Input or reference changed"
    );
    let record = serde_json::json!({"scope":"Effective photosite-to-working-RGB column fit and signed/zero-clipped prediction on a full-size independent debug export; no implementation-source access or physical camera-profile claim","error_grouping":"R/G/B photosite groups, each mean averaging all three developed RGB components","photosite_group_counts":count,"rgb_components_compared":developed.data.len(),"dimensions":[width,height],"source_sha256":source_hash,"reference_sha256":reference_hash,"inputs_unchanged":true,"decoded_reference_rgb_sha256":format!("{:x}",Sha256::digest(bytemuck::cast_slice(&developed.data))),"as_shot":source.metadata.as_shot,"declared_columns_rgb":declared_columns,"fitted_columns_rgb":fitted_columns,"coefficient_difference":std::array::from_fn::<_,3,_>(|i|std::array::from_fn::<_,3,_>(|c|fitted_columns[i][c]-declared_columns[i][c])),"fitted_samples":fitted_samples,"negative_samples":negative_samples,"negative_photosites_with_zero_output_rgb":negative_zero_rgb,"negative_photosite_maximum_absolute_rgb":negative_maximum_rgb,"signed_declared_prediction_mean_error_rgb":std::array::from_fn::<_,3,_>(|c|signed_prediction_error[c]/(count[c]*3) as f64),"signed_declared_prediction_maximum_error_rgb":signed_prediction_maximum,"clipped_declared_prediction_mean_error_rgb":std::array::from_fn::<_,3,_>(|c|clipped_prediction_error[c]/(count[c]*3) as f64),"clipped_fitted_prediction_mean_error_rgb":std::array::from_fn::<_,3,_>(|c|fitted_error[c]/(count[c]*3) as f64),"clipped_fitted_prediction_maximum_error_rgb":fitted_max_error});
    std::fs::write(&args.output, serde_json::to_string_pretty(&record)? + "\n")?;
    println!("{record}");
    Ok(())
}
