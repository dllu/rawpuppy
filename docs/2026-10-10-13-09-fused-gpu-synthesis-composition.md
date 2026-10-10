# Fused GPU synthesis composition

Saved neural layers previously ran through a separate sparse CPU composition
pass after GPU rendering. They now sample and compose in the **shared Rust photo
kernel**, after curve/split toning and before output conversion. Each layer sees
the preceding layer's RGB and gap opacity, retaining recipe order, inward feather
and premultiplied bilinear filtering. This removes that additional output pass
when the generated buffers fit the selected device.

The kernel retains **eight storage bindings**, including output, for portability.
Curve and retouch values join the existing parameter buffer; the freed slots hold
generated pixels and layer parameters. Copied CUDA and Vulkan/Metal sessions
retain uploaded generated data. Coherent CUDA borrows a single asset's existing
owned RGBA allocation directly, without a CPU/GPU pixel copy. Distinct assets
are deduplicated and packed once when the visible set changes, then reused.
Tests verify the single-asset pointer, duplicate offsets, stable packing and
rejection of an unsupported binding budget without discarding a live atlas.

GPU arithmetic can round normalized positions differently near brush boundaries.
Exact context bounds, outer paint and fully weighted cores therefore use integer
row spans generated with the historical CPU expression. Binary searches identify
their endpoints; metadata scales with selected rows and dabs, never another
whole-photo mask. The shader interpolates generated RGB and feather weights
inside that exact membership. Fully opaque source samples explicitly retain
opaque interpolation, so filled corners cannot reopen from division roundoff.

Visible asset identities and pixels resolve before output work. Missing/corrupt
assets still fail without partial composition. GPU storage/addressing limits
retain the existing parallel CPU compositor, including for an explicitly selected
GPU backend whose base rendering remains supported. They introduce no smaller
core file limit. A renderer status check lets tests require actual fusion rather
than inadvertently accepting the fallback.

The actual fused path passes **Vulkan**, **copied CUDA** and **coherent CUDA** on
RGB and Bayer sources, full/cropped views and **100,003×3** output. Controls combine
signed/HDR generated values, partial/transparent pixels, three overlapping layers,
gaps and feathering. Repeated cached output is identical; unchanged output samples
stay exact. Maximum component error against the existing CPU composition of the
same GPU base is **4.51e-6**, with alpha within 1e-6. Existing GPU retouch, AgX,
wide-source and HDR regressions also pass after parameter repacking.

The current full-size verifier runs the saved background-matched GFX edit with
`--require-fusion --compare-cpu-composition` and **no Moebius cache**:

- **8,736×11,648 / 101,756,928 pixels**; fusion confirmed on coherent CUDA.
- Every sample finite; **2,037,444** changed pixels remain inside declared paint.
- Every unpainted sample and alpha exact; RAW/XMP/sensor hashes unchanged.
- Maximum RGBA difference from CPU composition **4.06e-6**, mean **1.85e-9**.
- **1,388,507** painted float pixels differ in low bits; this is numerical parity,
  not a claim of bit-identical interpolation or blending across backends.
- Every **407,027,712** Display P3 RGBA16 TIFF component passes, plus ICC and
  straight-alpha tags; 777 strips and 814,062,462 encoded bytes.

The CPU comparison's full float hash matches the previously verified saved
result, `7208ed2d039a2faea0b25a1cc3acc51fe4e39a7e552136ec746a17d24779db39`.
The fused result has its own hash in the data record. Saved-layer rendering took
**0.249 seconds**, including first asset validation, row-span preparation and
base rendering. This is one observation on a shared machine, not isolated kernel
latency or a general speed guarantee. Probe peak RSS is **7,613,728 KiB** because
it temporarily holds three full float rasters for comparison; ordinary rendering
does not allocate the two reference rasters.

All **96 default** and **104 combined-feature** tests pass. Actual fusion parity,
GPU/HDR regressions, storage/membership controls, strict all-target combined-feature
Clippy, the no-GPU all-target build, formatting and diff checks pass. The current
combined native editor release also builds. The required macOS Metal gate includes
the new fusion test through its existing GPU suite and must complete after push;
local builds alone do not establish native Metal execution.

Implementation and measured identities are retained in
[fused-synthesis-gpu-2026-10-10.json](data/fused-synthesis-gpu-2026-10-10.json).
The [full-size probe](../examples/verify_synthesis_export.rs) keeps its old mode
and exposes the two additional verification flags. Assets, photographic outputs
and logs remain under `/tmp/rawpuppy-validation`. This advances the requested
single-pass composition; model-quality and physical colorimetry claims remain
bounded by their separate evidence in the full project audit.
