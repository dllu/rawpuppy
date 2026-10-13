//! Bounded coverage search for the largest gap-free corrected crop.
use crate::{edits::Edits, geometry::Geometry, input::SensorImage};
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Constraint {
    LargestArea,
    Aspect(f32),
}

/// Search every position in a conservative coverage grid, including lens and
/// chromatic-aberration mappings. Cost is independent of original megapixels.
pub fn largest(image: &SensorImage, edits: &Edits, constraint: Constraint) -> Result<[f32; 4]> {
    edits.validate()?;
    let mut e = edits.geometry.clone();
    e.crop = [0., 0., 1., 1.];
    let mut geometry = Geometry::compile(&e, image.metadata.width, image.metadata.height)?;
    geometry.lens = crate::lens::Correction::compile(
        image.metadata.lens_profile.as_ref(),
        &edits.lens,
        image.metadata.width,
        image.metadata.height,
    )?;
    let n = 384;
    let valid = |x: f32, y: f32| {
        (0..3).all(|c| {
            geometry
                .map([x, y], c)
                .is_some_and(|p| p.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v)))
        })
    };
    let vertices: Vec<_> = (0..(n + 1) * (n + 1))
        .map(|i| {
            valid(
                (i % (n + 1)) as f32 / n as f32,
                (i / (n + 1)) as f32 / n as f32,
            )
        })
        .collect();
    let cells: Vec<_> = (0..n * n)
        .map(|i| {
            let (x, y) = (i % n, i / n);
            let v = y * (n + 1) + x;
            vertices[v]
                && vertices[v + 1]
                && vertices[v + n + 1]
                && vertices[v + n + 2]
                && valid((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32)
        })
        .collect();
    let mut best = [0usize; 4];
    match constraint {
        Constraint::LargestArea => {
            let mut heights = vec![0usize; n];
            for y in 0..n {
                for x in 0..n {
                    heights[x] = if cells[y * n + x] { heights[x] + 1 } else { 0 };
                }
                let mut stack: Vec<(usize, usize)> = vec![];
                for (x, height) in heights
                    .iter()
                    .copied()
                    .chain(std::iter::once(0))
                    .enumerate()
                {
                    let mut left = x;
                    while stack.last().is_some_and(|(_, h)| *h > height) {
                        let (start, h) = stack.pop().unwrap();
                        let candidate = [start, y + 1 - h, x - start, h];
                        if better(candidate, best, n) {
                            best = candidate;
                        }
                        left = start;
                    }
                    if height > 0 && stack.last().is_none_or(|(_, h)| *h < height) {
                        stack.push((left, height));
                    }
                }
            }
        }
        Constraint::Aspect(aspect) => {
            ensure!(
                aspect.is_finite() && aspect > 0.,
                "Invalid crop aspect ratio"
            );
            let ratio = aspect as f64 * image.metadata.height as f64 / image.metadata.width as f64;
            let stride = n + 1;
            let mut sums = vec![0u32; stride * stride];
            for y in 0..n {
                let mut row = 0;
                for x in 0..n {
                    row += u32::from(!cells[y * n + x]);
                    sums[(y + 1) * stride + x + 1] = sums[y * stride + x + 1] + row;
                }
            }
            let find = |major: usize| {
                let (width, height) = if ratio >= 1. {
                    (major, (major as f64 / ratio).ceil() as usize)
                } else {
                    ((major as f64 * ratio).ceil() as usize, major)
                };
                let mut result = [0; 4];
                if height > n {
                    return result;
                }
                for y in 0..=n - height {
                    for x in 0..=n - width {
                        let invalid = sums[(y + height) * stride + x + width]
                            + sums[y * stride + x]
                            - sums[y * stride + x + width]
                            - sums[(y + height) * stride + x];
                        let candidate = [x, y, width, height];
                        if invalid == 0 && better(candidate, result, n) {
                            result = candidate;
                        }
                    }
                }
                result
            };
            let mut low = 1;
            let mut high = n;
            while low <= high {
                let width = (low + high) / 2;
                let candidate = find(width);
                if candidate[2] > 0 {
                    best = candidate;
                    low = width + 1;
                } else {
                    high = width - 1;
                }
            }
        }
    }
    ensure!(
        best[2] > 0 && best[3] > 0,
        "No gap-free crop found for this correction and aspect ratio"
    );
    let mut crop = best.map(|v| v as f32 / n as f32);
    if let Constraint::Aspect(aspect) = constraint {
        let ratio = aspect as f64 * image.metadata.height as f64 / image.metadata.width as f64;
        if ratio >= 1. {
            let height = (crop[2] as f64 / ratio) as f32;
            crop[1] += (crop[3] - height) * 0.5;
            crop[3] = height;
        } else {
            let width = (crop[3] as f64 * ratio) as f32;
            crop[0] += (crop[2] - width) * 0.5;
            crop[2] = width;
        }
    }
    Ok(crop)
}

fn better(a: [usize; 4], b: [usize; 4], n: usize) -> bool {
    let area = |r: [usize; 4]| r[2] * r[3];
    let distance = |r: [usize; 4]| {
        let x = (2 * r[0] + r[2]) as f64 - n as f64;
        let y = (2 * r[1] + r[3]) as f64 - n as f64;
        x * x + y * y
    };
    area(a) > area(b) || area(a) == area(b) && distance(a) < distance(b)
}
