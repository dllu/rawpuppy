# README screenshots and London before/after

Added two native editor screenshots and a side-by-side, low-resolution
comparison using the user's requested DSCF0027 RAW from `/mnt/dataz/pictures/raw`.
The photo shows The Standard hotel in London. Both-axis perspective and framing
were checked against its Darktable XMP and corresponding cooked export.
Rawpuppy supplied the actual renders, color calibration and AgX tone mapping.

Photographs are 1200×900 sRGB JPEGs derived from full-resolution renders;
screenshots are 1280×853 genuine captures of the current native editor. Recipes
and a small reproduction example accompany the assets. The README identifies
the baseline, adjustments and manual perspective alignment explicitly.

Verified every README image link, JPEG decoding, dimensions, image profiles,
nonzero vertical and horizontal corrections and the saved AgX recipe. The
reproduction example passed strict Clippy and rendered successfully on coherent
CUDA. The owned editor and X server closed normally. Original RAW, sidecar and
cooked reference files were read only. No production code changed.

See [asset provenance and reproduction](readme-showcase.md).
