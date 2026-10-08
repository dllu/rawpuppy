# Inpainting selection — 2026-10-08

The user requested recent models, specifically Moebius and Qwen Image 2.1.
LaMa is retained as an explicitly experimental reference backend, not the
application's quality target or default. No generative backend is yet wired into
the editor or persisted as a complete non-destructive synthesis module.

## Candidates and evidence

| Model | Release / licensing | Current evidence |
| --- | --- | --- |
| [Moebius](https://github.com/hustvl/Moebius) | June 2026; authors explicitly license code and pretrained weights Apache-2.0. Hugging Face metadata says MIT; both notices should be retained when integrating. | Downloaded scene checkpoint and VAE, verified hashes, ran actual GPU inference. Leading integration candidate; more photography and corner-fill comparisons needed. |
| [Qwen Image 2.1](https://qwen.ai/blog?id=qwen-image-2.1) | September 20, 2026; [research license](https://huggingface.co/Qwen/Qwen-Image-2.1/blob/main/LICENSE) limits use to research/evaluation without separate commercial permission. | Verified architecture and file inventory: about 33.1 GB of model artifacts. No local quality benchmark yet. Suitable comparison reference; do not assume earlier Qwen Apache terms apply. |
| [FLUX.2 klein 4B](https://huggingface.co/black-forest-labs/FLUX.2-klein-4B) | January 2026; Apache-2.0 4B model. | Additional modern editing candidate; not benchmarked yet. |
| [LaMa ONNX](https://huggingface.co/Carve/LaMa-ONNX) | 2022 architecture; published Apache-2.0 export. | Native Rust inference, finite output, missing-pixel reconstruction and exact unmasked-sample preservation verified. Public object-removal example shows visible artifacts; insufficient to establish final quality. |

Publisher benchmark claims are not Rawpuppy benchmarks. The Moebius paper compares
the specialist to larger general models, but it does not prove this is the best
model for the user's photographs or compare directly with September's Qwen 2.1.

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
external evaluation; the application implementation remains Rust. Native modern
model inference and a responsive GUI synthesis workflow are still to be built.

## Reproducibility and model identity

- Moebius source: `b88d462bacb9af6e7128a3b4cc4a07418bedfd61`.
- Scene checkpoint revision: `cd01f47fb648219d3fa605806c5ce00b713faa5e`.
- Scene checkpoint SHA-256: `6525afb888e55f9b5c74fa0a5d19ca0762d720d6c716fb0f8422fbeb6868a09a`.
- VAE revision: `012fd343158936a265b8a0ee38a791a7a2841f45`.
- VAE SHA-256: `a59d7ea697f2942d22002dc3469e8c53db807a6b78f7f5ec03bd4c1f70f98efe`.
- Qwen 2.1 inspected revision: `d26bb61231c349cf6b7896fa83353113880e1ba3`.

Models are cached outside photograph directories. `rawpuppy fetch-moebius`
downloads pinned artifacts and verifies them; it does not claim that modern
inference is integrated. `--features neural` enables the native LaMa reference
command `inpaint-lama`. The original photos under `~/pictures/raw` were not modified.
