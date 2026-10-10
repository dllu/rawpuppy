//! Working-linear boundary matching for bounded generated contexts.
use crate::pipeline::Rendered;
use anyhow::{Result, ensure};

/// Match new painted contexts in linear RGB, replacing the prediction only after
/// a successful solve. Unanchored contexts retain their generated colors.
pub fn match_background_v1(
    input: &Rendered,
    generated: &mut Rendered,
    mask: &[f32],
) -> Result<super::Harmonization> {
    ensure!(
        mask.iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
        "Invalid matching mask"
    );
    let mut binary = Vec::new();
    binary.try_reserve_exact(mask.len())?;
    binary.extend(mask.iter().map(|v| u8::from(*v > 0.)));
    let (matched, report) = harmonize(input, generated, &binary, 0., 2048)?;
    if report.boundary_pixels == 0 {
        Ok(super::Harmonization::None)
    } else {
        *generated = matched;
        Ok(super::Harmonization::BoundaryPoissonV1)
    }
}

fn buffer<T: Clone>(count: usize, value: T) -> Result<Vec<T>> {
    let mut values = Vec::new();
    values.try_reserve_exact(count)?;
    values.resize(count, value);
    Ok(values)
}
fn copy<T: Clone>(values: &[T]) -> Result<Vec<T>> {
    let mut result = Vec::new();
    result.try_reserve_exact(values.len())?;
    result.extend_from_slice(values);
    Ok(result)
}

fn neighbors(i: usize, width: usize, height: usize) -> impl Iterator<Item = usize> {
    let x = i % width;
    let y = i / width;
    [
        x.checked_sub(1).map(|_| i - 1),
        (x + 1 < width).then_some(i + 1),
        y.checked_sub(1).map(|_| i - width),
        (y + 1 < height).then_some(i + width),
    ]
    .into_iter()
    .flatten()
}

struct Equation {
    pixel: usize,
    neighbors: [usize; 4],
    diagonal: f64,
}
#[derive(serde::Serialize)]
pub struct Solve {
    pub boundary_pixels: usize,
    pub unknown_pixels: usize,
    pub iterations: usize,
    pub relative_residual: f64,
    pub screening: f64,
}

// Solve a smooth additive correction, anchored to opaque unselected neighbors.
// Selected source RGB never guides the solve: it may contain the removed object
// or withheld content. Screening limits interior changes to generated colors.
pub fn harmonize(
    input: &Rendered,
    generated: &Rendered,
    mask: &[u8],
    screening: f64,
    max_iterations: usize,
) -> Result<(Rendered, Solve)> {
    let n = crate::input::pixel_count(input.width, input.height, 1)?;
    ensure!(
        (generated.width, generated.height) == (input.width, input.height)
            && input.pixels.len() == n
            && generated.pixels.len() == n
            && mask.len() == n,
        "Mismatched rasters"
    );
    ensure!(
        screening.is_finite() && screening >= 0. && max_iterations > 0,
        "Invalid solver settings"
    );
    ensure!(
        input
            .pixels
            .iter()
            .chain(&generated.pixels)
            .flatten()
            .all(|v| v.is_finite()),
        "Nonfinite context"
    );
    let mut boundary = buffer(n, false)?;
    for (i, value) in boundary.iter_mut().enumerate() {
        *value = mask[i] > 0
            && neighbors(i, input.width, input.height)
                .any(|j| mask[j] == 0 && input.pixels[j][3] == 1.);
    }
    let boundary_count = boundary.iter().filter(|v| **v).count();
    if boundary_count == 0 {
        return Ok((
            Rendered {
                width: input.width,
                height: input.height,
                pixels: copy(&generated.pixels)?,
            },
            Solve {
                boundary_pixels: 0,
                unknown_pixels: 0,
                iterations: 0,
                relative_residual: 0.,
                screening,
            },
        ));
    }
    let mut boundary_colors = buffer(n, [0f64; 3])?;
    for i in 0..n {
        if boundary[i] {
            let mut count = 0;
            for j in neighbors(i, input.width, input.height) {
                if mask[j] == 0 && input.pixels[j][3] == 1. {
                    for (c, value) in boundary_colors[i].iter_mut().enumerate() {
                        *value += f64::from(input.pixels[j][c]);
                    }
                    count += 1;
                }
            }
            for value in &mut boundary_colors[i] {
                *value /= f64::from(count);
            }
        }
    }
    let mut mapping = buffer(n, usize::MAX)?;
    let mut equations = Vec::new();
    equations.try_reserve_exact(mask.iter().filter(|v| **v > 0).count() - boundary_count)?;
    for i in 0..n {
        if mask[i] > 0 && !boundary[i] {
            mapping[i] = equations.len();
            equations.push(Equation {
                pixel: i,
                neighbors: [usize::MAX; 4],
                diagonal: 0.,
            });
        }
    }
    let mut residual = buffer(equations.len(), [0.; 3])?;
    for (k, e) in equations.iter_mut().enumerate() {
        let mut count = 0;
        for j in neighbors(e.pixel, input.width, input.height) {
            e.diagonal += 1.;
            if mapping[j] != usize::MAX {
                e.neighbors[count] = mapping[j];
                count += 1;
            } else if boundary[j] {
                for (c, value) in residual[k].iter_mut().enumerate() {
                    *value += boundary_colors[j][c] - f64::from(generated.pixels[j][c]);
                }
            }
        }
        e.diagonal += screening;
    }
    let multiply = |values: &[[f64; 3]], product: &mut [[f64; 3]]| {
        for (k, e) in equations.iter().enumerate() {
            let mut out = values[k].map(|v| v * e.diagonal);
            for &j in &e.neighbors {
                if j != usize::MAX {
                    for c in 0..3 {
                        out[c] -= values[j][c];
                    }
                }
            }
            product[k] = out;
        }
    };
    let dot = |a: &[[f64; 3]], b: &[[f64; 3]]| -> f64 {
        a.iter()
            .zip(b)
            .map(|(a, b)| a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>())
            .sum()
    };
    let mut correction = buffer(equations.len(), [0.; 3])?;
    let mut direction = copy(&residual)?;
    let mut product = buffer(equations.len(), [0.; 3])?;
    let initial = dot(&residual, &residual);
    let mut norm = initial;
    let mut iterations = 0;
    while initial > 0. && norm / initial > 1e-14 && iterations < max_iterations {
        multiply(&direction, &mut product);
        let denominator = dot(&direction, &product);
        ensure!(
            denominator.is_finite() && denominator > 0.,
            "Unanchored or invalid solver domain"
        );
        let alpha = norm / denominator;
        for i in 0..equations.len() {
            for c in 0..3 {
                correction[i][c] += alpha * direction[i][c];
                residual[i][c] -= alpha * product[i][c];
            }
        }
        let next = dot(&residual, &residual);
        let beta = next / norm;
        for i in 0..equations.len() {
            for c in 0..3 {
                direction[i][c] = residual[i][c] + beta * direction[i][c];
            }
        }
        norm = next;
        iterations += 1;
    }
    let relative_residual = if initial > 0. {
        (norm / initial).sqrt()
    } else {
        0.
    };
    ensure!(
        relative_residual <= 1e-7,
        "Solver did not converge: {relative_residual}"
    );
    let mut pixels = copy(&input.pixels)?;
    for e in &equations {
        let k = mapping[e.pixel];
        for c in 0..3 {
            pixels[e.pixel][c] =
                (f64::from(generated.pixels[e.pixel][c]) + correction[k][c]) as f32;
        }
        pixels[e.pixel][3] = generated.pixels[e.pixel][3];
    }
    for i in 0..n {
        if boundary[i] {
            for c in 0..3 {
                pixels[i][c] = boundary_colors[i][c] as f32;
            }
            pixels[i][3] = generated.pixels[i][3];
        }
    }
    ensure!(
        pixels.iter().flatten().all(|v| v.is_finite()),
        "Nonfinite harmonized result"
    );
    Ok((
        Rendered {
            width: input.width,
            height: input.height,
            pixels,
        },
        Solve {
            boundary_pixels: boundary_count,
            unknown_pixels: equations.len(),
            iterations,
            relative_residual,
            screening,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn harmonic_correction_removes_uniform_shift_and_retains_generated_detail() {
        let (width, height) = (64, 48);
        let pixels = vec![[0.5, -0.125, 1.25, 1.]; width * height];
        let input = Rendered {
            width,
            height,
            pixels,
        };
        let mut generated = Rendered {
            width,
            height,
            pixels: input.pixels.clone(),
        };
        let mut mask = vec![0; width * height];
        for y in 8..40 {
            for x in 8..56 {
                let i = y * width + x;
                mask[i] = 255;
                for c in 0..3 {
                    generated.pixels[i][c] += [0.125, -0.25, 0.0625][c];
                }
                if x > 8 && x < 55 && y > 8 && y < 39 {
                    generated.pixels[i][0] += if (x + y) % 2 == 0 { 0.03125 } else { -0.03125 };
                }
            }
        }
        let (output, solve) = harmonize(&input, &generated, &mask, 0., 1024).unwrap();
        assert!(solve.relative_residual < 1e-7);
        let mut different_source = Rendered {
            width,
            height,
            pixels: input.pixels.clone(),
        };
        for (pixel, mask) in different_source.pixels.iter_mut().zip(&mask) {
            if *mask > 0 {
                pixel[..3].copy_from_slice(&[50., -20., 0.]);
            }
        }
        let (independent, _) = harmonize(&different_source, &generated, &mask, 0., 1024).unwrap();
        assert_eq!(
            output.pixels, independent.pixels,
            "Selected source colors leaked into the repair"
        );
        for (i, selected) in mask.iter().enumerate() {
            let x = i % width;
            let y = i / width;
            if *selected == 0 {
                assert_eq!(output.pixels[i], input.pixels[i]);
            } else {
                for c in 0..3 {
                    let detail = if c == 0 && x > 8 && x < 55 && y > 8 && y < 39 {
                        if (x + y) % 2 == 0 { 0.03125 } else { -0.03125 }
                    } else {
                        0.
                    };
                    assert!((output.pixels[i][c] - input.pixels[i][c] - detail).abs() < 1e-6);
                }
            }
        }
    }
    #[test]
    fn an_unanchored_full_mask_keeps_the_generation_exact() {
        let image = Rendered {
            width: 8,
            height: 8,
            pixels: vec![[0.5; 4]; 64],
        };
        let (output, solve) = harmonize(&image, &image, &[255; 64], 0., 1024).unwrap();
        assert_eq!(solve.boundary_pixels, 0);
        assert_eq!(output.pixels, image.pixels);
    }
    #[test]
    fn invalid_matching_requests_leave_the_prediction_unchanged() {
        let input = Rendered {
            width: 8,
            height: 8,
            pixels: vec![[0.5, -0.125, 1.25, 1.]; 64],
        };
        let mut generated = Rendered {
            width: 8,
            height: 8,
            pixels: vec![[0.75, 0.5, 0.25, 1.]; 64],
        };
        let before = generated.pixels.clone();
        for value in [f32::NAN, -0.1, 1.1] {
            assert!(match_background_v1(&input, &mut generated, &[value; 64]).is_err());
            assert_eq!(generated.pixels, before);
        }
        assert!(match_background_v1(&input, &mut generated, &[1.; 63]).is_err());
        assert_eq!(generated.pixels, before);
        let mut mask = [0; 64];
        for y in 1..7 {
            for x in 1..7 {
                mask[y * 8 + x] = 1;
            }
        }
        assert!(
            harmonize(&input, &generated, &mask, 0., 1).is_err(),
            "An unfinished solve was accepted"
        );
        assert_eq!(generated.pixels, before);
    }
}
