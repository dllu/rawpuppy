# LLaDA-Image-Turbo reference editing comparison

Evaluated another recent model rather than treating Moebius as a final winner.
The publisher released [LLaDA-Image/Turbo](https://github.com/inclusionAI/LLaDA-Image)
on September 4, 2026. Its model card identifies Apache-2.0, and the inspected
pipeline, transformer and custom text/MoE sources retain Apache notices. Turbo
uses four-step instruction-guided/reference editing; its official API has no
pixel-mask argument. A native Rust integration was not assumed from this Python
runtime evaluation.

Pinned publisher code at `e7c861b0aaa00d2f7ed49600a3a6f170e02a9d59` and the
[Turbo snapshot](https://huggingface.co/inclusionAI/LLaDA-Image-Turbo) at
`f4afc52d925bbac4e22a1c947111fc1f127e37e5`. Downloaded **49,260,806,115 bytes** into
an owned evaluation directory and verified all **34** required files: safetensors
SHA-256s against pinned metadata, configuration/source Git blob hashes and sizes.
The apparent 6B diffusion model also loads a large MoE text encoder; the complete
memory cost must include those components.

Loading first failed with Transformers 5.17: the model expects a `default` entry
removed from `ROPE_INIT_FUNCTIONS`. An owned environment using the publisher's
Transformers **4.57.6** then exposed a mismatch with the reused Diffusers development
build's newer Hub API. Matching publisher Diffusers **0.39.0** resolved it without
patching source or modifying other project installations. PyTorch **2.14.0+cu130**
and CUDA dependencies were reused read-only for GB10 support; this external
evaluation is separate from Rawpuppy's native LibTorch 2.13 binding.

The image is the same encoded 512-square GFX context from the exact Moebius
selection diagnosis, with its complete 49,604-pixel target. LLaDA receives one
original reference and an explicit prompt to remove the smaller background person
while preserving the foreground, framing and lighting. Its whole edited image
is retained; final composition uses only the declared target. Moebius's masked
conditioning is a different interface, and its exact-context run avoids this
input PNG round trip. These do not isolate architectural speed or universal quality.

| Configuration | Load | First edit | Warm edit | Peak CUDA tensor allocation |
| --- | ---: | ---: | ---: | ---: |
| Publisher stochastic scheduler | 209.56 s | 6.66 s | 3.88 s | 46.6 GiB |
| Non-stochastic, global/per-call seed reset | 207.30 s | 5.50 s | 3.96 s | 46.6 GiB |

Both configurations remove the recognizable person in their **full edited
outputs** and produce plausible blurred background. The raw outputs also change
unselected content: mean channel differences are 6.82/7.26 levels in the two
stochastic runs and 8.40 in the non-stochastic run, on the 8-bit scale. Composition
preserves all unselected pixels exactly but exposes a conspicuous pill-shaped
edge, brightness mismatch and broken fence/background alignment. The suggested
non-stochastic setting is somewhat sharper in this inspection; it does not repair
those seams. Neither composition is a finished photographic edit.

The initial two runs with the same supplied generator seed differed. Publisher
code forwards that generator for initial latents, but not to `scheduler.step()`;
its stochastic scheduler uses the global RNG. A small scheduler control confirms
that resetting that RNG produces exact repeated results. The wrapper now resets
CPU/CUDA global seeds as well. The final non-stochastic trial's raw and composed
PNGs are byte-identical across both repeats. Since that trial also changes the
scheduler, it does not isolate which change affects image quality.

The [comparison script](../tools/benchmark_llada_image.py) checks both pinned
source and model before loading, caps its evaluation CUDA allocation at 58 GiB,
records prompt/scheduler/versions/seed/tensor memory and checks immutable input
and target hashes. It never changes user photographs or installs an application
backend. Python syntax, real checkpoint validation, four CUDA samples, source
checkout integrity and diff checks passed; both real inference processes exited
normally. The preceding milestone passed all six desktop/native-learning CI jobs.

Full records are in [llada-turbo-gb10-2026-10-10.json](data/llada-turbo-gb10-2026-10-10.json).
Images, weights and the isolated environment remain under
`/tmp/rawpuppy-validation/llada-image-2026-10-10`. Model choice remains provisional:
this recent alternative is fast once loaded but substantially larger, and exact
photographic boundaries still need work. No universal model ranking is claimed.
