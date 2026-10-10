# Full saved synthesis export

The inward blend was previously checked on a bounded GFX context. The current
release now passed full document rendering and export using that saved layer,
Joint AI reconstruction and coherent CUDA, through a cache containing RawNIND
but **no Moebius model**. No generation was required to render saved pixels.

The [full-size probe](../examples/verify_synthesis_export.rs) loads the owned RAW
and saved recipe, renders the base and complete edited image at **8,736 × 11,648**,
and inspects every output pixel. All samples are finite and coverage is exact.
All **2,037,729 changed pixels** lie within declared paint; every unpainted pixel
is identical to the base. Sensor, RAW, recipe and rendered-buffer hashes remain
unchanged through rendering/encoding. Comparison retains two float rasters in
the verifier; that is not a new application processing stage.

Exported **101,756,928 pixels** to a **814,062,462-byte Display P3 RGBA16 TIFF**.
Decoded all **777 strips** and checked every **407,027,712 component** against
the declared primary/transfer conversion and quantization. The embedded ICC
matches the selected profile (allowing its creation timestamp), dimensions are
full-size, and the alpha tag describes straight/unassociated alpha. No sample is
skipped or inferred from only one crop. Independent LibTIFF large-offset checks
remain in the separate BigTIFF record; this decoder check uses the TIFF crate.

| Stage, after initial preparation | Observed time |
| --- | ---: |
| Full base render | 0.207 s |
| Full render with saved blended layer | 0.588 s |
| Display P3 TIFF export | 0.824 s |
| All-strip decode and component verification | 2.056 s |

The complete probe took **36.23 s**, including decode/full joint preparation,
hashing and all-pixel scans before those timed stages. Peak process RSS was
**5,984,476 KiB / 5.71 GiB**, including the verifier's two comparison rasters.
That is neither ordinary editor/export memory nor complete GB10 device/system
memory. These are single observations on a shared workstation, not latency
guarantees. The release process exited normally with code 0.

```sh
cargo build --release --features cuda,raw-ml,moebius --example verify_synthesis_export --locked
target/release/examples/verify_synthesis_export /path/to/owned-photo.raf /tmp/new-full-export \
  --backend cuda --color-space display-p3
```

Strict combined-feature/all-target Clippy, formatting and diff checks passed.
Settings, hashes, exact counts and measurement scope are retained in
[the data record](data/full-saved-synthesis-export-2026-10-10.json). RAWs and TIFF
remain under `/tmp/rawpuppy-validation`; original photograph directories and
other project processes were not changed. This is full-size workflow/color
conformance evidence, not universal camera, model or physical-display quality.
