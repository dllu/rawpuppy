//! Decode immutable originals, retain a single normalized sensor allocation, reconstruct on demand.
use crate::{
    color::{self, Matrix},
    edits::{RawEdits, Reconstruction},
};
use anyhow::{Context, Result, bail, ensure};
use image::ImageDecoder;
use rawler::{CFA, Orientation, RawImageData, rawimage::RawPhotometricInterpretation};
use rayon::prelude::*;
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, Serialize)]
pub struct Metadata {
    pub make: String,
    pub model: String,
    pub sensor_width: usize,
    pub sensor_height: usize,
    pub width: usize,
    pub height: usize,
    pub bits: usize,
    /// Decoding changes that invalidate fills made with older color interpretation.
    pub color_revision: u32,
    pub pattern: String,
    pub as_shot: [f32; 3],
    pub camera_to_working: Matrix,
    pub lens_model: Option<String>,
    pub focal_length_mm: Option<f32>,
    pub aperture: Option<f32>,
    pub lens_profile: Option<crate::lens::LensProfile>,
    pub lens_profile_error: Option<String>,
}

pub struct SensorImage {
    pub(crate) reconstruction: Reconstruction,
    pub metadata: Metadata,
    /// Row-major sensor/RGB values. Learned integer-RAW caches append packed
    /// clipping words after `sensor_width * sensor_height * cpp` values.
    pub data: Vec<f32>,
    pub cfa: Option<CFA>,
    pub cpp: usize,
    pub origin: [usize; 2],
    pub active: [usize; 2],
    pub orientation: Orientation,
    /// True only for integer camera RAW; developed RGB and float HDR stay intact.
    pub raw_integer: bool,
    /// Packed original clipping flags appended after a learned RGB allocation.
    pub(crate) clipping_offset: Option<usize>,
}

pub fn pixel_count(width: usize, height: usize, channels: usize) -> Result<usize> {
    ensure!(
        width > 0 && height > 0 && channels > 0,
        "Image dimensions must be nonzero"
    );
    width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(channels))
        .context("Image dimensions exceed the addressable memory range")
}

pub fn color_revision(path: &Path) -> Result<u32> {
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exr"))
    {
        return Ok(0);
    }
    let metadata = exr::meta::MetaData::read_from_file(path, false)?;
    Ok(metadata
        .headers
        .first()
        .and_then(|h| h.shared_attributes.chromaticities)
        .is_some_and(|c| c != crate::export::SRGB_CHROMATICITIES) as u32)
}

impl SensorImage {
    pub fn open(path: &Path) -> Result<Self> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "exr") {
            return Self::open_rgb(path);
        }
        let source = rawler::rawsource::RawSource::new(path)?;
        let params = rawler::decoders::RawDecodeParams::default();
        let decoded = rawler::get_decoder(&source).and_then(|decoder| {
            let mut raw = decoder.raw_image(&source, &params, false)?;
            let metadata = decoder.raw_metadata(&source, &params).ok();
            if let Some(orientation) = metadata.as_ref().and_then(|m| m.exif.orientation) {
                raw.orientation = Orientation::from_u16(orientation);
            }
            Ok((raw, metadata))
        });
        let (raw, raw_metadata) = match decoded {
            Ok(decoded) => decoded,
            Err(_) if matches!(ext.as_str(), "tif" | "tiff") => return Self::open_rgb(path),
            Err(e) => return Err(e).with_context(|| format!("Decoding {}", path.display())),
        };
        ensure!(
            raw.cpp == 1 || raw.cpp == 3,
            "Only RGB, Bayer, X-Trans and monochrome sensors are supported"
        );
        ensure!(
            raw.fuji_rotation_width.is_none(),
            "Rotated SuperCCD sensors are not yet supported"
        );
        let cfa = match &raw.photometric {
            RawPhotometricInterpretation::Cfa(config) => {
                ensure!(
                    config.cfa.is_rgb(),
                    "Unsupported non-RGB color filter array"
                );
                Some(config.cfa.clone())
            }
            _ => None,
        };
        let count = pixel_count(raw.width, raw.height, raw.cpp)?;
        let fallback = rawler::imgop::Rect::new(
            rawler::imgop::Point::new(0, 0),
            rawler::imgop::Dim2::new(raw.width, raw.height),
        );
        let crop = raw.crop_area.or(raw.active_area).unwrap_or(fallback);
        ensure!(
            crop.p
                .x
                .checked_add(crop.d.w)
                .is_some_and(|x| x <= raw.width)
                && crop
                    .p
                    .y
                    .checked_add(crop.d.h)
                    .is_some_and(|y| y <= raw.height),
            "Invalid sensor crop"
        );
        let (transpose, _, _) = raw.orientation.to_flips();
        let (width, height) = if transpose {
            (crop.d.h, crop.d.w)
        } else {
            (crop.d.w, crop.d.h)
        };
        use rawler::imgop::xyz::Illuminant;
        let selected_matrix = raw.color_matrix_find_first([
            Illuminant::D65,
            Illuminant::D50,
            Illuminant::D55,
            Illuminant::D75,
            Illuminant::A,
            Illuminant::B,
            Illuminant::C,
            Illuminant::Daylight,
            Illuminant::Flash,
        ]);
        let (xyz_to_cam, white): (Matrix, [f32; 3]) =
            if let Some((illuminant, flat)) = selected_matrix {
                ensure!(
                    flat.len() == 9,
                    "Camera matrix must have three color channels"
                );
                let white = match illuminant {
                    Illuminant::D50 => [0.96422, 1., 0.82521],
                    Illuminant::D55 => [0.95682, 1., 0.92149],
                    Illuminant::D75 => [0.94972, 1., 1.22638],
                    Illuminant::A => [1.09850, 1., 0.35585],
                    Illuminant::B => [0.99072, 1., 0.85223],
                    Illuminant::C => [0.98074, 1., 1.18232],
                    _ => [0.95047, 1., 1.08883],
                };
                (
                    std::array::from_fn(|i| std::array::from_fn(|j| flat[i * 3 + j])),
                    white,
                )
            } else {
                (
                    std::array::from_fn(|i| raw.xyz_to_cam[i]),
                    [0.95047, 1., 1.08883],
                )
            };
        let response = color::apply(xyz_to_cam, white);
        let fallback_wb = response.map(|v| 1. / v);
        let as_shot: [f32; 3] = std::array::from_fn(|i| {
            let v = raw.wb_coeffs[i];
            if v.is_finite() && v > 0. {
                v
            } else {
                fallback_wb[i]
            }
        });
        let as_shot = as_shot.map(|x| x / as_shot[1]);
        ensure!(
            as_shot.iter().all(|x| x.is_finite() && *x > 0.),
            "Invalid camera white point"
        );
        let camera_to_working = if raw.is_monochrome() {
            color::IDENTITY
        } else {
            // DNG matrices map XYZ to camera. Normalize camera responses to D65 RGB white,
            // then invert; the single color calibration module later applies as-shot gains.
            let adaptation = color::adapt_white([0.95047, 1., 1.08883], white)?;
            let mut srgb_to_cam =
                color::multiply(xyz_to_cam, color::multiply(adaptation, color::SRGB_TO_XYZ));
            for row in &mut srgb_to_cam {
                let sum: f32 = row.iter().sum();
                ensure!(sum.abs() > 1e-8, "Camera has no usable color calibration");
                for v in row {
                    *v /= sum;
                }
            }
            color::inverse(srgb_to_cam)?
        };
        let black = raw.blacklevel.as_vec();
        let white = raw.whitelevel.as_vec();
        ensure!(
            black.iter().all(|b| b.is_finite())
                && white.iter().all(|w| w.is_finite())
                && black.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                    < white.iter().copied().fold(f32::INFINITY, f32::min),
            "Invalid sensor black and white levels"
        );
        let bw = raw.blacklevel.width;
        let bh = raw.blacklevel.height;
        let bc = raw.blacklevel.cpp;
        ensure!(
            bw > 0 && bh > 0 && !black.is_empty() && !white.is_empty(),
            "Missing sensor levels"
        );
        let normalize = |i: usize, v: f32| {
            let pixel = i / raw.cpp;
            let x = pixel % raw.width;
            let y = pixel / raw.width;
            let channel = i % raw.cpp;
            let bi = ((y % bh) * bw + x % bw) * bc + channel.min(bc - 1);
            let b = black[bi];
            let w = white[channel.min(white.len() - 1)];
            (v - b) / (w - b)
        };
        let mut data = Vec::new();
        data.try_reserve_exact(count)?;
        #[cfg(feature = "cuda")]
        crate::gpu::advise_sensor_allocation(data.spare_capacity_mut());
        match &raw.data {
            RawImageData::Integer(v) => {
                ensure!(v.len() == count, "Incomplete sensor data");
                v.par_iter()
                    .enumerate()
                    .map(|(i, v)| normalize(i, *v as f32))
                    .collect_into_vec(&mut data);
            }
            RawImageData::Float(v) => {
                ensure!(v.len() == count, "Incomplete sensor data");
                v.par_iter()
                    .enumerate()
                    .map(|(i, v)| normalize(i, *v))
                    .collect_into_vec(&mut data);
            }
        }
        let (lens_profile, lens_profile_error) = match crate::lens::read_raf(&source) {
            Ok(profile) => (profile, None),
            Err(error) => (None, Some(format!("{error:#}"))),
        };
        let positive = |v: rawler::formats::tiff::Rational| {
            (v.n > 0 && v.d > 0).then(|| v.n as f32 / v.d as f32)
        };
        let metadata = Metadata {
            make: raw.clean_make.clone(),
            model: raw.clean_model.clone(),
            sensor_width: raw.width,
            sensor_height: raw.height,
            width,
            height,
            bits: raw.bps,
            color_revision: 0,
            pattern: cfa.as_ref().map_or("RGB/mono".into(), |c| c.name.clone()),
            as_shot,
            camera_to_working,
            lens_model: raw_metadata
                .as_ref()
                .and_then(|m| m.exif.lens_model.clone()),
            focal_length_mm: raw_metadata
                .as_ref()
                .and_then(|m| m.exif.focal_length)
                .and_then(positive),
            aperture: raw_metadata
                .as_ref()
                .and_then(|m| m.exif.fnumber)
                .and_then(positive),
            lens_profile,
            lens_profile_error,
        };
        Ok(Self {
            reconstruction: Reconstruction::Mhc,
            metadata,
            data,
            cfa,
            cpp: raw.cpp,
            origin: [crop.p.x, crop.p.y],
            active: [crop.d.w, crop.d.h],
            orientation: raw.orientation,
            raw_integer: matches!(&raw.data, RawImageData::Integer(_)),
            clipping_offset: None,
        })
    }

    fn open_rgb(path: &Path) -> Result<Self> {
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("tif") || e.eq_ignore_ascii_case("tiff"))
        {
            return Self::open_tiff(path);
        }
        let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
        reader.no_limits();
        let mut decoder = reader.into_decoder()?;
        let orientation = decoder.orientation()?;
        let icc = decoder.icc_profile()?;
        let bits = decoder.color_type().bits_per_pixel() as usize
            / decoder.color_type().channel_count() as usize;
        let mut image = image::DynamicImage::from_decoder(decoder)?;
        image.apply_orientation(orientation);
        let linear = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exr"));
        let image = image.to_rgb32f();
        let (width, height) = (image.width() as usize, image.height() as usize);
        pixel_count(width, height, 3)?;
        let mut data = image.into_raw();
        let mut color_revision = 0;
        if let Some(icc) = icc {
            let input = lcms2::Profile::new_icc(&icc).context("Reading input ICC profile")?;
            let output = crate::export::profile(color::OutputSpace::LinearSrgb)?;
            let transform: lcms2::Transform<[f32; 3], [f32; 3]> = lcms2::Transform::new(
                &input,
                lcms2::PixelFormat::RGB_FLT,
                &output,
                lcms2::PixelFormat::RGB_FLT,
                lcms2::Intent::RelativeColorimetric,
            )?;
            transform.transform_in_place(bytemuck::cast_slice_mut(&mut data));
        } else if linear {
            let metadata = exr::meta::MetaData::read_from_file(path, false)?;
            if let Some(chroma) = metadata
                .headers
                .first()
                .and_then(|h| h.shared_attributes.chromaticities)
            {
                color_revision = (chroma != crate::export::SRGB_CHROMATICITIES) as u32;
                let xy = |v: exr::math::Vec2<f32>| [v.x(), v.y()];
                let m = color::rgb_primaries_to_working(
                    [xy(chroma.red), xy(chroma.green), xy(chroma.blue)],
                    xy(chroma.white),
                )?;
                data.par_chunks_exact_mut(3).for_each(|p| {
                    let rgb = color::apply(m, [p[0], p[1], p[2]]);
                    p.copy_from_slice(&rgb);
                });
            }
        } else {
            data.par_iter_mut()
                .for_each(|v| *v = color::srgb_decode(*v));
        }
        let metadata = Metadata {
            make: String::new(),
            model: "RGB image".into(),
            sensor_width: width,
            sensor_height: height,
            width,
            height,
            bits,
            color_revision,
            pattern: "RGB".into(),
            as_shot: [1.; 3],
            camera_to_working: color::IDENTITY,
            lens_model: None,
            focal_length_mm: None,
            aperture: None,
            lens_profile: None,
            lens_profile_error: None,
        };
        Ok(Self {
            reconstruction: Reconstruction::Mhc,
            metadata,
            data,
            cfa: None,
            cpp: 3,
            origin: [0, 0],
            active: [width, height],
            orientation: Orientation::Normal,
            raw_integer: false,
            clipping_offset: None,
        })
    }

    fn open_tiff(path: &Path) -> Result<Self> {
        use tiff::decoder::{Decoder, DecodingResult};
        let mut limits = tiff::decoder::Limits::default();
        limits.decoding_buffer_size = usize::MAX;
        limits.intermediate_buffer_size = usize::MAX;
        limits.ifd_value_size = usize::MAX;
        let mut decoder = Decoder::new(std::fs::File::open(path)?)?.with_limits(limits);
        let (w, h) = decoder.dimensions()?;
        let orientation = decoder
            .find_tag_unsigned::<u16>(tiff::tags::Tag::Orientation)?
            .map(Orientation::from_u16)
            .unwrap_or(Orientation::Normal);
        let icc = decoder
            .find_tag(tiff::tags::Tag::IccProfile)?
            .map(|v| v.into_u8_vec())
            .transpose()?;
        let (channels, bits) = match decoder.colortype()? {
            tiff::ColorType::RGB(b) => (3, b),
            tiff::ColorType::RGBA(b) => (4, b),
            tiff::ColorType::Gray(b) => (1, b),
            tiff::ColorType::GrayA(b) => (2, b),
            other => bail!("Unsupported TIFF color type {other:?}"),
        };
        let samples: Vec<f32> = match decoder.read_image()? {
            DecodingResult::U8(v) => v.into_iter().map(|x| x as f32 / 255.).collect(),
            DecodingResult::U16(v) => v.into_iter().map(|x| x as f32 / 65535.).collect(),
            DecodingResult::F32(v) => v,
            other => bail!("Unsupported TIFF sample type {other:?}"),
        };
        let (width, height) = (w as usize, h as usize);
        ensure!(
            samples.len() == pixel_count(width, height, channels)?,
            "TIFF sample count disagrees with dimensions"
        );
        let mut data: Vec<f32> = samples
            .chunks_exact(channels)
            .flat_map(|p| {
                if channels <= 2 {
                    [p[0]; 3]
                } else {
                    [p[0], p[1], p[2]]
                }
            })
            .collect();
        if let Some(icc) = icc {
            let input = lcms2::Profile::new_icc(&icc)?;
            let output = crate::export::profile(color::OutputSpace::LinearSrgb)?;
            let transform: lcms2::Transform<[f32; 3], [f32; 3]> = lcms2::Transform::new(
                &input,
                lcms2::PixelFormat::RGB_FLT,
                &output,
                lcms2::PixelFormat::RGB_FLT,
                lcms2::Intent::RelativeColorimetric,
            )?;
            transform.transform_in_place(bytemuck::cast_slice_mut(&mut data));
        } else if bits != 32 {
            data.par_iter_mut()
                .for_each(|v| *v = color::srgb_decode(*v));
        }
        let mut image = Self::from_rgb(width, height, data)?;
        image.metadata.model = "TIFF image".into();
        image.metadata.bits = bits as usize;
        image.orientation = orientation;
        if orientation.to_flips().0 {
            image.metadata.width = height;
            image.metadata.height = width;
        }
        Ok(image)
    }

    /// Construct calibrated linear sRGB data for tests and external neural processing.
    pub fn from_rgb(width: usize, height: usize, data: Vec<f32>) -> Result<Self> {
        ensure!(
            data.len() == pixel_count(width, height, 3)?,
            "RGB buffer dimensions disagree"
        );
        ensure!(
            data.iter().all(|x| x.is_finite()),
            "RGB input must be finite"
        );
        Ok(Self {
            reconstruction: Reconstruction::Mhc,
            metadata: Metadata {
                make: String::new(),
                model: "Linear RGB".into(),
                sensor_width: width,
                sensor_height: height,
                width,
                height,
                bits: 32,
                color_revision: 0,
                pattern: "RGB".into(),
                as_shot: [1.; 3],
                camera_to_working: color::IDENTITY,
                lens_model: None,
                focal_length_mm: None,
                aperture: None,
                lens_profile: None,
                lens_profile_error: None,
            },
            data,
            cfa: None,
            cpp: 3,
            origin: [0, 0],
            active: [width, height],
            orientation: Orientation::Normal,
            raw_integer: false,
            clipping_offset: None,
        })
    }

    pub fn sensor_position(&self, uv: [f32; 2]) -> Option<[f32; 2]> {
        if uv
            .iter()
            .any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
        {
            return None;
        }
        let (transpose, fx, fy) = self.orientation.to_flips();
        let mut uv = if transpose { [uv[1], uv[0]] } else { uv };
        if fx {
            uv[0] = 1. - uv[0];
        }
        if fy {
            uv[1] = 1. - uv[1];
        }
        Some([
            self.origin[0] as f32 + uv[0] * self.active[0] as f32 - 0.5,
            self.origin[1] as f32 + uv[1] * self.active[1] as f32 - 0.5,
        ])
    }

    pub(crate) fn reflect(x: isize, length: usize) -> usize {
        if length == 1 {
            return 0;
        }
        let period = 2 * (length as isize - 1);
        let x = x.rem_euclid(period);
        if x >= length as isize {
            (period - x) as usize
        } else {
            x as usize
        }
    }
    pub(crate) fn raw_at(&self, x: isize, y: isize, channel: usize) -> f32 {
        let x = Self::reflect(x, self.metadata.sensor_width);
        let y = Self::reflect(y, self.metadata.sensor_height);
        self.data[(y * self.metadata.sensor_width + x) * self.cpp + channel.min(self.cpp - 1)]
    }
    pub(crate) fn clipping_at_sensor(&self, x: isize, y: isize, e: &RawEdits) -> u8 {
        if !self.raw_integer {
            return 0;
        }
        if let Some(offset) = self.clipping_offset {
            let x = Self::reflect(x, self.metadata.sensor_width);
            let y = Self::reflect(y, self.metadata.sensor_height);
            let i = y * self.metadata.sensor_width + x;
            return ((self.data[offset + i / 4].to_bits() >> (3 * (i % 4))) & 7) as u8;
        }
        if let Some(cfa) = &self.cfa {
            let mut mask = 0;
            // Cover one CFA period, including both green phases. The raw-domain
            // mask avoids interpreting demosaic ringing as physical clipping.
            let left = x - x.rem_euclid(cfa.width as isize);
            let top = y - y.rem_euclid(cfa.height as isize);
            for dy in 0..cfa.height {
                for dx in 0..cfa.width {
                    let sx = left + dx as isize;
                    let sy = top + dy as isize;
                    let value = self.clean_raw(sx, sy, e);
                    if (crate::highlights::MASK_THRESHOLD..=1.).contains(&value) {
                        let sx = Self::reflect(sx, self.metadata.sensor_width);
                        let sy = Self::reflect(sy, self.metadata.sensor_height);
                        mask |= 1 << cfa.color_at(sy, sx);
                    }
                }
            }
            mask
        } else {
            (0..3).fold(0, |mask, c| {
                mask | if (crate::highlights::MASK_THRESHOLD..=1.).contains(&self.raw_at(x, y, c)) {
                    1 << c
                } else {
                    0
                }
            })
        }
    }

    /// Original sensor clipping bits (R=1, G=2, B=4), including learned-source provenance.
    pub fn clipping_mask(&self, uv: [f32; 2], e: &RawEdits) -> u8 {
        let Some([x, y]) = self.sensor_position(uv) else {
            return 0;
        };
        let x = x.floor() as isize;
        let y = y.floor() as isize;
        self.clipping_at_sensor(x, y, e)
            | self.clipping_at_sensor(x + 1, y, e)
            | self.clipping_at_sensor(x, y + 1, e)
            | self.clipping_at_sensor(x + 1, y + 1, e)
    }

    pub(crate) fn clipping_weights(&self, uv: [f32; 2], e: &RawEdits) -> [f32; 3] {
        let Some([x, y]) = self.sensor_position(uv) else {
            return [0.; 3];
        };
        let ix = x.floor() as isize;
        let iy = y.floor() as isize;
        let tx = x - ix as f32;
        let ty = y - iy as f32;
        let masks = [
            self.clipping_at_sensor(ix, iy, e),
            self.clipping_at_sensor(ix + 1, iy, e),
            self.clipping_at_sensor(ix, iy + 1, e),
            self.clipping_at_sensor(ix + 1, iy + 1, e),
        ];
        std::array::from_fn(|c| {
            let v = masks.map(|m| if m & (1 << c) != 0 { 1. } else { 0. });
            (v[0] * (1. - tx) + v[1] * tx) * (1. - ty) + (v[2] * (1. - tx) + v[3] * tx) * ty
        })
    }

    pub(crate) fn clean_raw(&self, x: isize, y: isize, e: &RawEdits) -> f32 {
        let center = self.raw_at(x, y, 0);
        let mut neighbors = [
            self.raw_at(x - 2, y, 0),
            self.raw_at(x + 2, y, 0),
            self.raw_at(x, y - 2, 0),
            self.raw_at(x, y + 2, 0),
        ];
        let value = if e.hot_pixels {
            neighbors.sort_by(f32::total_cmp);
            let median = (neighbors[1] + neighbors[2]) * 0.5;
            if center > median.max(0.) * 4. + 0.02 {
                median
            } else {
                center
            }
        } else {
            center
        };
        if e.denoise <= 0. {
            return value;
        }
        let Some(cfa) = &self.cfa else {
            return value;
        };
        let xx = Self::reflect(x, self.metadata.sensor_width);
        let yy = Self::reflect(y, self.metadata.sensor_height);
        let channel = cfa.color_at(yy, xx);
        let mut sum = value;
        let mut weights = 1.;
        // Same-color bilateral neighborhood; preserves CFA parity and never mixes sensor colors.
        for dy in -2..=2 {
            for dx in -2..=2 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = Self::reflect(x + dx, self.metadata.sensor_width);
                let ny = Self::reflect(y + dy, self.metadata.sensor_height);
                if cfa.color_at(ny, nx) != channel {
                    continue;
                }
                let v = self.raw_at(x + dx, y + dy, 0);
                let difference = (v - value) / e.denoise;
                let w = (-0.5 * difference * difference - 0.125 * (dx * dx + dy * dy) as f32).exp();
                sum += w * v;
                weights += w;
            }
        }
        sum / weights
    }

    pub fn reconstruct(&self, x: isize, y: isize, e: &RawEdits) -> [f32; 3] {
        let Some(cfa) = &self.cfa else {
            return std::array::from_fn(|c| self.raw_at(x, y, c));
        };
        let xx = Self::reflect(x, self.metadata.sensor_width);
        let yy = Self::reflect(y, self.metadata.sensor_height);
        let channel = cfa.color_at(yy, xx);
        let at = |dx, dy| {
            if e.hot_pixels || e.denoise > 0. {
                self.clean_raw(x + dx, y + dy, e)
            } else {
                self.raw_at(x + dx, y + dy, 0)
            }
        };
        let center = at(0, 0);
        if cfa.width == 2 && cfa.height == 2 {
            // Malvar–He–Cutler 5×5 linear reconstruction, independently expressed from the paper.
            let h1 = at(-1, 0) + at(1, 0);
            let v1 = at(0, -1) + at(0, 1);
            let h2 = at(-2, 0) + at(2, 0);
            let v2 = at(0, -2) + at(0, 2);
            let diagonals = at(-1, -1) + at(1, -1) + at(-1, 1) + at(1, 1);
            if channel == 1 {
                let horizontal = (5. * center + 4. * h1 - h2 - diagonals + 0.5 * v2) / 8.;
                let vertical = (5. * center + 4. * v1 - v2 - diagonals + 0.5 * h2) / 8.;
                if cfa.color_at(yy, xx + 1) == 0 {
                    [horizontal, center, vertical]
                } else {
                    [vertical, center, horizontal]
                }
            } else {
                let green = (4. * center + 2. * (h1 + v1) - h2 - v2) / 8.;
                let opposite = (6. * center + 2. * diagonals - 1.5 * (h2 + v2)) / 8.;
                if channel == 0 {
                    [center, green, opposite]
                } else {
                    [opposite, green, center]
                }
            }
        } else {
            // RGB X-Trans fallback with a wider, color-aware neighborhood.
            let mut sums = [0.; 3];
            let mut weights = [0.; 3];
            for dy in -3..=3 {
                for dx in -3..=3 {
                    let nx = Self::reflect(x + dx, self.metadata.sensor_width);
                    let ny = Self::reflect(y + dy, self.metadata.sensor_height);
                    let c = cfa.color_at(ny, nx);
                    if c >= 3 {
                        continue;
                    }
                    let w = 1. / (1. + (dx * dx + dy * dy) as f32);
                    sums[c] += at(dx, dy) * w;
                    weights[c] += w;
                }
            }
            let mut rgb = std::array::from_fn(|i| sums[i] / weights[i].max(1e-8));
            rgb[channel] = center;
            rgb
        }
    }

    pub fn sample(&self, uv: [f32; 2], e: &RawEdits) -> Option<[f32; 3]> {
        let [x, y] = self.sensor_position(uv)?;
        let ix = x.floor() as isize;
        let iy = y.floor() as isize;
        let tx = x - ix as f32;
        let ty = y - iy as f32;
        let a = self.reconstruct(ix, iy, e);
        let b = self.reconstruct(ix + 1, iy, e);
        let c = self.reconstruct(ix, iy + 1, e);
        let d = self.reconstruct(ix + 1, iy + 1, e);
        Some(std::array::from_fn(|i| {
            (a[i] * (1. - tx) + b[i] * tx) * (1. - ty) + (c[i] * (1. - tx) + d[i] * tx) * ty
        }))
    }

    pub fn require_rgb(&self) -> Result<()> {
        if self.cpp != 3 || self.cfa.is_some() {
            bail!("Operation requires a developed RGB image");
        }
        Ok(())
    }
}
