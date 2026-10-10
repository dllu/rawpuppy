# Full-strength corner quality and saved sampling provenance

The completion review found implemented modules and direct evidence for the
requested compute, color, large-image and non-destructive workflows, but retained
a concrete synthesis-quality gap: opaque black wedges on an 18-degree GFX
rotation. Reviewed the actual earlier 20-step and 50-step/seed-42 exports. The
earlier improvement changed both step count and seed, so it did not isolate a
useful default change.

Extended the real-model verifier with explicit strength, seed, exposure and
matched base/filled preview export. On the same owned 8736×11648 photo, current
pipeline, graph identities, 18-degree rotation and +0.8 exposure, compared
strength 0.99 with 1.0 at 20 requested steps for seeds zero and 42. All four
real CUDA runs passed save/reload, finite/opaque perimeter and original-hash
checks. Each filled 22,722 missing perimeter samples and preserved 18,046 opaque
samples exactly. These counts include duplicate edge-strip corner points.

Visual review of seed zero showed a large black lower-left wedge at 0.99 and
continued grass at 1.0. In the matched 750×1000 previews, 22,114 of 91,510 original
gap samples had all sRGB channels below 8/255 at 0.99; none did at 1.0. Both
seed-42 results had zero such samples, with full strength also continuing the
grass in the inspected edge region. Every originally opaque preview RGB sample
remained unchanged in all four runs, and all target alpha was opaque. The dark
threshold is a diagnostic for this scene, not a general photographic metric.

The native sampler now defaults to strength 1.0. It starts from diffusion noise
instead of mixing in encoded original-image latents. At 20 requested steps this
uses all 20 leading-DDIM timesteps; 0.99 uses 19. Strength therefore changes the
starting state and schedule together, as defined by the sampler; this comparison
does not isolate either mechanism alone. Warm generation increased only modestly
in these runs, from about 2.94 to 3.08 seconds per context. Structures and details
can still be invented; this is one photo/two seeds, not a universal quality claim.

Generated fills now record actual guidance/strength/noise offset in an optional
sampling field when they differ from legacy values. Shared parameter validation
rejects nonfinite/out-of-range values and schedules with no steps. Older fills
deserialize to 2.0 / 0.99 / 0.0357 and omit the field when serialized, keeping
their historical shape and preceding-recipe hashes. Immutable saved assets remain
renderable without the model. A regression checks legacy omission/hash validity,
new-parameter roundtrip, and invalid parameter rejection.

All 54 default tests, 59 Moebius-build tests, strict Rust 1.99 Clippy for all
targets, formatting and diff checks passed. The updated release verifier ran once
more with default settings and saved strength 1.0 in all four XMP layer records;
the preview again had zero near-black gap samples and zero unselected RGB change.
The combined receipt is `docs/data/corner-strength-gb10-2026-10-09.json`; images,
owned RAW copies and assets remain under `/tmp/rawpuppy-validation/corner-quality-*`.
Original photographs under `~/pictures/raw` were not written. Broader quality and
display/device verification remain open in the full PROMPT.md goal.
