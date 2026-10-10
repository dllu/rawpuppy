//! Fixed order; each output sample executes the complete composed recipe.
use crate::{
    color::{self, Matrix},
    edits::{Edits, RetouchMode, ToneMapper},
    geometry::Geometry,
    input::{SensorImage, pixel_count},
};
use anyhow::{Result, ensure};
use rayon::prelude::*;

pub const MODULE_ORDER: &[&str] = &[
    "sensor normalization",
    "hot pixel correction",
    "sensor denoising",
    "demosaicing",
    "chromatic aberration / lens distortion / pinhole perspective / rotation / crop",
    "vignetting / graduated density / exposure",
    "color calibration",
    "AgX",
    "clone and heal",
    "tone curve",
    "split toning",
    "neural synthesis",
    "output color transform",
];

pub struct Pipeline<'a> {
    pub source: &'a SensorImage,
    pub edits: &'a Edits,
    pub geometry: Geometry,
    pub calibration: Matrix,
    pub curve: Vec<f32>,
    pub gradient: [f32; 2],
    pub retouch_index: Vec<Vec<usize>>,
    pub heal_offsets: Vec<[f32; 3]>,
}

#[derive(Debug)]
pub struct Rendered {
    pub width: usize,
    pub height: usize,
    /// Display-linear sRGB; transparent geometric gaps retain alpha=0.
    pub pixels: Vec<[f32; 4]>,
}

impl<'a> Pipeline<'a> {
    pub fn compile(source: &'a SensorImage, edits: &'a Edits) -> Result<Self> {
        edits.validate()?;
        ensure!(
            source.reconstruction == edits.raw.reconstruction,
            "Joint reconstruction needs its prepared camera RGB source"
        );
        let mut geometry = Geometry::compile(
            &edits.geometry,
            source.metadata.width,
            source.metadata.height,
        )?;
        geometry.lens = crate::lens::Correction::compile(
            source.metadata.lens_profile.as_ref(),
            &edits.lens,
            source.metadata.width,
            source.metadata.height,
        )?;
        let gain: Matrix = std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                if i == j {
                    source.metadata.as_shot[i] * edits.scene.calibration[i]
                } else {
                    0.
                }
            })
        });
        let calibration = color::multiply(
            edits.scene.mixer,
            color::multiply(source.metadata.camera_to_working, gain),
        );
        let (s, c) = edits.scene.graduated.angle.to_radians().sin_cos();
        let mut retouch_index = vec![Vec::new(); 64 * 64];
        for (i, brush) in edits.display.retouch.iter().enumerate() {
            let rx = brush.radius / geometry.crop[2];
            let ry = brush.radius / (geometry.crop[3] * geometry.aspect);
            let cell = |x: f32| (x * 64.).floor().clamp(0., 63.) as usize;
            for y in cell(brush.target[1] - ry)..=cell(brush.target[1] + ry) {
                for x in cell(brush.target[0] - rx)..=cell(brush.target[0] + rx) {
                    retouch_index[y * 64 + x].push(i);
                }
            }
        }
        let mut pipeline = Self {
            source,
            edits,
            geometry,
            calibration,
            curve: color::curve_lut(&edits.display.curve, 4096),
            gradient: [s, c],
            retouch_index,
            heal_offsets: Vec::new(),
        };
        pipeline.heal_offsets = edits
            .display
            .retouch
            .iter()
            .map(|brush| {
                if brush.mode != RetouchMode::Heal {
                    return [0.; 3];
                }
                let target = pipeline.base(brush.target);
                let source = pipeline.base(brush.source);
                std::array::from_fn(|i| target[i] - source[i])
            })
            .collect();
        Ok(pipeline)
    }

    fn base(&self, uv: [f32; 2]) -> [f32; 4] {
        let Some(p) = self.geometry.map(uv, 1) else {
            return [0.; 4];
        };
        let Some(mut rgb) = self.source.sample(p, &self.edits.raw) else {
            return [0.; 4];
        };
        let recovering = self.edits.raw.recover_highlights && self.source.raw_integer;
        let mut positions = [p; 3];
        if self.geometry.ca != [0.; 2] || self.geometry.lens.chromatic_aberration {
            for channel in [0, 2] {
                let Some(p) = self.geometry.map(uv, channel) else {
                    return [0.; 4];
                };
                let Some(sample) = self.source.sample(p, &self.edits.raw) else {
                    return [0.; 4];
                };
                rgb[channel] = sample[channel];
                positions[channel] = p;
            }
        }
        if recovering && rgb.iter().any(|v| *v >= crate::highlights::MASK_THRESHOLD) {
            let clipping = std::array::from_fn(|c| {
                self.source.clipping_weights(positions[c], &self.edits.raw)[c]
            });
            rgb = crate::highlights::recover(rgb, clipping, self.source.metadata.as_shot);
        }
        let x = p[0] - 0.5;
        let y = (p[1] - 0.5) * self.geometry.aspect;
        let r2 = 4. * (x * x + y * y) / (1. + self.geometry.aspect * self.geometry.aspect);
        let scene = &self.edits.scene;
        let grad = &scene.graduated;
        let dist = (uv[0] - grad.center[0]) * self.gradient[0]
            + (uv[1] - grad.center[1]) * self.gradient[1];
        let transition = (0.5 + dist / grad.width).clamp(0., 1.);
        let transition = transition * transition * (3. - 2. * transition);
        let camera_gain = if self.geometry.lens.vignette {
            self.geometry.lens.lookup(r2, 1)
        } else {
            1.
        };
        let gain = (scene.exposure
            + scene.vignette[0] * r2
            + scene.vignette[1] * r2 * r2
            + grad.exposure * transition)
            .exp2()
            * camera_gain;
        rgb = color::apply(self.calibration, rgb).map(|x| x * gain);
        rgb = match self.edits.tone.mapper {
            ToneMapper::Agx => color::agx_legacy(rgb),
            ToneMapper::AgxSdr => color::agx(rgb),
            ToneMapper::Linear => rgb,
        };
        let luma = color::luminance(rgb);
        let rgb = rgb.map(|x| luma + (x - luma) * self.edits.tone.saturation);
        [rgb[0], rgb[1], rgb[2], 1.]
    }

    pub fn sample(&self, uv: [f32; 2]) -> [f32; 4] {
        let mut out = self.base(uv);
        let cell = |x: f32| (x * 64.).floor().clamp(0., 63.) as usize;
        for &index in &self.retouch_index[cell(uv[1]) * 64 + cell(uv[0])] {
            let brush = &self.edits.display.retouch[index];
            let dx = (uv[0] - brush.target[0]) * self.geometry.crop[2];
            let dy = (uv[1] - brush.target[1]) * self.geometry.crop[3] * self.geometry.aspect;
            let d = (dx * dx + dy * dy).sqrt() / brush.radius;
            if d >= 1. {
                continue;
            }
            let weight = if brush.feather <= 0. {
                1.
            } else {
                let t = ((1. - d) / brush.feather).clamp(0., 1.);
                t * t * (3. - 2. * t)
            } * brush.opacity;
            let source_uv = [
                uv[0] + brush.source[0] - brush.target[0],
                uv[1] + brush.source[1] - brush.target[1],
            ];
            let mut clone = self.base(source_uv);
            if clone[3] == 0. {
                continue;
            }
            if brush.mode == RetouchMode::Heal {
                for (i, value) in clone[..3].iter_mut().enumerate() {
                    *value += self.heal_offsets[index][i];
                }
            }
            for i in 0..4 {
                out[i] += weight * (clone[i] - out[i]);
            }
        }
        let e = &self.edits.display;
        let identity_curve = e.curve == [[0., 0.], [1., 1.]];
        if !identity_curve {
            for v in &mut out[..3] {
                *v = color::srgb_decode(color::lookup(&self.curve, color::srgb_encode(*v)));
            }
        }
        let l = color::luminance([out[0], out[1], out[2]]).clamp(0., 1.);
        for (i, v) in out[..3].iter_mut().enumerate() {
            *v *= 1. + e.split_strength * ((1. - l) * e.shadows[i] + l * e.highlights[i] - 1.);
        }
        out
    }

    pub fn dimensions(&self, max_edge: Option<usize>) -> (usize, usize) {
        let (w, h) = (self.geometry.width, self.geometry.height);
        let ratio = max_edge.map_or(1., |m| (m as f64 / w.max(h) as f64).min(1.));
        (
            ((w as f64 * ratio).round() as usize).max(1),
            ((h as f64 * ratio).round() as usize).max(1),
        )
    }

    pub fn render(&self, max_edge: Option<usize>) -> Result<Rendered> {
        let (width, height) = self.dimensions(max_edge);
        self.render_region([0., 0., 1., 1.], width, height)
    }

    /// Viewport rendering reads the original at every zoom level, including 1:1.
    pub fn render_region(&self, region: [f32; 4], width: usize, height: usize) -> Result<Rendered> {
        let count = pixel_count(width, height, 1)?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(count)?;
        pixels.resize(count, [0.; 4]);
        pixels
            .par_chunks_mut(width)
            .enumerate()
            .for_each(|(y, row)| {
                for (x, p) in row.iter_mut().enumerate() {
                    *p = self.sample([
                        region[0] + region[2] * (x as f32 + 0.5) / width as f32,
                        region[1] + region[3] * (y as f32 + 0.5) / height as f32,
                    ]);
                }
            });
        Ok(Rendered {
            width,
            height,
            pixels,
        })
    }
}
