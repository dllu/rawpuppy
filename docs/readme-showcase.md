# README images

The example is DSCF0027, The Standard hotel in London, photographed with a
Fujifilm GFX 100S. Its 11648×8736 RAW is the source of both images and both
native editor screenshots. The original RAW, Darktable XMP and cooked reference
remain unchanged; work used a local copy.

The **before** image uses camera calibration, embedded lens corrections and a
linear SDR preview, with zero additional exposure and neutral relative gains.
The **after** image adds 2.1 EV, CAT16 colour calibration from the whole-image
mean (illuminant CIE xy `[0.42779413, 0.4136972]`), saturation 1.04, and AgX SDR
tone mapping. Both-axis pinhole correction uses −12.227947° pitch,
−0.205753° yaw and −0.793441° rotation. The reference supplied the initial
framing; the main façade's roof, base and verticals then refined the alignment
to remove the remaining top-edge tilt. The automatic detector declined this dark, curved
façade; the shown alignment is manually configured and checked against that
reference, rather than presented as an automatic success.

Reference correspondence fitting found 668 inliers among 726 feature matches,
with median geometric error 0.346 pixels at 1200 pixels wide. It supplied an
independent rotation/framing estimate; no Darktable implementation was copied.
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
RUST_MIN_STACK=33554432 cargo run --release --features cuda --example readme_showcase \
  -- /tmp/DSCF0027.raf /tmp/rawpuppy-readme-images
```

The example's recipe is specific to this photograph. It stores output recipes
and images in the supplied output directory and does not save edits beside the
input RAW.
