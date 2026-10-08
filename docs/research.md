# Implementation research — 2026-10-08

## Reconstruction and denoising

[Malvar, He, Cutler, 2004](https://www.microsoft.com/en-us/research/publication/high-quality-linear-interpolation-for-demosaicing-of-bayer-patterned-color-images/)
provides small, parallel, color-correcting reconstruction filters. These are a
useful independently implemented baseline with predictable compute cost. They
do not establish the final quality target. AMAZE is a possible classical upgrade;
joint learned reconstruction needs validation on real GFX noise and fine detail.

[Open Image Denoise](https://www.openimagedenoise.org/documentation.html) supports
CPU and GPU inference, in-place HDR RGB denoising, and tiling. Its supplied filters
target rendered images, so they should not be treated as a proven camera RAW
denoiser. The current baseline instead filters same-color sensor samples.

[TENet](https://github.com/guochengqian/TENet) has MIT-licensed research code and
addresses combined demosaicing, denoising, and super-resolution.
[RawNIND](https://arxiv.org/abs/2501.08924) uses real noisy RAW data and proposes
Bayer and linear RGB models. These are candidates for tiled inference; model
weight licensing, noise scaling, camera generalization, and RAW output color
semantics must be checked before adopting weights.

## Tone and color

[AgX's original configuration](https://github.com/sobotka/AgX) documents the
inset and logarithmic domain. The implemented analytic approximation is identified
as such. [Blender's configuration](https://github.com/blender/blender/blob/main/release/datafiles/colormanagement/config.ocio)
uses a different, evolved 3D LUT transform and includes HDR display transforms.
These variants should not be conflated when comparing results.

[Little CMS](https://www.littlecms.com/) handles ICC color management. Display P3
uses the piecewise sRGB transfer function; Adobe RGB uses gamma 563/256; Rec.2020
uses its piecewise BT.2020 transfer function. EXR is scene/display-linear storage,
not PQ-encoded integer HDR. Native HDR presentation requires a suitable swapchain
and operating system integration in addition to floating-point image processing.

## Accelerators and synthesis

[NVIDIA CUDA Rust](https://developer.nvidia.com/blog/introducing-cuda-rust-two-tracks-for-writing-gpu-kernels/)
describes Rust-CUDA and CubeCL routes. [DGX Spark guidance](https://docs.nvidia.com/dgx/dgx-spark-porting-guide/optimization.html)
calls for avoiding redundant host/device allocations and PCIe assumptions on
coherent shared memory. The GPU implementation needs persistent source residency,
composed kernels, bounded working allocations, and timing after warmup. Vulkan
and Metal are needed for other platforms.

[Qwen-Image-Edit-2511](https://huggingface.co/Qwen/Qwen-Image-Edit-2511) is an
Apache-2.0 open image editing candidate. Large diffusion models have substantial
memory and latency costs even on this workstation. Smaller dedicated inpainting
models, such as [LaMa](https://github.com/advimman/lama), are alternatives for
gap filling and object removal. Any adopted model needs its own license record,
deterministic saved results, and inference over bounded regions rather than an
entire 100 MP image. No model is integrated in the current core milestone.
