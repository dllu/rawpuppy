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
compositor instead of targeting the physical display twice. Surface behavior
without an explicit description remains implementation-defined in the protocol,
which recommends sRGB. Legacy custom ICC overrides are rejected on these managed
compositors until explicit custom surface-description negotiation is implemented.

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
physical monitor, multi-monitor, and managed Wayland runtime/colorimetry checks
remain in the completion audit. Native HDR presentation is separate work.

Sources: [ICC profiles in X specification](https://www.freedesktop.org/wiki/Specifications/icc_profiles_in_x_spec/),
[Wayland color-management protocol](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/staging/color-management/color-management-v1.xml),
[CAMetalLayer color space](https://developer.apple.com/documentation/quartzcore/cametallayer/colorspace),
[Windows Advanced Color ICC behavior](https://learn.microsoft.com/en-us/windows/win32/wcs/advanced-color-icc-profiles).
