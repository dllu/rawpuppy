# 2026-10-09 07:22 — Native HDR editor integration

`edit --hdr` now selects an advertised RGBA16F/ExtendedSrgbLinear pair and renders
float photo previews through the native editor. Unsupported surfaces retain the
SDR path. Automatic HDR bypasses physical-monitor ICC conversion and the SDR
surface bridge, avoiding duplicate Wayland color-control ownership. Display
preferences do not change the photo recipe.

A retained, licensed local egui-wgpu extension exposes native surface selection,
linear GUI output, shared HDR information and float frame capture. GUI/photo white
use the same signal scale, with linear alpha coverage. HDR screenshots convert
back to SDR ColorImage while a separate sink retains float signal values.

The actual managed Wayland editor frame preserved −0.317 to 10.148 signal values
from a signed/4×-white EXR at 203/80 scaling. The trace shows Vulkan WSI source
description readiness and clean teardown with no duplicate application control.
Owned X11 and managed Wayland smoke runs both closed normally and retained source
and XMP hashes. X11 correctly fell back when HDR was requested. The native editor
smoke example now runs in Linux/macOS/Windows CI.

Validation: 41 default tests, both actual HDR GPU readback tests, strict Rust 1.99
Clippy, formatting and diff checks passed. GUI white equals photo white, and
translucent GUI white matches linear blending. The prior HDR-component CI passed
all four jobs, including native Metal. New native-window CI results are pending.
All native display sessions and fixture files were isolated from original photos.

Physical HDR colorimetry, more Wayland WSI implementations, live capability
changes and broader platform/hardware coverage remain. The full PROMPT.md goal
remains active.
