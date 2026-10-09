# Native learning CI matrix

The optional LibTorch features previously ran CI only on Linux. Expanded native
learning builds/tests to Linux, Apple Silicon macOS and Windows, and added real
joint RAW reconstruction execution rather than relying on feature compilation.

Added a scratch-only graph preparation helper: download the authors' 31,059,270
byte checkpoint, verify its pinned SHA-256, fetch the pinned external oracle,
and run the independent graph exporter. Attribution is retained, and only the
manifest/NOTICE are uploaded as CI artifacts. No model weights or photographs
are added to the repository. Each job uses an isolated model cache.

The new ignored real-model test compares CPU against the automatically selected
native device across all Bayer phases with signed and above-white sensor values.
It verifies unchanged source data, finite output and camera-linear difference
below `3e-5`. macOS must actually select MPS, with CPU operator fallback disabled;
otherwise that check fails. Existing real-model checks run serially to bound
memory. CPU thread counts are bounded in each CI job.

Local preparation using matching PyTorch 2.13 completed with zero maximum graph
error against the external oracle. The local CUDA comparison passed with maximum
CPU/device difference `1.5497208e-6`. All seven real-model tests passed on Linux
CPU with the fresh graph installed in an owned scratch cache. Strict Rust 1.99
Clippy over both features/all targets, Python compilation, workflow parsing,
environment-writer newline validation, formatting and diff checks passed.

Original photographs and other projects' environments were not modified. Prior
commit `65724ce` passed all CI jobs. The newly expanded matrix still requires
observed macOS/Windows execution; that verification is the next step. The full
PROMPT.md objective remains active.
