# Grayscale input ICC interpretation

Input review found that PNG/TIFF ICC conversion always declared RGB input pixels,
even for a valid embedded gray profile. Little CMS rejected those images because
the profile's one-channel color space disagreed with the formatter. A tagged
gamma-2.2 gray PNG fixture reproduced the failure before conversion.

The shared input ICC helper now selects GRAY_FLT for gray profiles and RGB_FLT
for RGB profiles, converting both to the existing linear-sRGB working basis with
relative colorimetric intent. Gray inputs use their source scalar rather than
pretending the replicated RGB triple is a gray profile input. Non-RGB/gray ICC
spaces and gray-profile/color-raster mismatches return explicit errors.

The gray temporary buffer contains at most 16,384 floats (64 KiB), reused across
conversion chunks. No second full grayscale photo allocation or application
dimension cap is introduced. A single shared helper serves both the general
image-decoder and native TIFF path.

The regression writes 100,003×1 16-bit PNG and TIFF fixtures with a D50 gray ICC
and gamma 2.2. Every repeated black/midtone/white sample matches its analytic
linear intensity within 3e-5, including chunk boundaries; source files remain
byte-identical. A color PNG incorrectly tagged with the gray profile is rejected
with the profile/raster compatibility error.

All 62 standard tests, strict Rust 1.99 Clippy for all targets, formatting and
diff checks passed. Tests use generated temporary images; no original photograph
or other project was changed. Broader physical color measurements and camera
validation remain in the full PROMPT.md audit.
