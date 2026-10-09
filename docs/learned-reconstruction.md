# Joint RAW reconstruction pilot

The optional `raw-ml` feature provides native Rust inference for the RawNIND Bayer
joint denoising/demosaicing checkpoint. The region API processes bounded sensor
contexts; the editor and exporter cache a full camera-RGB reconstruction.
The editor offers **Joint AI (experimental)** under Sensor & detail after a verified
model is installed. Standard MHC/bilateral remains the default. Preview, export
and synthesis use the same prepared camera RGB. Selection remains provisional;
the measured quality gains below also come with some detail loss.

## Model selection and attribution

[RawNIND](https://arxiv.org/abs/2501.08924) operates on real RAW noise pairs and
keeps its Bayer output in linear camera RGB. This fits a photography pipeline
before white balance, camera calibration, geometry and tone mapping. Intel OIDN's
supplied rendered-image filters do not establish camera-RAW quality.

The [authors' repository](https://github.com/trougnouf/rawnind_jddc) licenses its
code under GPLv3 and explicitly dual-licenses pretrained weights under GPLv3 and
CC BY 4.0. This pilot uses the **CC BY 4.0 weight alternative**, with attribution
to Benoit Brummer and Christophe De Vleeschouwer. The preparation tool expresses
the parameter graph independently using generic PyTorch operators and checks it
against the original implementation as an external numerical oracle. The GPL
implementation is not copied into Rawpuppy or bundled with the graph.

The evaluated weights are
`DenoiserTrainingBayerToProfiledRGB_4ch_2024-02-21-bayer_ms-ssim_mgout_notrans_valeither_-4`,
iteration 4350000: 31,059,270 bytes, observed SHA-256
`c1f7de3de24cb12f4803ee65e11cfb467e9a5b80c3cb7126746ef026106c15e0`.
The authors' [published file](https://drive.google.com/uc?id=1dFTLeljWi9wwojcZUsam8bE31JdYy3oM)
does not provide a separate publisher-signed checksum; this pins the evaluated
download. Its training arguments explicitly reserve the two scenes used below.

More recent methods were also inspected. The August 2026
[structural-guidance paper](https://arxiv.org/abs/2608.09995) links to a GitHub
repository returning 404 on the inspection date. The March 2026
[frequency-domain model](https://github.com/Donghui-Zhang2005/Demosaicing-and-Denoising)
has neither a license notice nor released checkpoint files/assets in its current
repository. Samsung's 2025 [unified model](https://github.com/SamsungLabs/unified-demosaicing)
uses CC BY-NC-SA 4.0. Recency alone does not supply a deployable, validated camera
model; these alternatives remain research leads.

## Preparation and native use

Use matching LibTorch **2.13**, including its runtime loader paths, as described
for [native Moebius](moebius.md). Preparation was evaluated in the existing
isolated PyTorch 2.14 environment. Check out the external oracle at
`e80fb7b14dc440bba38c01f312fd3cc02deb566c`; only its model file is executed.

```sh
python tools/export_rawnind.py --weights /path/to/iter_4350000.pt \
  --oracle-repo /path/to/rawnind_jddc --output /path/to/prepared-raw-model
cargo build --release --features raw-ml --example benchmark_rawnind
target/release/examples/benchmark_rawnind photo.raf /path/to/new-results \
  --graph /path/to/prepared-raw-model --device cuda --crop 3000 4000 513 517
cargo build --release --features raw-ml,cuda
target/release/rawpuppy install-raw-model /path/to/prepared-raw-model
target/release/rawpuppy edit photo.raf
target/release/rawpuppy export photo.raf result.tiff --reconstruction raw-nind-v1
```

The exporter validates the checkpoint identity and unchanged oracle source,
determines the standard U-Net pooling/concatenation order by numerical comparison,
then checks multiple sizes and signed/above-white probes. Prepared graphs include
their hashes, attribution, source identities and maximum numerical differences.
The loader requires manifest version 2 and verifies the graph checksum.
`--device cpu` also ran successfully. The real graph's native MPS checks passed
on Apple Silicon macOS CI, including CPU parity and tiled/renderer integration.

The native-learning CI matrix now covers Linux, macOS and Windows with matching
PyTorch/LibTorch 2.13. `tools/prepare_ci_raw_model.py` downloads the published
31 MB weights, verifies their exact size/hash, checks out the pinned external
oracle, and prepares the independently expressed graph with attribution. Tests
install it into an isolated model cache and run the seven real-model checks,
including all Bayer phases, wide inputs, tiles, cancellation and renderer reuse.
The device comparison includes signed/above-white inputs and compares against
CPU output. macOS explicitly requires MPS, with CPU operator fallback disabled.
Only graph provenance and attribution are uploaded, not the weights or photos.
Linux CPU, Windows CPU and macOS MPS passed all seven real-model checks in CI run
`37971658127`, using the pinned local native-binding patch. The macOS probe
explicitly selected `Mps` and differed from CPU
by at most `1.55e-6` camera-linear units on the four-phase signed/above-white
probe. The local GB10 CUDA comparison observed the same maximum difference.
Windows preparation exposed Git's CRLF conversion; the owned oracle checkout
now disables conversion rather than relaxing the byte-identity check. The
Windows probe selected `Cpu`, matched its CPU reference exactly, and the graph
exporter matched the external oracle exactly. These checks establish execution/integrity, not photographic
restoration quality across all cameras or native inference latency guarantees.

Regions use **unrotated sensor coordinates**, including sensor margins. The model
receives packed RGGB `[R,G top-right,G bottom-left,B]`, without white balance.
Reflection at the photographed active area supplies context without sensor
overscan margins contaminating its borders. A 256-pixel halo covers
the receptive field, and the pooling grid stays anchored to the sensor's CFA
phase. Arbitrary odd region dimensions are cropped from the padded result.
Its output scale is learned arbitrarily; the adapter matches the sum of output
colors at observed CFA positions to the corresponding original sensor sum.
It does not independently rebalance colors or clip signed/above-white samples.
Full-image preparation uses 1024-pixel tiles, retaining their unscaled predictions
until one gain correction is computed from all observed active-area CFA samples.
It reflects valid RGB into sensor margins for continuous edge sampling. This
avoids changing exposure independently for neighboring tiles. An odd cropped-area
test with deliberately bright overscan values agrees with a single-context result
within `3e-6`, remains opaque and leaves the original untouched.

The graph embeds deterministic, full-float32 convolution requests. An initial
graph captured cuDNN's TF32 allowance, causing small differences between
overlapping contexts. Explicit per-operator precision fixed that check without
changing another model's process-wide policy. Full-float32 CPU/CUDA outputs on the
Canon region differed by at most `5.66e-7` camera-linear units; this is a measured
example, not a universal error bound.

## Held-out real noise comparison — 2026-10-09

Two public held-out scenes from the [RawNIND dataset](https://dataverse.uclouvain.be/dataset.xhtml?persistentId=doi:10.14428/DVN/DEQCIM)
were compared at 512×512 original sensor pixels. All four RAW downloads matched
their published SHA-1 identities. The clean reference itself uses MHC, so this
comparison evaluates reconstruction against an independently captured low-noise
reference rather than proving perfect demosaicing. Alignment uses only the
clean/noisy MHC green channels; only the clean reference is resampled. A single
exposure correction from observed CFA samples is shared by every noisy method.
Metrics exclude a 32-pixel border and reference camera values at or above 0.99.

| Camera / noisy ISO | MHC PSNR | Best tested bilateral + MHC PSNR | Joint RawNIND PSNR |
| --- | ---: | ---: | ---: |
| Canon EOS 500D / 3200 | 37.74 dB | 41.05 dB | 41.51 dB |
| Sony ILCE-7C / 40000 | 33.20 dB | 41.15 dB | 44.04 dB |

PSNR uses a peak of one in camera-linear RGB. The bilateral grid is fixed at
sigma 0.003, 0.01, 0.03, 0.1 and 0.3; the best result is selected after observing
the reference, giving that baseline an optimistic advantage. These are two regions,
not publisher MS-SSIM results or a general model ranking. The inspected Sony
result strongly reduces noise and retains the main painted pattern, but loses
some true fine speckles and softens details. The Canon result also smooths fine
surface variation. Display previews retain each capture's own exposure; the
numeric comparison applies the documented shared exposure correction.

Full-float32 warm CUDA inference was around 130 ms per 512-pixel region on GB10,
including preparation, padding, decoding and transfer, with a 1056×1056 context.
The Canon CPU run took 283 ms warm with eight workers. A 513×517 region from the
GFX100S 100 MP source ran in 132 ms warm; this validates a bounded region, not
full-frame neural performance or GFX high-ISO quality. Two same-device runs were
byte-identical. Other projects shared the workstation. GPU peak memory was not
measured here, and these timings are not latency guarantees.

## Editor and full-image cache — 2026-10-09

The renderer retains one prepared RGB source keyed by original identity and hot
pixel correction. Exposure, calibration, tone and geometry changes reuse it.
Hot pixel correction runs before learned inference; standard sensor bilateral
filtering is bypassed for the joint method. Old recipes omit the new reconstruction
field and retain their exact serialization/fill hashes. Changing the selected
method changes the saved recipe and invalidates preceding synthesis identities.
Direct pipeline calls reject an unprepared source for a neural recipe.

The GFX100S 101.8 MP image took **16.57 seconds** for its initial full reconstruction
and CUDA preview in the controlled benchmark. Three subsequent 1800-pixel previews
with exposure changes averaged **3.24 ms**. A full corrected 8736×11648 RGBA16 TIFF
export took **17.90 seconds** including reconstruction/render and **0.89 seconds**
for encoding; alpha is 65535 throughout. The inspected knit crop is close to the
MHC appearance with mild smoothing and no obvious tile boundary. These measurements
use one scene on a shared GB10, not a general quality or latency guarantee.

An isolated native X11 editor exercised model selection, a cached exposure change,
save, normal close and reload. The XMP retains `raw_nind_v1` and the exposure value;
reopening rebuilds the cache. Source-retirement tests confirm the renderer releases
old original allocations when switching documents. Saved synthesis uses the same
learned base source.

The cache is in RAM, with a single camera-RGB allocation (~1.24 GB for this sensor)
in addition to the original sensor. Eligible coherent CUDA devices share that
allocation with the fused renderer. No processed photo database is written. The
initial pass runs on one owned reconstruction worker. The editor shows an explicit
**Standard preview** with tile progress while Joint AI prepares; exposure and other
ordinary preview edits continue on the photo worker. A successful matching result
automatically replaces the temporary preview. Neural synthesis layers are withheld
from the temporary Standard image. Export and generation wait for the requested
learned source and never commit this temporary preview.

Changing reconstruction, hot-pixel correction or the document cancels superseded
work at a tile boundary. Requests coalesce on the single model worker, bounding
in-progress allocations. Reverting to an already cached configuration cancels a
different job still running. Cancelled results cannot activate on a newer source.
An isolated GFX session changed exposure from 1 to 2 EV while a hot-pixel rebuild
advanced from tile 1/108 to 3/108, switched to Standard, restarted and automatically
finished the learned preview. Tests cover cancellation, stale-job isolation,
source retirement and latest-request coalescing.

An isolated Linux CPU process now verifies full-cache allocation rejection using
the actual prepared graph. After model/thread warmup, a soft RLIMIT_AS allows
64 MiB of additional virtual address space. The 4096×4096 sensor's 192 MiB RGB
cache is rejected before its first tile; the returned error identifies the
allocation size. Restoring the original limit permits identical patch inference
on the same model, with the source unchanged. A worker test also verifies an
error does not poison its request queue or retain the failed source.

Build `reconstruction_memory_probe` with `--features raw-ml` and matching LibTorch,
then pass the prepared graph directory. The limit applies only to that probe's
address space; this does not exhaust shared system RAM or establish CUDA/MPS
device-allocation recovery. [The record](data/reconstruction-memory-2026-10-09.json)
retains the exact limit and failure. No partial reconstructed image is returned.

More device/memory-pressure coverage and wider camera/illuminant quality
remain outstanding. Full-frame execution does not establish HDR
restoration quality.

Tests cover all four Bayer phases, unchanged originals, observed-sample photometry,
odd regions on a 100,003-pixel-wide source, and overlapping context consistency.
The real-model tests are opt-in because the weights are external. Set
`RAWPUPPY_TEST_RAWNIND_GRAPH` and run `cargo test --features raw-ml --test raw_ml -- --ignored`.
Normal desktop builds remain independent of LibTorch. The shared device-selector
refactor also reproduced the previous native Moebius texture output byte for byte.

[raw_patch.rs](../examples/raw_patch.rs) exports checked sensor/MHC fixtures;
[compare_raw_reconstruction.py](../tools/compare_raw_reconstruction.py) records
alignment, shared exposure, masks, hashes, errors and color ratios. The complete
[data record](data/raw-reconstruction-2026-10-09.json) includes settings and provenance.
The dataset's umbrella license is CC BY-SA 4.0; the Canon scene has an explicit
CC0 permission entry. RAW files, derived photographs and weights stay outside the
repository. Nothing in `~/pictures/raw` was modified.
