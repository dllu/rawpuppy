# Bounded JPEG conversion

JPEG export previously retained both a full RGBA8 conversion and a full RGB8
copy before encoding. For a 101.8 MP photograph these require approximately
712 MB in addition to the immutable float source. Export now supplies the
encoder with an image view backed by one fallibly allocated RGB stripe. Stripes
align with its 8-row block traversal and convert in parallel with Rayon.
Scratch is at most 1 MiB except for one minimum block at very wide legal JPEG
dimensions; at the 65,535-pixel format limit it is 1,572,840 bytes.

The initial direct float view avoided the copies but converted serially during
encoding, slowing the control from 1.41 to 2.78 seconds. The bounded parallel
stripe keeps the memory reduction while restoring the observed export time.
At 8,736 pixels wide its 32-row buffer occupies 838,656 bytes. There is no
additional full raster or change to the public RGBA conversion helper.

An owned generated **8,736 × 11,648 / 101,756,928-pixel** signed/HDR/alpha control
was measured in separate release processes with eight Rayon workers:

| Path | Export time | Peak process RSS |
| --- | ---: | ---: |
| Previous full RGBA/RGB conversion | 1.4135 s | 2,322,484 KiB / 2.22 GiB |
| Direct serial float view, discarded implementation | 2.7753 s | 1,593,440 KiB / 1.52 GiB |
| Bounded parallel stripe | 1.4240 s | 1,594,228 KiB / 1.52 GiB |

Peak RSS fell **31.4%**. These single observations on a shared machine show
similar timing for the old and final paths, not a general performance guarantee.
The final timing started after our tests and build checks finished. An earlier
stripe timing overlapped those checks and is retained separately, excluded from
the final comparison. RSS includes the generated float source and the probe;
it is neither ordinary GUI memory nor aggregate GB10 memory.

Every source float remained unchanged. All outputs contain **32,991,687 bytes**.
Every JPEG byte agrees with the baseline after ignoring only the ICC header's
12 creation-date bytes, including the complete encoded scan and every other
profile byte. Different creation times account for different whole-file hashes.
A regression instead reuses exactly the same ICC bytes and requires whole JPEG
byte identity across all five output spaces, including 1×1, partial 8-pixel
blocks, multiple stripes and the maximum legal width. Signed/HDR input clipping,
RGB quantization, alpha omission, quality 95, subsampling and ICC tagging retain
their existing behavior. Atomic flush/sync/publication remains in the same path.

The opt-in [probe](../examples/verify_jpeg_export.rs) can reproduce the memory
control using a fresh output path:

```sh
cargo build --release --example verify_jpeg_export --locked
/usr/bin/time -v target/release/examples/verify_jpeg_export /tmp/new-photo.jpg
```

All **90** standard tests and strict all-target Clippy with CUDA, joint RAW ML
and Moebius passed; the expanded JPEG maximum-width regression also passed.
Formatting and diff checks passed. Exact timings, hashes and check receipts are
in [the data record](data/jpeg-export-2026-10-10.json). The preceding commit's
[CI run](https://github.com/dllu/rawpuppy/actions/runs/38070430176) passed all six
desktop/native-learning jobs, including its mandatory native Metal parity step.
This milestone checks export memory and encoding integrity, not photographic
model quality or completion of the full PROMPT.md audit.
