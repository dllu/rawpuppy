# Inpainting selection — 2026-10-09

The user requested recent models, specifically Moebius and Qwen Image 2.1.
LaMa is retained as an explicitly experimental reference backend, not the
application's quality target or default. Moebius now has native Rust inference,
editor mask painting/corner filling, and persisted non-destructive layers. See
[runtime setup and layer behavior](moebius.md). Selection remains provisional
until broader photography and alternative-model comparisons are complete.

## Candidates and evidence

| Model | Release / licensing | Current evidence |
| --- | --- | --- |
| [Moebius](https://github.com/hustvl/Moebius) | June 2026; authors explicitly license code and pretrained weights Apache-2.0. Hugging Face metadata says MIT; both notices should be retained when integrating. | Native compact pilot with verified scene checkpoint and VAE. A 16-pixel inference halo improves one removal but exact selection composition retains fringes/shadows; actual-pixel fabric repair is better preserved than the tested FLUX output. Broader photography and corner-fill comparisons needed before final selection. |
| [Qwen Image 2.1](https://qwen.ai/blog?id=qwen-image-2.1) | September 20, 2026; [research license](https://huggingface.co/Qwen/Qwen-Image-2.1/blob/main/LICENSE) limits use to research/evaluation without separate commercial permission. | Pinned and checksum-verified all seven checkpoints; actual GB10 comparison on identical image/mask. Promising 512-pixel removal, 13.72 s warm and 32.98 GiB allocation; unmasked changes and a failed 1024-pixel RGBA trial need further diagnosis. See [measurements](inpainting-comparison.md). |
| [Qwen 2.1 + PAI ControlNet Union](https://huggingface.co/alibaba-pai/Qwen-Image-2.1-Fun-Controlnet-Union) | Recent explicit-mask adapter; the same Qwen research license applies to its weights. | Independently checked adapter/base key coverage and shapes, then ran the publisher's pinned pipeline. At 512 pixels: about 18.3 s warm, 38.94 GiB allocation. The withheld fabric repair preserves its soft diagonal texture; tight-mask removal invents a concrete structure. This is a separate conditioning path from the base-model reference-mask experiment. See [adapter comparison](inpainting-comparison.md#qwen-21-explicit-mask-adapter--2026-10-09). |
| [FLUX.2 klein 4B](https://huggingface.co/black-forest-labs/FLUX.2-klein-4B) | January 2026; Apache-2.0 4B model. | Pinned, checksum-verified mask-based Diffusers comparison: about 1.8 s warm at 512 pixels, 15.56 GiB allocation. A 16-pixel inference halo improves raw removal; exact composition through the original tight mask retains fringe/shadows. A withheld actual-pixel fabric repair invents a mismatched stitch pattern; native Moebius better preserves this example's soft diagonal structure. [Local measurements](inpainting-comparison.md). |
| [LLaDA-Image / Turbo](https://github.com/inclusionAI/LLaDA-Image) | September 4, 2026; [model card](https://huggingface.co/inclusionAI/LLaDA-Image) identifies Apache-2.0. | Recent 6B image generation/editing family; Turbo uses four sampling steps. Publisher evidence only; local inpainting, detail and runtime comparisons pending. |
| [OSOR](https://github.com/Zhouqm-Git/osor) | June 2026; MIT code, but FLUX-Fill checkpoints are non-commercial and SDXL checkpoints use CreativeML Open RAIL++-M. | One-step object removal with learned shadow/reflection masks. Not benchmarked. Its mask expansion would need explicit handling alongside Rawpuppy's exact unpainted-pixel preservation. |
| [LaMa ONNX](https://huggingface.co/Carve/LaMa-ONNX) | 2022 architecture; published Apache-2.0 export. | Native Rust inference, finite output, missing-pixel reconstruction and exact unmasked-sample preservation verified. Public object-removal example shows visible artifacts; insufficient to establish final quality. |

Publisher benchmark claims are not Rawpuppy benchmarks. The Moebius paper compares
the specialist to larger general models, but it does not prove this is the best
model for the user's photographs or compare directly with September's Qwen 2.1.
Our own comparisons include one public removal example and one withheld texture
patch at actual GFX100S pixels. They do not establish a general winner.

## Moebius measurements

Used the authors' inference architecture and scene checkpoint on the NVIDIA GB10,
with batch size one, FP32, DDIM, CFG 2.0, fixed seed, and noise offset 0.0357.
A public image and mask from the LaMa publisher were fitted into a 512×512
square without stretching; replicated edge context occupies padding outside the
mask. Measurements include preprocessing, VAE and sampling, and output transfer.

- Model/VAE load: around 1.5–1.9 seconds.
- 20 requested steps: 3.57 seconds first run; 2.91 and 2.93 seconds warm.
- 10 requested steps: 2.03 seconds first run; 1.61 seconds warm.
- Peak PyTorch GPU allocation: 2,309,303,808 bytes (~2.15 GiB), excluding allocator
  reservations and other processes. This is not a system-wide memory measurement.
- Outputs visually remove the subject, but also invent detail such as graffiti.
  Quality claims remain provisional. The actual output was inspected.

The released local attention derives both spatial dimensions from sqrt(token
count) and failed on a 512×640 test input. Bounded square crops/padding worked.
The production adapter must maintain the photo's aspect ratio, preserve every
unmasked original sample, and blend only inside the chosen mask. The upstream
paste path applies a Gaussian mask blur that can affect outside pixels; Rawpuppy
must perform its own final composition. Large photos need selected regions and
context, not a resized or repeatedly regenerated entire image.

The inference-only evaluation omitted an unused teacher-model import, avoiding an
unnecessary Flash Linear Attention dependency. Python/PyTorch is used for this
external evaluation; application inference and seeded DDIM sampling run in Rust
through prepared, numerically checked TorchScript graphs and LibTorch 2.13.
Native CUDA sampling took approximately 2.1 seconds for the 10-step test. This
measurement uses a different input from the upstream comparison above and must
not be interpreted as a controlled runtime comparison. Editor generation,
save/reload, regeneration after exposure changes, and four-corner filling on a
GFX100S photo have been exercised. Prepared contexts contain 512×512 model detail;
large gaps and boundary/detail quality still need further evaluation. The same
prepared graphs ran natively on CPU, taking 27.3 seconds for the 10-step synthetic
test with eight OpenMP workers. Subsequent native-learning CI verified actual
Windows CPU and macOS MPS sampling with the same preparation and integrity
checks. Run `37980551392` passed all six desktop/native-learning jobs; its macOS
case required MPS and disabled CPU operator fallback. See
[runtime verification](moebius.md) for details. This establishes execution and
sample preservation, not general photographic quality.

An initial 18-degree GFX100S rotation exposed weak outpainting: corner-centered
context put most pixels outside the photograph and produced dark wedges despite
valid opaque output. Moving square context into the corrected canvas increased
known content and improved the inspected 20-step result, but one corner still
contained a dark wedge. A subsequent 50-step, seed-42 run filled all four visible
wedges in 36.4 seconds total. Steps and seed changed together, so this is not a
controlled attribution of the improvement. Opaque output is an integrity check,
not a quality metric. This example is not evidence of production-quality
large-gap extrapolation or full-resolution detail.

The same-input Qwen/Moebius comparison and subsequent FLUX and actual-pixel
texture extension are recorded in
[inpainting-comparison.md](inpainting-comparison.md). Extend them with identical original context and masks
for small distraction removal, associated shadows/reflections, textured surfaces,
fine structures and geometric gaps. Record visual artifacts at actual pixels,
boundary consistency, sampling settings, elapsed time and peak memory. Native
runtime integration alone does not resolve that comparison.

## Reproducibility and model identity

- Moebius source: `b88d462bacb9af6e7128a3b4cc4a07418bedfd61`.
- Scene checkpoint revision: `cd01f47fb648219d3fa605806c5ce00b713faa5e`.
- Scene checkpoint SHA-256: `6525afb888e55f9b5c74fa0a5d19ca0762d720d6c716fb0f8422fbeb6868a09a`.
- VAE revision: `012fd343158936a265b8a0ee38a791a7a2841f45`.
- VAE SHA-256: `a59d7ea697f2942d22002dc3469e8c53db807a6b78f7f5ec03bd4c1f70f98efe`.
- Qwen 2.1 inspected revision: `d26bb61231c349cf6b7896fa83353113880e1ba3`.

Models are cached outside photograph directories. `rawpuppy fetch-moebius`
downloads pinned artifacts and verifies them; `--features moebius` enables modern
native generation after graph preparation. `--features neural` enables the LaMa reference
command `inpaint-lama`. The original photos under `~/pictures/raw` were not modified.
