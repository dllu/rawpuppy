# Native narrow-corner generation and save/reload

Added `examples/verify_corner_fill.rs` to exercise real neural generation through
the renderer on an owned copy. It refuses an existing output directory, retains
the generated EXR assets and XMP, records model graph identities/settings, and
reloads the saved recipe with a fresh renderer. Four edge strips sample the
full-resolution perimeter without allocating a complete output photograph.

The GB10 run used matching LibTorch 2.13 CUDA and the previously checked portable
2.13 graphs, selected through an isolated task cache. CUDA was available and
the owned verifier process was observed in NVIDIA's compute-process report with
3,275 MiB allocated at one observation. This is a process snapshot, not peak
allocator or total system memory. Existing NVIDIA library installations were
read only; no other project's packages or processes were modified.

Actual camera dimensions were 8736×11648 with 0.01-degree rotation, 20 requested
steps, default strength/guidance/noise offset and seed zero. Four native fills
completed. After XMP/asset reload, all 8,918 missing perimeter samples were opaque
and finite; all 31,850 originally opaque samples were exactly unchanged. Corner
points appear twice across the four edge strips, so these are sample counts,
not distinct-pixel counts. Both the input fixture and the generated copy retained
their original RAW hash. No original under `~/pictures/raw` was written.

Debug-build first-context generation took 67.54 seconds, including graph checking/loading,
startup and sampling; subsequent contexts took 3.50, 3.47 and 3.46 seconds. The
overall interval after planning was 93.72 seconds, including save/reload and edge
verification. This does not isolate the reason for the slow first call or promise
workstation latency. Filled RGB values ranged from 0.00162 to 0.35682; finite,
opaque values establish conformance, not seamless photographic quality.

Repeated with the optimized release build in a new output directory. It again
filled 8,918 missing samples, exactly preserved 31,850 opaque samples, and kept
both RAW hashes unchanged. First-context generation took 7.74 seconds, followed
by 2.94, 2.93 and 2.93 seconds; the overall interval after planning was 17.08
seconds. The filled RGB range matched the debug observation. These timings come
from separate processes with cached files and a shared workstation; they do not
isolate any single startup component. The example now records its build profile.

The data receipt is in `docs/data/narrow-corner-gb10-2026-10-09.json`. Full artifacts
are local under `/tmp/rawpuppy-validation/narrow-neural-gfx-2026-10-09/evaluation`.
The release receipt is `docs/data/narrow-corner-gb10-release-2026-10-09.json`, with
artifacts in the sibling `evaluation-release` directory. Build/run and strict
Rust 1.99 Clippy for this Moebius-enabled example passed.
Previous implementation tests remain unchanged; this milestone adds executable
real-model verification rather than another sampling implementation. The full
PROMPT.md goal remains active, including broader quality and display validation.
