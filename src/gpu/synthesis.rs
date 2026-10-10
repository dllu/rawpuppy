//! Persistent generated pixels and exact integer paint spans for fused GPU output.
use crate::{pipeline::Rendered, synthesis::ResolvedLayer};
use anyhow::{Result, ensure};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct Atlas {
    images: Vec<Arc<Rendered>>,
    packed: Vec<[f32; 4]>,
    pub offsets: Vec<u32>,
}
impl Atlas {
    pub fn update(&mut self, layers: &[ResolvedLayer<'_>], max_bytes: usize) -> Result<bool> {
        let mut images: Vec<Arc<Rendered>> = Vec::new();
        images.try_reserve(layers.len())?;
        let mut offsets = Vec::new();
        offsets.try_reserve(layers.len())?;
        let mut starts = Vec::new();
        starts.try_reserve(layers.len())?;
        let mut count = 0usize;
        for layer in layers {
            let slot = if let Some(slot) = images
                .iter()
                .position(|image| Arc::ptr_eq(image, &layer.image))
            {
                slot
            } else {
                let slot = images.len();
                starts.push(u32::try_from(
                    count
                        .checked_mul(4)
                        .ok_or_else(|| anyhow::anyhow!("Layer addressing overflow"))?,
                )?);
                count = count
                    .checked_add(layer.image.pixels.len())
                    .ok_or_else(|| anyhow::anyhow!("Layer size overflow"))?;
                images.push(layer.image.clone());
                slot
            };
            offsets.push(starts[slot]);
        }
        ensure!(
            count <= u32::MAX as usize / 4,
            "Generated pixels exceed GPU addressing; use CPU"
        );
        ensure!(
            count
                .checked_mul(16)
                .is_some_and(|bytes| bytes <= max_bytes),
            "Generated pixels exceed GPU storage binding; use CPU"
        );
        let changed = self.images.len() != images.len()
            || self
                .images
                .iter()
                .zip(&images)
                .any(|(a, b)| !Arc::ptr_eq(a, b));
        if changed {
            let mut packed = Vec::new();
            if images.len() > 1 {
                packed.try_reserve_exact(count)?;
                for image in &images {
                    packed.extend_from_slice(&image.pixels);
                }
            }
            self.images = images;
            self.packed = packed;
        }
        self.offsets = offsets;
        Ok(changed)
    }
    pub fn data(&self) -> &[f32] {
        match self.images.len() {
            0 => &[0.],
            1 => bytemuck::cast_slice(&self.images[0].pixels),
            _ => bytemuck::cast_slice(&self.packed),
        }
    }
}

fn coordinate(origin: f32, extent: f32, i: usize, size: usize) -> f32 {
    // Retain the historical CPU composition expression, including its rounding.
    origin + extent * (i as f32 + 0.5) / size as f32
}
fn closest(size: usize, mut coordinate: impl FnMut(usize) -> f32, center: f32) -> usize {
    let mut lo = 0;
    let mut hi = size;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if coordinate(mid) < center {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let right = lo.min(size - 1);
    if right > 0 && (coordinate(right - 1) - center).abs() < (coordinate(right) - center).abs() {
        right - 1
    } else {
        right
    }
}
fn span(size: usize, center: usize, inside: impl Fn(usize) -> bool) -> [usize; 2] {
    if !inside(center) {
        return [0, 0];
    }
    let mut lo = 0;
    let mut hi = center;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if inside(mid) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    let start = lo;
    lo = center;
    hi = size;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if inside(mid) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    [start, lo]
}
fn weight(distance2: f32, radius: f32, feather: f32) -> f32 {
    if distance2 > radius * radius {
        return 0.;
    }
    if feather == 0. {
        return 1.;
    }
    let t = ((1. - distance2.sqrt() / radius) / feather).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// Twelve u32s per layer, three per dab, and four per selected row. No photo mask.
pub(super) fn arguments(
    layers: &[ResolvedLayer<'_>],
    offsets: &[u32],
    viewport: [f32; 4],
    width: usize,
    height: usize,
    index: &mut Vec<u32>,
    max_bytes: usize,
) -> Result<Vec<f32>> {
    let aspect = (height as f32 / viewport[3]) / (width as f32 / viewport[2]);
    ensure!(
        layers.is_empty() || aspect.is_finite() && aspect > 0.,
        "GPU layer aspect is not representable; use CPU"
    );
    let base = index.len();
    let descriptors = layers
        .len()
        .checked_mul(12)
        .ok_or_else(|| anyhow::anyhow!("Layer metadata overflow"))?;
    index.try_reserve(descriptors)?;
    index.resize(base + descriptors, 0);
    let mut info = Vec::new();
    for (i, layer) in layers.iter().enumerate() {
        let fill = layer.fill;
        let info_start = u32::try_from(info.len())?;
        info.try_reserve(6 + fill.dabs.len() * 3)?;
        info.extend(fill.region);
        info.extend([fill.feather, aspect]);
        for dab in &fill.dabs {
            info.extend(dab.center);
            info.push(dab.radius);
        }
        let dab_start = u32::try_from(index.len())?;
        let dab_descriptors = fill
            .dabs
            .len()
            .checked_mul(3)
            .ok_or_else(|| anyhow::anyhow!("Dab metadata overflow"))?;
        index.try_reserve(dab_descriptors)?;
        index.resize(index.len() + dab_descriptors, 0);
        for (j, dab) in fill.dabs.iter().enumerate() {
            let x_at = |x| coordinate(viewport[0], viewport[2], x, width);
            let y_at = |y| coordinate(viewport[1], viewport[3], y, height);
            let cx = closest(width, x_at, dab.center[0]);
            let cy = closest(height, y_at, dab.center[1]);
            let dx = x_at(cx) - dab.center[0];
            let dx2 = dx * dx;
            let distance2 = |x, y| {
                let dx = x_at(x) - dab.center[0];
                let dy = (y_at(y) - dab.center[1]) * aspect;
                dx * dx + dy * dy
            };
            let [y0, y1] = span(height, cy, |y| {
                let dy = (y_at(y) - dab.center[1]) * aspect;
                dx2 + dy * dy <= dab.radius * dab.radius
            });
            let rows = (y1 - y0)
                .checked_mul(4)
                .ok_or_else(|| anyhow::anyhow!("Layer row metadata overflow"))?;
            ensure!(
                index
                    .len()
                    .checked_add(rows)
                    .and_then(|n| n.checked_mul(4))
                    .is_some_and(|n| n <= max_bytes),
                "Layer row metadata exceeds GPU storage binding; use CPU"
            );
            let table = u32::try_from(index.len())?;
            index.try_reserve(rows)?;
            for y in y0..y1 {
                let outer = span(width, cx, |x| distance2(x, y) <= dab.radius * dab.radius);
                let core = span(width, cx, |x| {
                    weight(distance2(x, y), dab.radius, fill.feather) == 1.
                });
                for value in outer.into_iter().chain(core) {
                    index.push(u32::try_from(value)?);
                }
            }
            let descriptor = dab_start as usize + j * 3;
            index[descriptor..descriptor + 3].copy_from_slice(&[
                u32::try_from(y0)?,
                u32::try_from(y1 - y0)?,
                table,
            ]);
        }
        let x_at = |x| coordinate(viewport[0], viewport[2], x, width);
        let y_at = |y| coordinate(viewport[1], viewport[3], y, height);
        let x_center = closest(width, x_at, fill.region[0] + fill.region[2] * 0.5);
        let y_center = closest(height, y_at, fill.region[1] + fill.region[3] * 0.5);
        let [x0, x1] = span(width, x_center, |x| {
            (0.0..=1.0).contains(&((x_at(x) - fill.region[0]) / fill.region[2]))
        });
        let [y0, y1] = span(height, y_center, |y| {
            (0.0..=1.0).contains(&((y_at(y) - fill.region[1]) / fill.region[3]))
        });
        index[base + i * 12..base + (i + 1) * 12].copy_from_slice(&[
            offsets[i],
            u32::try_from(layer.image.width)?,
            u32::try_from(layer.image.height)?,
            info_start,
            u32::from(fill.fill_gaps),
            u32::try_from(fill.dabs.len())?,
            dab_start,
            u32::try_from(x0)?,
            u32::try_from(x1)?,
            u32::try_from(y0)?,
            u32::try_from(y1)?,
            0,
        ]);
    }
    if info.is_empty() {
        info.push(0.);
    }
    ensure!(
        info.len() * 4 <= max_bytes && index.len() * 4 <= max_bytes,
        "Layer arguments exceed GPU storage binding; use CPU"
    );
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthesis::MaskDab;
    #[test]
    fn atlas_borrows_single_assets_and_reuses_deduplicated_packing() {
        let edits: crate::edits::Edits = serde_json::from_str(include_str!(
            "../../tests/data/legacy-synthesis-recipe.json"
        ))
        .unwrap();
        let fill = &edits.display.synthesis[0];
        let a = Arc::new(Rendered {
            width: 2,
            height: 1,
            pixels: vec![[1., 2., 3., 1.]; 2],
        });
        let b = Arc::new(Rendered {
            width: 1,
            height: 1,
            pixels: vec![[4., 5., 6., 1.]],
        });
        let mut atlas = Atlas::default();
        let single = [ResolvedLayer {
            fill,
            image: a.clone(),
        }];
        assert!(atlas.update(&single, usize::MAX).unwrap());
        assert_eq!(
            atlas.data().as_ptr(),
            a.pixels.as_ptr().cast::<f32>(),
            "Single asset copied instead of borrowed"
        );
        let layers = [
            ResolvedLayer {
                fill,
                image: a.clone(),
            },
            ResolvedLayer { fill, image: b },
            ResolvedLayer { fill, image: a },
        ];
        assert!(atlas.update(&layers, usize::MAX).unwrap());
        assert_eq!(atlas.offsets, [0, 8, 0]);
        assert_eq!(atlas.data().len(), 12);
        let pointer = atlas.data().as_ptr();
        assert!(!atlas.update(&layers, usize::MAX).unwrap());
        assert_eq!(
            atlas.data().as_ptr(),
            pointer,
            "Unchanged asset set repacked"
        );
        assert!(
            atlas.update(&layers, 8).is_err(),
            "GPU binding limit was ignored"
        );
        assert_eq!(
            atlas.data().as_ptr(),
            pointer,
            "Rejected packing discarded a live atlas"
        );
    }
    #[test]
    fn spans_match_exact_membership_on_wide_rows_and_subpixel_boundaries() {
        for (width, height, viewport) in [
            (100_003, 3, [0., 0., 1., 1.]),
            (257, 193, [0.2, 0.1, 0.6, 0.7]),
        ] {
            let aspect = (height as f32 / viewport[3]) / (width as f32 / viewport[2]);
            for dab in [
                MaskDab {
                    center: [0.500001, 0.5],
                    radius: 0.0137,
                },
                MaskDab {
                    center: [-0.01, 0.4],
                    radius: 0.2,
                },
            ] {
                let x_at = |x| coordinate(viewport[0], viewport[2], x, width);
                let cx = closest(width, x_at, dab.center[0]);
                for y in 0..height {
                    let dy =
                        (coordinate(viewport[1], viewport[3], y, height) - dab.center[1]) * aspect;
                    let inside = |x| {
                        let dx = x_at(x) - dab.center[0];
                        dx * dx + dy * dy <= dab.radius * dab.radius
                    };
                    let [left, right] = span(width, cx, inside);
                    for x in 0..width {
                        assert_eq!((left..right).contains(&x), inside(x));
                    }
                }
            }
        }
    }
}
