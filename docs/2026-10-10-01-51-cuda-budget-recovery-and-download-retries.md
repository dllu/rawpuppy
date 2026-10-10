# CUDA budget recovery and pinned download retries

Added an isolated real-model CUDA allocation probe. tch does not expose its
allocator budget directly, so a small diagnostic bridge calls the matching
LibTorch caching-allocator API. The bridge is an explicitly supplied temporary
library, not linked into production. The probe loads each real model, warms a
baseline, sets its own allocator allowance to 64 MiB, restores the original
fraction with a scope guard, and retries inference on the same model.

RawNIND returns a CUDA out-of-memory error requesting 32 MiB. Moebius returns
the same class of error requesting 128 MiB in its encoder. Both recover with
exactly identical outputs after the limit is restored, including seeded
synthesis. Original synthetic sensor values are unchanged. This verifies these
allocation-error paths, not device-loss recovery or every possible mid-sampler
failure. [PyTorch documents](https://docs.pytorch.org/docs/2.14/generated/torch.cuda.memory.set_per_process_memory_fraction.html)
this budget as local to the process's caching allocator. The actual bridge was
built against the installed matching 2.13 headers/runtime.

The cap does not exhaust global GPU/system memory or change another process's
allocator. Only owned synthetic inputs, temporary artifacts and already verified
model graphs are used. The public record keeps scoped allocation errors and
graph identities in `docs/data/cuda-inference-budget-2026-10-10.json`; detailed
tracebacks stay in `/tmp/rawpuppy-validation/cuda-inference-budget-2026-10-10`.

The preceding TIFF commit's Windows native-learning job failed before graph
preparation because the model publisher returned HTTP 503; the other five jobs
passed. Added a shared pinned-artifact downloader with at most three attempts
for transient HTTP/transport failures. Integrity failures are not retried or
published. Each attempt starts fresh hash/length state and cleans its temporary
file; verified bytes publish without clobbering an existing destination. Both CI
model preparers use it. Exact RAW size/SHA-256 and all Moebius pins remain intact.

Three deterministic tests cover 503 recovery, corruption rejection, exhausted
retries and existing-file safety. They run in native-learning CI. An actual
publisher download also passed its 31,059,270-byte size and pinned SHA-256.
The release CUDA probe, diagnostic bridge build, all 64 standard tests, strict
Rust 1.99 Clippy with Moebius/RawNIND, Python compilation, formatting and diff
checks passed. Physical display and broader camera/MPS evidence remain open.
