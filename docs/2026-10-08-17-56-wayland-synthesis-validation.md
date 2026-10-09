# Wayland synthesis and desktop CI verification

The implementation at `f55cbf2` passed GitHub CI on Linux, macOS and Windows,
including Linux software-Vulkan parity and the separate matching-LibTorch native
runtime build/test job: [run 37866032847](https://github.com/dllu/rawpuppy/actions/runs/37866032847).
The preceding Windows asset-staging failure is resolved; all five synthesis
integrity tests pass on that platform.

Started an isolated GNOME/Mutter 46.2 headless Wayland compositor with a 1440×960
virtual monitor, separate runtime/config/data/cache directories and a private
D-Bus session. X11 and Xwayland were disabled. The native editor opened an existing
generated layer and used CUDA photo rendering. Exercised AI mask painting, actual
Moebius generation, undo/redo (layer count 2 → 1 → 2), and the unsaved-close dialog's
Save action. The editor exited normally after saving both layer references.
Source and asset hashes match the saved identities. All test processes were shut
down; snapshots, test scripts, sidecars and assets stay under `/tmp/rawpuppy-validation`.

This verifies the tested Wayland rendering and mouse-driven synthesis workflow.
Keyboard shortcut coverage, other compositors, display-profile discovery, native
HDR presentation and macOS/Windows GUI/model runtime coverage remain outstanding.
No originals in `~/pictures/raw` were modified, and no other project's processes
or environments were changed.
