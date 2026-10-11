# Automatic perspective with semantic selection and strict straightness

Implemented independent native Rust edge verification, homogeneous robust
vanishing-point fitting, vertical/two-axis camera fitting and centered pinhole
composition. Added editor actions, an optional SAM 3 mask toggle, worker-scoped
results, one-step undo, XMP persistence and a JSON-reporting CLI.

The original pinhole controls lacked a translation. The new `geometry.shift`
is folded into the existing homography; zero shifts are omitted from serialized
recipes, preserving historical geometry JSON and generated-layer hashes.
New RAW documents use actual EXIF equivalent focal length, oriented aspect and
lens framing for field of view. Existing recipes keep their stored values.

SAM 3 uses a pinned Hugging Face checkpoint and a verified, separately installed
native TorchScript graph. One image encoder serves building/wall/tree/person
concepts. Whole-edge curvature checks and endpoint continuation independently
reject arches, including short flat pixel runs within curves. Semantic boundaries
are never used as lines. Ceiling-beam consensus cannot masquerade as vertical
architecture when its vanishing point falls inside the photograph.

Selected 13 architectural photographs visually from 83 Darktable sidecars with
active perspective and cooked references. Used owned RAW/sidecar copies under
`/tmp/rawpuppy-validation/perspective-2026-10-10`; original files and cooked
references were read only. Six vertical corrections were accepted and compared
visually with the cooked references: DSCF9471, DSCF9529, DSCF9590, DSCF9618,
DSCF9935 and DSCF0224. Seven cases were declined, including the spiral staircase,
the train-station roof and two interiors dominated by roof beams. This is a
small difficult-case validation, not a universal quality ranking or ground-truth
accuracy benchmark. Some usable façades are still conservatively declined.

The matched native GFX editor run used the glass courtyard with a circular stair:
26 vertical edges, 0.38° fitted residual, 6.8955° pitch and 1.2097° rotation.
Its 11648×8736 sensor remains the source of the preview. Saving, undoing, saving,
redoing and saving restored byte-identical corrected XMP. The owned editor and
X server closed normally. Corner gaps remain transparent and crop is retained.

The eager/prepared SAM graph comparison disabled TF32 and had a maximum mask
probability difference of 4.77e-5, with no threshold disagreements on its three
validation photographs. A native CPU comparison caught device literals in the
initial CUDA trace; the exporter now derives tensor-factory devices from its
input instead of relying on tensor-storage relocation alone.

Validation covers synthetic known camera poses in both orientations, infinite
vanishing points, curve-only abstention, masked evidence, invalid masks,
center preservation, legacy recipe serialization, undo and stale-worker results.
GPU comparisons include nonzero translation with local edits, Bayer/RGB,
orientations, saved neural layers, wide canvases and copied/coherent CUDA memory.
See [setup and behavior](perspective-correction.md) and the committed numeric
[validation report](data/perspective-2026-10-10.json) for the final checks and per-photo outcomes.

The final combined build passed 112 tests, the CPU-only build passed 100, and
strict Clippy passed with CUDA/Moebius/RawNIND/SAM 3 enabled. All eight native
GPU controls passed with the required CUDA compiler stack configuration; the
Vulkan composition control also passed separately. The device-portability
regression passed with both CUDA PyTorch 2.14 and CPU PyTorch 2.13. Both-axis
correction was accepted in two of the 13 real cases; the other directions remain
ambiguous or inconsistent and are conservatively declined.

The portable trained graph also ran natively on CPU and CUDA. Their final
architectural masks disagreed at five of 1,555,200 pixels (0.00032%). The full
CPU/reference comparison took 212 seconds; this model is intended primarily
for GPU-assisted analysis, with the classical detector available on CPU.
Transparent lens gaps and image cutouts are explicitly excluded from the
derivative support, so missing image regions cannot create architectural edges.
