# Large saved layer loading

The file-size audit found that saved generated EXR assets still used the image
adapter's default 512 MiB decode budget. Loading then copied the decoded float
raster into RGBA storage. This was an artificial limit unrelated to available
system memory, even though ordinary image input already disabled that budget.

The opt-in `verify_large_layer` example reproduced the problem with a
100,003 × 336 layer: **537,616,128 decoded RGBA bytes**, just above 512 MiB.
The layer and XMP saved successfully, but a new layer cache failed to reload it
with `Memory limit exceeded`. This used an owned synthetic original and available
system memory well above the probe's requirements.

The final loader uses the existing EXR crate to decode directly into one owned
`Vec<[f32; 4]>`, with checked pixel counts and fallible reservation. It removes
the image adapter's allocation cap and full-raster intermediary/copies. Asset
hash verification still precedes decode, samples must be finite, and the cache
receives the completed immutable image. All fallible layer preparation remains
before composition, preserving the existing rollback behavior on bad assets.

The same large probe now passes store, XMP save/reload, fresh-cache decode and
complete composition. All 33,601,008 pixels retain **[1.5, −0.125, 0.625, 1]**
exactly; source pixels, original bytes and sidecar bytes are unchanged.
Store took 1.393 seconds, cold load plus composition 1.501 seconds, and the whole
probe 3.41 seconds. Peak process RSS was 1,074,308 KiB (about 1.02 GiB), including
the probe's source/cache and destination/cache phases. These single synthetic
measurements are not neural-inference or photographic-quality evidence. The
numerical record is [large-layer-2026-10-10.json](data/large-layer-2026-10-10.json).

A regular regression verifies a cold native EXR load with negative/HDR RGB and
zero/partial/opaque alpha, comparing every component exactly. Reloading from
the same cache returns the same Arc; original bytes remain unchanged.
Reproduce the opt-in large check with a new output directory:

```sh
cargo build --release --example verify_large_layer
target/release/examples/verify_large_layer /tmp/new-large-layer
```

All 78 standard tests, three DNG regressions, strict Rust 1.99 Clippy for all
targets with CUDA enabled, formatting and diff checks passed. Decoder workspace
and the resident layer/destination still require system memory; this removes the
application's arbitrary decode budget rather than promising unlimited memory.
The broader PROMPT.md goal remains active.
