# Moebius MPS verification and stable curve dragging

Observed CI `37980551392` for `dbd52ca`: all six jobs passed. The macOS log
explicitly reports `Mps` and successful native sampling with CPU operator
fallback disabled, then successful explicit CPU sampling. Both cases verify
finite output, a reconstructed selection and exact unselected-sample
preservation. Observed sampling was 43.66 seconds MPS / 190.30 seconds CPU on
that CI runner; these are conformance runs, not latency guarantees. Updated the
runtime guide and requirement evidence to remove the obsolete Windows/MPS gap.

Reproduced a tone-curve interaction bug with actual egui pointer events. Starting
on the first interior point, dragging vertically and then across the second
point's x-position changed the second point. The widget previously recomputed
the nearest point every frame. It now selects from the original press position
and retains that point until release. Release/reset clears temporary selection;
a subsequent drag selects independently. Monotone y-bounds remain enforced.

The regression failed with the wrong point values before the fix and passed
afterward. The headless harness explicitly clears unapplied font texture deltas;
it does not create a native window or modify a photograph. The test also checks
the second drag and unchanged endpoints. All 46 default tests, strict Rust 1.99
Clippy/all targets, formatting and diff checks passed.

The session now restricts Git metadata writes and network access. Code and docs
remain reviewable in the working tree; publication must be verified separately.
The full PROMPT.md goal remains active, including remaining photographic,
interaction and display verification.
