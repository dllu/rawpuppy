//! Two-illuminant camera matrix selection from as-shot neutral; no pixel clipping.
use crate::color::{self, Matrix};
use anyhow::{Context, Result, ensure};
use rawler::imgop::xyz::{FlatColorMatrix, Illuminant};
use std::collections::HashMap;

// Robertson (1968) isotherm data, as published by Colour Developers (BSD-3-Clause).
// See docs/licenses/colour-robertson.txt. The interpolation implementation is independent.
const ISOTHERMS: [[f64; 4]; 31] = [
    [0., 0.18006, 0.26352, -0.24341],
    [10., 0.18066, 0.26589, -0.25479],
    [20., 0.18133, 0.26846, -0.26876],
    [30., 0.18208, 0.27119, -0.28539],
    [40., 0.18293, 0.27407, -0.30470],
    [50., 0.18388, 0.27709, -0.32675],
    [60., 0.18494, 0.28021, -0.35156],
    [70., 0.18611, 0.28342, -0.37915],
    [80., 0.18740, 0.28668, -0.40955],
    [90., 0.18880, 0.28997, -0.44278],
    [100., 0.19032, 0.29326, -0.47888],
    [125., 0.19462, 0.30141, -0.58204],
    [150., 0.19962, 0.30921, -0.70471],
    [175., 0.20525, 0.31647, -0.84901],
    [200., 0.21142, 0.32312, -1.0182],
    [225., 0.21807, 0.32909, -1.2168],
    [250., 0.22511, 0.33439, -1.4512],
    [275., 0.23247, 0.33904, -1.7298],
    [300., 0.24010, 0.34308, -2.0637],
    [325., 0.24792, 0.34655, -2.4681],
    [350., 0.25591, 0.34951, -2.9641],
    [375., 0.26400, 0.35200, -3.5814],
    [400., 0.27218, 0.35407, -4.3633],
    [425., 0.28039, 0.35577, -5.3762],
    [450., 0.28863, 0.35714, -6.7262],
    [475., 0.29685, 0.35823, -8.5955],
    [500., 0.30505, 0.35907, -11.324],
    [525., 0.31320, 0.35968, -15.628],
    [550., 0.32129, 0.36011, -23.325],
    [575., 0.32931, 0.36038, -40.770],
    [600., 0.33724, 0.36051, -116.45],
];

fn reciprocal_temperature(xyz: [f32; 3]) -> Result<f64> {
    ensure!(
        xyz.iter().all(|v| v.is_finite() && *v > 0.),
        "Invalid inferred camera white point"
    );
    let [x, y, z] = xyz.map(f64::from);
    let denominator = x + 15. * y + 3. * z;
    let (u, v) = (4. * x / denominator, 6. * y / denominator);
    let distance =
        |line: [f64; 4]| (v - line[2] - line[3] * (u - line[1])) / (1. + line[3] * line[3]).sqrt();
    for pair in ISOTHERMS.windows(2) {
        let (a, b) = (distance(pair[0]), distance(pair[1]));
        if a == 0. {
            return Ok(pair[0][0] * 1e-6);
        }
        if a * b <= 0. {
            let fraction = a / (a - b);
            return Ok((pair[0][0] + fraction * (pair[1][0] - pair[0][0])) * 1e-6);
        }
    }
    let nearest = if distance(ISOTHERMS[0]).abs() < distance(ISOTHERMS[30]).abs() {
        ISOTHERMS[0][0]
    } else {
        ISOTHERMS[30][0]
    };
    Ok(nearest * 1e-6)
}

fn white(illuminant: Illuminant) -> Option<[f32; 3]> {
    Some(match illuminant {
        Illuminant::A | Illuminant::Tungsten => [1.09850, 1., 0.35585],
        Illuminant::B => [0.99072, 1., 0.85223],
        Illuminant::C => [0.98074, 1., 1.18232],
        Illuminant::D50 => [0.96422, 1., 0.82521],
        Illuminant::D55 => [0.95682, 1., 0.92149],
        Illuminant::D65 => [0.95047, 1., 1.08883],
        Illuminant::D75 => [0.94972, 1., 1.22638],
        _ => return None,
    })
}

pub(crate) fn supports_pair(matrices: &HashMap<Illuminant, FlatColorMatrix>) -> bool {
    matrices.len() == 2
        && matrices
            .iter()
            .all(|(i, m)| white(*i).is_some() && m.len() == 9)
        && {
            let mut references = matrices.keys().map(|i| white(*i).unwrap());
            references.next() != references.next()
        }
}

pub(crate) struct Calibration {
    pub xyz_to_camera: Matrix,
    pub white: [f32; 3],
    pub forward: Option<Matrix>,
}

#[derive(Default)]
struct ExtraCalibration {
    analog: Option<Matrix>,
    camera: HashMap<Illuminant, Matrix>,
    forward: HashMap<Illuminant, Matrix>,
}

fn read_extra(source: &rawler::rawsource::RawSource) -> Result<ExtraCalibration> {
    use rawler::{
        formats::tiff::{GenericTiffReader, Value, reader::TiffReader},
        tags::DngTag,
    };
    let tiff = GenericTiffReader::new_with_buffer(source.buf(), 0, 0, Some(1))?;
    let root = tiff.root_ifd();
    let signature = |tag| -> Result<&[u8]> {
        root.get_entry(tag)
            .map(|e| {
                let bytes = match &e.value {
                    Value::Byte(bytes) => bytes.as_slice(),
                    _ => e
                        .as_string()
                        .map(|value| value.as_bytes())
                        .context("Unsupported camera calibration signature encoding")?,
                };
                let end = bytes.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1);
                Ok(&bytes[..end])
            })
            .transpose()
            .map(|v| v.unwrap_or(b""))
    };
    let use_camera = signature(DngTag::CameraCalibrationSignature)?
        == signature(DngTag::ProfileCalibrationSignature)?;
    let matrix = |tag| -> Result<Option<Matrix>> {
        let Some(entry) = root.get_entry(tag) else {
            return Ok(None);
        };
        ensure!(
            entry.count() == 9,
            "Camera calibration tag must have nine coefficients"
        );
        let m = std::array::from_fn(|i| std::array::from_fn(|j| entry.force_f32(i * 3 + j)));
        color::inverse(m)?;
        Ok(Some(m))
    };
    let mut extra = ExtraCalibration::default();
    if let Some(entry) = root.get_entry(DngTag::AnalogBalance) {
        ensure!(
            entry.count() == 3,
            "Analog balance must have three channels"
        );
        let balance: [f32; 3] = std::array::from_fn(|i| entry.force_f32(i));
        ensure!(
            balance.iter().all(|v| v.is_finite() && *v > 0.),
            "Invalid camera analog balance"
        );
        extra.analog = Some(std::array::from_fn(|i| {
            std::array::from_fn(|j| if i == j { balance[i] } else { 0. })
        }));
    }
    for (illum, cc, fm) in [
        (
            DngTag::CalibrationIlluminant1,
            DngTag::CameraCalibration1,
            DngTag::ForwardMatrix1,
        ),
        (
            DngTag::CalibrationIlluminant2,
            DngTag::CameraCalibration2,
            DngTag::ForwardMatrix2,
        ),
    ] {
        let illuminant = root.get_entry(illum).map_or(21, |e| e.force_u16(0));
        let Ok(illuminant) = Illuminant::try_from(illuminant) else {
            continue;
        };
        if use_camera && let Some(m) = matrix(cc)? {
            extra.camera.insert(illuminant, m);
        }
        if let Some(m) = matrix(fm)? {
            extra.forward.insert(illuminant, m);
        }
    }
    Ok(extra)
}

pub(crate) fn from_dng(
    source: &rawler::rawsource::RawSource,
    matrices: &HashMap<Illuminant, FlatColorMatrix>,
    as_shot: [f32; 3],
) -> Result<Option<Calibration>> {
    interpolate_with(matrices, as_shot, &read_extra(source)?)
}

pub(crate) fn revision(
    source: &rawler::rawsource::RawSource,
    matrices: &HashMap<Illuminant, FlatColorMatrix>,
) -> Result<u32> {
    let extra = read_extra(source)?;
    Ok(u32::from(
        supports_pair(matrices)
            || (matrices.len() == 1
                && (extra.analog.is_some()
                    || !extra.camera.is_empty()
                    || !extra.forward.is_empty())),
    ))
}

#[cfg(test)]
fn interpolate(
    matrices: &HashMap<Illuminant, FlatColorMatrix>,
    as_shot: [f32; 3],
) -> Result<Option<Calibration>> {
    interpolate_with(matrices, as_shot, &ExtraCalibration::default())
}

fn interpolate_with(
    matrices: &HashMap<Illuminant, FlatColorMatrix>,
    as_shot: [f32; 3],
    extra: &ExtraCalibration,
) -> Result<Option<Calibration>> {
    let single = matrices.len() == 1
        && (extra.analog.is_some() || !extra.camera.is_empty() || !extra.forward.is_empty());
    if !supports_pair(matrices) && !single {
        return Ok(None);
    }
    let mut profiles: Vec<_> = matrices
        .iter()
        .map(|(illuminant, flat)| {
            let reference = white(*illuminant).unwrap_or([0.95047, 1., 1.08883]);
            let matrix: Matrix = std::array::from_fn(|i| std::array::from_fn(|j| flat[i * 3 + j]));
            ensure!(
                flat.iter().all(|v| v.is_finite()),
                "Nonfinite camera calibration matrix"
            );
            color::inverse(matrix)?;
            Ok((reciprocal_temperature(reference)?, matrix, *illuminant))
        })
        .collect::<Result<_>>()?;
    profiles.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (cool, first, first_illum) = profiles[0];
    let (warm, second, second_illum) = *profiles.last().unwrap();
    if !single && (warm - cool).abs() < 1e-8 {
        return Ok(None);
    }
    ensure!(
        as_shot.iter().all(|v| v.is_finite() && *v > 0.),
        "Invalid camera neutral gains"
    );
    let neutral = as_shot.map(|v| 1. / v);
    let mut estimate = [0.96422, 1., 0.82521];
    for _ in 0..64 {
        let weight = if single {
            0.
        } else {
            ((reciprocal_temperature(estimate)? - cool) / (warm - cool)).clamp(0., 1.) as f32
        };
        let blend = |a: Matrix, b: Matrix| {
            std::array::from_fn(|i| {
                std::array::from_fn(|j| a[i][j] * (1. - weight) + b[i][j] * weight)
            })
        };
        let cm = std::array::from_fn(|i| {
            std::array::from_fn(|j| first[i][j] * (1. - weight) + second[i][j] * weight)
        });
        let cc = blend(
            *extra.camera.get(&first_illum).unwrap_or(&color::IDENTITY),
            *extra.camera.get(&second_illum).unwrap_or(&color::IDENTITY),
        );
        let reference_to_camera = color::multiply(extra.analog.unwrap_or(color::IDENTITY), cc);
        let matrix = color::multiply(reference_to_camera, cm);
        let xyz = color::apply(color::inverse(matrix)?, neutral);
        ensure!(
            xyz[1].is_finite() && xyz[1] > 0.,
            "Invalid camera neutral luminance"
        );
        let next = xyz.map(|v| v / xyz[1]);
        let difference = estimate
            .iter()
            .zip(next)
            .map(|(a, b)| (*a - b).abs())
            .fold(0., f32::max);
        if difference < 1e-6 || single {
            let forward = if let (Some(a), Some(b)) = (
                extra.forward.get(&first_illum),
                extra.forward.get(&second_illum),
            ) {
                let fm = blend(*a, *b);
                let reference_inverse = color::inverse(reference_to_camera)?;
                let reference_neutral = color::apply(reference_inverse, neutral);
                ensure!(
                    reference_neutral.iter().all(|v| v.is_finite() && *v > 0.),
                    "Invalid reference camera neutral"
                );
                let scale: Matrix = std::array::from_fn(|i| {
                    std::array::from_fn(|j| {
                        if i == j {
                            1. / reference_neutral[i]
                        } else {
                            0.
                        }
                    })
                });
                let neutral_scale: Matrix = std::array::from_fn(|i| {
                    std::array::from_fn(|j| if i == j { neutral[i] } else { 0. })
                });
                let d50_to_working = color::multiply(
                    color::inverse(color::SRGB_TO_XYZ)?,
                    color::adapt_white(
                        [0.96422, 1., 0.82521],
                        color::apply(color::SRGB_TO_XYZ, [1.; 3]),
                    )?,
                );
                Some(color::multiply(
                    d50_to_working,
                    color::multiply(
                        fm,
                        color::multiply(scale, color::multiply(reference_inverse, neutral_scale)),
                    ),
                ))
            } else {
                None
            };
            return Ok(Some(Calibration {
                xyz_to_camera: matrix,
                white: next,
                forward,
            }));
        }
        // Damping prevents a two-cycle for strongly distinct calibration matrices.
        estimate = std::array::from_fn(|i| 0.5 * (estimate[i] + next[i]));
    }
    anyhow::bail!("Camera white-point interpolation did not converge")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_white_temperatures_and_reciprocal_interpolation() {
        assert!(
            (1. / reciprocal_temperature(white(Illuminant::A).unwrap()).unwrap() - 2856.).abs()
                < 2.
        );
        assert!(
            (1. / reciprocal_temperature(white(Illuminant::D65).unwrap()).unwrap() - 6504.).abs()
                < 4.
        );
        let first = color::inverse(color::SRGB_TO_XYZ).unwrap();
        let second = color::multiply(
            [[1.05, 0.02, -0.01], [0.01, 0.98, 0.01], [-0.01, 0.02, 0.94]],
            first,
        );
        let matrices = HashMap::from([
            (Illuminant::D65, first.into_iter().flatten().collect()),
            (Illuminant::A, second.into_iter().flatten().collect()),
        ]);
        for target in [
            white(Illuminant::A).unwrap(),
            [0.975, 1., 0.62],
            white(Illuminant::D65).unwrap(),
        ] {
            let weight = ((reciprocal_temperature(target).unwrap()
                - reciprocal_temperature(white(Illuminant::D65).unwrap()).unwrap())
                / (reciprocal_temperature(white(Illuminant::A).unwrap()).unwrap()
                    - reciprocal_temperature(white(Illuminant::D65).unwrap()).unwrap()))
            .clamp(0., 1.) as f32;
            let expected: Matrix = std::array::from_fn(|i| {
                std::array::from_fn(|j| first[i][j] * (1. - weight) + second[i][j] * weight)
            });
            let neutral = color::apply(expected, target);
            let gain = neutral.map(|v| neutral[1] / v);
            let actual = interpolate(&matrices, gain).unwrap().unwrap();
            assert!(
                actual
                    .white
                    .iter()
                    .zip(target)
                    .all(|(a, b)| (*a - b).abs() < 0.00002)
            );
            assert!(
                actual
                    .xyz_to_camera
                    .into_iter()
                    .flatten()
                    .zip(expected.into_iter().flatten())
                    .all(|(a, b)| (a - b).abs() < 0.00001)
            );
        }
        assert!(
            interpolate(
                &HashMap::from([(Illuminant::D65, first.into_iter().flatten().collect())]),
                [1.; 3]
            )
            .unwrap()
            .is_none()
        );
    }
}
