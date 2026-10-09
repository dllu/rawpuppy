# 2026-10-09 03:23 — Cached joint reconstruction in the editor

Connected the optional learned Bayer reconstruction to the shared editor, export
and synthesis renderer. Standard reconstruction remains the default; Joint AI is
experimental because wider camera/detail/HDR quality validation is still pending.
The full PROMPT.md goal remains active.

## Implementation

- Added a versioned recipe method, omitted for Standard so old JSON serialization
  and saved synthesis hashes remain unchanged. Direct pipeline use rejects a
  neural recipe without its corresponding prepared source.
- Reconstruct 1024-pixel tiles with sensor-anchored pooling and reflected active
  photograph context. Keep unscaled tile predictions until one image-wide gain
  matches observed CFA photometry. Reflect valid RGB into physical margins for
  continuous sampling at image boundaries. Original RAW allocations stay immutable.
- Retain one prepared camera-RGB source keyed by original identity and hot pixel
  correction. Global color/geometry/tone edits reuse it in the existing fused
  renderer; hot pixel cleanup occurs before learned inference. The joint method
  replaces standard denoise/demosaic rather than adding a second denoiser.
- Added verified model installation into the local cache, CLI export selection,
  and the experimental editor choice. Model attribution is retained. Model setup
  remains separate from photograph directories.
- Routed synthesis dimensions and context rendering through the same prepared
  source, preventing accidental Standard reconstruction of a saved neural recipe.

## Measurements and exercised behavior

On the GFX100S 101.8 MP photograph, the first controlled reconstruction/preview
took **16.57 s**; three cached 1800-pixel exposure previews averaged **3.24 ms**.
The full 8736×11648 corrected RGBA16 TIFF exported with **17.90 s** for preparation
and rendering and **0.89 s** for encoding. Alpha is 65535 throughout. The actual
knit crop was inspected against the Standard export: mild smoothing is visible,
with no obvious tile boundary. This is one scene, not an overall quality proof.

An isolated X11 editor selected Joint AI, reused the cache for an exposure change,
saved `raw_nind_v1` with exposure 1 EV, closed normally and reloaded the same saved
choice. Initial GUI previews were approximately 12.6 and 13.8 seconds in those
runs. Both owned editor processes exited; the owned Xvfb was stopped only after
checking its command and logfile ownership. The real desktop was untouched.

One Moebius layer was generated on a held-out Canon image using the learned base,
saved and re-exported after reload. The decoded uint16 RGBA arrays are identical.
The PNG container hashes differ, so this is explicitly a pixel-equality claim.

## Verification and remaining work

The 33 default tests and 35 native optional tests passed. Four actual-model tests
passed, covering phase/photometry/source identity, overlapping contexts, tiled
versus single-context reconstruction with odd crop/bright overscan, and renderer
composition plus release of retired originals. Strict Clippy passed for default
and CUDA/native-learning targets; formatting and whitespace checks passed.
The preceding native-pilot commit's Linux, macOS, Windows and native-learning CI
jobs were verified successful.

The prepared image currently uses a single ~1.24 GB RGB allocation for this sensor,
plus the original sensor. Eligible coherent CUDA shares it with the fused renderer.
No processed-image database or disk cache is added. The first preparation pass
occupies the photo worker; UI controls respond, while preview requests queue until
preparation finishes. Progressive/cancellable work, more memory-pressure coverage,
broader camera/detail/illuminant quality and native MPS/HDR validation remain.

All test photos, sidecars, layers and exports are under `/tmp/rawpuppy-validation`;
`~/pictures/raw` was not modified. Measurements, hashes and scoped claims are in
[the integration data record](data/raw-reconstruction-integration-2026-10-09.json).
[Learned reconstruction](learned-reconstruction.md) describes setup and operation.
