# X11 monitor-zero profile selection

Reviewing multi-monitor discovery found that the unsuffixed root ICC property
was gated by RandR's primary flag. With two monitors and no RandR primary,
Xinerama monitor zero's valid profile was ignored and discovery fell back without
an ICC. Root profile indexing belongs to Xinerama; zero uses `_ICC_PROFILE`,
while later monitors use suffixed atoms. The [GNOME color-manager discussion](https://bugzilla.gnome.org/show_bug.cgi?id=794486)
quotes this indexing convention and distinguishes it from RandR output identity.
No Darktable implementation was consulted.

Added an actual protocol regression in an owned Xvfb display. It creates two
side-by-side RandR monitors, verifies their Xinerama enumeration, designates
neither primary, and assigns P3/Adobe RGB profiles to the two root atoms. Before
the fix, monitor zero returned no profile. Discovery now reads its indexed root
property regardless of the unrelated primary flag; monitor one remains correctly
assigned its own profile. RandR output-profile precedence remains in place.

The regression also converts synthetic photo samples through one cached encoder
in monitor order zero → one → zero. Each matches its independent ICC conversion,
the two monitor encodings differ, returning to zero restores its original encoding,
and the source values remain exact. It cleans up its owned monitor/property changes.
Existing live root-profile replacement/removal checks pass alongside it. CI runs
both X11 tests serially in its isolated Xvfb session to avoid shared-atom races.

All 61 standard tests, both isolated X11 protocol/conversion tests, strict Rust
1.99 Clippy for all targets, formatting and diff checks passed. Only temporary
display state and synthetic pixels were used; physical monitor settings, other
processes and original photographs were not changed. Physical multi-monitor and
managed colorimetry remain unverified in the full requirements audit.
