# Wide RAW DNG and hot-pixel verification

The previous >100,000-pixel checks primarily constructed developed RGB images.
That does not prove a wide RAW file passes decoding, CFA addressing and sensor
cleanup. The controlled DNG generator now accepts `--width` and `--height`, scales
its nine constant/noisy Bayer patches and interior sampling regions accordingly,
and keeps the existing 768-square defaults. TIFF dimensions narrow only to their
actual format representation, sample allocation is fallible, and the old five
calibration/white-balance regressions still pass.

A new file-level regression writes an actual **100,003 × 24** uncompressed,
16-bit RGGB DNG with black 512 and white 15360. It encodes one isolated white
sensor sample at **x=70,000, y=12**, beyond a 16-bit coordinate range. Decoding
retains the full dimensions and normalized value 1.0; reconstruction without
cleanup retains that red sample. With hot-pixel cleanup enabled, the full RAW row
matches its quantized, analytically specified colors within 0.000005 at all
**99,985** patch-interior positions. Only three pixels at either side of each
patch are excluded from that constant-color comparison, because reconstruction
properly mixes colors there. File bytes and normalized sensor hashes are unchanged.

The GPU regression includes **all 100,003 row pixels**, including patch borders
and the corrected hot sample. Vulkan, copied CUDA and coherent CUDA all passed,
with finite values, exact alpha and unchanged file/sensor hashes. Maximum error
against CPU was 0.00000003 for Vulkan and 0.00000001 for both CUDA memory modes.
The test is now part of the Linux Vulkan and macOS Metal CI probe steps; the CPU
file test runs in the existing DNG step on all three desktop platforms.

The release generator also produced constant and signed-noise wide DNGs,
decoded and exported their full float EXR images, and recorded all nine patch
means. The constant controls agree with analytic calibration within 3.0e-8.
ExifTool independently confirms the 100,003-pixel width, 24-pixel height, 16-bit
samples, CFA layout and black/white levels. These synthetic controls establish
wide DNG addressing and renderer behavior; they do not establish universal RAW
camera/encoding or photographic-quality coverage.

```sh
cargo run --release --example raw_color_fixture -- /tmp/new-wide-controls --width 100003 --height 24
cargo test --example raw_color_fixture --locked
RUST_MIN_STACK=33554432 cargo test --example raw_color_fixture wide_dng_gpu_preserves_raw_coordinates_and_hot_pixel_cleanup -- --ignored --nocapture
RUST_MIN_STACK=33554432 RAWPUPPY_TEST_CUDA=1 cargo test --features cuda --example raw_color_fixture wide_dng_gpu_preserves_raw_coordinates_and_hot_pixel_cleanup -- --ignored --nocapture
```

All six DNG file/calibration regressions, three local GPU memory/backend paths,
strict CUDA/all-target Clippy, formatting and diff checks passed. The generator's
18-pixel minimum supplies enough samples for its 3 × 3 Bayer controls; it is not
an application input limit. Settings, hashes and output means are retained in
[the data record](data/wide-dng-2026-10-10.json).
