# Moebius selection and context diagnosis

The latest native GFX workflow left a recognizable background person and a brown
patch. The failure was recorded rather than treated as successful removal. This
follow-up isolates the actual rendered context, saved selection and graph cache.

The benchmark now supports `--saved-fill` on an original and its Rawpuppy sidecar.
It verifies source/recipe/color identities, renders the saved region in float
working RGB, includes preceding saved layers, and derives its exact paint mask.
There is no input PNG quantization round trip. It retains float EXR context,
input/target/inference PNGs, sampler/model identities and optional comparison with
the original asset. Reference images are checked for 512-square dimensions before
fallible allocation. Source and sidecar bytes remain unchanged.

The installed graph reproduces the native asset **exactly at all 512 × 512 × 4
float samples**. The portable graph uses the same pinned scene checkpoint/VAE;
its maximum RGBA difference is 0.0018967 and mean 0.00000445. A recognizable person
remains with either graph. This does not indicate a graph-cache or renderer defect.

The saved target contains **20,473 pixels**, a narrow central strip. Inspection
shows visible parts of the head and arms outside it. Complete object removal is
incompatible with preserving those pixels. An explicit **32-pixel target dilation**
selects **49,604 pixels** and removes the recognizable person on the same float
context, checkpoint, seed zero and 20 full-strength steps. Some soft patch/brightness
differences remain; this is an improved controlled example, not universal quality.

Expanding only the inference mask by 32 pixels produces the **exact same encoded
inferred image** as that wider target. Compositing through the original narrow
target retains unselected object remnants and a conspicuous narrow patch. Target
selection and inference context must stay distinct. The diagnostic flags name
both changes explicitly; the user's paint and XMP are never widened automatically.

Adding 16 inference pixels to the already wider target still removes the person,
but its inspected wall/grass boundary is more visible. This provides no basis for
changing production sampling defaults. The editor's Paint area tooltip now says
to cover the entire object, including edges and shadows, and explains that only
painted pixels are replaced.

```sh
cargo build --release --features cuda,raw-ml,moebius --example benchmark_moebius --locked
target/release/examples/benchmark_moebius \
  --models /path/to/prepared-moebius --image /path/to/owned-photo.raf \
  --saved-fill 0 --seed 0 --steps 20 --strength 1 --iterations 1 \
  --reference-context /path/to/saved-asset.exr --output /tmp/new-control
```

`--target-padding` deliberately changes the diagnostic committed selection;
`--mask-padding` changes only model conditioning. Defaults are zero. Saved-context
comparison currently supports painted fills; corner conformance has its separate
probe. Existing PNG image/mask input also passed a real native run. Final runs
verify exact unselected float samples as well as encoded pixels. Float-buffer
hashes are recorded separately from EXR file hashes, whose bytes may differ due
to parallel compression order.

Strict combined-feature/all-target Clippy, formatting and diff checks passed.
The earlier failure-status milestone passed all six desktop/native-learning CI
jobs. All source/model/settings identities and observations are in
[the data record](data/moebius-selection-diagnosis-2026-10-10.json). Photo/context/
mask/output artifacts remain under `/tmp/rawpuppy-validation`. The last two runs
overlapped, so their times are not a latency comparison. Broader photography,
seam and alternative-model evaluation remain in the complete project audit.
