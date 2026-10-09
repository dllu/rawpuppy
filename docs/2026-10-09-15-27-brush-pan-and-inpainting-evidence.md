# Brush panning and current inpainting evidence

Canvas mask and clone/heal painting previously accepted generic egui click/drag
responses. A middle-button drag therefore both panned and painted. Both painting
branches now require primary-button input. Clone-source selection requires an
Alt-primary click, and holding Alt suppresses clone/heal painting.

A headless regression drives actual egui pointer events through the shared brush
input predicate. It checks that a middle-button drag remains a pan gesture
without painting, then releases and verifies a primary-button drag paints. The
test clears unapplied font texture deltas and does not open a native window or
modify photographs. All 47 default tests passed, as did Rust 1.99 strict Clippy
for all targets, formatting and diff checks. Logs are retained locally under
`/tmp/rawpuppy-validation/brush-default-tests.log` and `brush-clippy.log`.

Rechecked the Moebius publisher repository and Qwen Image 2.1's published license.
Moebius remains the integrated provisional compact backend; the author explicitly
licenses code and weights Apache-2.0. Qwen 2.1's September 20 research agreement
limits use to research/evaluation and requires separate permission for commercial
use. Existing GB10 comparisons include both the base Qwen reference-mask path
and the explicit-mask PAI adapter, plus FLUX.2 klein. Their measured quality
failures and limited photographic coverage remain documented; integration and
speed do not establish a universal quality winner.

Corrected the research guide's stale statement that Windows/MPS execution was
untested. Previously observed CI run `37980551392` verifies those platforms;
this current GUI change has local test evidence and has not run in remote CI.

Git metadata is read-only in this session, so committing and pushing remain
unavailable. Changes remain reviewable in the working tree and an exported patch.
The full PROMPT.md goal remains active. Original RAW photographs were unchanged.
