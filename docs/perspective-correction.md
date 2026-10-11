# Automatic architectural perspective

**Auto vertical** aligns reliable vertical architecture while retaining the
scene's horizontal perspective. **Auto both** also aligns one unambiguous
horizontal façade direction. Both run on the photo worker, apply one undoable
geometry change, and leave the recipe unchanged when evidence is insufficient.
Save the result normally; it is restored from the Rawpuppy XMP without rerunning
segmentation. Existing crop and scale are retained. Corner gaps are available
for cropping or the existing neural fill tools.

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

The independent Rust detector uses lens-corrected pixels before existing
perspective, crop, retouch or generated layers. It verifies continuous Scharr
edge support, fits the entire segment, rejects residual bow, follows the edge
beyond its endpoints to reject quantized arc tangents, and suppresses duplicate
edges. Deterministic robust consensus requires at least four agreeing edges,
spatial separation, adequate support and no comparably supported competing
direction. Apparent verticals converging inside the image, extreme poses and
inconsistent corrected lines cause abstention. Masks select photographic edges;
mask boundaries never supply line evidence.

Vanishing points remain homogeneous, including points at infinity. Square-pixel
coordinates handle portrait and landscape images. The solver fits camera
directions and decomposes their orthogonal rotation into the existing pinhole
controls. A translation keeps the original center centered. Everything is folded
into the existing backward homography, shared by CPU, CUDA, Vulkan and Metal;
there is no additional full-image resampling pass.

New RAW documents use an actual EXIF 35 mm equivalent focal length when available
in RAF's embedded EXIF or a TIFF EXIF directory. The conversion uses the
[CIPA reference diagonal](https://www.cipa.jp/std/documents/e/DCG-X001-2018_E.pdf),
oriented aspect and camera lens framing scale. Existing recipes retain their
field of view. Without this metadata, set **Field of view** to match the camera;
the default is 50°. Both-axis fitting respects these intrinsics rather than
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
(SAM 3)** checkbox disables semantic selection for a comparison. Explicit CLI
`--mask-provider sam3` requires the feature and installed model. An installed but
invalid model reports an error instead of silently ignoring semantic selection.

Segmentation does not establish straightness or world orientation. Windows,
posts and façade edges can still be missed, and a curved roof contains genuinely
straight receding beams. Conservative abstention remains necessary. Classical
detection cannot reliably distinguish a genuinely straight tree trunk from a
building column. Multiple façades and interiors often suit vertical correction
better than forcing both axes. CUDA and CPU are tested locally; physical Metal
inference of this SAM graph remains unverified.
