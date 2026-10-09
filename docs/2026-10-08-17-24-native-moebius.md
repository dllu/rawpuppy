# Native Moebius and non-destructive synthesis layers

Implemented Rust Moebius VAE handling, CFG and seeded DDIM sampling using verified
prepared TorchScript graphs. Added numerical model preparation, a pinned manifest,
checkpoint attribution/license records, CPU/CUDA/MPS device selection, and matching
LibTorch 2.13 integration. PyTorch 2.14 was incompatible with the binding; a matching
runtime was installed in Rawpuppy's cache without modifying other project environments.
CUDA registration is loaded explicitly when available because native linkers can
omit its registration-only library. Real native CUDA sampling was ~2.1 seconds for
the 10-step test; the same CUDA-prepared graphs also ran on CPU (~27.3 seconds
with eight workers). Debug checksum verification is much slower than release builds.

Added UI mask painting, corner-fill generation, regeneration with sampling controls,
undoable generated layers, immutable float EXR assets and XMP references, original/
recipe identity checks, stale-result handling, and a modern `inpaint` CLI. Large-photo
generation renders bounded context directly from the original. Composition culls
outside regions and checks exact target membership, preserving unpainted samples.

Validation: actual-model native generation test passed; frozen prepared graphs match
the source network with zero maximum absolute error in their preparation checks.
New integrity tests cover saved-layer reload, exact untouched pixels, original
immutability, corrupt/stale rejection, and square context for portrait and 100,003-
pixel-wide images. All 20 scheduler/core/integrity tests in the native build and
strict Clippy passed. Native model
tests require runtime/artifact setup and are exercised explicitly, not inferred from
ignored tests.

Used an isolated 1440×960 X11 test session to paint and generate, save XMP, reload
generated content, change exposure, and update the stale fill. CLI reload/export
matched generated output exactly at 8-bit inspection resolution. A GFX100S original
was read through a temporary symlink with its sidecar/assets entirely under `/tmp`;
four geometric-corner layers were generated. No contents in `~/pictures/raw` changed.

The first corner test exposed subpixel alpha slivers when the inference-resolution
mask controlled full-resolution composition. Context layers now remain opaque while
exact output-resolution masks decide membership. The corrected GFX corner export
has no nonopaque pixels at the inspected 512-pixel preview resolution, and a
regression test covers a gap smaller than one inference texel. CI now checks the
optional native runtime with matching CPU LibTorch. Broader color, platform,
quality and unified-memory work remains in the full-scope completion audit.

Visual review distinguished alpha correctness from quality: the initial rotated
photo had opaque dark wedges. Moving corner context inward increased known image
content and improved the 20-step result, while one corner remained dark. This
limitation is recorded in the model comparison; the backend is provisional.
A 50-step seed-42 run subsequently filled all four visible wedges in 36.4 seconds.
This changes both sampling variables; it is not a controlled model comparison.
