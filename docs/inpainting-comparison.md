# Controlled image/mask comparison on GB10

This is a research evaluation of current models, not a final selection or a
production quality claim. Both models receive the same 512×512 original and mask.
Moebius is a dedicated masked inpainting model. Qwen Image 2.1 receives the original
and white-on-black mask as two reference images, plus an explicit removal prompt.
Its current official Diffusers pipeline has no separate `mask_image` parameter.

## Settings and measurements

The NVIDIA GB10 test uses batch size one. Moebius uses the scene checkpoint,
FP32, DDIM, 20 requested steps, CFG 2.0 and seed 0. Qwen uses BF16, 40 steps,
seed 42, `true_cfg_scale=1`, and prefix KV caching. These are different sampling
and precision configurations; the measurements do not isolate an architectural
speed difference. No model is compiled, quantized, or CPU-offloaded per step.

| Model / output grid | Measured inference | Peak PyTorch GPU allocation |
| --- | --- | --- |
| Moebius / 512×512 | 3.57 s first; 2.91 and 2.93 s warm | 2.15 GiB |
| Qwen Image 2.1 / 512×512 | 17.39 s first; 13.72 s warm | 32.98 GiB |
| Qwen Image 2.1 / 1024×1024 | 67.86 s first; no warm measurement | 40.91 GiB |

Qwen's initial baseline CPU load followed by `.to("cuda")` took 200.56 seconds,
excluding imports and checksum verification. This is a loader measurement,
not sampling time or an optimized startup claim. Allocator reservation peaked at
33.76 GiB for the 512-pixel test. GPU allocation, allocator reservation, and process
RSS account for different things and must not be added into a GB10 system total.
Other projects shared the machine; these are observations, not latency guarantees.
Direct CUDA placement via `device_map="cuda"` took 199.38 seconds and its 512-pixel
inference took 14.46 seconds. It did not demonstrate a startup improvement; this
single run is not an equivalence bound on the loaders. Its RGB PNG was identical
to both baseline runs. The native alpha minimum was 253, with no zero-alpha pixels.
The native 512 and 1024 results were inspected alongside their RGB compositions.

Exact measurements, checkpoint identities and output hashes are retained in
[the data record](data/inpainting-gb10-2026-10-08.json).

## Inspected results

At 512 pixels, Qwen removed the seated person and continued the circular opening,
distant vegetation and concrete wall without Moebius's invented graffiti-covered
structure. This is a useful improvement on this example, not proof that Qwen wins
on textures, fine structures, portraits, or geometric gaps generally.

The 1024-pixel trial failed visually with magenta/striped regions and unwanted
transparency. Its native RGBA output contained 2,432 fully transparent target
pixels out of 70,492 selected pixels on the 1024 grid. Dropping alpha to compare
RGB is diagnostic only and does not repair this result. The cause remains open;
this is a failure of the tested reference/prompt/seed/inference configuration,
not evidence that all Qwen editing or high-resolution output is defective.
Both tested sizes are below the publisher's recommended native 2K sizes.
The original portrait example was fitted into a square with replicated edge
padding for Moebius; this may also affect a general editor's interpretation.
Unpadded photographic contexts, explicit opacity instructions, and alternative
seeds are still needed to diagnose the failed trial. No inference-code defect or
model-wide resolution limit has been established.

Qwen also removed or changed shadows outside the selection despite the preservation
instruction. Across channels outside the mask, its raw output changed by 7.18 levels
on average and 52 levels at the 95th percentile on the 8-bit scale. These quantify
preservation, not perceptual quality. Compositing only through the original mask
reduced outside changes to exactly zero, but revealed seams and retained the
original shadows. The mask covers the person, not all related shadows. Both masks
and alignment need attention before claiming clean non-destructive removal.

The two same-seed 512-pixel Qwen runs produced byte-identical raw and composed PNGs
in this setup. This does not promise cross-device or cross-version reproducibility.
Generated images remain under `/tmp/rawpuppy-validation` and are not bundled with
Rawpuppy. The model's research terms also make this an evaluation reference rather
than a generally distributable application backend.

## Reproduction

The checked-in [benchmark script](../tools/benchmark_qwen_image_21.py) records the
checkpoint hashes, pinned download revision, installed pipeline Git revision,
dependency versions, exact prompt, seed, timing and memory. It verifies safetensors
checksums against Hugging Face download metadata when present, retains raw/native
and mask-composed output, and refuses to replace a completed measurement directory.

Use an isolated environment with PyTorch 2.14.0+cu130, Transformers 5.17.0,
Accelerate 1.15.0 and Diffusers commit
`d961a388fd02e4db38d17350c8dd9b8abe642e05`. The evaluation reused the installed
PyTorch packages read only through an isolated environment; no other project's
packages were changed. The model is revision
`d26bb61231c349cf6b7896fa83353113880e1ba3` of
[`Qwen/Qwen-Image-2.1`](https://huggingface.co/Qwen/Qwen-Image-2.1).
All seven checkpoint SHA-256 values matched its pinned download metadata.

The prompt was:

> Image 1 is the original photograph. Image 2 is a mask: white pixels identify the area to edit, and black pixels identify the area to preserve. Remove the seated man from the white masked area of image 1, and reconstruct the background behind him. Continue the circular opening, distant trees and buildings, and the straight concrete wall naturally through the removed area. Keep the framing, geometry, colors, lighting and all unmasked content of image 1 unchanged. Return the edited photograph, without a mask or annotations.

Run `python tools/benchmark_qwen_image_21.py --help` for arguments. The original
input hash is `25b87deec1bd2a25774395e9b4adb43308b059332ccb08a1028f96e8d8712880`;
the mask hash is `6d104d8029084009ff6629013c0f272d49d54cf765a8ac6d446c795e33777f2c`.
The source example is from the [LaMa ONNX publisher](https://huggingface.co/Carve/LaMa-ONNX),
fitted and edge-padded to a square without stretching.

Moebius remains a compact native pilot, with LaMa as a historical reference.
Recent permissive candidates such as LLaDA-Image/Turbo and FLUX.2 klein still need
the same local comparison. Selection must include textured surfaces, fine detail,
associated shadows/reflections and geometric gaps, at actual photographic pixels.
