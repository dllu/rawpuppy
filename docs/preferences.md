# Editor preferences

The editor creates `rawpuppy/config.toml` in the platform's user configuration
directory on first launch:

| Platform | Location |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/rawpuppy/config.toml`, or `~/.config/rawpuppy/config.toml` |
| macOS | `~/Library/Application Support/rawpuppy/config.toml` |
| Windows | `%APPDATA%\rawpuppy\config.toml` |

**Preferences…** changes font size, sidebar width and automatic section expansion.
Font and width values use logical points, so monitor scaling still applies.
Dragging the sidebar divider also saves its width. Sections expand when there is
space; clicking a section keeps your choice for the rest of the session.
The sidebar scrolls whenever the visible controls exceed the window height.

The initial configuration is:

```toml
[ui]
font_size = 18.0
sidebar_width = 420.0
expand_sections = true

[defaults.light]
exposure = 0.7
vignette = [0.0, 0.0]

[defaults.color]
saturation = 1.05

[defaults.detail]
sharpen = 0.2
local_contrast = 0.0

[defaults.sensor]
hot_pixels = false
denoise = 0.0
# Omit recover_highlights for automatic selection: enabled for integer RAWs.
# recover_highlights = true
```

**Save as default** in Light, Color calibration, Detail and Sensor saves the
corresponding values for future photos. Light saves exposure and vignetting;
Color saves saturation; Detail saves both amounts; Sensor saves hot-pixel
correction, denoising and the explicit highlight recovery setting. Reconstruction,
illuminant, channel calibration, geometry, graduated filters, retouch and generated
fills remain photo-specific.

These defaults apply when the editor opens a photo without a Rawpuppy sidecar,
and when **Reset all edits** is used. Saving defaults does not change the current
photo or its undo history. Existing sidecars keep their exact saved recipes.
The CLI's recipe and export behavior remains independent of editor preferences.

Configuration tables and individual values can be omitted to use factory defaults.
Hand edits take effect on the next launch. Invalid configuration produces a visible
error with the file location; the editor starts with factory settings and leaves
the file intact until settings are saved from the UI. Saves validate the settings
and replace the file atomically.

Slider tracks fill the sidebar width, with labels and numeric inputs above them.
Exposure dragging covers −2 to +2 EV; its numeric input accepts −32 to +32 EV.
Displaying values outside a slider's track never clamps them. Rotation dragging
covers −5° to +5°, with unrestricted numeric entry. The **Grid** menu above the
image contains visibility and style controls; `G` toggles visibility directly.
