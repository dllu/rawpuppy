# CUDA system-memory path on GB10

On an eligible integrated CUDA device, Rawpuppy binds the existing Rust allocations
directly to its GPU kernels. The normalized sensor, parameters, AgX lattice,
retouch tables and output stay in system DRAM. There is no second device copy of
the sensor or GPU-to-CPU preview readback. The processing kernel is still the same
CubeCL Rust definition used by copied CUDA, Vulkan and Metal.

Selection requires a 64-bit process and positive CUDA attributes for integrated
memory, pageable access, host page tables and concurrent managed access. Other
devices retain CubeCL's copied allocation path. Runtime capability checks select
the path, rather than a camera model, GPU name, or operating-system assumption.
The `Renderer::with_cuda_memory` and `GpuRenderer::with_cuda_memory` interfaces
can explicitly select Auto, Copy or System. The benchmark exposes the same choice
through `--cuda-memory`; an unsupported explicit System request fails.

## Ownership and synchronization

CubeCL's public kernel and compiler APIs produce CUDA code from the shared Rust
kernel. Cudarc supplies compilation/loading, a dedicated stream and driver launch.
Its argument pointers refer to naturally aligned, typed Rust allocations and use
CubeCL's own metadata builder. No private runtime allocation map or deallocator
is modified. Programs are cached by their preparation/render specialization;
ordinary edit changes update data rather than recompile a program.

Immutable `Arc<SensorImage>` ownership keeps the current source alive and prevents
mutable access through a unique Arc while kernels use it. Each launch synchronizes
before borrowed parameters or output can be read, modified or dropped. An RAII
guard also synchronizes during error propagation and unwinding. Output allocations
remain `MaybeUninit` until every tile has written all components successfully.
Retired sources are released after replacement. Sensor cleanup uses a separate
cached system allocation when requested, leaving original samples unchanged.

Launches retain checked addressing and bounded tile indexing. The system path
does not inherit a storage-buffer binding-size limit from a copied allocation,
but its current kernels still use 32-bit GPU indexing. Unsupported addressing
selects CPU fallback in Auto mode; the core representation remains `usize`.

## Pages and first touch

Using ordinary pageable allocations without preparation was correct but slow on
this machine: measured warm previews took about 400–500 ms. GPU page access was
the measured hotspot, rather than additional photographic processing.

On Linux, hints apply only to complete huge-page regions wholly inside owned
allocations. The hint size is read from the kernel's transparent-huge-page size;
unsupported advice is ignored. The application changes neither global settings
nor other processes. Source normalization reserves its destination and advises it
before CPU first touch. Output/sensor-cleanup allocations are advised and touched
on CPU before GPU writes. Existing source/lattice regions may be collapsed once.

This avoids the expensive page handling seen in the first prototype. Small arrays
and allocation fringes continue using ordinary pages. The hints are an optimization,
not a file-size cap or a requirement to pin all memory. Memory pressure and kernel
policy may still affect performance; tests do not establish a universal bound.

## Measurements and verification

Measurements use the same GFX100S source, camera correction, 1,800-pixel output,
eight CPU workers, eight preview iterations and changing exposure. Copied and final
system runs were sequential; the initial exploratory runs overlapped and are not
used to compare final latency. GPU setup/JIT is included in the first render.

| Path | RAW decode | First render | Mean of seven warm renders |
| --- | --- | --- | --- |
| Copied CUDA | 400.04 ms | 942.86 ms | 12.22 ms |
| Coherent system CUDA | 405.53 ms | 400.90 ms | 4.62 ms |

The sampled checksums agree at the benchmark's six decimal places. The same
corrected 101.8 MP image also exported through the system path: 0.48 s render and
0.92 s TIFF export in the observed run. Compared with the previous copied-path
TIFF, 580 of 407,027,712 channel samples differ by one 16-bit level; all other
samples agree. Alpha is 65535 throughout. File bytes differ and bit-identical
rendering is not claimed.

Nsight Systems confirmed the baseline sensor upload was about 413.47 MB, with
about 38.88 MB of readback per preview. The final system run contains no GPU memory
copy records. Its three traced render kernels averaged 2.80 ms. These observations
verify removal of application data transfers; they are not a system-wide RAM
measurement or a claim about driver-internal allocations.

Verification covers CPU/system/copied-CUDA parity for RGB/Bayer, all orientations,
camera/manual/local edits and wide images; source immutability; cleanup changes
and retired-source release; default tests and WGPU parity; and strict Clippy for
CUDA/default/no-GPU configurations. The native editor rendered a real GFX photo
with system memory and closed normally. CI additionally compiles the CUDA adapter
on Linux without requiring a GPU. CUDA execution remains a local hardware check.

Reproduce with `cargo run --release --features cuda --example benchmark --
photo.raf --backend cuda --cuda-memory system --iterations 8`, then use Copy for
the comparison. Records are retained in [the data artifact](data/gb10-system-memory-2026-10-09.json).
All validation photographs, exports and profiler databases remain under
`/tmp/rawpuppy-validation`.

Sources: [NVIDIA's unified/system memory guide](https://docs.nvidia.com/cuda/cuda-programming-guide/02-basics/understanding-memory.html),
[unified-memory tuning](https://docs.nvidia.com/cuda/cuda-programming-guide/04-special-topics/unified-memory.html),
and [DGX Spark optimization guidance](https://docs.nvidia.com/dgx/dgx-spark-porting-guide/optimization.html).
