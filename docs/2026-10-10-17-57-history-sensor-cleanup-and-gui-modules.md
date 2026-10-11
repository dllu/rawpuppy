# History races, sensor cleanup and GUI modules

Fixed the asynchronous generation/undo race. The old acceptance check hashed
the base recipe with generated layers removed. Undoing or redoing a layer edit
therefore left that hash unchanged, allowing an obsolete result to modify the
current document and clear redo. Generation now carries the complete requested
edit snapshot and a document-history revision. A reply must match both before
it changes edits. Automatic perspective uses the same revision guard. View and
display changes keep their separate preview revision and do not invalidate
generation. Accepted append/regeneration operations remain one undoable step.

Extracted bounded history into its own module. Parameter gestures are finalized
before undo, redo or starting asynchronous edits, preventing a later mouse
release from adding an obsolete undo entry. Brush strokes now use the same
200-entry bound as parameter changes. Opening another photo resets gestures
with the rest of its history.

Split hot-pixel repair from bilateral denoising. Every bilateral tap reads the
repaired sensor signal, so an uncorrected defect cannot bleed into neighboring
pixels after its center is repaired. The shared Rust GPU kernel uses the same
stage ordering. Bayer retains its existing same-color four-site stencil;
X-Trans selects the nearest distinct, actual same-color photosites for each
phase, including boundaries. No corrected full-sensor copy is allocated and
original samples remain immutable. X-Trans reconstruction continues to use the
existing CPU fallback rather than the Bayer GPU kernel.

Replaced the 3,342-line `src/gui.rs` with `src/gui/mod.rs` (363 lines), plus
worker, document, history, controls, canvas, display-state, toolbar and curve
modules. Widget, worker, history and generation tests live separately under
`src/gui/tests`. Photo processing remains on the worker and existing native
presentation behavior is retained.

Validation passed 108 default, 119 combined-feature and 104 CPU-only tests,
strict Clippy with CUDA/Moebius/RawNIND/SAM 3, six Vulkan and nine CUDA GPU
controls. New regressions compare denoising a defective sensor with denoising
its defect-free reference across all Bayer layouts and X-Trans. The color-plane
control covers all 36 X-Trans phases and borders. GPU tests verify that adjacent
defects leave no halo with both copied and coherent CUDA memory.

The native release editor also exercised real Moebius regeneration on an owned
101.8 MP GFX copy. A seed-1 regeneration became one history entry. During a
seed-2 regeneration, Undo restored the original seed-0 fill. The completed
seed-2 result was discarded and Redo remained available. Saving the undo state
restored the initial XMP byte for byte; Redo and Save restored the seed-1 XMP
byte for byte. Both displayed photo comparisons were exact over all 470,448
pixels. The owned editor/X server closed normally; the source RAW and starting
fixture were unchanged. Three immutable assets remain, including the discarded
result, so history never loses an asset it might need.

Artifacts are under `/tmp/rawpuppy-validation/history-cleanup-2026-10-10`;
numeric results are in [the validation record](data/history-cleanup-2026-10-10.json).
