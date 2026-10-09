# Native bridge portability

Observed Windows native-learning failure in CI `37968716966`: C++20 resolved
the LibTorch header requirements, but the pinned binding's `module` alias at the
start of function declarations is parsed as a C++20 module declaration. A local
Clang/MSVC-driver grammar probe reproduced that error. The binding also passes
an MSBuild-only `/p:DefineConstants` argument directly to the compiler, which
Clang rejects. Changing only the caller's language flags is insufficient.

Vendored the published MIT/Apache-2.0 `torch-sys` 0.26.0 crate (~2.1 MiB), with
licenses, pinned upstream revision and original packaged-file hashes. The patch
changes only three implementation files: rename the bridge's alias/locals to
`torch_module`, select C++20/conforming MSVC mode, and use the compiler's proper
preprocessor-definition API. Exported C symbols, pointer types and the Rust API
are unchanged. LibTorch and model weights are not bundled. Cargo's registry and
other projects remain untouched.

Removed the CI-only language override; ordinary Windows Cargo builds now receive
the same setup from the pinned local dependency. Updated native-runtime guidance
and recorded the patch scope in `vendor/torch-sys/RAWPUPPY.md`.

Validation: the corrected C++20 grammar/definition probe compiled; strict Rust
1.99 Clippy over both neural features/all targets passed. Both real Moebius
tests passed on GB10 CUDA and CPU, preserving unselected samples. All seven
real RAW reconstruction tests passed through the patched bridge, including
camera-linear CPU/CUDA difference `1.5497208e-6`, unchanged originals, tiles,
cancellation and renderer reuse. Formatting/diff checks and a provenance check
confirming exactly three changed upstream implementation files passed.

Windows and macOS execution through this binding patch must now be observed in
CI. Previous native MPS and Linux CPU jobs passed, but those results alone do
not prove the new Windows bridge. The full PROMPT.md objective remains active.
