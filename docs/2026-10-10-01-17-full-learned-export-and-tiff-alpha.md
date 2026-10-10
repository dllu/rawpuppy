# Full learned export and TIFF alpha metadata

Revalidated the current whole-photo learned path after adding clipping
provenance. A fresh owned copy of the earlier 100 MP GFX fixture and private
XDG model-cache link isolate the run. The release CUDA/RawNIND build uses matching
LibTorch 2.13 and the pinned graph. No original photo directory is written.

The initial 1800-pixel preview, including full camera-RGB preparation, took
19.20 seconds. Three cached exposure previews averaged 5.00 ms on coherent CUDA
system memory. Peak process RSS was 2,829,000 KiB. Initial full RGBA16 TIFF
render/export took 20.79/0.90 seconds and peaked at 5,891,484 KiB RSS. These are
process measurements on a shared workstation; driver/device memory is not fully
accounted by RSS, and this is not a general quality or latency guarantee.

Independent TIFF inspection then exposed a real interoperability defect: four
samples were written but ExtraSamples was absent. Readers had to guess whether
the fourth channel was alpha. Switched the TIFF branch to the direct encoder,
retaining RGBA16 and ICC and explicitly writing unassociated alpha (tag value 2).
This matches the renderer's straight RGB/alpha representation. PNG/JPEG/EXR paths
retain their existing encoders and atomic persistence behavior.

A regression writes transparent, partial and opaque pixels, verifies the alpha
tag and ICC, checks every quantized component against the original, and compares
two decoder outputs. A rebuilt actual CLI repeats the full GFX export: 20.85 s
render and 0.92 s encoding. The corrected 8736×11648 uint16 RGBA file has valid
sRGB ICC and alpha 65535 everywhere; every pixel matches the earlier export
exactly, while the RAW hash remains unchanged. Only metadata interpretation is
corrected. The public record is `docs/data/full-reconstruction-provenance-2026-10-10.json`;
large TIFFs and copies remain under `/tmp/rawpuppy-validation`.

All 64 standard tests, strict Rust 1.99 Clippy for all targets, formatting and
diff checks passed. The preceding CLI-sidecar commit's six CI jobs completed
successfully. Broader photography, physical display and device-pressure evidence
remain open in the full PROMPT.md audit.
