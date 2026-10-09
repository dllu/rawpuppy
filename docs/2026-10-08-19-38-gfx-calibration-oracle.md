# GFX100S linear camera calibration comparison

Compared 1350×1800 scene-linear exports of the same isolated GFX100S RAF copy
using Rawpuppy's fixed input/calibration pipeline and Darktable 5.5.0+1479.
Darktable used workflow `none`, no custom presets, no OpenCL, eight workers,
linear Rec.709 EXR, and an isolated database/configuration. Rawpuppy used a
separate linear recipe with default geometry/intensity/calibration controls.
Neither comparison applies AgX, a manual curve, split toning or synthesis.

| Whole-image mean | Rawpuppy | Darktable | Ratio |
| --- | ---: | ---: | ---: |
| R | 0.062434456 | 0.062423664 | 1.000173 |
| G | 0.062009776 | 0.062003464 | 1.000102 |
| B | 0.045815074 | 0.045801030 | 1.000307 |

Five additional normalized rectangular patches have per-channel ratios between
about 0.988 and 1.004. The largest discrepancy occurs on a patch with edges.
Rawpuppy samples the original with MHC reconstruction; the oracle uses its
default reconstruction and downsampling. These spatial differences confound
pixelwise equality and attribution of the remaining errors to calibration.

The close global means support the existing sensor normalization, as-shot gain
normalization and camera-to-working matrix for this photograph. They do not
prove agreement for every camera, illuminant, profile selection or user mixer.
Those checks remain in the completion audit. `examples/compare_color.rs` measures
independent float-export means and normalized patches; images remain under
`/tmp/rawpuppy-validation/color`. The copied RAF was used so even an oracle's
accidental sidecar write would stay outside `~/pictures/raw`.
