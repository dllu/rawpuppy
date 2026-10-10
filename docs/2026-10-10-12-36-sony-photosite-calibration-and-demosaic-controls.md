# Sony photosite calibration and demosaic controls

The preceding stage control excluded a meaningful whole-image mean effect from
highlight reconstruction and confirmed identical as-shot gains. The residual
high-ISO blue difference still needed separation from demosaicing. This follow-up
uses the independent oracle's actual UI and serialized settings; no Darktable
implementation source is read or copied.

Opened a fresh owned Sony ISO 40000 ARW and the existing highlight-disabled,
matched-crop history in a private X server, temporary configuration/cache and
in-memory library. The UI identifies its baseline method as **RCD**, with color
smoothing, green matching and capture sharpening disabled. Selected **PPG** and
**photosite color (debug)**, then explicitly wrote each sidecar through the UI.
The captured histories identify method values 5, 0 and 4 respectively. Numerical
controls retain the original history and change only its method byte; incidental
UI state and appended history are not substituted into those controls.

The GUI harness initially selected a hidden GTK helper window and had a focus
issue; the actual mapped main window's PID was verified before further input.
Explicit sidecar writing supplied the final records. The owned main window
received WM_DELETE_WINDOW and exited zero. Its harness cleaned the private X
server, and both verified PIDs are absent. No other project process was changed.

With all other original settings held fixed, the same linear Rec.709 float EXR
export at 1800×1200 yields:

| PPG/RCD whole-image mean ratio | Red | Green | Blue |
| --- | ---: | ---: | ---: |
| Demosaic method change | 1.019672 | 0.941203 | 1.102526 |

The blue mean changes **10.25%**, larger than the earlier diagnostic clipping
path's **8.55%** residual. All but four of the 6,480,000 output components change;
mean absolute difference is 0.0015002 and RMS difference 0.0026389. These values
demonstrate a material reconstruction effect. They do not establish which
demosaicer is more accurate or fully attribute every end-to-end difference.

The photosite debug output is exported at **6000×4000**, without resizing. Its
individual Bayer sites let the new
[diagnostic](../examples/trace_oracle_photosites.rs) compare the response directly
with each declared calibration column times the site's normalized value and
as-shot gain. The declared transform predicts **all 72 million RGB components**
with these errors, grouped by R/G/B source photosites:

| Photosite group | Mean absolute RGB-component error | Maximum absolute error |
| --- | ---: | ---: |
| Red | 1.05e-7 | 3.29e-5 |
| Green | 2.43e-7 | 3.27e-5 |
| Blue | 2.36e-7 | 8.51e-5 |

Fitting effective columns from positive sites changes declared coefficients by
at most **0.00003286**. More strongly, the direct signed prediction covers every
site, including **8,416,738 negative normalized sensor values**. None of those
negative sites has an all-zero developed RGB response. The diagnostic clipped
prediction instead has per-component mean errors of 0.00119/0.00087/0.00195.
This rejects sensor zero clipping as an explanation for this debug path and
supports agreement of normalization, gains and effective calibration on the
actual capture.

The ordinary RCD/PPG versus Rawpuppy MHC comparison remains an end-to-end
reconstruction comparison, rather than evidence of a hidden matrix defect.
Production signed values and calibration are unchanged. Physical camera-profile
accuracy, reconstruction quality and other cameras need their own evidence;
agreement with this oracle is not a universal color guarantee.

The actual probe covers all active sites, refuses resized or unaligned inputs,
checks input/reference hashes and writes a fresh receipt. The release probe and
both controlled oracle exports passed. Strict all-target Clippy with CUDA/raw-ml/
Moebius, formatting and diff checks passed. The preceding diagnostics commit's
six desktop/native-learning CI jobs all passed. Production paths were unchanged,
so those application tests were not repeated merely for another example tool.

Parameter identities, complete RGB comparison, effective-column fits, maximum
errors, negative-site counts, immutable hashes and process outcomes are retained
in [the data record](data/sony-photosite-calibration-2026-10-10.json). RAWs, EXRs,
UI captures and the owned harness remain under `/tmp/rawpuppy-validation`.
The full project goal remains active.
