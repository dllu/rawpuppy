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
is performed in Rust. The same CUDA-prepared graphs have been executed on both
CUDA and CPU; MPS and Windows runtime validation remain outstanding.

## Editor and CLI

Open “AI removal & corner fill,” paint the selected area, then generate. Context
is square in image pixels, with extra surrounding content; portrait or wide photos
are never stretched. “Fill geometric corners” computes a separate region for each
corner containing missing pixels, shifting the square inward where it fits to
retain more photographic context. Sampling count and seed control regeneration.
Generation runs on the photo worker, and ordinary preview updates remain separate.

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
Only exact target pixels are composited; unpainted original pixels remain unchanged.
Corner membership is checked at output resolution, avoiding leftover transparent
slivers from the smaller inference mask. Preview and export share the same layers.
Undo/redo removes or restores layer references without changing the original.

Changing a preceding edit makes its fills stale. Preview shows the current base
image and offers regeneration; export rejects stale fills. A loaded layer can be
rendered without loading the model, and the editor can regenerate the same masks
with new steps/seed. Generated-layer caches are bounded and offscreen layers are
culled. Unreferenced immutable assets are retained for undo/recovery.

Visible layer assets are resolved and validated before compositing changes any
output pixel. A missing or corrupt later asset therefore leaves the entire input
raster unchanged. The persistent cache remains limited to 16 entries; one apply
call temporarily retains its visible immutable context snapshots until completion.
That temporary memory scales with visible contexts, not a rollback copy of the
full photograph. Offscreen contexts remain culled.

Model selection remains provisional: generation can invent structures or details.
Large-gap outpainting can retain dark wedges even when all output alpha is opaque.
Native CUDA/CPU execution and save/reload have been exercised, but more photography,
corner detail and alternative-model comparisons remain necessary.
