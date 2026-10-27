# Roomier controls and saved editor defaults

The develop sidebar now starts at 420 logical points and can be resized by
dragging its divider. Body/button text starts at 18 points instead of egui's
13-point default. Slider labels and precise numeric inputs sit above tracks that
span the available width, including temperature and fine channel gains.
Exposure dragging covers ±2 EV in 0.01 EV steps; typing still accepts ±32 EV.
Rotation retains its ±5° drag range, 0.1° steps and unrestricted numeric entry.
Values outside a track's range survive redraws unchanged.

Top-level sections use a height budget and measured body heights to expand when
there is room. Light stays open initially; further sections open as space permits.
Manual open/close choices persist through resizing and preference changes during
the session. Framing tools can open their section. Vignetting and graduated
controls are grouped within Light; export/display controls have their own section.
Grid visibility and style share one menu above the image, with `G` still available.
The toolbar wraps and the status bar hides shortcut hints on compact windows.

The editor creates a TOML config using the platform's configuration directory
through `dirs`: XDG on Linux, Application Support on macOS and roaming AppData
on Windows. Preferences changes and divider resizing save it atomically. The UI
shows load/save errors; invalid files are left intact during loading. Partial
tables merge factory defaults, including partial Detail tables without changing
historical recipe deserialization.

New editor documents start with +0.7 EV, 1.05× saturation and 0.2 sharpening.
Light, Color calibration, Detail and Sensor each offer **Save as default**.
Only the documented global controls are copied; calibration, local edits,
geometry and reconstruction remain photo-specific. Open requests carry a snapshot
of these defaults to the worker. Existing sidecars take precedence; Reset all
edits uses the configured defaults. CLI behavior remains independent.
See [preferences.md](preferences.md) for locations and the TOML schema.

Validation:

- `cargo test --locked --no-default-features`: full CPU suite passed, with the
  existing hardware/photograph-dependent tests ignored.
- Latest `cargo test --locked --no-default-features --lib --test config`:
  60 library tests and 3 config tests passed. Added interaction coverage for a
  10-pixel exposure drag, typed exposure outside the track, and saving module
  defaults without changing photo history. Config tests cover partial files,
  validation failures, atomic save rejection, image-aware defaults and existing
  sidecar precedence.
- `cargo check --locked --all-features`: passed with the local LibTorch setup.
- `cargo clippy --locked --all-features --lib --tests -- -D warnings
  -A clippy::manual_range_contains`: passed. The exception covers an existing
  lint in `src/perspective/edges.rs`; that code is unchanged.
- Native X11 review under an isolated Xvfb display/config directory at 800×600,
  1440×960 and 1440×1680. Checked section expansion, slider layout, the Preferences
  window, saving 22-point text and persisting a divider drag from 420 to 500 points.
  All preview/edit actions used a temporary copy of a repository JPEG; original
  photographs were untouched. macOS/Windows config paths are selected by `dirs`
  and were not exercised natively on this Linux machine.
