# Perspective guides and direct crop manipulation

Replaced the sidebar's numeric crop extents with an on-image crop frame: eight
resize handles, a move gesture, outside shading and a full uncropped editing
canvas. `C` enters crop; `P` enters perspective. The framing section moves to the
top of the sidebar, opens with its tool, and closes on Done/Enter/Escape. Advanced
angles, shifts, scale and manual lens coefficients remain available in a closed
section. Common aspect presets, ratio inversion and six guide-grid choices are
shared by the framing tools.

The main perspective inputs are focal length and physical sensor dimensions.
GFX 100/100S initialization uses the published 43.8×32.9 mm sensor and actual
EXIF focal length. Other cameras can use actual/equivalent EXIF to estimate
dimensions; users can replace those dimensions. Lens framing and portrait
orientation are included. Existing sidecars retain their field of view until
edited. Specifications were checked against the
[Fujifilm GFX100S manual](https://fujifilm-dsc.com/en/manual/gfx100s/technical_notes/spec/)
and [GFX100 specification sheet](https://dl.fujifilm-x.com/global/products/cameras/gfx100/pdf/gfx100_catalogue_01.pdf).
The interaction reference was darktable's documented
[crop guides](https://docs.darktable.org/usermanual/3.6/en/module-reference/processing-modules/crop-rotate/)
and [largest-area/original-format cropping](https://www.darktable.org/2016/03/a-new-module-for-automatic-perspective-correction/);
the implementation is independent Rust code.

Automatic detection now retains all segments, including excluded evidence.
Find guides permits review without applying the fitted correction. Vertical,
horizontal and excluded lines use distinct colors. Users can draw, select,
move, resize, enable, relabel and remove segments. Manual fitting accepts two
independent explicitly labeled lines per axis while preserving the automatic
detector's stricter consensus rules. Guides live in lens-corrected coordinates
before perspective/crop, so they remain editable after a correction and reopen
from XMP. Editor evidence and aspect preferences do not invalidate saved neural
layers unless their rendered geometry actually changes.

Added a background automatic crop search over conservative 384×384 coverage.
The unconstrained histogram search maximizes rectangle area across all sampled
positions; a summed-area table and monotone size search support exact fixed
output aspects, including extreme aspect ratios. Coverage includes all three
color-channel mappings and current camera lens corrections. Candidate cells
check corners and centers. The search is bounded independently of original size;
its boundary precision is one grid cell. It crops original-image coverage,
without counting generated corner layers.

Correction plus its optional automatic crop arrives as one undo transaction.
Standalone crop results and perspective results check both document identity
and history revision. Handle/segment drags coalesce into one history step and
stop on undo or disabled editing. Preview textures retain corrected-canvas
coordinates across transitions between full-canvas editing and cropped viewing.
Full-canvas previews extend the existing cropped coordinate system instead of
overriding the recipe's crop, preserving retouch placement and valid neural
layers while showing the surrounding image. Mask overlays use the same mapping.

Regression coverage includes independent pinhole cameras in both orientations,
disabled/duplicate evidence, the known 45° rotated-square crop optimum,
dense checks for empty pixels under lens/CA corrections, extreme fixed ratios,
sidecar/hash preservation, actual native-widget pointer gestures, undo/redo,
and stale background results. Native X11 screenshots were inspected with an
owned JPEG copy on a private Xvfb display; originals were untouched.

The default full test suite passed. The broader build initially exhausted disk
space; clearing only Rawpuppy's regenerable debug example cache recovered 97 GB.
The pre-existing `manual_range_contains` lint in `perspective/edges.rs` remains
outside this change; strict Clippy is checked with only that lint allowed.
All-feature GUI tests passed (29), as did the all-feature all-targets Clippy check
with that single existing lint allowed. Formatting and whitespace checks passed.
The all-feature release build also passed. The README now shows the inspected
native crop interface in `docs/images/editor-crop.png`.
