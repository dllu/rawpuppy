# Three-slot and custom-illuminant DNG profiles

The decoder's ordinary color-matrix map omits ColorMatrix3 and does not preserve
custom illuminant descriptions. The earlier two-profile path therefore could
use the first two matrices for a file whose declared third or custom calibration
should influence color. Added a metadata reader for these extended linear profiles.

It reads ColorMatrix/CameraCalibration/ForwardMatrix slots 1–3 directly from the
root IFD, keeps signature gating and analog balance, and checks required matrix
counts, distinct white points and the three-profile all-or-none forward-matrix
rule. Custom IlluminantData supports both rational xy coordinates and rational
spectral samples in the containing IFD's byte order. Spectra are linearly
interpolated, extended with their endpoint values outside the declared interval,
and integrated at 1 nm against the CIE 1931 2-degree observer over 360–830 nm.
These operations follow the linear model and payload definitions in
[Adobe's DNG specification](https://helpx.adobe.com/content/dam/help/en/photoshop/pdf/DNG_Spec_1_7_1_0.pdf).

Two-profile weighting retains reciprocal correlated temperature. The independent
three-profile policy uses convex barycentric weights in CIE 1960 uv; outside the
triangle it uses the nearest boundary segment. Degenerate geometry preserves
exact vertices and favors shorter tied segments. The DNG three-profile model
does not mandate the same reciprocal-temperature interpolation used for exactly
two profiles; this is the application's explicit interpolation choice. The
as-shot neutral/white-point iteration is damped and bounded, and output remains
the existing composed linear calibration matrix rather than a pixel pass.

The observer's published numerical data is sourced from
[Colour Developers' table](https://github.com/colour-science/colour/blob/develop/colour/colorimetry/datasets/cmfs.py)
and attributed to [CIE's standard-observer dataset](https://cie.co.at/datatable/cie-1931-colour-matching-functions-2-degree-observer).
The full BSD-3-Clause notice is retained in
[colour-robertson.txt](licenses/colour-robertson.txt). Rust payload parsing,
integration and interpolation code is independent; no Darktable implementation
source was used. Source and embedded-table hashes are recorded.

Extended interpretations use source color revision 2. Prior generated fills
are rejected, and matching new fills validate and compose. Existing standard
one/two-profile source revisions keep their established paths. File regressions
cover custom xy, custom equal-energy spectra and a deliberately distinct third
matrix against analytic endpoint calibration, with matrix error below 0.00004
and unchanged input bytes. Metadata-only revision lookup and fill compatibility
are checked. Math regressions cover both payload byte orders, equal-energy
integration, malformed lengths and convex vertex/interior/boundary weights.

The release fixture generator has `--triple` and `--custom-spectrum` modes.
Four constant/noisy DNG 1.6 fixtures passed **16** CPU/CUDA/Vulkan comparisons,
finite opaque default perimeters, exact alpha and unchanged file/sensor hashes.
Maximum RGB difference was 0.0002031, below the existing 0.003 tolerance. EXIFTool
independently reports the version, third/custom illuminant IDs and third matrix;
binary illuminant payload interpretation is covered by the parser regressions.
Parameters and results are in
[extended-dng-2026-10-10.json](data/extended-dng-2026-10-10.json).

```sh
cargo test --lib dng_profiles
cargo test --example raw_color_fixture --locked
cargo run --release --example raw_color_fixture -- /tmp/new-triple-dng --triple
cargo run --release --example raw_color_fixture -- /tmp/new-spectral-dng --custom-spectrum
```

All 80 standard tests, four DNG regressions, strict Rust 1.99 Clippy for all
targets with CUDA enabled, formatting and diff checks passed. The preceding
commit's six desktop/native-learning jobs passed. Extra camera profile IFD
selection, nonlinear profile tables and physical camera/colorimetric verification
remain in the full PROMPT.md audit; these fixtures do not establish universal
profile or camera accuracy.
