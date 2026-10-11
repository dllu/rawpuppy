//! Vanishing direction consensus and constrained pinhole rectification.
use super::{Mode, Report, Segment};
use crate::edits::GeometryEdits;
use anyhow::{Result, ensure};

/// Fit verified source-image edges. Kept separate from raster extraction for
/// independent camera/line regression tests and reviewable diagnostics.
pub fn fit_segments(
    segments: Vec<Segment>,
    analysis_size: [usize; 2],
    geometry: &GeometryEdits,
    mode: Mode,
) -> Result<Report> {
    ensure!(
        analysis_size.iter().all(|v| *v > 0)
            && geometry.field_of_view.is_finite()
            && geometry.field_of_view > 0.
            && geometry.field_of_view < 175.,
        "Invalid perspective canvas or intrinsics"
    );
    ensure!(
        segments.iter().all(
            |s| s.start.iter().chain(s.end.iter()).all(|v| v.is_finite())
                && s.length() > 1e-8
                && s.straightness.is_finite()
                && s.straightness >= 0.
        ),
        "Invalid straight-line evidence"
    );
    let mut report = Report {
        analysis_size,
        mask_provider: "classical".into(),
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
    let Some(v) = consensus(&report.segments, &vertical, true, analysis_size[0], None) else {
        return Ok(report);
    };
    report.vertical_vanishing_point = Some(v.point);
    report.vertical_inliers = v.inliers.clone();
    let aspect = analysis_size[1] as f64 / analysis_size[0] as f64;
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
                let project = |p: [f64; 2]| {
                    let p = [p[0] / f, p[1] / f, 1.];
                    let z = cross(x, y);
                    let depth = dot(z, p);
                    [dot(x, p) / depth, dot(y, p) / depth]
                };
                let a = project(s.start);
                let b = project(s.end);
                let d = [b[0] - a[0], b[1] - a[1]];
                (d[0].abs() > d[1].abs() * 1.3).then_some(i)
            })
            .collect();
        let horizon = [y[0] / f, y[1] / f, y[2]];
        let Some(h) = consensus(
            &report.segments,
            &horizontal,
            false,
            analysis_size[0],
            Some(horizon),
        ) else {
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
fn consensus(
    segments: &[Segment],
    candidates: &[usize],
    vertical: bool,
    width: usize,
    constraint: Option<[f64; 3]>,
) -> Option<Consensus> {
    if candidates.len() < 4 {
        return None;
    }
    let threshold = 0.7_f64.to_radians();
    let mut hypotheses = vec![];
    for (a, &i) in candidates.iter().enumerate() {
        let planes: Vec<_> = if let Some(horizon) = constraint {
            vec![horizon]
        } else {
            candidates[a + 1..]
                .iter()
                .map(|&j| segments[j].plane())
                .collect()
        };
        for plane in planes {
            let point = normalize(cross(segments[i].plane(), plane));
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
    // A steep receding façade can contain more nominally vertical image lines
    // than the actual gravity direction. Select the near-upright family, and
    // let strongly supported non-gravity families account for their own votes.
    let gravity = |p: [f64; 3]| p[0].abs() <= p[1].abs() * 0.466307658;
    let (score, mut point, mut inliers) = hypotheses
        .iter()
        .find(|h| !vertical || gravity(h.1))?
        .clone();
    let mut excluded = vec![false; segments.len()];
    if vertical {
        for (support, alternative, other) in &hypotheses {
            if gravity(*alternative) || *support < score || other.len() < 4 {
                continue;
            }
            if other.iter().filter(|i| inliers.contains(i)).count() * 4 > inliers.len() {
                continue;
            }
            for &i in other {
                if !inliers.contains(&i) {
                    excluded[i] = true;
                }
            }
        }
    }
    let total = candidates
        .iter()
        .filter(|&&i| !excluded[i])
        .map(|&i| segments[i].weight())
        .sum::<f64>();
    if inliers.len() < 4 || score < total * 0.48 {
        return None;
    }
    for (_, alternative, other) in hypotheses.iter().skip(1).take_while(|h| h.0 > score * 0.88) {
        if vertical && !gravity(*alternative) {
            continue;
        }
        // Distinct, comparably supported directions indicate a competing
        // façade or accidental tree structure. Shared models are harmless.
        let overlap = other.iter().filter(|i| inliers.contains(i)).count();
        if overlap * 2 < inliers.len() && dot(point, *alternative).abs() < 0.9994 {
            return None;
        }
    }
    for _ in 0..12 {
        let mut matrix = [[0.; 3]; 3];
        for &i in &inliers {
            let s = &segments[i];
            let p = s.plane();
            let m = s.midpoint();
            let d = (point[0] - m[0] * point[2]).hypot(point[1] - m[1] * point[2]);
            // Propagate TLS position uncertainty to angular uncertainty, with
            // a systematic floor for blur/correlated texture. Robust IRLS keeps
            // mildly bent boundaries and inaccurate short frames from steering
            // a precise global direction even after they pass coarse consensus.
            let pixels = s.length() * width as f64;
            let variance = 12. * s.straightness.max(0.15).powi(2) / pixels.powi(3)
                + 0.08_f64.to_radians().powi(2);
            let robust_scale = (2.5 * variance.sqrt()).max(0.15_f64.to_radians());
            let robust = 1. / (1. + (error(s, point) / robust_scale).powi(2));
            let weight = robust / variance / d.max(0.1).powi(2);
            for r in 0..3 {
                for c in 0..3 {
                    matrix[r][c] += weight * p[r] * p[c];
                }
            }
        }
        point = if let Some(constraint) = constraint {
            let normal = normalize(constraint);
            let u = normalize(cross(normal, [0., 0., 1.]));
            let v = cross(normal, u);
            let apply = |p: [f64; 3]| std::array::from_fn(|i| dot(matrix[i], p));
            let a = dot(u, apply(u));
            let b = dot(u, apply(v));
            let c = dot(v, apply(v));
            let angle = 0.5 * (2. * b).atan2(a - c);
            normalize(std::array::from_fn(|i| {
                -angle.sin() * u[i] + angle.cos() * v[i]
            }))
        } else {
            smallest_eigenvector(matrix)
        };
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
