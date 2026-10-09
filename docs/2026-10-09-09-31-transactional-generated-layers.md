# 2026-10-09 09:31 — Transactional generated-layer application

Layer identity checks previously preceded compositing, but asset loading was
interleaved with pixel mutation. A later missing or invalid asset could return an
error after an earlier fill had already changed the caller's raster.

Application now resolves all visible immutable layer snapshots before any output
pixel changes. Compositing then has no remaining fallible asset operation. The
16-entry persistent cache and offscreen culling remain. Temporary references hold
visible contexts through the operation; memory scales with those contexts and
does not require a full-photo rollback raster or repeated photo rasterization.

The regression combines a valid first fill with a missing later asset, then with
a corrupt file whose content hash matches its identity. Both failures preserve
the entire raster. Moving the bad context offscreen still permits the visible
valid fill. The original remains unchanged.

Validation: six synthesis tests, all 44 default tests, strict Rust 1.99 Clippy,
formatting and diff checks passed. The preceding scoped-preview-error CI also
passed all four jobs. Broader model/photo/platform coverage remains; the full
PROMPT.md goal is active.
