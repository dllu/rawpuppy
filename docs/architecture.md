# Pipeline and invariants

The immutable original feeds a normalized floating-point sensor buffer. Sensor
normalization subtracts the repeating black level and divides by the sensor
range. Negative noise samples and values above sensor white are retained.
Hot pixels and same-color bilateral denoising run before demosaicing. Bayer
reconstruction uses independently implemented Malvar–He–Cutler 5×5 filters;
X-Trans currently uses a color-aware weighted interpolation fallback.

Every output sample traverses a compiled backward map: normalized crop, pinhole
homography (yaw/pitch/roll and focal length), radial distortion, lateral color
aberration, EXIF orientation, and sensor crop. Trigonometric operations and matrix
composition occur when the recipe changes, never per pixel. Sampling reconstructs
sensor RGB on demand. A preview samples directly from the original at its output
resolution; it does not require a 100 MP intermediate RGB image.

Vignetting, graduated density, and exposure combine into one scalar gain.
As-shot sensor gains, camera calibration, user gains, and the RGB mixer combine
into one matrix. XYZ white points are adapted with Bradford where needed.
The working space is scene-linear sRGB/Rec.709 with D65 white, with unbounded
highlights. Camera values are not clipped before tone mapping.

AgX provides an analytic compact approximation (inset, logarithmic encoding,
sixth-order contrast approximation, outset, linearization); it is not the current
Blender 3D LUT variant. The alternative linear mode preserves scene values for
HDR interchange. Clone/heal reads the tone-mapped source, followed by a monotone
cubic curve in perceptual sRGB coordinates and split toning in display-linear
RGB. Neural synthesis belongs after those local edits. Finally, output conversion
changes primaries and applies the correct transfer function. Little CMS generates
matching ICC profiles and interprets embedded RGB input profiles.

Sidecars use their own `https://rawpuppy.org/ns/1.0/` XMP namespace and full
original filenames plus `.rawpuppy.xmp`. They contain a versioned JSON recipe in
an RDF property. Reads verify the namespace and reject unknown versions or invalid
parameters. Writes stage in the destination directory and rename atomically.

Dimensions and indices are `usize`; checked multiplication detects address-space
overflow. Export narrows dimensions only to the actual file format's representation.
JPEG's 65535-pixel limit is a format constraint. PNG/TIFF/EXR do not inherit that
constraint. Decoder allocation limits are removed, leaving system memory and core
format constraints. Export never replaces the original path, including aliases.

No Darktable source has been copied. Its local checkout can be used to generate
comparison outputs as an independent oracle.
