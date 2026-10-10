# Streaming PNG export

PNG export previously allocated a complete RGBA16 conversion beside the float
raster, followed by the image adapter's complete byte-order copy. This made
temporary storage scale with image area. It also delegated the final PNG chunk
to the adapter's destruction path, which cannot report a final-chunk write error.

The exporter now uses the existing PNG 0.18.1 codec directly, without adding
another codec version. It converts pixels to big-endian RGBA16 in two reusable
1 MiB buffers. Scoped Rayon tasks overlap encoding one block with converting the
next; both finish before a buffer swap or write-error return. The codec retains
its filtering rows and compression state, so width still affects that workspace.
No image-size cap is introduced. PNG and TIFF share the same final color transform
and quantization helper. Fast compression, ICC and straight alpha are retained.
The stream and writer are explicitly finished, including the IEND write, before
the shared flush/sync/atomic-publication path.

Tests decode every component for all five output spaces, transparent/partial/
opaque alpha, clipped negative/highlight RGB, wide rows and partial blocks.
Injected IDAT and IEND write failures return errors with unchanged source pixels;
the actual CLI file-size-failure regression still preserves existing destinations
and cleans staging files for every format.

The opt-in `verify_png_export` example generates deterministic float RGBA,
exports through the public path, verifies every source pixel unchanged, drops
the source and decodes the PNG one row at a time. It checks all quantized values,
exact alpha, ICC and finalization. An old-encoder baseline was built from
990180f's export implementation with only the probe/direct dependency added.
On this shared GB10 workstation with eight CPU workers:

| 8,736 × 11,648 / 101,756,928 pixels | Export time | Peak process RSS |
| --- | ---: | ---: |
| Previous full-buffer encoder | 1.764 s | 3,456,160 KiB (3.30 GiB) |
| Pipelined streaming encoder | 2.184 s | 1,595,600 KiB (1.52 GiB) |

Memory fell by about 54%; this change trades some encoding time for bounded
temporary storage. Timings are single measurements of a synthetic raster,
including file sync; they exclude RAW decoding/reconstruction/rendering and
are not a universal photographic performance claim. An earlier streaming run
overlapped this task's build, so a repeat without that build was used to assess
speed before adding the pipeline. Process RSS includes the full float source.

The probe checked every one of the 101.8 million decoded pixels with zero
quantized RGB error, exact alpha and unchanged source values. Independently,
OpenCV 5.0.0 using LibPNG 1.6.58 decoded the complete baseline and pipelined PNGs;
all four channels matched exactly at every pixel. The 100,003 × 3 probe also
passed complete Rust and independent LibPNG component checks. Numerical results
are in [png-export-2026-10-10.json](data/png-export-2026-10-10.json).

```sh
cargo build --release --example verify_png_export
target/release/examples/verify_png_export /tmp/new-wide.png
target/release/examples/verify_png_export /tmp/new-gfx-sized.png --width 8736 --height 11648
```

All 75 standard Linux tests and strict Rust 1.99 Clippy for all targets with CUDA
enabled passed, along with formatting/diff checks. The preceding commit's six
desktop/native-learning CI jobs completed successfully. The full PROMPT.md audit
remains active.
