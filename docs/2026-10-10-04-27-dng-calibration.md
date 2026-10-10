# DNG illuminant-dependent linear calibration

The loader previously selected the first preferred matrix, usually D65, even
when a DNG supplied a distinct warm calibration and an as-shot neutral under
warm lighting. It also did not apply the DNG analog balance, camera calibration
or forward-matrix tags that change the linear camera transform.

The new private camera-profile module implements the linear one/two-profile
processing described in Chapter 6 of Adobe's
[DNG 1.7.1 specification](https://helpx.adobe.com/content/dam/help/en/photoshop/pdf/DNG_Spec_1_7_1_0.pdf):

- A supported dual profile uses the as-shot camera neutral to infer a white point
  iteratively. Matrix weights use reciprocal correlated color temperature and
  clamp to the nearest endpoint outside the calibration interval.
- AnalogBalance and matching-signature CameraCalibration matrices participate in
  the camera/XYZ conversion. Mismatching calibration signatures use identity.
  Both supported ASCII and BYTE signature encodings are compared as bytes.
- Forward matrices, when supplied for the applicable profiles, map the reference
  white-balanced camera to D50. The combined transform converts into linear
  working sRGB, preserving neutral working white without clipping pixels.

The iteration is damped and bounded, with explicit errors for invalid matrices,
white points and non-convergence. All work happens during source loading; the
renderer still uses its existing composed calibration matrix/gains. Non-DNG and
single-matrix DNGs without extra linear-calibration tags retain their prior path.
Changed DNG interpretation uses source color revision 1, with a metadata-only
revision lookup for saved layers. Old generated fills are rejected; new fills
with the matching revision apply. Recipes and RAW bytes remain unchanged.

The Robertson isotherm numerical table comes from
[Colour Developers](https://github.com/colour-science/colour/blob/develop/colour/temperature/robertson1968.py),
with the full BSD-3-Clause notice in
[colour-robertson.txt](licenses/colour-robertson.txt). The Rust distance,
interpolation and fixed-point code was written independently; no Darktable
implementation was used. The source-data and specification hashes are recorded.

The camera-profile math check covers Standard-A, D65 and an intermediate white.
File-level DNG regressions cover deliberately distinct warm/daylight matrices,
analytic single-matrix signed samples, analog balance, matching and mismatching
camera signatures, forward matrices and saved-fill revision rejection/reload.
Endpoint and extra-tag matrix differences are below 0.00003; forward working white
is within 0.000005. The existing desktop CI example test now runs all three cases.

The fixture generator has a `--dual-warm` mode with distinct D65/Standard-A
matrices and a known Standard-A neutral. Actual release constant/noisy fixtures
passed eight default/composed CPU/CUDA/Vulkan comparisons with unchanged file and
sensor hashes, finite opaque perimeters and exact alpha. Maximum RGB difference
was 0.0002245, below the existing 0.003 tolerance. Results and parameters are in
[dng-calibration-2026-10-10.json](data/dng-calibration-2026-10-10.json).

```sh
cargo test --lib camera_profiles
cargo test --example raw_color_fixture --locked
cargo run --release --example raw_color_fixture -- /tmp/new-dual-dng --dual-warm
```

All 76 standard tests, three file-level DNG regressions, strict Rust 1.99 Clippy
for all targets with CUDA enabled, formatting and diff checks passed. The prior
commit's six desktop/native-learning CI jobs also passed. These establish the
implemented linear controls, not physical camera color accuracy. Three-illuminant
and custom-illuminant data, extra profile IFD selection and nonlinear DNG profile
tables remain to evaluate, as does the original Sony ARW reconstruction residual.
The full PROMPT.md goal remains active.
