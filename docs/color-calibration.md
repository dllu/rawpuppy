# Colour calibration

Choose **As shot** to use the camera's illuminant, **Daylight** (4000–25000 K),
**Black body** (1000–25000 K), or **Custom colour**. The swatch selects the colour
of the light source. Assuming a warmer light produces a cooler corrected image;
assuming a cooler light produces a warmer image. Moving the temperature slider
selects a spectral illuminant. For custom colours its initial value is only a
closest-temperature approximation; CIE x/y retains the actual chosen colour.

**Estimate whole image** assumes the mean scene colour should be neutral.
**Draw neutral box** lets you drag over a surface that should appear neutral;
the rectangle measures the illuminant for the entire photograph. Escape cancels
selection. The box follows the corrected, cropped canvas when zooming or panning.
Avoid coloured objects and windows containing warm lamps when choosing a neutral
patch. Mixed lighting cannot be corrected everywhere with one global illuminant.

Estimation uses bounded floating-point scene samples before calibration,
exposure, channel mixing, tone mapping, display profiles, retouch and generated
fills. It follows lens/perspective/crop and colour-channel geometry, excludes
dark/clipped/transparent samples, and uses the standard sensor reconstruction
when the original RAW is selected for Joint AI reconstruction. This keeps the
picker responsive without requiring a full 100 MP neural reconstruction.

CAT16 adapts scene XYZ from the selected illuminant to the working D65 white.
Its matrix, camera profile, fine channel gains and channel mixer compose into
the existing CPU/GPU calibration matrix. Scene highlights and negative values
are retained. Camera ColorMatrices provide an unbalanced XYZ basis and the
as-shot neutral sets its luminance normalization. Embedded DNG ForwardMatrices
retain their preferred colour rendering; inverse CAT16 exposes their scene
illuminant before the new adaptation is applied.

Old recipes keep their exact historical calibration matrix and serialization
until an illuminant control is changed. New documents start in **As shot** when the profile provides a usable illuminant.
Sampling is one undoable edit. Changes to the document/history while sampling
discard its result; zooming and panning do not invalidate it. Estimation resets
fine channel gains to unity; manual changes between modern illuminants preserve
them. Existing channel-mixer adjustments remain available in the same module.

The interaction follows Darktable's modern
[color calibration](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/color-calibration/)
module. Its implementation was inspected at the user's request; Rawpuppy's
implementation is independent. This implements illuminant adaptation and mean
area estimation, not Darktable's complete colour-checker/profiling feature set.
Colour math uses the published
[CAT16 model](https://onlinelibrary.wiley.com/doi/10.1002/col.22131), CIE daylight
chromaticities, and Planck spectra integrated against the existing attributed
CIE 1931 observer data.
