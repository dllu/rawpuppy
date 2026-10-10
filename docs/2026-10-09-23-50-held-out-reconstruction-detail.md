# Held-out reconstruction detail checks

Extended the earlier one-ROI-per-camera noise comparison with three preselected
512-square ROIs on each held-out Canon/Sony scene. Their centers are the 25%, 50%
and 75% diagonal sensor positions, rounded/aligned to RGGB. Bilateral sigmas
0.03/0.1 were fixed before the new results; MHC and the same pinned RawNIND graph
provide the other methods. All six use the same source/context, shared observed-CFA
exposure correction, reference-only registration/resampling and saturation mask.
Original downloads remain byte-identical. Nothing in `~/pictures/raw` was used.

The comparator now reports detail diagnostics. A seven-pixel valid-mask erosion
keeps derivative support inside the valid interior. Clean-reference green Sobel
magnitude selects pixels at or above its 75th percentile, independently of each
method. Gradient RMSE compares both directions there. Green minus a 7×7 Gaussian
sigma-one image provides a high-pass Pearson correlation on the eroded interior.
The reference's MHC reconstruction, interpolation and residual noise remain
limitations, and these diagnostics do not separate demosaicing from denoising.

All six passed the established registration and integrity gates. RawNIND PSNR
exceeds the better fixed bilateral result by 0.46–0.85 dB on Canon and 3.14–6.67 dB
on Sony. High-pass correlation is higher on every ROI. Reference-edge error is
slightly worse on Canon (1.6–4.4%) and better on Sony (7.1–33.5%). These mixed
results support the measured noise reduction without declaring universal detail
superiority. Selection remains provisional.

Inspected standalone figures use the central 224 sensor pixels of every ROI,
shared exposure, aligned clean reference, and identical gamma/limits within each
row. Sony main patterns and numerals remain recognizable with much lower noise;
fine surface speckle is smoothed. Canon marble and carving remain visible, with
some fine surface variation softened. Figures and patches remain in the owned
`/tmp/rawpuppy-validation/reconstruction-detail-2026-10-09` directory; only numeric
records and source/model identities are published.

The record is `docs/data/raw-reconstruction-detail-2026-10-09.json`. Prepared graph
identity is constant across all six. Actual release fixture extraction, real
native CPU model inference and extended comparisons passed; Python compilation
and diff checks passed. No Rust production behavior changed, so the completed
Rust test suites were not repeated solely for these reporting additions.
