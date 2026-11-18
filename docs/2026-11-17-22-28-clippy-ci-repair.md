# Clippy CI repair

GitHub Actions run `44249278397` stopped before tests on all three platforms.
Strict Clippy flagged an indexed loop in the channel mixer and an approximate
standard constant in the golden-ratio crop preset.

The mixer now iterates over rows and coefficients directly. The preset uses
the standard `f32` golden-ratio constant, which has the same bit pattern as
the previous literal.

Run `44251673200` then reached the tests and exposed two headless egui frames
that discarded texture updates. Debug builds require those updates to be
explicitly cleared when the frame has no renderer. The fine-grid and framing
tool visibility tests now clear the updates before inspecting paint shapes.
