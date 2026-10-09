# 2026-10-09 05:55 — Embedded camera chromatic aberration

Camera red/blue curves now participate in the same backward map as distortion,
perspective, crop, orientation and manual fringe scales. The shared CPU/GPU
lookup has four components: green map, scene gain, red map and blue map. Camera
RGB channels are aligned before calibration mixes them. Automatic camera framing
protects every channel, including CA-only profiles. No extra photo raster or
storage buffer is introduced.

The tiny original GF55 coefficients were difficult to distinguish from resampling.
Regular temporary RAF copies amplify only their 18 CA numerators by +40, −40 or
zero, preserving sensor data and all other bytes. UI-derived CA-only oracle exports
and independent feature fitting favor the direct fractional scale `1 + coefficient`
over percent units or the opposite sign. Camera-basis median residuals are
0.037–0.050 pixels for the direct model, versus 0.190–0.284 for percent units.
This establishes units/direction on one lens, not identical rendering or universal
optical accuracy. No Darktable source was used.

New documents enable available CA curves. Historical embedded recipes keep CA off,
and false is omitted from serialization so their saved-fill hashes remain stable.
The native GFX editor enabled CA, saved on/off, reloaded off and closed normally.
The 101.8 MP sRGB TIFF is fully opaque, with a valid ICC and unchanged input hash.
CUDA system-memory rendering took 0.57 seconds and encoding 0.87 seconds. Warm
1,800-pixel previews averaged 10.99 ms with CA versus 4.81 ms without it.

Validation: 38 default tests, three actual CUDA tests, actual Vulkan parity,
strict Rust 1.99 Clippy over all CUDA targets, formatting and diff checks passed.
GPU parity includes CA-only/no-manual-fringe cases, combined corrections and all
eight orientations for RGB and Bayer inputs. Native Metal CA coverage awaits CI.
All photographs and generated validation artifacts stay outside the repository.
The full PROMPT.md goal remains active.

Follow-up: [CI run 37933420194](https://github.com/dllu/rawpuppy/actions/runs/37933420194)
passed Linux, Windows, macOS and native learning. The native Metal parity step
itself passed, including the CA-only and combined-camera cases.
