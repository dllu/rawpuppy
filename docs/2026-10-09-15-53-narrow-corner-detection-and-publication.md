# Narrow corner detection and restored publication

Reproduced missed geometric gaps on a 11648×8736 canvas rotated by 0.01 degrees:
every sample of the old 128×128 overview was opaque, so corner planning returned
no regions despite missing perimeter pixels at output resolution. The regression
failed before the correction. Planning now samples actual output pixel centers
on the entire perimeter, alongside at most 128×128 interior samples. Bounds use
physical output pixels, with wider integer intermediates and square context
placement. This avoids a full-resolution probe raster and preserves support for
large dimensions.

The bounded 512-pixel inference context can miss the same thin gap. Its model
mask now also covers missing output perimeter samples in aligned 8×8 blocks,
retaining them when Moebius downsamples to its 64-pixel latent grid. These blocks
affect inference context only: saved-layer composition still tests exact missing
pixel membership at output resolution and preserves opaque original samples.

The renderer exposes the same detection to editor and CLI. Immutable saved-layer
alpha snapshots are validated and resolved before sparse sampling, sharing target
membership with composition. Already filled gaps are excluded from both planning
and inference-mask augmentation. Identity validation, checksum/decode failures,
offscreen culling and the bounded persistent cache remain in effect.

Tests verify the missed narrow rotation, all detected top/bottom perimeter samples
being covered, bounded sampling count, masks missed again at inference resolution,
and positive samples surviving the actual nearest latent-grid coordinates. A
renderer test checks that a saved opaque corner fill suppresses both planning and
mask growth. Existing transaction, unpainted-pixel and saved-layer tests passed.

Ran the ignored real-source check on the owned GFX copy
`/tmp/rawpuppy-validation/gui.raf`: output dimensions 8736×11648, four narrow
contexts, 129.76 ms planning in this debug run. Each initial model mask was empty;
each augmented mask survived latent sampling. The test passed in 11.65 seconds,
including decoding and input hashing; the copied RAW hash was unchanged. Original
photographs under `~/pictures/raw` were not written. This checks actual camera
decoding, geometry and mask delivery, not the visual quality of a new neural fill.

All 51 default tests and 54 normal Moebius-build tests passed. Matching LibTorch
2.13 CPU was used for the feature build; the separately ignored native-model
samplers were not repeated. Strict Rust 1.99 Clippy for all targets with Moebius,
formatting and diff checks passed. Logs are retained under
`/tmp/rawpuppy-validation/narrow-corners-*.log` and
`narrow-gfx-corner-validation.log`. Clippy first requested the standard
`contains` form for one test assertion; the final check passed after that change.

The user restored full filesystem access. Git fetch succeeded and confirmed the
local and remote base matched, resolving the earlier metadata write restriction.
The pending curve/brush and context/regeneration milestones can now be published
together with this completed correction. The full PROMPT.md goal remains active;
broader photographic quality, display and device verification is still pending.
