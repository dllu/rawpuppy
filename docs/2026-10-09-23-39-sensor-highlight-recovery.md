# Sensor-informed highlight recovery

Implemented the repair motivated by the preceding GFX diagnosis. New integer RAW
documents enable a persisted Recover clipped highlights control. Existing recipes
default it off and omit false when serialized, preserving their pixels and recipe
hashes. Float RAW, developed RGB, signed data and original samples above nominal
white remain intact. No original sensor values are changed.

Near-ceiling sensor samples identify affected channels; mask coverage interpolates
through the sampling footprint and follows the separate CA channel maps. Camera
RGB uses intact white-balanced channels to estimate missing intensity before
calibration and scene gains. An affected channel remains a lower bound and is
only increased. Fully clipped samples use a neutral estimate. Coverage and the
transition into fully clipped regions are continuous. This is an estimate of
missing intensity, not recovery of lost colors or detail.

The first Vulkan photo check exposed a binary-mask boundary disagreement. Replaced
binary output-sample decisions with interpolated coverage and smoothly weighted
remaining-channel confidence. The two updated real GFX CUDA/Vulkan runs passed
their existing maximum/mean RGB tolerances and identical-alpha checks, with all
40,768 default perimeter samples finite and opaque. Largest RGB differences were
0.001301 for the wide photo and 0.000370 for the macro photo.

Inspected before/after previews show the selected lavender window and pavement
areas corrected. A window sample changes from sRGB (230,217,248) to (242,242,242).
Pavement red/blue mean minus green changes from 8.5 to −0.5 code values; selected
shadow samples remain exactly unchanged. These are scene diagnostics rather than
a general quality score. Warm 1800-pixel previews observed 4.50–11.63 ms on CUDA
and 12.43–21.14 ms on Vulkan. Original/copy and sensor hashes remain unchanged.
The public record is `docs/data/highlight-recovery-gfx-2026-10-09.json`; photos
remain under the owned `/tmp/rawpuppy-validation` directories.

Learned integer-RAW RGB retains original clipping provenance as four three-bit
masks in each normal float's mantissa, appended to the RGB allocation. It adds
about one byte per pixel, keeps RGB photometry exact, avoids another GPU binding,
and preserves coherent CUDA allocation sharing. A real-model test substitutes
arbitrary learned HDR values while checking that the original mask governs
recovery. It passed on CPU, Vulkan and CUDA. The standard path and GPU kernel
share the same estimation and source-domain ordering.

All 60 standard tests, five CUDA-build GPU tests, eight real RawNIND tests, the
CUDA packed-mask test, strict Rust 1.99 Clippy across CUDA/Moebius/RawNIND,
formatting and diff checks passed. Camera-default, lower-bound, red-light,
continuous-coverage, signed/above-white, XMP and historical-hash regressions are
included. Broad colored-light/camera saturation and physical-display validation
remain open in the full PROMPT.md objective.
