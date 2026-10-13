# Perspective guides and crop

**Auto vertical** aligns reliable vertical architecture while retaining the
scene's horizontal perspective. **Auto both** also aligns one unambiguous
horizontal façade direction. Detection and fitting run on the photo worker.
Correction and an optional automatic crop form one undoable geometry change.
Insufficient evidence leaves the correction unchanged and exposes the detected
lines for review and manual editing.
Save the result normally; it is restored from the Rawpuppy XMP without rerunning
segmentation. Existing scale is retained. **After perspective** can keep the
current crop, choose the largest area without empty corners, or fit the selected
aspect ratio. Corner gaps are also available for the existing neural fill tools.

## On-image controls

Select **Perspective** above the photograph or press `P`. Its sidebar section
opens at the top. **Find guides** detects features without applying a correction;
the automatic correction buttons also expose the features used by their fit.
Blue segments represent vertical features, amber represents horizontal features,
and gray segments are excluded. Choose **Draw vertical** or **Draw horizontal**,
then drag on the image to add a segment. Click a line to select it, drag an
endpoint to adjust its extent, or drag its body to move it. **Use line** and the
axis buttons change its role. Right-click a line, press Delete on a selection,
or use **Remove selected line** to remove it. **Apply vertical** and **Apply both**
fit the enabled guides; two separated segments per requested axis suffice.
Automatic detection keeps its stricter four-line consensus requirements.

Select **Crop** or press `C` to reveal the full corrected canvas. Drag its eight
corner/edge handles to resize the crop; drag inside to move it. The excluded area
is shaded. Presets include Free, Original, 1:1, 3:2, 4:3, 5:4, 7:5, 16:9 and 2.39:1;
**Swap width / height** switches orientation. Ratios refer to physical output
pixels, including portrait images. Grid choices are None, Rule of thirds, Golden
ratio, Diagonals, Center and Architecture. Grids use the crop frame and disappear
when you leave the framing tools. **Done**, Enter or Escape returns to the cropped
photo. Each handle or guide drag is one undo step; guide edits and aspect locks
are saved in the sidecar.

**Largest area** finds an unconstrained crop without empty corners. **Fit aspect**
uses the selected ratio. Both run in the background and account for perspective,
manual distortion and active camera lens/CA corrections. The search maximizes
coverage over a conservative 384×384 grid, checking corners and centers of every
candidate cell. Fixed ratios are retained exactly; crop-boundary precision is
approximately one grid cell. The work and memory do not depend on RAW resolution.
Results are discarded if the document or its history changed during the request.
The crop excludes original-image gaps; generated corner fills are not used to
enlarge it.

The main camera inputs are focal length and physical sensor width/height.
**Advanced geometry** keeps rotation, pitch/yaw, field of view, scale, shifts and
manual lens coefficients available, closed by default. The rendering pipeline
continues to use the single composed backward homography.

## CLI and detector

```sh
rawpuppy perspective photo.raf --output corrected.png
rawpuppy perspective photo.raf --mode both --save-edits
rawpuppy perspective photo.raf --mask-provider classical
```

The CLI prints a JSON report with accepted segments, vanishing directions,
residual, rejection reason and proposed geometry. A declined correction returns
an error and writes neither edits nor an export. Exports default to a 1440-pixel
maximum edge; `--max-edge` controls output size. Analysis remains a bounded
preview regardless of the original's resolution.

The independent Rust detector uses four scene-linear samples per analysis pixel,
then normalizes exposure before tone mapping. Existing exposure, colour looks,
perspective, crop, retouch and generated layers do not influence its evidence.
It verifies continuous subpixel Scharr
edge support, fits the entire segment, rejects residual bow, follows the edge
beyond its endpoints to reject quantized arc tangents, and suppresses duplicate
edges. Collinear interrupted window/masonry fragments supply long baselines.
Deterministic robust consensus requires at least four agreeing edges,
spatial separation, adequate support and no comparably supported competing
direction. Apparent verticals converging inside the image, extreme poses and
inconsistent corrected lines cause abstention. Masks select photographic edges;
mask boundaries never supply line evidence.

Vanishing points remain homogeneous, including points at infinity. Square-pixel
coordinates handle portrait and landscape images. The solver fits camera
directions with robust angular uncertainty. A near-upright gravity prior prevents
steep receding façades from dominating the vertical family. Horizontal candidates
are selected after vertical alignment and constrained to the resulting horizon.
The solver decomposes the orthogonal rotation into the existing pinhole
controls. A translation keeps the original center centered. Everything is folded
into the existing backward homography, shared by CPU, CUDA, Vulkan and Metal;
there is no additional full-image resampling pass.

The wider cooked-reference evaluation covers 22 architectural photographs and
two non-architectural controls, including seven architecture cases reserved from
development. Results and remaining reference-alignment discrepancies are in
[the evaluation journal](2026-10-11-general-perspective-evaluation.md).

New GFX 100/100S documents use their published
[43.8×32.9 mm sensor dimensions](https://fujifilm-dsc.com/en/manual/gfx100s/technical_notes/spec/)
with the actual EXIF focal length. For other cameras, physical dimensions are
estimated when both actual and 35 mm equivalent focal lengths are present;
the dimensions remain editable. Equivalent-only metadata still supplies field
of view. The conversion uses the
[CIPA reference diagonal](https://www.cipa.jp/std/documents/e/DCG-X001-2018_E.pdf),
oriented aspect and camera lens framing scale. Existing recipes retain their
field of view. Without this metadata, enter the camera's sensor dimensions and
focal length; the existing 50° fallback remains until you set them. Portrait
orientation swaps the horizontal sensor dimension automatically. Both-axis fitting respects these intrinsics rather than
changing focal length to force unrelated line families to appear orthogonal.

## Optional SAM 3

The `sam3` feature adds native local inference from a separately installed
[Meta SAM 3 image checkpoint](https://huggingface.co/facebook/sam3), pinned to
revision `3c879f39826c281e95690f02c7821c4de09afae7`. It shares one 1008-pixel image
encoding across `building`, `wall`, `tree` and `person` prompts. Architectural
support excludes trees and people, with small boundary margins. The prepared
graph is about 1.8 GiB; it is cached across editor requests. Photographs stay local.
CPU inference is substantially slower than CUDA. The classical option remains
available for machines where running this model is impractical.

Use the same LibTorch 2.13 setup as [Moebius](moebius.md), adding `sam3` to the
feature list. Preparation requires Python, PyTorch and Transformers 5.17, and
access to Meta's checkpoint:

```sh
python tools/export_sam3.py --output /path/to/prepared-sam3 \
  --image facade.jpg --image curved-interior.jpg --image trees.jpg
cargo build --release --features sam3
# NVIDIA photo compute and SAM inference
cargo build --release --features cuda,sam3
rawpuppy install-sam3 /path/to/prepared-sam3
```

The exporter pins/checks original parameters, disables TF32, verifies eager
versus prepared masks on three distinct photographs, makes tensor factories
follow the inference device, and saves graph hashes, provenance, the SAM License
and attribution. Installation validates these files before publishing the cache.
Model access and use follow [Meta's SAM License](https://github.com/facebookresearch/sam3/blob/main/LICENSE).

Auto uses the installed model when the feature and model are available;
otherwise it uses the classical detector. The editor's **Architecture mask
(SAM 3)** checkbox under **Advanced geometry** disables semantic selection for a comparison. Explicit CLI
`--mask-provider sam3` requires the feature and installed model. An installed but
invalid model reports an error instead of silently ignoring semantic selection.

Segmentation does not establish straightness or world orientation. Windows,
posts and façade edges can still be missed, and a curved roof contains genuinely
straight receding beams. Conservative abstention remains necessary. Classical
detection cannot reliably distinguish a genuinely straight tree trunk from a
building column. Multiple façades and interiors often suit vertical correction
better than forcing both axes. CUDA and CPU are tested locally; physical Metal
inference of this SAM graph remains unverified.
