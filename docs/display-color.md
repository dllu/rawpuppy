# Display color policy

Display conversion is independent of the photograph's edit recipe and export
space. A separate worker discovers profiles, so display-service delays do not
block photo processing or the UI. Requests follow the native window's current
monitor and are refreshed every two seconds. A changed profile triggers a new
preview; unchanged profiles do not re-render the photograph.

On X11, the application first reads an attached RandR output's `_ICC_PROFILE`,
then the matching Xinerama root property (`_ICC_PROFILE` or `_ICC_PROFILE_n`).
It avoids applying a primary display's profile to an unprofiled secondary display.
Colord is an additional source when the monitor name matches the display device's
`XRANDR_name` metadata. Missing profiles fall back to sRGB.

On legacy Wayland without the color-management global, a matching colord display
profile is used when available; otherwise the status identifies the legacy sRGB
fallback. When `wp_color_manager_v1` is advertised, previews stay sRGB for the
compositor instead of targeting the physical display twice. The owned native
window explicitly negotiates an sRGB image description on Winit's existing
Wayland connection. Named sRGB primaries/transfer are used only if advertised;
otherwise an advertised ICC creator receives the application's sRGB profile.
Rendering intent is selected from advertised capabilities. The image description
must be ready before it is applied, and its color-control object is retained for
the window lifetime. Legacy physical-monitor ICC overrides remain rejected on
managed compositors to prevent double conversion.

Negotiation dispatches a private pending event queue without blocking the UI.
The window is retained while guest protocol state exists; the bridge does not
take over its listener, commit its buffers, destroy its surface or close Winit's
connection. Failure and timeout paths clean up only owned protocol objects. If
the compositor has no color-management global, the existing legacy policy applies.

On macOS, the owned CAMetalLayer tree is explicitly tagged sRGB for automatic
ColorSync matching. A manual ICC selection converts the photo to device values
and disables that layer matching. The UI updates the layer policy with the encoded
preview it presents. This changes the application's surface, not monitor settings.

On Windows, the current monitor's device context supplies its default ICC via
`GetICMProfileW`. Missing profiles use sRGB. Windows documents that its profile
APIs return no profile under Advanced Color unless a legacy compatibility helper
is enabled, allowing automatic system matching of sRGB in that case. Manual
overrides require an appropriate legacy display configuration.

ICC profiles must describe RGB. The photo worker caches a Little CMS transform
by profile content hash, converting linear RGBA directly to display RGBA8 with
alpha retained. This removes the earlier intermediate RGB copies and repeated
transform setup. Custom profiles are reread so replacing a file is detected;
the editor can return to Automatic. Neither setting dirties the XMP recipe.

Validation covers numeric RGB-reference agreement, alpha, profile replacement,
monitor geometry, and live X11 root-property replacement/removal in an owned
Xvfb session. The editor's automatic X11 selection and live preview change were
also visually inspected. Native macOS/Windows code is exercised in desktop CI;
physical monitor, multi-monitor, and managed Wayland colorimetry checks
remain in the completion audit.

The optional `edit --hdr` launch selects an advertised extended-linear float
surface and uses automatic compositor matching. Its photo and GUI pixels share
the reference-white scale; physical-monitor ICC transforms and the SDR surface
bridge are bypassed. Unsupported native surfaces retain SDR presentation.
[HDR presentation](hdr-presentation.md) records native-frame checks and remaining
display/physical coverage.

An owned headless Mutter session without the color-management global also
rendered the editor through Vulkan, loaded its saved synthesis layers, and showed
“Automatic: legacy Wayland sRGB fallback.” The editor closed normally without
changing the recipe. This verifies the legacy policy and UI; it does not measure
display colorimetry or establish managed Wayland behavior. The private compositor,
bus, and PipeWire instance were stopped after inspection.

A private Weston 16.0.0 session with color management enabled exercised the
managed path. Its installed Little CMS 2.14 advertised no exact named sRGB
transfer, so the ICC fallback was used: 588-byte sRGB profile, `ready` event,
then relative-colorimetric surface description on the existing native surface.
Protocol logs verify owned color-object teardown before Winit's normal surface
destruction. A separate unmanaged Weston session verifies legacy operation and
normal probe close. [wayland_surface_probe.rs](../examples/wayland_surface_probe.rs)
provides the native probe; it fails if negotiation or expected tagging does not
complete. The editor's rendered image was captured in the managed session.

These are protocol/runtime checks on a headless compositor, not physical-monitor
measurements or HDR presentation proof. Exact parametric sRGB selection is covered
by capability tests; the actual runtime exercise used the ICC path. The private
Weston build and newer protocol XML were installed only in the evaluation cache,
without replacing the system compositor or libraries.

Sources: [ICC profiles in X specification](https://www.freedesktop.org/wiki/Specifications/icc_profiles_in_x_spec/),
[Wayland color-management protocol](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/staging/color-management/color-management-v1.xml),
[CAMetalLayer color space](https://developer.apple.com/documentation/quartzcore/cametallayer/colorspace),
[Windows Advanced Color ICC behavior](https://learn.microsoft.com/en-us/windows/win32/wcs/advanced-color-icc-profiles).
