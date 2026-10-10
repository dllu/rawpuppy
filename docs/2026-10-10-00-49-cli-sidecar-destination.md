# CLI sidecar destination protection

Save/export review found that the CLI save command accepted any output filename
and implicitly replaced it. Passing a photo filename could write XMP over the
original; passing a conventional `.raf.xmp` could replace another editor's sidecar.
A command-level regression on owned files reproduced acceptance of `photo.raf`.

CLI save now requires the documented `.rawpuppy.xmp` suffix before recipe reading
or any output mutation. That convention matches the editor's path_for behavior
and the README command. Rejection names the required suffix. The dedicated
library serializer and normal replacement of Rawpuppy's own sidecar keep their
existing behavior. Save command help and documentation name the requirement.

The actual binary regression covers a photo filename, another editor's `.xmp`,
and the recipe JSON itself, checking failure and unchanged bytes in each case.
The intended `photo.raf.rawpuppy.xmp` destination succeeds and round-trips through
the namespaced reader. Temporary dummy files are used; no real photo is modified.

All 63 standard tests, strict Rust 1.99 Clippy for all targets, formatting and
diff checks passed. The full PROMPT.md goal remains active while the broader
completion audit is open.
