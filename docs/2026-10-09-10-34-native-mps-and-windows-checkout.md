# Native MPS verification and Windows checkout correction

Observed CI run `37965948083` for `cdd58ee`. Standard editor jobs passed on all
three desktop platforms. The native learning jobs passed on Linux and macOS,
including all seven real-model RAW tests. The macOS log explicitly reports
`Mps`, with CPU operator fallback disabled, and maximum camera-linear difference
`1.5497208e-6` from CPU across the four signed/above-white Bayer probes. Tiled
reconstruction, cancellation, large-width photometry and renderer/cache checks
also passed. This verifies native graph execution, not universal camera quality.

Windows failed during graph preparation: Git's automatic LF-to-CRLF checkout
changed the pinned oracle source bytes. The exporter correctly rejected it.
Configured `core.autocrlf=false` and `core.eol=lf` only in the owned external
checkout, before fetching/checking out the oracle. Its source verification stays
strict; no global Git settings are changed.

Repeated the complete preparation helper using a temporary Git configuration
that enables CRLF conversion. The local checkout overrides preserved exact
upstream bytes and graph parity remained zero at every oracle probe. Python
compilation and diff checks passed. The Windows CI rerun is the next required
verification; the full PROMPT.md objective remains active.
