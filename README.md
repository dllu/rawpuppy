# Rawpuppy

A minimal, opinionated, non-destructive RAW photo editor in Rust. One photo,
a fixed processing order, and edits saved as `photo.raf.rawpuppy.xmp` independently
of other editors. Development is ongoing; current capabilities and remaining
requirements are recorded in [docs](docs).

Requires a recent stable Rust toolchain and Little CMS 2 development libraries
(`liblcms2-dev` on Debian/Ubuntu; `brew install little-cms2` on macOS).

```sh
cargo build --release
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
EXR stores floating-point linear sRGB (`--color-space linear-srgb`).
Use `--overwrite` to replace an existing export. Originals are never overwritten
by export; the CLI defaults to eight CPU workers to share the workstation.

In the editor: drop a photo to open it, scroll to zoom, drag to pan, `F` to fit,
`1` for actual pixels, and `B` to compare. Save with `Ctrl/Cmd S`; undo with
`Ctrl/Cmd Z`. Clone/heal uses Alt-click to choose a source. A custom monitor ICC
can be selected in the editor or passed with `edit --display-profile monitor.icc`.
