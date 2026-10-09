# Recoverable inpainting tensor failures

The native sampler uses tch's convenient tensor operators, which unwrap
recoverable LibTorch errors into Rust panics. The photo worker handles returned
errors but does not catch an escaping sampling panic. Such a failure could stop
the worker and poison the process-wide sampler mutex, preventing another attempt.

Sampling now executes inside a panic boundary while the RNG mutex remains held.
The thread-local no-grad guard and temporary tensors unwind within that boundary;
the panic becomes a returned generation error before the mutex guard is dropped.
Input slices and frozen model weights remain immutable. Ordinary returned errors
continue to propagate, and each operation resets its explicit seed. The same
sampling body and arithmetic are used by the successful path.

A real two-value CPU tensor reshaped into three values reproduces the failure
without memory exhaustion or CUDA stress. The regression failed before the
boundary, with the tensor panic escaping the sampler. Afterward it checks the
returned error, an unpoisoned mutex, a subsequent seeded operation producing the
same eight random values, and restored gradient tracking. The harness clears a
poisoned mutex only on its failing pre-fix path before failing the test, preserving
isolation from other tests. This establishes recoverable exception handling;
fatal allocator aborts or lost-device recovery are not claimed by this check.

All 58 tests in the Moebius build and strict Rust 1.99 Clippy for all targets
passed. The change is confined to the optional Moebius module. Formatting and
diff checks passed. Retained logs are
`/tmp/rawpuppy-validation/sampler-panic-{before,after}.log` and
`sampler-recovery-{feature-tests,clippy}.log`.

The updated release example also repeated real CUDA generation on the owned
8736×11648 GFX fixture rotated by 0.01 degrees. After saving/reloading the layers,
all 8,918 missing perimeter samples became opaque and all 31,850 original opaque
samples remained exactly unchanged; both input hashes stayed unchanged. The
receipt is `docs/data/sampler-recovery-gfx-gb10-2026-10-09.json`, with artifacts
under `/tmp/rawpuppy-validation/sampler-recovery-gfx-2026-10-09`. Perimeter counts
include duplicate corner samples, and this is conformance evidence rather than
a new photographic-quality ranking. Original photographs were not written.

The full PROMPT.md goal remains active, including broader quality, display and
device verification.
