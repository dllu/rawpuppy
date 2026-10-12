# Uniform scroll zoom while previews render

Scrolling updated zoom after the canvas geometry had already been calculated.
The next frame then stretched the previous viewport texture across the new
visible rectangle. When a portrait photo already filled the canvas height,
only its displayed width grew while the replacement render was pending.

Apply scroll, pan and double-click navigation before calculating the displayed
viewport and sending its render request. Preview replies now carry their source
region, which stays attached to the accepted SDR or HDR texture. Drawing maps
that region into the current viewport and clips its overlap, preserving both
scale and image position while a new render is pending. HDR callbacks accept
the same UV crop through a separate uniform binding. Zooming out can briefly
show the checkerboard outside the previously rendered region until the new
preview arrives.

Regression tests inspect actual egui photo meshes while withholding render
replies. They cover portrait, landscape and cropped photos, repeated scrolling
past both canvas edges, same-frame render requests, and zooming out from an
accepted subregion. The first regression failed before the fix. The native GPU
HDR readback test also verifies cropping on both axes and preserves its existing
reference-white and extended-signal checks.

Validation passed:

- Default library tests: 40 passed, 3 unrelated device tests ignored.
- All-feature GUI tests: 25 passed.
- HDR tests including native GPU readback: 5 passed.
- Formatting and whitespace checks.

Strict default all-targets Clippy stops at a pre-existing
`manual_range_contains` warning in `src/perspective/edges.rs:343`.
All-feature all-targets Clippy passes with that single lint allowed and every
other warning denied; the unrelated perspective code is unchanged.
