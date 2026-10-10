# Continuous photo brush strokes

The editor previously added one clone/heal or AI-mask dab per frame. A rapid
pointer jump left holes between those dabs, including an unpainted start when
the first movement already exceeded the drag threshold. The reproduced failure
left the first inspected sample on a long diagonal stroke unchanged.

Clone, heal and mask now share a physical-distance stroke sampler. It includes
the press origin and interpolates at spacing no greater than one fifth of the
brush radius, with output aspect ratio preserved. Very small motion accumulates
before another dab is added. Release coordinates complete a captured drag. The
clone source offset remains fixed within the gesture, and retouch undo still
records one snapshot per gesture. Release, disabled painting, panning and document
changes reset the interpolation state, keeping separate strokes separate.

Captured pointer travel is clipped against the photo plus a brush radius before
sampling. Work therefore scales with the part of the path that can paint. A
regression takes the pointer to normalized coordinates ten and twenty million
units away, then back, on square and 100,003×17-aspect canvases. Each crossing
remains below 500 dabs and wholly outside motion produces none.

Actual egui pointer regressions exercise clone, heal and AI masks on a cropped
portrait fixture with separate movement frames, movement batched with the press,
and a large final jump carried by the release event. Each checks 101 samples along
the painted path, an unpainted sample between separate gestures, and whole-gesture
retouch undo. The earlier cropped-cursor and middle-button-pan regressions also
pass. All 57 standard tests, strict Rust 1.99 Clippy for all targets, formatting
and diff checks passed. All 18 library tests also passed with both `moebius` and
`raw-ml` enabled against matching CPU LibTorch 2.13.
Tests use synthetic originals and temporary sidecars;
photographs under `~/pictures/raw` were not written. Broader photographic and
platform validation remains in the complete requirements audit.
