# README images

The source is DSCF0027, The Standard hotel in London, photographed with a
Fujifilm GFX 100S. The 11648 × 8736 RAF is cached locally as
`.local-fixtures/DSCF0027.raf`. The cache is ignored by Git.

The before export uses camera calibration, embedded lens correction and a
linear SDR preview at zero added exposure. The after export uses +2.1 EV,
AgX SDR, saturation 1.04 and an illuminant estimated from the corrected,
cropped photo. The classical detector fitted −11.14° pitch, −2.46° yaw and
−0.36° rotation on this run. **Fit largest area** then selected a gap-free crop
that includes the roof above the old frame. Both exports were rendered at full
resolution and downsampled to 1200 pixels wide.

The three editor captures show color calibration, a level line in Straighten,
and Crop with the complete projected footprint and both grid layers. They
were captured from the native Wayland editor using CUDA system memory.

The saved [before](images/standard-london-before.json) and
[after](images/standard-london-after.json) recipes reproduce the exports.
The example can also rerun detection and the crop fit from the original RAW.

```sh
RUST_MIN_STACK=33554432 cargo run --release --example readme_showcase \
  -- .local-fixtures/DSCF0027.raf /tmp/rawpuppy-readme-images
```

A build with an installed SAM 3 model can use a different set of detected
edges. The example writes its previews and recipes to the output directory.
