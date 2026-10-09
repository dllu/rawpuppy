# HDR presentation

The photo pipeline and float EXR output preserve signed and above-white values.
The current editor still encodes previews to RGBA8. A native HDR editor therefore
needs both float photo presentation and an explicitly configured native surface;
neither a float working buffer nor the Linear tone choice supplies the latter.

The new `display::hdr` module implements the float photo renderer. A bounded
RGBA16F texture preserves extended RGB within the storage format's finite range
of ±65,504. Alpha is clamped to its coverage range, and RGB is premultiplied before
linear filtering. Its quad writes linear values with premultiplied-alpha blending
to an RGBA16F target. GPU readback verifies negative values, 2×/4× white and
translucent highlights over a known background. The renderer also exposes an egui
paint callback for the later native surface integration.

Converting a preview requires an explicit reference-white signal scale. This is
display policy, separate from the photograph's edits and EXR values. It must be
chosen alongside the desktop's surface luminance convention; merely preserving
the float numbers does not establish matching brightness between SDR and HDR.

`SurfaceChoice::extended_linear` requires an advertised pair of **RGBA16F and
ExtendedSrgbLinear**. A float format advertised only with sRGB or encoded extended
sRGB does not satisfy that contract. Advisory HDR information is kept distinct:
missing luminance or headroom means unknown, not a proof of an SDR display.
These conventions follow [wgpu's HDR surface API](https://wgpu.rs/doc/wgpu/documentation/color/hdr_surfaces/index.html).

## Native probe

```sh
cargo run --example hdr_surface_probe -- --output /tmp/hdr-capabilities.json
# Require actual extended-linear presentation, rather than only reporting capabilities:
cargo run --example hdr_surface_probe -- --require-hdr
cargo test --test hdr -- --include-ignored
```

The probe creates its own native window, queries actual format/color-space pairs
and advisory display information, and presents three float frames when the
extended-linear pair is supported. It exits normally when that pair is absent;
`--require-hdr` instead fails. Result files must be fresh. It does not edit photos,
change monitor settings, or claim physical HDR capability from surface support.

On the GB10, an isolated Xvfb/X11 session advertises only sRGB BGRA8 formats.
The same Vulkan device under private managed Weston 16 advertises RGBA16F with
extended-linear sRGB, and the probe presents all three frames. Weston is using a
float shadow buffer with an SDR default output; this is native signal/protocol
validation, not a physical HDR monitor measurement.

The Wayland trace shows the Vulkan WSI implementation owns the color-control
object for this explicitly selected HDR swapchain: sRGB primaries, extended-linear
transfer, ready event before use, and teardown of its owned color objects. The
editor's current SDR bridge **must not obtain a second color-control object** on
this path. That would violate the protocol's one-control-per-surface rule.
[The Wayland specification](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/staging/color-management/color-management-v1.xml)
also defines the luminance/reference-white parameters. This tested driver requests
`set_luminances(0, 80, 203)`, which needs explicit reference-white scaling during
editor integration; it is not a universal statement about every driver or desktop.

The measured capability reports and protocol assertions are in
[the validation record](data/hdr-native-gb10-2026-10-09.json).
The private compositor and Xvfb processes were stopped after each probe.

## Remaining editor integration

The installed egui-wgpu renderer still selects its preferred SDR format and uses
the default surface color space. The float callback is not yet wired into the
editor. Native integration must select the advertised pair, render GUI colors in
the same linear signal with appropriate reference white, bypass physical-monitor
ICC conversion and the SDR surface bridge on compositor-managed HDR, and retain
SDR fallback on unsupported surfaces. Display transitions, macOS/Windows native
runtime, and physical HDR colorimetry still need verification. The full project
goal remains active.
