# Canon/Sony camera pipeline validation

Extended the real-input checks beyond GFX using the existing four held-out
[RawNIND dataset](https://dataverse.uclouvain.be/dataset.xhtml?persistentId=doi:10.14428/DVN/DEQCIM)
downloads: Canon EOS 500D CR2 at ISO 100/3200 and Sony ILCE-7C ARW at ISO 50/40000.
Each still matches its published SHA-1 and previously recorded SHA-256. Dataset
attribution and permissions are retained in the numerical record; RAWs and
derived photographs remain outside the repository.

The current release CUDA verifier decoded all four with RGGB patterns and
4752 × 3168 / 6000 × 4000 active output. EXIFTool independently confirmed
orientation 1, focal lengths 18/32 mm and apertures f/9/f/8. Lens descriptions
use differing vendor/decoder names for the same lenses; their literal strings
are retained rather than asserted equal. Sony's independent default crop is
6000 × 4000 at (12, 12), within the stored 6048 × 4024 sensor.

All 71,680 full-resolution perimeter samples were finite and opaque. Sixteen
CPU/CUDA/Vulkan comparisons passed for camera defaults and composed crop,
perspective, rotation, CA, exposure, calibration, graduated filter, curve and
split-toning edits. Maximum RGB error was 0.001490 and the largest mean error
0.00000849, below the existing 0.003/0.00003 tolerances; alpha matched exactly.
Original file and normalized sensor hashes stayed unchanged. Warm 1800-pixel
previews took roughly 2.28–3.71 ms on coherent CUDA and 8.20–10.90 ms on Vulkan.
These are measurements on this shared workstation, not universal latency bounds.
All four default previews were inspected: framing and scene colors are coherent,
and the high-ISO frames visibly retain substantial noise with Standard defaults.

An additional independent comparison used Darktable 5.5.0+1351~gd7c766cc19-dirty
strictly through its executable and serialized output history. It used new
ordinary RAW copies, isolated config/cache/in-memory library, workflow `none`,
no custom presets or OpenCL, eight workers and 32-bit linear Rec.709 EXR.
Rawpuppy's independent linear recipe disables tone mapping, highlight recovery
and display edits. No Darktable implementation source was read or copied.

Initial whole-image low-ISO per-channel means differed by at most about 1%.
The oracle defaulted to a different crop (1800 × 1197 versus 1800 × 1200), so
normalized patch comparisons can differ substantially and are not pure
calibration measurements. For high-ISO Sony, the oracle's serialized rawprepare
crop was set to the independently verified active rectangle, and the resulting
1800 × 1200 output was checked. That control retained the substantial signed
mean discrepancy.

Added `trace_raw_normalization`, a diagnostic that records active CFA channel
statistics, exports the normal signed linear result, and exports an explicitly
counterfactual result with negative sensor samples clipped to zero. It modifies
only its own diagnostic allocation; production signed-value handling is unchanged.
It also records signed statistics and hashes from supplied developed references.

In the ISO 40000 Sony RAW, 33.5–38.8% of active CFA samples are negative after
black subtraction, reaching −0.03448. In the matched-crop linear comparison:

| Whole-image mean ratio to oracle | Red | Green | Blue |
| --- | ---: | ---: | ---: |
| Normal signed reconstruction | 0.8420 | 0.9658 | 0.7071 |
| Diagnostic sensor zero clipping | 1.0018 | 0.9943 | 1.0858 |

The clipping control accounts for much of the red/green mean difference and
demonstrates how altering signed noise changes dark color. It does not identify
the oracle's exact processing stage or establish full calibration agreement:
the blue residual is 8.6%, and reconstruction methods also differ. Darktable's
working-RGB export still contains negative values, so a blanket assertion that
it clips all final RGB would be incorrect. The remaining discrepancy stays open
in the camera/color audit rather than motivating a production clamp.

The complete settings, hashes, metadata and measurements are in
[canon-sony-camera-2026-10-10.json](data/canon-sony-camera-2026-10-10.json).
The diagnostic can be reproduced with a new output directory:

```sh
cargo build --release --example trace_raw_normalization
target/release/examples/trace_raw_normalization copied-photo.arw /tmp/new-normalization --reference independent-linear.exr
```

All 75 standard tests and strict Rust 1.99 Clippy for all targets with CUDA enabled
passed, with formatting/diff checks. The preceding streaming-PNG commit's six
desktop/native-learning CI jobs completed successfully. A stale color-document
paragraph about unimplemented HDR/profile discovery was corrected to reflect the
existing signal checks and remaining physical validation. The full PROMPT.md goal
remains active.
