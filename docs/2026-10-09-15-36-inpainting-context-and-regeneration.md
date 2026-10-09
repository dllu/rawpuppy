# Inpainting context placement and regeneration

Reproduced wasted brush context near canvas edges. A selection at normalized
`[0.01, 0.01]` in a 11648×8736 canvas produced context
`[-0.038, -0.054, 0.096, 0.128]`, extending into empty space despite room to place
the square inside the photograph. Brush generation now uses the same inward
placement as geometric corner filling. Oversized contexts retain their square
physical shape and extend outside only on axes where they cannot fit.

The new edge regression failed before this change and passed afterward. It covers
all four corners of landscape, portrait and 100003×17 canvases, checks physical
square shape, and samples the circular brush boundary inside the canvas to check
coverage. Existing exact unpainted-sample, saved-layer, corruption rollback and
corner-alpha tests also pass. Context placement alone does not prove better
photographic synthesis quality.

The editor previously reused saved context rectangles during regeneration even
after crop/aspect changes. It now replans brush contexts for current dimensions
and probes current geometric gaps using the base photograph. Saved brush
selections remain intact; current corner jobs are inserted once, at the first
old corner group's position, rather than repeated for every obsolete layer.
If no gaps remain, replacement can be empty and remove the obsolete references
without inference. The UI explains that the updated edits need saving. Existing
immutable assets remain available for undo/recovery.

Planning tests cover an aspect change, changed corner position, duplicate old
corners, retained brushes and a gap-free result without mutating saved layers.
A real editor-worker test verifies successful empty replacement for an unrotated
canvas with an intentionally nonexistent old corner asset. It checks original
bytes, absence of sidecars/assets, and reply identity. No model is loaded in
that case. All 52 normal tests in the Moebius build passed with matching LibTorch
2.13 CPU; this run does not repeat the separately ignored real-model samplers.
All 49 default tests also passed. Rust 1.99 strict Clippy for all targets with
Moebius enabled, formatting and diff checks passed. Logs are under
`/tmp/rawpuppy-validation/context-regeneration-*.log`.

Git metadata remains read-only, so publication is unavailable in this session.
Changes are reviewable in the working tree and exported patch. The full PROMPT.md
goal remains active, including broader photographic, display and device checks.
