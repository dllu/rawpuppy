//! Architectural perspective from verified straight edges and robust vanishing directions.
//!
//! Detection runs on a bounded, lens-corrected preview, before the existing
//! perspective/crop/display edits. Semantic masks select evidence; their
//! boundaries never become evidence. All fitting uses square-pixel coordinates
//! and homogeneous vanishing points, including points at infinity.
use crate::{
    color,
    edits::{Edits, GeometryEdits, Reconstruction, ToneMapper},
    input::{SensorImage, pixel_count},
    pipeline::Rendered,
    render::Renderer,
};
use anyhow::{Result, ensure};
use rayon::prelude::*;
use serde::Serialize;
use std::sync::Arc;

mod edges;
mod solve;
use edges::extract;
pub use solve::fit_segments;

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
    analysis.raw.hot_pixels = true;
    analysis.display = Default::default();
    analysis.scene = Edits::for_image(&image).scene;
    analysis.tone.mapper = ToneMapper::Linear;
    analysis.tone.saturation = 1.;
    let (width, height) = renderer.dimensions(image.clone(), &analysis, Some(1440))?;
    // Average four stratified scene-linear samples per analysis pixel. Sparse
    // single-site RAW previews alias masonry and move short edge orientations.
    // This stays bounded and never reconstructs a full-resolution intermediate.
    let mut preview = renderer.render_region(
        image.clone(),
        &analysis,
        [-0.25 / width as f32, -0.25 / height as f32, 1., 1.],
        width,
        height,
    )?;
    for [dx, dy] in [[0.25, -0.25], [-0.25, 0.25], [0.25, 0.25]] {
        let sample = renderer.render_region(
            image.clone(),
            &analysis,
            [dx / width as f32, dy / height as f32, 1., 1.],
            width,
            height,
        )?;
        preview
            .pixels
            .par_iter_mut()
            .zip(&sample.pixels)
            .for_each(|(a, b)| {
                for i in 0..4 {
                    a[i] += b[i];
                }
            });
    }
    preview.pixels.par_iter_mut().for_each(|p| {
        for v in p.iter_mut() {
            *v *= 0.25;
        }
    });
    // Photographic edit exposure/looks must not change geometric evidence.
    // Normalize a bounded linear preview before AgX: night façades otherwise
    // lose their shadow edges while bright windows dominate the detector.
    let mut histogram = [0usize; 256];
    let mut count = 0;
    for p in &preview.pixels {
        let y = color::luminance([p[0], p[1], p[2]]);
        if p[3] < 0.99 || y <= 1e-8 {
            continue;
        }
        let bin = ((y.log2() + 24.) / 40. * 256.).clamp(0., 255.) as usize;
        histogram[bin] += 1;
        count += 1;
    }
    let mut total = 0;
    let bin = histogram
        .iter()
        .position(|n| {
            total += n;
            total >= count * 9 / 10
        })
        .unwrap_or(128);
    let percentile = 2f32.powf(-24. + (bin as f32 + 0.5) * 40. / 256.);
    let gain = if count == 0 {
        1.
    } else {
        (0.35 / percentile).clamp(0.25, 32.)
    };
    preview.pixels.par_iter_mut().for_each(|p| {
        let rgb = color::agx([p[0] * gain, p[1] * gain, p[2] * gain]);
        p[..3].copy_from_slice(&rgb);
    });
    Ok(preview)
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
    let mut report = fit_segments(
        extract(image, mask),
        [image.width, image.height],
        geometry,
        mode,
    )?;
    if mask.is_some() {
        report.mask_provider = "semantic".into();
    }
    Ok(report)
}
