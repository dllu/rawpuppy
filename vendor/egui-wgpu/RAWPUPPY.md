# Rawpuppy local extension

This is egui-wgpu 0.36.2 from the published crates.io source. Upstream source
hashes are retained in UPSTREAM.json; MIT and Apache license notices are included.
The files were copied into this repository, without changing the shared Cargo
registry or another project's dependencies.

Rawpuppy changes:

- Optional advertised RGBA16F/ExtendedSrgbLinear surface selection with SDR fallback.
- Explicit selected color space and shared advisory display information in RenderState.
- Linear HDR GUI output with reference-white scaling and correct alpha coverage.
- Float screenshot stride/conversion and an optional raw float readback sink.

The standard renderer constructor and default surface selection retain SDR
behavior. Rawpuppy requests HDR explicitly. The extension changes lib.rs,
winit.rs, renderer.rs, egui.wgsl and capture.rs, and adds the half dependency.
All original source and license notices are retained. This is a local integration
patch, not a claim that upstream egui supplies these additional interfaces.
