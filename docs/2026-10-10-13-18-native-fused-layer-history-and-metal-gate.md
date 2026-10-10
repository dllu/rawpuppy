# Native fused-layer history and Metal gate

Verified the current combined release at `172cb7f` through the actual X11 egui
editor after saved-layer GPU fusion. A fresh owned copy of the **101.8 MP GFX100S**
RAW, its background-matched XMP and EXR asset uses coherent CUDA and joint RAW
reconstruction. The cache contains RAW graphs but **no Moebius model directory**.

The native loaded photo matches the preceding hybrid GPU/CPU composition release
at **every one of its 470,448 displayed RGBA pixels**. This does not contradict
the small full-float interpolation differences recorded by the
[full-size fused verifier](2026-10-10-13-09-fused-gpu-synthesis-composition.md):
the actual native preview agrees on this photograph at display quantization.

Opening the AI controls keeps all photo pixels exact. “Clear generated fills”
and Ctrl-S remove the layer and restore the original background person, changing
**9,002** displayed pixels. Ctrl-Z/Ctrl-S restore the one matched layer, its
original sidecar bytes and every photo pixel. Ctrl-Shift-Z/Ctrl-S reproduce the
cleared recipe and preview exactly. A final undo/save restores the original state.

The editor receives WM_DELETE_WINDOW and exits zero. A fresh native window with
the same model-free cache then reproduces every restored photo pixel. The RAW,
restored XMP and asset hashes remain unchanged. It also closes normally; both
editor/X-server pairs are cleaned and all four verified child PIDs are absent.
No other project process is changed.

| Native comparison | Changed photo pixels |
| --- | ---: |
| Previous hybrid preview versus current fused preview | 0 |
| AI panel opened | 0 |
| Cleared versus loaded matched result | 9,002 |
| Undo versus loaded matched result | 0 |
| Redo versus first cleared result | 0 |
| Final undo versus loaded matched result | 0 |
| Cold reopen versus restored result | 0 |

The fusion implementation's Linux, Windows and macOS desktop CI jobs also passed.
The required **“Metal compute parity on native GPU”** step is explicitly successful
in [run 38082697336](https://github.com/dllu/rawpuppy/actions/runs/38082697336).
Its existing GPU suite includes the new fused-layer RGB/Bayer, cropped/wide,
overlap/gap/alpha/HDR comparison. Thus native Metal execution of the new kernel
has evidence in addition to local Vulkan and both CUDA memory modes. The three
native-learning jobs were still running at recording time; no all-six-job claim
is made for this run yet.

No source fix was needed. Runtime identities, immutable hashes, history states,
complete displayed-photo comparisons and process outcomes are retained in
[native-fused-layer-ui-2026-10-10.json](data/native-fused-layer-ui-2026-10-10.json).
Owned screenshots and the harness remain under `/tmp/rawpuppy-validation`.
Formatting and diff checks passed; application tests were not repeated for this
documentation-only evidence update. These checks validate the current native
workflow and required Metal gate, not physical monitor colorimetry, universal
inpainting quality or completion of the entire project goal.
