# Pipeline and invariants

The immutable original feeds a normalized floating-point sensor buffer. Sensor
normalization subtracts the repeating black level and divides by the sensor
range. Negative noise samples and values above sensor white are retained.
Hot pixels and same-color bilateral denoising run before demosaicing. Bayer
reconstruction uses independently implemented Malvar–He–Cutler 5×5 filters;
X-Trans currently uses a color-aware weighted interpolation fallback.

Every output sample traverses a compiled backward map: normalized crop, pinhole
homography (yaw/pitch/roll and focal length), radial distortion, lateral color
aberration, EXIF orientation, and sensor crop. Trigonometric operations and matrix
composition occur when the recipe changes, never per pixel. Sampling reconstructs
sensor RGB on demand. A preview samples directly from the original at its output
resolution; it does not require a 100 MP intermediate RGB image.
Versioned embedded RAF distortion and vignetting use one shared radial lookup;
[lens-correction.md](lens-correction.md) describes conventions, framing and legacy
recipe preservation. Manual coefficients remain part of the same map and gain.

Vignetting, graduated density, and exposure combine into one scalar gain.
Integer camera RAW may recover clipped channel intensity before calibration,
using original sensor clipping coverage and a neutral estimate from intact
white-balanced channels. Intact signed/HDR data stays unchanged. Learned camera
RGB retains packed original clipping provenance in its existing allocation.
Old recipes default this recovery off; see [highlight recovery](highlight-recovery.md).
As-shot sensor gains, camera calibration, user gains, and the RGB mixer combine
into one matrix. XYZ white points are adapted with Bradford where needed.
The working space is scene-linear sRGB/Rec.709 with D65 white, with unbounded
highlights. Camera values are not clipped before tone mapping.

Photographic AgX uses an independently sampled, versioned formation lattice with
Rust tetrahedral lookup in a wide Rec.2020 basis. Existing recipes preserve the
historical polynomial; [color.md](color.md) describes provenance and verification.
The alternative linear mode preserves scene values for
HDR interchange. Clone/heal reads the tone-mapped source, followed by a monotone
cubic curve in perceptual sRGB coordinates and split toning in display-linear
RGB. Saved neural synthesis layers are composited after those local edits. Finally, output conversion
changes primaries and applies the correct transfer function. Little CMS generates
matching ICC profiles and interprets embedded RGB input profiles.
EXR input chromaticities and white points also determine color interpretation;
exports carry explicit linear-sRGB chromaticities.
EXR input decodes the first non-deep RGB layer directly into its owned RGB
allocation. Display/data-window offsets are retained, pixels outside the display
window are cropped, missing pixels remain black, and nonfinite working RGB is
rejected. No intermediate full RGBA raster or conversion copy is retained.

Sidecars use their own `https://rawpuppy.org/ns/1.0/` XMP namespace and full
original filenames plus `.rawpuppy.xmp`. They contain a versioned JSON recipe in
an RDF property. The CLI save command requires this suffix before reading
or writing the recipe, preventing accidental replacement of a photo or another
editor's sidecar. Reads verify the namespace and reject unknown versions or invalid
parameters. Writes stage in the destination directory and rename atomically.

Modern generation uses bounded 512-square context rendered directly from the
original, with seeded Moebius sampling in Rust. Generated pixels persist in
content-addressed float EXR assets beside the XMP, with source and preceding-recipe
identities. Exact output-resolution masks preserve every unpainted pixel.
New painted layers blend inward over 15% of brush radius in working linear RGB,
retaining the generated core and every unpainted sample. Saved legacy layers use
zero blend, and missing geometry always receives full coverage. The same sparse
weights drive composition and gap-coverage checks without another photo raster.
New painted generation first applies a versioned harmonic background correction
within its bounded context. Only opaque unselected neighbors anchor the solve;
selected original colors never enter its guidance. Corrected pixels and the
method identity persist in the asset/XMP. Existing layers and geometric fills
retain their previous behavior, and rendering saved layers runs no solver.
Earlier edits invalidate their fills and require regeneration. Saved layers render without
the model; inference runs only when explicitly requested. See [Moebius](moebius.md).

Dimensions and indices are `usize`; checked multiplication detects address-space
overflow. Export narrows dimensions only to the actual file format's representation.
JPEG's 65535-pixel limit is a format constraint. PNG/TIFF/EXR do not inherit that
constraint. Decoder allocation limits are removed, leaving system memory and core
format constraints. Export never replaces the original path, including aliases.
JPEG conversion uses a fallibly allocated RGB stripe aligned with the encoder's
8-row blocks. Rayon converts that stripe from immutable working floats; scratch
is at most 1 MiB except when one 8-row block is wider. No full RGBA/RGB conversion
rasters are retained. Quality, ICC tagging and final quantization remain the same.

No Darktable source has been copied. Its local checkout can be used to generate
comparison outputs as an independent oracle.

The GPU path uses a composed CubeCL Rust kernel with persistent sensor residency
and output tiles bounded to 64 MiB (or the device's smaller storage binding limit).
Tiles can cross row boundaries, so a row wider than a tile does not impose an
output allocation limit. Each tile retains global pixel coordinates in the full
viewport. CPU and GPU share once-computed pixel steps and fused multiply-add;
radial geometry adds displacement to the original point to preserve precision
near image borders. CUDA is an optional build feature; Vulkan and
Metal use the wgpu runtime. Sensor cleanup is a cached preparation kernel because
it has a spatial neighborhood; geometry, reconstruction, intensity, calibration,
AgX, retouch, curve and split toning run in the output kernel. GPU retouch uses the
same spatial index and precomputed heal offsets as CPU. Source/device addressing
and binding limits select CPU fallback in auto mode and never limit the core image
representation. GPU X-Trans currently selects CPU.
Wgpu renderers share one lazily registered CubeCL client for the process's default
device. Auto and explicit native API selection use the same registration; an
explicit request must match its actual API. Each renderer retains its own sensor,
prepared buffer and layer state. Dropping one renderer retires its source without
affecting another renderer on the same client.
In a CUDA build, Auto tries CUDA first and then the native wgpu API if CUDA
initialization fails or panics. CPU follows only if both initialization attempts
fail. Explicit backend requests retain their requested backend. Initialization
errors identify both unavailable APIs when automatic selection exhausts them.
On eligible coherent integrated CUDA devices, a driver adapter launches the same
Rust kernel using owned system allocations, avoiding sensor duplication and
preview readback. Capability checks, synchronized borrows and local page hints
are described in [system-memory.md](system-memory.md). Other devices use copied
buffers through the existing runtime.
Saved synthesis currently uses sparse CPU composition after GPU readback, with
bounded layer caches and viewport culling. Disjoint rows of each visible layer
composite in parallel, checking target membership before sampling the asset.
Layers finish in recipe order so overlaps and evolving gap coverage remain
deterministic. All visible assets resolve before any output changes. It does not
create another full-image raster; fusing layer sampling into the GPU kernel
remains an optimization.

The CUDA compiler worker requires a 32 MiB stack for the composed Bayer kernel.
The executable sets `RUST_MIN_STACK` at process startup before threads exist;
library callers and GPU test commands must configure it themselves. Normal edit
changes update parameters and LUTs without compiling another shader. The first
GPU preview includes device setup and compilation; warm previews avoid both.
