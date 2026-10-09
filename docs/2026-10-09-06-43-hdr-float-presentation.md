# 2026-10-09 06:43 — Float HDR presentation and native surface probe

The new display module supplies an FP16 photo texture, linear premultiplied-alpha
quad renderer and egui paint callback. Signed and above-white values are retained
within the float storage format. Explicit reference-white scaling belongs to
display policy rather than the photograph's recipe. Surface selection requires
an advertised RGBA16F/ExtendedSrgbLinear pair; float format alone is insufficient.

GPU readback preserves −0.125 and 2×/4× white and matches linear alpha blending
over a known background. An owned Xvfb session exposes only sRGB BGRA8 formats.
A private managed Weston 16 session exposes the extended-linear float pair and
presents three frames through the production quad renderer. Advisory display
luminance is unknown on both Linux paths, which is distinct from signal support.

The Wayland trace proves the Vulkan WSI owns this HDR surface's color-control
object, describes sRGB primaries with extended-linear transfer, waits for readiness
and destroys its owned controls. The existing SDR bridge must be bypassed on this
path to avoid obtaining a duplicate control object. This driver describes
luminance/reference white as `0,80,203`; editor integration must account for those
signal units. The compositor's output remains SDR, so this is not physical HDR
colorimetry proof.

Validation: 40 default tests, actual float GPU readback, strict Rust 1.99 Clippy,
formatting and diff checks passed. CI now includes float GPU readback alongside
Vulkan and native Metal parity. The preceding embedded-CA CI also passed all four
jobs, including its native Metal step. Both private display sessions were stopped.

[The HDR design and measurements](hdr-presentation.md) record the tested contract
and outstanding work. The editor still uses its SDR texture/surface path; native
HDR editor integration, reference-white UI matching, display transitions and
physical/platform verification remain. The full PROMPT.md goal remains active.
