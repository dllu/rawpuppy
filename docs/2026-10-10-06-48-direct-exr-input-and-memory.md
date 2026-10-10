# Direct EXR input and memory

The EXR input audit found two avoidable full-raster allocations. The image
adapter decodes into its own float buffer and copies it into a second one;
converting RGBA into the editor's RGB buffer allocates another raster. Its
allocation budget was already disabled, but these copies still inflate memory
on large HDR photographs.

Input now reads EXR blocks directly into one owned RGB vector with fallible
allocation. Header selection and decoding use the same initialized file reader.
The first non-deep RGB layer determines the window and chromaticities; metadata-
only color-revision lookup uses the same layer selection. Existing display-window
placement, crop/padding, alpha omission, half/float conversion and in-place
working-space conversion are preserved. Coordinates are widened before window
offset arithmetic. Nonfinite RGB is rejected by the existing source constructor.

Four regressions cover a depth layer preceding half-float RGB, signed/HDR values,
positive/negative data-window offsets and clipping, distant legal window origins,
NaN and both infinities, and a file without RGB channels. Source bytes remain
unchanged. Before the implementation, the nonfinite and metadata-only depth-file
checks failed; window placement passed. The reference EXR implementation limits
window coordinates to its documented bounds within the signed storage type, so
the distant-window fixture uses those legal bounds.

An owned **100,003 × 400** RGBA fixture decodes to **480,014,400 RGB bytes**.
Separate release processes with eight Rayon workers measured:

| Input path | Decode time | Peak process RSS |
| --- | ---: | ---: |
| Previous image adapter | 0.659 s | 1,262,624 KiB / 1.20 GiB |
| Direct RGB | 0.286 s | 481,592 KiB / 0.46 GiB |

Peak RSS fell **61.9%**. The decoded float RGB SHA-256 is identical to the
baseline, all components match the expected signed/HDR values within 2.4e-7,
and the input hash is unchanged. These are observations on a shared machine,
not general latency promises; hashing is outside the decode timer.

A larger **100,003 × 1,018 / 101,803,054-pixel** file also passed every decoded
component: **1,221,636,648 RGB bytes**, 0.708 s decode, **1,205,344 KiB / 1.15 GiB**
peak RSS, the same error bound and unchanged file hash. An initial probe overlapped
fixture generation and correctly failed its immutability check; it was excluded.
The reported run started after generation completed.

The opt-in [probe](../examples/verify_exr_input.rs) generates a constant fixture
without first allocating a full image, then measures import in a separate run:

```sh
cargo build --release --example verify_exr_input --locked
target/release/examples/verify_exr_input /tmp/new-photo.exr --generate --height 1018
target/release/examples/verify_exr_input /tmp/new-photo.exr --receipt /tmp/new-receipt.json
```

All **86** standard tests, strict CUDA/all-target Clippy, formatting and diff
checks passed. Existing Rec.2020 EXR color/revision checks and the oriented float
TIFF → Vulkan/coherent CUDA → EXR import round trips passed. Measurements and
hashes are retained in [the data record](data/exr-input-2026-10-10.json). This is
input allocation and correctness evidence; deeper image formats and physical
colorimetric verification remain in the complete PROMPT.md audit.
