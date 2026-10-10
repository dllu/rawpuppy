# Complete prompt implementation audit

Audited the full original PROMPT.md against current source at `dc26440`, current
native artifacts, recorded numeric controls and the completed required CI matrix.
The latest production change is `172cb7f`; later commits record native verification.
All **six jobs** in [run 38082697336](https://github.com/dllu/rawpuppy/actions/runs/38082697336)
are successful: desktop Linux/macOS/Windows and native-learning Linux/macOS/Windows.
The required native Metal step, macOS MPS inference, Windows native inference and
actual native editor signal probes are successful. Local checks cover coherent
and copied CUDA on the GB10.

This audit retains all nineteen explicit requirement groups. Its completion claim
is implementation and verified operation within the requested scope; measured
tradeoffs and unsupported extra formats remain documented. It does not claim a
universally best neural model, every camera's physical accuracy or colorimeter
measurements across every monitor.

| Original requirement group | Inspected current implementation and proving evidence |
| --- | --- |
| Rust, cross-platform, minimal single-photo egui | Rust/Cargo/native window and worker source; current 101.8 MP editor open, clear/undo/redo/save/cold-reopen; exact displayed samples; native Wayland/macOS/Windows signals and desktop CI. |
| CUDA Rust, Vulkan, Metal | Shared CubeCL Rust kernel; actual copied/coherent CUDA and Vulkan execution; mandatory native Metal parity including fused synthesis; all six current implementation CI jobs. |
| RAW decoding and GFX100S | Rawler, immutable normalized sensor/crop/orientation data; actual 103 MP sensor and 101.8 MP active photograph, eight further GFX captures across named lenses/orientations, Canon/Sony captures and 100,003-wide DNG. |
| Demosaicing, denoise, hot pixels | Independent MHC/same-color cleanup and optional joint RawNIND; analytic Bayer constants, x=70,000 hot-pixel correction, real native learned graphs, full camera-RGB cache, cancellation and photometry controls. |
| Chromatic aberration | Manual and embedded red/blue radial sampling before calibration; known channel coordinates, amplified GF55 metadata/oracle fit and CPU/shared-kernel checks. |
| Embedded/manual distortion, perspective, crop, rotation | One backward pinhole/homography/radial map with once-computed trigonometry; known coordinates/orientations, channel-safe framing and independent optical-direction controls. |
| Vignetting, graduated ND, exposure, one calibration | Composed spatial scalar and camera/user matrix; lens ratios, DNG profiles/white/analog/spectral/forward controls, unbounded exposure and all 72 million signed oracle photosite components. |
| AgX and independent Darktable oracle | Numeric observations with provenance, independent Rust lattice interpolation and 1,677 goldens; gray/gamut/legacy and GPU parity. Darktable source is not copied. |
| Clone/heal, curve, split toning | Native controls, fixed display order, spatially indexed retouch and monotone curve; pointer/gesture/crop/undo tests and GPU/HDR regressions. |
| Recent licensed neural removal/gap filling | Native June 2026 Moebius, actual complete-selection removal and rotated/narrow corner generation, versioned background matching and content-addressed float assets; pinned Qwen 2.1/adapter, FLUX.2 klein and September LLaDA evaluations with licenses, quality/latency/memory records. |
| Opinionated composition/order, original sparse previews, GPU final pass | MODULE_ORDER and executable CPU/kernel order; composed coordinates/scalars/matrices/LUTs, cached neighborhood/joint preparation; final saved-layer fusion with eight bindings, exact row membership and bounded output tiles. |
| Classical/OIDN and superior joint ML research | Published methods and 2026 release/license alternatives reviewed; attributed CC BY 4.0 weights and independently expressed RawNIND graph. PSNR and high-pass correlation improve over fixed MHC/bilateral on all six preselected held-out detail regions; edge/detail tradeoffs are explicit. |
| Working/input/output/display color on X11/Wayland/macOS | Linear-sRGB D65 basis, Little CMS, declared matrices/transfer functions and five tagged output spaces; signed/HDR/gray controls; live X11 ICC, managed/legacy Wayland surface policy, macOS ColorSync layer tagging and Windows discovery. |
| HDR images and display consideration | Float import/export and explicit chromaticities; advertised format/color-space pair, float photo/UI/reference-white/alpha renderer, real native SDR/HDR signals and fallback. |
| 100 MP and >100,000 width, format/memory limits | Checked usize indices/products and disabled application decoder budgets; actual full GFX and 100,003-wide RAW/EXR output, bounded conversion scratch, >4 GiB BigTIFF with independent LibTIFF offsets. GPU limits retain CPU fallback, and JPEG limits are its format constraint. |
| GB10 unified-memory best practices | Capability-selected synchronized owned-system pointers, local page advice, retained source/assets and direct single-asset borrowing; Nsight removes application transfers, full-size coherent rendering and warm-preview observations. |
| Original-folder/resource/process protection | Read-only original use and owned validation copies; default eight workers, bounded contexts/jobs/tiles, real allocation-pressure recovery, original/alias export rejection and explicit owned-child cleanup. |
| No gallery/global database, non-destructive XMP, CLI | Single document; atomic independent namespace and full-filename `.rawpuppy.xmp`, asset identities and save/load; durable write-failure/no-clobber/undo/cold-render checks. |
| README, progress journal, commit/push delivery | Concise current build/run README, linked setup, timestamped concrete journals and hashed records; milestone commits pushed to main; final audit delivery committed and pushed. |

The [machine-readable audit](data/prompt-completion-audit-2026-10-10.json) pins
each inspected implementation/evidence file by its current hash and records the
successful CI jobs and steps. Files are evidence pointers, not proof by existence:
the table identifies the executed/numeric/native behavior that supports each group.
The latest production checks include **96 default**, **104 combined-feature**,
actual accelerator/HDR tests, strict combined all-target Clippy and no-GPU builds.

Recorded limitations retain their actual scope. Neural methods can smooth fine
texture or invent structure; quality measurements are bounded and model setup is
optional/documented. Color/frame/protocol checks establish software conformance,
not physical monitor measurement. Additional sensor arrangements, nonlinear
profiles and future hardware coverage remain extension/validation work rather than
claims already proved. Performance figures are observations on the shared host.
The requested implementation and its required delivery/gates are complete; those
limits are retained rather than represented as universal guarantees.
