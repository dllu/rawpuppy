//! Embedded lens metadata, read independently of immutable sensor pixels.
use anyhow::{Context, Result, bail, ensure};
use rawler::{
    decoders::raf::FujiIFD,
    formats::tiff::{IFD, SRational, Value},
    rawsource::RawSource,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RadialTable {
    /// The header stores this pixel radius in its numerator, not n/d.
    pub radius_pixels: f32,
    /// Manufacturer-normalized radius and manufacturer value, in increasing radius.
    pub knots: Vec<[f32; 2]>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct LensProfile {
    pub distortion: Option<RadialTable>,
    pub red_ca: Option<RadialTable>,
    pub blue_ca: Option<RadialTable>,
    pub vignette: Option<RadialTable>,
}

fn rational(value: SRational) -> Result<f32> {
    ensure!(value.d != 0, "Zero denominator in embedded lens data");
    let value = value.n as f64 / value.d as f64;
    ensure!(value.is_finite(), "Nonfinite embedded lens value");
    Ok(value as f32)
}

fn tables(value: &Value, channels: usize) -> Result<Vec<RadialTable>> {
    let Value::SRational(values) = value else {
        bail!("Unsupported Fuji lens table type")
    };
    let header = values.first().context("Empty embedded lens table")?;
    // The apparent first rational is a pair: reference radius / knot count.
    // Its denominator must not be used to scale the reference radius.
    ensure!(
        header.n > 0 && header.d >= 2,
        "Invalid Fuji lens table header"
    );
    let count = usize::try_from(header.d)?;
    let expected = count
        .checked_mul(channels + 1)
        .and_then(|n| n.checked_add(if channels == 2 { 2 } else { 1 }))
        .context("Embedded lens table size overflow")?;
    ensure!(values.len() == expected, "Invalid Fuji lens table length");
    if channels == 2 {
        let repeated = values.last().unwrap();
        ensure!(
            repeated.n == header.n && repeated.d == header.d,
            "Inconsistent Fuji chromatic-aberration table header"
        );
    }
    let radii: Vec<f32> = values[1..=count]
        .iter()
        .copied()
        .map(rational)
        .collect::<Result<_>>()?;
    ensure!(
        radii.first().is_some_and(|r| *r > 0.) && radii.windows(2).all(|r| r[1] > r[0]),
        "Embedded lens radii must be positive and strictly increasing"
    );
    (0..channels)
        .map(|channel| {
            let start = 1 + count * (channel + 1);
            let values = values[start..start + count]
                .iter()
                .copied()
                .map(rational)
                .collect::<Result<Vec<_>>>()?;
            Ok(RadialTable {
                radius_pixels: header.n as f32,
                knots: radii
                    .iter()
                    .copied()
                    .zip(values)
                    .map(|(r, v)| [r, v])
                    .collect(),
            })
        })
        .collect()
}

/// RAF's second TIFF contains these tags. Older files may have only raw pixels
/// at that location; they correctly yield no embedded profile.
pub fn read_raf(source: &RawSource) -> Result<Option<LensProfile>> {
    let signature = source.subview(0, 8)?;
    if signature != b"FUJIFILM" {
        return Ok(None);
    }
    let pointer = source.subview(100, 4)?;
    let offset = u32::from_be_bytes(pointer.try_into()?);
    let marker = source.subview(u64::from(offset), 4)?;
    if marker != b"II*\0" && marker != b"MM\0*" {
        return Ok(None);
    }
    let root = IFD::new_root_with_correction(
        &mut source.reader(),
        0,
        offset,
        0,
        10,
        &[FujiIFD::FujiIFD as u16],
    )?;
    let mut profile = LensProfile::default();
    for (tag, channels) in [
        (FujiIFD::GeometricDistortionParams, 1),
        (FujiIFD::ChromaticAberrationParams, 2),
        (FujiIFD::VignettingParams, 1),
    ] {
        let Some(ifd) = root.find_first_ifd_with_tag(tag) else {
            continue;
        };
        let data = tables(&ifd.get_entry(tag).unwrap().value, channels)
            .with_context(|| format!("Reading {tag:?}"))?;
        let mut data = data.into_iter();
        match tag {
            FujiIFD::GeometricDistortionParams => profile.distortion = data.next(),
            FujiIFD::VignettingParams => profile.vignette = data.next(),
            FujiIFD::ChromaticAberrationParams => {
                profile.red_ca = data.next();
                profile.blue_ca = data.next();
            }
            _ => unreachable!(),
        }
    }
    Ok((profile != LensProfile::default()).then_some(profile))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(channels: usize) -> Value {
        let header = SRational { n: 7280, d: 3 };
        let mut v = vec![header];
        v.extend([1, 2, 3].map(|n| SRational { n, d: 3 }));
        for channel in 0..channels {
            v.extend([10, 20, 30].map(|n| SRational {
                n: n + channel as i32,
                d: 100,
            }));
        }
        if channels == 2 {
            v.push(header);
        }
        Value::SRational(v)
    }

    #[test]
    fn fuji_header_is_radius_and_count_and_ca_channels_are_separate() {
        let single = tables(&value(1), 1).unwrap();
        assert_eq!(single[0].radius_pixels, 7280.);
        assert_eq!(single[0].knots[2], [1., 0.3]);
        let ca = tables(&value(2), 2).unwrap();
        assert_eq!(ca.len(), 2);
        assert_eq!(ca[0].knots[2][1], 0.3);
        assert_eq!(ca[1].knots[2][1], 0.31);
    }

    #[test]
    fn malformed_tables_do_not_reach_processing() {
        for mutation in 0..5 {
            let Value::SRational(mut v) = value(2) else {
                unreachable!()
            };
            match mutation {
                0 => v[0].d = 0,
                1 => v[2].d = 0,
                2 => v[2] = v[1],
                3 => {
                    v.pop();
                }
                4 => v.last_mut().unwrap().n += 1,
                _ => unreachable!(),
            }
            assert!(tables(&Value::SRational(v), 2).is_err());
        }
    }
}
