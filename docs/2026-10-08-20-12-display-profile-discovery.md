# Automatic display discovery and cached preview conversion

Added native monitor requests and asynchronous display-profile discovery, with
RandR/Xinerama ICC properties, matching colord devices, Windows default profile
lookup, and explicit macOS sRGB Metal-layer tagging. Wayland discovery distinguishes
managed compositor transport from legacy output and reports its fallback. Custom
ICC selection remains separate from photo edits and can return to Automatic.

Display conversion now caches its Little CMS transform and works on RGBA directly.
Three new numeric/geometry tests pass, including independent RGB reference output
and exact 8-bit alpha. The ignored X11 test was explicitly executed on an owned
Xvfb display and passed root-profile replacement/removal. The live native editor
also changed its preview and displayed its discovered ICC after a profile was
published on that isolated root. The real desktop's properties were read only.

All 26 default tests and Rust 1.99 strict Clippy passed locally. CI now explicitly
runs the ICC discovery test on its own Xvfb. A native macOS layer test covers sRGB
tagging and disabling matching for manual transforms. Platform build/runtime and
additional compositor/physical-display checks are tracked separately from the
implemented policies in [display-color.md](display-color.md).

All test assets and display configuration changes remain under the isolated test
session and `/tmp/rawpuppy-validation`. No contents of `~/pictures/raw` changed.
