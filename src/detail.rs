//! Bounded final-output detail, after saved fills and before output colour.
//! Spatial filters need neighbours; the composed RAW pipeline still runs once
//! per requested sample. Tile aprons cover the entire filter dependency radius.
use crate::{color, edits::DetailEdits, input::pixel_count, pipeline::Rendered};
use anyhow::{Result, ensure};
use rayon::prelude::*;

pub(crate) struct Plan {
    steps: Vec<usize>,
    sharp: f32,
    contrast: f32,
    pub apron: usize,
}
impl Plan {
    pub fn new(
        edits: &DetailEdits,
        region: [f32; 4],
        size: [usize; 2],
        source: [usize; 2],
    ) -> Result<Self> {
        ensure!(
            size.iter().all(|v| *v > 0)
                && source.iter().all(|v| *v > 0)
                && region.iter().all(|v| v.is_finite())
                && region[2] > 0.
                && region[3] > 0.,
            "Invalid detail viewport"
        );
        let whole = [size[0] as f32 / region[2], size[1] as f32 / region[3]];
        ensure!(
            whole.iter().all(|v| v.is_finite()),
            "Detail viewport is too small"
        );
        let density = (whole[0] / source[0] as f32)
            .min(whole[1] / source[1] as f32)
            .min(1.);
        let radius = (whole[0].min(whole[1]) * 0.002).max(1.);
        let steps = if edits.local_contrast > 0. {
            [1., 2., 4.].map(|s| (radius * s).round() as usize).to_vec()
        } else {
            vec![]
        };
        let apron = steps
            .iter()
            .try_fold(1usize, |sum, step| sum.checked_add(*step))
            .ok_or_else(|| anyhow::anyhow!("Detail apron overflow"))?;
        ensure!(apron < isize::MAX as usize, "Detail apron overflow");
        Ok(Self {
            apron,
            steps,
            sharp: edits.sharpen * density * density,
            contrast: edits.local_contrast,
        })
    }

    pub fn apply(&self, image: &mut Rendered) -> Result<()> {
        let w = image.width;
        let h = image.height;
        let count = pixel_count(w, h, 1)?;
        let mut original = Vec::new();
        original.try_reserve_exact(count)?;
        original.extend(image.pixels.iter().map(|p| {
            let y = color::luminance([p[0], p[1], p[2]]);
            if p[3] < 0.999 || y <= 1e-6 {
                f32::NAN
            } else {
                y.log2()
            }
        }));
        let mut blur = original.clone();
        let mut work = Vec::new();
        work.try_reserve_exact(count)?;
        work.resize(count, 0.);
        // A-trous separable bilateral diffusion. Original luminance guides every
        // scale, so a strong edge never becomes an invitation to blur across it.
        for &step in &self.steps {
            for vertical in [false, true] {
                work.par_iter_mut().enumerate().for_each(|(i, out)| {
                    let center = original[i];
                    if !center.is_finite() {
                        *out = center;
                        return;
                    }
                    let x = i % w;
                    let y = i / w;
                    let mut sum = 0.;
                    let mut weights = 0.;
                    for tap in -1isize..=1 {
                        let xx = if vertical {
                            x
                        } else {
                            x.saturating_add_signed(tap * step as isize).min(w - 1)
                        };
                        let yy = if vertical {
                            y.saturating_add_signed(tap * step as isize).min(h - 1)
                        } else {
                            y
                        };
                        let j = yy * w + xx;
                        if !original[j].is_finite() {
                            continue;
                        }
                        let delta = (original[j] - center) / 0.5;
                        let range = (1. - delta * delta).max(0.);
                        let weight = range * range * if tap == 0 { 2. } else { 1. };
                        sum += blur[j] * weight;
                        weights += weight;
                    }
                    *out = sum / weights;
                });
                std::mem::swap(&mut blur, &mut work);
            }
        }
        image.pixels.par_iter_mut().enumerate().for_each(|(i, p)| {
            let center = original[i];
            if !center.is_finite() {
                return;
            }
            let x = i % w;
            let y = i / w;
            let mut sum = 0.;
            let mut weights = 0.;
            let mut low = center;
            let mut high = center;
            for dy in -1isize..=1 {
                for dx in -1isize..=1 {
                    let v = original[y.saturating_add_signed(dy).min(h - 1) * w
                        + x.saturating_add_signed(dx).min(w - 1)];
                    if !v.is_finite() {
                        continue;
                    }
                    let weight = (if dx == 0 { 2. } else { 1. }) * (if dy == 0 { 2. } else { 1. });
                    sum += v * weight;
                    weights += weight;
                    low = low.min(v);
                    high = high.max(v);
                }
            }
            let difference = center - sum / weights;
            // Suppress tiny differences and high-contrast discontinuities. Clamp
            // to observed extrema to prevent bright/dark overshoot halos.
            let threshold = 0.015;
            let fine = difference.signum() * (difference.abs() - threshold).max(0.);
            let edge = (1. - (high - low) / 2.).clamp(0., 1.);
            let sharpened = (center + self.sharp * edge * fine).clamp(low, high);
            let local = if self.steps.is_empty() {
                0.
            } else {
                (center - blur[i]).clamp(-0.25, 0.25) * self.contrast
            };
            let target = (sharpened + local).clamp(low, high);
            let gain = (target - center).exp2();
            for value in &mut p[..3] {
                *value *= gain;
            }
        });
        Ok(())
    }
}

/// Render overlapping bounded tiles, discard their aprons, and copy only the
/// requested interior. Coordinates use the caller's original pixel lattice.
pub(crate) fn render(
    plan: &Plan,
    region: [f32; 4],
    width: usize,
    height: usize,
    mut sample: impl FnMut([f32; 4], usize, usize) -> Result<Rendered>,
) -> Result<Rendered> {
    ensure!(
        width > 0
            && height > 0
            && region.iter().all(|v| v.is_finite())
            && region[2] > 0.
            && region[3] > 0.,
        "Invalid detail viewport"
    );
    let count = pixel_count(width, height, 1)?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(count)?;
    pixels.resize(count, [0.; 4]);
    for y in (0..height).step_by(256) {
        for x in (0..width).step_by(512) {
            let tw = (width - x).min(512);
            let th = (height - y).min(256);
            let pad = plan.apron;
            let w = tw
                .checked_add(
                    pad.checked_mul(2)
                        .ok_or_else(|| anyhow::anyhow!("Detail apron overflow"))?,
                )
                .ok_or_else(|| anyhow::anyhow!("Detail width overflow"))?;
            let h = th
                .checked_add(pad * 2)
                .ok_or_else(|| anyhow::anyhow!("Detail height overflow"))?;
            let tile_region = [
                region[0] + (x as f64 - pad as f64) as f32 * region[2] / width as f32,
                region[1] + (y as f64 - pad as f64) as f32 * region[3] / height as f32,
                w as f32 * region[2] / width as f32,
                h as f32 * region[3] / height as f32,
            ];
            let mut tile = sample(tile_region, w, h)?;
            plan.apply(&mut tile)?;
            for row in 0..th {
                pixels[(y + row) * width + x..(y + row) * width + x + tw].copy_from_slice(
                    &tile.pixels[(row + pad) * w + pad..(row + pad) * w + pad + tw],
                );
            }
        }
    }
    Ok(Rendered {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(region: [f32; 4], w: usize, h: usize) -> Result<Rendered> {
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let u = region[0] + (x as f32 + 0.5) * region[2] / w as f32;
                let v = region[1] + (y as f32 + 0.5) * region[3] / h as f32;
                let l = 0.1
                    + 0.03 * (u * 74.).sin()
                    + 0.04 * (v * 92.).sin()
                    + if u > 0.47 { 0.2 } else { 0. };
                pixels.push([l, l * 0.5, l * 0.25, if u < 0.02 { 0. } else { 1. }]);
            }
        }
        Ok(Rendered {
            width: w,
            height: h,
            pixels,
        })
    }
    #[test]
    fn neighbouring_export_strips_and_viewports_have_no_detail_seams() {
        let detail = DetailEdits {
            sharpen: 0.2,
            local_contrast: 0.45,
        };
        let region = [0., 0., 1., 1.];
        let plan = Plan::new(&detail, region, [640, 320], [640, 320]).unwrap();
        let full = render(&plan, region, 640, 320, sample).unwrap();
        let sub = [0.2, 0.4, 0.6, 0.3];
        let sub_plan = Plan::new(&detail, sub, [384, 96], [640, 320]).unwrap();
        let strip = render(&sub_plan, sub, 384, 96, sample).unwrap();
        let error = (0..96)
            .flat_map(|y| (0..384).map(move |x| (y, x)))
            .flat_map(|(y, x)| (0..4).map(move |c| (y, x, c)))
            .map(|(y, x, c)| {
                (full.pixels[(y + 128) * 640 + x + 128][c] - strip.pixels[y * 384 + x][c]).abs()
            })
            .fold(0., f32::max);
        assert!(error < 2e-6, "{error}");
    }
    #[test]
    fn transparent_borders_and_strong_steps_do_not_create_halos_or_clip_hdr() {
        let mut image = Rendered {
            width: 65,
            height: 33,
            pixels: (0..65 * 33)
                .map(|i| {
                    let v = if i % 65 < 32 { 0.1 } else { 4. };
                    [v, v * 0.75, v * 0.5, if i % 65 < 3 { 0. } else { 1. }]
                })
                .collect(),
        };
        let before = image.pixels.clone();
        let plan = Plan::new(
            &DetailEdits {
                sharpen: 1.,
                local_contrast: 1.,
            },
            [0., 0., 1., 1.],
            [65, 33],
            [65, 33],
        )
        .unwrap();
        plan.apply(&mut image).unwrap();
        for (p, old) in image.pixels.iter().zip(before) {
            assert_eq!(p[3], old[3]);
            for c in 0..3 {
                assert!((p[c] - old[c]).abs() < 1e-6, "{p:?} vs {old:?}");
            }
        }
    }
    #[test]
    fn detail_boosts_texture_preserves_rgb_ratios_and_suppresses_tiny_noise() {
        let mut image = sample([0., 0., 1., 1.], 128, 64).unwrap();
        let before = image.pixels.clone();
        let plan = Plan::new(
            &DetailEdits {
                sharpen: 0.2,
                local_contrast: 0.5,
            },
            [0., 0., 1., 1.],
            [128, 64],
            [128, 64],
        )
        .unwrap();
        plan.apply(&mut image).unwrap();
        assert!(
            image
                .pixels
                .iter()
                .zip(&before)
                .any(|(a, b)| (a[0] - b[0]).abs() > 0.001)
        );
        for p in &image.pixels {
            assert!((p[0] - p[1] * 2.).abs() < 1e-6 && (p[0] - p[2] * 4.).abs() < 1e-6);
        }
        let mut flat = Rendered {
            width: 31,
            height: 31,
            pixels: (0..31 * 31)
                .map(|i| [0.2 + if i % 2 == 0 { 0.0001 } else { -0.0001 }; 4])
                .collect(),
        };
        for p in &mut flat.pixels {
            p[3] = 1.;
        }
        let original = flat.pixels.clone();
        Plan::new(
            &DetailEdits {
                sharpen: 0.2,
                local_contrast: 0.,
            },
            [0., 0., 1., 1.],
            [31, 31],
            [31, 31],
        )
        .unwrap()
        .apply(&mut flat)
        .unwrap();
        assert_eq!(flat.pixels, original);
    }
}
