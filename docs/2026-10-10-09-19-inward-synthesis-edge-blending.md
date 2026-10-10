# Inward synthesis edge blending

Recent Moebius and LLaDA comparisons both exposed hard target boundaries. Their
full edited images can look plausible while exact target-only composition creates
visible seams. Wider model conditioning does not justify changing unpainted pixels.
A bounded linear-RGB prototype showed that inward transitions soften Moebius's
edge, while LLaDA's shifted fence/background features remain wrong.

New painted fills now record `feather: 0.15`, expressed as a fraction of each
brush radius. The existing sparse compositor evaluates a smoothstep transition
inside the painted circles and takes the maximum across overlapping dabs. The
generated core retains its original samples; every unpainted pixel is unchanged.
The operation runs in display-linear working RGB before display/output conversion,
with no extra whole-image raster or repeated network inference. The coverage
resolver uses the same weights.

Geometric gaps always have weight one, independently of brush feather, so this
does not create partially transparent filled corners. Historical layers default
to zero, omit that field when serialized, and retain their existing hard-mask
behavior and recipe/asset identities. New metadata rejects nonfinite, negative
or out-of-range feather values. Existing sampling parameters and model defaults
are unchanged; user paint is never enlarged automatically.

Regression tests cover legacy omission/validation, an independently known boundary
weight (11/256), untouched signed/HDR samples outside paint, unchanged generated
core, exact full/cropped viewport correspondence, saved XMP/cold asset reload and
complete filling of transparent/partly transparent geometry. All **89 default**
and **97 combined-feature** tests, six DNG calibration/file tests, strict
CUDA/raw-ml/moebius all-target Clippy, the no-GPU all-target build, formatting and
diff checks passed.

The [real-model probe](../examples/verify_painted_blend.rs) copies the owned GFX
source and uses its prior saved context/settings. It explicitly doubles the
diagnostic brush radius to cover the complete object, generates one fresh native
Moebius asset and compares hard/feathered composition of that **same asset**.
The 512-square context represents the 101.8 MP document; it is not a new full-size
photograph raster.

On that context, **214,091 unselected pixels** and **39,029 selected-core pixels**
remain exact. Only **9,021 inward transition pixels** differ from hard composition.
Alpha is exact and cold saved-layer reload reproduces every sample. Maximum
added RGB contribution across a selected/unselected neighboring pair falls from
**0.06417045 to 0.00107813**, a **98.3%** reduction. This measures the introduced
boundary step, not universal visual quality. Matched images show a softer edge,
but generated background detail/structure mismatches still remain. Original RAW
and original sidecar hashes are unchanged.

```sh
cargo build --release --features cuda,raw-ml,moebius --example verify_painted_blend --locked
target/release/examples/verify_painted_blend /path/to/owned-photo.raf /tmp/new-blend-control
```

Measurements, source/asset/recipe identities and test-log hashes are in
[the data record](data/inward-synthesis-blend-2026-10-10.json). Photos, snapshots
and model outputs remain under `/tmp/rawpuppy-validation`. Broader synthesis,
camera and physical display quality remain in the full project audit.
