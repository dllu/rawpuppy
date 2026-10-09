# Camera lens correction

New documents use available embedded Fujifilm distortion, vignetting and lateral
chromatic-aberration data.
The editor's Geometry & lens controls show the lens identity and let either
correction be disabled. Manual distortion, fringe scales and vignetting remain adjustments to
the same composed map and gain. The original sensor buffer is unchanged.

Saved recipes explicitly record `lens.mode = "embedded_v1"`. Recipes without the
new field retain their previous rendering with camera corrections off. The default
off field is omitted when serializing, preserving historical recipe hashes and
saved synthesis layers. Existing embedded recipes without the new
`lens.chromatic_aberration` flag keep CA off; false is omitted during serialization,
preserving their hashes too. New documents explicitly enable CA when both camera
channel curves are available. Selecting camera corrections changes that hash and
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
Red and blue CA use fractional backward scales `1 + coefficient`, with green
as the reference. Their small values are not divided by 100. Channel maps are
composed in camera RGB before the calibration matrix mixes channels.

Values are linearly interpolated at the recorded radii. When no central knot is
present, distortion and CA start at zero and transmission at 100 percent. An explicit
zero-radius knot is supported. Outside the recorded interval the endpoint value
is retained. An image's physical half-diagonal scales the radius relative to the
table's reference; orientation does not change that physical distance.

The current implementation assumes the optical center is the cropped image's
center. Off-center in-camera crops and other camera/lens combinations need further
validation. CA scales must remain positive; malformed or missing selected curves
are rejected before rendering. Manual red/blue fringe scales multiply the compiled
camera maps, so each channel samples the original through one composed map.

## Composition and framing

A 4,097-entry, four-component lookup stores the green map, linear gain, red map
and blue map on a uniform squared-radius grid. It uses about 64 KiB and is shared
by CPU and GPU.
The geometry composes camera distortion with the pinhole map, manual radial model,
CA, orientation and sensor crop. Camera vignetting multiplies the existing scalar
exposure/vignette/graduated-filter gain before tone mapping. No additional photo
raster is created. GPU data is appended to the existing parameter buffer, retaining
the eight-storage-buffer interface used by the photographic AgX kernel.

Automatic camera framing derives the greatest uniform scale whose compiled radial
maps keep all three channel boundaries within the input. It accounts for all LUT nodes
between the short-side midpoint and corner radii, including small extrema rather
than assuming a monotone coefficient. A floating-point margin keeps boundary
roundoff inside the sensor. Perspective and rotation may still create gaps for
cropping or synthesis; the camera setting does not automatically crop those edits.

## Validation

Eight lens tests cover known-coordinate maps and gains, barrel/pincushion boundary
coverage for every channel, actual GFX curve samples and invalid data, historical
saved-fill hashes and legacy embedded-recipe hashes. CUDA and Vulkan parity explicitly include camera correction alongside
manual geometry, scene and local edits, all EXIF orientations and the
100,003-pixel-wide case.
The CA-inclusive native Metal cases passed in macOS
[CI run 37933420194](https://github.com/dllu/rawpuppy/actions/runs/37933420194).

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

### CA unit and direction experiment

The original GF55 coefficients induce subpixel shifts at the 1,200×1,600 oracle
export grid. Ordinary temporary RAF copies amplify only the 18 CA rational
numerators by +40, −40, or zero. Their sensor payload and every byte outside that
table remain identical to the input copy. Each variant is exported with the same
UI-derived CA-only preset, disabled tone workflow, and linear Rec.2020 output.
No Darktable source is used.

SIFT matches the zero-coefficient image against each amplified image. Converting
the exports to an approximate camera basis using Rawpuppy's calibration separates
channel motion better than matching the mixed working RGB channels. Each fit
allows common radial framing and translation. Robust feature selection is shared
by the competing fixed-unit models, rather than discarding different outliers
for each hypothesis.

| Variant / channel | Features | Direct fractional scale median residual / pixels | Dividing coefficient by 100 | Opposite fractional sign |
| --- | ---: | ---: | ---: | ---: |
| +40 / red | 7,879 | 0.037 | 0.190 | 0.402 |
| +40 / blue | 7,163 | 0.050 | 0.284 | 0.597 |
| −40 / red | 7,066 | 0.046 | 0.198 | 0.418 |
| −40 / blue | 7,845 | 0.041 | 0.279 | 0.583 |

Unconstrained fitted metadata multipliers are approximately +38.6/+35.0 and
−38.7/−35.5 for red/blue. These measurements support fractional units and the
direct backward-map sign. Approximate camera conversion, resampling and curve
interpolation still differ; they do not establish identical renderer output or
a universal CA calibration bound. Full identities, hypotheses and residuals are
in [the CA record](data/lens-ca-gfx-oracle-2026-10-09.json).

The earlier distortion/vignetting session selected both corrections automatically, changed the preview when
vignetting was disabled, saved its explicit choices, reopened with the same state,
and closed normally. Corrected 1,800-pixel previews averaged 12.78 ms warm on CUDA
and 12.04 ms on Vulkan in the shared GB10 workstation. Benchmarks include pipeline
composition and readback, not display encoding or presentation; cold setup remains
separate. All temporary photographs and oracle data remain under
`/tmp/rawpuppy-validation`; the original photograph directory was not changed.
A full corrected 8,736×11,648, 16-bit sRGB TIFF also exported through CUDA. The
observed render/export times were 2.36/0.94 seconds on this workstation.

With embedded CA added, a fresh native GFX document enables all three camera
corrections. Saving CA on records the flag explicitly; saving it off omits the
flag and reopening correctly retains off. Both tested editor sessions closed
normally. A full CA-corrected 8,736×11,648, 16-bit sRGB TIFF rendered in 0.57 seconds
and encoded in 0.87 seconds; every alpha sample is 65,535 and its ICC is valid.
The input copy's SHA-256 remains unchanged.

An isolated eight-iteration CUDA system-memory preview comparison at a maximum
edge of 1,800 pixels averaged 10.99 ms warm with CA, versus 4.81 ms without CA.
Only CA is toggled; both retain camera distortion/vignetting, framing and the same
exposure sequence. Each excludes its first call of about 0.4 seconds. The
benchmark's `--no-camera-ca` flag reproduces the latter configuration. These are
shared-workstation pipeline observations, excluding display encoding/presentation.
They do not promise constant latency across machines or system load.

Tag identifiers are documented by [ExifTool](https://exiftool.org/TagNames/FujiFilm.html).
The application uses [Rawler's TIFF API](https://docs.rs/rawler/0.8.0/rawler/formats/tiff/).
