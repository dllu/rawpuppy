# Rust GPU composition on CUDA and Vulkan

Added a single composed CubeCL Rust kernel, runtime backend selection, persistent
source residency, cached sensor cleanup, 64 MiB output tiles, and automatic CPU
fallback for GPU binding/addressing limits and unsupported CFA types. Both the
CLI and native editor now use the shared renderer. CUDA is optional; default
GPU builds use wgpu's Vulkan/Metal runtime. CPU-only photo compute still builds.

Verified CUDA and Vulkan against CPU for RGB/Bayer, every EXIF orientation,
sensor hot-pixel/denoise controls, perspective/rotation/crop, distortion/CA,
exposure/calibration/gradient/vignette, AgX, curve/split toning, clone/heal, and
100,003-pixel width. Normal core tests and strict Clippy pass. Tests requiring a
GPU are explicit, rather than claiming that an ignored test proves acceleration:

```sh
cargo test --test gpu -- --ignored
RUST_MIN_STACK=33554432 RAWPUPPY_TEST_CUDA=1 cargo test --features cuda --test gpu -- --ignored
cargo run --release --features cuda --example benchmark -- photo.raf --backend cuda
```

The comparison exposed a signed-remainder border issue on Vulkan. Reflection
now reduces positive absolute coordinates, avoiding that backend difference.
Fully inlining sensor denoise into every reconstructed sample produced a huge
kernel and excessive compile time. Replaced it with a cached spatial preparation
pass. CUDA's compiler worker overflowed its default stack on the larger kernel;
the executable configures a 32 MiB stack before creating any threads, and the
library returns a descriptive error if a CUDA caller omits that configuration.

On the real 11808×8754 GFX100S sensor, repeated 1350×1800 previews with exposure
changes averaged 13.20 ms on CUDA and 10.63 ms on Vulkan after initialization.
First render costs were about 0.89 s / 1.96 s, respectively, in those runs.
CPU warm previews were around 0.13 s. These are end-to-end render timings
including host output, with eight CPU workers, not isolated kernel benchmarks.
The machine also runs other projects; measurements are observational.

Full 8736×11648 16-bit TIFF export: CUDA 4.40 s wall, 2.08 s rendering including
setup, 6,297,828 KiB peak RSS; Vulkan 6.63 s wall, 4.10 s rendering including
setup, 3,787,500 KiB peak RSS. The runs overlapped briefly and should not be
treated as controlled comparative throughput results. Preview comparisons
against CPU had mean absolute 8-bit code differences of 0.000012 (CUDA) and
0.000224 (Vulkan), maximum one code value, with no larger outliers. All outputs
were written to `/tmp/rawpuppy-validation`; original samples remain untouched.

Verified the native editor with CUDA on X11 using the temporary RAW copy, including
editing and saving after a GPU preview. The earlier editor CI checkpoint now
passes all tests on Linux, macOS and Windows. The new GPU checkpoint still needs
its own CI run. Metal execution, further reduction of unified-memory copies and
allocator overhead, neural synthesis and stronger color/quality validation remain.
