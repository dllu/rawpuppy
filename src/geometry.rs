//! One backward map: crop → pinhole homography → radial lens model → sensor orientation.
use crate::{
    color::{Matrix, apply, inverse, multiply},
    edits::{Edits, GeometryEdits},
    input::SensorImage,
};
use anyhow::{Result, ensure};

pub const FULL_FRAME: [f32; 4] = [0., 0., 1., 1.];

/// Express a normalized subrectangle in its parent rectangle's coordinates.
pub(crate) fn subrect(parent: [f32; 4], child: [f32; 4]) -> [f32; 4] {
    [
        parent[0] + child[0] * parent[2],
        parent[1] + child[1] * parent[3],
        child[2] * parent[2],
        child[3] * parent[3],
    ]
}

pub fn wrap_rotation(degrees: f32) -> f32 {
    (degrees + 180.).rem_euclid(360.) - 180.
}

/// Residual roll from a line in corrected-canvas coordinates. Reversing its
/// endpoints gives the same correction; choose the nearest horizontal/vertical.
pub fn level_angle(start: [f32; 2], end: [f32; 2], aspect: f32) -> Option<f32> {
    let dx = end[0] - start[0];
    let dy = (end[1] - start[1]) * aspect;
    if !dx.is_finite() || !dy.is_finite() || dx.hypot(dy) < 1e-6 {
        return None;
    }
    Some((dy.atan2(dx).to_degrees() + 45.).rem_euclid(90.) - 45.)
}

#[derive(Clone, Debug)]
pub struct Geometry {
    pub homography: Matrix,
    pub distortion: [f32; 2],
    pub ca: [f32; 2],
    pub crop: [f32; 4],
    pub aspect: f32,
    pub width: usize,
    pub height: usize,
    pub lens: crate::lens::Correction,
}

impl Geometry {
    pub fn compile(e: &GeometryEdits, width: usize, height: usize) -> Result<Self> {
        let (sy, cy) = e.yaw.to_radians().sin_cos();
        let (sp, cp) = e.pitch.to_radians().sin_cos();
        let (sr, cr) = e.rotation.to_radians().sin_cos();
        let yaw = [[cy, 0., sy], [0., 1., 0.], [-sy, 0., cy]];
        let pitch = [[1., 0., 0.], [0., cp, -sp], [0., sp, cp]];
        let roll = [[cr, -sr, 0.], [sr, cr, 0.], [0., 0., 1.]];
        let f = 0.5 / (e.field_of_view.to_radians() * 0.5).tan();
        let aspect = height as f32 / width as f32;
        let kinv = [
            [1. / (f * e.scale), 0., (-0.5 + e.shift[0]) / (f * e.scale)],
            [
                0.,
                aspect / (f * e.scale),
                (-0.5 + e.shift[1]) * aspect / (f * e.scale),
            ],
            [0., 0., 1.],
        ];
        let k = [[f, 0., 0.5], [0., f / aspect, 0.5], [0., 0., 1.]];
        Ok(Self {
            homography: multiply(k, multiply(multiply(yaw, multiply(pitch, roll)), kinv)),
            distortion: e.distortion,
            ca: e.chromatic_aberration,
            crop: e.crop,
            aspect,
            width: ((width as f64 * e.crop[2] as f64).round() as usize).max(1),
            height: ((height as f64 * e.crop[3] as f64).round() as usize).max(1),
            lens: crate::lens::Correction::default(),
        })
    }

    pub(crate) fn for_image(image: &SensorImage, edits: &Edits) -> Result<Self> {
        let meta = &image.metadata;
        let mut geometry = Self::compile(&edits.geometry, meta.width, meta.height)?;
        geometry.lens = crate::lens::Correction::compile(
            meta.lens_profile.as_ref(),
            &edits.lens,
            meta.width,
            meta.height,
        )?;
        Ok(geometry)
    }

    /// Bounds of the sensor footprint in the fixed, uncropped output coordinate system.
    pub fn corrected_bounds(&self) -> Result<[f32; 4]> {
        let forward = inverse(self.homography)?;
        let corners = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
        let radial = self.lens.distortion
            || self.lens.chromatic_aberration
            || self.distortion != [0.; 2]
            || self.ca != [0.; 2];
        // A homography maps an undistorted edge to a straight line. Lens
        // correction curves it, so trace the perimeter before projecting it.
        let steps = if radial { 128 } else { 1 };
        let mut perimeter = Vec::with_capacity(4 * steps);
        for (a, b) in corners.into_iter().zip(corners.into_iter().cycle().skip(1)) {
            for i in 0..steps {
                let t = i as f32 / steps as f32;
                let point = std::array::from_fn(|j| a[j] + t * (b[j] - a[j]));
                let point = if radial {
                    self.lens_boundary(point)
                } else {
                    point
                };
                perimeter.push(apply(forward, [point[0], point[1], 1.]));
            }
        }
        let center_depth = apply(forward, [0.5, 0.5, 1.])[2];
        ensure!(
            center_depth > 0.,
            "Corrected canvas faces away from the sensor"
        );
        // There is no finite extent when the optical horizon crosses the
        // footprint. Clip that case to an eightfold perspective magnification.
        let near = if perimeter.iter().any(|p| p[2] <= center_depth * 1e-6) {
            center_depth / 8.
        } else {
            0.
        };
        let mut min = [f32::INFINITY; 2];
        let mut max = [f32::NEG_INFINITY; 2];
        let mut include = |point: [f32; 3]| {
            for j in 0..2 {
                let value = point[j] / point[2];
                min[j] = min[j].min(value);
                max[j] = max[j].max(value);
            }
        };
        let mut previous = *perimeter.last().unwrap();
        for next in perimeter {
            let visible = next[2] > near;
            if visible != (previous[2] > near) {
                let t = (near - previous[2]) / (next[2] - previous[2]);
                include([
                    previous[0] + t * (next[0] - previous[0]),
                    previous[1] + t * (next[1] - previous[1]),
                    near,
                ]);
            }
            if visible {
                include(next);
            }
            previous = next;
        }
        let snap = |v: f32| {
            let edge = v.round();
            if (v - edge).abs() <= 8. * f32::EPSILON && (0. ..=1.).contains(&edge) {
                edge
            } else {
                v
            }
        };
        let [x, y] = min.map(snap);
        let [right, bottom] = max.map(snap);
        let extent = [x, y, right - x, bottom - y];
        ensure!(
            extent.iter().all(|v| v.is_finite()) && extent[2] > 0. && extent[3] > 0.,
            "No finite corrected canvas"
        );
        Ok(extent)
    }

    fn lens_boundary(&self, edge: [f32; 2]) -> [f32; 2] {
        // Invert the center-connected radial map along this sensor boundary ray.
        // A pixel is available only when all three channel samples are inside.
        let r2 = self.radius2(edge);
        let inside = |t: f32| {
            (0..3).all(|c| {
                let radius = t * self.radial_scale(r2 * t * t, c);
                radius > 0. && radius <= 1.
            })
        };
        let mut low = 0.;
        let mut high = 1. / 16.;
        while inside(high) {
            low = high;
            if high >= 2. {
                high *= 2.;
            } else {
                high += 1. / 16.;
            }
        }
        for _ in 0..16 {
            let middle = (low + high) * 0.5;
            if inside(middle) {
                low = middle;
            } else {
                high = middle;
            }
        }
        edge.map(|v| 0.5 + (v - 0.5) * low)
    }

    fn radius2(&self, point: [f32; 2]) -> f32 {
        let x = point[0] - 0.5;
        let y = (point[1] - 0.5) * self.aspect;
        4. * (x * x + y * y) / (1. + self.aspect * self.aspect)
    }

    fn radial_scale(&self, r2: f32, channel: usize) -> f32 {
        let ca = match channel {
            0 => self.ca[0],
            2 => self.ca[1],
            _ => 0.,
        };
        let camera = if self.lens.distortion || self.lens.chromatic_aberration {
            let component = if self.lens.chromatic_aberration {
                match channel {
                    0 => 2,
                    2 => 3,
                    _ => 0,
                }
            } else {
                0
            };
            self.lens.lookup(r2, component)
        } else {
            1.
        };
        (1. + self.distortion[0] * r2 + self.distortion[1] * r2 * r2) * (1. + ca) * camera
    }

    /// Normalized output sample, normalized oriented input result. No raster intermediates.
    pub fn map(&self, uv: [f32; 2], channel: usize) -> Option<[f32; 2]> {
        let uv = [
            self.crop[0] + uv[0] * self.crop[2],
            self.crop[1] + uv[1] * self.crop[3],
            1.,
        ];
        let p = apply(self.homography, uv);
        if p[2] <= 1e-6 {
            return None;
        }
        let point = [p[0] / p[2], p[1] / p[2]];
        let x = point[0] - 0.5;
        let radial = self.radial_scale(self.radius2(point), channel);
        // Apply the radial displacement to the original normalized point.
        // Centering and uncentering otherwise loses fractional pixels near
        // image borders, especially for very wide images and tiny corrections.
        let displacement = radial - 1.;
        Some([
            point[0] + x * displacement,
            point[1] + (point[1] - 0.5) * displacement,
        ])
    }
}
