# Rawpuppy

A minimal, opinionated, non-destructive RAW photo editor in Rust. One photo,
a fixed processing order, and edits saved as `photo.raf.rawpuppy.xmp` independently
of other editors. Development is ongoing; current capabilities and remaining
requirements are recorded in [docs](docs).

Requires a recent stable Rust toolchain and Little CMS 2 development libraries
(`liblcms2-dev` on Debian/Ubuntu; `brew install little-cms2` on macOS).
Linux X11 windows also require `libxkbcommon-x11-0` at runtime.

```sh
cargo build --release
# NVIDIA CUDA (the default build uses Vulkan / Metal)
cargo build --release --features cuda
cargo run --release
cargo run --release -- edit photo.raf
cargo run --release -- inspect photo.raf
cargo run --release -- export photo.raf photo.png
cargo run --release -- export photo.raf preview.jpg --max-edge 1800 --exposure 1
cargo run --release -- recipe > edits.json
cargo run --release -- save edits.json photo.raf.rawpuppy.xmp
cargo test
```

PNG/TIFF export is 16-bit; JPEG is 8-bit. `--color-space` selects sRGB,
Display P3, Adobe RGB, Rec.2020, or linear sRGB, with embedded ICC profiles.
EXR stores floating-point linear sRGB with explicit primaries
(`--color-space linear-srgb`); tagged HDR input chromaticities are respected.
Use `--overwrite` to replace an existing export. Originals are never overwritten
by export; the CLI defaults to eight CPU workers to share the workstation.
`--backend auto|cpu|cuda|vulkan|metal` selects photo compute. Auto falls back to
CPU for unsupported sources or device limits; GPU sources stay resident between
edits. `cargo build --no-default-features` omits photo compute acceleration.
Eligible coherent CUDA devices share the existing sensor/output allocations;
[system-memory.md](docs/system-memory.md) describes selection and GB10 measurements.

In the editor: drop a photo to open it, scroll to zoom, drag to pan, `F` to fit,
`1` for actual pixels, and `B` to compare. Save with `Ctrl/Cmd S`; undo with
`Ctrl/Cmd Z`. Clone/heal uses Alt-click to choose a source. A custom monitor ICC
can be selected in the editor or passed with `edit --display-profile monitor.icc`.
Automatic display selection and platform policies are described in [display color](docs/display-color.md).
`edit photo.exr --hdr` requests native HDR preview with automatic SDR fallback;
see [HDR presentation](docs/hdr-presentation.md) for signal and platform limits.
New RAF documents use available camera distortion, vignetting and CA corrections; existing
recipes retain their saved behavior. See [lens correction](docs/lens-correction.md).

The optional `moebius` feature adds native AI removal and corner filling, saved as
non-destructive layers. See [runtime setup](docs/moebius.md) and [model evaluation](docs/inpainting-research.md).
The `neural` feature exposes `inpaint-lama` as an experimental comparison backend.
The optional `raw-ml` feature adds experimental joint AI Bayer reconstruction
with a reusable editor/export cache; see [setup and validation](docs/learned-reconstruction.md).
