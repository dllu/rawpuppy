# Native matched-layer undo and reopen

Verified the current combined CUDA/raw-ml/Moebius release at `469cc0f` through
actual egui input events. The source is a fresh owned copy of the **101.8 MP
GFX100S** RAW, its saved versioned background-matched layer and content-addressed
EXR. Both native editor windows use an isolated cache with joint RAW graphs and
**no Moebius model directory**. There is no sampling or solver execution on reload.
The original photograph directory remains untouched.

The editor opens the layer with joint RAW reconstruction and 1.8 EV exposure.
Opening the AI panel preserves every photo pixel. Clicking “Clear generated
fills” and Ctrl-S removes its one layer; the original background person returns
and **9,002 displayed photo pixels** change. Ctrl-Z and Ctrl-S restore the matched
layer, all photo pixels and the original sidecar bytes. Ctrl-Shift-Z and Ctrl-S
restore the cleared recipe and preview exactly. A final Ctrl-Z/Ctrl-S restores
the initial matched recipe before normal window-manager closure.

| Native state | Changed photo pixels from initial matched preview |
| --- | ---: |
| AI panel opened | 0 |
| Layer cleared and saved | 9,002 |
| Undo and save | 0 |
| Redo and save | 9,002, identical to the first cleared state |
| Final undo and save | 0 |
| Fresh reopen | 0 |

Comparisons cover **all 470,448 RGBA samples** in the actual 594×792 photo
rectangle. The fresh reopened window also matches every sample despite Moebius
remaining absent. This is display-resolution evidence; the new layer's existing
[full-size verification](2026-10-10-11-35-generated-background-matching.md)
separately covers every full render pixel and exported TIFF component.

The RAW hash remains
`09dd4764e595f18c49a51a40507fb54bd24b97b7e8b5d134e5fb6497a2a447fc`.
The restored sidecar is byte-identical to its initial hash
`2b7a82e6ba01b7b7aeaa9b85abdfcd11aa6bae8e9d521548bb546ba5ceb0b9c5`;
the generated asset retains its content hash and `boundary_poisson_v1` method.
Both editors receive WM_DELETE_WINDOW and exit **zero**. Their owned X servers
are cleaned up; all four verified child PIDs are absent afterward. No other
project's process is changed.

No implementation fix was needed. This strengthens the actual editor and saved
history evidence for the latest quality change. Its three desktop CI jobs passed
at recording time; native-learning jobs were still running. Formatting and diff
checks passed. All runtime identities, recipe states, frame comparisons, process
outcomes and artifact hashes are retained in
[the data record](data/native-matched-layer-ui-2026-10-10.json). Screenshots and
the owned harness remain under `/tmp/rawpuppy-validation/native-matched-layer-ui-2026-10-10`.
This does not establish physical display colorimetry, general photographic quality
or completion of the full project goal.
