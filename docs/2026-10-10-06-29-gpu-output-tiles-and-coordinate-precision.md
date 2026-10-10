# GPU output tiles and coordinate precision

The output renderer previously divided images into whole-row tiles. A row wider
than a device's storage binding limit rejected GPU rendering, and a row wider
than 64 MiB exceeded the intended tile budget. Output tiles now contain a bounded
number of pixels and can start/end within a row. Copied CUDA and Vulkan/Metal use
at most 64 MiB or the device's smaller binding limit; coherent CUDA writes those
same bounded slices directly into the owned output allocation.

The kernel receives the full viewport dimensions, tile origin and valid pixel
count. It derives global x/y coordinates without constructing an overflowing
global 32-bit linear index. Every tile uses the original viewport, so tile
boundaries do not change geometry, graduated density, retouch or sample positions.
Source storage/addressing limits still select CPU fallback in Auto; this change
does not implement source tiling or remove those device limits.

A periodic-ramp source exposed a separate precision problem at 100,003 pixels
wide. Tiny and full GPU tiles were identical, while the CPU comparison initially
differed at sharp wraps. CPU and GPU now share host-computed viewport pixel steps
and explicit fused multiply-add. Radial geometry adds displacement to the
original projected point instead of centering and uncentering it, avoiding
fractional pixel loss near borders. The original 0.0003 pixel-value tolerance was
kept. A standard regression checks identity geometry on both very wide and very
tall images, requiring less than 0.0001 pixel coordinate error.

On NVIDIA GB10, Vulkan, copied CUDA and coherent CUDA all passed two composed
RGB/Bayer cases using **64-pixel output tiles**, plus a **100,003 × 3** RGB source
with **8,192-pixel tiles** (128 KiB versus a 1,600,048-byte row). Each tiny-tile
result is exactly equal to a single-tile render. Alpha is exact, sensor values are
unchanged, and the maximum observed wide-source CPU error is 0.00000012 on all
three paths. The Vulkan and macOS CI GPU steps now include this regression.

The current CUDA release also passed four CPU/CUDA/Vulkan comparisons on an
ordinary owned GFX100S copy: default rendering and a composed lens/perspective/
crop/intensity/color/display recipe. All **40,768** full-resolution perimeter
samples were finite and opaque. Maximum preview RGB error was 0.0007597 and
maximum mean error 0.00000398, within the existing tolerances; alpha and input/
sensor hashes were unchanged. This is renderer agreement, not an independent
physical calibration measurement or a full-resolution comparison of every pixel.

All **82** standard tests, **five** DNG regressions, **six** Vulkan compute/HDR
regressions, the three tile paths, strict Rust 1.99 Clippy with CUDA/all targets,
formatting and diff checks passed. Measurements and settings are retained in
[the data record](data/gpu-flat-tiles-2026-10-10.json).
