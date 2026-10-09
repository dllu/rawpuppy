# Completed corner jobs and exact opaque interpolation

Corner contexts can overlap on narrow canvases or after scaling. The model mask
also includes unknown padding outside the photograph. Previously that padding
could trigger inference and a new layer even after every target in the actual
canvas had been filled. Generation now checks a canvas-clipped target mask,
augmented by exact perimeter targets, separately from model conditioning. Unknown
padding remains in the model mask. Completed geometric regions return no layer
before model loading/sampling or asset storage. Editor, CLI and verifier handle
that result; the editor reports when no geometric gaps remain. Sampling settings
are still validated before an empty operation can return successfully.

The new 100003×17 overlap regression initially failed: the earlier opaque fill
was resampled with alpha slightly below one and consequently triggered another
generation. A separate 127×113 resampling regression reproduced alpha
`0.99999994` from a fully opaque 2×2 layer. Bilinear weights can sum just below
one in float32. Alpha now divides by their actual sum, while RGB retains the
existing premultiplied normalization. This preserves opaque alpha exactly without
discarding genuinely fractional coverage using an epsilon threshold.

All 13 synthesis tests passed after the correction, including no new asset for a
completed overlapping region, preserved fractional target weights, unchanged
input bytes and rejection of invalid sampling settings. The full default suite
passed 53 tests; the Moebius suite passed 57. Strict Rust 1.99 Clippy for all targets
with Moebius, formatting and diff checks passed.

Extended the real-model verifier with scale and skipped-job recording. On an
owned 100003×17 constant-color PNG at scale 0.99, real CUDA inference generated
two layers and skipped two completed contexts. The skips took about 5.4 and
6.2 ms; all 2,034 missing perimeter samples became opaque, and all 198,006 opaque
samples stayed exactly unchanged after XMP/asset reload. Total time after
planning was 10.45 seconds. These are sample counts with duplicated corners,
and the synthetic fixture is not photographic-quality evidence.

Repeated the real GFX narrow-corner CUDA check with the updated implementation:
all 8,918 missing / 31,850 opaque perimeter samples retained their respective
fill/preservation results after save/reload. Both runs preserved input hashes.
Receipts are `docs/data/overlapping-corners-{wide,gfx}-gb10-2026-10-09.json`; local
artifacts are under `/tmp/rawpuppy-validation/overlapping-corners-2026-10-09`.
No original photograph under `~/pictures/raw` was written.

Observed CI run `38003472663` for the preceding `46729d7` complete successfully
across all six desktop/native-learning jobs. This current correction has local
evidence above and needs its own remote checks after publication. The full
PROMPT.md goal remains active, including broader quality and display verification.
