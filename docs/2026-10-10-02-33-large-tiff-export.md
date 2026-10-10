# Large TIFF export

TIFF export previously always selected the classic encoder and allocated a
complete RGBA16 conversion buffer beside the rendered float image. Classic TIFF
has 32-bit offsets; [BigTIFF extends them to 64 bits](https://libtiff.gitlab.io/libtiff/specification/bigtiff.html).
That restriction was unnecessary for exports supported by the workstation's
available memory.

The exporter now estimates uncompressed RGBA16 size plus ICC data, both strip
metadata arrays and fixed directory overhead. Saturating arithmetic selects
BigTIFF before classic offsets overflow, including dimensions whose byte count
would overflow u64. Small files retain classic TIFF. Conversion uses reusable
strips of about 1 MiB, or a single row for exceptionally wide images, retaining
the existing output-space transform, quantization, ICC and unassociated alpha.
Atomic temporary-file publication and overwrite policy remain in place.

Regression checks cover both header versions, exact ICC/alpha and every decoded
component, multiple strips including a partial final strip, and 100,003-pixel
width. Selection checks include a real 100,003-pixel-wide near-boundary pair,
additional ICC overhead and overflowing dimensions.

The opt-in `verify_large_tiff` example allocates a deterministic float RGBA source,
exports through the public atomic path, verifies every source pixel unchanged,
then drops that source and decodes selected strips without loading the full
encoded raster. Its small default export uses classic TIFF. On this GB10 host:

```sh
cargo build --release --example verify_large_tiff
target/release/examples/verify_large_tiff /tmp/new-small.tiff
target/release/examples/verify_large_tiff /tmp/new-large.tiff --height 5371
```

The 100,003 × 5,371 export produced **4,297,015,760 bytes**, used version 43,
and stored its final strip at offset **4,296,129,464**, beyond the classic limit.
Export took 2.991 seconds with eight CPU workers; the entire allocation/export/
verification probe took 5.48 seconds and peaked at 8,396,736 KiB process RSS
(about 8.01 GiB, including the existing full float source). This measures export
of a synthetic rendered image, not RAW decoding/reconstruction/rendering.

Rust decoded strips 0, 2685 and 5370 and validated all components and alpha.
System LibTIFF 4.5.1 independently read those same strips with zero quantized RGB
error and exact alpha, identified BigTIFF and its high offset, and read the RGB
ICC and straight-alpha tags. It warned about tag 317 (Predictor=None on an
uncompressed image), but decoded the data successfully. This checks selected
encoded strips rather than every strip. The numerical receipt is
[bigtiff-export-2026-10-10.json](data/bigtiff-export-2026-10-10.json).

All 67 standard tests passed, as did the final targeted TIFF checks, strict Rust
1.99 Clippy for all targets, formatting and diff checks. The preceding commit's
six desktop/native-learning CI jobs also completed successfully. Broader camera,
display and device audit items in the full PROMPT.md goal remain open.
