# Full prompt audit and required Metal gate

The previous milestone verified every full-size saved-synthesis pixel and every
Display P3 TIFF component. This audit returns to the complete PROMPT.md instead
of equating that successful workflow with every requested property. Current
source, tests, recipes, native artifacts and CI job/step states were inspected.
The worktree was clean at implementation commit
`95d5c1f49f87fa2d7cd2b0f76a29a9d167670448`.

One concrete gate contradicted the prompt's requirement strength: Metal compute
parity still used `continue-on-error: true`. The latest native macOS step actually
passed, but a future failure could still produce a green job. The override is now
removed; Metal parity is mandatory alongside Linux Vulkan parity. The new gate
still needs its own completed CI run before completion can be asserted.

| Explicit prompt group | Inspected implementation | Authoritative evidence and scope |
| --- | --- | --- |
| Rust, native, cross-platform, minimal single-photo egui | Cargo/Rust source; dark native editor; worker, undo/save/open/export and brush UI | Actual X11 GFX and Wayland editor workflows; native macOS/Windows signal probes and desktop builds. This does not prove every possible interaction or physical display. |
| CUDA in Rust, Vulkan, Metal | Shared CubeCL Rust kernel, persistent sessions, coherent CUDA driver adapter | Local copied/coherent CUDA and Vulkan comparisons; native Metal CI step passed. Metal is now a required gate. |
| RAW decoding, GFX100S | Rawler, immutable normalized sensor data, CFA/crop/orientation handling | Actual 103 MP GFX sensor, eight more GFX copies/lenses/orientations, Canon CR2/Sony ARW and file-level wide DNG checks. Other formats remain outside those observations. |
| Demosaicing, denoising, hot pixels | MHC, sensor bilateral/median cleanup, optional joint RawNIND | Bayer-layout/golden constants and real held-out noise/detail comparisons; actual x=70,000 hot pixel; native model tests and full preparation. Detail gains/losses are documented. |
| Chromatic aberration | Manual red/blue mapping and embedded RAF curves | Analytic coordinate checks and independent amplified GF55 metadata/oracle fit support units/sign. This is not every lens. |
| Distortion, perspective, crop, rotation | One crop/pinhole/radial backward map; trigonometry/matrices compiled when edits change | Known coordinate/orientation tests, embedded/manual framing, independent optical-direction fit and CPU/GPU comparisons. |
| Vignetting, graduated ND, exposure, calibration | Composed spatial scalar and one camera/user calibration matrix | Independent GFX scalar ratios, DNG constant/noise controls, standard/custom/three-slot/xy profiles, analytic and CPU/GPU checks. High-ISO and physical-camera residuals are retained. |
| AgX and Darktable oracle without source copying | Versioned photographic lattice from numeric observations, Rust interpolation, legacy transform | 1,677 goldens; gray/source/gamut tests and CPU/GPU parity; offline CLI oracle script/provenance. Geometry and shader code were inspected as independently expressed logic. |
| Clone/heal, curve, split toning | Fixed display-stage operations in CPU/shared kernel, native controls | Known clone/curve checks, captured/batched brush and crop/undo tests, composed backend parity. |
| Neural gaps/removal and recent-model research | Native Moebius sampler, saved float assets, exact membership/inward composition | Real narrow/overlapping corner fills; exact native removal reproduction and complete-selection diagnosis; saved/cold/full-size rendering. Pinned Qwen 2.1, adapter, FLUX.2 klein and September LLaDA comparisons record licenses, memory, timing and failures. No universal ranking. |
| Opinionated ordering and one purpose per module | MODULE_ORDER and actual sample/kernel bodies; single calibration path | Normalization → sensor cleanup/reconstruction → composed geometry → scalar/calibration → AgX → retouch → curve/split → saved synthesis → output transform. No separate competing white-balance module. |
| Composition, sparse original previews, fused output | Compiled homography/scalar/matrices/LUTs, persistent source, strict bounded flat output tiles | 64-pixel/whole-tile identity, very wide rows, 1:1 viewport tests, source immutability and measured warm GFX previews. Sensor-neighborhood/joint preparation is cached separately. |
| Research classical/OIDN/joint ML methods | Research documents, independently expressed attributed RawNIND graph and native cache | Publisher/operator oracle checks, Bayer phases, camera-linear photometry and held-out quality controls; later methods were reviewed for released artifacts and licensing. Final superiority across all captures is not inferred. |
| Working/input/output/display color, X11/Wayland/macOS | Little CMS, normalized primary/white matrices; platform discovery/surface policy | All five output-space ICC tests, tagged float HDR/gray controls, live X11 updates, managed/legacy Wayland protocol, native layer/window checks. Physical colorimetry is separate. |
| HDR images/displays | Float import/export; advertised format/color-space pair, relative-white photo/UI shader | Actual negative/superwhite native float frames, GUI/photo white and linear alpha; SDR fallback; platform probes. Advisory HDR support does not imply physical luminance accuracy. |
| 100 MP, >100000 px, format/memory limits | `usize` checked products/indices; decoder budgets removed; TIFF/PNG scratch, BigTIFF, direct EXR input/assets | Actual wide RAW and full GFX workflows, >4 GiB TIFF with independent LibTIFF offsets, large EXR/native import and all-pixel export checks. JPEG's 65535 limit is its format constraint; GPU source limits use CPU fallback rather than limiting core representation. |
| GB10 unified memory and resource cooperation | Capability-selected owned system pointers, synchronized borrows, local page hints; bounded model contexts/worker count | Nsight records no application memory-copy events on the coherent path; warm copied/system comparison; recoverable owned CUDA allocator/address-space controls. No global setting or other-project process changes. |
| Immutable originals, no database, XMP isolation and optional CLI | Single document, own suffix/namespace, atomic stage/sync/rename, export alias protection | Source/recipe/sensor hashes, sidecar independence, original/dot/symlink alias tests and actual failed-write publication tests. Model/asset caches are file-based. |
| Concise README, journal, commit/push milestones | README build/run/editor/features; evidence journals; Git state | Current README commands/artifacts were checked; implementation milestones are committed/pushed. Detailed records stay in docs rather than README. |

The [machine-readable mapping](data/prompt-audit-2026-10-10.json) pins the inspected
source/evidence files by hash and keeps all nineteen groups. It does not turn a
file's existence into proof; the table identifies what its executable/numeric/
native evidence actually covers.

Completion is still unproven at this milestone. The new mandatory Metal gate has
not completed on its new commit, model/camera quality has measured limits, and
physical colorimetry is not inferred from software/runtime checks. The full goal
remains active. Research breadth, missing camera/nonlinear profile coverage and
visible model detail/structure failures remain in the existing requirements audit;
they are not hidden by a single successful output or a green default test suite.
