# Composed camera distortion and vignetting

Implemented versioned camera corrections in the existing backward map and
scene-linear gain, using one small shared radial lookup. GPU lookup data extends
the existing parameter buffer rather than adding a storage binding or raster.
New documents enable available RAF distortion/vignetting data; saved recipes keep
their prior behavior. Camera controls are explicit in the editor and XMP. Compare
preserves the camera map. Manual adjustments remain available in the same modules.

Validated the GFX100S/GF55mm F1.7 conventions through an isolated Darktable output
oracle, using separate UI-captured correction settings and independent linear
exports. Geometric feature matching strongly favors the direct distortion sign;
vignetting is reciprocal transmission percent rather than its square. Native
vignetting ratios closely match the oracle. No Darktable source code was used.

Four new tests cover geometry/gain values, camera framing, real curve samples,
invalid data and the exact recipe hashes of two historical synthesis fills.
All 32 default tests and strict Clippy with the optional neural feature passed.
Explicit CUDA and Vulkan parity tests passed with camera and manual corrections
composed alongside all orientations, tone/local edits and a 100,003-pixel width.

Built the native Moebius/CUDA release, rendered the real GFX photo, and exercised
the editor in an owned X11 session. Automatic correction selection, vignetting
toggle, atomic save, reopen and normal close worked. CUDA/Vulkan 1,800-pixel
previews averaged 12.78/12.04 ms warm on GB10. Native camera CA, other vendors and
off-center crop validation remain open. See [lens-correction.md](lens-correction.md)
for measurement scope and limitations.
A full corrected 8,736×11,648 TIFF was rendered through CUDA in 2.36 seconds and
exported in 0.94 seconds. Inspection confirmed 16-bit RGBA and an sRGB ICC profile.

All photographs, sidecars, exports, oracle databases and temporary displays were
kept under `/tmp/rawpuppy-validation`. Owned UI/oracle/display processes were
closed or stopped after inspection. No contents of `~/pictures/raw` changed.
