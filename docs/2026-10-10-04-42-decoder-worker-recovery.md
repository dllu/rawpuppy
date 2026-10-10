# Decoder failure and editor worker recovery

The input-recovery review reproduced a dependency panic with a deliberately
malformed synthetic DNG. Its CFA tag contains three entries while its repeat
dimensions declare 2 × 2. Rawler panics with `Unknown CFA size "RGG"` rather
than returning an error. The editor's worker previously unwound and disconnected,
so it could not process the next queued photograph.

Image loading and metadata-only color-revision lookup now share a narrow input
boundary that converts Rust decoder unwinds to ordinary errors with the input
path and underlying message. Decoder/preparation buffers are owned inside the
boundary and unwind there; the long-lived worker, renderer and channel remain
available. Successful decoding and returned errors keep their existing paths.
No process-wide panic hook is changed.

The actual editor/channel regression first failed with a disconnected worker.
With the fix, the malformed RAW returns `ErrorScope::Load(1)`, the same worker
opens a valid PNG as request 2, and request 3 produces a complete opaque preview.
The metadata-only lookup also returns the expected error. Both file byte streams
remain unchanged. All test files are generated in an owned temporary directory;
no user photographs are modified.

All 77 standard tests, three DNG file regressions, strict Rust 1.99 Clippy for
all targets with CUDA enabled, formatting and diff checks passed. This covers
recoverable Rust unwinds at the input boundary rather than process aborts or
fatal device failures. Broader camera/platform checks remain in the full
PROMPT.md audit.
