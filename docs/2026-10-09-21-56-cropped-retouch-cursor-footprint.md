# Cropped retouch cursor footprint

The requirements review found a concrete clone/heal interaction mismatch. The
editor displays brush size as a fraction of cropped output width, while the
renderer and saved retouch recipe use width before cropping. Saving the same
fraction made a brush twice as wide as its cursor after a half-width crop, or
four times as wide after a quarter-width crop.

New retouch strokes convert the displayed radius into the existing recipe's
canvas units. Existing recipes keep their established rendering behavior. The
retouch radius comment now names the coordinate system explicitly. Neural mask
painting already uses output-width units and needs no conversion.

A regression uses actual egui pointer press/release events on the editor canvas,
then saves and reloads XMP before comparing rendered samples with the original
pipeline. It covers clone and heal on uncropped, cropped landscape and cropped
portrait synthetic photographs. Eight directions just inside and outside the
visible circular brush verify the painted footprint; original sensor values
remain unchanged. Before the fix, the half-width crop changed the first sample
outside the cursor by 0.126875 in camera-linear RGB. All six cases pass after the
conversion.

All 55 standard tests, the targeted regression, strict Rust 1.99 Clippy for all
targets, formatting and diff checks passed. The targeted regression also passed
with both `moebius` and `raw-ml` enabled against matching CPU LibTorch 2.13.
This closes a specific interaction
bug; broader interaction and photographic validation remain in the full
requirements audit. Tests use in-memory synthetic originals and temporary
sidecars, without writing photographs under `~/pictures/raw`.
