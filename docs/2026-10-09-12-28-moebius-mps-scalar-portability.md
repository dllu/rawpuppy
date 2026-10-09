# Moebius MPS scalar portability

Observed CI `37976801157` for the real Moebius matrix. Windows CPU and Linux CPU
passed both native sampling cases, including finite output and exact unselected
sample preservation. macOS passed graph preparation and explicit CPU sampling,
but MPS rejected graph loading because four wrapped scalar constants were
serialized as float64 tensors. MPS does not support float64 tensors.

The otherwise float32 graphs contain only scalar values `0.5` and `1.0` at these
sites. Both are exactly representable in float32. Preparation now reloads each
temporary serialized graph and replaces only exact zero-dimensional float64
constants with float32. It refuses inexact scalars and non-scalar tensors, then
checks both the modified graph and its final saved/reloaded form against the
original network. The manifest records the converted values. Computation
modules, checkpoint data, shape and native MPS requirement are unchanged.

An initial in-memory conversion found no constants: serialization materializes
these wrapped values as tensors. Inspecting the reloaded graph exposed this,
and the correction deliberately operates on the representation that native
loading actually consumes. Final reloaded graphs contain no float64 tensor
constants. Encoder converts `[0.5, 1.0]`, decoder `[1.0]`, denoiser `[1.0]`.

All numerical checks remain within `1e-4`: maximum encoder/decoder differences
are `8.1062e-6` / `8.7917e-6`; denoiser is exact. Full preparation took 64.75
seconds and about 3.45 GiB peak process RSS locally. Corrected graphs passed
native CUDA and CPU sampling, taking 1.99 / 19.13 seconds in these debug runs
apart from loading/hash checks. These are observations, not platform latency
guarantees; cached application graphs were not replaced.

Three serialized-graph regression checks pass: exact scalar conversion preserves
results across save/load, while inexact and non-scalar conversion are rejected.
They are included in CI before real model preparation. Python compilation,
workflow parsing and diff checks pass. Native MPS must still be observed in the
rerun with CPU operator fallback disabled. The full PROMPT.md goal stays active.
