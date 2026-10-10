# Combined native GFX editing workflow

The full-prompt review moved from isolated feature checks to a current combined
release build with `cuda,raw-ml,moebius` and matching LibTorch 2.13. Ran the actual
native editor on a fresh ordinary copy of the previously checked 101.8 MP
GFX100S RAF, in an owned 1440 × 960 Xvfb session. Private model-cache links keep
the workflow separate from original photograph folders and existing model files.

The editor opened on coherent CUDA, then Joint AI was selected through the native
UI. Its temporary Standard preview was explicitly labeled while full camera-RGB
preparation ran, and the completed learned preview displayed correctly. Changed
exposure by dragging the slider to 1.2 EV and saved with Ctrl-S. Ctrl-Z/Ctrl-S
saved 0 EV; Ctrl-Shift-Z/Ctrl-S restored 1.2 EV. Parsed XMP at each checkpoint
verified the actual persisted values and retained Joint AI selection.

Painted a continuous background-person removal selection and generated native
Moebius with 20 steps, seed 0, strength 1.0, guidance 2 and noise offset 0.0357.
The sidecar records 36 mask dabs and one generated EXR layer. Source and asset
SHA-256 identities match. Layer undo/save produced zero layers; redo/save restored
the complete original generated recipe. Inspected native snapshots show the
background person replaced inside the selection, but fence/stump-like texture
and a gray lump remain visible. This records a concrete quality limitation.
Primary subjects outside the selection are retained.

Compared native photo rectangles before/after layer composition: 3,710 of
470,448 displayed pixels changed, with none outside the stored selection expanded
by 0.004 normalized image width (about 2.4 display pixels). That margin accounts
for display filtering; it is not an exact original-resolution selection test.
Existing compositor regressions cover the exact pixel-preservation invariant.

Sent the normal WM_DELETE_WINDOW protocol and observed exit code 0. Reopened
using a separate cache containing only the RawNIND graph and **no Moebius
directory**. The saved layer loaded through the current native EXR path without
the inference model. All 470,448 photo-rectangle pixels match the pre-close state
exactly; source RAW and sidecar hashes remain unchanged. Closed normally again
with exit 0. Both owned editor/Xvfb pairs were cleaned up, and their handles are
terminal. No other project processes or original photographs were changed.

The editor reported an 8 ms CUDA preview after generation. Process RSS samples
were approximately 2.9–3.4 GiB; these are samples rather than peak or complete
device/driver measurements. Startup and preparation frames have different costs.
This checks actual editing and persistence on one native X11 workflow, not
physical display colorimetry or general model quality. The complete settings and
numeric comparisons are in
[native-gfx-workflow-2026-10-10.json](data/native-gfx-workflow-2026-10-10.json).
RAWs, assets, snapshots and the machine-specific interaction harness remain under
`/tmp/rawpuppy-validation/native-gfx-workflow-2026-10-10`.

The prior commit's six desktop/native-learning CI jobs passed. The current full
scope remains in PROMPT.md: broader camera/quality validation, advanced profile
handling and physical display/platform checks are still recorded as open rather
than inferred from this single workflow. No production implementation changed
for this validation.
