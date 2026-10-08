# Native inference reference and modern model evaluation

Implemented optional Rust ONNX Runtime inference, pinned model downloads with
SHA-256 verification, bounded LaMa model inputs, image/mask validation, a
`inpaint-lama` reference CLI, and model-independent research on current candidates.
The inference test used actual model weights and proved finite generated pixels,
reconstruction of a missing constant patch, preservation of every unmasked sample,
and original-source immutability. Strict Clippy and core tests pass with neural
support. ONNX Runtime loads and executes on this ARM64 system.

The user steered selection toward current models. LaMa is explicitly a baseline;
modern synthesis integration remains open. Downloaded and verified Moebius
checkpoints, inspected code/licensing, and completed GB10 inference benchmarks.
Recorded the real square-grid limitation and provisional visual quality findings
in `inpainting-research.md`. Qwen Image 2.1 was verified against its new research
license; earlier Apache-licensed Qwen releases must not be substituted silently.
Also identified FLUX.2 klein 4B as a permissive 2026 comparison candidate.

The GPU checkpoint `232766e` passed Linux, macOS and Windows CI, including actual
Vulkan parity on a Linux software GPU. Native Metal and Wayland runtime tests,
modern Rust synthesis integration, saved synthesis recipes, further color/quality
comparisons, and unified-memory optimization remain open.
