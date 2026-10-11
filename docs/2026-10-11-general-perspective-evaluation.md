# General perspective fitting and cross-photo evaluation

Replaced the showcase's hand-fitted camera angles with the production automatic
detector. The correction now uses subpixel edge locations, narrow global support
while growing each edge, and verified collinear fragments across interrupted
window columns. Rounded jamb endings stop continuation without treating slow
arc tangents as architecture. Circular-arc and transparency regressions remain
in place.

The original near-vertical image-line classifier could mistake a steep receding
façade's horizontal edges for gravity. The fitter now distinguishes a near-upright
vanishing family from strongly supported competing directions. Robust angular
uncertainty refinement prevents a large number of noisy short frames from
dominating the pose. Horizontal candidates are selected after vertical alignment
and fitted on the gravity-derived horizon, rather than estimating an unconstrained
point and repairing orthogonality afterward. This uses public pinhole/vanishing
geometry, implemented independently.

Analysis is now independent of exposure, colour edits and display looks. Four
stratified scene-linear samples reduce sparse RAW aliasing before exposure
normalization and AgX. No full-resolution reconstruction intermediate is added.
The optional local SAM 3 mask still selects actual photographic edge evidence;
its boundaries do not supply lines. Source, extraction and fitting live in
separate modules under `src/perspective/`.

Found an additional GUI issue during review: slider ranges silently clamped
valid automatic poses when displayed. For example, a −52.5° estimate could become
−45° without input. Sliders now clamp edits rather than existing values, and a
widget regression covers angles, field of view and scale.

Indexed 8922 cooked JPEGs and sampled 1275 exports across capture dates. Of these,
265 had enabled perspective history. Visually screened contact sheets and chose
15 architectural development images, seven reserved architectural images, and
two reserved non-architectural controls. The photographs span Sony and Fujifilm,
2017–2025, flat and corner façades, glass towers, circular windows, interiors,
trees, daylight and night scenes. All work used owned RAW/reference copies.

The production detector receives no cooked export or Darktable parameters. An
independent SIFT/MAGSAC mapping transfers each export's vertical image axis into
the analysis frame for evaluation. This measures agreement with the user's edit,
not exact gravity truth: exports can preserve intentional perspective, and
different lens corrections need not be perfectly homographic. Selected edges,
match counts and reprojection errors are recorded alongside the pose.

| Evaluation | Previous detector | Updated detector |
| --- | ---: | ---: |
| Development architecture accepted | 9/15 | 15/15 |
| Reserved architecture accepted | 6/7 | 7/7 |
| Reserved non-architecture corrected | 0/2 | 0/2 |

Median reference-axis discrepancy per image is 0.104° in development and 0.153°
in the reserved architecture set. Median 90th-percentile discrepancy is 0.168°
and 0.298°, respectively. Two reserved scenes remain around 0.4–0.5° median
disagreement, so this is not a claim of uniformly exact correction or improvement
on every photograph. The earlier 6–26° wrong-axis failures are corrected; one
reserved office view improved from a 1.135° 90th-percentile discrepancy to 0.171°.

The London example now finds 43 semantic vertical supports and eight horizontal
supports automatically. Its recipe preserves the chosen composition while the
angles come from the general detector. Full-resolution export and genuine native
screenshots were regenerated. Camera angles are no longer stored as constants in
the example program.

Validation includes independent vanishing-family regressions, long/short angular
uncertainty, interrupted columns, exposure/look invariance, real photographic
numeric edge fixtures and passive GUI controls. CPU/Vulkan/CUDA antialiased
analysis agrees within 0.0002. The combined feature suite passes 136 tests, and
strict all-target Clippy passes on Rust 1.99. Detailed outcomes are in
[validation data](data/perspective-general-2026-10-11.json).

Reproduce local corpus runs with `examples/perspective_suite.rs`; score separately
with `tools/score_perspective.py CORPUS RESULTS` (OpenCV and NumPy). Reference
images remain in the owned validation workspace and are not distributed in the
repository. The published [DeepLSD paper](https://openaccess.thecvf.com/content/CVPR2023/papers/Pautrat_DeepLSD_Line_Segment_Detection_and_Refinement_With_Deep_Image_Gradients_CVPR_2023_paper.pdf)
was reviewed for line/refinement context; no new line-detector checkpoint or its
implementation is incorporated.
