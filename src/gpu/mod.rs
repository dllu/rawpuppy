//! Persistent sensor residency, bounded output tiles, and a shared composed Rust kernel.
mod kernel;
use crate::{
    edits::{Edits, RawEdits, ToneMapper},
    input::{SensorImage, pixel_count},
    pipeline::{Pipeline, Rendered},
};
use anyhow::{Result, ensure};
use cubecl::{prelude::*, server::Handle};
use std::sync::Arc;

pub use crate::render::Backend;

pub enum GpuRenderer {
    Wgpu(Session<cubecl::wgpu::WgpuRuntime>),
    #[cfg(feature = "cuda")]
    Cuda(Session<cubecl::cuda::CudaRuntime>),
}

pub struct Session<R: Runtime> {
    client: ComputeClient<R>,
    source: Option<Arc<SensorImage>>,
    input: Option<Handle>,
    prepared: Option<Handle>,
    preparation: RawEdits,
}

impl GpuRenderer {
    pub fn new(backend: Backend) -> Result<Self> {
        match backend {
            Backend::Cpu => anyhow::bail!("CPU is not a GPU backend"),
            Backend::Cuda => {
                #[cfg(feature = "cuda")]
                {
                    ensure!(
                        std::env::var("RUST_MIN_STACK")
                            .ok()
                            .and_then(|v| v.parse::<usize>().ok())
                            .is_some_and(|n| n >= 32 * 1024 * 1024),
                        "CUDA compilation needs RUST_MIN_STACK=33554432 for the compiler worker; the Rawpuppy executable configures this at startup"
                    );
                    Ok(Self::Cuda(Session::new(cubecl::cuda::CudaRuntime::client(
                        &Default::default(),
                    ))))
                }
                #[cfg(not(feature = "cuda"))]
                {
                    anyhow::bail!("CUDA support requires cargo build --release --features cuda")
                }
            }
            Backend::Auto => {
                #[cfg(feature = "cuda")]
                {
                    Self::new(Backend::Cuda)
                }
                #[cfg(not(feature = "cuda"))]
                {
                    Ok(Self::Wgpu(Session::new(cubecl::wgpu::WgpuRuntime::client(
                        &Default::default(),
                    ))))
                }
            }
            Backend::Vulkan | Backend::Metal => {
                let device = cubecl::wgpu::WgpuDevice::default();
                if backend == Backend::Vulkan {
                    cubecl::wgpu::init_setup::<cubecl::wgpu::Vulkan>(&device, Default::default());
                } else {
                    cubecl::wgpu::init_setup::<cubecl::wgpu::Metal>(&device, Default::default());
                }
                Ok(Self::Wgpu(Session::new(cubecl::wgpu::WgpuRuntime::client(
                    &device,
                ))))
            }
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Self::Wgpu(s) => cubecl::wgpu::WgpuRuntime::name(&s.client),
            #[cfg(feature = "cuda")]
            Self::Cuda(_) => "cuda",
        }
    }
    pub fn render(
        &mut self,
        image: Arc<SensorImage>,
        edits: &Edits,
        max_edge: Option<usize>,
    ) -> Result<Rendered> {
        let pipeline = Pipeline::compile(&image, edits)?;
        let (w, h) = pipeline.dimensions(max_edge);
        self.render_region(image, edits, [0., 0., 1., 1.], w, h)
    }
    pub fn render_region(
        &mut self,
        image: Arc<SensorImage>,
        edits: &Edits,
        region: [f32; 4],
        width: usize,
        height: usize,
    ) -> Result<Rendered> {
        match self {
            Self::Wgpu(s) => s.render(image, edits, region, width, height),
            #[cfg(feature = "cuda")]
            Self::Cuda(s) => s.render(image, edits, region, width, height),
        }
    }
}

impl<R: Runtime> Session<R> {
    fn new(client: ComputeClient<R>) -> Self {
        Self {
            client,
            source: None,
            input: None,
            prepared: None,
            preparation: RawEdits::default(),
        }
    }
    fn render(
        &mut self,
        image: Arc<SensorImage>,
        edits: &Edits,
        region: [f32; 4],
        width: usize,
        height: usize,
    ) -> Result<Rendered> {
        let pipeline = Pipeline::compile(&image, edits)?;
        let mosaic = image.cfa.is_some();
        ensure!(
            image
                .cfa
                .as_ref()
                .is_none_or(|c| c.width == 2 && c.height == 2),
            "GPU reconstruction currently supports Bayer and RGB; use CPU for X-Trans"
        );
        let source_bytes = image
            .data
            .len()
            .checked_mul(4)
            .ok_or_else(|| anyhow::anyhow!("Sensor buffer size overflow"))?;
        let page = self.client.properties().memory.max_page_size as usize;
        ensure!(
            source_bytes <= page,
            "Sensor allocation exceeds this GPU's storage binding size; use CPU"
        );
        ensure!(
            image.data.len() <= u32::MAX as usize
                && image.metadata.sensor_width < i32::MAX as usize
                && image.metadata.sensor_height < i32::MAX as usize,
            "Sensor indexing exceeds this GPU kernel's addressing range; use CPU"
        );
        let count = pixel_count(width, height, 1)?;
        ensure!(
            width <= u32::MAX as usize && height <= u32::MAX as usize,
            "Output dimensions exceed GPU addressing; use CPU"
        );
        if self
            .source
            .as_ref()
            .is_none_or(|source| !Arc::ptr_eq(source, &image))
        {
            self.input = None;
            self.prepared = None;
            self.source = None;
            self.input = Some(
                self.client
                    .create_from_slice(bytemuck::cast_slice(&image.data)),
            );
            self.source = Some(image.clone());
        }
        let g = &pipeline.geometry;
        let s = &edits.scene;
        let display = &edits.display;
        let mut params: Vec<f32> = g
            .homography
            .into_iter()
            .flatten()
            .chain(pipeline.calibration.into_iter().flatten())
            .collect();
        params.extend(g.crop);
        params.push(g.aspect);
        params.extend(g.distortion);
        params.extend(g.ca);
        params.push(s.exposure);
        params.extend(s.vignette);
        params.extend([
            s.graduated.exposure,
            pipeline.gradient[0],
            pipeline.gradient[1],
            s.graduated.center[0],
            s.graduated.center[1],
            s.graduated.width,
            edits.tone.saturation,
            edits.raw.denoise,
            display.split_strength,
        ]);
        params.extend(display.shadows);
        params.extend(display.highlights);
        params.extend(region);
        assert_eq!(params.len(), 49);
        let (transpose, fx, fy) = image.orientation.to_flips();
        let mut dims = vec![
            image.metadata.sensor_width as u32,
            image.metadata.sensor_height as u32,
            image.cpp as u32,
            image.origin[0] as u32,
            image.origin[1] as u32,
            image.active[0] as u32,
            image.active[1] as u32,
            transpose as u32,
            fx as u32,
            fy as u32,
            2,
            2,
        ];
        for y in 0..2 {
            for x in 0..2 {
                dims.push(image.cfa.as_ref().map_or(0, |c| c.color_at(y, x)) as u32);
            }
        }
        dims.extend([
            edits.raw.hot_pixels as u32,
            (edits.tone.mapper == ToneMapper::Agx) as u32,
            (display.curve != [[0., 0.], [1., 1.]]) as u32,
            width as u32,
            0,
        ]);
        if mosaic && (edits.raw.hot_pixels || edits.raw.denoise > 0.) {
            if self.prepared.is_none() || self.preparation != edits.raw {
                self.prepared = None;
                let output = self.client.empty(source_bytes);
                let dimensions = self.client.create_from_slice(bytemuck::cast_slice(&dims));
                let parameters = self.client.create_from_slice(bytemuck::cast_slice(&params));
                let groups = (image.data.len() as u32).div_ceil(256);
                let gx = groups.min(65535);
                let gy = groups.div_ceil(gx);
                // SAFETY: sensor indexing is checked above and each invocation guards its index.
                unsafe {
                    kernel::prepare::launch_unchecked::<R>(
                        &self.client,
                        CubeCount::Static(gx, gy, 1),
                        CubeDim::new_1d(256),
                        ArrayArg::from_raw_parts(
                            self.input.as_ref().unwrap().clone(),
                            image.data.len(),
                        ),
                        ArrayArg::from_raw_parts(dimensions, dims.len()),
                        ArrayArg::from_raw_parts(parameters, params.len()),
                        ArrayArg::from_raw_parts(output.clone(), image.data.len()),
                    );
                }
                self.prepared = Some(output);
                self.preparation = edits.raw.clone();
            }
        } else {
            self.prepared = None;
        }
        let mut brushes: Vec<f32> = Vec::new();
        for (i, b) in display.retouch.iter().enumerate() {
            brushes.extend(b.source);
            brushes.extend(b.target);
            brushes.extend([b.radius, b.feather, b.opacity]);
            brushes.extend(pipeline.heal_offsets[i]);
        }
        if brushes.is_empty() {
            brushes.push(0.);
        }
        let mut index = vec![0u32; 4097];
        let mut references = Vec::new();
        for (i, cell) in pipeline.retouch_index.iter().enumerate() {
            index[i] = u32::try_from(references.len())?;
            references.extend(cell.iter().map(|i| *i as u32));
        }
        index[4096] = u32::try_from(references.len())?;
        index.extend(references);
        let lut = self
            .client
            .create_from_slice(bytemuck::cast_slice(&pipeline.curve));
        let brush_buffer = self
            .client
            .create_from_slice(bytemuck::cast_slice(&brushes));
        let index_buffer = self.client.create_from_slice(bytemuck::cast_slice(&index));
        let row_bytes = width
            .checked_mul(16)
            .ok_or_else(|| anyhow::anyhow!("Row byte size overflow"))?;
        ensure!(
            row_bytes <= page,
            "Output row exceeds the GPU binding limit; use CPU"
        );
        let rows_per_tile = (page.min(64 * 1024 * 1024) / row_bytes).max(1).min(height);
        let mut pixels: Vec<[f32; 4]> = Vec::new();
        pixels.try_reserve_exact(count)?;
        for y in (0..height).step_by(rows_per_tile) {
            let rows = rows_per_tile.min(height - y);
            let tile_count = pixel_count(width, rows, 1)?;
            ensure!(
                tile_count <= u32::MAX as usize / 4,
                "GPU tile indexing overflow"
            );
            params[46] = region[1] + region[3] * y as f32 / height as f32;
            params[48] = region[3] * rows as f32 / height as f32;
            dims[20] = rows as u32;
            let parameters = self.client.create_from_slice(bytemuck::cast_slice(&params));
            let dimensions = self.client.create_from_slice(bytemuck::cast_slice(&dims));
            let output = self.client.empty(tile_count * 16);
            let groups = (tile_count as u32).div_ceil(256);
            let gx = groups.min(65535);
            let gy = groups.div_ceil(gx);
            // SAFETY: all arrays remain live through readback; input/metadata lengths are
            // validated and the kernel guards every output index. Grid overshoot is expected.
            unsafe {
                kernel::render::launch_unchecked::<R>(
                    &self.client,
                    CubeCount::Static(gx, gy, 1),
                    CubeDim::new_1d(256),
                    ArrayArg::from_raw_parts(
                        self.prepared
                            .as_ref()
                            .or(self.input.as_ref())
                            .unwrap()
                            .clone(),
                        image.data.len(),
                    ),
                    ArrayArg::from_raw_parts(dimensions.clone(), dims.len()),
                    ArrayArg::from_raw_parts(parameters.clone(), params.len()),
                    ArrayArg::from_raw_parts(lut.clone(), pipeline.curve.len()),
                    ArrayArg::from_raw_parts(brush_buffer.clone(), brushes.len()),
                    ArrayArg::from_raw_parts(index_buffer.clone(), index.len()),
                    ArrayArg::from_raw_parts(output.clone(), tile_count * 4),
                    mosaic,
                    false,
                );
            }
            let bytes = self.client.read_one(output)?;
            let values = bytemuck::try_cast_slice::<u8, [f32; 4]>(&bytes)
                .map_err(|e| anyhow::anyhow!("GPU output alignment: {e}"))?;
            ensure!(
                values.len() == tile_count,
                "GPU returned an incomplete tile"
            );
            pixels.extend_from_slice(values);
        }
        Ok(Rendered {
            width,
            height,
            pixels,
        })
    }
}
