# Native failure status and current GFX workflow

The current combined release (`cuda,raw-ml,moebius`, LibTorch 2.13) was run on a
fresh ordinary copy of the 101.8 MP GFX100S RAF in an owned Xvfb session. The
full native workflow exposed a UI bug: requesting corner fill on an image with
no gaps correctly returned a scoped error and cleared busy state, but the footer
continued saying “Generating local fill…” even after dismissing the error.

Applicable errors now replace progress text with an operation-specific failure
when no operation remains active. An older save/export error still displays its
destination message, while preserving a newer load's busy state and progress.
The poll-level regression covers opening, preview, save, export and generation,
plus those delayed durable errors. It failed before the change. The revised
native release showed “Could not generate fill” for the same no-gap request,
including after dismissal, and remained usable.

The native session also selected Joint AI and completed preparation on the full
GFX source. Exposure/save/undo/save/redo/save persisted **1.8 → 0 → 1.8 EV** with
Joint AI retained. A 39-dab painted selection generated one native Moebius layer
with 20 steps, seed 0, strength 1, guidance 2 and offset 0.0357. The asset checksum
matches its saved recipe. The installed graph cache used here is recorded by its
actual manifest hash; it differs from the portable graph in the earlier workflow.

This removal is a **photographic quality failure**: a recognizable background
person and a mismatched vertical brown patch remain. The current mask, exposure
and graph cache differ from the earlier workflow, so this does not isolate a
cause or support a model-wide conclusion. Successful inference, opaque selection
composition and persistence are separate from successful object removal.

Compared displayed native photo rectangles: 4,010 of 470,448 pixels changed, with
none outside the saved selection expanded by 0.004 normalized width for display
filtering. Existing compositor tests cover exact output-resolution preservation.
Reopening through a separate cache containing RawNIND and **no Moebius directory**
reproduced all 470,448 displayed photo pixels exactly. RAW and XMP hashes remain
unchanged. Both native editors closed via WM_DELETE_WINDOW with exit 0; both owned
editor/Xvfb pairs were cleaned up and their process handles confirmed absent.

All **87 default** and **95 combined-feature** tests, strict all-target Clippy
with all three features, formatting and diff checks passed. The preceding wide
RAW milestone passed all six desktop/native-learning CI jobs. This change keeps
the full PROMPT.md goal active; the observed inpainting quality failure remains
unresolved rather than being hidden by execution/persistence checks.

Settings, source/model/asset identities and numeric comparisons are retained in
[the data record](data/native-error-status-2026-10-10.json). Photos, screenshots
and the machine-specific interaction harness remain under
`/tmp/rawpuppy-validation/native-current-gfx-2026-10-10`.
