# Default CUDA selection and a single inpainting path

The default Cargo build now includes dynamically loaded CUDA alongside
Vulkan/Metal. The existing automatic renderer selection prefers CUDA, then the
native graphics API, then CPU. No build feature or backend flag is required for
normal CUDA use. Builds do not require a CUDA SDK; actual CUDA execution needs
the driver, NVRTC and toolkit headers. Missing libraries and headers are checked
before device/compiler initialization so ordinary unavailable-CUDA cases can
fall through without CUDA loading panics. `CUDA_PATH` locates a nonstandard
toolkit installation. Explicit backend overrides and reduced CPU/Vulkan/Metal
builds remain available for hardware diagnostics and packaging.

Removed the legacy LaMa implementation, `neural` Cargo feature, `ort` dependency,
model URL/checksum/download helpers, `fetch-lama` and `inpaint-lama` commands, and
its tests. Cargo.lock also drops ONNX Runtime and its unused transitive
dependencies. Moebius is the sole application inpainting implementation.
Historical comparison measurements remain in the research journal; the current
selection document now identifies LaMa as historical rather than executable.

README includes `cargo build --release --all-features` and
`cargo run --release --all-features -- edit photo.raf`, covering CUDA, Moebius,
joint RAW reconstruction and SAM 3. It links the matching LibTorch 2.13 and
prepared-model setup instead of duplicating those instructions. CI checks the
default CUDA build on all desktop platforms, plus CPU and Vulkan/Metal builds
without CUDA, replacing the old ONNX inference check.

Validation on Rust 1.99:

- Default tests passed. The CLI regression compares automatic output and
  missing-toolkit fallback with an explicit CPU export to within two 16-bit code
  values. With `RAWPUPPY_TEST_CUDA=1`, it also requires actual CUDA selection.
- Strict all-features, all-targets Clippy passed with matching LibTorch 2.13.
- The documented all-features release build passed. Its executable exposes
  Moebius generation, RAW model installation and SAM 3 installation, omits both
  LaMa commands, and successfully exports using coherent CUDA without a backend
  flag. CPU-only and Vulkan/Metal-only all-targets builds also passed.
- A default-build export on the GB10 selected `cuda (system memory)` without a
  backend flag. An owned fake CUDA driver returning no device selected native
  Vulkan. Missing headers also selected native Vulkan.
- Owned-process loader probes hiding the CUDA driver or NVRTC selected Mesa
  Vulkan with no panics. These probes explicitly selected the software Vulkan
  ICD so hiding the CUDA driver did not also disable NVIDIA's Vulkan device.
- Formatting, whitespace and the resolved feature/dependency catalog were
  checked. No active LaMa commands, code, tests or dependencies remain.

Probe outputs are under
`/tmp/rawpuppy-validation/default-cuda-2026-10-11`; source photographs were not
modified. Physical macOS/Windows execution was not rerun locally; the existing
desktop CI matrix covers those platforms.
