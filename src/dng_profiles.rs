//! Extended linear DNG profiles: custom illuminants and three calibration slots.
//! This product includes DNG technology under license by Adobe.
use crate::{
    camera_profiles::{self, Calibration},
    color::{self, Matrix},
};
use anyhow::{Context, Result, bail, ensure};
use rawler::{
    bits::Endian,
    formats::tiff::{GenericTiffReader, IFD, Value, reader::TiffReader},
    rawsource::RawSource,
    tags::DngTag,
};

// Published CIE numerical observer data from Colour Developers, BSD-3-Clause.
const OBSERVER: [[f64; 4]; 471] = include!("data/cie1931.rs");

struct Profile {
    white: [f32; 3],
    color: Matrix,
    camera: Matrix,
    forward: Option<Matrix>,
}

fn extended(root: &IFD) -> bool {
    root.has_entry(DngTag::AsShotWhiteXY)
        || root.has_entry(DngTag::ColorMatrix3)
        || root.has_entry(DngTag::CalibrationIlluminant3)
        || [
            DngTag::CalibrationIlluminant1,
            DngTag::CalibrationIlluminant2,
        ]
        .into_iter()
        .any(|tag| root.get_entry(tag).is_some_and(|e| e.force_u16(0) == 255))
}

pub(crate) fn revision(source: &RawSource) -> Result<Option<u32>> {
    let tiff = GenericTiffReader::new_with_buffer(source.buf(), 0, 0, Some(1))?;
    let root = tiff.root_ifd();
    Ok(extended(root).then(|| {
        if root.has_entry(DngTag::AsShotWhiteXY) {
            3
        } else {
            2
        }
    }))
}

fn bytes(value: &Value) -> Result<&[u8]> {
    match value {
        Value::Undefined(v) | Value::Byte(v) => Ok(v),
        _ => bail!("Custom illuminant must contain binary data"),
    }
}

fn custom_white(data: &[u8], endian: Endian) -> Result<[f32; 3]> {
    let u16_at = |offset: usize| -> Result<u16> {
        let array: [u8; 2] = data
            .get(offset..offset + 2)
            .context("Truncated custom illuminant")?
            .try_into()?;
        Ok(if endian == Endian::Little {
            u16::from_le_bytes(array)
        } else {
            u16::from_be_bytes(array)
        })
    };
    let u32_at = |offset: usize| -> Result<u32> {
        let array: [u8; 4] = data
            .get(offset..offset + 4)
            .context("Truncated custom illuminant")?
            .try_into()?;
        Ok(if endian == Endian::Little {
            u32::from_le_bytes(array)
        } else {
            u32::from_be_bytes(array)
        })
    };
    let rational = |offset: usize| -> Result<f64> {
        let denominator = u32_at(offset + 4)?;
        ensure!(
            denominator != 0,
            "Zero custom illuminant rational denominator"
        );
        Ok(f64::from(u32_at(offset)?) / f64::from(denominator))
    };
    match u16_at(0)? {
        0 => {
            ensure!(data.len() == 18, "Invalid custom xy illuminant length");
            let (x, y) = (rational(2)?, rational(10)?);
            ensure!(
                x > 0. && y > 0. && x + y < 1.,
                "Invalid custom illuminant chromaticity"
            );
            Ok([(x / y) as f32, 1., ((1. - x - y) / y) as f32])
        }
        1 => {
            let count = usize::try_from(u32_at(2)?)?;
            ensure!(
                count >= 2
                    && count <= data.len().saturating_sub(22) / 8
                    && data.len() == 22 + count * 8,
                "Invalid custom illuminant spectrum length"
            );
            let start = rational(6)?;
            let spacing = rational(14)?;
            ensure!(spacing > 0., "Invalid custom spectrum wavelength spacing");
            let mut powers = Vec::new();
            powers.try_reserve_exact(count)?;
            for i in 0..count {
                powers.push(rational(22 + i * 8)?);
            }
            let mut xyz = [0f64; 3];
            for (i, row) in OBSERVER.iter().enumerate() {
                let position = ((row[0] - start) / spacing).clamp(0., (count - 1) as f64);
                let index = position.floor() as usize;
                let fraction = position - index as f64;
                let power =
                    powers[index] * (1. - fraction) + powers[(index + 1).min(count - 1)] * fraction;
                let weight = if i == 0 || i == OBSERVER.len() - 1 {
                    0.5
                } else {
                    1.
                };
                for c in 0..3 {
                    xyz[c] += power * row[c + 1] * weight;
                }
            }
            ensure!(
                xyz.iter().all(|v| v.is_finite() && *v > 0.),
                "Custom illuminant spectrum has no valid tristimulus response"
            );
            Ok(xyz.map(|v| (v / xyz[1]) as f32))
        }
        _ => bail!("Unknown custom illuminant data type"),
    }
}

fn matrix(root: &IFD, tag: DngTag) -> Result<Option<Matrix>> {
    let Some(entry) = root.get_entry(tag) else {
        return Ok(None);
    };
    ensure!(
        entry.count() == 9,
        "DNG profile matrix requires nine coefficients"
    );
    let m: Matrix = std::array::from_fn(|i| std::array::from_fn(|j| entry.force_f32(i * 3 + j)));
    ensure!(
        m.iter().flatten().all(|v| v.is_finite()),
        "Nonfinite DNG profile matrix"
    );
    Ok(Some(m))
}

fn signature(root: &IFD, tag: DngTag) -> Result<&[u8]> {
    let Some(entry) = root.get_entry(tag) else {
        return Ok(b"");
    };
    let data = match &entry.value {
        Value::Byte(v) => v.as_slice(),
        _ => entry
            .as_string()
            .map(|s| s.as_bytes())
            .context("Invalid DNG calibration signature")?,
    };
    let end = data.iter().rposition(|v| *v != 0).map_or(0, |i| i + 1);
    Ok(&data[..end])
}

fn read_profiles(root: &IFD) -> Result<(Vec<Profile>, Matrix)> {
    let camera_matches = signature(root, DngTag::CameraCalibrationSignature)?
        == signature(root, DngTag::ProfileCalibrationSignature)?;
    let mut profiles = Vec::new();
    for (illum, cm, cc, fm, data) in [
        (
            DngTag::CalibrationIlluminant1,
            DngTag::ColorMatrix1,
            DngTag::CameraCalibration1,
            DngTag::ForwardMatrix1,
            DngTag::IlluminantData1,
        ),
        (
            DngTag::CalibrationIlluminant2,
            DngTag::ColorMatrix2,
            DngTag::CameraCalibration2,
            DngTag::ForwardMatrix2,
            DngTag::IlluminantData2,
        ),
        (
            DngTag::CalibrationIlluminant3,
            DngTag::ColorMatrix3,
            DngTag::CameraCalibration3,
            DngTag::ForwardMatrix3,
            DngTag::IlluminantData3,
        ),
    ] {
        let Some(color) = matrix(root, cm)? else {
            ensure!(
                !root.has_entry(illum),
                "Illuminant slot has no color matrix"
            );
            continue;
        };
        color::inverse(color)?;
        let id = root.get_entry(illum).map_or(21, |e| e.force_u16(0));
        let white = if id == 255 {
            custom_white(
                bytes(
                    &root
                        .get_entry(data)
                        .context("Missing custom illuminant data")?
                        .value,
                )?,
                root.endian,
            )?
        } else {
            camera_profiles::white(
                id.try_into()
                    .map_err(|_| anyhow::anyhow!("Unsupported DNG calibration illuminant"))?,
            )
            .context("Unsupported DNG calibration illuminant")?
        };
        profiles.push(Profile {
            white,
            color,
            camera: if camera_matches {
                matrix(root, cc)?.unwrap_or(color::IDENTITY)
            } else {
                color::IDENTITY
            },
            forward: matrix(root, fm)?,
        });
    }
    ensure!(!profiles.is_empty(), "DNG profile has no color calibration");
    if root.has_entry(DngTag::CalibrationIlluminant3) || root.has_entry(DngTag::ColorMatrix3) {
        ensure!(
            profiles.len() == 3,
            "Third calibration requires the first two profiles"
        );
        let count = profiles.iter().filter(|p| p.forward.is_some()).count();
        ensure!(
            count == 0 || count == 3,
            "Three-illuminant profile requires all forward matrices or none"
        );
    }
    for i in 0..profiles.len() {
        for j in i + 1..profiles.len() {
            ensure!(
                uv(profiles[i].white) != uv(profiles[j].white),
                "Duplicate DNG calibration white points"
            );
        }
    }
    let analog = if let Some(entry) = root.get_entry(DngTag::AnalogBalance) {
        ensure!(entry.count() == 3, "Analog balance requires three channels");
        let values: [f32; 3] = std::array::from_fn(|i| entry.force_f32(i));
        ensure!(
            values.iter().all(|v| v.is_finite() && *v > 0.),
            "Invalid analog balance"
        );
        diagonal(values)
    } else {
        color::IDENTITY
    };
    Ok((profiles, analog))
}

fn uv(xyz: [f32; 3]) -> [f64; 2] {
    let [x, y, z] = xyz.map(f64::from);
    let d = x + 15. * y + 3. * z;
    [4. * x / d, 6. * y / d]
}

fn segment_weights(point: [f64; 2], a: [f64; 2], b: [f64; 2]) -> (f64, f64) {
    let direction = [b[0] - a[0], b[1] - a[1]];
    let length = direction[0] * direction[0] + direction[1] * direction[1];
    let t = if length > 0. {
        ((point[0] - a[0]) * direction[0] + (point[1] - a[1]) * direction[1]) / length
    } else {
        0.
    }
    .clamp(0., 1.);
    let delta = [
        point[0] - a[0] - t * direction[0],
        point[1] - a[1] - t * direction[1],
    ];
    (t, delta[0] * delta[0] + delta[1] * delta[1])
}

fn weights(profiles: &[Profile], white: [f32; 3]) -> Result<[f32; 3]> {
    if profiles.len() == 1 {
        return Ok([1., 0., 0.]);
    }
    if profiles.len() == 2 {
        let a = camera_profiles::reciprocal_temperature(profiles[0].white)?;
        let b = camera_profiles::reciprocal_temperature(profiles[1].white)?;
        let t = if (b - a).abs() > 1e-10 {
            ((camera_profiles::reciprocal_temperature(white)? - a) / (b - a)).clamp(0., 1.)
        } else {
            segment_weights(uv(white), uv(profiles[0].white), uv(profiles[1].white)).0
        };
        return Ok([(1. - t) as f32, t as f32, 0.]);
    }
    let [a, b, c] = [
        uv(profiles[0].white),
        uv(profiles[1].white),
        uv(profiles[2].white),
    ];
    let p = uv(white);
    for (i, profile) in profiles.iter().enumerate() {
        let vertex = uv(profile.white);
        if (p[0] - vertex[0]).powi(2) + (p[1] - vertex[1]).powi(2) < 1e-18 {
            let mut w = [0.; 3];
            w[i] = 1.;
            return Ok(w);
        }
    }
    let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if d.abs() > 1e-12 {
        let x = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / d;
        let y = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / d;
        let z = 1. - x - y;
        if x >= -1e-8 && y >= -1e-8 && z >= -1e-8 {
            let sum = x.max(0.) + y.max(0.) + z.max(0.);
            return Ok([
                (x.max(0.) / sum) as f32,
                (y.max(0.) / sum) as f32,
                (z.max(0.) / sum) as f32,
            ]);
        }
    }
    let mut best = ([1., 0., 0.], f64::INFINITY, f64::INFINITY);
    for (i, j) in [(0, 1), (1, 2), (2, 0)] {
        let (t, distance) = segment_weights(p, uv(profiles[i].white), uv(profiles[j].white));
        let (a, b) = (uv(profiles[i].white), uv(profiles[j].white));
        let length = (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
        if distance < best.1 - 1e-16 || ((distance - best.1).abs() < 1e-16 && length < best.2) {
            let mut w = [0.; 3];
            w[i] = (1. - t) as f32;
            w[j] = t as f32;
            best = (w, distance, length);
        }
    }
    Ok(best.0)
}

fn diagonal(values: [f32; 3]) -> Matrix {
    std::array::from_fn(|i| std::array::from_fn(|j| if i == j { values[i] } else { 0. }))
}

fn solve(
    profiles: &[Profile],
    analog: Matrix,
    gains: [f32; 3],
    declared_white: Option<[f32; 3]>,
) -> Result<Calibration> {
    ensure!(
        gains.iter().all(|v| v.is_finite() && *v > 0.),
        "Invalid camera neutral"
    );
    let neutral = gains.map(|v| 1. / v);
    let mut estimate = declared_white.unwrap_or([0.96422, 1., 0.82521]);
    for _ in 0..96 {
        let w = weights(profiles, estimate)?;
        let blend = |get: fn(&Profile) -> Matrix| -> Matrix {
            std::array::from_fn(|i| {
                std::array::from_fn(|j| {
                    profiles
                        .iter()
                        .enumerate()
                        .map(|(p, v)| get(v)[i][j] * w[p])
                        .sum()
                })
            })
        };
        let cc = blend(|p| p.camera);
        let reference_to_camera = color::multiply(analog, cc);
        let cm = color::multiply(reference_to_camera, blend(|p| p.color));
        let xyz = color::apply(color::inverse(cm)?, neutral);
        ensure!(
            xyz.iter().all(|v| v.is_finite() && *v > 0.),
            "Invalid inferred DNG white point"
        );
        let next = declared_white.unwrap_or_else(|| xyz.map(|v| v / xyz[1]));
        let error = estimate
            .iter()
            .zip(next)
            .map(|(a, b)| (*a - b).abs())
            .fold(0., f32::max);
        if declared_white.is_some() || profiles.len() == 1 || error < 1e-6 {
            let forward = if profiles.iter().all(|p| p.forward.is_some()) {
                let fm = blend(|p| p.forward.unwrap());
                let inverse = color::inverse(reference_to_camera)?;
                let reference_neutral = color::apply(inverse, neutral);
                ensure!(
                    reference_neutral.iter().all(|v| v.is_finite() && *v > 0.),
                    "Invalid reference neutral"
                );
                let pcs_to_working = color::multiply(
                    color::inverse(color::SRGB_TO_XYZ)?,
                    color::adapt_white(
                        [0.96422, 1., 0.82521],
                        color::apply(color::SRGB_TO_XYZ, [1.; 3]),
                    )?,
                );
                Some(color::multiply(
                    pcs_to_working,
                    color::multiply(
                        fm,
                        color::multiply(
                            diagonal(reference_neutral.map(|v| 1. / v)),
                            color::multiply(inverse, diagonal(neutral)),
                        ),
                    ),
                ))
            } else {
                None
            };
            return Ok(Calibration {
                xyz_to_camera: cm,
                white: next,
                forward,
                revision: if declared_white.is_some() { 3 } else { 2 },
                as_shot: Some(gains),
            });
        }
        estimate = std::array::from_fn(|i| 0.5 * (estimate[i] + next[i]));
    }
    bail!("Extended DNG white-point interpolation did not converge")
}

pub(crate) fn from_source(source: &RawSource, mut gains: [f32; 3]) -> Result<Option<Calibration>> {
    let tiff = GenericTiffReader::new_with_buffer(source.buf(), 0, 0, Some(1))?;
    if !extended(tiff.root_ifd()) {
        return Ok(None);
    }
    let (profiles, analog) = read_profiles(tiff.root_ifd())?;
    let root = tiff.root_ifd();
    let mut declared_white = None;
    if let Some(entry) = root.get_entry(DngTag::AsShotWhiteXY) {
        ensure!(
            !root.has_entry(DngTag::AsShotNeutral),
            "DNG white balance must use either camera neutral or xy, not both"
        );
        ensure!(
            entry.count() == 2,
            "DNG as-shot xy white point requires two values"
        );
        let (x, y) = (entry.force_f32(0), entry.force_f32(1));
        ensure!(
            x.is_finite() && y.is_finite() && x > 0. && y > 0. && x + y < 1.,
            "Invalid DNG as-shot white point"
        );
        let white = [x / y, 1., (1. - x - y) / y];
        declared_white = Some(white);
        let w = weights(&profiles, white)?;
        let blend = |get: fn(&Profile) -> Matrix| -> Matrix {
            std::array::from_fn(|i| {
                std::array::from_fn(|j| {
                    profiles
                        .iter()
                        .enumerate()
                        .map(|(p, v)| get(v)[i][j] * w[p])
                        .sum()
                })
            })
        };
        let xyz_to_camera = color::multiply(
            analog,
            color::multiply(blend(|p| p.camera), blend(|p| p.color)),
        );
        let neutral = color::apply(xyz_to_camera, white);
        ensure!(
            neutral.iter().all(|v| v.is_finite() && *v > 0.),
            "Invalid camera neutral derived from DNG xy white point"
        );
        gains = neutral.map(|v| neutral[1] / v);
    }
    solve(&profiles, analog, gains, declared_white).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile(white: [f32; 3], color: Matrix) -> Profile {
        Profile {
            white,
            color,
            camera: color::IDENTITY,
            forward: None,
        }
    }
    #[test]
    fn three_profile_vertices_interior_and_boundary_are_convex() {
        let profiles = [
            profile([1.09850, 1., 0.35585], color::IDENTITY),
            profile([0.95047, 1., 1.08883], color::IDENTITY),
            profile([0.9, 1., 0.6], color::IDENTITY),
        ];
        for i in 0..3 {
            let w = weights(&profiles, profiles[i].white).unwrap();
            assert!(
                w.iter()
                    .enumerate()
                    .all(|(j, v)| (*v - f32::from(i == j)).abs() < 1e-5)
            );
        }
        for white in [[0.97, 1., 0.7], [1.3, 1., 0.2], [0.6, 1., 1.5]] {
            let w = weights(&profiles, white).unwrap();
            assert!(w.iter().all(|v| *v >= 0.));
            assert!((w.iter().sum::<f32>() - 1.).abs() < 1e-6);
        }
        let collinear = [
            profile([1.1, 1., 0.4], color::IDENTITY),
            profile([0.9, 1., 1.2], color::IDENTITY),
            profile([1., 1., 0.8], color::IDENTITY),
        ];
        assert_eq!(
            weights(&collinear, collinear[2].white).unwrap(),
            [0., 0., 1.]
        );
        let w = weights(&collinear, [1.05, 1., 0.6]).unwrap();
        assert!(w[1] < 1e-5 && w[0] > 0. && w[2] > 0.);
    }
    #[test]
    fn custom_xy_and_equal_energy_spectra_are_endian_correct() {
        for endian in [Endian::Little, Endian::Big] {
            let mut xy = Vec::new();
            xy.extend(if endian == Endian::Little {
                0u16.to_le_bytes()
            } else {
                0u16.to_be_bytes()
            });
            for v in [1u32, 3, 1, 3] {
                xy.extend(if endian == Endian::Little {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                });
            }
            let white = custom_white(&xy, endian).unwrap();
            assert!(white.iter().all(|v| (*v - 1.).abs() < 1e-6));
            let mut spectrum = Vec::new();
            spectrum.extend(if endian == Endian::Little {
                1u16.to_le_bytes()
            } else {
                1u16.to_be_bytes()
            });
            for v in [2u32, 360, 1, 470, 1, 1, 1, 1, 1] {
                spectrum.extend(if endian == Endian::Little {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                });
            }
            let white = custom_white(&spectrum, endian).unwrap();
            assert!(white.iter().all(|v| (*v - 1.).abs() < 0.001));
            assert!(custom_white(&xy[..17], endian).is_err());
            let mut zero_denominator = xy.clone();
            zero_denominator[6..10].fill(0);
            assert!(custom_white(&zero_denominator, endian).is_err());
            let mut zero_spacing = spectrum.clone();
            zero_spacing[14..18].fill(0);
            assert!(custom_white(&zero_spacing, endian).is_err());
            let mut no_power = spectrum.clone();
            no_power[22..26].fill(0);
            no_power[30..34].fill(0);
            assert!(custom_white(&no_power, endian).is_err());
            assert!(custom_white(&spectrum[..10], endian).is_err());
        }
    }
}
