# Sony oracle stage and linear mean controls

The high-ISO Sony comparison retains an unexplained blue discrepancy. This
follow-up separates two possible causes instead of treating whole-image means
from different RAW developers as a pure calibration test.

The serialized oracle history enables highlight reconstruction, while Rawpuppy's
diagnostic linear recipe disables it. Copied the same owned ISO 40000 ARW,
matched active-crop history and private config into a new directory. The final
comparison histories differ by **one character**, changing only the highlight
enable flag from 1 to 0. No other parameters or original namespace bytes change.
An initial XML reserialization changed prefixes and was rejected before image
processing; that failed setup was excluded and corrected by preserving the
original serialized bytes.

Darktable 5.5.0+1351 is used strictly through its executable and output history,
with OpenCL/custom presets disabled, eight workers, private config/cache and an
in-memory library. No implementation source is read or copied. Its 1800×1200
linear Rec.709 float EXR with highlights enabled reproduces the existing oracle
**exactly at all 6,480,000 decoded RGB components**. The new
`compare_color --pixels` option checks component hashes, changed counts and
maximum/mean/RMS differences in addition to the existing patch means.

Disabling highlight reconstruction changes **4,198 components**, with a maximum
RGB difference of 1.7545 and mean absolute difference 0.00000877. It affects
bright samples, while whole-image channel means move only slightly:

| Disabled/enabled oracle mean ratio | Red | Green | Blue |
| --- | ---: | ---: | ---: |
| Highlight toggle | 1.000342 | 0.998454 | 1.000298 |

That change does not account for the remaining blue mean discrepancy. The
oracle's recorded white-balance coefficient prefix is also **exactly equal**
to Rawpuppy's declared as-shot gains: 1.82421875, 1 and 2.4296875.

The normalization diagnostic now predicts a linear working-RGB mean directly
from active CFA channel means, as-shot gains and the declared camera-to-working
matrix. A linear constant-preserving reconstruction should retain those means
apart from finite boundaries and preview sampling. This is an algebraic
consistency check against declared metadata, not an independent physical profile
measurement or a reference for nonlinear reconstruction.

| Actual/predicted Rawpuppy mean ratio | Red | Green | Blue |
| --- | ---: | ---: | ---: |
| Signed sensor | 1.000379 | 1.000158 | 1.000582 |
| Diagnostic sensor zero clipping | 1.000463 | 1.000177 | 1.000867 |

Signed means differ by at most **0.059%**, and diagnostic clipped means by
**0.087%**. This supports consistency of Rawpuppy's reconstruction/calibration
mean with its declared inputs. It does not explain all stages of the independent
oracle. Even after the highlight toggle, the clipped-to-oracle blue ratio is
**1.085460**. Production continues to preserve signed sensor values; no matching
factor or clamp is introduced from this comparison.

The owned ARW remains unchanged and the oracle writes no extra input sidecar.
Both corrected exports and the actual diagnostic finish successfully. Missing
comparison arguments produce a normal CLI usage error, with no panic. Strict
all-target CUDA/raw-ml/Moebius Clippy, formatting and diff checks passed; no
production path changed, so the already passing application tests were not
repeated. The preceding matching implementation passed all six desktop/native-
learning CI jobs, including native Metal/MPS and Windows execution.

Hashes, full statistics, source/parameter identities and scope limits are in
[the data record](data/sony-oracle-stage-controls-2026-10-10.json). RAWs, EXRs and
isolated configurations remain under `/tmp/rawpuppy-validation`. The remaining
oracle discrepancy and broader physical-camera/profile verification stay open
in the full project audit.
