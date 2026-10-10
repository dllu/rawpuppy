# Generated background matching

The modern inpainting comparisons show that exact selection composition can
expose color and brightness seams even after the model removes an object.
Inward feathering softens the boundary but does not match the generated patch's
low-frequency color to its surroundings. This milestone adds a bounded harmonic
correction to **new painted Moebius generation**, before storing its asset.

The independently implemented solver follows the gradient-domain formulation in
[Pérez, Gangnet and Blake's Poisson Image Editing paper](https://www.cs.jhu.edu/~misha/Fall07/Papers/Perez03.pdf).
It averages opaque, unselected neighbors as anchors on the selected inner rim,
then solves a smooth additive linear-RGB correction through the generated
interior. Its guidance comes from the generated pixels. The original selected
RGB is never used for matching: it can contain the removed object or missing
content. The first prototype used selected rim colors and failed a withheld
texture control by pulling black into the repair; that implementation was
discarded before application integration.

The conjugate-gradient solve uses f64 and a relative residual threshold of
1e-7. Application version one is unscreened, with a 2,048-iteration guard.
Allocation is fallible, neighbor stencils are fixed-size and iteration scratch
is reused. Work stays inside the existing 512-square model context; it creates
no full-photo correction raster. An unsuccessful solve reports an error before
storing any asset. Contexts without opaque unselected anchors retain their
generated colors. Geometric corner generation keeps its existing behavior.

Corrected values are baked into the content-addressed EXR, and XMP records
`harmonization: boundary_poisson_v1`. Existing layers omit this field and retain
their assets, historical recipe hashes and displayed pixels. Loading and rendering
saved layers require neither the solver nor model. The context benchmark applies
the recorded method when reproducing a new asset. Inward feathering remains a
separate final composition weight, with exact unpainted samples and generated
asset core.

On the existing **96×96 withheld fabric patch at actual GFX pixels**, only the
blackened input, generated prediction and mask are passed to matching. The intact
patch is used separately for assessment. Compared on 8-bit sRGB:

| Saved Moebius prediction | Selected RGB mean absolute error | Selected RGB RMS error |
| --- | ---: | ---: |
| Unmatched | 4.6341 | 5.5564 |
| Harmonic correction | 2.7305 | 3.8551 |
| Screened correction, coefficient 1/256 | 3.2021 | 4.2662 |

Harmonic correction lowers mean error **41.1%** on this patch. Inspection shows
a less visible square edge and retains its soft diagonal texture. Every
unselected sample remains exact. This is one withheld reconstruction assessment,
not a universal perceptual or model ranking. The screened path is retained in
the opt-in comparator; application generation uses the unscreened version.

The recorded complete-selection person-removal context also has a less visible
tone mismatch after matching, while fence alignment and generated detail remain
imperfect. The corrected prototype solved that context in about 0.131 seconds
and the texture context in 0.014 seconds on the shared GB10 host. These are bounded
CPU-solve observations, not model sampling times or general latency guarantees.

A fresh **native Rust/CUDA** generation uses the previous saved 39-dab selection
without changing its radius or sampling settings. It records the method, preserves
all **214,091** unselected context samples, keeps alpha exact and cold-reloads
every sample exactly. The newly matched asset is
`a147f7a1129c4192b14a1518b6a177d4daa3a5a331ae37a5ede1927b22094726.exr`.
Repeating the recorded sampling plus matching reproduces **every 512×512 RGBA
sample exactly**; the canonical pixel-buffer hash is
`afc96dbd7f40f3bb35077b44dd48447d0e555452d1bb3712cc6e48420c5a7b8a`.
Different EXR compression order can still change file hashes.

The new saved layer also passes full **8,736×11,648 / 101,756,928-pixel** joint
RAW/coherent CUDA rendering with the Moebius cache absent. All samples are finite,
**2,037,444** changed pixels stay inside paint, every unpainted sample and alpha
is exact, and RAW/XMP/sensor hashes are unchanged. Every **407,027,712** component
of the **814,062,462-byte** Display P3 RGBA16 TIFF is decoded and verified, plus
ICC and straight-alpha tags. Saved-layer rendering took 0.272 seconds after RAW
preparation; this validates the persisted result without rerunning matching.

Analytic tests verify a known uniform color shift, retained generated fine detail,
exact outside samples, identical output when all selected source colors change,
unanchored-context fallback and atomic rejection of invalid/unfinished requests.
Legacy omission, method serialization, recipe identity, XMP reload, ordered
composition and crop/gap tests pass. All **94 default** and **102 combined-feature**
tests, strict all-target CUDA/raw-ml/Moebius Clippy, formatting and diff checks
passed. The current native release also builds. The preceding commit passed all
six desktop/native-learning CI jobs.

Use the [comparator](../examples/benchmark_synthesis_harmonization.rs) with a
saved context, prediction and target mask to compare screened settings without
rerunning a network. Native generation/reproduction use the existing
[painted-layer probe](../examples/verify_painted_blend.rs) and
[context benchmark](../examples/benchmark_moebius.rs). Measurements, checkpoint/
asset/source identities and check receipts are in
[the data record](data/synthesis-background-match-2026-10-10.json).
Images and weights remain outside the repository under `/tmp/rawpuppy-validation`.
This improves the recorded tone seams; it does not repair arbitrary invented
structure or prove the whole PROMPT.md goal complete.
