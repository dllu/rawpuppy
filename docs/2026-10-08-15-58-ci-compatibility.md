# Newer Rust lint compatibility

The first native-editor CI run compiled the application on Linux, macOS and
Windows, but strict Clippy in Rust 1.99 rejected constant-size `chunks_exact`
in JPEG export. The local toolchain is Rust 1.95, whose Clippy did not flag it.
Replaced that iteration with `as_chunks::<4>()`, preserving the exact output.
The cross-platform test runs must be checked again after this correction.
