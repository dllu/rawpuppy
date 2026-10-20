# CI repairs and legacy RAW checks

Run `39217186500` failed for three independent reasons:

- DNG fixture decoding reached the new capture-metadata reader, which tried to
  serialize an empty EXIF directory. Rawler rejects empty directories. RAWs
  without photographic EXIF tags now retain their camera identity without
  constructing that optional directory. All six existing DNG checks pass.
- Windows checked out a recipe fixture with CRLF line endings. The historical
  calibration test compared it to JSON serialized with LF. The comparison now
  normalizes line endings while still checking the complete serialized recipe
  and its historical rendering behavior.
- An expensive RawNIND test ran reconstruction, overwrote the learned pixels,
  and then asserted highlight values from the old calibration defaults. That
  redundant numeric test was removed; the ordinary highlight tests already
  explicitly check the legacy calibration values and valid signed/HDR samples.

Routine CI retains compilation and model-free checks for `raw-ml`, but no longer
downloads, exports, installs and executes the legacy RawNIND model on every
push. Its seven remaining integration checks are available through the Rust
workflow's `legacy_raw_inference` manual input. This avoids expanding work on
the reconstruction pipeline scheduled for replacement. The current runtime
and model installer remain unchanged; the new demosaic project is not integrated.
DNG color calibration, original-data preservation, hot-pixel halo regressions
and the existing Moebius/SAM3 checks remain in routine CI.

Reviewing the later, previously unreached steps also found that Vulkan/Metal
CI invoked every ignored GPU test with CUDA enabled by default. That includes
three checks requiring NVIDIA hardware unavailable on the hosted runners.
The native GPU integration step now enables only `gpu`, leaving CUDA checks
available for machines with CUDA. The CAT16 parity check also selects Metal
on macOS rather than explicitly requesting Vulkan.

Local validation: 174 tests passed with `moebius,raw-ml,sam3` enabled,
including the six DNG example tests. The historical fixture comparison was
also checked with both LF and CRLF text. Strict Clippy 1.99 for all features
and targets, rustfmt, whitespace checks and actionlint 1.7.12 passed.
