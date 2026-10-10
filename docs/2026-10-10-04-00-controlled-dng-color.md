# Controlled DNG color and signed-noise comparison

The previous full Sony ISO 40000 comparison retained an 8.6% blue mean residual
after a diagnostic sensor zero clamp. To separate calibration from spatial
reconstruction and clipping behavior, added `raw_color_fixture`: a reproducible
synthetic Bayer DNG generator and independent-reference measurement tool.

The DNG uses RGGB, black 512, white 15360, explicit D65 ColorMatrix1 and as-shot
neutral tags, nine 256 × 256 camera-color patches, and 768 × 768 output. It can
derive an equivalent normalized camera matrix and gains from a read-only RAW;
the current run used the Sony fixture. This equivalence does not copy its full
original camera profile or prove the camera's physical spectral accuracy.
The format reference is Adobe's
[DNG specification resource](https://www.adobe.com/support/downloads/dng/dng_sdk.html).
This product includes DNG technology under license by Adobe.

One fixture has constant fields. The other adds deterministic equal-magnitude
noise of ±224 DN around each chosen level, with seed 0x92d68ca2. The fields remain
within the representable unsigned sensor range, including values below the
declared black level. Exports use Rawpuppy's Standard MHC reconstruction, linear
tone mapping, disabled highlight recovery and no display edits. Inner 192 × 192
patch means exclude 32 pixels around every patch boundary. The generator records
measured CFA means rather than assuming the finite random realization averages
to exactly zero, and checks each noiseless rendered patch against analytic
calibration within 0.000003.

Darktable 5.5.0+1351~gd7c766cc19-dirty independently decoded both generated DNGs
with isolated config/cache/in-memory library, workflow none, no custom presets
or OpenCL, eight workers and 32-bit linear Rec.709 EXR. Only its executable and
serialized output history were used; no implementation source was read/copied.
EXIFTool independently read the DNG/CFA/black/white/matrix/neutral tags.

Across eight constant patches whose sensor RGB is nonnegative, the largest
per-channel mean difference is **0.00001538**. This includes patches whose
calibrated working RGB is negative, so it checks an unbounded color transform.
The separate negative-sensor patch behaves very differently: reconstructing the
oracle's camera mean through the inverse calibration yields approximately zero
red/blue rather than the known negative input. That is evidence consistent with
clipping before calibration, not clipping all final working RGB.

For the balanced noisy black patch, the working-RGB means are:

| Pipeline | Red | Green | Blue |
| --- | ---: | ---: | ---: |
| Rawpuppy signed MHC | 0.00001851 | −0.00003776 | 0.00007707 |
| Independent oracle default | 0.01570094 | 0.00217848 | 0.02305930 |

Inferred oracle camera means are about [0.007653, 0.007537, 0.007618], close to
the directly measured zero-clipped sensor means [0.007553, 0.007539, 0.007548].
These controls distinguish a substantial noise/clipping-dependent positive bias
from a uniform calibration-matrix error in this explicitly tagged DNG. They do
not identify the exact oracle processing stage or completely resolve its native
Sony ARW reconstruction differences. Production signed handling is unchanged.

Both DNGs also passed eight default/composed CPU/CUDA/Vulkan comparisons, finite
opaque default perimeters, exact alpha agreement and immutable file/sensor hashes.
Maximum RGB error was 0.0002592, below the existing 0.003 tolerance.
Numerical settings, output history, hashes and all patch measurements are in
[raw-color-fixture-2026-10-10.json](data/raw-color-fixture-2026-10-10.json).
Images remain in temporary storage.

```sh
cargo build --release --example raw_color_fixture
target/release/examples/raw_color_fixture /tmp/new-fixture --like-camera copied-photo.arw
target/release/examples/raw_color_fixture /tmp/new-comparison --like-camera copied-photo.arw --reference-dir /tmp/oracle-exports
```

The reference directory contains `constant-darktable.exr` and
`noisy-darktable.exr`, exported from the matching deterministic DNGs.
A file-level regression separately generates a known calibration DNG, verifies
its RGGB/white-balance metadata and negative sensor values, checks all nine patch
centers against their analytic color values, and verifies unchanged file bytes.
It runs on all three desktop CI platforms with
`cargo test --example raw_color_fixture --locked`.

All 75 standard tests plus the new DNG regression, strict Rust 1.99 Clippy for
all targets with CUDA enabled, formatting and diff checks passed. The previous
commit's six desktop/native-learning jobs also passed. The broader PROMPT.md
camera/color audit remains active.
