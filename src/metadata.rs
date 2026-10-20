//! Photographic EXIF directories, independent of source raster layout and byte order.
use anyhow::{Result, ensure};
use rawler::formats::tiff::{IFD, Value};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    fmt,
    io::{Cursor, Read, Seek, Write},
    sync::Arc,
};
use tiff::{
    encoder::{DirectoryEncoder, TiffEncoder, TiffKind, TiffValue},
    tags::{ByteOrder, Tag, Type, ValueBuffer},
};

type Fields = BTreeMap<u16, Arc<ValueBuffer>>;
#[derive(Clone, Default)]
pub struct Capture {
    root: Fields,
    exif: Fields,
    gps: Fields,
    interop: Fields,
}
impl fmt::Debug for Capture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Capture")
            .field("root_fields", &self.root.len())
            .field("exif_fields", &self.exif.len())
            .field("gps_fields", &self.gps.len())
            .finish()
    }
}
fn root_tag(tag: u16) -> bool {
    matches!(tag, 269..=272 | 305..=306 | 315 | 33432 | 40091..=40095 | 18246 | 18249)
}
fn exif_tag(tag: u16) -> bool {
    // Offset-bearing maker notes/thumbnails and coordinates of old pixels are
    // not portable to a newly rendered image. Keep the photographic fields.
    !matches!(tag, 37500 | 40965 | 513..=514 | 37121..=37122 | 37396 | 41492)
}
struct Bytes<const ASCII: bool>(Vec<u8>);
impl<const ASCII: bool> TiffValue for Bytes<ASCII> {
    const BYTE_LEN: u8 = 1;
    const FIELD_TYPE: Type = if ASCII { Type::ASCII } else { Type::UNDEFINED };
    fn count(&self) -> usize {
        self.0.len()
    }
    fn data(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.0)
    }
}
fn buffer(value: &Value) -> Option<Arc<ValueBuffer>> {
    macro_rules! primitive {
        ($v:expr) => {
            ValueBuffer::from_value(&$v.as_slice())
        };
    }
    Some(Arc::new(match value {
        Value::Byte(v) => primitive!(v),
        Value::Short(v) => primitive!(v),
        Value::Long(v) => primitive!(v),
        Value::SByte(v) => primitive!(v),
        Value::SShort(v) => primitive!(v),
        Value::SLong(v) => primitive!(v),
        Value::Float(v) => primitive!(v),
        Value::Double(v) => primitive!(v),
        Value::Ascii(v) => ValueBuffer::from_value(&Bytes::<true>(v.as_vec_with_nul())),
        Value::Undefined(v) => ValueBuffer::from_value(&Bytes::<false>(v.clone())),
        Value::Rational(v) => ValueBuffer::from_value(
            &v.iter()
                .map(|r| tiff::encoder::Rational { n: r.n, d: r.d })
                .collect::<Vec<_>>()
                .as_slice(),
        ),
        Value::SRational(v) => ValueBuffer::from_value(
            &v.iter()
                .map(|r| tiff::encoder::SRational { n: r.n, d: r.d })
                .collect::<Vec<_>>()
                .as_slice(),
        ),
        Value::Unknown(..) => return None,
    }))
}
fn fields(ifd: &IFD, keep: impl Fn(u16) -> bool) -> Fields {
    ifd.entries()
        .iter()
        .filter(|(t, _)| keep(**t))
        .filter_map(|(t, e)| buffer(&e.value).map(|v| (*t, v)))
        .collect()
}
fn put<T: TiffValue>(fields: &mut Fields, tag: u16, value: T) {
    fields.insert(tag, Arc::new(ValueBuffer::from_value(&value)));
}
fn write_fields<W: Write + Seek, K: TiffKind>(
    out: &mut DirectoryEncoder<'_, W, K>,
    fields: &Fields,
) -> Result<()> {
    for (tag, value) in fields {
        out.write_tag_buf(Tag::from_u16_exhaustive(*tag), value)?;
    }
    Ok(())
}
impl Capture {
    pub fn from_ifd(root: &IFD) -> Self {
        use rawler::tags::ExifTag;
        let exif = root.get_sub_ifd(ExifTag::ExifOffset);
        Self {
            root: fields(root, root_tag),
            exif: exif.map(|d| fields(d, exif_tag)).unwrap_or_default(),
            gps: root
                .get_sub_ifd(ExifTag::GPSInfo)
                .map(|d| fields(d, |_| true))
                .unwrap_or_default(),
            interop: exif
                .and_then(|d| d.get_sub_ifd(40965u16))
                .map(|d| fields(d, |_| true))
                .unwrap_or_default(),
        }
    }
    pub fn from_exif(bytes: &[u8]) -> Result<Self> {
        use rawler::formats::tiff::{GenericTiffReader, reader::TiffReader};
        let bytes = bytes.strip_prefix(b"Exif\0\0").unwrap_or(bytes);
        let tiff = GenericTiffReader::new_with_buffer(bytes, 0, 0, None)?;
        Ok(Self::from_ifd(tiff.root_ifd()))
    }
    fn from_jpeg(bytes: &[u8]) -> Result<Self> {
        use rawler::formats::{
            jfif::Jfif,
            tiff::{GenericTiffReader, reader::TiffReader},
        };
        let jpeg = Jfif::parse(Cursor::new(bytes))?;
        let Some(root) = jpeg.exif_ifd() else {
            return Ok(Self::default());
        };
        // Jfif's convenience IFD reader follows only EXIF/SubIFD links. Re-read
        // with the generic TIFF reader so GPS directories are included as well.
        let tiff = GenericTiffReader::new_with_buffer(bytes, root.base, 0, None)?;
        Ok(Self::from_ifd(tiff.root_ifd()))
    }
    pub fn from_raw(
        metadata: Option<&rawler::decoders::RawMetadata>,
        source: &rawler::rawsource::RawSource,
    ) -> Result<Self> {
        use rawler::{
            formats::tiff::{GenericTiffReader, reader::TiffReader, writer::TiffWriter},
            tags::ExifTag,
        };
        let mut capture = Self::default();
        if let Some(metadata) = metadata {
            let mut bytes = Cursor::new(Vec::new());
            let mut tiff = TiffWriter::new(&mut bytes)?;
            let mut root = tiff.new_directory();
            let mut exif = tiff.new_directory();
            metadata.write_exif_tags(&mut tiff, &mut root, &mut exif)?;
            root.add_tag(ExifTag::Make, metadata.make.as_str());
            root.add_tag(ExifTag::Model, metadata.model.as_str());
            // A valid RAW need not contain photographic EXIF fields.
            if !exif.is_empty() {
                let offset = exif.build(&mut tiff)?;
                root.add_tag(ExifTag::ExifOffset, [offset]);
            }
            tiff.build(root)?;
            capture = Self::from_exif(bytes.get_ref())?;
        }
        // Generic directories supplement decoder-normalized fields (including
        // subsecond timestamps, GPS, focal-length equivalent and user comments).
        let direct = if source.subview(0, 8)? == b"FUJIFILM" {
            let h = source.subview(84, 8)?;
            let p = u32::from_be_bytes(h[..4].try_into()?) as u64;
            let n = u32::from_be_bytes(h[4..].try_into()?) as u64;
            Self::from_jpeg(source.subview(p, n)?).ok()
        } else {
            GenericTiffReader::new_with_buffer(source.buf(), 0, 0, None)
                .ok()
                .map(|t| Self::from_ifd(t.root_ifd()))
        };
        if let Some(d) = direct {
            capture.root.extend(d.root);
            capture.exif.extend(d.exif);
            capture.gps.extend(d.gps);
            capture.interop.extend(d.interop);
        }
        Ok(capture)
    }
    /// Read classic TIFF and BigTIFF without interpreting or copying their pixels.
    pub fn from_tiff(reader: impl Read + Seek) -> Result<Self> {
        let mut limits = tiff::decoder::Limits::default();
        limits.ifd_value_size = usize::MAX;
        limits.decoding_buffer_size = usize::MAX;
        limits.intermediate_buffer_size = usize::MAX;
        let mut decoder = tiff::decoder::Decoder::new(reader)?.with_limits(limits);
        let mut capture = Self::default();
        let root = decoder
            .ifd_pointer()
            .ok_or_else(|| anyhow::anyhow!("Missing image directory"))?;
        fn read<W: Read + Seek>(
            decoder: &mut tiff::decoder::Decoder<W>,
            ptr: tiff::tags::IfdPointer,
            keep: impl Fn(u16) -> bool,
        ) -> Result<Fields> {
            let dir = decoder.read_directory(ptr)?;
            let mut out = Fields::new();
            let mut tags = decoder.read_directory_tags(&dir);
            for (tag, _) in dir.iter().filter(|(tag, _)| keep(tag.to_u16())) {
                let mut value = ValueBuffer::empty(Type::BYTE);
                tags.find_tag_buf(tag, &mut value)?;
                value.set_byte_order(ByteOrder::native());
                out.insert(tag.to_u16(), Arc::new(value));
            }
            Ok(out)
        }
        capture.root = read(&mut decoder, root, root_tag)?;
        if let Some(ptr) = decoder
            .find_tag(Tag::ExifDirectory)?
            .map(|v| v.into_u64().map(tiff::tags::IfdPointer))
            .transpose()?
        {
            capture.exif = read(&mut decoder, ptr, exif_tag)?;
            let dir = decoder.read_directory(ptr)?;
            if let Some(ptr) = decoder
                .read_directory_tags(&dir)
                .find_tag(Tag::from_u16_exhaustive(40965))?
                .map(|v| v.into_u64().map(tiff::tags::IfdPointer))
                .transpose()?
            {
                capture.interop = read(&mut decoder, ptr, |_| true)?;
            }
        }
        if let Some(ptr) = decoder
            .find_tag(Tag::GpsDirectory)?
            .map(|v| v.into_u64().map(tiff::tags::IfdPointer))
            .transpose()?
        {
            capture.gps = read(&mut decoder, ptr, |_| true)?;
        }
        Ok(capture)
    }
    pub(crate) fn byte_len(&self) -> usize {
        [&self.root, &self.exif, &self.gps, &self.interop]
            .iter()
            .flat_map(|f| f.values())
            .fold(2048usize, |n, v| {
                n.saturating_add(v.as_bytes().len()).saturating_add(20)
            })
    }
    pub fn for_output(&self, width: u32, height: u32, space: crate::color::OutputSpace) -> Self {
        let mut out = self.clone();
        put(&mut out.root, 256, width);
        put(&mut out.root, 257, height);
        put(&mut out.root, 274, 1u16);
        put(
            &mut out.root,
            305,
            concat!("rawpuppy ", env!("CARGO_PKG_VERSION")),
        );
        put(&mut out.exif, 40962, width);
        put(&mut out.exif, 40963, height);
        if !out.exif.contains_key(&36864) {
            put(&mut out.exif, 36864, Bytes::<false>(b"0232".to_vec()));
        }
        put(
            &mut out.exif,
            40961,
            if space == crate::color::OutputSpace::Srgb {
                1u16
            } else {
                65535u16
            },
        );
        // Interoperability indices describe the original encoding, not capture.
        out.interop.clear();
        out
    }
    pub(crate) fn directories<W: Write + Seek, K: TiffKind>(
        &self,
        encoder: &mut TiffEncoder<W, K>,
    ) -> Result<Vec<(Tag, ValueBuffer)>> {
        let mut pointers = Vec::new();
        for (tag, fields) in [
            (Tag::ExifDirectory, &self.exif),
            (Tag::GpsDirectory, &self.gps),
        ] {
            if fields.is_empty() {
                continue;
            }
            let mut dir = encoder.extra_directory()?;
            write_fields(&mut dir, fields)?;
            let offset = dir.finish_with_offsets()?.offset;
            let value = if <K::OffsetType as TiffValue>::FIELD_TYPE == Type::LONG8 {
                ValueBuffer::from_value(&tiff::encoder::Ifd8(offset.into()))
            } else {
                ValueBuffer::from_value(&offset)
            };
            pointers.push((tag, value));
        }
        Ok(pointers)
    }
    pub(crate) fn root<W: Write + Seek, K: TiffKind>(
        &self,
        dir: &mut DirectoryEncoder<'_, W, K>,
    ) -> Result<()> {
        write_fields(dir, &self.root)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut encoder = TiffEncoder::new(&mut bytes)?;
            let pointers = self.directories(&mut encoder)?;
            let mut root = encoder.image_directory()?;
            self.root(&mut root)?;
            for (tag, offset) in pointers {
                root.write_tag_buf(tag, &offset)?;
            }
            root.finish()?;
        }
        ensure!(
            bytes.get_ref().len() <= u32::MAX as usize,
            "EXIF exceeds the classic TIFF format limit"
        );
        Ok(bytes.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{color::OutputSpace, pipeline::Rendered};
    fn fixture() -> Capture {
        let mut c = Capture::default();
        put(&mut c.root, 271, "Camera maker");
        put(&mut c.root, 272, "Portrait camera");
        put(&mut c.root, 274, 6u16);
        put(&mut c.root, 315, "Photographer");
        put(&mut c.root, 33432, "Copyright photographer");
        put(&mut c.exif, 33434, tiff::encoder::Rational { n: 15, d: 2 });
        put(&mut c.exif, 33437, tiff::encoder::Rational { n: 56, d: 10 });
        put(&mut c.exif, 34855, 100u16);
        put(&mut c.exif, 36867, "2025:09:02 20:48:07");
        put(&mut c.exif, 36881, "+01:00");
        put(&mut c.exif, 37521, "123");
        put(&mut c.exif, 42036, "Example 20mm lens");
        put(&mut c.exif, 41989, 16u16);
        put(
            &mut c.exif,
            37510,
            Bytes::<false>(b"ASCII\0\0\0A preserved comment".to_vec()),
        );
        put(
            &mut c.exif,
            37500,
            Bytes::<false>(b"unsafe maker offsets".to_vec()),
        );
        put(&mut c.gps, 0, [2u8, 3, 0, 0]);
        put(&mut c.gps, 1, "N");
        put(&mut c.gps, 3, "W");
        put(
            &mut c.gps,
            2,
            &[
                tiff::encoder::Rational { n: 51, d: 1 },
                tiff::encoder::Rational { n: 30, d: 1 },
                tiff::encoder::Rational { n: 1, d: 1 },
            ][..],
        );
        // Simulate capture of a standalone, big-endian or little-endian EXIF payload.
        Capture::from_exif(&c.encode().unwrap()).unwrap()
    }
    fn compare_capture(source: &Capture, output: &Capture) {
        for tag in [271, 272, 315, 33432] {
            assert_eq!(source.root[&tag].as_bytes(), output.root[&tag].as_bytes());
        }
        for tag in [
            33434, 33437, 34855, 36867, 36881, 37521, 42036, 41989, 37510,
        ] {
            assert_eq!(
                source.exif[&tag].as_bytes(),
                output.exif[&tag].as_bytes(),
                "tag {tag}"
            );
        }
        for tag in [0, 1, 2, 3] {
            assert_eq!(source.gps[&tag].as_bytes(), output.gps[&tag].as_bytes());
        }
        assert!(!output.exif.contains_key(&37500));
    }
    #[test]
    fn every_supported_exif_export_preserves_capture_and_updates_geometry_and_color() {
        use image::{ImageDecoder, metadata::Orientation};
        let dir = tempfile::tempdir().unwrap();
        let source = fixture();
        let image = Rendered {
            width: 32,
            height: 24,
            pixels: vec![[0.2, 0.3, 0.4, 1.]; 32 * 24],
        };
        for ext in ["jpg", "jpeg", "png", "tif", "tiff"] {
            let path = dir.path().join(format!("edited.{ext}"));
            crate::export::write_with_metadata(
                &path,
                &image,
                OutputSpace::DisplayP3,
                false,
                &source,
            )
            .unwrap();
            let capture = if matches!(ext, "tif" | "tiff") {
                let mut d =
                    tiff::decoder::Decoder::new(std::fs::File::open(&path).unwrap()).unwrap();
                assert_eq!(d.dimensions().unwrap(), (32, 24));
                assert_eq!(d.get_tag_unsigned::<u16>(Tag::Orientation).unwrap(), 1);
                assert!(!d.get_tag_u8_vec(Tag::IccProfile).unwrap().is_empty());
                Capture::from_tiff(std::fs::File::open(&path).unwrap()).unwrap()
            } else {
                let mut d = image::ImageReader::open(&path)
                    .unwrap()
                    .into_decoder()
                    .unwrap();
                assert_eq!(d.dimensions(), (32, 24));
                assert_eq!(d.orientation().unwrap(), Orientation::NoTransforms);
                assert!(d.icc_profile().unwrap().is_some());
                Capture::from_exif(&d.exif_metadata().unwrap().unwrap()).unwrap()
            };
            compare_capture(&source, &capture);
            assert_eq!(capture.exif[&40962].as_bytes(), 32u32.to_ne_bytes());
            assert_eq!(capture.exif[&40963].as_bytes(), 24u32.to_ne_bytes());
            assert_eq!(capture.exif[&40961].as_bytes(), 65535u16.to_ne_bytes());
            // Opening an exported photograph must preserve metadata on the next export.
            let reopened = crate::input::SensorImage::open(&path).unwrap();
            compare_capture(&source, &reopened.metadata.capture);
        }
    }
    #[test]
    fn embedded_jpeg_capture_follows_gps_links_as_well_as_the_exif_directory() {
        use image::ImageEncoder;
        let source = fixture();
        let mut jpeg = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg);
        encoder.set_exif_metadata(source.encode().unwrap()).unwrap();
        encoder
            .encode(&[20, 40, 60], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        compare_capture(&source, &Capture::from_jpeg(&jpeg).unwrap());
    }
    #[test]
    fn bigtiff_metadata_uses_real_subdirectories_without_creating_extra_images() {
        let source = fixture().for_output(2, 1, OutputSpace::Srgb);
        let mut file = Cursor::new(Vec::new());
        {
            let mut tiff =
                TiffEncoder::<_, tiff::encoder::TiffKindBig>::new_generic(&mut file).unwrap();
            let pointers = source.directories(&mut tiff).unwrap();
            let mut image = tiff
                .new_image::<tiff::encoder::colortype::RGB8>(2, 1)
                .unwrap();
            source.root(image.encoder()).unwrap();
            for (tag, offset) in pointers {
                image.encoder().write_tag_buf(tag, &offset).unwrap();
            }
            image.write_data(&[20, 40, 60, 20, 40, 60]).unwrap();
        }
        file.set_position(0);
        let output = Capture::from_tiff(&mut file).unwrap();
        compare_capture(&source, &output);
        file.set_position(0);
        let decoder = tiff::decoder::Decoder::new(file).unwrap();
        assert!(!decoder.more_images());
    }
}
