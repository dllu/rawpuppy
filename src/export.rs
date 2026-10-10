//! Atomic, color-tagged export. Integer outputs clip only at final encoding.
use crate::{
    color::{self, OutputSpace},
    pipeline::Rendered,
};
use anyhow::{Context, Result, bail, ensure};
use image::{ExtendedColorType, ImageEncoder};
use lcms2::{CIExyY, CIExyYTRIPLE, Profile, ToneCurve};
use rayon::prelude::*;
use std::{
    borrow::Cow,
    io::{BufWriter, Seek, Write},
    path::Path,
};

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

fn rgba16_pixel(p: &[f32; 4], matrix: color::Matrix, space: OutputSpace) -> [u16; 4] {
    let rgb = color::apply(matrix, [p[0], p[1], p[2]]);
    [
        quantize16(space.encode(rgb[0])),
        quantize16(space.encode(rgb[1])),
        quantize16(space.encode(rgb[2])),
        quantize16(p[3]),
    ]
}

fn write_png(writer: impl Write + Send, image: &Rendered, space: OutputSpace) -> Result<()> {
    let mut info = png::Info::with_size(u32::try_from(image.width)?, u32::try_from(image.height)?);
    info.icc_profile = Some(Cow::Owned(profile(space)?.icc()?));
    let mut encoder = png::Encoder::with_info(writer, info)?;
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Sixteen);
    encoder.set_compression(png::Compression::Fast);
    let mut output = encoder.write_header()?;
    {
        let mut stream = output.stream_writer_with_size(65_536)?;
        // Conversion need not retain a whole raster, or even a whole wide row.
        // The PNG codec retains its own filtering rows and compression state.
        let batch = image.pixels.len().min(131_072);
        let mut buffers = [Vec::<[u8; 8]>::new(), Vec::new()];
        for bytes in &mut buffers {
            bytes.try_reserve_exact(batch)?;
            bytes.resize(batch, [0; 8]);
        }
        let [mut bytes, mut next] = buffers;
        let matrix = space.matrix();
        let convert = |values: &mut [[u8; 8]], pixels: &[[f32; 4]]| {
            values.par_iter_mut().zip(pixels).for_each(|(out, p)| {
                for (channel, value) in out
                    .as_chunks_mut::<2>()
                    .0
                    .iter_mut()
                    .zip(rgba16_pixel(p, matrix, space))
                {
                    *channel = value.to_be_bytes();
                }
            });
        };
        let mut chunks = image.pixels.chunks(batch);
        let first = chunks.next().expect("Validated nonempty rendered raster");
        convert(&mut bytes[..first.len()], first);
        let mut previous_len = first.len();
        for pixels in chunks {
            // Finish both scoped tasks before swapping their buffers or returning
            // a write error. Conversion reads only the immutable rendered pixels.
            let (written, ()) = rayon::join(
                || stream.write_all(bytemuck::cast_slice(&bytes[..previous_len])),
                || convert(&mut next[..pixels.len()], pixels),
            );
            written?;
            std::mem::swap(&mut bytes, &mut next);
            previous_len = pixels.len();
        }
        stream.write_all(bytemuck::cast_slice(&bytes[..previous_len]))?;
        stream.finish()?;
    }
    // Validate the data stream and final IEND write; Drop cannot report errors.
    output.finish()?;
    Ok(())
}

fn tiff_layout(width: u32, height: u32, icc_bytes: usize) -> (u32, bool) {
    let row_bytes = u64::from(width) * 8;
    // Bound conversion scratch to about 1 MiB, except when a single row is larger.
    let rows = (1_048_576 / row_bytes).max(1).min(u64::from(height));
    let strips = u64::from(height).div_ceil(rows);
    // Both offset/count arrays, ICC data, and ample room for the small fixed IFD.
    // Saturation selects BigTIFF even for dimensions whose byte count overflows u64.
    let bound = row_bytes
        .saturating_mul(u64::from(height))
        .saturating_add(strips * 16)
        .saturating_add(icc_bytes as u64)
        .saturating_add(4096);
    (rows as u32, bound > u64::from(u32::MAX))
}

fn write_tiff<K: tiff::encoder::TiffKind>(
    writer: impl Write + Seek,
    image: &Rendered,
    space: OutputSpace,
    icc: &[u8],
    rows_per_strip: u32,
) -> Result<()> {
    let mut encoder = tiff::encoder::TiffEncoder::<_, K>::new_generic(writer)?;
    let mut output = encoder.new_image::<tiff::encoder::colortype::RGBA16>(
        u32::try_from(image.width)?,
        u32::try_from(image.height)?,
    )?;
    output.rows_per_strip(rows_per_strip)?;
    output
        .encoder()
        .write_tag(tiff::tags::Tag::IccProfile, icc)?;
    output.encoder().write_tag(
        tiff::tags::Tag::ExtraSamples,
        &[tiff::tags::ExtraSamples::UnassociatedAlpha.to_u16()][..],
    )?;
    let strip_pixels = usize::try_from(output.next_strip_sample_count())? / 4;
    let mut rgba = Vec::<[u16; 4]>::new();
    rgba.try_reserve_exact(strip_pixels)?;
    rgba.resize(strip_pixels, [0; 4]);
    let matrix = space.matrix();
    for pixels in image.pixels.chunks(strip_pixels) {
        let values = &mut rgba[..pixels.len()];
        values.par_iter_mut().zip(pixels).for_each(|(out, p)| {
            *out = rgba16_pixel(p, matrix, space);
        });
        output.write_strip(bytemuck::cast_slice(values))?;
    }
    output.finish()?;
    Ok(())
}

pub const SRGB_CHROMATICITIES: exr::meta::attribute::Chromaticities =
    exr::meta::attribute::Chromaticities {
        red: exr::math::Vec2(0.64, 0.33),
        green: exr::math::Vec2(0.30, 0.60),
        blue: exr::math::Vec2(0.15, 0.06),
        white: exr::math::Vec2(0.3127, 0.329),
    };
/// Stream full-precision linear RGBA and explicit primaries, including HDR/negative values.
pub fn write_linear_exr(
    image: &Rendered,
    writer: impl std::io::Write + std::io::Seek,
    chroma: exr::meta::attribute::Chromaticities,
) -> Result<()> {
    use exr::prelude::{Image, SpecificChannels, Vec2, WritableImage};
    ensure!(
        image.pixels.len() == crate::input::pixel_count(image.width, image.height, 1)?,
        "Invalid EXR raster"
    );
    let channels = SpecificChannels::rgba(|Vec2(x, y): Vec2<usize>| {
        let p = image.pixels[y * image.width + x];
        (p[0], p[1], p[2], p[3])
    });
    let mut output = Image::from_channels((image.width, image.height), channels);
    output.attributes.chromaticities = Some(chroma);
    output.write().to_buffered(writer)?;
    Ok(())
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
        let mut writer = BufWriter::new(temporary.as_file_mut());
        match extension.as_str() {
            "png" => write_png(&mut writer, image, space)?,
            "tif" | "tiff" => {
                let icc = profile(space)?.icc()?;
                let (rows, big) = tiff_layout(width, height, icc.len());
                if big {
                    write_tiff::<tiff::encoder::TiffKindBig>(
                        &mut writer,
                        image,
                        space,
                        &icc,
                        rows,
                    )?;
                } else {
                    write_tiff::<tiff::encoder::TiffKindStandard>(
                        &mut writer,
                        image,
                        space,
                        &icc,
                        rows,
                    )?;
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
                let mut encoder =
                    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 95);
                encoder.set_icc_profile(profile(space)?.icc()?)?;
                encoder.write_image(&rgb, width, height, ExtendedColorType::Rgb8)?;
            }
            "exr" => {
                ensure!(
                    space == OutputSpace::LinearSrgb,
                    "EXR stores linear sRGB; select --color-space linear-srgb"
                );
                write_linear_exr(image, &mut writer, SRGB_CHROMATICITIES)?;
            }
            _ => bail!("Export extension must be png, jpg, tif, tiff or exr"),
        }
        // Dropping BufWriter ignores flush failures. Publish only after every
        // encoded byte has reached the temporary file successfully.
        writer.flush().context("Flushing encoded image")?;
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
    let profile = monitor
        .map(|path| crate::display::Icc::from_bytes(std::fs::read(path)?))
        .transpose()?;
    crate::display::Encoder::default().encode(image, profile.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tiff::{decoder::DecodingResult, encoder::TiffKind, tags::Tag};

    #[test]
    fn png_stream_preserves_wide_rows_color_alpha_and_partial_batches() {
        for (width, height) in [(3001, 100), (100_003, 3)] {
            let image = Rendered {
                width,
                height,
                pixels: (0..width * height)
                    .map(|i| {
                        let t = (i % 257) as f32 / 256.;
                        [t * 1.5 - 0.125, 0.25, 1. - t, (i % 3) as f32 / 2.]
                    })
                    .collect(),
            };
            for space in [
                OutputSpace::Srgb,
                OutputSpace::LinearSrgb,
                OutputSpace::DisplayP3,
                OutputSpace::AdobeRgb,
                OutputSpace::Rec2020,
            ] {
                let mut file = Vec::new();
                write_png(&mut file, &image, space).unwrap();
                let reader = png::Decoder::new(Cursor::new(&file)).read_info().unwrap();
                assert_eq!(reader.info().bit_depth, png::BitDepth::Sixteen);
                assert_eq!(reader.info().color_type, png::ColorType::Rgba);
                let icc = reader.info().icc_profile.as_ref().unwrap();
                let mut expected_icc = profile(space).unwrap().icc().unwrap();
                assert_eq!(icc.len(), expected_icc.len());
                expected_icc[24..36].copy_from_slice(&icc[24..36]);
                assert_eq!(icc.as_ref(), expected_icc);
                let decoded = image::ImageReader::new(Cursor::new(&file))
                    .with_guessed_format()
                    .unwrap()
                    .decode()
                    .unwrap()
                    .to_rgba16();
                assert_eq!(decoded.dimensions(), (width as u32, height as u32));
                let matrix = space.matrix();
                for (actual, pixel) in decoded
                    .as_raw()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(&image.pixels)
                {
                    let rgb = color::apply(matrix, [pixel[0], pixel[1], pixel[2]]);
                    let expected = [
                        space.encode(rgb[0]),
                        space.encode(rgb[1]),
                        space.encode(rgb[2]),
                        pixel[3],
                    ]
                    .map(|v| (v.clamp(0., 1.) * 65535. + 0.5) as u16);
                    assert_eq!(*actual, expected);
                }
            }
        }
    }

    #[test]
    fn png_stream_and_final_chunk_write_failures_are_reported() {
        struct RejectChunk {
            bytes: Vec<u8>,
            rejected: bool,
            marker: &'static [u8],
        }
        impl Write for RejectChunk {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if bytes == self.marker {
                    self.rejected = true;
                    return Err(std::io::Error::other("PNG chunk write failed"));
                }
                self.bytes.write(bytes)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        for (width, height, marker) in [(2, 2, &b"IEND"[..]), (100_003, 3, &b"IDAT"[..])] {
            let image = Rendered {
                width,
                height,
                pixels: (0..width * height)
                    .map(|i| {
                        let t = (i % 257) as f32 / 256.;
                        [t, 0.25, 1. - t, (i % 3) as f32 / 2.]
                    })
                    .collect(),
            };
            let original = image.pixels.clone();
            let mut sink = RejectChunk {
                bytes: Vec::new(),
                rejected: false,
                marker,
            };
            let error = write_png(&mut sink, &image, OutputSpace::Srgb).unwrap_err();
            assert!(sink.rejected);
            if marker == b"IEND" {
                assert!(sink.bytes.windows(4).any(|b| b == b"IDAT"));
            }
            assert!(error.to_string().contains("PNG chunk write failed"));
            assert_eq!(image.pixels, original);
        }
    }

    #[test]
    fn tiff_selects_64_bit_offsets_before_classic_size_overflows() {
        assert!(!tiff_layout(8736, 11648, 4096).1);
        assert!(!tiff_layout(100_003, 5368, 4096).1);
        assert!(tiff_layout(100_003, 5369, 4096).1);
        // An otherwise fitting raster must account for its ICC payload too.
        assert!(tiff_layout(100_003, 5368, 1_048_576).1);
        assert!(tiff_layout(u32::MAX, u32::MAX, 4096).1);
    }

    fn roundtrip<K: TiffKind>(width: usize, height: usize, version: u16) {
        let image = Rendered {
            width,
            height,
            pixels: (0..width * height)
                .map(|i| {
                    let t = (i % 257) as f32 / 256.;
                    [t, 0.25, 1. - t, (i % 3) as f32 / 2.]
                })
                .collect(),
        };
        let space = OutputSpace::DisplayP3;
        let icc = profile(space).unwrap().icc().unwrap();
        let (rows, _) = tiff_layout(width as u32, height as u32, icc.len());
        let mut file = Cursor::new(Vec::new());
        write_tiff::<K>(&mut file, &image, space, &icc, rows).unwrap();
        let bytes = file.into_inner();
        let native_version = if &bytes[..2] == b"II" {
            u16::from_le_bytes([bytes[2], bytes[3]])
        } else {
            assert_eq!(&bytes[..2], b"MM");
            u16::from_be_bytes([bytes[2], bytes[3]])
        };
        assert_eq!(native_version, version);
        let mut decoder = tiff::decoder::Decoder::new(Cursor::new(&bytes)).unwrap();
        assert_eq!(decoder.dimensions().unwrap(), (width as u32, height as u32));
        assert_eq!(decoder.get_tag_u8_vec(Tag::IccProfile).unwrap(), icc);
        assert_eq!(decoder.get_tag_u16_vec(Tag::ExtraSamples).unwrap(), [2]);
        assert_eq!(
            decoder.get_tag_u64_vec(Tag::StripOffsets).unwrap().len(),
            height.div_ceil(rows as usize)
        );
        let DecodingResult::U16(decoded) = decoder.read_image().unwrap() else {
            panic!("Expected RGBA16");
        };
        assert_eq!(decoded.len(), image.pixels.len() * 4);
        let matrix = space.matrix();
        for (got, p) in decoded.as_chunks::<4>().0.iter().zip(&image.pixels) {
            let rgb = color::apply(matrix, [p[0], p[1], p[2]]);
            let expected = [
                space.encode(rgb[0]),
                space.encode(rgb[1]),
                space.encode(rgb[2]),
                p[3],
            ]
            .map(|v| (v.clamp(0., 1.) * 65535. + 0.5) as u16);
            assert_eq!(*got, expected);
        }
        let independent =
            image::ImageReader::with_format(Cursor::new(&bytes), image::ImageFormat::Tiff)
                .decode()
                .unwrap()
                .to_rgba16();
        assert_eq!(independent.as_raw(), &decoded);
    }

    #[test]
    fn tiff_variants_preserve_wide_and_partial_strips_with_color_and_alpha() {
        for (width, height) in [(3001, 100), (100_003, 3)] {
            roundtrip::<tiff::encoder::TiffKindStandard>(width, height, 42);
            roundtrip::<tiff::encoder::TiffKindBig>(width, height, 43);
        }
    }
}
