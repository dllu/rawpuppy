# A project-owned LibTorch runtime

Earlier local all-feature tests used PyTorch 2.13.0+cu130 from Rawpuppy's cache,
but borrowed missing NVIDIA libraries from an unrelated ComfyUI environment.
Removed that dependency from the local testing setup by installing the exact
CUDA library packages required by this Torch wheel into the same Rawpuppy cache:

| Package | Version |
| --- | --- |
| nvidia-cudnn-cu13 | 9.20.0.48 |
| nvidia-cusparselt-cu13 | 0.8.1 |
| nvidia-nccl-cu13 | 2.29.7 |
| nvidia-nvshmem-cu13 | 3.4.5 |

The runtime root is
`~/.cache/rawpuppy/evaluation/torch-2.13`. Its wheel metadata supplies these pins;
the machine's CUDA 13 toolkit supplies the remaining system CUDA libraries.
No packages or configuration in the unrelated project were changed.

Added `tools/libtorch-env.sh`, sourced from the repository root. It configures
the matching LibTorch build variables and replaces the loader path with this
installation's Torch and NVIDIA package libraries, excluding inherited paths
from other environments. `RAWPUPPY_TORCH_ROOT` selects a different complete
installation. A missing Torch installation returns an actionable error.
The Moebius setup document now shows a complete local Torch installation and
this shared build/launch setup.

`ldd` resolves every dependency of both the all-feature executable and
`libtorch_cuda.so` using the owned installation and system libraries, with no
unresolved libraries or ComfyUI paths. The executable starts with this
environment. Shell syntax and manual configuration/error checks passed,
including replacement of an inherited unrelated loader path. The all-feature
release build and strict all-targets Clippy also passed using this environment.

The full all-feature test suite passed. The real prepared Moebius graphs ran on
`Cuda(0)` and explicit CPU, checking finite generated samples and exact
preservation outside the mask. RawNIND's selected device was also `Cuda(0)`; its
camera-linear output agreed with explicit CPU to a maximum difference of
`0.000002026558`. All of these processes used only the owned runtime loader path.
