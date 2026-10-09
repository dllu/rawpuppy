# 2026-10-09 07:44 — Live HDR reference white

HDR photo pixels now remain in relative-white units in their uploaded texture.
A shader uniform applies the current display signal scale at draw time. GUI and
photo white therefore update together when reported brightness/monitor values
change, without rebuilding the photo preview. HDR preview requests no longer
capture a stale display-white value. Invalid scale values are rejected before
updating the uniform; draw output stays within the float target's finite range.

The GPU test reuses one texture through 1×, 2×, 0.5× and restored 1× white, checking
signed/superwhite RGB and linear alpha against a fixed background. The GUI/photo
white test now exercises shader scaling too. Actual managed Wayland SDR/HDR
editor smoke runs pass with the same signed/4×-white fixture, unchanged input/XMP
hashes and normal close. Native frame extrema remain −0.317 and 10.148 in the
tested 203/80 signal convention.

Validation: 41 default tests, both actual GPU HDR checks, strict Rust 1.99 Clippy,
formatting and diff checks passed. The separate Linux CI dependency correction
passed all four jobs in run 37945480744, including native editor tests on Linux,
macOS and Windows. Physical brightness transitions and surface capability changes
still need broader verification. The full PROMPT.md goal remains active.
