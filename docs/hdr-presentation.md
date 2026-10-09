# HDR presentation

The photo pipeline and float EXR output preserve signed and above-white values.
`rawpuppy edit photo.exr --hdr` requests native HDR preview. The editor selects
the advertised RGBA16F/ExtendedSrgbLinear pair and uploads a float photo texture.
Unsupported surfaces retain the SDR path. The default launch remains SDR.
The Linear tone choice preserves scene values for HDR preview; AgX retains its
photographic SDR tone rendering on either surface.

The new `display::hdr` module implements the float photo renderer. A bounded
RGBA16F texture preserves extended RGB within the storage format's finite range
of ±65,504. Alpha is clamped to its coverage range, and RGB is premultiplied before
linear filtering. Its quad writes linear values with premultiplied-alpha blending
to an RGBA16F target. GPU readback verifies negative values, 2×/4× white and
translucent highlights over a known background. The renderer also exposes an egui
paint callback used by the native HDR editor.

HDR editor textures store relative-white photo pixels. A shader uniform applies
the current reference-white signal scale when drawing, so changing display-white
values does not re-render or re-upload the photograph. GUI and photo scale update
together. This is
display policy, separate from the photograph's edits and EXR values. It must be
chosen alongside the desktop's surface luminance convention. The tested Vulkan
Wayland path uses 203/80 signal units per relative white. Apple EDR uses system
relative white. Windows uses reported SDR-white nits divided by scRGB's 80-nit
unit; an unavailable or invalid Windows value keeps unit scaling. These are
display choices and never change the saved recipe or exported scene values.
GUI and photo white share the same scale, and GUI coverage is blended in linear
light. Automatic native HDR bypasses physical-monitor ICC conversion and the
application's SDR surface bridge; the Vulkan WSI owns its HDR color description.
Custom physical ICC overrides require the SDR launch.

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

## Editor verification and remaining coverage

The native editor has been run with a signed/above-white linear EXR fixture on
private X11 and managed Wayland sessions. Its actual float frame retains negative
RGB and a 4×-white source, with signal extrema −0.317 and 10.148 at 203/80 scaling.
X11 correctly falls back to an SDR surface. Both paths close normally without
changing the input or its XMP recipe. GPU tests verify matching GUI/photo white
and linear GUI alpha. Native HDR screenshots convert to an SDR ColorImage while
an optional float capture retains the original signal for verification.

The local [egui-wgpu extension](../vendor/egui-wgpu/RAWPUPPY.md) adds explicit
surface selection, linear GUI output, live advisory HDR information and float
capture. Its upstream code and license notices are retained. Normal SDR rendering
uses its prior format and shader path.

`editor_signal_probe` creates its own float fixture, runs the real editor in SDR
and requested-HDR modes, checks source values when HDR is available, and verifies
that both source and recipe hashes remain unchanged. It uses an explicitly enabled
debug-build report hook; release builds omit that hook. Desktop CI now runs these
native windows on Linux, macOS and Windows. [Run 37945480744](https://github.com/dllu/rawpuppy/actions/runs/37945480744)
passed all four jobs after the X11 runtime dependency fix. GPU readback checks
1×, 2×, 0.5× and restored 1× signal white using the same uploaded texture.
[The editor record](data/hdr-editor-gb10-2026-10-09.json) retains
the locally observed native frames.

Physical HDR colorimetry, additional Wayland WSI implementations, live surface
capability changes, and hardware/platform coverage remain. Headroom is advisory
and may be unavailable; surface support does not prove physical HDR capability.
The full project goal remains active.
