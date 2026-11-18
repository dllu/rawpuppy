# Cursor zoom and navigator

Wheel zoom keeps the corrected-image point under the mouse in place while
there is room to pan. The view clamps to the available canvas at an edge.
Shift-wheel still adjusts brush size. Middle-drag pans in Crop and Guides.
The 100% command uses the scale of the current canvas and display.

The collapsible **Navigator** below the RGB histogram shows the active frame,
with a rectangle for the visible area. Click to move the view, drag the
rectangle to pan, or scroll over the thumbnail to zoom. The header shows Fit
or the current percent of actual pixels. In Crop and Guides the overview
covers the extended corrected canvas used by the tool.
[Darktable navigation reference](https://docs.darktable.org/usermanual/5.8/en/module-reference/utility-modules/darkroom/navigation/)

The photo worker generates a thumbnail at up to 256 pixels on the long edge.
It reuses the thumbnail when the viewport moves and refreshes it for changes
to the rendered frame, display profile, edits, or reconstruction. The preview
and the overview use the same corrected coordinates.

All 80 release library tests passed. The native Wayland test on DSCF0027
verified zoom and thumbnail panning at 100% with the framing tools active.
