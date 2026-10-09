# Actual Qwen Image 2.1 comparison

Downloaded pinned Qwen Image 2.1 artifacts outside the photograph directories and
verified all seven safetensors SHA-256 values. An isolated research environment
uses the official Diffusers implementation without modifying another project's
dependencies. Added a reusable benchmark that retains native RGBA, RGB and exact
mask-composed output, model/input/output identities, prompt, sampling settings,
timing and memory measurements. Python syntax and actual CUDA inference passed.
The native 1024-pixel output failed visual/alpha inspection, despite inference
completing without an error; it is recorded as a failed quality trial.

The same original 512-pixel image and mask previously used for Moebius now have
Qwen results. At 40 steps, Qwen took 17.39 seconds first and 13.72 seconds warm,
with 32.98 GiB of peak GPU allocation. Its removal avoided the tested Moebius
result's invented graffiti-covered structure. It also changed unmasked shadows;
exact mask composition preserves outside pixels but exposes seams and retains
those shadows. The 1024-pixel trial took 67.86 seconds and 40.91 GiB and produced
magenta/striped areas and transparent pixels within the target. Its cause remains
unresolved rather than attributed to the model generally.

Direct CUDA loading did not show an improvement over the baseline loader in this
trial. It reproduced exactly the same 512-pixel RGB PNG and showed native alpha
of at least 253 throughout. Two repeated baseline runs also produced identical
PNGs. Measurements and limitations are documented in
[the comparison](inpainting-comparison.md), with full identities and numeric
records in [the data artifact](data/inpainting-gb10-2026-10-08.json).

Updated model research to reflect actual comparison evidence. Moebius remains a
compact native pilot rather than a final quality choice; Qwen remains an evaluation
reference under its research license. Native 2K, unpadded contexts, more masks and
seeds, textures/fine structures/corners, and permissive September models still
need comparison. The application's default Rust features are unchanged.

All temporary photographs, outputs, logs and model caches stayed outside
`~/pictures/raw`. The inference processes exited normally. The preceding display
milestone passed all desktop and native Moebius CI jobs, including the actual
macOS Metal parity probe.
