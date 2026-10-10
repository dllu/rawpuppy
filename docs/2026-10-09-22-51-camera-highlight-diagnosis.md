# Camera highlight diagnosis

Investigated the lavender highlights found in the eight-camera validation, using
the owned GF20–35 orientation-6 and Sigma 70 mm copies. Added a read-only
`trace_camera_highlights` example. It counts normalized active-sensor values by
CFA channel and records a 19×19 grid through camera RGB, as-shot gains, calibrated
scene-linear RGB and AgX formation. It checks the input hash again after tracing.
The output path must be new, so it cannot replace an existing original.

The macro sample at output UV (0.25, 0.45) has camera RGB approximately
(0.77577, 0.99726, 0.63631). As-shot gains produce (1.29722, 0.99726, 1.17358);
camera calibration and lens gain produce scene-linear (1.60309, 1.13771, 1.53791).
The green deficit already exists before AgX. Formation retains it as
(0.82444, 0.75795, 0.81595). The wide sample at (0.8, 0.35) similarly has green
at 1.00272 after interpolation, with scene-linear (1.17943, 1.04524, 1.85254).

Both captures have concentrated near-ceiling sensor populations. Red reaches
exactly one for 2,284,468 wide samples and 620,568 macro samples. The most common
bright green level is about 0.994608, with 1,590,564 and 583,215 occurrences.
The macro blue ceiling is about 0.996079. This is consistent with partially
clipped camera channels rather than a display-profile or AgX-neutral error.
Interpolated camera RGB can exceed the underlying mosaic ceiling, so a repair
cannot classify clipping solely from developed RGB.

The [Darktable manual](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/highlight-reconstruction/)
describes the same green-clipping/white-balance mechanism and the limits of
remaining-channel/neighbor reconstruction. Its [magenta-highlight explanation](https://www.darktable.org/2012/07/magenta-highlights/)
also cautions against losing intact channels by clipping them down. Only published
documentation was consulted; no Darktable implementation was read or copied.

The next implementation needs a sensor-domain clipping mask and reconstruction
before camera calibration, with valid channel values preserved. The learned
camera-RGB cache must retain clipping provenance from its original sensor;
guessing a mask from model output could alter valid HDR values. Existing sidecar
rendering/hashes and unclipped signed/above-white data need explicit compatibility
checks. This journal records diagnosis, not a completed reconstruction feature.

Build with `cargo build --release --example trace_camera_highlights`, then pass an
owned RAW and a new JSON path. Full private traces and camera JPEGs remain under
`/tmp/rawpuppy-validation/real-raw-diversity-2026-10-09`; the public data record
retains histogram peaks and six numerical sample points, not photographs.
Two real release traces, strict Rust 1.99 Clippy for the example, formatting and
diff checks passed. Originals under `~/pictures/raw` were not written.
