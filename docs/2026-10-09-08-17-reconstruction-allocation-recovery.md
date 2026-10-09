# 2026-10-09 08:17 — Reconstruction allocation rejection and recovery

The native joint reconstruction cache now reports its requested byte count when
fallible allocation fails. A real-model Linux CPU probe warms the graph and
workers, then restricts only its own virtual address space with RLIMIT_AS. The
4096×4096 sensor needs a 201,326,592-byte camera-RGB cache; a 64 MiB address-space
headroom rejects it before the first of 16 tiles. Shared machine RAM is not
exhausted, and no other process limit is changed.

After restoring the original limit, patch inference using the same loaded model
is identical to its pre-failure result. Every sensor sample remains unchanged.
The graph's existing allocation path returns an error and exposes no partial RGB
source. A separate worker test verifies a failed request does not retain its
original allocation or poison the queue; the next request succeeds.

The checked-in `reconstruction_memory_probe` reproduces the owned-process test
with the external verified graph and matching LibTorch. The exact address-space
limit and allocation error are retained in
[the data record](data/reconstruction-memory-2026-10-09.json).

Validation: real prepared model on CPU, six native-learning library tests,
strict Rust 1.99 Clippy across raw-ml targets, formatting and diff checks passed.
The preceding live-HDR CI run 37946789137 also passed all four jobs. This is CPU
address-space rejection coverage, not a CUDA/MPS allocator recovery or global OOM
claim. Broader model quality, device memory and platform coverage remain, and the
full PROMPT.md goal remains active.
