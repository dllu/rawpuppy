# 2026-10-09 01:24 — Recent masked editing and actual-pixel texture repair

Extended the existing Moebius/Qwen comparison with the Apache-2.0 FLUX.2 klein
4B checkpoint and its official Diffusers mask-based pipeline. This milestone adds
reproducible evaluation tools and evidence; FLUX remains an external evaluation,
while the existing optional native Moebius backend remains provisional. LaMa is
the historical reference rather than the application's quality target.

## Concrete work

- Added `tools/benchmark_flux2_klein.py`, with pinned checkpoint/checksum checks,
  exact input/output identities, explicit dimensions, native-mask inference,
  optional inference-only mask dilation, timing/memory records, retained native
  output and exact final-mask composition. It refuses existing output directories.
- Added `examples/benchmark_moebius.rs` to exercise the production Rust sampler
  directly on sRGB PNG/mask pairs without an editor recipe or Python inference.
  It verifies prepared graphs and unchanged unmasked RGB bytes, and records model
  manifests, sampling settings, latency and hashes.
- Evaluated FLUX on the original public removal input at 512 and 1024 pixels,
  then separately tested withheld selected source pixels and a 16-pixel inference
  halo. Every case has two same-seed runs and retained, inspected outputs.
- Compared native Moebius and FLUX on a withheld 96×96 knitted-fabric patch in a
  512-pixel crop of a full-resolution GFX100S export. The target was blacked out
  before inference; the intact crop was used only for assessment.

## Findings

FLUX took approximately 1.8 seconds warm at 512 pixels with 15.56 GiB peak GPU
allocation. At 1024 it took 6.56 seconds warm with 17.35 GiB. The initial tight-mask
removal retained person-shaped artifacts. Withholding the source did not help;
adding an inference halo produced a substantially cleaner raw removal. Exact
composition through the original selection still retained some original fringe
and associated shadows. This makes selection and inference-mask context part of
the evaluation, rather than a reason to dismiss the model from the first trial.

On the actual-pixel texture crop, native Moebius continued the soft diagonal knit
with a faint square boundary. FLUX invented conspicuously sharper, differently
oriented stitches. Selected-pixel mean absolute/RMS errors on the 8-bit channel
scale were 4.63/5.56 for Moebius and 31.16/43.01 for FLUX. These are reconstruction
errors for one known withheld patch, not general perceptual-quality scores.
Native Moebius took 2.98 seconds warm at 20 steps; FLUX took 1.80 seconds at four
steps. Configurations, precision and frameworks differ, so these measurements do
not isolate architectural speed. Native Moebius memory was not measured here.

All same-seed output pairs were byte-identical within this environment, and all
compositions changed zero RGB bytes outside their original masks. Qwen has not
been evaluated on this texture crop, and neither Qwen nor Moebius has yet received
the same inference halo. No general model winner is established. Qwen 2.1's
current research/evaluation license was rechecked; it still requires separate
commercial permission. September's Apache-2.0 LLaDA-Image/Turbo remains unbenchmarked.

## Validation and provenance

The native example built in release mode with matching LibTorch 2.13. Strict
Rust 1.99 Clippy passed with `moebius,cuda` and with default features. Formatting,
whitespace and Python compilation checks passed. Both tools rejected existing
output directories. All launched evaluation and compilation processes exited.
Application processing code and dependencies were unchanged, so production test
suites were not repeated for these evaluation-only changes.

Four FLUX safetensors files were verified at model revision
`e7b7dc27f91deacad38e78976d1f2b499d76a294`. The official pipeline uses the existing
isolated Diffusers revision `d961a388fd02e4db38d17350c8dd9b8abe642e05` and PyTorch
2.14 environment; other projects' packages were read only. Other workloads shared
the GB10, and a local compiler overlapped part of the texture trial. Startup was
approximately 95–101 seconds for the tested CPU-then-CUDA loader. GPU allocation,
reservation and process RSS are separate measurements, not additive memory totals.

Sources and settings are in [inpainting-comparison.md](inpainting-comparison.md)
and [the six-case data record](data/inpainting-gb10-2026-10-09.json). Photograph
crops and generated outputs stay under `/tmp/rawpuppy-validation`, outside the
repository. Nothing under `~/pictures/raw` was modified.
