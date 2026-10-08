# Completion audit

The scope remains the whole PROMPT.md. This file distinguishes implementation
from verification; the project is not complete merely because core tests pass.

| Requirement | Current evidence / remaining work |
| --- | --- |
| Rust, cross-platform egui single-photo UI | Native egui editor implemented; X11 GFX editing/save/reload/zoom verified; macOS/Windows CI configured, platform runtime validation pending |
| CUDA / Vulkan / Metal acceleration | Shared Rust compute kernel; CUDA and Vulkan parity, real GFX previews/full exports verified; Metal native runtime verification pending |
| Real RAW decoding including GFX100S | Rawler integration; actual 103 MP sensor decoded; more real files to verify |
| Demosaicing, denoise, hot pixels | MHC and sensor bilateral baseline, tests; quality comparison and ML upgrade research ongoing |
| Chromatic aberration | Composed radial red/blue resampling implemented; real lens validation pending |
| Distortion, perspective, crop, rotation | Composed manual controls and pinhole homography, known-coordinate tests; embedded lens metadata pending |
| Vignetting, graduated ND, exposure, calibration | Composed scalar gain and calibration matrix; tests and real preview; oracle comparisons pending |
| AgX | Analytic approximation with numeric invariants; comparison against oracle pending |
| Clone/heal, curve, split toning | Core and GUI implemented; retouch spatial index; clone and monotone-curve tests; more interaction checks pending |
| Neural synthesis | Model research only; actual inference and saved non-destructive result integration pending |
| Fixed order / minimal rasterization | Explicit order, compiled geometry/color, direct sparse previews, fused GPU output kernel, cached sensor preparation and bounded output tiles |
| Display profiles and X11/Wayland/macOS | Explicit ICC preview transform exists; automatic profile discovery and platform validation pending |
| Output spaces and HDR | Matching ICC integer exports and floating-point EXR; native HDR display integration pending |
| 100 MP and >100,000-pixel width | GFX CPU/CUDA/Vulkan full 102 MP export; warm GPU previews around 11–13 ms; 100,003-pixel CPU/GPU test and viewport equivalence; GPU unified-memory optimization remains |
| No database; non-destructive XMP | Separate namespace and suffix; atomic writes; Darktable sidecar isolation test |
| README / progress journal / commits and push | Documentation present; update journal and push each verified milestone |
