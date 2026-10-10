# GFX diversity and GPU client reuse

Read-only ExifTool sampling of 100 evenly selected files from 9,481 RAWs found
more lens/orientation coverage than the earlier GF55 fixture. Made eight ordinary
owned copies under `/tmp/rawpuppy-validation/real-raw-diversity-2026-10-09`:
GF20–35 at 20/35 mm and orientation 6, GF500 and GF110 at orientation 8, GF50,
adapted Sigma 70 mm and an unnamed manual lens. Nothing in the original photo
directory was written; the original/copy hashes were checked again after each run.

Added `verify_camera_input`, which checks independent EXIF orientation, decodes the
full sensor, samples every pixel on the four full-resolution output edges, and
compares CPU with explicit CUDA/Vulkan previews. It exercises defaults and a
combined off-center crop, roll/yaw/pitch, manual distortion/CA, camera correction,
exposure/calibration/ND, curve and split-tone recipe. It checks finite pixels,
alpha agreement and unchanged RAW/sensor bytes, and retains previews and receipts.

The first wide-angle run uncovered an actual initialization failure: a second
explicit Vulkan renderer registered CubeCL's default service again and panicked.
Auto and explicit wgpu renderers now use one lazily registered client. Explicit
API requests are checked against the registered API; each renderer's source and
prepared allocations remain independent. A real-device regression failed before
the fix and now verifies Auto plus two native renderers, distinct photo values,
source retirement on drop, conflicting API rejection and continued rendering.
The existing CI ignored-GPU commands include it on Linux and macOS.

All eight updated release runs passed. Each default perimeter has 40,768 finite,
opaque samples, counting the four edge-strip corner points twice. Outputs are
11648×8736 or 8736×11648, with independently matched orientation 1/6/8. All 32
CPU/GPU comparisons passed the declared maximum/mean RGB tolerances 0.003/0.00003
with zero alpha mismatches. Observed maxima are 0.00170118 and 0.0000131378.
ExifTool 12.76 independently matches all four nine-knot lens tables in every file,
with knot differences below 5e-6; manual-lens identity tables are valid.
Decode took 0.38–0.43 s. Warm 1800-pixel previews took 4.45–11.76 ms on shared-memory
CUDA and 11.02–18.89 ms on Vulkan, excluding checks/encoding/presentation.

The aggregate is `docs/data/gfx-input-diversity-gb10-2026-10-09.json`. RAW copies,
rendered photos and camera JPEG previews remain outside the repository. Visual
inspection confirmed upright previews, but bright areas in the wide/macro examples
show lavender tint against neutral highlights in their embedded camera JPEGs.
Those JPEGs have a different rendering pipeline; they establish a concrete
highlight/color comparison to diagnose, not an exact color oracle. The full
goal remains active, with this finding retained in the requirements audit.

All 57 standard tests, both ignored native Vulkan tests, all four CUDA-build GPU
tests, strict Rust 1.99 Clippy for all targets with CUDA, formatting and diff
checks passed. No system-wide memory pressure or unrelated process changes were
used for the experiments.
