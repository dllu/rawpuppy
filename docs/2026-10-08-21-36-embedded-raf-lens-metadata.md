# Structured embedded RAF lens metadata

Raw input now retains the lens model, focal length and aperture from decoded EXIF,
and independently reads Fujifilm's embedded distortion, red/blue CA and vignetting
tables from the RAF's second TIFF. It uses Rawler's existing TIFF parser rather
than another pixel decoder or an external application dependency.

The table header is interpreted as reference pixel radius and knot count. It is
not an ordinary rational value. On the copied GFX100S example this yields 7,280
pixels, exactly the cropped 11,648×8,736 sensor's half-diagonal. All four curves
contain nine knots and match the values independently read by ExifTool. Lens
identity is GF55mm F1.7 R WR, 55 mm at f/1.7. No serial number is retained.

Metadata parsing validates lengths, denominator validity, strictly increasing
positive radii, and the duplicated CA header. Two new tests cover header/channel
interpretation and malformed inputs. All 28 default tests and Rust 1.99 strict
Clippy pass. The real 103 MP source was decoded and inspected with the new parser.

This milestone exposes validated manufacturer data without changing rendering
or edit recipes. Correction direction, value units, interpolation and composed
CPU/GPU application remain to be validated and implemented. An isolated Darktable
session is being used as an output oracle; its source code is not used.

Test photographs, metadata, presets and display configuration remain under
`/tmp/rawpuppy-validation`. No contents of `~/pictures/raw` changed. The preceding
Qwen comparison commit passed all desktop/native Moebius CI jobs.
