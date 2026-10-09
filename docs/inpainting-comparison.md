# Controlled image/mask comparison on GB10

These are research evaluations of current models, not a final selection or a
production quality claim. The initial Moebius/Qwen comparison uses the same
512×512 original and mask. The FLUX extension below uses those same inputs too.
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
LLaDA-Image/Turbo still needs local comparison. Selection must include textured
surfaces, fine detail, associated shadows/reflections and geometric gaps, at actual
photographic pixels.

## FLUX.2 klein extension — 2026-10-09

The Apache-2.0 [FLUX.2 klein 4B checkpoint](https://huggingface.co/black-forest-labs/FLUX.2-klein-4B)
is evaluated with the official `Flux2KleinInpaintPipeline`, using its native
`mask_image` input. Its original-image latent reference conditioning and per-step
mask reinsertion differ from both the Moebius specialist and Qwen's two-reference
editing interface. The 4B distilled model uses BF16, four executed steps,
strength 1.0, guidance 1.0 and seed 42. These configurations do not isolate an
architectural speed difference. No compilation, quantization, or per-step CPU
offloading is used.

On the same 512-pixel removal input, inference took 3.17 seconds first and
1.83 seconds warm, with 15.56 GiB peak GPU allocation and 15.82 GiB allocator
reservation. Loading on CPU followed by `.to("cuda")` took 95.30 seconds,
excluding imports and checkpoint verification. Two same-seed outputs were
byte-identical in this setup. Outside the mask, the raw output changed by
1.81 levels on average and 5 levels at the 95th percentile on the 8-bit scale;
mask composition preserved those original pixels exactly.

The 1024-pixel run explicitly upsamples that same 512-pixel context and mask;
it does not add original photographic detail. Inference took 7.32 seconds first
and 6.56 seconds warm, with 17.35 GiB peak allocation and 18.52 GiB reservation.
It returned opaque RGB without the transparency failure of the earlier Qwen
trial, but the inspected composition still has a head-shaped remnant, seams and
retained shadows. Qwen received 512-pixel references with a requested 1024-pixel
output, whereas this FLUX pipeline requires the reference itself to be enlarged;
these high-resolution paths are not identical experiments. Two same-seed FLUX
outputs were again byte-identical. Qwen's published native sizes are larger than
either comparison, so these results do not establish its native-resolution quality.

The inspected result has conspicuous remnants around the person's head and legs,
a mismatched wall patch, and retained shadows. Tight selection boundaries can
preserve remnants in the original, and a latent mask does not guarantee a clean
pixel boundary. This result does not justify selecting FLUX on speed alone.
Moebius and Qwen's compositions also retain seams or original shadow remnants;
none of the tested tight-mask removals is a finished photographic edit.

Replacing the selected source pixels with black before the 512-pixel FLUX run,
with the prompt and sampler otherwise unchanged, did not remove the artifact.
Its inspected result has stronger dark outlines and mismatched patches. Inference
took 2.57 seconds first and 1.80 seconds warm with the same peak allocation.
This trial changes reference preprocessing; it is not evidence that withholding
is always harmful, nor does it diagnose the artifact's cause.

A further run dilates only the inference mask by 16 model pixels, keeping the
original image, prompt, sampler and final composition mask unchanged. This
substantially improves the inspected **raw** removal: the head-shaped remnant
disappears, and the wall and background continue plausibly through the removed
person. Inference took 2.58 seconds first and 1.80 seconds warm with the same
peak allocation. Two same-seed outputs were byte-identical.

Composition through the original tight selection still leaves original fringe
pixels, some tone mismatch and untouched associated shadows. These are reasons to
choose a sufficient user selection and distinguish the inference mask from the
final edit mask. A model may generate a larger region for context while the editor
commits only selected pixels. The raw result also changes some surrounding content
(2.70 levels mean and 9 levels p95 outside the original selection), so accepting
it wholesale would violate exact preservation. This experiment establishes that
mask preprocessing matters; it does not prove a particular cause of the original
artifact or an overall quality ranking. The Moebius follow-up below tests the same
16-pixel growth. Qwen has not been retested with it, and FLUX's texture test below
uses zero inference padding.

### Actual-pixel GFX100S texture repair

A separate 512×512 crop contains blue knitted fabric at original image pixels,
without resizing the 101.8 MP photograph to create the context. A central 96×96
square is replaced with black **before** either model receives it. Both receive
that same withheld input and binary mask; the intact crop is retained only for
assessment. Thus the original target detail cannot leak through an image reference
or a partial-strength initialization.

Native Moebius uses the scene checkpoint, FP32, 20 steps, strength 1.0, CFG 2.0,
noise offset 0.0357 and seed 42. Inference took 3.45 seconds first and 2.98 seconds
warm. FLUX uses the four-step configuration above and took 2.60 seconds first and
1.80 seconds warm, again allocating 15.56 GiB. Native Moebius memory was not
measured in this run. Two same-seed runs of each model were byte-identical, and
both preserved every unmasked RGB byte after composition.

Moebius continued the soft diagonal structure and overall color, with a faint
square boundary and some changed local detail. FLUX invented a much sharper,
differently oriented stitch pattern with a conspicuous square seam. These are
inspected results for this context and prompt, not a claim about every possible
prompt or model configuration.

| Model | Selected-region mean absolute error / 8-bit channel | Selected-region RMS error / 8-bit channel |
| --- | ---: | ---: |
| Native Moebius | 4.63 | 5.56 |
| FLUX.2 klein 4B | 31.16 | 43.01 |

The errors compare the 9,216 withheld pixels with their known intact values.
They describe reconstruction fidelity for this example, not general perceptual
quality: a plausible generated texture can differ from the original. Qwen has not
been tested on this texture context. No overall model winner has been established.
The private photographic crop and generated images remain under
`/tmp/rawpuppy-validation`; they are not bundled with Rawpuppy.

### Reproduction and constraints

[benchmark_flux2_klein.py](../tools/benchmark_flux2_klein.py) verifies all four
Diffusers-layout safetensors files against download checksums at checkpoint
revision `e7b7dc27f91deacad38e78976d1f2b499d76a294`. It records the pipeline revision,
input hashes, prompt, executed steps, timings, allocations and output hashes,
retains native and mask-composed results, and refuses an existing output directory.
`--mask-padding 16` grows the inference mask without growing the final edit mask;
the default is zero, matching the first removal and texture comparisons.
It uses the same isolated environment and Diffusers commit as the Qwen comparison.
Run the script with `--help` for arguments. The snapshot download should omit
the redundant root-level `flux-2-klein-4b.safetensors` single-file export.

[benchmark_moebius.rs](../examples/benchmark_moebius.rs) exercises the production
native sampler directly on 512-pixel sRGB PNG/mask pairs. Build it with matching
LibTorch 2.13 and `cargo build --release --features moebius,cuda --example benchmark_moebius`.
It checks graph identities through `Moebius::open`, records the graph manifest and
input/output hashes, and rejects any changed unmasked pixel. `--models` selects
the prepared graph directory; `--image`, `--mask`, and `--output` select the local
fixture and a fresh result directory. Timings include native preprocessing,
sampling, VAE decoding and output transfer, but exclude PNG encoding.

The current upstream FLUX pipeline adopts input dimensions, rounds them to
multiples of 16 and caps their area at one megapixel. The benchmark explicitly
resizes the input when a different output grid is requested, preserves aspect
ratio, and checks the actual returned dimensions. This library limit is another
reason to infer bounded contexts; it is not a new Rawpuppy photo-size limit.
Peak GPU allocation, reservation, and process RSS must not be added into a GB10
system total. Other projects shared the workstation, and a local compiler was
also active during part of the texture run; these observations are not latency
guarantees. Full settings and provenance are in
[the extension data record](data/inpainting-gb10-2026-10-09.json).

## Native Moebius inference-mask follow-up — 2026-10-09

The native benchmark now accepts `--mask-padding`, with zero as its unchanged
default. A positive value grows the model's binary selection by that many pixels
in each axis while final composition uses the original fractional selection in
display-linear sRGB. It retains the expanded mask and the inferred composition
separately. That inferred image already preserves pixels outside the model mask;
it is not the raw VAE output. The editor's default generation behavior is unchanged.

Both the public removal and withheld GFX fabric inputs were run with zero and
16-pixel growth, using FP32, 20 steps, strength 1.0, guidance 2.0 and seed 42.
This removal baseline differs from the earlier seed-0/strength-0.99 Moebius run;
only this pair isolates the mask change. Two runs of each case produced identical
PNG bytes. The unpadded fabric output also matches the earlier native benchmark's
hash. Warm inference was 2.96–2.97 seconds for all four cases; first calls were
3.37–3.38 seconds. These timings exclude mask construction, model loading and PNG
encoding, and do not establish a hardware-independent latency bound.

In the inspected expanded Moebius removal, the wall and distant vegetation continue
through the person, and the conspicuous dark triangular patch of the unpadded run
is reduced. Composition through the original selection still has seams, fringe
pixels and retained shadows. The FLUX halo result has similar selection limitations
and a visibly brighter wall patch. These are inspected differences on one input,
not an overall model ranking or proof of an artifact's cause.

The public selection includes 1,937 fractional edge pixels. Moebius treats any
positive selection as masked for conditioning; its expanded inference mask is
binary. FLUX's benchmark grows the grayscale mask before pipeline preprocessing.
Final fractional composition also uses different color representations between
the two adapters. Thus equal 16-pixel growth is a useful practical comparison,
not proof that both models receive identical internal mask tensors.

| Native Moebius fabric run | Selected-region mean absolute error / 8-bit channel | Selected-region RMS error / 8-bit channel |
| --- | ---: | ---: |
| No inference growth | 4.634 | 5.556 |
| 16-pixel inference growth | 4.645 | 5.557 |

The fabric results retain the soft diagonal pattern and a faint square boundary;
growth makes little difference on this particular 96×96 withheld patch. This does
not measure general perceptual quality, and FLUX has not had the fabric halo trial.
All four final compositions preserve every unselected RGB byte. An independent
Pillow binary MaxFilter check matches both expanded masks exactly, including the
public selection's fractional support. Full settings, hashes, measurements and
validation are in [the follow-up record](data/moebius-halo-gb10-2026-10-09.json).

Reproduce with `benchmark_moebius --mask-padding 0` and `--mask-padding 16`, keeping
the input, original mask and sampling options fixed and choosing fresh output
directories. The private input and output images remain under
`/tmp/rawpuppy-validation` and are not bundled with Rawpuppy.
