//! Optional joint Bayer reconstruction over bounded, immutable sensor contexts.
use crate::{
    edits::{RawEdits, Reconstruction},
    input::{SensorImage, pixel_count},
    ml_runtime::InferenceDevice,
    models,
};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::path::Path;
use tch::{CModule, Device, Kind, Tensor};

pub const WEIGHT_SHA256: &str = "c1f7de3de24cb12f4803ee65e11cfb467e9a5b80c3cb7126746ef026106c15e0";

#[derive(Deserialize)]
struct Manifest {
    version: u32,
    allow_tf32: bool,
    deterministic_convolutions: bool,
    weights_sha256: String,
    graph_sha256: String,
    packed_dimension_multiple: usize,
    checks: Vec<Check>,
}
#[derive(Deserialize)]
struct Check {
    max_absolute_error: f64,
}

pub struct BayerModel {
    module: CModule,
    device: Device,
}

pub struct CameraRgbPatch {
    pub origin: [usize; 2],
    pub size: [usize; 2],
    /// Camera-native linear RGB, before white balance or any color calibration.
    pub pixels: Vec<[f32; 3]>,
    pub gain: f32,
    pub context_origin: [isize; 2],
    pub context_size: [usize; 2],
}

impl BayerModel {
    pub fn open(directory: &Path, preference: InferenceDevice) -> Result<Self> {
        let manifest: Manifest = serde_json::from_slice(
            &std::fs::read(directory.join("manifest.json"))
                .context("Prepare RawNIND parameters with tools/export_rawnind.py")?,
        )?;
        ensure!(
            manifest.version == 2
                && !manifest.allow_tf32
                && manifest.deterministic_convolutions
                && manifest.weights_sha256 == WEIGHT_SHA256
                && manifest.packed_dimension_multiple == 16
                && manifest.checks.len() >= 4
                && manifest
                    .checks
                    .iter()
                    .all(|c| c.max_absolute_error.is_finite() && c.max_absolute_error <= 0.0001),
            "Unverified RawNIND graph manifest"
        );
        let path = directory.join("bayer.pt");
        ensure!(
            models::sha256(&path)? == manifest.graph_sha256,
            "RawNIND graph checksum mismatch"
        );
        let device = preference.resolve()?;
        let mut module = CModule::load_on_device(path, device)?;
        module.set_eval();
        Ok(Self { module, device })
    }

    pub fn device(&self) -> Device {
        self.device
    }

    pub fn reconstruct_patch(
        &self,
        source: &SensorImage,
        origin: [usize; 2],
        size: [usize; 2],
    ) -> Result<CameraRgbPatch> {
        self.reconstruct_region(source, origin, size, false, true)
    }

    fn reconstruct_region(
        &self,
        source: &SensorImage,
        origin: [usize; 2],
        size: [usize; 2],
        hot_pixels: bool,
        match_local_gain: bool,
    ) -> Result<CameraRgbPatch> {
        let count = pixel_count(size[0], size[1], 1)?;
        ensure!(
            source.cpp == 1,
            "Joint model requires a Bayer sensor source"
        );
        ensure!(
            source.metadata.sensor_width >= 2
                && source.metadata.sensor_height >= 2
                && source.data.len()
                    == pixel_count(
                        source.metadata.sensor_width,
                        source.metadata.sensor_height,
                        1
                    )?,
            "Joint model requires a complete, consistent Bayer sensor allocation"
        );
        let cfa = source
            .cfa
            .as_ref()
            .filter(|c| c.width == 2 && c.height == 2)
            .context("Joint model requires an RGB Bayer array")?;
        ensure!(
            origin[0]
                .checked_add(size[0])
                .is_some_and(|x| x <= source.metadata.sensor_width)
                && origin[1]
                    .checked_add(size[1])
                    .is_some_and(|y| y <= source.metadata.sensor_height),
            "Requested region is outside the sensor"
        );
        let red = (0..2)
            .flat_map(|y| (0..2).map(move |x| [x, y]))
            .find(|p| cfa.color_at(p[1], p[0]) == 0)
            .context("Missing red CFA sample")?;
        ensure!(
            cfa.color_at(red[1], red[0] + 1) == 1
                && cfa.color_at(red[1] + 1, red[0]) == 1
                && cfa.color_at(red[1] + 1, red[0] + 1) == 2,
            "Unsupported Bayer layout"
        );
        // Keep pooling anchored to the sensor lattice, rather than moving it
        // with every ROI. 256 raw pixels cover this graph's receptive-field
        // radius; reflection supplies context beyond physical sensor edges.
        let left = context_start(origin[0], red[0])?;
        let top = context_start(origin[1], red[1])?;
        let width = context_extent(origin[0], size[0], left)?;
        let height = context_extent(origin[1], size[1], top)?;
        let packed_count = pixel_count(width / 2, height / 2, 4)?;
        let plane = packed_count / 4;
        let inside_active = origin[0] >= source.origin[0]
            && origin[1] >= source.origin[1]
            && origin[0] + size[0] <= source.origin[0] + source.active[0]
            && origin[1] + size[1] <= source.origin[1] + source.active[1];
        let cleanup = RawEdits {
            hot_pixels,
            ..Default::default()
        };
        let mut packed = Vec::new();
        packed.try_reserve_exact(packed_count)?;
        packed.resize(packed_count, 0.);
        for py in 0..height / 2 {
            for px in 0..width / 2 {
                for c in 0..4 {
                    let x = left + (2 * px + c % 2) as isize;
                    let y = top + (2 * py + c / 2) as isize;
                    let (x, y) = if inside_active {
                        (
                            source.origin[0] as isize
                                + SensorImage::reflect(
                                    x - source.origin[0] as isize,
                                    source.active[0],
                                ) as isize,
                            source.origin[1] as isize
                                + SensorImage::reflect(
                                    y - source.origin[1] as isize,
                                    source.active[1],
                                ) as isize,
                        )
                    } else {
                        (x, y)
                    };
                    let value = if hot_pixels {
                        source.clean_raw(x, y, &cleanup)
                    } else {
                        source.raw_at(x, y, 0)
                    };
                    ensure!(value.is_finite(), "Nonfinite sensor input");
                    packed[c * plane + py * (width / 2) + px] = value;
                }
            }
        }
        let _no_grad = tch::no_grad_guard();
        tch::jit::f_set_graph_executor_optimize(false)?;
        let input = Tensor::f_from_slice(&packed)?
            .f_reshape([1, 4, (height / 2) as i64, (width / 2) as i64])?
            .f_to_device(self.device)?;
        let output = self.module.forward_ts(&[input])?;
        ensure!(
            output.size() == [1, 3, height as i64, width as i64],
            "Unexpected reconstruction graph output"
        );
        let output = output
            .f_to_device(Device::Cpu)?
            .f_to_kind(Kind::Float)?
            .f_contiguous()?;
        let values_count = pixel_count(width, height, 3)?;
        let mut values = Vec::new();
        values.try_reserve_exact(values_count)?;
        values.resize(values_count, 0.);
        output.f_copy_data(&mut values, values_count)?;
        let offset = [origin[0] as isize - left, origin[1] as isize - top];
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(count)?;
        let mut input_sum = 0f64;
        let mut generated_sum = 0f64;
        for y in 0..size[1] {
            for x in 0..size[0] {
                let index = (offset[1] as usize + y) * width + offset[0] as usize + x;
                let pixel: [f32; 3] = std::array::from_fn(|c| values[c * width * height + index]);
                ensure!(
                    pixel.iter().all(|v| v.is_finite()),
                    "Nonfinite reconstruction output"
                );
                let channel = cfa.color_at(origin[1] + y, origin[0] + x);
                input_sum += if hot_pixels {
                    source.clean_raw((origin[0] + x) as isize, (origin[1] + y) as isize, &cleanup)
                } else {
                    source.data[(origin[1] + y) * source.metadata.sensor_width + origin[0] + x]
                } as f64;
                generated_sum += pixel[channel] as f64;
                pixels.push(pixel);
            }
        }
        // This checkpoint learns an arbitrary output scale. Match only the
        // actually observed CFA colors; averaging RGB equally would misweight
        // the sensor's two green samples and could alter exposure/color.
        ensure!(
            generated_sum.abs() > 1e-12 || input_sum.abs() <= 1e-12,
            "Cannot match learned output to sensor photometry"
        );
        let gain = if input_sum.abs() <= 1e-12 {
            0.
        } else {
            (input_sum / generated_sum) as f32
        };
        ensure!(gain.is_finite(), "Invalid learned exposure gain");
        if match_local_gain {
            for pixel in &mut pixels {
                for component in pixel {
                    *component *= gain;
                    ensure!(
                        component.is_finite(),
                        "Reconstruction exceeds finite float range"
                    );
                }
            }
        }
        Ok(CameraRgbPatch {
            origin,
            size,
            pixels,
            gain,
            context_origin: [left, top],
            context_size: [width, height],
        })
    }

    /// Reconstruct once in bounded tiles, using one image-wide exposure gain.
    /// The result remains camera-native RGB, compatible with composed sampling.
    pub fn reconstruct_image(
        &self,
        source: &SensorImage,
        hot_pixels: bool,
        tile_edge: usize,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<SensorImage> {
        self.reconstruct_image_controlled(source, hot_pixels, tile_edge, |done, total| {
            progress(done, total);
            true
        })
    }

    pub fn reconstruct_image_controlled(
        &self,
        source: &SensorImage,
        hot_pixels: bool,
        tile_edge: usize,
        mut control: impl FnMut(usize, usize) -> bool,
    ) -> Result<SensorImage> {
        ensure!(
            tile_edge > 0 && source.active.iter().all(|v| *v >= 2),
            "Invalid reconstruction tile or active image dimensions"
        );
        ensure!(
            source.cpp == 1
                && source
                    .cfa
                    .as_ref()
                    .is_some_and(|c| c.width == 2 && c.height == 2),
            "Joint reconstruction requires RGB Bayer RAW"
        );
        ensure!(
            source.origin[0]
                .checked_add(source.active[0])
                .is_some_and(|v| v <= source.metadata.sensor_width)
                && source.origin[1]
                    .checked_add(source.active[1])
                    .is_some_and(|v| v <= source.metadata.sensor_height)
                && source.data.len()
                    == pixel_count(
                        source.metadata.sensor_width,
                        source.metadata.sensor_height,
                        1
                    )?,
            "Invalid active sensor area or allocation"
        );
        let width = source.metadata.sensor_width;
        let height = source.metadata.sensor_height;
        let rgb_count = pixel_count(width, height, 3)?;
        let mask_words = if source.raw_integer {
            pixel_count(width, height, 1)?.div_ceil(4)
        } else {
            0
        };
        let count = rgb_count
            .checked_add(mask_words)
            .context("Clipping provenance allocation overflow")?;
        let columns = source.active[0].div_ceil(tile_edge);
        let rows = source.active[1].div_ceil(tile_edge);
        let total = columns.checked_mul(rows).context("Tile count overflow")?;
        ensure!(control(0, total), "Reconstruction cancelled");
        let mut data = Vec::new();
        data.try_reserve_exact(count).with_context(|| {
            format!(
                "Cannot allocate {} bytes for the joint camera-RGB cache",
                count.saturating_mul(std::mem::size_of::<f32>())
            )
        })?;
        #[cfg(feature = "cuda")]
        crate::gpu::advise_sensor_allocation(data.spare_capacity_mut());
        data.resize(count, 0.);
        let mut input_sum = 0f64;
        let mut generated_sum = 0f64;
        let cleanup = RawEdits {
            hot_pixels,
            ..Default::default()
        };
        for row in 0..rows {
            for column in 0..columns {
                ensure!(
                    control(row * columns + column, total),
                    "Reconstruction cancelled"
                );
                let origin = [
                    source.origin[0] + column * tile_edge,
                    source.origin[1] + row * tile_edge,
                ];
                let size = [
                    (source.origin[0] + source.active[0] - origin[0]).min(tile_edge),
                    (source.origin[1] + source.active[1] - origin[1]).min(tile_edge),
                ];
                let patch = self.reconstruct_region(source, origin, size, hot_pixels, false)?;
                let cfa = source.cfa.as_ref().context("Missing Bayer pattern")?;
                for y in 0..size[1] {
                    for x in 0..size[0] {
                        let pixel = patch.pixels[y * size[0] + x];
                        let index = (origin[1] + y) * width + origin[0] + x;
                        data[index * 3..index * 3 + 3].copy_from_slice(&pixel);
                        input_sum += if hot_pixels {
                            source.clean_raw(
                                (origin[0] + x) as isize,
                                (origin[1] + y) as isize,
                                &cleanup,
                            )
                        } else {
                            source.data[index]
                        } as f64;
                        generated_sum += pixel[cfa.color_at(origin[1] + y, origin[0] + x)] as f64;
                    }
                }
                ensure!(
                    control(row * columns + column + 1, total),
                    "Reconstruction cancelled"
                );
            }
        }
        ensure!(
            generated_sum.abs() > 1e-12 || input_sum.abs() <= 1e-12,
            "Cannot preserve image-wide sensor photometry"
        );
        let gain = if input_sum.abs() <= 1e-12 {
            0.
        } else {
            (input_sum / generated_sum) as f32
        };
        ensure!(gain.is_finite(), "Invalid image-wide reconstruction gain");
        use rayon::prelude::*;
        data[..rgb_count].par_iter_mut().for_each(|v| *v *= gain);
        // Populate sensor margins by reflecting valid image RGB, so continuous
        // sampling at the crop boundary never mixes in zero/overscan values.
        for y in 0..height {
            for x in 0..width {
                if x >= source.origin[0]
                    && x < source.origin[0] + source.active[0]
                    && y >= source.origin[1]
                    && y < source.origin[1] + source.active[1]
                {
                    continue;
                }
                let sx = source.origin[0]
                    + SensorImage::reflect(
                        x as isize - source.origin[0] as isize,
                        source.active[0],
                    );
                let sy = source.origin[1]
                    + SensorImage::reflect(
                        y as isize - source.origin[1] as isize,
                        source.active[1],
                    );
                let pixel: [f32; 3] =
                    data[(sy * width + sx) * 3..(sy * width + sx) * 3 + 3].try_into()?;
                data[(y * width + x) * 3..(y * width + x) * 3 + 3].copy_from_slice(&pixel);
            }
        }
        // Four three-bit masks fit in each normal float's mantissa. Keeping the
        // packed original flags in this allocation needs no extra GPU binding
        // or sensor copy; it adds about one byte per reconstructed pixel.
        data[rgb_count..]
            .par_iter_mut()
            .enumerate()
            .for_each(|(word, slot)| {
                let mut bits = 0u32;
                for lane in 0..4 {
                    let i = word * 4 + lane;
                    if i < width * height {
                        let x = source.origin[0]
                            + SensorImage::reflect(
                                (i % width) as isize - source.origin[0] as isize,
                                source.active[0],
                            );
                        let y = source.origin[1]
                            + SensorImage::reflect(
                                (i / width) as isize - source.origin[1] as isize,
                                source.active[1],
                            );
                        bits |= (source.clipping_at_sensor(x as isize, y as isize, &cleanup)
                            as u32)
                            << (3 * lane);
                    }
                }
                *slot = f32::from_bits(0x3f80_0000 | bits);
            });
        ensure!(
            data.iter().all(|v| v.is_finite()),
            "Nonfinite reconstructed camera RGB"
        );
        Ok(SensorImage {
            reconstruction: Reconstruction::RawNindV1,
            metadata: source.metadata.clone(),
            data,
            cfa: None,
            cpp: 3,
            origin: source.origin,
            active: source.active,
            orientation: source.orientation,
            raw_integer: source.raw_integer,
            clipping_offset: source.raw_integer.then_some(rgb_count),
        })
    }
}

fn context_start(position: usize, phase: usize) -> Result<isize> {
    let position =
        isize::try_from(position).context("Sensor coordinate exceeds addressable range")?;
    Ok((position - 256 - phase as isize).div_euclid(32) * 32 + phase as isize)
}

fn context_extent(position: usize, size: usize, start: isize) -> Result<usize> {
    let end = position
        .checked_add(size)
        .and_then(|v| v.checked_add(256))
        .context("Context size overflow")?;
    let extent = isize::try_from(end)
        .context("Context exceeds addressable range")?
        .checked_sub(start)
        .context("Context extent overflow")?;
    let rounded = usize::try_from(extent)?
        .checked_add(31)
        .context("Context padding overflow")?
        / 32
        * 32;
    ensure!(
        rounded <= isize::MAX as usize,
        "Context exceeds addressable range"
    );
    Ok(rounded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contexts_keep_sensor_pooling_phase_and_cover_large_odd_regions() {
        for phase in [0, 1] {
            for position in [0, 1, 257, 100_003] {
                let left = context_start(position, phase).unwrap();
                let width = context_extent(position, 100_003, left).unwrap();
                assert_eq!((left - phase as isize).rem_euclid(32), 0);
                assert!(left <= position as isize - 256);
                assert_eq!(width % 32, 0);
                assert!(left + width as isize >= (position + 100_003 + 256) as isize);
            }
        }
        assert!(context_start(usize::MAX, 0).is_err());
        assert!(context_extent(usize::MAX, 1, 0).is_err());
    }
}
