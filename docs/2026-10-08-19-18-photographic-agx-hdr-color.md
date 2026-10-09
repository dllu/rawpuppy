# Photographic AgX and HDR color interpretation

Built independent float exposure/color probes and verified their identity path
through an isolated Darktable CLI. The old polynomial mapped 18% gray to ~21.45%
and differed substantially on saturated HDR colors. Added a versioned photographic
AgX lattice, generated from numeric oracle observations, with Rust tetrahedral
sampling on CPU and inside the shared CUDA/Vulkan/Metal output kernel. The lattice
is cached on the GPU. Existing `agx` recipes retain the old math and appearance.

Recorded executable/preset/data hashes, reproducible offline tooling and 1,677
independent regression samples. Tested 11,308 sweeps/random HDR probes and a larger
129³ candidate. The selected 97³ data uses about 10.4 MiB; fixture errors and the
deliberate negative-gamut policy are documented in [color.md](color.md).

Fixed EXR input color interpretation: declared RGB chromaticities and white point
now determine the transform to linear sRGB, including Bradford white adaptation.
EXR exports now explicitly tag sRGB primaries. Added source color revisions so
saved fills from an older interpretation of a wide-gamut HDR original need
regeneration. Untagged and sRGB-tagged sources keep their earlier interpretation.

New tests cover normalized primary matrices, white adaptation, tagged Rec.2020
HDR/negative input values, output tags, stale HDR fills, photographic AgX oracle
samples, and old recipe compatibility. Existing saved PNG-source fills also
re-rendered without the model or a stale-recipe error. GPU tests now cover modern,
legacy and linear tone modes alongside the composed edits. CUDA parity passed;
warm GFX100S previews at max edge 1800 averaged 13.6 ms on CUDA and 11.8 ms on
Vulkan. Cold render setup took about 0.86 s and 1.89 s respectively. These timings
include no saved synthesis layers, and other workloads may change them. Desktop
CI is checked after pushing the milestone.
All 23 default color/core/layer tests and strict Clippy passed. The complete
offline rebaking command reproduced the committed numeric lattice byte-for-byte
(SHA-256 `3a250c38f332f9659d56dd6a1607c50cfeb21202e389228eb7458a190d28c1f8`).

No contents in `~/pictures/raw` or another project's source/environment were
modified. Synthetic inputs, configurations, scratch libraries and outputs remain
under `/tmp/rawpuppy-validation`; no Darktable source was copied.
