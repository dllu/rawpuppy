//! Architectural perspective from verified straight edges and robust vanishing directions.
//!
//! Detection runs on a bounded, lens-corrected preview, before the existing
//! perspective/crop/display edits. Semantic masks select evidence; their
//! boundaries never become evidence. All fitting uses square-pixel coordinates
//! and homogeneous vanishing points, including points at infinity.
use crate::{
    color,
    edits::{Edits, GeometryEdits, Reconstruction},
    input::{SensorImage, pixel_count},
    pipeline::Rendered,
    render::Renderer,
};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Mode {
    #[default]
    Vertical,
    Both,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum MaskProvider {
    #[default]
    Auto,
    Classical,
    Sam3,
}

/// CIPA's equivalent focal length uses the 43.27 mm reference diagonal. Image
/// orientation/aspect and the existing lens framing scale determine horizontal FOV.
pub fn horizontal_fov(
    equivalent: f32,
    width: usize,
    height: usize,
    frame_scale: f32,
) -> Option<f32> {
    if !equivalent.is_finite()
        || equivalent <= 0.
        || width == 0
        || height == 0
        || !frame_scale.is_finite()
        || frame_scale <= 0.
    {
        return None;
    }
    let aspect = height as f64 / width as f64;
    let reference_width = 43.27 / (1. + aspect * aspect).sqrt();
    Some(
        (2. * (reference_width * f64::from(frame_scale) / (2. * f64::from(equivalent)))
            .atan()
            .to_degrees()) as f32,
    )
}

#[derive(Default)]
pub struct Engine {
    #[cfg(feature = "sam3")]
    model: Option<crate::sam3::Model>,
}
impl Engine {
    pub fn run(
        &mut self,
        renderer: &mut Renderer,
        image: Arc<SensorImage>,
        edits: &Edits,
        mode: Mode,
        provider: MaskProvider,
    ) -> Result<Report> {
        let preview = preview(renderer, image, edits)?;
        #[cfg(feature = "sam3")]
        let mask = if provider == MaskProvider::Sam3
            || (provider == MaskProvider::Auto
                && crate::sam3::directory()?.join("manifest.json").is_file())
        {
            if self.model.is_none() {
                self.model = Some(crate::sam3::Model::open(
                    &crate::sam3::directory()?,
                    crate::ml_runtime::InferenceDevice::Auto,
                )?);
            }
            Some(self.model.as_ref().unwrap().mask(&preview)?)
        } else {
            None
        };
        #[cfg(not(feature = "sam3"))]
        let mask: Option<Mask> = {
            ensure!(
                provider != MaskProvider::Sam3,
                "Build with the sam3 feature and install its local model to use semantic masks"
            );
            None
        };
        let mut report = analyze(&preview, mask.as_ref(), &edits.geometry, mode)?;
        if mask.is_some() {
            report.mask_provider = "SAM 3".into();
        }
        Ok(report)
    }
}

/// Architectural support in the exact analysis canvas, row-major probabilities.
pub struct Mask {
    pub width: usize,
    pub height: usize,
    pub values: Vec<f32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Segment {
    /// Endpoints in centered square-pixel coordinates, measured in image widths.
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub straightness: f64,
}
impl Segment {
    fn vector(&self) -> [f64; 2] {
        [self.end[0] - self.start[0], self.end[1] - self.start[1]]
    }
    fn length(&self) -> f64 {
        let d = self.vector();
        d[0].hypot(d[1])
    }
    fn midpoint(&self) -> [f64; 2] {
        [
            (self.start[0] + self.end[0]) * 0.5,
            (self.start[1] + self.end[1]) * 0.5,
        ]
    }
    fn plane(&self) -> [f64; 3] {
        let d = self.vector();
        let length = self.length();
        [
            -d[1] / length,
            d[0] / length,
            (d[1] * self.start[0] - d[0] * self.start[1]) / length,
        ]
    }
    fn weight(&self) -> f64 {
        self.length().min(0.25)
    }
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub analysis_size: [usize; 2],
    pub mask_provider: String,
    pub segments: Vec<Segment>,
    pub vertical_inliers: Vec<usize>,
    pub horizontal_inliers: Vec<usize>,
    pub vertical_vanishing_point: Option<[f64; 3]>,
    pub horizontal_vanishing_point: Option<[f64; 3]>,
    pub residual_degrees: Option<f64>,
    pub reason: String,
    /// None means abstention: the caller must retain the existing geometry.
    pub solution: Option<GeometryEdits>,
}

/// Native pixels for analysis retain lens distortion correction but deliberately
/// exclude old perspective, crop, brushes and generated layers.
pub fn preview(
    renderer: &mut Renderer,
    image: Arc<SensorImage>,
    edits: &Edits,
) -> Result<Rendered> {
    let mut analysis = edits.clone();
    analysis.geometry.yaw = 0.;
    analysis.geometry.pitch = 0.;
    analysis.geometry.rotation = 0.;
    analysis.geometry.shift = [0.; 2];
    analysis.geometry.scale = 1.;
    analysis.geometry.crop = [0., 0., 1., 1.];
    analysis.raw.reconstruction = Reconstruction::Mhc;
    analysis.raw.denoise = 0.;
    analysis.display = Default::default();
    analysis.scene.graduated = Default::default();
    renderer.render(image, &analysis, Some(1440))
}

pub fn analyze(
    image: &Rendered,
    mask: Option<&Mask>,
    geometry: &GeometryEdits,
    mode: Mode,
) -> Result<Report> {
    ensure!(
        image.width >= 32 && image.height >= 32,
        "Perspective analysis needs at least 32 pixels on each edge"
    );
    ensure!(
        image.width <= 2048 && image.height <= 2048,
        "Use a bounded preview for perspective analysis"
    );
    ensure!(
        image.pixels.len() == pixel_count(image.width, image.height, 1)?,
        "Incomplete perspective preview"
    );
    ensure!(
        image.pixels.iter().flatten().all(|x| x.is_finite()),
        "Perspective preview contains nonfinite pixels"
    );
    if let Some(mask) = mask {
        ensure!(
            mask.width == image.width
                && mask.height == image.height
                && mask.values.len() == image.pixels.len()
                && mask
                    .values
                    .iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
            "Invalid architectural support mask"
        );
    }
    ensure!(
        geometry.field_of_view.is_finite()
            && geometry.field_of_view > 0.
            && geometry.field_of_view < 175.,
        "Invalid perspective field of view"
    );
    let segments = extract(image, mask);
    let mut report = Report {
        analysis_size: [image.width, image.height],
        mask_provider: if mask.is_some() {
            "semantic"
        } else {
            "classical"
        }
        .into(),
        segments,
        vertical_inliers: vec![],
        horizontal_inliers: vec![],
        vertical_vanishing_point: None,
        horizontal_vanishing_point: None,
        residual_degrees: None,
        reason: "Not enough consistent, spatially separated straight architectural edges".into(),
        solution: None,
    };
    let vertical: Vec<_> = report
        .segments
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let d = s.vector();
            (d[1].abs() > d[0].abs() * 1.3).then_some(i)
        })
        .collect();
    let Some(v) = consensus(&report.segments, &vertical, true) else {
        return Ok(report);
    };
    report.vertical_vanishing_point = Some(v.point);
    report.vertical_inliers = v.inliers.clone();
    let aspect = image.height as f64 / image.width as f64;
    if v.point[2].abs() > 1e-8 && (v.point[1] / v.point[2]).abs() < aspect * 0.6 {
        report.reason="The apparent verticals converge inside the photograph; they may be roof beams or other receding structures".into();
        return Ok(report);
    }
    let f = 0.5 / (f64::from(geometry.field_of_view).to_radians() * 0.5).tan();
    let mut y = ray(v.point, f);
    if y[1] < 0. {
        y = y.map(|x| -x);
    }
    let mut x = normalize([
        y[1].hypot(y[2]),
        -y[0] * y[1] / y[1].hypot(y[2]),
        -y[0] * y[2] / y[1].hypot(y[2]),
    ]);
    if mode == Mode::Both {
        // A façade's horizontal direction is selected after removing the
        // vertical tilt. Receding roads and sloping roofs cannot vote as vertical.
        let horizontal: Vec<_> = report
            .segments
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let d = s.vector();
                (d[0].abs() > d[1].abs() * 1.3).then_some(i)
            })
            .collect();
        let Some(h) = consensus(&report.segments, &horizontal, false) else {
            report.reason = "Vertical evidence found, but no unambiguous horizontal façade direction; try Auto vertical".into();
            return Ok(report);
        };
        report.horizontal_vanishing_point = Some(h.point);
        report.horizontal_inliers = h.inliers.clone();
        x = ray(h.point, f);
        if x[0] < 0. {
            x = x.map(|a| -a);
        }
        let orthogonality = dot(x, y);
        if orthogonality.abs() > 0.06 {
            report.reason =
                "Horizontal and vertical directions do not fit one orthogonal pinhole camera"
                    .into();
            return Ok(report);
        }
        x = normalize(std::array::from_fn(|i| x[i] - orthogonality * y[i]));
    }
    let z = cross(x, y);
    let pitch = (-z[1]).asin().to_degrees();
    let yaw = z[0].atan2(z[2]).to_degrees();
    let rotation = x[1].atan2(y[1]).to_degrees();
    if z[2] <= 0. || pitch.abs() > 55. || yaw.abs() > 55. || rotation.abs() > 30. {
        report.reason =
            "The estimated camera pose is too extreme for a reliable automatic correction".into();
        return Ok(report);
    }
    let mut solution = geometry.clone();
    solution.pitch = pitch as f32;
    solution.yaw = yaw as f32;
    solution.rotation = rotation as f32;
    // Keep the photograph's original center centered after the camera rotation.
    // This translation is folded into the existing backward homography.
    solution.shift = [
        (f * x[2] / z[2] * f64::from(geometry.scale)) as f32,
        (f * y[2] / z[2] / aspect * f64::from(geometry.scale)) as f32,
    ];
    let residual =
        corrected_residual(&report.segments, &v.inliers, x, y, z, f, true).max(corrected_residual(
            &report.segments,
            &report.horizontal_inliers,
            x,
            y,
            z,
            f,
            false,
        ));
    report.residual_degrees = Some(residual);
    if !residual.is_finite() || residual > 1. {
        report.reason = "The fitted correction leaves inconsistent architectural lines".into();
        return Ok(report);
    }
    report.reason = format!(
        "{} vertical{} straight edges agree; residual {:.2}°",
        v.inliers.len(),
        if report.horizontal_inliers.is_empty() {
            String::new()
        } else {
            format!(", {} horizontal", report.horizontal_inliers.len())
        },
        residual
    );
    report.solution = Some(solution);
    Ok(report)
}

fn extract(image: &Rendered, mask: Option<&Mask>) -> Vec<Segment> {
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
            if mag >= 0.018 {
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
    let min_length = (w as f64).hypot(h as f64) * 0.035;
    for seed in seeds {
        if seen[seed] {
            continue;
        }
        let normal = normals[seed];
        let mut points = vec![seed];
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
                    seen[j] = true;
                    points.push(j);
                }
            }
        }
        if points.len() < min_length as usize {
            continue;
        }
        if let Some(segment) = fit_segment(&points, &magnitude, w, h, min_length)
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
        if unique.len() == 240 {
            break;
        }
    }
    unique
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
            let point = [
                ((next % w) as f64 + 0.5) / w as f64 - 0.5,
                ((next / w) as f64 + 0.5 - h as f64 * 0.5) / w as f64,
            ];
            if (plane[0] * point[0] + plane[1] * point[1] + plane[2]).abs() * w as f64 > 1.7 {
                return false;
            }
            let mut local = [-normals[next][1], normals[next][0]];
            if local[0] * tangent[0] + local[1] * tangent[1] < 0. {
                local = local.map(|x| -x);
            }
            tangent = local;
            if traveled > minimum * 1.5 {
                break;
            }
        }
    }
    true
}

fn fit_segment(
    points: &[usize],
    magnitude: &[f64],
    w: usize,
    h: usize,
    minimum: f64,
) -> Option<Segment> {
    let count = points.len() as f64;
    let mean = [
        points.iter().map(|i| (i % w) as f64).sum::<f64>() / count,
        points.iter().map(|i| (i / w) as f64).sum::<f64>() / count,
    ];
    let mut covariance = [0.; 3];
    for &i in points {
        let x = (i % w) as f64 - mean[0];
        let y = (i / w) as f64 - mean[1];
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
        let p = [(i % w) as f64 - mean[0], (i / w) as f64 - mean[1]];
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

struct Consensus {
    point: [f64; 3],
    inliers: Vec<usize>,
}
fn error(s: &Segment, p: [f64; 3]) -> f64 {
    let plane = s.plane();
    let mid = s.midpoint();
    let distance = (p[0] - mid[0] * p[2]).hypot(p[1] - mid[1] * p[2]);
    if distance < 1e-9 {
        return f64::INFINITY;
    }
    (dot(plane, p).abs() / distance).min(1.).asin()
}
fn consensus(segments: &[Segment], candidates: &[usize], vertical: bool) -> Option<Consensus> {
    if candidates.len() < 4 {
        return None;
    }
    let threshold = 0.7_f64.to_radians();
    let mut hypotheses = vec![];
    for (a, &i) in candidates.iter().enumerate() {
        for &j in &candidates[a + 1..] {
            let point = normalize(cross(segments[i].plane(), segments[j].plane()));
            if point.iter().any(|v| !v.is_finite()) {
                continue;
            }
            let inliers: Vec<_> = candidates
                .iter()
                .copied()
                .filter(|&k| error(&segments[k], point) < threshold)
                .collect();
            let score = inliers.iter().map(|&k| segments[k].weight()).sum::<f64>();
            hypotheses.push((score, point, inliers));
        }
    }
    hypotheses.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
    let (score, mut point, mut inliers) = hypotheses.first()?.clone();
    let total = candidates
        .iter()
        .map(|&i| segments[i].weight())
        .sum::<f64>();
    if inliers.len() < 4 || score < total * 0.48 {
        return None;
    }
    for (_, alternative, other) in hypotheses.iter().skip(1).take_while(|h| h.0 > score * 0.88) {
        // Distinct, comparably supported directions indicate a competing
        // façade or accidental tree structure. Shared models are harmless.
        let overlap = other.iter().filter(|i| inliers.contains(i)).count();
        if overlap * 2 < inliers.len() && dot(point, *alternative).abs() < 0.9994 {
            return None;
        }
    }
    for _ in 0..3 {
        let mut matrix = [[0.; 3]; 3];
        for &i in &inliers {
            let s = &segments[i];
            let p = s.plane();
            let m = s.midpoint();
            let d = (point[0] - m[0] * point[2]).hypot(point[1] - m[1] * point[2]);
            let weight = s.weight() / d.max(0.1).powi(2);
            for r in 0..3 {
                for c in 0..3 {
                    matrix[r][c] += weight * p[r] * p[c];
                }
            }
        }
        point = smallest_eigenvector(matrix);
        inliers = candidates
            .iter()
            .copied()
            .filter(|&i| error(&segments[i], point) < threshold)
            .collect();
    }
    if inliers.len() < 4 {
        return None;
    }
    let positions: Vec<_> = inliers
        .iter()
        .map(|&i| segments[i].midpoint()[usize::from(!vertical)])
        .collect();
    let spread = positions.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - positions.iter().copied().fold(f64::INFINITY, f64::min);
    if spread < 0.08 {
        return None;
    }
    Some(Consensus { point, inliers })
}

fn smallest_eigenvector(mut a: [[f64; 3]; 3]) -> [f64; 3] {
    let mut v = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    for _ in 0..32 {
        let (p, q) = [(0, 1), (0, 2), (1, 2)]
            .into_iter()
            .max_by(|&(p, q), &(r, s)| a[p][q].abs().total_cmp(&a[r][s].abs()))
            .unwrap();
        if a[p][q].abs() < 1e-14 {
            break;
        }
        let theta = 0.5 * (2. * a[p][q]).atan2(a[q][q] - a[p][p]);
        let (s, c) = theta.sin_cos();
        let app = a[p][p];
        let aqq = a[q][q];
        let apq = a[p][q];
        a[p][p] = c * c * app - 2. * s * c * apq + s * s * aqq;
        a[q][q] = s * s * app + 2. * s * c * apq + c * c * aqq;
        a[p][q] = 0.;
        a[q][p] = 0.;
        for r in 0..3 {
            if r != p && r != q {
                let arp = a[r][p];
                let arq = a[r][q];
                a[r][p] = c * arp - s * arq;
                a[p][r] = a[r][p];
                a[r][q] = s * arp + c * arq;
                a[q][r] = a[r][q];
            }
            let vrp = v[r][p];
            let vrq = v[r][q];
            v[r][p] = c * vrp - s * vrq;
            v[r][q] = s * vrp + c * vrq;
        }
    }
    let i = (0..3).min_by(|&i, &j| a[i][i].total_cmp(&a[j][j])).unwrap();
    normalize([v[0][i], v[1][i], v[2][i]])
}
fn ray(p: [f64; 3], f: f64) -> [f64; 3] {
    normalize([p[0] / f, p[1] / f, p[2]])
}
fn normalize(p: [f64; 3]) -> [f64; 3] {
    let n = dot(p, p).sqrt();
    p.map(|x| x / n)
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn corrected_residual(
    segments: &[Segment],
    inliers: &[usize],
    x: [f64; 3],
    y: [f64; 3],
    z: [f64; 3],
    f: f64,
    vertical: bool,
) -> f64 {
    let mut angles: Vec<_> = inliers
        .iter()
        .map(|&i| {
            let project = |p: [f64; 2]| {
                let p = [p[0] / f, p[1] / f, 1.];
                let d = dot(z, p);
                [dot(x, p) / d, dot(y, p) / d]
            };
            let a = project(segments[i].start);
            let b = project(segments[i].end);
            let d = [b[0] - a[0], b[1] - a[1]];
            if vertical {
                d[0].abs().atan2(d[1].abs()).to_degrees()
            } else {
                d[1].abs().atan2(d[0].abs()).to_degrees()
            }
        })
        .collect();
    if angles.is_empty() {
        return 0.;
    }
    angles.sort_unstable_by(f64::total_cmp);
    angles[(angles.len() * 9 / 10).min(angles.len() - 1)]
}
