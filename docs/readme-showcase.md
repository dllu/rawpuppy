# README images

The example is DSCF0027, The Standard hotel in London, photographed with a
Fujifilm GFX 100S. Its 11648×8736 RAW is the source of both images and both
native editor screenshots. The original RAW, Darktable XMP and cooked reference
remain unchanged; work used a local copy.

The **before** image uses camera calibration, embedded lens corrections and a
linear SDR preview, with zero additional exposure and neutral relative gains.
The **after** image adds 2.1 EV, CAT16 colour calibration from the whole-image
mean (illuminant CIE xy `[0.42714328, 0.413429]`), saturation 1.04, and AgX SDR
tone mapping. The general automatic detector now supplies both-axis pinhole
correction: −11.275674° pitch, −0.49201426° yaw and −0.7281773° rotation.
SAM 3 selects architectural support, while verified straight fragments and
interrupted window columns determine the pose. No reference export is passed
to the detector. The recipe retains the photograph's chosen scale/translation
for composition; its camera angles are estimated automatically.

Reference correspondence fitting found 668 inliers among 726 feature matches,
with median geometric error 0.346 pixels at 1200 pixels wide. That historical
fit supplied the original composition; it is not used by the new detector.
The reference's tone mapping is Filmic, while the shown Rawpuppy result uses AgX.

Both photographs were rendered at full sensor resolution and then reduced to
1200×900, with embedded sRGB profiles. The two genuine native editor captures
are reduced to 1280×853; they show interactive previews and the applied controls.
The four JPEG assets together are approximately 1.3 MB. RAW files and the
Darktable export are not included in the repository.

The complete recipes are saved beside the images as
[before](images/standard-london-before.json) and
[after](images/standard-london-after.json). To reproduce the comparison:

```sh
cp /path/to/DSCF0027.raf /tmp/DSCF0027.raf
RUST_MIN_STACK=33554432 cargo run --release --features cuda,sam3 --example readme_showcase \
  -- /tmp/DSCF0027.raf /tmp/rawpuppy-readme-images
```

Build with `cuda,sam3` and the installed model to reproduce the semantic
detector's pose. A `cuda` build uses classical detection and can choose a
different horizontal façade direction.

The example's exposure, colour and composition are specific to this photograph;
the detector is shared with every input in the editor. It stores output recipes
and images in the supplied output directory and does not save edits beside the
input RAW.
