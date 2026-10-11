//! Illuminant-based scene calibration. Public CAT16 coefficients (Li et al., 2017),
//! CIE daylight chromaticities and Planck spectra; no display pixels enter estimation.
use crate::{
    color::{self, Matrix},
    edits::{Edits, SceneEdits},
    input::{Metadata, SensorImage},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

pub const D65: [f32; 2] = [0.3127, 0.3290];
const CAT16: Matrix = [
    [0.401288, 0.650173, -0.051461],
    [-0.250268, 1.204414, 0.045854],
    [-0.002079, 0.048952, 0.953127],
];

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Illuminant {
    /// Exact historical sensor-gain calibration; omitted from old recipe JSON.
    #[default]
    Legacy,
    Camera,
    Daylight {
        kelvin: f32,
    },
    BlackBody {
        kelvin: f32,
    },
    Custom {
        xy: [f32; 2],
    },
}
impl Illuminant {
    pub fn is_legacy(&self) -> bool {
        *self == Self::Legacy
    }
    pub fn validate(self) -> Result<()> {
        match self {
            Self::Daylight { kelvin } => ensure!(
                (4000. ..=25000.).contains(&kelvin),
                "Daylight temperature must be 4000–25000 K"
            ),
            Self::BlackBody { kelvin } => ensure!(
                (1000. ..=25000.).contains(&kelvin),
                "Black-body temperature must be 1000–25000 K"
            ),
            Self::Custom { xy } => {
                adaptation(xy)?;
            }
            _ => {}
        }
        Ok(())
    }
    pub fn xy(self, metadata: &Metadata) -> Result<[f32; 2]> {
        self.validate()?;
        match self {
            Self::Legacy | Self::Camera => chromaticity(color::apply(
                metadata
                    .camera_to_xyz
                    .context("No camera illuminant profile is available")?,
                metadata.as_shot.map(|v| 1. / v),
            )),
            Self::Custom { xy } => Ok(xy),
            Self::Daylight { kelvin } => {
                let t = kelvin as f64;
                let x = if t <= 7000. {
                    -4.607e9 / t.powi(3) + 2.9678e6 / t.powi(2) + 99.11 / t + 0.244063
                } else {
                    -2.0064e9 / t.powi(3) + 1.9018e6 / t.powi(2) + 247.48 / t + 0.237040
                };
                Ok([x as f32, (-3. * x * x + 2.87 * x - 0.275) as f32])
            }
            Self::BlackBody { kelvin } => {
                const OBSERVER: [[f64; 4]; 471] = include!("data/cie1931.rs");
                // Exact SI h, c and k. Relative radiance is sufficient for xy.
                let c2 = 6.62607015e-34 * 299792458. / 1.380649e-23;
                let mut xyz = [0.; 3];
                for [nm, x, y, z] in OBSERVER {
                    let um = nm * 1e-3;
                    let power = 1. / (um.powi(5) * (c2 / (nm * 1e-9 * kelvin as f64)).exp_m1());
                    for (sum, observer) in xyz.iter_mut().zip([x, y, z]) {
                        *sum += power * observer;
                    }
                }
                chromaticity(xyz.map(|v| v as f32))
            }
        }
    }
}

pub fn xyz(xy: [f32; 2]) -> Result<[f32; 3]> {
    ensure!(
        xy.iter().all(|v| v.is_finite() && *v > 0.) && xy[0] + xy[1] < 1.,
        "Invalid illuminant chromaticity"
    );
    Ok([xy[0] / xy[1], 1., (1. - xy[0] - xy[1]) / xy[1]])
}
pub fn chromaticity(xyz: [f32; 3]) -> Result<[f32; 2]> {
    ensure!(
        xyz.iter().all(|v| v.is_finite() && *v > 0.),
        "The sample has no usable illuminant colour"
    );
    let sum: f32 = xyz.iter().sum();
    let xy = [xyz[0] / sum, xyz[1] / sum];
    self::xyz(xy)?;
    Ok(xy)
}
pub fn temperature(xy: [f32; 2]) -> Result<f32> {
    Ok((1. / crate::camera_profiles::reciprocal_temperature(xyz(xy)?)?) as f32)
}
/// Project an illuminant into display-linear sRGB for egui's float RGB picker.
/// egui performs the transfer encoding; colour samples from it are also linear.
pub fn swatch(xy: [f32; 2]) -> Result<[f32; 3]> {
    let rgb = color::apply(color::inverse(color::SRGB_TO_XYZ)?, xyz(xy)?);
    let peak = rgb.iter().copied().fold(1e-6, f32::max);
    Ok(rgb.map(|v| (v / peak).clamp(0., 1.)))
}
pub fn adaptation(white: [f32; 2]) -> Result<Matrix> {
    let source = color::apply(CAT16, xyz(white)?);
    let target = color::apply(CAT16, xyz(D65)?);
    ensure!(
        source.iter().all(|v| *v > 1e-6),
        "Illuminant lies outside the usable adaptation range"
    );
    Ok(color::multiply(
        color::inverse(CAT16)?,
        color::multiply(
            diagonal(std::array::from_fn(|i| target[i] / source[i])),
            CAT16,
        ),
    ))
}
fn diagonal(v: [f32; 3]) -> Matrix {
    std::array::from_fn(|i| std::array::from_fn(|j| if i == j { v[i] } else { 0. }))
}
/// Preserve an embedded DNG ForwardMatrix's preferred colour rendering. It is
/// already adapted to the working white, so undo that adaptation before exposing
/// the scene illuminant to the single CAT16 stage.
pub(crate) fn camera_xyz(
    xyz_to_camera: Matrix,
    as_shot: [f32; 3],
    forward: Option<Matrix>,
) -> Result<Matrix> {
    let physical = color::inverse(xyz_to_camera)?;
    let neutral = color::apply(physical, as_shot.map(|v| 1. / v));
    ensure!(
        neutral[1].is_finite() && neutral[1] > 1e-8,
        "Invalid camera neutral luminance"
    );
    if let Some(forward) = forward {
        let inverse_adaptation = color::inverse(adaptation(chromaticity(neutral)?)?)?;
        Ok(color::multiply(
            inverse_adaptation,
            color::multiply(
                color::SRGB_TO_XYZ,
                color::multiply(forward, diagonal(as_shot)),
            ),
        ))
    } else {
        Ok(physical.map(|row| row.map(|v| v / neutral[1])))
    }
}
pub fn matrix(metadata: &Metadata, scene: &SceneEdits) -> Result<Matrix> {
    let base = if scene.illuminant.is_legacy() {
        color::multiply(
            metadata.camera_to_working,
            diagonal(std::array::from_fn(|i| {
                metadata.as_shot[i] * scene.calibration[i]
            })),
        )
    } else {
        let physical = metadata
            .camera_to_xyz
            .context("No camera illuminant profile is available")?;
        color::multiply(
            color::inverse(color::SRGB_TO_XYZ)?,
            color::multiply(
                adaptation(scene.illuminant.xy(metadata)?)?,
                color::multiply(physical, diagonal(scene.calibration)),
            ),
        )
    };
    Ok(color::multiply(scene.mixer, base))
}

#[derive(Clone, Debug)]
pub struct Sample {
    pub xy: [f32; 2],
    pub pixels: usize,
}

/// Mean scene XYZ (gray world / neutral patch). Region is in the corrected,
/// cropped canvas, independent of zoom. Bounded sparse MHC samples avoid a full
/// 100 MP reconstruction; dark, clipped and transparent samples are excluded.
pub fn estimate(image: &SensorImage, edits: &Edits, region: [f32; 4]) -> Result<Sample> {
    ensure!(
        region.iter().all(|v| v.is_finite())
            && region[0] >= 0.
            && region[1] >= 0.
            && region[2] > 0.
            && region[3] > 0.
            && region[0] + region[2] <= 1.000001
            && region[1] + region[3] <= 1.000001,
        "Invalid calibration sampling box"
    );
    let physical = image
        .metadata
        .camera_to_xyz
        .context("No camera illuminant profile is available")?;
    let mut analysis = edits.clone();
    analysis.raw.reconstruction = image.reconstruction;
    analysis.display.retouch.clear();
    analysis.display.synthesis.clear();
    let pipeline = crate::pipeline::Pipeline::compile(image, &analysis)?;
    let aspect = image.metadata.height as f32 * edits.geometry.crop[3] * region[3]
        / (image.metadata.width as f32 * edits.geometry.crop[2] * region[2]);
    let width = (512. / aspect.max(1.)).round().max(16.) as usize;
    let height = (512. * aspect.min(1.)).round().max(16.) as usize;
    let mut sums = [0f64; 3];
    let mut pixels = 0;
    for y in 0..height {
        for x in 0..width {
            let uv = [
                region[0] + (x as f32 + 0.5) / width as f32 * region[2],
                region[1] + (y as f32 + 0.5) / height as f32 * region[3],
            ];
            let Some(p) = pipeline.geometry.map(uv, 1) else {
                continue;
            };
            let Some(mut rgb) = image.sample(p, &analysis.raw) else {
                continue;
            };
            let mut valid = true;
            for c in [0, 2] {
                if let Some(sample) = pipeline
                    .geometry
                    .map(uv, c)
                    .and_then(|p| image.sample(p, &analysis.raw))
                {
                    rgb[c] = sample[c];
                } else {
                    valid = false;
                }
            }
            if !valid || image.raw_integer && rgb.iter().any(|v| *v >= 0.98) {
                continue;
            }
            let xyz = color::apply(physical, rgb);
            if xyz.iter().any(|v| !v.is_finite() || *v <= 0.) || xyz[1] < 1e-5 {
                continue;
            }
            for i in 0..3 {
                sums[i] += xyz[i] as f64;
            }
            pixels += 1;
        }
    }
    ensure!(
        pixels >= 16,
        "Choose a brighter, unclipped neutral area (too few usable samples)"
    );
    let xy = chromaticity(sums.map(|v| (v / pixels as f64) as f32))?;
    adaptation(xy)?;
    Ok(Sample { xy, pixels })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_forward_profile_survives_camera_adaptation_without_double_balance() {
        let source = SensorImage::from_rgb(2, 2, vec![0.2; 12]).unwrap();
        let mut metadata = source.metadata;
        metadata.as_shot = [2., 1., 1.5];
        // A neutral-preserving profile with nontrivial channel mixing.
        let forward = [[1.1, -0.1, 0.], [-0.05, 1.15, -0.1], [0., -0.1, 1.1]];
        metadata.camera_to_xyz = Some(
            camera_xyz(
                color::inverse(color::SRGB_TO_XYZ).unwrap(),
                metadata.as_shot,
                Some(forward),
            )
            .unwrap(),
        );
        let scene = SceneEdits {
            illuminant: Illuminant::Camera,
            ..Default::default()
        };
        let expected = color::multiply(forward, diagonal(metadata.as_shot));
        let actual = matrix(&metadata, &scene).unwrap();
        for (a, b) in actual
            .into_iter()
            .flatten()
            .zip(expected.into_iter().flatten())
        {
            assert!((a - b).abs() < 3e-6, "{a} vs {b}");
        }
    }
}
