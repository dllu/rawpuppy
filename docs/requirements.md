# Completion audit

The scope remains the whole PROMPT.md. This file distinguishes implementation
from verification; the project is not complete merely because core tests pass.

| Requirement | Current evidence / remaining work |
| --- | --- |
| Rust, cross-platform egui single-photo UI | Rust core exists; GUI and platform runtime validation pending |
| CUDA / Vulkan / Metal acceleration | Pending composed compute kernels and backend parity/performance checks |
| Real RAW decoding including GFX100S | Rawler integration; actual 103 MP sensor decoded; more real files to verify |
| Demosaicing, denoise, hot pixels | MHC and sensor bilateral baseline, tests; quality comparison and ML upgrade research ongoing |
| Chromatic aberration | Composed radial red/blue resampling implemented; real lens validation pending |
| Distortion, perspective, crop, rotation | Composed manual controls and pinhole homography, known-coordinate tests; embedded lens metadata pending |
| Vignetting, graduated ND, exposure, calibration | Composed scalar gain and calibration matrix; tests and real preview; oracle comparisons pending |
| AgX | Analytic approximation with numeric invariants; comparison against oracle pending |
| Clone/heal, curve, split toning | Core implementation; clone and monotone-curve tests; GUI interaction pending |
| Neural synthesis | Model research only; actual inference and saved non-destructive result integration pending |
| Fixed order / minimal rasterization | Explicit order, compiled geometry/color, direct sparse previews; GPU fusion pending |
| Display profiles and X11/Wayland/macOS | Explicit ICC preview transform exists; automatic profile discovery and platform validation pending |
| Output spaces and HDR | Matching ICC integer exports and floating-point EXR; native HDR display integration pending |
| 100 MP and >100,000-pixel width | Real decode/preview and 100,003-pixel render test; full export and memory/performance measurements pending |
| No database; non-destructive XMP | Separate namespace and suffix; atomic writes; Darktable sidecar isolation test |
| README / progress journal / commits and push | Documentation present; update journal and push each verified milestone |
