# Native Windows verification and editor activity

Observed CI `37971658127` for the pinned binding patch `eef8335`: all six jobs
passed. Windows now builds both optional neural features with ordinary MSVC
Cargo configuration and executes all seven real RAW reconstruction checks.
Its log explicitly selects `Cpu`, reports zero CPU/reference difference and
passes tiles, cancellation, large widths, source preservation and renderer reuse.
The retained Windows manifest identifies PyTorch 2.13.0+cpu and zero error at
every independent oracle probe. Linux CPU and macOS MPS jobs passed as well.
This verifies joint RAW execution; Moebius graph execution on Windows/MPS and
broader camera quality remain separate outstanding work.

Fixed an editor race: an earlier save/export failure could clear the busy flag
for a newer load, while a saved reply was identified only by destination path.
Work and completion messages now carry the document generation. Busy activity
records its operation kind/document, so only the owning completion can finish
it. Preview/save failures cannot finish a load, generation or export. Saved
snapshots cannot mark a reopened document clean merely because its path matches.

Durable requests still execute. Their failures identify the failed destination
and remain visible even after changing photos, without clearing the newer
activity or cancelling a different document's pending save/close. Saving during
generation/export preserves that activity's status. Keyboard export/generation
now respect the same busy gating as their buttons, and saving during a pending
load cannot accidentally act on its previous photograph.

The actual-worker regression forces blocked save and export destinations, then
opens the next document and successfully saves/exports it. It verifies the
scopes, destination context, protected activity and unchanged original bytes.
An optional-feature regression rejects an empty generation before loading the
model and finishes only generation activity. All four worker cases passed with
both neural features, and all 45 default tests passed. Strict Clippy over both
features/all targets, formatting and diff checks passed.

Only owned synthetic fixtures were written. Original photographs and other
projects' environments were not modified. The full PROMPT.md goal remains
active; these milestones do not resolve broader quality/platform/display work.
