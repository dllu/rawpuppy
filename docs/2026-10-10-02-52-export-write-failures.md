# Export write failures

The atomic-output review found that export handed ownership of its `BufWriter`
to each encoder and relied on destruction to write any remaining bytes. Rust's
[BufWriter documentation](https://doc.rust-lang.org/std/io/struct.BufWriter.html)
states that destruction ignores flush errors. Syncing the file afterward cannot
recover bytes that never reached it.

An actual CLI regression reproduced the failure: a child-only 64-byte file-size
limit caused PNG output writes to fail, but the command returned success and
published truncated output. The test ignores SIGXFSZ only in its owned child so
the kernel reports a write error. Parent limits, other processes and original
photographs are unaffected.

Encoders now borrow the buffered writer. The export path explicitly flushes it
with error propagation before file sync and atomic publication. A failure drops
the temporary file and leaves the destination in its prior state. The existing
file format, color conversion and overwrite choices are unchanged; the shared
path also serves editor exports and generated EXR assets.

The Linux regression runs the actual CLI for PNG, JPEG, TIFF and EXR, with both
new and existing output destinations. All eight cases report the expected write
error, preserve existing export and input bytes, publish no new output and leave
no staging files. A separate portable CLI regression rejects the original and a
dot-path spelling with and without `--overwrite`; on Unix it also checks a
symlink alias with a different extension and confirms the symlink survives.
Input and saved-XMP bytes stay unchanged throughout.

All 73 standard Linux tests passed, as did strict Rust 1.99 Clippy for all targets,
formatting and diff checks. The write-error injection is Linux-only; the flush
implementation and direct-path CLI protection checks are portable. This verifies
ordinary I/O failure handling rather than power-loss durability or every possible
filesystem fault. The broader PROMPT.md audit remains active.
