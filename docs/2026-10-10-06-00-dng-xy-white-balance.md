# DNG xy white balance

The white-balance audit found that a DNG can encode its selected white using
AsShotWhiteXY instead of AsShotNeutral. The decoder's fallback derives gains
using one matrix, which misses the profile interpolation needed under warm or
third/custom illumination. Applying the correct color matrix afterward cannot
repair gains derived from the wrong neutral.

The direct DNG path now uses a declared xy white point to weight its profile
matrices and calculate `AB * CC * CM * XYZ`, then normalizes the resulting camera
neutral to the existing green-referenced gains. The known xy point is used
directly rather than solved again by iteration. Returned gains replace the
decoder fallback before final validation, so an inappropriate fallback cannot
reject a valid declared white. Calibration/gains remain in the existing composed
module; pixels are not clipped or rerasterized. Missing/invalid coordinates and
conflicting xy/neutral tags return errors, matching the
[DNG specification's white-balance model](https://helpx.adobe.com/content/dam/help/en/photoshop/pdf/DNG_Spec_1_7_1_0.pdf).

This changes interpretations that may already have used revision 2 for a custom
or third profile, so xy-based sources use **revision 3**. Metadata-only lookup
returns the same revision. A regression rejects an old revision-2 generated
fill, validates a revision-3 fill and applies it; older source kinds retain their
established revision. Input bytes and recipes are not rewritten automatically.

File tests cover deliberately different warm/daylight matrices and a distinct
third/custom matrix. The fixtures encode xy rationals at 1e-8 resolution so the
analytic comparison measures calibration rather than coarse fixture quantization.
Camera gains and matrix coefficients match expected selected-profile values
within 0.00005. Invalid xy and a file containing both mutually exclusive tags
are rejected, with unchanged file bytes. The existing CI DNG example test runs
this fifth regression on all three desktop platforms.

The release generator has `--white-xy`, usable with `--dual-warm` or `--triple`.
Four constant/noisy fixtures passed **16** CPU/CUDA/Vulkan comparisons, finite
opaque default perimeters, exact alpha agreement and immutable source/sensor
hashes. All report source revision 3. Maximum RGB error was 0.0002245, below the
existing 0.003 tolerance. Settings, metadata and comparisons are in
[dng-white-xy-2026-10-10.json](data/dng-white-xy-2026-10-10.json).

```sh
cargo test --example raw_color_fixture --locked
cargo run --release --example raw_color_fixture -- /tmp/new-warm-xy --dual-warm --white-xy
cargo run --release --example raw_color_fixture -- /tmp/new-third-xy --triple --white-xy
```

All 80 standard tests, five DNG regressions, strict Rust 1.99 Clippy for all
targets with CUDA enabled, formatting and diff checks passed. Extra/nonlinear
profile handling and physical camera/colorimetric verification remain in the
full PROMPT.md audit. These synthetic checks are not universal camera accuracy
evidence.
