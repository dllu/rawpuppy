# Coherent system allocations for CUDA photo processing

Added a capability-gated CUDA driver adapter that compiles the existing shared
CubeCL Rust kernels and binds owned Rust memory directly. The source, output,
small arguments and AgX data no longer require duplicate device allocations or
application copy calls on the GB10. Copied CUDA remains selectable, and Vulkan,
Metal and unsupported CUDA hardware keep their existing memory paths.

The first correct prototype was slow because of page access. Nsight confirmed
that the render kernel itself took hundreds of milliseconds. Local huge-page
advice and CPU first touch reduced warm GFX previews to about 4.6 ms. Advising the
sensor allocation before normalization also reduced first-render setup to 0.4 s.
A sequential copied-path run measured 12.2 ms warm and 0.94 s first render. These
are shared-workstation measurements rather than timing guarantees.

Nsight traces show no application GPU memory copies in the final system run,
versus a 413.47 MB sensor upload and 38.88 MB per-preview readback before it.
The full corrected 101.8 MP TIFF rendered in 0.48 s and exported in 0.92 s. Pixel
comparison against the earlier copied export found only 580 channel samples with
a one-level 16-bit difference, and fully opaque alpha.

Explicit tests passed for both CUDA memory paths and CPU reference output,
including composed camera/manual/local edits, all orientations and wide dimensions.
New lifecycle checks verify cleanup-cache rebuilds, sensor immutability and release
of retired inputs. The direct Rust-allocation probe passed. All 32 default tests,
WGPU parity and strict Clippy for CUDA/default/no-GPU builds passed. The native
editor used the new path on the real GFX source and closed normally.

No source photograph was changed. Local page advice did not alter system-wide
memory settings or other projects. All test inputs, outputs, traces and private
display configuration stayed under `/tmp/rawpuppy-validation`; owned UI/display
processes exited after inspection. See [system-memory.md](system-memory.md) for
ownership invariants, selection, profiling, and measurement limitations.

[Implementation CI](https://github.com/dllu/rawpuppy/actions/runs/37898172763)
passed all desktop and native Moebius jobs. Linux's CUDA adapter compilation step
passed without a GPU; the native macOS Metal parity step also passed explicitly.
