//! One backward map: crop → pinhole homography → radial lens model → sensor orientation.
use crate::{
    color::{Matrix, apply, multiply},
    edits::GeometryEdits,
};
use anyhow::Result;

#[derive(Clone, Debug)]
pub struct Geometry {
    pub homography: Matrix,
    pub distortion: [f32; 2],
    pub ca: [f32; 2],
    pub crop: [f32; 4],
    pub aspect: f32,
    pub width: usize,
    pub height: usize,
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
            [1. / (f * e.scale), 0., -0.5 / (f * e.scale)],
            [0., aspect / (f * e.scale), -0.5 * aspect / (f * e.scale)],
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
        })
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
        let x = p[0] / p[2] - 0.5;
        let y = (p[1] / p[2] - 0.5) * self.aspect;
        let r2 = 4. * (x * x + y * y) / (1. + self.aspect * self.aspect);
        let ca = match channel {
            0 => self.ca[0],
            2 => self.ca[1],
            _ => 0.,
        };
        let radial = (1. + self.distortion[0] * r2 + self.distortion[1] * r2 * r2) * (1. + ca);
        Some([x * radial + 0.5, y * radial / self.aspect + 0.5])
    }
}
