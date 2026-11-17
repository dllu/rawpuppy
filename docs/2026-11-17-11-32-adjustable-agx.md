# Adjustable AgX

The Tone panel exposes AgX white and black exposure, contrast, and toe and
shoulder power. Expand Curve preview to see the neutral response. Advanced curve
holds the pivot, range margin, toe and shoulder starts, output targets and
gamma. Look and Primaries have their own sections. Saturation and its existing
new-photo default are now under Tone. Display curve opens the manual curve.

Auto levels, Auto white and Auto black analyze the current crop. Sample area can
apply those same measurements to a rectangle and can also set the curve pivot,
with an option to preserve its previous neutral output. Analysis uses a grid of
at most 640 samples on its longer edge and ignores geometric gaps. For levels,
the measurement uses the largest channel in the AgX input basis at each
position. The picker trims the outer 0.1% on each end of large samples and
applies the configured margin in stops around 18% gray. The input already
includes exposure, calibration, lens shading and graduated density. The
adjustment is a single undoable edit and is stored in the sidecar.

Untouched AgX recipes still use the existing 97³ lattice. Adjusted recipes use
a shared 4,097-sample per-channel curve, input and output matrices, and hue
restoration in the composed CPU and GPU render paths. The independently
implemented curve uses endpoint-normalized rational roll-offs in log exposure.
Its defaults and matrices were fitted from the existing numerical oracle.
The fit used the neutral diagonal to recover curve powers and pivot slope, then
least squares over 14,000 seeded Rec.2020 lattice coordinates to recover both
white-preserving matrices and the shortest-arc HSV hue restoration amount.

The UI follows the workflow described in [Darktable's AgX manual](https://docs.darktable.org/usermanual/5.8/en/module-reference/processing-modules/agx/).
Rawpuppy's implementation uses the project's existing gamut and saturation
conventions. The original polynomial recipe and the Linear / HDR choice are
also supported.

The adjustable formation passed the 1,677 held-out oracle samples. The composed
render passed CPU comparisons on CUDA with system memory and copied device
memory. A separate Vulkan export of a 480 × 320 gradient using picker settings
differed from CPU by at most one 16-bit channel code. A Wayland editor session
exercised whole-crop auto levels and a white-only rectangle, including sidecar
autosave.
