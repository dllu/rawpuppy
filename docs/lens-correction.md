# Camera lens correction

New documents use available embedded Fujifilm distortion and vignetting data.
The editor's Geometry & lens controls show the lens identity and let either
correction be disabled. Manual distortion and vignetting remain adjustments to
the same composed map and gain. The original sensor buffer is unchanged.

Saved recipes explicitly record `lens.mode = "embedded_v1"`. Recipes without the
new field retain their previous rendering with camera corrections off. The default
off field is omitted when serializing, preserving historical recipe hashes and
saved synthesis layers. Selecting camera corrections changes that hash and
requires regeneration of preceding-edit-dependent fills. Compare retains geometry
and camera corrections, keeping the same framing while comparing other edits.

## Metadata and conventions

RAF's second TIFF supplies FujiIFD tags `0xf00b` (distortion), `0xf00f` (CA), and
`0xf010` (vignetting). Rawler parses the TIFF; Rawpuppy validates table shape,
denominators and nonnegative, increasing radius samples. The first rational's
numerator is a reference pixel radius and its denominator is the knot count.
The apparent floating-point quotient is not the physical reference radius.

On the GFX100S/GF55mm F1.7 sample, that radius is 7,280 pixels: half the diagonal
of the cropped 11,648×8,736 sensor. All four tables have nine knots and match
ExifTool's independently read values. Distortion uses a backward radial factor
`1 + percent / 100`. Vignetting uses a scene-linear gain of
`100 / transmission_percent`, evaluated at the sampled sensor position.

Values are linearly interpolated at the recorded radii. When no central knot is
present, distortion starts at zero and transmission at 100 percent. An explicit
zero-radius knot is supported. Outside the recorded interval the endpoint value
is retained. An image's physical half-diagonal scales the radius relative to the
table's reference; orientation does not change that physical distance.

The current implementation assumes the optical center is the cropped image's
center. Off-center in-camera crops and other camera/lens combinations need further
validation. Embedded CA tables are retained but are not automatically applied.
Their output comparison couples small channel shifts with the oracle's framing
and calibration; manual red/blue fringe correction remains available.

## Composition and framing

A 4,097-entry, two-component lookup stores radial scale and linear gain on a
uniform squared-radius grid. It uses about 32 KiB and is shared by CPU and GPU.
The geometry composes camera distortion with the pinhole map, manual radial model,
CA, orientation and sensor crop. Camera vignetting multiplies the existing scalar
exposure/vignette/graduated-filter gain before tone mapping. No additional photo
raster is created. GPU data is appended to the existing parameter buffer, retaining
the eight-storage-buffer interface used by the photographic AgX kernel.

Automatic camera framing derives the greatest uniform scale whose compiled radial
map keeps rectangle boundaries within the input. It accounts for all LUT nodes
between the short-side midpoint and corner radii, including small extrema rather
than assuming a monotone coefficient. A floating-point margin keeps boundary
roundoff inside the sensor. Perspective and rotation may still create gaps for
cropping or synthesis; the camera setting does not automatically crop those edits.

## Validation

Four new tests cover known-coordinate maps and gains, barrel/pincushion boundary
coverage, actual GFX curve samples and invalid data, and historical saved-fill
hashes. CUDA and Vulkan parity explicitly include camera correction alongside
manual geometry, scene and local edits, all EXIF orientations and the
100,003-pixel-wide case.
Native Metal parity also passed on macOS CI with the same camera-correction cases.

An isolated Darktable session supplied separate distortion, vignetting and CA
presets through its UI. No Darktable source was read for the correction algorithm.
Across 4,482 matched features, the direct distortion sign gave a median residual
of 0.068 pixels after fitting a uniform framing scale and translation, versus
1.893 without radial correction and 3.786 with the opposite sign. That fit verifies
the shape/direction, not identical framing or decoder quality.

Independent 1,200×1,600 linear exports compared each editor's corrected/neutral
vignetting gain. The mean absolute gain difference was 0.000368, with a 99th
percentile of 0.000986; sampled annulus medians agree closely. The maximum difference
was 0.00899, including reconstruction/downsampling and dark-edge effects. This is
one photograph/lens/aperture, not a general calibration accuracy bound.
The numerical records and preset identities are retained in
[the oracle data](data/lens-gfx-oracle-2026-10-08.json).

The real editor selected both corrections automatically, changed the preview when
vignetting was disabled, saved its explicit choices, reopened with the same state,
and closed normally. Corrected 1,800-pixel previews averaged 12.78 ms warm on CUDA
and 12.04 ms on Vulkan in the shared GB10 workstation. Benchmarks include pipeline
composition and readback, not display encoding or presentation; cold setup remains
separate. All temporary photographs and oracle data remain under
`/tmp/rawpuppy-validation`; the original photograph directory was not changed.
A full corrected 8,736×11,648, 16-bit sRGB TIFF also exported through CUDA. The
observed render/export times were 2.36/0.94 seconds on this workstation.

Tag identifiers are documented by [ExifTool](https://exiftool.org/TagNames/FujiFilm.html).
The application uses [Rawler's TIFF API](https://docs.rs/rawler/0.8.0/rawler/formats/tiff/).
