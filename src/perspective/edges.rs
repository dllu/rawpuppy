//! Straight edge extraction, subpixel support and interrupted architectural lines.
use super::{Mask, Segment};
use crate::{color, pipeline::Rendered};

pub(super) fn extract(image: &Rendered, mask: Option<&Mask>) -> Vec<Segment> {
    let w = image.width;
    let h = image.height;
    let n = w * h;
    let gray: Vec<f64> = image
        .pixels
        .iter()
        .map(|p| {
            let rgb = [p[0], p[1], p[2]].map(|v| f64::from(color::srgb_encode(v.max(0.))));
            0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
        })
        .collect();
    let mut magnitude = vec![0.; n];
    let mut normals = vec![[0.; 2]; n];
    let mut peaks = vec![false; n];
    // Scharr derivatives after a small separable blur, for stable subpixel
    // edge support without mistaking demosaicing detail for structure.
    let mut blurred = vec![0.; n];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            blurred[y * w + x] = (gray[(y - 1) * w + x - 1]
                + 2. * gray[(y - 1) * w + x]
                + gray[(y - 1) * w + x + 1]
                + 2. * gray[y * w + x - 1]
                + 4. * gray[y * w + x]
                + 2. * gray[y * w + x + 1]
                + gray[(y + 1) * w + x - 1]
                + 2. * gray[(y + 1) * w + x]
                + gray[(y + 1) * w + x + 1])
                / 16.;
        }
    }
    for y in 2..h - 2 {
        for x in 2..w - 2 {
            let i = y * w + x;
            // Blur/derivative support must come from actual photograph pixels.
            // Transparent lens gaps and cutouts must not manufacture edges.
            if (y - 2..=y + 2)
                .any(|yy| (x - 2..=x + 2).any(|xx| image.pixels[yy * w + xx][3] < 0.99))
            {
                continue;
            }
            let gx = (3. * (blurred[i - w + 1] - blurred[i - w - 1])
                + 10. * (blurred[i + 1] - blurred[i - 1])
                + 3. * (blurred[i + w + 1] - blurred[i + w - 1]))
                / 32.;
            let gy = (3. * (blurred[i + w - 1] - blurred[i - w - 1])
                + 10. * (blurred[i + w] - blurred[i - w])
                + 3. * (blurred[i + w + 1] - blurred[i - w + 1]))
                / 32.;
            let mag = gx.hypot(gy);
            if mag > 1e-8 {
                magnitude[i] = mag;
                normals[i] = [gx / mag, gy / mag];
            }
        }
    }
    for y in 3..h - 3 {
        for x in 3..w - 3 {
            let i = y * w + x;
            let [nx, ny] = normals[i];
            let offset: isize = if nx.abs() > ny.abs() * 2.414 {
                1
            } else if ny.abs() > nx.abs() * 2.414 {
                w as isize
            } else if nx * ny > 0. {
                w as isize + 1
            } else {
                w as isize - 1
            };
            peaks[i] = magnitude[i] >= 0.018
                && magnitude[i] >= magnitude[(i as isize + offset) as usize]
                && magnitude[i] > magnitude[(i as isize - offset) as usize];
        }
    }
    let mut seeds: Vec<_> = (0..n)
        .filter(|&i| peaks[i] && mask.is_none_or(|m| m.values[i] >= 0.5))
        .collect();
    seeds.sort_unstable_by(|&a, &b| magnitude[b].total_cmp(&magnitude[a]));
    let mut seen = vec![false; n];
    let mut result = vec![];
    let min_length = ((w as f64).hypot(h as f64) * 0.018).max(20.);
    for seed in seeds {
        if seen[seed] {
            continue;
        }
        let normal = normals[seed];
        let mut points = vec![seed];
        let mut support = EdgeSupport::default();
        support.add(subpixel(seed, &magnitude, &normals, w));
        seen[seed] = true;
        let mut cursor = 0;
        while cursor < points.len() {
            let i = points[cursor];
            cursor += 1;
            let px = i % w;
            let py = i / w;
            for dy in -2_isize..=2 {
                for dx in -2_isize..=2 {
                    let x = px as isize + dx;
                    let y = py as isize + dy;
                    if x < 3 || y < 3 || x >= (w - 3) as isize || y >= (h - 3) as isize {
                        continue;
                    }
                    let j = y as usize * w + x as usize;
                    if seen[j]
                        || !peaks[j]
                        || mask.is_some_and(|m| m.values[j] < 0.5)
                        || (normals[j][0] * normal[0] + normals[j][1] * normal[1]) < 0.985
                    {
                        continue;
                    }
                    if (dx as f64 * normal[0] + dy as f64 * normal[1]).abs() > 1.5 {
                        continue;
                    }
                    let position = subpixel(j, &magnitude, &normals, w);
                    // Local connectivity alone floods into parallel masonry,
                    // window corners and branching texture. Keep one narrow
                    // global straight support as the region grows.
                    if support.distance(position, normal).abs() > 1.25 {
                        continue;
                    }
                    seen[j] = true;
                    points.push(j);
                    support.add(position);
                }
            }
        }
        if points.len() < min_length as usize {
            continue;
        }
        if let Some(segment) = fit_segment(&points, &magnitude, &normals, w, h, min_length)
            && straight_continuation(&points, &segment, &peaks, &normals, w, h, min_length)
        {
            result.push(segment);
        }
    }
    result.sort_unstable_by(|a, b| b.length().total_cmp(&a.length()));
    // Opposite edges of one narrow beam/window frame are one geometric vote.
    let mut unique: Vec<Segment> = vec![];
    for candidate in result {
        let plane = candidate.plane();
        let duplicate = unique.iter().any(|s| {
            let p = s.plane();
            let aligned = (plane[0] * p[0] + plane[1] * p[1]).abs() > 0.9998;
            let m = candidate.midpoint();
            aligned
                && (p[0] * m[0] + p[1] * m[1] + p[2]).abs() < 0.006
                && (candidate.midpoint()[0] - s.midpoint()[0])
                    .hypot(candidate.midpoint()[1] - s.midpoint()[1])
                    < (candidate.length() + s.length()) * 0.55
        });
        if !duplicate {
            unique.push(candidate);
        }
        if unique.len() == 480 {
            break;
        }
    }
    merge_collinear(unique, w)
}

/// Repeated window jambs and masonry often interrupt a physical straight line.
/// Recover its long baseline from several independently verified fragments,
/// rather than estimating perspective from each tiny window frame's angle.
fn merge_collinear(segments: Vec<Segment>, width: usize) -> Vec<Segment> {
    let tolerance = 1.5 / width as f64;
    let mut groups = Vec::new();
    for i in 0..segments.len() {
        for j in i + 1..segments.len() {
            let a = segments[i].midpoint();
            let b = segments[j].midpoint();
            let delta = [b[0] - a[0], b[1] - a[1]];
            let separation = delta[0].hypot(delta[1]);
            if separation < 0.12 {
                continue;
            }
            let direction = [delta[0] / separation, delta[1] / separation];
            if [i, j].iter().any(|&k| {
                let v = segments[k].vector();
                (v[0] * direction[0] + v[1] * direction[1]).abs() / segments[k].length() < 0.9988
            }) {
                continue;
            }
            let normal = [-direction[1], direction[0]];
            let distance = |p: [f64; 2]| (p[0] - a[0]) * normal[0] + (p[1] - a[1]) * normal[1];
            let members: Vec<_> = segments
                .iter()
                .enumerate()
                .filter_map(|(k, s)| {
                    (distance(s.start).abs() < tolerance && distance(s.end).abs() < tolerance)
                        .then_some(k)
                })
                .collect();
            if members.len() < 3 {
                continue;
            }
            let points: Vec<_> = members
                .iter()
                .flat_map(|&k| [segments[k].start, segments[k].end])
                .collect();
            let mean = [
                points.iter().map(|p| p[0]).sum::<f64>() / points.len() as f64,
                points.iter().map(|p| p[1]).sum::<f64>() / points.len() as f64,
            ];
            let mut covariance = [0.; 3];
            for p in &points {
                let x = p[0] - mean[0];
                let y = p[1] - mean[1];
                covariance[0] += x * x;
                covariance[1] += x * y;
                covariance[2] += y * y;
            }
            let angle = 0.5 * (2. * covariance[1]).atan2(covariance[0] - covariance[2]);
            let direction = [angle.cos(), angle.sin()];
            let normal = [-direction[1], direction[0]];
            let residual =
                |p: [f64; 2]| (p[0] - mean[0]) * normal[0] + (p[1] - mean[1]) * normal[1];
            let rms = (points.iter().map(|&p| residual(p).powi(2)).sum::<f64>()
                / points.len() as f64)
                .sqrt()
                * width as f64;
            if rms > 0.55
                || points
                    .iter()
                    .any(|&p| residual(p).abs() * width as f64 > 1.25)
            {
                continue;
            }
            let project =
                |p: [f64; 2]| (p[0] - mean[0]) * direction[0] + (p[1] - mean[1]) * direction[1];
            let mut intervals: Vec<_> = members
                .iter()
                .map(|&k| {
                    let a = project(segments[k].start);
                    let b = project(segments[k].end);
                    [a.min(b), a.max(b)]
                })
                .collect();
            intervals.sort_unstable_by(|a, b| a[0].total_cmp(&b[0]));
            let start = intervals[0][0];
            let end = intervals
                .iter()
                .map(|p| p[1])
                .fold(f64::NEG_INFINITY, f64::max);
            let coverage = members.iter().map(|&k| segments[k].length()).sum::<f64>();
            if end - start < 0.18 || coverage < (end - start) * 0.25 {
                continue;
            }
            let mut separated = 0;
            let mut previous = f64::NEG_INFINITY;
            for interval in intervals {
                if interval[0] > previous + 3. / width as f64 {
                    separated += 1;
                }
                previous = previous.max(interval[1]);
            }
            if separated < 3 {
                continue;
            }
            let point = |t: f64| [mean[0] + direction[0] * t, mean[1] + direction[1] * t];
            groups.push((
                coverage * (end - start),
                members,
                Segment {
                    start: point(start),
                    end: point(end),
                    straightness: rms,
                },
            ));
        }
    }
    groups.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
    let mut used = vec![false; segments.len()];
    let mut result = Vec::new();
    for (_, members, segment) in groups {
        if members.iter().any(|&k| used[k]) {
            continue;
        }
        for k in members {
            used[k] = true;
        }
        result.push(segment);
    }
    result.extend(
        segments
            .into_iter()
            .enumerate()
            .filter_map(|(i, s)| (!used[i]).then_some(s)),
    );
    result.sort_unstable_by(|a, b| b.length().total_cmp(&a.length()));
    result.truncate(320);
    result
}

/// A quantized arc can contain a perfectly flat short staircase run. Follow
/// the physical edge beyond both fitted endpoints, allowing its local tangent
/// to turn, and reject a smooth departure. Sharp architectural corners stop
/// the walk. This context is read from the photograph, even outside its mask.
fn straight_continuation(
    points: &[usize],
    segment: &Segment,
    peaks: &[bool],
    normals: &[[f64; 2]],
    w: usize,
    h: usize,
    minimum: f64,
) -> bool {
    let vector = segment.vector();
    let length = segment.length();
    let direction = [vector[0] / length, vector[1] / length];
    let plane = segment.plane();
    for sign in [-1., 1.] {
        let Some(&start) = points.iter().max_by(|&&a, &&b| {
            let project =
                |i: usize| sign * ((i % w) as f64 * direction[0] + (i / w) as f64 * direction[1]);
            project(a).total_cmp(&project(b))
        }) else {
            return false;
        };
        let mut current = start;
        let mut tangent = direction.map(|x| x * sign);
        let mut traveled = 0.;
        for _ in 0..(minimum * 2.) as usize {
            let px = current % w;
            let py = current / w;
            let normal = normals[current];
            let mut best = None;
            let mut best_score = f64::NEG_INFINITY;
            for dy in -3_isize..=3 {
                for dx in -3_isize..=3 {
                    let x = px as isize + dx;
                    let y = py as isize + dy;
                    if x < 3 || y < 3 || x >= (w - 3) as isize || y >= (h - 3) as isize {
                        continue;
                    }
                    let advance = dx as f64 * tangent[0] + dy as f64 * tangent[1];
                    let perpendicular = dx as f64 * tangent[1] - dy as f64 * tangent[0];
                    if advance < 0.6 || advance > 3.2 || perpendicular.abs() > 2. {
                        continue;
                    }
                    let i = y as usize * w + x as usize;
                    if !peaks[i] || normals[i][0] * normal[0] + normals[i][1] * normal[1] < 0.85 {
                        continue;
                    }
                    let score = advance - 0.75 * perpendicular.abs();
                    if score > best_score {
                        best_score = score;
                        best = Some(i);
                    }
                }
            }
            let Some(next) = best else {
                break;
            };
            traveled += ((next % w) as f64 - px as f64).hypot((next / w) as f64 - py as f64);
            current = next;
            let mut local = [-normals[next][1], normals[next][0]];
            if local[0] * tangent[0] + local[1] * tangent[1] < 0. {
                local = local.map(|x| -x);
            }
            // A straight jamb may end in a rounded window corner. A rapid
            // departure is a corner, while slow arc tangents still fail the
            // extended straight-support check below.
            if (local[0] * direction[0] * sign + local[1] * direction[1] * sign) < 0.866 {
                break;
            }
            let point = [
                ((next % w) as f64 + 0.5) / w as f64 - 0.5,
                ((next / w) as f64 + 0.5 - h as f64 * 0.5) / w as f64,
            ];
            if (plane[0] * point[0] + plane[1] * point[1] + plane[2]).abs() * w as f64 > 1.7 {
                return false;
            }
            tangent = local;
            if traveled > minimum * 1.5 {
                break;
            }
        }
    }
    true
}

/// Locate the gradient peak along its own normal instead of fitting integer
/// staircase pixels. Subpixel line orientation matters for distant vanishing points.
fn subpixel(i: usize, magnitude: &[f64], normals: &[[f64; 2]], w: usize) -> [f64; 2] {
    let x = (i % w) as f64;
    let y = (i / w) as f64;
    let [nx, ny] = normals[i];
    let sample = |x: f64, y: f64| {
        let ix = x.floor() as usize;
        let iy = y.floor() as usize;
        let tx = x - ix as f64;
        let ty = y - iy as f64;
        (1. - ty) * ((1. - tx) * magnitude[iy * w + ix] + tx * magnitude[iy * w + ix + 1])
            + ty * ((1. - tx) * magnitude[(iy + 1) * w + ix]
                + tx * magnitude[(iy + 1) * w + ix + 1])
    };
    let a = sample(x - nx, y - ny);
    let b = magnitude[i];
    let c = sample(x + nx, y + ny);
    let curvature = a - 2. * b + c;
    let offset = if curvature < -1e-8 {
        (0.5 * (a - c) / curvature).clamp(-0.5, 0.5)
    } else {
        0.
    };
    [x + nx * offset, y + ny * offset]
}

#[derive(Default)]
struct EdgeSupport {
    count: usize,
    mean: [f64; 2],
    covariance: [f64; 3],
}
impl EdgeSupport {
    fn add(&mut self, p: [f64; 2]) {
        self.count += 1;
        let delta = [p[0] - self.mean[0], p[1] - self.mean[1]];
        for (mean, d) in self.mean.iter_mut().zip(delta) {
            *mean += d / self.count as f64;
        }
        self.covariance[0] += delta[0] * (p[0] - self.mean[0]);
        self.covariance[1] += delta[0] * (p[1] - self.mean[1]);
        self.covariance[2] += delta[1] * (p[1] - self.mean[1]);
    }
    fn distance(&self, p: [f64; 2], seed_normal: [f64; 2]) -> f64 {
        let normal = if self.count < 8 {
            seed_normal
        } else {
            let angle =
                0.5 * (2. * self.covariance[1]).atan2(self.covariance[0] - self.covariance[2]);
            [-angle.sin(), angle.cos()]
        };
        (p[0] - self.mean[0]) * normal[0] + (p[1] - self.mean[1]) * normal[1]
    }
}

fn fit_segment(
    points: &[usize],
    magnitude: &[f64],
    normals: &[[f64; 2]],
    w: usize,
    h: usize,
    minimum: f64,
) -> Option<Segment> {
    let count = points.len() as f64;
    let mean = [
        points
            .iter()
            .map(|&i| subpixel(i, magnitude, normals, w)[0])
            .sum::<f64>()
            / count,
        points
            .iter()
            .map(|&i| subpixel(i, magnitude, normals, w)[1])
            .sum::<f64>()
            / count,
    ];
    let mut covariance = [0.; 3];
    for &i in points {
        let position = subpixel(i, magnitude, normals, w);
        let x = position[0] - mean[0];
        let y = position[1] - mean[1];
        covariance[0] += x * x;
        covariance[1] += x * y;
        covariance[2] += y * y;
    }
    let angle = 0.5 * (2. * covariance[1]).atan2(covariance[0] - covariance[2]);
    let direction = [angle.cos(), angle.sin()];
    let normal = [-direction[1], direction[0]];
    let mut projected = Vec::with_capacity(points.len());
    let mut squared = 0.;
    for &i in points {
        let position = subpixel(i, magnitude, normals, w);
        let p = [position[0] - mean[0], position[1] - mean[1]];
        let t = p[0] * direction[0] + p[1] * direction[1];
        let r = p[0] * normal[0] + p[1] * normal[1];
        squared += r * r;
        projected.push((t, r, magnitude[i]));
    }
    projected.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let start = projected.first()?.0;
    let end = projected.last()?.0;
    let length = end - start;
    let rms = (squared / count).sqrt();
    if length < minimum || rms > 0.65 || projected.iter().any(|p| p.1.abs() > 1.7) {
        return None;
    }
    // Test the whole edge, including its slow bow. Short tangents extracted
    // from an arc fail these binned residual/curvature checks even when a
    // permissive local line detector would call them straight.
    let mut bins = [[0.; 3]; 8];
    for &(t, r, weight) in &projected {
        let b = (((t - start) / length * 8.) as usize).min(7);
        bins[b][0] += r * weight;
        bins[b][1] += weight;
        bins[b][2] += 1.;
    }
    if bins.iter().any(|b| b[2] < 2.) {
        return None;
    }
    let residuals: Vec<_> = bins.iter().map(|b| b[0] / b[1]).collect();
    if residuals.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - residuals.iter().copied().fold(f64::INFINITY, f64::min)
        > 0.75
    {
        return None;
    }
    let ends = (residuals[0] + residuals[1] + residuals[6] + residuals[7]) * 0.25;
    let middle = (residuals[3] + residuals[4]) * 0.5;
    if (ends - middle).abs() > 0.35 {
        return None;
    }
    // Require continuous support, not a line drawn through unrelated twigs.
    if projected.windows(2).any(|p| p[1].0 - p[0].0 > 4.) {
        return None;
    }
    let coordinate = |t: f64| {
        [
            (mean[0] + direction[0] * t + 0.5) / w as f64 - 0.5,
            (mean[1] + direction[1] * t + 0.5 - h as f64 * 0.5) / w as f64,
        ]
    };
    Some(Segment {
        start: coordinate(start),
        end: coordinate(end),
        straightness: rms,
    })
}
