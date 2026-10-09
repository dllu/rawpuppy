# Qwen 2.1 explicit-mask evaluation

The earlier Qwen 2.1 comparison supplied the removal mask as a reference image
to its base editor. Alibaba PAI publishes a newer ControlNet Union adapter with
explicit inpainting conditioning. Evaluated that separate path rather than
treating the base-model trial as evidence about every Qwen inpainting method.

Added `tools/benchmark_qwen_image_21_controlnet.py`. It verifies the pinned
adapter checkpoint, every base parameter's key/shape, and exact adapter coverage
of the configured control branch. It imports unchanged Qwen computation modules
from a pinned external VideoX-Fun checkout, omitting unrelated package aggregator
imports. Existing isolated evaluation dependencies and cached base weights were
reused; other projects' environments were not modified.

Ran the same public 512-pixel removal and withheld actual-pixel GFX100S fabric
contexts with zero and 16-pixel inference-mask growth, twice each: eight samples.
BF16, 40 steps, seed 42, guidance/control scale 1, prefix KV caching, no compile,
quantization or per-step offload. Warm inference took 18.23–18.50 seconds with
38.94 GiB peak PyTorch GPU allocation; initial loads took 185.81–201.04 seconds,
excluding hashing/imports. Allocation is not total system memory.

The inspected tight-mask removal invents a concrete structure; mask growth
instead invents a vertical slab and does not fix that tested removal. Both fabric
repairs plausibly continue the soft diagonal pattern. Against the withheld
original, mean selected-channel errors are 3.389 and 3.221 levels on the 8-bit
scale, versus native Moebius's 4.634 and 4.645 in its earlier matched-mask trials.
This single crop and these different sampling configurations establish no general
quality ranking. Larger native-resolution contexts, shadows/reflections, other
prompts/seeds, detail and corner filling remain open comparisons.

All four configurations produced identical native/raw/composed PNGs across their
two same-seed runs. Independently verified output hashes and exact preservation
of every unselected RGB byte in all eight saved compositions. Native output has
no fully transparent selected pixels, though its minimum alpha is 253/254 rather
than 255. Python composition uses Pillow RGB; application layers compose in
display-linear samples. Original RAW photographs were not modified.

Updated the research shortlist and comparison, retaining exact prompts, source
and checkpoint revisions, hashes, settings, timings and independent validation
in `docs/data/qwen-controlnet-gb10-2026-10-09.json`. The adapter carries the same
Qwen research license; this is an evaluation, not an application backend release.
Moebius remains the provisional native option and LaMa a historical reference.

Validation: actual CUDA evaluation and saved-image inspection, independent
hash/preservation/error checks, Python compilation, JSON parsing, and
`git diff --check` passed. Existing application commit `1735f65` also passed all
CI jobs. The full PROMPT.md implementation goal remains active.
