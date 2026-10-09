# 2026-10-09 04:52 — Explicit Wayland surface color

Added explicit SDR source description negotiation on the editor's existing native
Wayland surface. The full PROMPT.md goal remains active; this closes a protocol
gap without claiming physical-monitor colorimetry or native HDR presentation.

## Implementation

The UI retains the window while a guest Wayland backend owns its private registry,
color manager and surface-color control. It borrows the live surface identifier
without taking over Winit's listener, queue, ownership or buffer commits. Registry,
capability and image-description events are dispatched asynchronously, with a
timeout, so the client does not block the UI on roundtrips.

Exact named sRGB primaries and transfer are requested only when all required
capabilities are advertised. Otherwise an advertised ICC creator receives the
application's sRGB ICC profile via an owned temporary FD. The compositor must
deliver `ready` before the description is set. Rendering intent is selected from
advertised values. The immutable image description can then be destroyed; the
surface-color control stays alive because destroying it unsets the surface state.
Cleanup destroys only owned objects while the retained Winit window is live.

On compositors without the color-management global, existing legacy policy still
applies. Managed compositors keep sRGB preview bytes for compositor conversion;
legacy physical-monitor ICC overrides are rejected to avoid double matching.

## Runtime evidence

Built Weston 16.0.0 from the official mirror's release commit
`d1882b0a544ae2197b597a6e39478e719bc54302` under the private evaluation cache. Protocol
XML 1.49 and a required display-info subproject were also installed only in that
prefix. The system Wayland libraries/compositor were not replaced.

The managed headless session uses Little CMS 2.14, which advertises no exact
named sRGB transfer. This exposed the need for ICC fallback. Logs show a 588-byte
sRGB ICC creator, image-description `ready`, and relative-colorimetric
`set_image_description` on Winit's existing `wl_surface@13`. The editor remained
connected and rendered its photograph; its capture was inspected. The capture
orientation is vertically flipped by this renderer path, so it is a runtime
artifact rather than a physical-display measurement.

The native probe returned `surface_tagged=true`, destroyed its color control and
manager, then closed normally; Winit subsequently destroyed its own surfaces.
A separate unmanaged Weston session returned `surface_tagged=false` and also
closed normally. The first legacy probe was started before its socket was ready;
it was retried against the same confirmed-live compositor after readiness.

The owned managed-policy test passed: Automatic supplies no legacy ICC transform,
and a custom monitor-profile request is rejected. Capability tests ensure Gamma
2.2 cannot stand in for exact sRGB and that unadvertised rendering intents are
never requested. All 34 default tests, strict default/no-default Clippy, formatting
and CUDA all-target compilation passed. The preceding background-reconstruction
commit's desktop/native-learning CI jobs were verified successful.

## Limits and cleanup

The actual managed runtime used ICC description creation; named parametric sRGB
selection is covered by capability tests rather than that compositor. More
compositors, multi-monitor transitions, physical calibration and HDR presentation
remain unverified. New dependencies are Linux-only protocol bindings.

Managed editor processes were stopped with SIGTERM after checking their command
and log ownership; normal close is verified specifically by the probes. Both
private compositors were stopped after the same checks. Screenshot authorization
was enabled only on the private debug compositor/runtime directory. No monitor
profiles, real desktop settings, user RAWs or other projects' processes changed.

[Display policy](display-color.md) describes behavior. Protocol lines and versions
are retained in [the validation record](data/wayland-surface-2026-10-09.json).
