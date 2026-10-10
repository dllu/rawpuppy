# Automatic native GPU fallback

Reviewing the CUDA-enabled build found that Auto returned CUDA initialization
failure directly to the renderer, which selected CPU without trying Vulkan/Metal.
That stranded a usable native GPU on a machine without CUDA. Auto now tries CUDA,
then the native wgpu API, then leaves CPU fallback to the renderer when neither
initializes. Recoverable initialization panics are contained per attempt, and
the final error includes both failure causes. Explicit backend selection remains
explicit; successful CUDA retains priority.

A real native-device regression injects both a missing-device error and an
initialization panic, then renders a synthetic image through the native API and
compares its values/alpha with CPU. Its commands are added to existing Linux and
macOS GPU CI steps. A policy regression checks CUDA priority and combined errors.
A CUDA-device test also confirms the real Auto constructor selects working CUDA
and renders unchanged source values.

An isolated child process additionally tests the public CUDA-enabled Auto path
with an owned libcuda.so stub whose cuInit returns CUDA_ERROR_NO_DEVICE. The
public multi-renderer test succeeds on Vulkan. An initial .so.1 alias also
intercepted NVIDIA Vulkan's driver dependency, causing both APIs to fail as
reported; removing only that owned alias lets native Vulkan load its real driver.
The stub and environment apply only to the test child, without changing system
libraries, GPU configuration or other processes. Files remain under
`/tmp/rawpuppy-validation/auto-gpu-fallback-2026-10-10`.

All 61 standard tests, native error/panic rendering, public no-device Auto
selection, real working-CUDA priority, strict Rust 1.99 Clippy with CUDA,
formatting and diff checks passed. The preceding detail-comparison commit's
six CI jobs also completed successfully. The whole PROMPT.md objective remains
active; this closes one concrete cross-platform selection defect.
