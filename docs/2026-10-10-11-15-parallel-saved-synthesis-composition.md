# Parallel saved synthesis composition

Saved neural layers previously composited serially after GPU rendering and
bilinearly sampled the generated asset before checking paint membership. The
compositor now checks target weight first and uses Rayon over disjoint rows of
each visible layer. Layers still finish sequentially in recipe order. Overlaps
and gap coverage therefore see the same preceding pixels; only independent
pixels within a layer run concurrently.

All visible assets are still validated and loaded before any output changes.
Missing or corrupt later assets leave the whole output untouched. The change
uses the existing output allocation and immutable cached assets, adding no full
composition or rollback raster. Inward blending, premultiplied interpolation,
cropped viewport coordinates and legacy recipes retain their existing behavior.

The new control combines three overlapping layers: a half-opaque painted layer,
a quarter-opaque gap layer and a transparent asset. Signed/HDR RGB and initial
alpha 0, 0.25 and 1 are checked against known RGB/alpha results. Cold assets
after XMP reload produce exactly identical output with **one, two and eight**
workers on **257 × 129** and **100,003 × 7** images. The original and saved recipe
remain unchanged. Existing cropped-feather, gap-opacity and late-asset-failure
checks also pass.

A controlled before/after release run used the same saved 39-dab inward-blended
edit on an owned **8,736 × 11,648 / 101,756,928-pixel GFX100S** RAW. Both processes
used eight Rayon workers, coherent CUDA, joint RAW preparation and an isolated
cache containing RAW graphs but no Moebius model. Sampling was not rerun.

| Stage | Previous serial composition | Parallel, target-first composition |
| --- | ---: | ---: |
| Full render before synthesis | 0.1915 s | 0.1956 s |
| Full render with saved layers | 0.5901 s | 0.2748 s |
| Display P3 TIFF export | 0.8179 s | 0.8096 s |

The observed saved-layer rendering stage fell **53.4%**. That timer includes
full GPU rendering and initial layer validation/loading, so it does not isolate
composition or inference latency. Our builds and tests completed before each
timed probe; other projects share the machine. These are single observations,
not a general latency guarantee.

The complete float RGBA output SHA-256 is identical before and after:
`2efdcf2ff526b53b2b8f6d5104afbe1063f1b382bd998f41f46ca19be637ebac`.
All pixels are finite, **2,037,729** changed samples remain inside paint, every
unpainted sample and alpha stays exact, and source RAW/XMP/sensor hashes are
unchanged. Both exported **814,062,462-byte** TIFFs pass every **407,027,712**
RGBA16 component check across 777 strips, plus Display P3 ICC and straight-alpha
tag checks. This is numerical conformance, not physical display colorimetry.

Peak process RSS is 5,985,284 KiB before and 6,014,964 KiB after, both about
5.7 GiB. These probes hold base and rendered float rasters together for complete
comparison; this is not ordinary editor/export memory. The code adds no whole
photo allocation.

All **91** standard tests, strict all-target Clippy with CUDA/raw-ml/Moebius,
formatting and diff checks passed. The existing
[full-size verifier](../examples/verify_synthesis_export.rs) performs the checks;
measurements, immutable identities and check hashes are retained in
[the data record](data/parallel-synthesis-composition-2026-10-10.json). This
improves full-size saved-edit responsiveness without changing the generated
content or establishing a universal model-quality result. The full PROMPT.md
goal remains active.
