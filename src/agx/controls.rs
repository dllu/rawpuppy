use super::{TO_REC2020, TO_SRGB};
use crate::color::{self, IDENTITY, Matrix, apply, inverse, multiply};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const BLACK_EV: std::ops::RangeInclusive<f32> = -24. ..=-0.125;
pub const WHITE_EV: std::ops::RangeInclusive<f32> = 0.125..=24.;
pub const PIVOT_MARGIN: f32 = 0.0625;
pub const MID_GRAY: f32 = 0.18;
pub(crate) const CURVE_SAMPLES: usize = 4097;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct AgxEdits {
    pub black_ev: f32,
    pub white_ev: f32,
    pub range_margin: f32,
    pub pivot_ev: f32,
    pub pivot_output: f32,
    pub contrast: f32,
    pub toe_power: f32,
    pub shoulder_power: f32,
    pub toe_start: f32,
    pub shoulder_start: f32,
    pub target_black: f32,
    pub target_white: f32,
    pub auto_gamma: bool,
    pub gamma: f32,
    pub slope: f32,
    pub lift: f32,
    pub brightness: f32,
    pub preserve_hue: f32,
    pub primaries: Primaries,
}

impl Default for AgxEdits {
    fn default() -> Self {
        Self {
            black_ev: -10.,
            white_ev: 6.5,
            range_margin: 0.1,
            pivot_ev: 0.,
            pivot_output: MID_GRAY,
            contrast: 2.4,
            toe_power: 1.5,
            shoulder_power: 3.3,
            toe_start: 0.,
            shoulder_start: 0.,
            target_black: 0.,
            target_white: 1.,
            auto_gamma: false,
            gamma: 2.2,
            slope: 1.,
            lift: 0.,
            brightness: 1.,
            preserve_hue: 0.6,
            primaries: Primaries::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BasePrimaries {
    #[default]
    Rec2020,
    DisplayP3,
    Srgb,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Primaries {
    pub disabled: bool,
    pub base: BasePrimaries,
    pub attenuation: [f32; 3],
    pub rotation: [f32; 3],
    pub reverse_all: bool,
    pub purity_master: f32,
    pub rotation_master: f32,
    pub purity: [f32; 3],
    pub reversal: [f32; 3],
}

impl Default for Primaries {
    fn default() -> Self {
        Self {
            disabled: false,
            base: BasePrimaries::Rec2020,
            attenuation: [0.; 3],
            rotation: [0.; 3],
            reverse_all: false,
            purity_master: 1.,
            rotation_master: 1.,
            purity: [0.; 3],
            reversal: [0.; 3],
        }
    }
}

impl AgxEdits {
    pub(crate) fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub(crate) fn uses_lattice(&self) -> bool {
        Self {
            range_margin: Self::default().range_margin,
            ..*self
        }
        .is_default()
    }

    pub fn clamp_pivot(&mut self) {
        self.pivot_ev = self
            .pivot_ev
            .clamp(self.black_ev + PIVOT_MARGIN, self.white_ev - PIVOT_MARGIN);
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            BLACK_EV.contains(&self.black_ev)
                && WHITE_EV.contains(&self.white_ev)
                && (self.black_ev + PIVOT_MARGIN..=self.white_ev - PIVOT_MARGIN)
                    .contains(&self.pivot_ev)
                && (0. ..=1.).contains(&self.range_margin),
            "Invalid AgX input range or pivot"
        );
        ensure!(
            (0. ..=0.98).contains(&self.target_black)
                && (0.02..=1.).contains(&self.target_white)
                && (self.target_black + 0.001..=self.target_white - 0.001)
                    .contains(&self.pivot_output)
                && (0.1..=8.).contains(&self.contrast)
                && [self.toe_power, self.shoulder_power]
                    .iter()
                    .all(|p| (0.5..=8.).contains(p))
                && [self.toe_start, self.shoulder_start]
                    .iter()
                    .all(|p| (0. ..=0.95).contains(p))
                && (0.5..=8.).contains(&self.gamma),
            "Invalid AgX curve settings"
        );
        ensure!(
            (0. ..=2.).contains(&self.slope)
                && (-0.5..=0.5).contains(&self.lift)
                && (0.25..=4.).contains(&self.brightness)
                && (0. ..=1.).contains(&self.preserve_hue),
            "Invalid AgX look settings"
        );
        let p = self.primaries;
        ensure!(
            (0. ..=1.).contains(&p.purity_master)
                && (0. ..=1.).contains(&p.rotation_master)
                && p.attenuation
                    .iter()
                    .chain(&p.purity)
                    .all(|a| (0. ..=0.75).contains(a))
                && p.rotation
                    .iter()
                    .chain(&p.reversal)
                    .all(|a| (-30. ..=30.).contains(a)),
            "Invalid AgX primary adjustments"
        );
        Ok(())
    }
}

pub(crate) struct Formation {
    pub(crate) input: Matrix,
    pub(crate) output: Matrix,
    pub(crate) curve: Vec<f32>,
    pub(crate) black_ev: f32,
    pub(crate) inv_range: f32,
    pub(crate) preserve_hue: f32,
}

impl Formation {
    pub(crate) fn compile(edits: &AgxEdits) -> Result<Self> {
        let (input, output) = edits.primaries.matrices()?;
        Ok(Self {
            input,
            output: multiply(TO_SRGB, output),
            curve: Curve::new(edits).samples(),
            black_ev: edits.black_ev,
            inv_range: 1. / (edits.white_ev - edits.black_ev),
            preserve_hue: edits.preserve_hue,
        })
    }

    pub(crate) fn map(&self, rgb: [f32; 3]) -> [f32; 3] {
        let scene = apply(self.input, super::rec2020_in_gamut(rgb));
        let mut formed = scene.map(|x| {
            let coord = (relative_ev(x.max(f32::MIN_POSITIVE)) - self.black_ev) * self.inv_range;
            color::lookup(&self.curve, coord)
        });
        if self.preserve_hue > 0. {
            formed = restore_hue(scene, formed, self.preserve_hue);
        }
        apply(self.output, formed).map(super::roundoff)
    }
}

pub(crate) struct Curve {
    edits: AgxEdits,
    gamma: f64,
    pivot: f64,
    toe: RollOff,
    shoulder: RollOff,
}

impl Curve {
    pub(crate) fn new(edits: &AgxEdits) -> Self {
        let encode = |v: f32, gamma: f64| f64::from(v).powf(1. / gamma);
        let gamma = if edits.auto_gamma {
            let x =
                f64::from((edits.pivot_ev - edits.black_ev) / (edits.white_ev - edits.black_ev));
            let (mut low, mut high) = (0.5, 8.);
            for _ in 0..20 {
                let middle = (low + high) * 0.5;
                let black = encode(edits.target_black, middle);
                let y = (encode(edits.pivot_output, middle) - black)
                    / (encode(edits.target_white, middle) - black);
                if y < x {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            (low + high) * 0.5
        } else {
            f64::from(edits.gamma)
        };
        let pivot = encode(edits.pivot_output, gamma);
        // Keep the slope in linear output per stop independent of curve gamma.
        let linear_slope = f64::from(edits.contrast) * 0.18_f64.powf(1. - 1. / 2.2) / 6.;
        let slope = linear_slope / (gamma * pivot.powf(gamma - 1.));
        Self {
            edits: *edits,
            gamma,
            pivot,
            toe: RollOff::new(
                f64::from(edits.pivot_ev - edits.black_ev),
                pivot - encode(edits.target_black, gamma),
                slope,
                edits.toe_power,
                edits.toe_start,
            ),
            shoulder: RollOff::new(
                f64::from(edits.white_ev - edits.pivot_ev),
                encode(edits.target_white, gamma) - pivot,
                slope,
                edits.shoulder_power,
                edits.shoulder_start,
            ),
        }
    }

    pub(crate) fn at(&self, ev: f32) -> f32 {
        let e = &self.edits;
        let distance = f64::from(ev - e.pivot_ev);
        let y = if distance < 0. {
            self.pivot - self.toe.at(-distance)
        } else {
            self.pivot + self.shoulder.at(distance)
        };
        (y * f64::from(e.slope) + (1. - y) * f64::from(e.lift))
            .clamp(0., 1.)
            .powf(self.gamma / f64::from(e.brightness)) as f32
    }

    fn samples(&self) -> Vec<f32> {
        let e = &self.edits;
        (0..CURVE_SAMPLES)
            .map(|i| {
                self.at(
                    e.black_ev + (e.white_ev - e.black_ev) * i as f32 / (CURVE_SAMPLES - 1) as f32
                )
            })
            .collect()
    }
}

struct RollOff {
    length: f64,
    height: f64,
    slope: f64,
    start: f64,
    power: f64,
    ratio_power: f64,
}

impl RollOff {
    fn new(length: f64, height: f64, slope: f64, power: f32, start: f32) -> Self {
        let power = f64::from(power);
        let start = f64::from(start) * length.min(height / slope);
        let ratio = slope * (length - start) / (height - slope * start);
        Self {
            length,
            height,
            slope,
            start,
            power,
            ratio_power: ratio.powf(power),
        }
    }

    // Normalize a rational shoulder to the endpoint. Its derivative at the
    // join is the slope of the central linear segment.
    fn at(&self, distance: f64) -> f64 {
        if distance >= self.length {
            return self.height;
        }
        if distance <= self.start {
            return self.slope * distance;
        }
        let t = ((distance - self.start) / (self.length - self.start)).powf(self.power);
        let bend = (1. - t + self.ratio_power * t).powf(1. / self.power);
        self.slope * (self.start + (distance - self.start) / bend)
    }
}

// Fitted from the committed Rec.2020 oracle lattice using its neutral sweep
// and independent chromatic probes. Unit row sums retain the neutral axis.
const PRESET_INPUT: Matrix = [
    [0.8566338, 0.09511607, 0.04825016],
    [0.13731965, 0.76124024, 0.10144012],
    [0.11189833, 0.07680009, 0.8113016],
];
const PRESET_OUTPUT: Matrix = [
    [1.1281847, -0.1115303, -0.01665432],
    [-0.14007011, 1.1567241, -0.01665402],
    [-0.14312097, -0.11153032, 1.2546513],
];

impl Primaries {
    pub(crate) fn matrices(&self) -> Result<(Matrix, Matrix)> {
        if self.disabled {
            return Ok((IDENTITY, IDENTITY));
        }
        let base_to_xyz = match self.base {
            BasePrimaries::Rec2020 => color::REC2020_TO_XYZ,
            BasePrimaries::DisplayP3 => color::P3_TO_XYZ,
            BasePrimaries::Srgb => color::SRGB_TO_XYZ,
        };
        let to_base = match self.base {
            BasePrimaries::Rec2020 => IDENTITY,
            BasePrimaries::DisplayP3 => multiply(inverse(base_to_xyz)?, color::REC2020_TO_XYZ),
            BasePrimaries::Srgb => TO_SRGB,
        };
        let from_base = match self.base {
            BasePrimaries::Rec2020 => IDENTITY,
            BasePrimaries::DisplayP3 => inverse(to_base)?,
            BasePrimaries::Srgb => TO_REC2020,
        };
        let in_base = primary_adjustment(
            base_to_xyz[1],
            self.attenuation.map(|v| 1. - v),
            self.rotation,
        );
        let out_base = inverse(if self.reverse_all {
            in_base
        } else {
            primary_adjustment(
                base_to_xyz[1],
                self.purity.map(|v| 1. - v * self.purity_master),
                self.reversal.map(|v| v * self.rotation_master),
            )
        })?;
        Ok((
            multiply(multiply(PRESET_INPUT, in_base), to_base),
            multiply(from_base, multiply(out_base, PRESET_OUTPUT)),
        ))
    }
}

// Rotate each primary around equal RGB, scale its chroma, then distribute
// the offsets so the matrix preserves both luminance and D65 neutrals.
fn primary_adjustment(luma: [f32; 3], gain: [f32; 3], angle: [f32; 3]) -> Matrix {
    let columns = std::array::from_fn::<_, 3, _>(|i| {
        let (s, c) = angle[i].to_radians().sin_cos();
        let tangent: [f32; 3] = std::array::from_fn(|j| {
            (IDENTITY[(i + 1) % 3][j] - IDENTITY[(i + 2) % 3][j]) / 3_f32.sqrt()
        });
        let rotated: [f32; 3] =
            std::array::from_fn(|j| gain[i] * (c * (IDENTITY[i][j] - 1. / 3.) + s * tangent[j]));
        let y: f32 = rotated.into_iter().zip(luma).map(|(v, w)| v * w).sum();
        std::array::from_fn::<_, 3, _>(|j| rotated[j] - y + luma[i] - IDENTITY[i][j])
    });
    std::array::from_fn(|r| {
        let total: f32 = columns.iter().map(|col| col[r]).sum();
        std::array::from_fn(|c| IDENTITY[r][c] + columns[c][r] - total * luma[c])
    })
}

fn hue([r, g, b]: [f32; 3], top: f32, chroma: f32) -> f32 {
    if top == r {
        (g - b) / chroma
    } else if top == g {
        2. + (b - r) / chroma
    } else {
        4. + (r - g) / chroma
    }
}

fn restore_hue(scene: [f32; 3], formed: [f32; 3], amount: f32) -> [f32; 3] {
    let top = |[r, g, b]: [f32; 3]| r.max(g).max(b);
    let bottom = |[r, g, b]: [f32; 3]| r.min(g).min(b);
    let max = top(formed);
    let min = bottom(formed);
    let chroma = max - min;
    let source_max = top(scene);
    let source_chroma = source_max - bottom(scene);
    if chroma <= 1e-7 || source_chroma <= 1e-7 {
        return formed;
    }
    let h = hue(formed, max, chroma);
    let delta = hue(scene, source_max, source_chroma) - h;
    let shortest = delta - ((delta + 3.) / 6.).floor() * 6.;
    let h = h + amount * shortest;
    [0., 4., 2.].map(|offset| {
        let t: f32 = h + offset;
        let sector = t - (t / 6.).floor() * 6.;
        min + chroma * ((sector - 3.).abs() - 1.).clamp(0., 1.)
    })
}

pub(crate) fn relative_ev(value: f32) -> f32 {
    value.log2() - MID_GRAY.log2()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjustable_default_tracks_independent_agx_oracle() {
        let formation = Formation::compile(&AgxEdits::default()).unwrap();
        let samples: Vec<([f32; 3], [f32; 3])> =
            serde_json::from_str(include_str!("../../tests/data/agx-oracle.json")).unwrap();
        let mut sum = 0.;
        for (input, expected) in &samples {
            for (actual, expected) in formation.map(*input).into_iter().zip(*expected) {
                let error = (actual - expected).abs();
                assert!(error < 0.01, "{input:?}: {actual} vs {expected}");
                sum += error;
            }
        }
        assert!(sum / ((samples.len() * 3) as f32) < 0.0002);
    }

    #[test]
    fn adjustable_curve_is_monotone_and_reaches_its_endpoints_and_pivot() {
        for (contrast, auto_gamma) in [(0.1, false), (1., true), (8., false), (8., true)] {
            let edits = AgxEdits {
                black_ev: -4.,
                white_ev: 3.,
                pivot_ev: 1.,
                pivot_output: 0.25,
                target_black: 0.02,
                target_white: 0.85,
                toe_start: 0.25,
                shoulder_start: 0.8,
                toe_power: 0.5,
                shoulder_power: 8.,
                gamma: 4.,
                auto_gamma,
                contrast,
                ..Default::default()
            };
            let curve = Curve::new(&edits);
            let samples = curve.samples();
            assert!(samples.windows(2).all(|p| p[0] <= p[1]));
            for (ev, value) in [
                (edits.black_ev - 2., edits.target_black),
                (edits.pivot_ev, edits.pivot_output),
                (edits.white_ev + 2., edits.target_white),
            ] {
                assert!((curve.at(ev) - value).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn primary_reversal_matches_copied_settings_at_slider_limits() {
        for base in [
            BasePrimaries::Rec2020,
            BasePrimaries::Srgb,
            BasePrimaries::DisplayP3,
        ] {
            for bits in 0..64 {
                let attenuation =
                    std::array::from_fn(|c| if bits & (1 << c) == 0 { 0. } else { 0.75 });
                let rotation =
                    std::array::from_fn(|c| if bits & (8 << c) == 0 { -30. } else { 30. });
                let mut primaries = Primaries {
                    base,
                    attenuation,
                    rotation,
                    reverse_all: true,
                    ..Default::default()
                };
                let (input, output) = primaries.matrices().unwrap();
                for v in [apply(input, [0.18; 3]), apply(output, [0.18; 3])]
                    .into_iter()
                    .flatten()
                {
                    assert!((v - 0.18).abs() < 2e-6);
                }
                primaries.reverse_all = false;
                primaries.purity = attenuation;
                primaries.reversal = rotation;
                assert_eq!((input, output), primaries.matrices().unwrap());
            }
        }
    }
}
