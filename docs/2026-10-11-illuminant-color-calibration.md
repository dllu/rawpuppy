# Illuminant colour calibration

Implemented an illuminant workflow in the existing colour calibration module:
as-shot camera light, custom colour/CIE xy, daylight and black-body temperature,
whole-image estimation and a drawn neutral sampling rectangle. Inspected the
local Darktable `channelmixerrgb` implementation at the user's explicit request
and its [current module documentation](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/color-calibration/).
The implementation uses independent colour math from the published
[CAT16 model](https://onlinelibrary.wiley.com/doi/10.1002/col.22131), CIE daylight
chromaticities and Planck integration against existing attributed CIE observer
data. No Darktable code was copied.

Camera input now retains an unbalanced XYZ basis separately from the historical
row-normalized working RGB matrix. As-shot sensor neutral sets the basis's
luminance. DNG ForwardMatrices retain their preferred rendering through inverse
and forward CAT16. Old recipes omit the new illuminant field and retain their
exact prior matrix, serialization and recipe hashes. New documents select the
camera illuminant when usable; historical calibration remains available for
inputs without a usable illuminant profile.

The adaptation, camera profile, channel gains and mixer compile into the existing
matrix consumed by CPU, Vulkan/Metal and CUDA. No extra rendering pass or GPU
kernel stage is added. Signed scene values and HDR scale remain intact. Sampling
uses at most 512 samples per dimension from floating-point source data, follows
the corrected/cropped canvas and individual channel maps, and excludes black,
clipped and transparent areas. It does not sample tone-mapped display pixels or
generated content and does not require a full learned RAW reconstruction.

Calibration controls and canvas selection live in their own GUI module. Whole
and box estimates each create one undoable edit; document revision and complete
recipe checks reject stale replies after undo/redo or changing photos. The box
works at zoom and in either drag direction, without painting or panning. Passive
temperature formatting and the colour picker's RGB/HSV float round trips must
not mutate the recipe. The float RGB picker takes linear RGB; it performs display
transfer encoding itself.

Validated on an owned copy of the user's 11648×8736 GFX 100S London RAW. Native
whole-image estimation gave xy `[0.42779413, 0.4136972]` from 195731 usable samples.
A deliberately imperfect box overlapping a warm window gave xy
`[0.46170053, 0.4321765]` and a visibly cooler correction. This demonstrates the
need to choose a neutral patch in mixed lighting, not an accuracy claim for that
box. Native temperature and swatch edits, estimation, save, undo and redo were
checked. RAW and Darktable sidecar were verified unchanged.

The README comparison now uses whole-image CAT16 calibration. Refined the main
façade alignment from manually identified roof/base/vertical points to remove
the remaining roof tilt: pitch −12.227947°, yaw −0.205753°, roll −0.793441°.
Those point constraints have 0.213 pixel RMS after refinement versus 1.306 before,
at 1200 pixels wide. This is a manual refinement, not an automatic-detection
success. The full 101.8 MP render was downsampled for the comparison; native
editor screenshots were captured separately. The before recipe is unchanged,
and its regenerated image is pixel-identical.

Checks cover reference illuminants and temperature direction, CAT16 neutral
adaptation with signed HDR scale, physical camera neutral, DNG ForwardMatrix
preservation, old recipes, sampling independence from exposure/tone/mixer,
crop and zoomed box geometry, invalid samples, passive controls and asynchronous
undo/redo. The final combined-feature suite passed 130 tests (30 external/device
checks skipped); the default suite passed 119 (15 skipped). Strict all-target
Clippy passed on Rust 1.99. CPU/Vulkan/CUDA agreement was checked on signed HDR data at multiple
illuminants. See [validation data](data/calibration-2026-10-11.json),
[usage](color-calibration.md), and the updated
[showcase recipe](readme-showcase.md).
