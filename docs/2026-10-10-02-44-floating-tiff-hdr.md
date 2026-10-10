# Floating TIFF HDR import and export

The HDR input review found that TIFF import accepted the decoder's F32 result
but rejected F16 and F64. Transfer-function selection also relied on `bits != 32`
rather than the sample format, which would have incorrectly decoded newly
supported 16/64-bit float values as sRGB.

Import now accepts all three float representations, converting to the editor's
float32 working storage. Untagged floating TIFFs assume linear sRGB, extending
the existing F32 policy; ICC-tagged data continues through Little CMS. Integer
TIFF interpretation remains unchanged. Three-channel data reuses the normalized
RGB buffer, avoiding another whole-raster allocation. Unsupported-format errors
no longer format the complete decoded sample vector into their message.

The new fixtures first failed with an unsupported F16 error. With the fix:

- All three float formats retain negative values, midtones and highlights through
  import, +1 EV exposure, library EXR export and reload. The actual CLI also loads
  the saved XMP and produces the same EXR pixels, leaving both input and XMP bytes
  unchanged.
- Linear Rec.2020 ICC fixtures retain signed and above-one working RGB, matching
  an independently calculated primary-matrix conversion within 0.0002. Little
  CMS describes this floating-point behavior in its
  [unbounded CMM paper](https://www.littlecms.com/CIC18_UnboundedCMM.pdf).
- A grayscale F64 TIFF expands linear signed/HDR values correctly. NaN and both
  infinities are rejected for all three formats; F64 values outside finite
  float32 storage are rejected rather than entering the renderer.
- An actual F64 TIFF with orientation tag 6 passes native WGPU and coherent GB10
  CUDA rendering, +1 EV exposure, EXR export and reload. Every component matches
  CPU within 0.00001, output is opaque, negative/highlight values remain present,
  and file bytes and resident source values stay unchanged.

All 71 standard tests passed, as did strict Rust 1.99 Clippy for all targets with
CUDA enabled, formatting and diff checks. The native GPU check is included in
the existing ignored GPU suite used by Linux/macOS CI. Local reproduction:

```sh
cargo test --test color
RUST_MIN_STACK=33554432 cargo test --test gpu imported_float_hdr -- --ignored --nocapture
RUST_MIN_STACK=33554432 RAWPUPPY_TEST_CUDA=1 cargo test --features cuda --test gpu imported_float_hdr -- --ignored --nocapture
```

These are deterministic format/color/workflow fixtures, not a physical HDR
display measurement or a claim about every possible TIFF encoding. The full
PROMPT.md audit remains active.
