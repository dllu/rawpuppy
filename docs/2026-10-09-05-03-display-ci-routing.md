# 2026-10-09 05:03 — Display CI routing

The managed Wayland surface milestone's Linux CI failed in the Xvfb display step:
its unfiltered `--ignored` selection also ran the new managed-Wayland test, without
that test's required compositor/environment. The X11 discovery test itself passed,
as did Windows, macOS and native-learning jobs.

The Xvfb step now names the X11 test explicitly. The managed Wayland test remains
an opt-in test for its own compositor session; it is not silently skipped. The
corrected Xvfb invocation was run locally. No application behavior changed, and
the full PROMPT.md goal remains active.
