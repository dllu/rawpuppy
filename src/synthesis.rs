//! Cached generated layers, recipe identity, context crops, and exact mask composition.
use crate::{edits::Edits, input::pixel_count, models, pipeline::Rendered};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MaskDab {
    pub center: [f32; 2],
    pub radius: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GeneratedFill {
    pub region: [f32; 4],
    pub dabs: Vec<MaskDab>,
    pub fill_gaps: bool,
    pub steps: usize,
    pub seed: i64,
    pub asset: String,
    pub sha256: String,
    pub source_sha256: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub source_color_revision: u32,
    pub recipe_sha256: String,
    pub model: String,
}

pub fn asset_directory(original: &Path) -> PathBuf {
    let mut path = original.as_os_str().to_os_string();
    path.push(".rawpuppy-assets");
    PathBuf::from(path)
}
pub fn recipe_hash(edits: &Edits) -> Result<String> {
    let mut base = edits.clone();
    base.display.synthesis.clear();
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&base)?)))
}
pub fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn is_zero(value: &u32) -> bool {
    *value == 0
}

impl GeneratedFill {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.source_color_revision <= 1,
            "Unsupported source color decoding revision"
        );
        ensure!(
            self.region.iter().all(|v| v.is_finite()) && self.region[2] > 0. && self.region[3] > 0.,
            "Invalid synthesis region"
        );
        ensure!(
            valid_hash(&self.sha256)
                && valid_hash(&self.source_sha256)
                && valid_hash(&self.recipe_sha256),
            "Invalid synthesis identity"
        );
        ensure!(
            self.asset == format!("{}.exr", self.sha256),
            "Synthesis assets must use their content identity"
        );
        ensure!(
            self.model == "moebius-scene-2026-v1",
            "Unsupported synthesis model version"
        );
        ensure!(
            (2..=1000).contains(&self.steps),
            "Invalid synthesis sampling steps"
        );
        for dab in &self.dabs {
            ensure!(
                dab.center.iter().all(|v| v.is_finite())
                    && dab.radius.is_finite()
                    && dab.radius > 0.,
                "Invalid synthesis mask"
            );
        }
        Ok(())
    }
}

pub struct Layers {
    original: PathBuf,
    source_hash: Option<String>,
    loaded: HashMap<String, Arc<Rendered>>,
}
impl Layers {
    pub fn new(original: PathBuf) -> Self {
        Self {
            original,
            source_hash: None,
            loaded: HashMap::new(),
        }
    }
    pub fn source_hash(&mut self) -> Result<&str> {
        if self.source_hash.is_none() {
            self.source_hash = Some(models::sha256(&self.original)?);
        }
        Ok(self.source_hash.as_deref().unwrap())
    }
    pub fn store(&mut self, image: &Rendered) -> Result<(String, String)> {
        let directory = asset_directory(&self.original);
        std::fs::create_dir_all(&directory)?;
        let temporary = tempfile::Builder::new()
            .suffix(".exr")
            .tempfile_in(&directory)?
            .into_temp_path();
        // Export atomically replaces this staging path. Close its initial file
        // handle first: Windows cannot replace an open destination file.
        crate::export::write(
            &temporary,
            image,
            crate::color::OutputSpace::LinearSrgb,
            true,
        )?;
        let hash = models::sha256(&temporary)?;
        let name = format!("{hash}.exr");
        let path = directory.join(&name);
        if path.is_file() {
            ensure!(
                models::sha256(&path)? == hash,
                "Existing synthesis asset is corrupt"
            );
        } else {
            temporary.persist_noclobber(path)?;
        }
        self.cache(
            hash.clone(),
            Arc::new(Rendered {
                width: image.width,
                height: image.height,
                pixels: image.pixels.clone(),
            }),
        );
        Ok((name, hash))
    }
    fn cache(&mut self, hash: String, image: Arc<Rendered>) {
        if self.loaded.len() >= 16 && !self.loaded.contains_key(&hash) {
            self.loaded.clear();
        }
        self.loaded.insert(hash, image);
    }
    fn load(&mut self, fill: &GeneratedFill) -> Result<Arc<Rendered>> {
        if let Some(image) = self.loaded.get(&fill.sha256) {
            return Ok(image.clone());
        }
        let path = asset_directory(&self.original).join(&fill.asset);
        ensure!(
            models::sha256(&path).context("Reading saved generated pixels")? == fill.sha256,
            "Synthesis asset checksum mismatch"
        );
        let image = image::ImageReader::open(path)?.decode()?.to_rgba32f();
        let (width, height) = (image.width() as usize, image.height() as usize);
        let pixels = image.into_raw().as_chunks::<4>().0.to_vec();
        ensure!(
            pixels.len() == pixel_count(width, height, 1)?
                && pixels.iter().flatten().all(|v| v.is_finite()),
            "Invalid synthesis asset samples"
        );
        let image = Arc::new(Rendered {
            width,
            height,
            pixels,
        });
        self.cache(fill.sha256.clone(), image.clone());
        Ok(image)
    }
    /// Resolve all visible immutable layer snapshots before changing any output pixel.
    pub fn apply(&mut self, edits: &Edits, image: &mut Rendered, viewport: [f32; 4]) -> Result<()> {
        if edits.display.synthesis.is_empty() {
            return Ok(());
        }
        let recipe = recipe_hash(edits)?;
        let source = self.source_hash()?.to_owned();
        let color_revision = crate::input::color_revision(&self.original)?;
        for fill in &edits.display.synthesis {
            fill.validate()?;
            ensure!(
                fill.source_sha256 == source,
                "Generated fill belongs to a different original"
            );
            ensure!(
                fill.source_color_revision == color_revision,
                "Generated fills need regeneration after updated HDR color decoding"
            );
            ensure!(
                fill.recipe_sha256 == recipe,
                "Generated fills need regeneration after changing the preceding edits"
            );
        }
        let mut prepared = Vec::new();
        prepared.try_reserve(edits.display.synthesis.len())?;
        for fill in &edits.display.synthesis {
            let x0 = ((fill.region[0] - viewport[0]) / viewport[2] * image.width as f32)
                .floor()
                .clamp(0., image.width as f32) as usize;
            let y0 = ((fill.region[1] - viewport[1]) / viewport[3] * image.height as f32)
                .floor()
                .clamp(0., image.height as f32) as usize;
            let x1 = ((fill.region[0] + fill.region[2] - viewport[0]) / viewport[2]
                * image.width as f32)
                .ceil()
                .clamp(0., image.width as f32) as usize;
            let y1 = ((fill.region[1] + fill.region[3] - viewport[1]) / viewport[3]
                * image.height as f32)
                .ceil()
                .clamp(0., image.height as f32) as usize;
            if x0 >= x1 || y0 >= y1 {
                continue;
            }
            let layer = self.load(fill)?;
            prepared.push((fill, [x0, y0, x1, y1], layer));
        }
        // No fallible asset operation remains once compositing begins. Only
        // visible context snapshots are retained, never a rollback photo raster.
        for (fill, [x0, y0, x1, y1], layer) in prepared {
            let aspect = (image.height as f32 / viewport[3]) / (image.width as f32 / viewport[2]);
            for y in y0..y1 {
                for x in x0..x1 {
                    let uv = [
                        viewport[0] + viewport[2] * (x as f32 + 0.5) / image.width as f32,
                        viewport[1] + viewport[3] * (y as f32 + 0.5) / image.height as f32,
                    ];
                    let u = (uv[0] - fill.region[0]) / fill.region[2];
                    let v = (uv[1] - fill.region[1]) / fill.region[3];
                    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                        continue;
                    }
                    let sample = sample_layer(&layer, u, v);
                    let alpha = sample[3].clamp(0., 1.);
                    if alpha == 0. {
                        continue;
                    }
                    let pixel = &mut image.pixels[y * image.width + x];
                    let painted = fill.dabs.iter().any(|dab| {
                        let dx = uv[0] - dab.center[0];
                        let dy = (uv[1] - dab.center[1]) * aspect;
                        dx * dx + dy * dy <= dab.radius * dab.radius
                    });
                    if !(painted || fill.fill_gaps && pixel[3] < 1.) {
                        continue;
                    }
                    for c in 0..3 {
                        pixel[c] += alpha * (sample[c] - pixel[c]);
                    }
                    pixel[3] += alpha * (1. - pixel[3]);
                }
            }
        }
        Ok(())
    }
}

fn sample_layer(image: &Rendered, u: f32, v: f32) -> [f32; 4] {
    let x = (u * image.width as f32 - 0.5).clamp(0., (image.width - 1) as f32);
    let y = (v * image.height as f32 - 0.5).clamp(0., (image.height - 1) as f32);
    let ix = x.floor() as usize;
    let iy = y.floor() as usize;
    let tx = x - ix as f32;
    let ty = y - iy as f32;
    let a = image.pixels[iy * image.width + ix];
    let b = image.pixels[iy * image.width + (ix + 1).min(image.width - 1)];
    let c = image.pixels[(iy + 1).min(image.height - 1) * image.width + ix];
    let d =
        image.pixels[(iy + 1).min(image.height - 1) * image.width + (ix + 1).min(image.width - 1)];
    // Premultiply while filtering so transparent context cannot create dark/color fringes.
    let weights = [
        (1. - tx) * (1. - ty),
        tx * (1. - ty),
        (1. - tx) * ty,
        tx * ty,
    ];
    let mut out = [0.; 4];
    for (p, w) in [a, b, c, d].into_iter().zip(weights) {
        for i in 0..3 {
            out[i] += p[i] * p[3] * w;
        }
        out[3] += p[3] * w;
    }
    if out[3] > 0. {
        let alpha = out[3];
        for v in &mut out[..3] {
            *v /= alpha;
        }
    }
    out
}

/// Square in image pixels, with context around the brush bounds and no aspect stretching.
pub fn brush_context(dabs: &[MaskDab], width: usize, height: usize) -> Result<[f32; 4]> {
    ensure!(!dabs.is_empty(), "Paint an inpainting mask first");
    let w = width as f64;
    let h = height as f64;
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for dab in dabs {
        let r = dab.radius as f64 * w;
        for i in 0..2 {
            let p = dab.center[i] as f64 * if i == 0 { w } else { h };
            min[i] = min[i].min(p - r);
            max[i] = max[i].max(p + r);
        }
    }
    let side = ((max[0] - min[0]).max(max[1] - min[1]) * 1.6).max(128.);
    Ok([
        ((min[0] + max[0] - side) * 0.5 / w) as f32,
        ((min[1] + max[1] - side) * 0.5 / h) as f32,
        (side / w) as f32,
        (side / h) as f32,
    ])
}

pub fn context_mask(
    image: &Rendered,
    region: [f32; 4],
    dabs: &[MaskDab],
    fill_gaps: bool,
    aspect: f32,
) -> Vec<f32> {
    image
        .pixels
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let uv = [
                region[0] + region[2] * ((i % image.width) as f32 + 0.5) / image.width as f32,
                region[1] + region[3] * ((i / image.width) as f32 + 0.5) / image.height as f32,
            ];
            let painted = dabs.iter().any(|dab| {
                let dx = uv[0] - dab.center[0];
                let dy = (uv[1] - dab.center[1]) * aspect;
                dx * dx + dy * dy <= dab.radius * dab.radius
            });
            if painted || (fill_gaps && p[3] < 1.) {
                1.
            } else {
                0.
            }
        })
        .collect()
}

/// Inspect each corner independently and add square, physical-pixel context.
pub fn gap_contexts(probe: &Rendered, width: usize, height: usize) -> Vec<[f32; 4]> {
    let mut regions = Vec::new();
    for qy in 0..2 {
        for qx in 0..2 {
            let mut bounds: Option<[usize; 4]> = None;
            for y in qy * probe.height / 2..(qy + 1) * probe.height / 2 {
                for x in qx * probe.width / 2..(qx + 1) * probe.width / 2 {
                    if probe.pixels[y * probe.width + x][3] >= 1. {
                        continue;
                    }
                    if let Some(b) = &mut bounds {
                        b[0] = b[0].min(x);
                        b[1] = b[1].min(y);
                        b[2] = b[2].max(x + 1);
                        b[3] = b[3].max(y + 1);
                    } else {
                        bounds = Some([x, y, x + 1, y + 1]);
                    }
                }
            }
            if let Some(b) = bounds {
                let min = [
                    b[0] as f64 / probe.width as f64 * width as f64,
                    b[1] as f64 / probe.height as f64 * height as f64,
                ];
                let max = [
                    b[2] as f64 / probe.width as f64 * width as f64,
                    b[3] as f64 / probe.height as f64 * height as f64,
                ];
                let side = ((max[0] - min[0]).max(max[1] - min[1]) * 1.6).max(128.);
                let mut region = [
                    ((min[0] + max[0] - side) * 0.5 / width as f64) as f32,
                    ((min[1] + max[1] - side) * 0.5 / height as f64) as f32,
                    (side / width as f64) as f32,
                    (side / height as f64) as f32,
                ];
                // Move context into the corrected canvas where it fits. Centering
                // it on a corner needlessly masked most of the input with empty
                // space outside the canvas, starving outpainting of known pixels.
                for axis in 0..2 {
                    if region[axis + 2] <= 1. {
                        region[axis] = region[axis].clamp(0., 1. - region[axis + 2]);
                    }
                }
                regions.push(region);
            }
        }
    }
    regions
}
