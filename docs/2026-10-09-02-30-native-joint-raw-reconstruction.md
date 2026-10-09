# 2026-10-09 02:30 — Native joint RAW reconstruction pilot

Added an optional `raw-ml` library/headless path for learned Bayer reconstruction
and denoising. The editor still uses its MHC/bilateral baseline; cache integration,
whole-image blending and more quality validation remain required. The overall
PROMPT.md goal remains active.

## Implemented and exercised

- Independently expressed the RawNIND four-level parameter graph and checked it
  against the pinned author's implementation. All four CPU shape/range probes
  matched exactly, including signed and above-white inputs. Exported weights use
  the authors' CC BY 4.0 alternative, retain attribution and remain outside the
  repository. The original GPL implementation is only an external oracle.
- Added a native Rust adapter with checksum/manifest validation, camera-native
  RGB output, RGGB packing for all Bayer phases, sensor-anchored pooling, a
  reflected 256-pixel halo, fallible allocations, arbitrary ROI dimensions and
  exposure matching at observed CFA colors. No full-image RGB allocation or RAW
  upload is needed to reconstruct a region.
- Added headless native inference, checked sensor/MHC fixture export, and a
  comparison tool using clean-reference-only alignment and one shared exposure
  correction for all noisy methods. Both evaluation tools refuse existing result
  destinations. All original RAWs remained untouched.
- Shared native device selection with Moebius without coupling model features.
  The previous Moebius texture fixture reproduced its exact output hash after
  this refactor.
- Extended native-learning CI compilation/tests to include `raw-ml`.

## Evidence and limits

Downloaded two held-out public RAW pairs, Canon EOS 500D ISO 100/3200 and Sony
ILCE-7C ISO 50/40000. All four matched published filename SHA-1 values. On the
tested 512-pixel regions, joint reconstruction improved camera-linear PSNR from
the best of five bilateral/MHC settings by approximately **0.46 dB** and
**2.89 dB** respectively. The clean references themselves use MHC. A 32-pixel
border and saturated reference values are excluded; these are scoped region
comparisons, not publisher metrics or proof of universal superiority.

Inspection showed substantially reduced noise, especially on the Sony vase,
but some true fine speckles and surface variation are smoothed. Broader quality,
GFX high-ISO pairs, active-area borders and HDR behavior are still unverified.
Recent 2026 reconstruction leads were checked: one repository is inaccessible,
another lacks released checkpoints/license notices; Samsung's available model
uses non-commercial terms. These facts explain the choice of a deployable real-RAW
candidate without claiming it is the final best model.

An initial CUDA overlap check exposed cuDNN TF32 precision differences. The
version-2 graph explicitly requests deterministic IEEE float32 convolutions per
operator, preserving the other model's process-wide policy. The final graph
passed overlap checks and matched the Canon CPU result within `5.66e-7` camera
units. This is a measurement, not an arbitrary-input error bound.

Warm CUDA reconstruction of a 512-pixel region took about **130 ms**, including
context preparation and transfer. The padded context is 1056×1056; weights load
in about 0.34 seconds. CPU took 283 ms warm with eight workers. A 513×517 region
from a GFX100S 100 MP source ran in 132 ms warm. Full-frame neural performance,
GPU peak memory and MPS/Windows inference have not been established. Two
same-device runs of each final case produced identical bytes.

## Verification

The 32 default tests and 34 nonignored `moebius,raw-ml` tests passed. Strict
Rust 1.99 Clippy passed for default and combined CUDA/native-learning targets.
Real-graph tests on GB10 passed for all four Bayer phases, unchanged sensor data,
observed-sample photometry, odd regions on a 100,003-pixel-wide source, and
overlapping contexts. One test run initially omitted LibTorch runtime loader
paths; it was rerun successfully with the matching libraries.

Formatting, Python compilation and whitespace checks passed. Source files,
weights, generated graphs, RAWs and derived images are kept in evaluation caches
or `/tmp/rawpuppy-validation`. Neither `~/pictures/raw` nor other projects'
packages/processes were modified. The shared PyTorch installation was read only;
small evaluation dependencies were added to Rawpuppy's isolated environment.

Reproduction, attribution, measurements and remaining integration work are in
[learned-reconstruction.md](learned-reconstruction.md) and
[the data record](data/raw-reconstruction-2026-10-09.json).
