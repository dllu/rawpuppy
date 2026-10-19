# EXIF export, automatic sidecars and stable controls

Photo exports now carry photographic EXIF in JPEG, PNG and TIFF, including
BigTIFF. This applies to the native editor and CLI export, perspective and
inpainting commands. The capture directories survive RAW decoding, RGB import
and learned reconstruction independently of the pixel allocation. Export keeps
camera/lens identity, exposure, ISO, original capture dates, timezone/subsecond
fields, artist/copyright, GPS and comments. The output orientation is normal,
pixel dimensions match the resized/cropped output, software identifies Rawpuppy,
and EXIF color space agrees with the output ICC profile. Non-sRGB profiles use
the EXIF uncalibrated value rather than being falsely tagged sRGB.

The implementation uses the existing Rawler and TIFF codecs, without a runtime
ExifTool dependency. JPEG uses APP1 EXIF; PNG uses the standard
[eXIf chunk](https://www.w3.org/TR/png-3/#11eXIf). TIFF stores actual EXIF/GPS
subdirectories, with IFD8 pointers in BigTIFF. Metadata is written inside the
existing atomic export, so an encoding or metadata failure cannot replace a
completed destination with a partial file. The existing ICC handling and bounded
pixel conversion remain in place. Opaque maker notes with source-relative
offsets, source thumbnails and coordinates referring to original pixels are
excluded; blindly relocating those would produce stale or broken metadata.
EXR has no standardized EXIF directory block and retains its existing linear
pixel/chromaticity encoding. The native export dialog starts with JPEG and a
`.jpg` filename; PNG, TIFF and EXR remain selectable.

The editor now saves its independent `.rawpuppy.xmp` automatically after a
400 ms pause in changes. A dedicated serial writer keeps sidecar I/O independent
of rendering, export and model inference. Only one snapshot is pending at a time;
newer edits, undo and redo are saved afterward. Replies identify the document and
snapshot, so an old completion cannot declare newer edits saved. View-only
changes do not trigger writes. The writer still uses atomic sidecar replacement
and syncs the file before reporting success.

Opening another photo or closing flushes the newest snapshot before proceeding.
The ordinary workflow no longer asks whether to save edits. If a save fails,
edits remain dirty, an error is shown, and automatic retries stop; **Save edits**
retries explicitly. Close/open failure retains recovery choices. After a close
has been committed, the editor stops accepting further edits in that frame.
Raw originals and Darktable's own sidecars are never overwritten.

Selectable buttons previously added layout space when their hover frame gained
a border. The theme now uses fill/text feedback with equal border/expansion
metrics in every interaction state. The save button also keeps a constant label
while automatic saves start and finish.

Regression coverage includes all EXIF-capable export extensions, classic TIFF
and BigTIFF subdirectories, ICC/orientation/dimension updates, re-import and
re-export, timestamps, GPS and comment bytes. Autosave tests cover debounce,
coalescing, undo, save failures/retry, switching photos with an older write in
flight, independent writer ordering, and preservation of originals/other
sidecars. Native widget tests compare neighboring button rectangles across
idle, hover and selected states.

The real London GFX RAW exposed a dependency issue: Rawler's embedded-JPEG
convenience reader followed the EXIF link but omitted GPS. Re-reading its TIFF
payload with the generic directory reader fixes this; an embedded-JPEG/GPS
regression now covers that path.

Validation: 168 all-feature tests passed (32 device/model tests ignored),
153 CPU-only tests passed, and strict Clippy 1.99 for all features/targets,
formatting and whitespace checks passed. An independent ExifTool comparison
verified capture fields, including GPS, on 480 × 360 JPEG/PNG/TIFF exports from
an owned copy of the 101.8 MP London RAW. A Display P3 PNG retained its ICC profile
and used EXIF color-space value 65535; sRGB outputs used 1.

The actual native editor saved a line adjustment and its undo without a Save
command, then flushed a redo when closed immediately. It exited normally with
no save prompt. RAW SHA-256 still matches the prior immutable source. No original
RAW, Darktable sidecar or cooked reference was written during this validation.
See [machine-readable results](data/exif-autosave-2026-10-19.json).
