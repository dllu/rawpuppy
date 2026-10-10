# Moebius runtime and generated layers

Moebius inference, seeded DDIM sampling, mask composition and layer storage run
in Rust. The optional `moebius` feature uses LibTorch through `tch`; Python is only
used once to prepare inference graphs from the published checkpoint. The normal
editor and saved-layer rendering do not require Python or the model.

## Build and model preparation

Use LibTorch **2.13**, matching `tch 0.26`, with the appropriate CPU/CUDA/MPS
backend. Set `LIBTORCH` to its distribution directory and put its libraries and
CUDA dependencies on the loader path (Linux `LD_LIBRARY_PATH`, Windows `PATH`).
The `LIBTORCH_USE_PYTORCH=1` alternative is supported by `tch` for matching Python
installations. Do not bypass a runtime-version mismatch: PyTorch 2.14 removed
symbols used by this binding.

The pinned local `torch-sys` patch selects C++20/conforming mode for Windows
MSVC builds and removes its conflicting `module` type alias. The matching 2.13
headers use designated initializers and bit-field defaults that MSVC rejects in
C++17 mode. No extra Windows `CXXFLAGS` are required. Runtime libraries must
still be on `PATH`; this does not permit an incompatible LibTorch version.
See [binding patch](../vendor/torch-sys/RAWPUPPY.md) for its exact scope.

```sh
cargo build --release --features moebius,cuda
target/release/rawpuppy fetch-moebius
python tools/export_moebius.py --repo /path/to/Moebius \
  --models ~/.cache/rawpuppy/models/moebius \
  --output ~/.cache/rawpuppy/models/moebius/torchscript --device cuda
target/release/rawpuppy edit photo.raf
```

Use the pinned upstream checkout described in `inpainting-research.md` and its
inference dependencies for preparation. The exporter loads only the student
modules, checks source checkpoint hashes, verifies graph outputs against the
original network, and records a manifest with graph checksums. Weight-folding
optimizations are disabled because they measurably changed denoiser output.
Prepared device literals use the input tensor's device. CPU/CUDA/MPS selection
is performed in Rust. Real graphs have executed natively on CUDA, Linux/Windows
CPU and macOS MPS. The desktop CI matrix also checks an explicit CPU case.

The native-learning CI matrix now prepares real 512-pixel graphs independently
on Linux, macOS and Windows, using matching PyTorch 2.13 and pinned preparation
dependencies. It verifies the upstream source revision and checkpoint hashes,
records trace/freeze errors and package versions, and rejects non-finite checks
or an existing output directory. Preparation loads student/factory modules
without importing the unused teacher or Python image-pipeline aggregators.
Tests use `RAWPUPPY_TEST_MOEBIUS_GRAPH` to select these scratch graphs; macOS must
actually sample on MPS with CPU operator fallback disabled. An explicit CPU
case also runs on every platform. Only graph provenance is retained in CI.
CI run `37980551392` passed all six jobs. Its macOS sampling log explicitly
selects `Mps` with CPU operator fallback disabled; both MPS and explicit CPU
cases passed finite-output, filled-selection and exact unselected-sample checks.
These validate execution and integrity, not general photographic quality or
physical display colorimetry. Cached graphs from older exporters should be
regenerated for MPS as described below.

Frozen graphs can serialize wrapped `0.5`/`1.0` scalar constants as float64
tensors, which MPS rejects while loading an otherwise float32 network. The
exporter reloads its temporary graph, converts only exactly representable scalar
constants to float32, and checks the final serialized file against the original
network. Inexact scalars and non-scalar float64 constants are rejected. The
manifest records the converted values and the maximum checked discrepancy.
Regenerate graphs prepared with an older exporter before using them on MPS;
existing CPU/CUDA graphs remain supported.

## Editor and CLI

Open “AI removal & corner fill,” paint the selected area, then generate. Context
is square in image pixels, with extra surrounding content; portrait or wide photos
are never stretched. “Fill geometric corners” computes a separate region for each
corner containing missing pixels, shifting the square inward where it fits to
retain more photographic context. Sampling count and seed control regeneration.
New generation uses full diffusion strength (1.0), starting the selected content
from noise. The earlier 0.99 setting retained some original-image latents; on the
controlled rotated GFX example it left a black wedge with seed zero. At the same
20 requested steps and seed, full strength filled that wedge. Seed 42 was also
checked. This is an observed improvement on one photograph, not a general quality
ranking. The [comparison record](data/corner-strength-gb10-2026-10-09.json) retains
the settings, graph identities, timings and preview-preservation checks.
Generation runs on the photo worker, and ordinary preview updates remain separate.
Recoverable tensor-operation panics are converted to generation errors inside
the serialized sampler. Temporary tensors and gradient state unwind before the
RNG guard is released, preserving the worker and allowing another seeded attempt.
The editor's existing scoped error handling reports the failed operation.
Brush context also shifts inward at canvas edges wherever the physical square
fits, retaining more photographic content without stretching or dropping the
selected part inside the canvas.

New painted layers record an inward edge blend of 15% of brush radius. The
generated core and all unpainted pixels stay exact; blending is in working linear
RGB and does not widen the selection. Missing geometric pixels retain full
coverage. Legacy layers omit this field and retain zero blend. See the
[inward-blend validation](2026-10-10-09-19-inward-synthesis-edge-blending.md).

Corner detection checks every perimeter pixel at the full output dimensions,
alongside a sparse interior grid. Narrow edge gaps therefore survive even when
the overview misses them. Missing perimeter samples are also projected into
8-pixel inference blocks so they remain represented in the model's smaller
latent mask. Final composition still selects only missing output pixels. Saved
layer coverage is resolved once for these samples, avoiding redundant generation
over already filled pixels. This uses a bounded interior grid plus work
proportional to the perimeter, without allocating a full-resolution probe raster.

A real GFX 8736×11648 image rotated by 0.01 degrees has been exercised through
native CUDA generation and saved-layer reload: all 8,918 missing perimeter
samples became opaque, and all 31,850 originally opaque perimeter samples stayed
exactly unchanged. Input hashes also stayed unchanged. These edge counts include
the duplicated corner samples of the four edge strips; they are not a check of
every interior pixel or a perceptual quality score. The checked-in
[`verify_corner_fill` example](../examples/verify_corner_fill.rs) makes an owned
input copy, retains assets/XMP and records graph identities and timings in a new
output directory. See [the local data record](data/narrow-corner-gb10-2026-10-09.json).
The release build repeated those checks successfully: first-context generation
took 7.74 seconds, subsequent contexts about 2.93 seconds, and generation plus
save/reload verification took 17.08 seconds after planning. See the
[release receipt](data/narrow-corner-gb10-release-2026-10-09.json). The debug build
took 67.54 seconds for its first context and about 3.5 seconds thereafter. These
observations include context preparation and do not isolate loader/startup cost
or guarantee latency on another workload.

```sh
target/release/rawpuppy inpaint photo.raf result.png \
  --erase 0.4,0.3,0.04 --steps 20 --seed 0 --save-edits
target/release/rawpuppy inpaint photo.raf filled.png --fill-gaps --save-edits
```

CLI erase coordinates/radius are normalized to the corrected canvas; radius is
a fraction of width. Context rendering samples the original rather than building
a full-resolution intermediate merely to infer a local fill. Final exports still
use the requested resolution. Each inference context currently has 512×512 model
detail; wide areas may need smaller selections or a future model/refinement path.

Generated display-linear RGB is stored as immutable float EXR assets in
`photo.raf.rawpuppy-assets`. The XMP recipe stores content hashes, source identity,
the preceding recipe identity, mask strokes, region, model version, steps and seed.
New non-legacy settings also record guidance, strength and noise offset. Older
sidecars imply the previous 2.0 / 0.99 / 0.0357 values and retain their serialized
form; existing asset pixels and preceding-recipe hashes remain compatible.
Only exact target pixels are composited; unpainted original pixels remain unchanged.
Corner membership is checked at output resolution, avoiding leftover transparent
slivers from the smaller inference mask. Preview and export share the same layers.
Undo/redo removes or restores layer references without changing the original.

Changing a preceding edit makes its fills stale. Preview shows the current base
image and offers regeneration; export rejects stale fills. A loaded layer can be
rendered without loading the model, and the editor can regenerate the same masks
with new steps/seed. Generated-layer caches are bounded and offscreen layers are
culled. Unreferenced immutable assets are retained for undo/recovery.

Regeneration replans brush context for the current canvas dimensions and detects
geometric corners again from the base photograph. It preserves painted selections,
adds each current corner once, and removes obsolete corner references if cropping
or geometry changes leave no gaps. That gap-free update does not load the model
or old assets. Save the updated recipe to retain the new layer references.

Overlapping corner jobs also check which canvas targets remain after earlier
fills. A completed region is skipped without inference or a new asset. Empty
padding outside the photograph remains unknown model context, but it cannot
alone cause another generation. Interpolated layer alpha is normalized so an
opaque fill stays exactly opaque instead of creating tiny false gaps.

The CUDA verifier exercises this on a 100003×17 synthetic fixture scaled to 0.99:
two contexts generated fills, two completed contexts were skipped, and save/reload
filled 2,034 missing perimeter samples while preserving 198,006 opaque samples
exactly. A repeat on the real GFX copy retained its 8,918 / 31,850 conformance
counts. See [wide](data/overlapping-corners-wide-gb10-2026-10-09.json) and
[GFX](data/overlapping-corners-gfx-gb10-2026-10-09.json) receipts. These are integrity
checks, with duplicated edge-strip corner samples, rather than quality rankings.

Visible layer assets are resolved and validated before compositing changes any
output pixel. A missing or corrupt later asset therefore leaves the entire input
raster unchanged. The persistent cache remains limited to 16 entries; one apply
call temporarily retains its visible immutable context snapshots until completion.
That temporary memory scales with visible contexts, not a rollback copy of the
full photograph. Offscreen contexts remain culled.

Model selection remains provisional: generation can invent structures or details.
Large-gap outpainting with the legacy 0.99 strength can retain dark wedges even
when all output alpha is opaque; full strength resolved the inspected GFX case.
Native CUDA/CPU execution and save/reload have been exercised, but more photography,
corner detail and alternative-model comparisons remain necessary.
