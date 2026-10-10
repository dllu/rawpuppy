# Sensor highlight recovery

New integer RAW documents enable **Recover clipped highlights** under Sensor &
detail. Existing sidecars keep their previous off behavior unless enabled. Its
false field is omitted from serialization, preserving old recipe hashes and
generated layers. Float RAW, developed RGB and HDR imports preserve their values.
Changing recovery changes the preceding recipe and requires regeneration of
dependent synthesized layers.

Recovery runs in camera RGB after demosaicing/channel geometry and before camera
calibration, scalar scene gains and AgX. Near-ceiling sensor samples in [0.98, 1]
identify affected channels. Values above nominal white remain intact. Mask coverage
interpolates continuously through the sampling footprint, with separate red/blue
maps for CA. It uses sensor-cleanup values in Standard reconstruction; learned
reconstruction retains the original flags after hot-pixel cleanup.

Intact channels remain exact. The remaining white-balanced channels estimate
clipped intensity; with no intact channel, a neutral estimate uses the largest
remaining lower bound. Reconstruction only increases affected channel values,
blending smoothly near the ceiling and into the fully clipped estimate. This is
a neutral estimate, not restoration of lost color or texture. Difficult colored
lights and camera-specific saturation points need broader validation; disable it
when its assumption is inappropriate.

The original sensor stays immutable. Learned RGB appends four three-bit masks
inside each normal float's mantissa, using about one extra byte per pixel. The
RGB prefix keeps its exact model/photometry values; the tail receives no gain.
CPU and CUDA/Vulkan/Metal use the same layout, without an additional GPU storage
binding or duplicated sensor allocation. Tests exercise actual prepared RawNIND
graphs, signed/above-white values, original clipping provenance and CPU/GPU parity.

On two inspected GFX examples, recovery removed lavender from the selected bright
window and pavement areas while preserving inspected shadow samples. A wide
preview sample changed from sRGB (230,217,248) to (242,242,242); a pavement sample's
red/blue mean minus green fell from 8.5 codes to −0.5. These are scene diagnostics,
not a general color-quality score. Both real-camera perimeter and composed-preview
checks passed; [the record](data/highlight-recovery-gfx-2026-10-09.json) retains
settings, hashes, timings and samples. Images remain outside the repository.
