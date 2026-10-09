# 2026-10-09 07:36 — Native window CI runtime

The native editor signal check passed SDR and HDR on macOS and Windows in
[run 37944284405](https://github.com/dllu/rawpuppy/actions/runs/37944284405).
Both paths exited normally and retained source/XMP hashes. The macOS native Metal
compute and float GUI/photo checks also passed. Linux failed while opening its
Xvfb window; the helper previously reported only the private child-log filename.

The helper now includes child diagnostics on failure, and failed CI retains its
generated fixture/report/log directory. A local Mesa software-renderer run passes
both launch modes. An isolated loader-path test that makes only the X11 keyboard
runtime unavailable reproduces the immediate `libxkbcommon-x11` startup panic.
Winit loads that library dynamically, while `libxkbcommon-dev` depends only on the
base runtime. CI now installs `libxkbcommon-x11-0` explicitly; README records the
runtime requirement. The original CI child log was not retained, so its precise
cause remains an inference pending the corrected run.

Follow-up: [corrected run 37945480744](https://github.com/dllu/rawpuppy/actions/runs/37945480744)
passed every job, including the Linux native editor step. The explicit runtime
dependency resolves the previously failing test environment.

No system libraries or other projects were changed. This milestone fixes the
native-test environment and diagnostics; the full PROMPT.md goal remains active.
