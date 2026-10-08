//! Atomic, color-tagged export. Integer outputs clip only at final encoding.
use crate::{
    color::{self, OutputSpace},
    pipeline::Rendered,
};
use anyhow::{Context, Result, bail, ensure};
use image::{ExtendedColorType, ImageEncoder};
use lcms2::{CIExyY, CIExyYTRIPLE, Profile, ToneCurve};
use rayon::prelude::*;
use std::{io::BufWriter, path::Path};

pub fn profile(space: OutputSpace) -> Result<Profile> {
    if space == OutputSpace::Srgb {
        return Ok(Profile::new_srgb());
    }
    let primaries = match space {
        OutputSpace::DisplayP3 => [[0.68, 0.32], [0.265, 0.69], [0.15, 0.06]],
        OutputSpace::AdobeRgb => [[0.64, 0.33], [0.21, 0.71], [0.15, 0.06]],
        OutputSpace::Rec2020 => [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]],
        _ => [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]],
    };
    let xy = |p: [f64; 2]| CIExyY {
        x: p[0],
        y: p[1],
        Y: 1.,
    };
    let rgb = CIExyYTRIPLE {
        Red: xy(primaries[0]),
        Green: xy(primaries[1]),
        Blue: xy(primaries[2]),
    };
    let white = xy([0.3127, 0.3290]);
    let curve = match space {
        OutputSpace::DisplayP3 => {
            ToneCurve::new_parametric(4, &[2.4, 1. / 1.055, 0.055 / 1.055, 1. / 12.92, 0.04045])?
        }
        OutputSpace::AdobeRgb => ToneCurve::new(563. / 256.),
        OutputSpace::Rec2020 => ToneCurve::new_parametric(
            4,
            &[
                1. / 0.45,
                1. / 1.09929682680944,
                0.09929682680944 / 1.09929682680944,
                1. / 4.5,
                4.5 * 0.018053968510807,
            ],
        )?,
        _ => ToneCurve::new(1.),
    };
    Ok(Profile::new_rgb(&white, &rgb, &[&curve, &curve, &curve])?)
}

pub fn rgba8(image: &Rendered, space: OutputSpace) -> Vec<u8> {
    let m = space.matrix();
    image
        .pixels
        .par_iter()
        .flat_map_iter(|p| {
            let rgb = color::apply(m, [p[0], p[1], p[2]]);
            [
                quantize8(space.encode(rgb[0])),
                quantize8(space.encode(rgb[1])),
                quantize8(space.encode(rgb[2])),
                quantize8(p[3]),
            ]
        })
        .collect()
}
fn quantize8(x: f32) -> u8 {
    (x.clamp(0., 1.) * 255. + 0.5) as u8
}
fn quantize16(x: f32) -> u16 {
    (x.clamp(0., 1.) * 65535. + 0.5) as u16
}

pub fn write(path: &Path, image: &Rendered, space: OutputSpace, overwrite: bool) -> Result<()> {
    let extension = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    ensure!(
        image.pixels.len() == crate::input::pixel_count(image.width, image.height, 1)?,
        "Invalid rendered image"
    );
    let (width, height) = (u32::try_from(image.width)?, u32::try_from(image.height)?);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    {
        let writer = BufWriter::new(temporary.as_file_mut());
        match extension.as_str() {
            "png" | "tif" | "tiff" => {
                let m = space.matrix();
                let rgba: Vec<u16> = image
                    .pixels
                    .par_iter()
                    .flat_map_iter(|p| {
                        let rgb = color::apply(m, [p[0], p[1], p[2]]);
                        [
                            quantize16(space.encode(rgb[0])),
                            quantize16(space.encode(rgb[1])),
                            quantize16(space.encode(rgb[2])),
                            quantize16(p[3]),
                        ]
                    })
                    .collect();
                let bytes: &[u8] = bytemuck::cast_slice(&rgba);
                let icc = profile(space)?.icc()?;
                if extension == "png" {
                    let mut encoder = image::codecs::png::PngEncoder::new(writer);
                    encoder.set_icc_profile(icc)?;
                    encoder.write_image(bytes, width, height, ExtendedColorType::Rgba16)?;
                } else {
                    let mut encoder = image::codecs::tiff::TiffEncoder::new(writer);
                    encoder.set_icc_profile(icc)?;
                    encoder.write_image(bytes, width, height, ExtendedColorType::Rgba16)?;
                }
            }
            "jpg" | "jpeg" => {
                ensure!(
                    width <= 65535 && height <= 65535,
                    "JPEG format limits dimensions to 65535; use TIFF, PNG or EXR"
                );
                let rgba = rgba8(image, space);
                let rgb: Vec<u8> = rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| p[..3].iter().copied())
                    .collect();
                let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(writer, 95);
                encoder.set_icc_profile(profile(space)?.icc()?)?;
                encoder.write_image(&rgb, width, height, ExtendedColorType::Rgb8)?;
            }
            "exr" => {
                ensure!(
                    space == OutputSpace::LinearSrgb,
                    "EXR stores linear sRGB; select --color-space linear-srgb"
                );
                image::codecs::openexr::OpenExrEncoder::new(writer).write_image(
                    bytemuck::cast_slice(&image.pixels),
                    width,
                    height,
                    ExtendedColorType::Rgba32F,
                )?;
            }
            _ => bail!("Export extension must be png, jpg, tif, tiff or exr"),
        }
    }
    temporary.as_file().sync_all()?;
    if overwrite {
        temporary.persist(path)?;
    } else {
        temporary.persist_noclobber(path)?;
    }
    Ok(())
}

pub fn display_rgba8(image: &Rendered, monitor: Option<&Path>) -> Result<Vec<u8>> {
    let Some(monitor) = monitor else {
        return Ok(rgba8(image, OutputSpace::Srgb));
    };
    let input = profile(OutputSpace::LinearSrgb)?;
    let output = Profile::new_file(monitor).context("Opening display ICC profile")?;
    let transform: lcms2::Transform<[f32; 3], [u8; 3]> = lcms2::Transform::new(
        &input,
        lcms2::PixelFormat::RGB_FLT,
        &output,
        lcms2::PixelFormat::RGB_8,
        lcms2::Intent::RelativeColorimetric,
    )?;
    let rgb: Vec<_> = image.pixels.iter().map(|p| [p[0], p[1], p[2]]).collect();
    let mut encoded = vec![[0; 3]; rgb.len()];
    transform.transform_pixels(&rgb, &mut encoded);
    Ok(encoded
        .iter()
        .zip(&image.pixels)
        .flat_map(|(p, a)| [p[0], p[1], p[2], quantize8(a[3])])
        .collect())
}
