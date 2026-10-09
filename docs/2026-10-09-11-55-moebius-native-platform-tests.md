# Moebius native platform tests

The real RAW graph now runs across desktop platforms, but Moebius execution was
still exercised only on local CPU/CUDA. Expanded the native-learning matrix to
prepare the actual inpainting graphs and execute native sampling on each desktop
platform, with explicit MPS selection and disabled CPU operator fallback on macOS.

Added `tools/prepare_ci_moebius.py` for fresh scratch-only preparation. It obtains
the pinned checkpoint/VAE and source revision, verifies hashes, preserves source
line endings and invokes the existing exporter on CPU with two threads. Local
validation reads already cached verified weights; CI downloads them into its
own scratch directory. Runtime graphs/weights and photographs are not uploaded;
only the graph manifest joins existing RAW provenance artifacts.

The exporter now requires the unchanged pinned checkout and a fresh output,
records preparation versions/source identity, and rejects non-finite or excessive
trace/freeze errors. Inference imports omit the unused teacher and Python image
pipeline package aggregators; evaluated computation modules are unchanged. An
initial missing-OpenCV import exposed that unused pipeline dependency before any
model construction. The namespace import correction passed full preparation.

With PyTorch 2.13.0+cpu and pinned Diffusers 0.41.0 dependencies, trace comparisons
were exact. Frozen encoder/decoder errors were `8.1062e-6` / `8.7917e-6`, and the
denoiser matched exactly. Full helper preparation took 57.90 seconds and about
3.14 GiB peak process RSS on this shared workstation; this is not a CI latency
guarantee or total system-memory measurement.

The CPU-prepared graphs passed native CUDA and explicit CPU sampling, preserving
every unselected original sample, returning finite data and filling the selected
hole. Sampling took 2.13 seconds CUDA and 18.60 seconds CPU in these debug test
runs; graph loading/hash checks are separate and much slower in debug mode.
Existing cached application graphs were not replaced.

Strict Rust 1.99 Clippy over both features/all targets, Python compilation,
workflow/environment parsing, formatting and diff checks passed. Previous
editor commit `8e8cf8f` passed all six CI jobs. The new Moebius Windows/MPS
execution still needs observed CI evidence; the full PROMPT.md goal stays active.
