//! Bounded, local LaMa inference in display-referred RGB. Originals never change.
use crate::{color, models, pipeline::Rendered};
use anyhow::{Result, ensure};
use ort::{
    session::{Session, builder::GraphOptimizationLevel},
    value::Tensor,
};
use std::path::Path;

pub struct Lama {
    session: Session,
}
impl Lama {
    pub fn open(path: &Path) -> Result<Self> {
        models::verify_lama(path)?;
        let session = Session::builder()?
            .with_intra_threads(8)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .with_inter_threads(1)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .commit_from_file(path)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Self { session })
    }
    /// Fixed 512×512 model contract; image samples are display-linear sRGB.
    pub fn inpaint_512(&mut self, image: &Rendered, mask: &[f32]) -> Result<Rendered> {
        const N: usize = 512 * 512;
        ensure!(
            image.width == 512 && image.height == 512 && image.pixels.len() == N && mask.len() == N,
            "LaMa expects a 512×512 image and matching mask"
        );
        ensure!(
            mask.iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "Inpainting mask must be finite and in [0,1]"
        );
        ensure!(
            image.pixels.iter().flatten().all(|v| v.is_finite()),
            "Inpainting input must be finite"
        );
        let mut chw = vec![0f32; N * 3];
        for (i, p) in image.pixels.iter().enumerate() {
            for c in 0..3 {
                chw[c * N + i] = color::srgb_encode(p[c]).clamp(0., 1.);
            }
        }
        let image_tensor = Tensor::from_array(([1usize, 3, 512, 512], chw))?;
        let binary: Vec<f32> = mask.iter().map(|v| if *v > 0. { 1. } else { 0. }).collect();
        let mask_tensor = Tensor::from_array(([1usize, 1, 512, 512], binary))?;
        let outputs = self
            .session
            .run(ort::inputs!["image"=>image_tensor,"mask"=>mask_tensor])?;
        let (shape, output) = outputs["output"].try_extract_tensor::<f32>()?;
        ensure!(
            shape.as_ref() == [1, 3, 512, 512] && output.len() == N * 3,
            "Unexpected LaMa output dimensions"
        );
        ensure!(
            output.iter().all(|v| v.is_finite()),
            "LaMa produced nonfinite output"
        );
        let pixels = image
            .pixels
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let mut result = *p;
                if mask[i] > 0. {
                    for c in 0..3 {
                        let generated =
                            color::srgb_decode((output[c * N + i] / 255.).clamp(0., 1.));
                        result[c] += mask[i] * (generated - result[c]);
                    }
                    result[3] += mask[i] * (1. - result[3]);
                }
                result
            })
            .collect();
        Ok(Rendered {
            width: 512,
            height: 512,
            pixels,
        })
    }

    /// Infer only tiles touching the mask. Read a stable image until all patches are ready.
    /// A 64-pixel context surrounds each 384-pixel output tile; edge context is replicated.
    pub fn inpaint(&mut self, image: &mut Rendered, mask: &[f32]) -> Result<usize> {
        ensure!(
            mask.len() == image.pixels.len(),
            "Mask dimensions disagree with rendered image"
        );
        ensure!(
            image.pixels.len() == crate::input::pixel_count(image.width, image.height, 1)?,
            "Invalid image dimensions"
        );
        ensure!(
            mask.iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "Mask must be finite and in [0,1]"
        );
        let (w, h) = (image.width, image.height);
        if w <= 512 && h <= 512 {
            if !mask.iter().any(|v| *v > 0.) {
                return Ok(0);
            }
            let mut pixels = Vec::with_capacity(512 * 512);
            let mut padded_mask = Vec::with_capacity(512 * 512);
            for y in 0..512 {
                for x in 0..512 {
                    pixels.push(image.pixels[y.min(h - 1) * w + x.min(w - 1)]);
                    padded_mask.push(if x < w && y < h { mask[y * w + x] } else { 0. });
                }
            }
            let generated = self.inpaint_512(
                &Rendered {
                    width: 512,
                    height: 512,
                    pixels,
                },
                &padded_mask,
            )?;
            for y in 0..h {
                image.pixels[y * w..(y + 1) * w]
                    .copy_from_slice(&generated.pixels[y * 512..y * 512 + w]);
            }
            return Ok(1);
        }
        let mut patches = Vec::new();
        for y in (0..h).step_by(384) {
            for x in (0..w).step_by(384) {
                let tw = 384.min(w - x);
                let th = 384.min(h - y);
                if !(0..th).any(|dy| {
                    mask[(y + dy) * w + x..(y + dy) * w + x + tw]
                        .iter()
                        .any(|v| *v > 0.)
                }) {
                    continue;
                }
                let mut pixels = Vec::with_capacity(512 * 512);
                let mut tile_mask = Vec::with_capacity(512 * 512);
                for dy in 0..512 {
                    for dx in 0..512 {
                        let sx = (x as i128 + dx as i128 - 64).clamp(0, w as i128 - 1) as usize;
                        let sy = (y as i128 + dy as i128 - 64).clamp(0, h as i128 - 1) as usize;
                        pixels.push(image.pixels[sy * w + sx]);
                        tile_mask.push(mask[sy * w + sx]);
                    }
                }
                let tile = Rendered {
                    width: 512,
                    height: 512,
                    pixels,
                };
                let generated = self.inpaint_512(&tile, &tile_mask)?;
                let mut patch = Vec::with_capacity(tw * th);
                for dy in 0..th {
                    patch.extend_from_slice(
                        &generated.pixels[(dy + 64) * 512 + 64..(dy + 64) * 512 + 64 + tw],
                    );
                }
                patches.push((x, y, tw, th, patch));
            }
        }
        let count = patches.len();
        for (x, y, tw, th, patch) in patches {
            for dy in 0..th {
                image.pixels[(y + dy) * w + x..(y + dy) * w + x + tw]
                    .copy_from_slice(&patch[dy * tw..(dy + 1) * tw]);
            }
        }
        Ok(count)
    }
}
