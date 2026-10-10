# Color interpretation and photographic AgX

The working representation is linear sRGB/Rec.709 with D65 white. It is a
coordinate basis, not a clipping boundary: negative RGB and HDR values are valid.
ICC-tagged raster inputs use Little CMS. EXR inputs use their declared RGB
chromaticities and white point; normalized primary matrices and Bradford
adaptation convert them to the working basis. Untagged EXR defaults to linear
sRGB. EXR export writes explicit sRGB chromaticities and full-precision linear
RGBA, preserving negative values and highlights.
TIFF import accepts 16-, 32- and 64-bit floating-point samples and converts them
to the working float32 representation. Untagged float TIFFs assume linear sRGB;
tagged TIFFs honor their ICC profile. Integer TIFFs keep the existing sRGB
fallback. Signed values and highlights survive exposure editing and EXR export;
non-finite or float32-overflowing values are rejected. Three-channel TIFF imports
reuse their normalized RGB allocation instead of creating another full raster.
PNG export converts fixed blocks into two reusable 1 MiB byte buffers, overlapping
conversion with streaming compression. The codec's filtering rows still scale
with image width. ICC tagging, RGBA16 quantization and straight alpha are retained;
the data stream and final PNG chunk are explicitly finished with error propagation.
At 101.8 MP, the measured export probe's peak RSS fell from 3.30 to 1.52 GiB;
[the PNG journal](2026-10-10-03-15-streaming-png-export.md) records timing and
complete decoded-pixel verification, including an independent LibPNG reader.
RGBA16 TIFF exports explicitly identify their fourth channel as unassociated
(straight) alpha, alongside the selected ICC profile. Transparent and partial
alpha retain their independently stored RGB values.
TIFF conversion reuses roughly 1 MiB of strip scratch (or one row if wider),
instead of allocating a complete second raster. Small exports retain classic
TIFF; a conservative bound including pixel data, ICC and strip metadata selects
BigTIFF before classic 32-bit offsets overflow. Both variants preserve the same
color conversion, quantization and straight-alpha metadata. A real
100,003 × 5,371 RGBA16 export exceeded 4 GiB and passed selected-strip decoding
with both Rust and system LibTIFF, including a strip offset beyond 32 bits; see
[the large TIFF journal](2026-10-10-02-33-large-tiff-export.md).

Grayscale PNG/TIFF ICC profiles use Little CMS's gray float input format before
conversion to working RGB. Their declared tone curve and white point are honored.
The shared importer validates profile/raster compatibility; a gray profile on a
color raster is rejected. Gray expansion uses a bounded 64 KiB workspace, with
100,003-pixel-wide 16-bit PNG/TIFF regression fixtures and unchanged source bytes.

New recipes select `agx_sdr_v1`. Its photographic formation is a 97³ lattice of
independent numeric observations of Darktable's `blender-like|base` preset,
sampled using synthetic linear Rec.2020 probes. No Darktable source or lookup
table was copied into the application. Rust CPU and GPU implementations perform
the interpolation; Darktable is only used offline as an oracle.

The input shaper includes zero and puts 18% gray exactly on a lattice node. It
allocates half the coordinates to shadows and half to 16 stops above gray. Four
vertices define each tetrahedral interpolation. The GPU retains its lattice
between edits, and applies formation in the existing composed output kernel.
Observations are stored in linear Rec.2020, then expressed in the working basis;
wide-gamut coordinates remain available for output-space conversion. Inputs
outside Rec.2020 move toward the neutral axis while preserving luminance, a
deliberate difference from the oracle's handling of extreme out-of-gamut data.

The historical `agx` recipe value retains the earlier polynomial exactly. The
editor identifies that legacy mode; selecting AgX changes to `agx_sdr_v1` and
invalidates fills made from the preceding rendering. Tagged HDR sources whose
colors were interpreted incorrectly by older versions also invalidate those
fills using a source color revision.

The numeric lattice and its provenance are in `src/data`. Reproduction uses
`tools/bake_agx.py --darktable /path/to/darktable-cli --preset /path/to/agx-default.xmp
--output /path/to/result.f32`. This uses an isolated configuration and database,
eight workers, float EXR and only generated test images. The `color_oracle`
example generates separate sweeps and deterministic random probes; `--reference`
compares independent output and `--golden` records regression samples.

The measured fixture contains 11,308 probes. Of these, 11,051 lie in Rec.2020;
the remaining 257 intentionally exercise the different gamut-boundary policy.
The 1,677 stored regression samples include random colors between shadows and
HDR highlights. The largest observed linear-channel error in the in-gamut
probes is about 0.0079. The sampled sRGB-display Oklab distance ×100 has mean
0.0206, 99th percentile 0.118 and maximum 0.439. These describe these fixtures,
not universal perceptual bounds. A 129³ experiment improves most errors but
requires more than twice the storage; the 97³ lattice is the current choice.

Native HDR presentation and automatic monitor profile discovery are implemented
and have platform signal checks; physical colorimetry and display transitions
remain to verify. GFX and further Canon/Sony camera comparisons are recorded in
the journals, with crop/reconstruction/signed-noise limits on their interpretation.
The [Canon/Sony check](2026-10-10-03-40-canon-sony-camera-validation.md) includes
a reproducible normalization diagnostic and a high-ISO oracle discrepancy.
Further [controlled Bayer DNG checks](2026-10-10-04-00-controlled-dng-color.md)
separate constant-color calibration agreement from signed-noise behavior.
Float HDR interchange and SDR presentation on an HDR desktop are distinct
capabilities.
