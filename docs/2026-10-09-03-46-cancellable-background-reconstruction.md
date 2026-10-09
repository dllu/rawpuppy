# 2026-10-09 03:46 — Cancellable background reconstruction

Joint AI's initial preparation now runs on one dedicated model worker. Ordinary
photo preview and sidecar work remain on the photo worker, allowing exposure and
other global edits while the larger reconstruction proceeds. The full PROMPT.md
goal remains active.

## Behavior and implementation

The editor displays **Standard preview** with a tile progress counter while the
selected learned method prepares. On completion, only the matching original/hot
pixel configuration can become active. A short receive timeout refreshes progress
and automatically renders the ready result without requiring another user edit.
Temporary Standard previews withhold neural synthesis layers; saved output and
generation wait for the requested learned camera RGB and never use that temporary
path.

Changing method, hot-pixel correction or the document cancels superseded jobs at
tile boundaries. The single owned worker coalesces queued requests, retains its
loaded model and limits simultaneous in-progress RGB allocations. Returning to an
already cached configuration also cancels a different outstanding job. Dropping
a job sets its cancellation flag; cancelled/stale results cannot activate through
a later source request. Existing preview coalescing and durable save/export
operations keep their behavior.

## Verification

An isolated native X11 GFX100S session showed Standard preview and progress while
the hot-pixel rebuild ran. Exposure changed from 1 EV at tile 1/108 to 2 EV at tile
3/108, with the displayed photograph responding during preparation. Selecting
Standard cancelled that rebuild and produced a normal preview observed at 48 ms.
Restarting Joint AI automatically switched to the completed learned result; the
final preview showed 14 ms. These are GUI observations, not general latency bounds.
The saved temporary-sidecar recipe retained the chosen method, hot-pixel flag and
2 EV exposure.

The CLI re-export of a saved Moebius layer on a learned Canon base exactly matched
the previous synchronous export's decoded uint16 RGBA samples. This verifies that
background preparation did not substitute Standard processing for durable output.

All 33 default and 36 native optional tests passed. Six real-model tests passed,
including tile-boundary cancellation/source preservation, nonblocking preview
readiness, superseded-source isolation, phase/photometry, tiled active-border
agreement and renderer lifetime/composition. A dependency-free worker unit test
checks latest-request coalescing, cancellation and release of retired originals.
Strict Clippy passed on default and CUDA/native-learning targets; formatting and
whitespace checks passed. The preceding cache-integration commit's desktop and
native-learning CI jobs were verified successful.

Both the editor and its owned Xvfb were closed/stopped after ownership checks.
All photos, screenshots, sidecars and exports remained in temporary validation
directories; nothing under `~/pictures/raw` was changed. Other projects' processes
were untouched.

## Remaining scope

Cancellation waits for an executing tile/kernel to finish. Full RGB cache memory
and total reconstruction compute are still required. Model quality remains
experimental, with previously observed fine-detail losses; broader camera/HDR,
memory-pressure and native MPS validation are pending. This improves interactive
work during preparation rather than claiming faster total inference.

Settings and observations are retained in
[the background validation record](data/raw-reconstruction-background-2026-10-09.json).
[Learned reconstruction](learned-reconstruction.md) describes setup and scope.
