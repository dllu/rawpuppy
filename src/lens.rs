//! Embedded lens metadata, read independently of immutable sensor pixels.
use crate::edits::{LensEdits, LensMode};
use anyhow::{Context, Result, bail, ensure};
use rawler::{
    decoders::raf::FujiIFD,
    formats::tiff::{IFD, SRational, Value},
    rawsource::RawSource,
};
use serde::{Deserialize, Serialize};

pub const LUT_SAMPLES: usize = 4097;
pub const MAX_RADIUS2: f32 = 1.25;

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

impl LensProfile {
    pub fn validate(&self) -> Result<()> {
        for table in [
            &self.distortion,
            &self.red_ca,
            &self.blue_ca,
            &self.vignette,
        ]
        .into_iter()
        .flatten()
        {
            table.validate()?;
        }
        if let Some(table) = &self.distortion {
            ensure!(
                table.knots.iter().all(|k| k[1] > -100.),
                "Embedded distortion must retain positive radial scale"
            );
        }
        if let Some(table) = &self.vignette {
            ensure!(
                table.knots.iter().all(|k| k[1] > 0.),
                "Embedded lens transmission must be positive"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Correction {
    pub distortion: bool,
    pub vignette: bool,
    /// Interleaved backward radial scale and scene-linear gain; uniform in r².
    pub lut: Vec<[f32; 2]>,
    pub frame_scale: f32,
}
impl Default for Correction {
    fn default() -> Self {
        Self {
            distortion: false,
            vignette: false,
            lut: Vec::new(),
            frame_scale: 1.,
        }
    }
}

impl RadialTable {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.radius_pixels.is_finite() && self.radius_pixels > 0.,
            "Invalid lens reference radius"
        );
        ensure!(
            !self.knots.is_empty() && self.knots[0][0] >= 0.,
            "Missing lens radius samples"
        );
        ensure!(
            self.knots.iter().flatten().all(|v| v.is_finite())
                && self.knots.windows(2).all(|k| k[1][0] > k[0][0]),
            "Invalid lens samples"
        );
        Ok(())
    }
    fn interpolate(&self, radius: f32, center: f32) -> f32 {
        let mut previous = [0., center];
        for &next in &self.knots {
            if next[0] == 0. {
                previous = next;
                continue;
            }
            if radius <= next[0] {
                let t = (radius - previous[0]) / (next[0] - previous[0]);
                return previous[1] + t * (next[1] - previous[1]);
            }
            previous = next;
        }
        previous[1]
    }
}

impl Correction {
    pub fn compile(
        profile: Option<&LensProfile>,
        e: &LensEdits,
        width: usize,
        height: usize,
    ) -> Result<Self> {
        if e.mode == LensMode::Off {
            return Ok(Self::default());
        }
        ensure!(
            width > 0 && height > 0,
            "Lens correction needs nonzero image dimensions"
        );
        let profile = profile.context("This image has no usable embedded lens profile")?;
        let distortion = if e.distortion {
            Some(
                profile
                    .distortion
                    .as_ref()
                    .context("No embedded distortion table")?,
            )
        } else {
            None
        };
        let vignette = if e.vignette {
            Some(
                profile
                    .vignette
                    .as_ref()
                    .context("No embedded vignetting table")?,
            )
        } else {
            None
        };
        for table in [distortion, vignette].into_iter().flatten() {
            table.validate()?;
        }
        if let Some(t) = distortion {
            ensure!(
                t.knots.iter().all(|k| k[1] > -100.),
                "Embedded distortion must retain positive radial scale"
            );
        }
        if let Some(t) = vignette {
            ensure!(
                t.knots.iter().all(|k| k[1] > 0.),
                "Embedded lens transmission must be positive"
            );
        }
        let diagonal = (width as f64).hypot(height as f64) * 0.5;
        let mut out = Self {
            distortion: distortion.is_some(),
            vignette: vignette.is_some(),
            lut: Vec::with_capacity(LUT_SAMPLES),
            frame_scale: 1.,
        };
        for i in 0..LUT_SAMPLES {
            let radius = ((i as f32 / (LUT_SAMPLES - 1) as f32) * MAX_RADIUS2).sqrt();
            let geometric = distortion.map_or(1., |t| {
                1. + t.interpolate(radius * (diagonal / t.radius_pixels as f64) as f32, 0.) * 0.01
            });
            let gain = vignette.map_or(1., |t| {
                100. / t.interpolate(radius * (diagonal / t.radius_pixels as f64) as f32, 100.)
            });
            ensure!(
                geometric.is_finite() && gain.is_finite(),
                "Lens correction exceeds numeric range"
            );
            out.lut.push([geometric, gain]);
        }
        if e.auto_frame && out.distortion {
            // Every rectangle-boundary radius lies between the short-side midpoint
            // and the corner. Maxima of our piecewise-linear LUT occur at its nodes.
            let edge = width.min(height) as f64 / (2. * diagonal);
            let edge2 = (edge * edge) as f32;
            let mut maximum = out.lookup(edge2, 0).max(out.lookup(1., 0));
            for (i, node) in out.lut.iter().enumerate() {
                let r2 = i as f32 / (LUT_SAMPLES - 1) as f32 * MAX_RADIUS2;
                if r2 >= edge2 && r2 <= 1. {
                    maximum = maximum.max(node[0]);
                }
            }
            out.frame_scale = 1. / (maximum * (1. + 1e-6));
            for node in &mut out.lut {
                node[0] *= out.frame_scale;
            }
        }
        Ok(out)
    }

    pub fn lookup(&self, radius2: f32, component: usize) -> f32 {
        if self.lut.is_empty() {
            return 1.;
        }
        let u = (radius2 / MAX_RADIUS2).clamp(0., 1.) * (LUT_SAMPLES - 1) as f32;
        let i = (u.floor() as usize).min(LUT_SAMPLES - 2);
        let t = u - i as f32;
        self.lut[i][component] + t * (self.lut[i + 1][component] - self.lut[i][component])
    }
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
        radii.first().is_some_and(|r| *r >= 0.) && radii.windows(2).all(|r| r[1] > r[0]),
        "Embedded lens radii must be nonnegative and strictly increasing"
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
    profile.validate()?;
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
