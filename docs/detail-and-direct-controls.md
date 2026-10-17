# Direct controls and detail

**Straighten** (`S`) measures a drawn horizontal or vertical line and adjusts
rotation only. It works on the corrected preview, including existing perspective,
crop and zoom. Draw along a horizon, jamb or other edge you want level. The nearer
image axis is selected automatically; reversing the drag gives the same result.
The angle is shown while drawing. Very short drags are ignored. Escape cancels;
a completed line is one undo step. Perspective estimates and line leveling share
the same rotation value.

Rotation is near the top of **Perspective & crop**, with a fine ±5° slider and
0.1° keyboard/numeric-drag steps. Numeric entry accepts larger angles, and ±90°
buttons handle quarter turns. Viewing a precise automatic estimate preserves its
full stored precision. The other geometry angle sliders also use 0.1° steps.

**Grid** (`G`) works independently of the active tool. **Guides** selects thirds,
golden ratio, diagonals, center or architecture grids. The grid follows the crop
and remains registered while zooming or panning. It is hidden in Before view.
Grid visibility and brush size are view settings, without photo history entries.

Under **Light**, **Draw graduated filter** places the existing graduated ND
filter directly. Drag from the unchanged side to the filtered side; the endpoints
set the transition's direction, center and width together. An inactive filter
starts at −1 EV. Drawing a new line replaces its placement while preserving an
existing exposure amount. Use the exposure slider to adjust strength. Escape
cancels an unfinished line, and undo restores the entire previous placement.
Existing direct crop handles, perspective guides and neutral-area color
calibration remain available. Shift-wheel changes the retouch/removal brush
radius on the photo, without zooming it.

**Detail** has separate sharpening and local contrast amounts. New documents
start with sharpening at 0.2 and local contrast off. Existing sidecars retain
their prior rendering; zero detail is omitted when serializing historical
recipes. The detail stage runs after retouch, curves, toning and generated fills,
before output color conversion. Changing it does not invalidate saved fills or
sharpen an inference context twice. Perspective analysis excludes detail edits.

The implementation is independent of Darktable. Its manual describes
[line-based leveling](https://www.darktable.org/2024/12/howto-in-5.0/)
and [edge-aware local contrast](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/local-contrast/).
Its [sharpen documentation](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/sharpen/)
also explains why large unsharp-mask radii are undesirable. These informed the
interaction and conservative defaults; no Darktable processing code was copied.

Detail operates on log luminance and applies one scalar RGB gain, preserving
chromatic ratios and unbounded HDR values. Sharpening uses a small binomial
neighborhood, a noise threshold, strong-edge attenuation, and a clamp to observed
local extrema. Local contrast uses three scales of separable bilateral diffusion,
guided by the original luminance. Range rejection prevents mixing across strong
edges; bounded residuals and local extrema constrain overshoot. Transparent
samples do not contribute, and nonpositive luminance is left unchanged.

Fine sharpening attenuates in reduced previews to avoid exaggerating unresolved
texture; use 100% to judge it. Local contrast scales with the full image's shorter
output dimension, keeping its physical footprint consistent across viewport and
export strips. This is a conservative detail enhancer, not lens deconvolution or
missed-focus reconstruction.

The RAW/color/geometry composition remains on the selected backend. The final
spatial detail filter currently runs on CPU using Rayon, on bounded overlapping
tiles with complete dependency aprons; it requires no second full-size RGB
intermediate. CPU, Vulkan, Metal and CUDA entry points share this final stage.
Metal is structurally supported but was not physically tested on this Linux
machine. Setting both amounts to zero skips the stage completely.
