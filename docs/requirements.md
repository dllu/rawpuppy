# Completion audit

The scope remains the whole PROMPT.md. This file distinguishes implementation
from verification; the project is not complete merely because core tests pass.

| Requirement | Current evidence / remaining work |
| --- | --- |
| Rust, cross-platform egui single-photo UI | Native egui editor implemented; X11 GFX editing/save/reload/zoom and isolated Wayland synthesis/save/undo verified; desktop CI passes; macOS/Windows GUI runtime validation pending |
| CUDA / Vulkan / Metal acceleration | Shared Rust compute kernel; CUDA and Vulkan parity, real GFX previews/full exports verified; Metal native runtime verification pending |
| Real RAW decoding including GFX100S | Rawler integration; actual 103 MP sensor decoded; more real files to verify |
| Demosaicing, denoise, hot pixels | MHC and sensor bilateral baseline, tests; quality comparison and ML upgrade research ongoing |
| Chromatic aberration | Composed radial red/blue resampling implemented; real lens validation pending |
| Distortion, perspective, crop, rotation | Composed manual controls and pinhole homography, known-coordinate tests; embedded lens metadata pending |
| Vignetting, graduated ND, exposure, calibration | Composed scalar gain and calibration matrix; numeric tests and real preview; one GFX100S linear oracle comparison has mean RGB ratios within 0.04%; additional cameras/illuminants and controls pending |
| AgX | Versioned photographic lattice from independent oracle observations; 1,677 golden samples, gray preservation and legacy recipe compatibility; CPU/CUDA/Vulkan parity; 1800-pixel GFX previews ~12–14 ms; desktop CI being checked |
| Clone/heal, curve, split toning | Core and GUI implemented; retouch spatial index; clone and monotone-curve tests; more interaction checks pending |
| Neural synthesis | Native Rust Moebius sampler/graphs, GUI masks/corners/regeneration, EXR assets and XMP identities implemented; native CUDA/CPU, X11/Wayland workflows, reload and opaque corner composition verified; model/platform/quality comparisons pending |
| Fixed order / minimal rasterization | Explicit order, compiled geometry/color, direct sparse previews, fused GPU output kernel, cached sensor preparation and bounded output tiles |
| Display profiles and X11/Wayland/macOS | Explicit ICC preview transform and X11/Wayland presentation verified; automatic profile discovery, additional compositors and native macOS validation pending |
| Output spaces and HDR | Matching ICC integer exports; float EXR imports respect chromaticities/white points and exports tag primaries; HDR/negative and adaptation tests; native HDR display integration pending |
| 100 MP and >100,000-pixel width | GFX CPU/CUDA/Vulkan full 102 MP export; warm GPU previews around 11–13 ms; 100,003-pixel CPU/GPU test and viewport equivalence; GPU unified-memory optimization remains |
| No database; non-destructive XMP | Separate namespace and suffix; atomic writes; Darktable sidecar isolation test |
| README / progress journal / commits and push | Documentation present; update journal and push each verified milestone |
