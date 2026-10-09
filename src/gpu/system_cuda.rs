//! CUDA driver adapter for coherent system allocations; kernels stay in the shared Rust DSL.
use super::{ShaderArguments, kernel};
use crate::{
    edits::{Edits, RawEdits, ToneMapper},
    input::{SensorImage, pixel_count},
    pipeline::{Pipeline, Rendered},
};
use anyhow::{Context, Result, ensure};
use cubecl::{
    Compiler, CubeTask,
    codegen::InfoBuilder,
    cuda::{CudaCompiler, CudaRuntime},
    prelude::*,
};
use cudarc::{
    driver::{
        CudaContext, CudaFunction, CudaStream, LaunchConfig, PushKernelArg,
        sys::CUdevice_attribute::*,
    },
    nvrtc::{CompileOptions, compile_ptx_with_opts},
};
use std::{
    collections::HashMap,
    marker::PhantomData,
    mem::{ManuallyDrop, MaybeUninit},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Program {
    Prepare,
    Render(bool),
}

pub struct SystemCuda {
    client: ComputeClient<CudaRuntime>,
    context: Arc<CudaContext>,
    stream: Arc<CudaStream>,
    functions: HashMap<Program, CudaFunction>,
    source: Option<Arc<SensorImage>>,
    prepared: Option<Vec<f32>>,
    preparation: RawEdits,
    lattice_advised: bool,
}

struct HostArray<'a> {
    pointer: u64,
    length: usize,
    _allocation: PhantomData<&'a [u8]>,
}
impl<'a> HostArray<'a> {
    fn read<T: bytemuck::Pod>(slice: &'a [T]) -> Self {
        Self {
            pointer: slice.as_ptr() as usize as u64,
            length: slice.len(),
            _allocation: PhantomData,
        }
    }
    fn write<T>(slice: &'a mut [MaybeUninit<T>], elements_per_item: usize) -> Self {
        Self {
            pointer: slice.as_mut_ptr() as usize as u64,
            length: slice.len() * elements_per_item,
            _allocation: PhantomData,
        }
    }
}

// A launch cannot outlive its CPU allocations, including while unwinding.
struct Synchronize<'a>(&'a CudaStream);
impl Drop for Synchronize<'_> {
    fn drop(&mut self) {
        let _ = self.0.synchronize();
    }
}

fn uninitialized<T>(length: usize) -> Result<Vec<MaybeUninit<T>>> {
    let mut buffer = Vec::new();
    buffer.try_reserve_exact(length)?;
    // SAFETY: every bit state is valid for MaybeUninit<T>; no T is read here.
    unsafe {
        buffer.set_len(length);
    }
    advise_owned(&buffer, false);
    // SAFETY: the entire reserved allocation is writable. Zeroing MaybeUninit
    // bytes is valid even for T with restricted bit patterns. CPU first touch
    // installs the page-table entries before the GPU starts writing the result.
    unsafe {
        std::ptr::write_bytes(buffer.as_mut_ptr(), 0, length);
    }
    Ok(buffer)
}

pub(super) fn advise_owned<T>(slice: &[T], collapse: bool) {
    #[cfg(target_os = "linux")]
    {
        use std::sync::OnceLock;
        static HUGE: OnceLock<usize> = OnceLock::new();
        let huge = *HUGE.get_or_init(|| {
            std::fs::read_to_string("/sys/kernel/mm/transparent_hugepage/hpage_pmd_size")
                .ok()
                .and_then(|s| s.trim().parse::<usize>().ok())
                .filter(|size| size.is_power_of_two() && *size >= 4096)
                .unwrap_or(2 * 1024 * 1024)
        });
        let start = slice.as_ptr() as usize;
        let Some(end) = start.checked_add(std::mem::size_of_val(slice)) else {
            return;
        };
        let Some(aligned_start) = start.checked_add(huge - 1).map(|p| p / huge * huge) else {
            return;
        };
        let aligned_end = end / huge * huge;
        if aligned_end <= aligned_start {
            return;
        }
        // SAFETY: only complete pages strictly within this owned allocation are
        // advised. Neither protection nor contents change; unsupported advice is
        // ignored. No process-wide or system-wide settings are modified.
        unsafe {
            libc::madvise(
                aligned_start as *mut _,
                aligned_end - aligned_start,
                libc::MADV_HUGEPAGE,
            );
            if collapse {
                libc::madvise(
                    aligned_start as *mut _,
                    aligned_end - aligned_start,
                    libc::MADV_COLLAPSE,
                );
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (slice, collapse);
}
unsafe fn initialized<T>(buffer: Vec<MaybeUninit<T>>) -> Vec<T> {
    let mut buffer = ManuallyDrop::new(buffer);
    // SAFETY: caller proves every element was initialized. MaybeUninit<T> has
    // exactly T's layout, and ownership transfers using the same allocator.
    unsafe { Vec::from_raw_parts(buffer.as_mut_ptr().cast(), buffer.len(), buffer.capacity()) }
}

fn compile<K: CubeKernel>(kernel: K) -> Result<(String, String, usize)> {
    let task = KernelTask::<CudaCompiler, _>::new(kernel);
    let mut options = <CudaCompiler as Compiler>::CompilationOptions::default();
    options.supports_features.fast_math = true;
    let compiled = task.compile(
        &mut CudaCompiler::default(),
        &options,
        ExecutionMode::Unchecked,
        AddressType::U32.unsigned_type(),
    )?;
    let shared = compiled
        .repr
        .as_ref()
        .context("CUDA compiler returned no representation")?
        .shared_memory_size();
    Ok((compiled.source, compiled.entrypoint_name, shared))
}

impl SystemCuda {
    pub fn try_new() -> Result<Option<Self>> {
        let context = CudaContext::new(0)?;
        if std::mem::size_of::<usize>() != 8 {
            return Ok(None);
        }
        for attr in [
            CU_DEVICE_ATTRIBUTE_INTEGRATED,
            CU_DEVICE_ATTRIBUTE_PAGEABLE_MEMORY_ACCESS,
            CU_DEVICE_ATTRIBUTE_PAGEABLE_MEMORY_ACCESS_USES_HOST_PAGE_TABLES,
            CU_DEVICE_ATTRIBUTE_CONCURRENT_MANAGED_ACCESS,
        ] {
            if context.attribute(attr)? != 1 {
                return Ok(None);
            }
        }
        let stream = context.new_stream()?;
        Ok(Some(Self {
            client: CudaRuntime::client(&Default::default()),
            context,
            stream,
            functions: HashMap::new(),
            source: None,
            prepared: None,
            preparation: RawEdits::default(),
            lattice_advised: false,
        }))
    }

    fn function(&mut self, program: Program) -> Result<CudaFunction> {
        if let Some(function) = self.functions.get(&program) {
            return Ok(function.clone());
        }
        let client = self.client.clone();
        let context = self.context.clone();
        // The composed Bayer compiler needs a larger stack than a caller's main thread.
        let function = std::thread::Builder::new()
            .name("cuda-system-compiler".into())
            .stack_size(32 * 1024 * 1024)
            .spawn(move || -> Result<CudaFunction> {
                let settings = KernelSettings::default().cube_dim(CubeDim::new_1d(256));
                let arg = ArrayCompilationArg { inplace: None };
                let (source, entry, shared) = match program {
                    Program::Prepare => compile(kernel::prepare::Prepare::<CudaRuntime>::new(
                        settings,
                        client,
                        arg.clone(),
                        arg.clone(),
                        arg.clone(),
                        arg,
                    )),
                    Program::Render(mosaic) => compile(kernel::render::Render::<CudaRuntime>::new(
                        settings,
                        client,
                        arg.clone(),
                        arg.clone(),
                        arg.clone(),
                        arg.clone(),
                        arg.clone(),
                        arg.clone(),
                        arg.clone(),
                        arg,
                        mosaic,
                        false,
                    )),
                }?;
                ensure!(
                    shared == 0,
                    "System-memory photo kernels must not use dynamic shared memory"
                );
                let major = context.attribute(CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR)?;
                let minor = context.attribute(CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR)?;
                let suffix = if major >= 9 { "a" } else { "" };
                let ptx = compile_ptx_with_opts(
                    &source,
                    CompileOptions {
                        options: vec![format!("--gpu-architecture=sm_{major}{minor}{suffix}")],
                        include_paths: vec![
                            cubecl::cuda::install::include_path().display().to_string(),
                            cubecl::cuda::install::cccl_include_path()
                                .display()
                                .to_string(),
                        ],
                        ..Default::default()
                    },
                )?;
                Ok(context.load_module(ptx)?.load_function(&entry)?)
            })?
            .join()
            .map_err(|_| anyhow::anyhow!("CUDA system-memory compiler panicked"))??;
        self.functions.insert(program, function.clone());
        Ok(function)
    }

    fn launch(&mut self, program: Program, arrays: &[HostArray<'_>], work: usize) -> Result<()> {
        let function = self.function(program)?;
        let mut info = InfoBuilder::default();
        for array in arrays {
            info.metadata.register_array(
                array.length as u64,
                array.length as u64,
                AddressType::U32,
            );
        }
        let info = info.finish(AddressType::U32).data;
        let info_pointer = info.as_ptr() as usize as u64;
        let mut launch = self.stream.launch_builder(&function);
        for array in arrays {
            launch.arg(&array.pointer);
        }
        launch.arg(&info_pointer);
        let groups = u32::try_from(work)?.div_ceil(256);
        let gx = groups.min(65535);
        let gy = groups.div_ceil(gx);
        let guard = Synchronize(&self.stream);
        // SAFETY: try_new queried coherent host access on this exact CUDA device.
        // Typed allocations are naturally aligned, their lengths match metadata,
        // and all kernels guard grid overshoot. The stream is synchronized before
        // any borrowed allocation can be read, changed or freed, including on errors.
        unsafe {
            launch.launch(LaunchConfig {
                grid_dim: (gx, gy, 1),
                block_dim: (256, 1, 1),
                shared_mem_bytes: 0,
            })?;
        }
        self.stream.synchronize()?;
        drop(guard);
        Ok(())
    }

    pub fn render(
        &mut self,
        image: Arc<SensorImage>,
        edits: &Edits,
        region: [f32; 4],
        width: usize,
        height: usize,
    ) -> Result<Rendered> {
        ensure!(
            image
                .cfa
                .as_ref()
                .is_none_or(|c| c.width == 2 && c.height == 2),
            "GPU reconstruction supports Bayer and RGB; use CPU for X-Trans"
        );
        ensure!(
            image.data.len() <= u32::MAX as usize
                && image.metadata.sensor_width < i32::MAX as usize
                && image.metadata.sensor_height < i32::MAX as usize,
            "Sensor indexing exceeds this GPU kernel; use CPU"
        );
        ensure!(
            width <= u32::MAX as usize && height <= u32::MAX as usize,
            "Output dimensions exceed GPU addressing; use CPU"
        );
        let count = pixel_count(width, height, 1)?;
        let pipeline = Pipeline::compile(&image, edits)?;
        let ShaderArguments {
            mut params,
            mut dims,
            brushes,
            index,
        } = ShaderArguments::new(&pipeline, edits, region, width)?;
        if self.source.as_ref().is_none_or(|s| !Arc::ptr_eq(s, &image)) {
            self.prepared = None;
            self.source = Some(image.clone());
            advise_owned(&image.data, true);
        }
        let mosaic = image.cfa.is_some();
        if mosaic && (edits.raw.hot_pixels || edits.raw.denoise > 0.) {
            if self.prepared.is_none() || self.preparation != edits.raw {
                let mut prepared = uninitialized::<f32>(image.data.len())?;
                self.launch(
                    Program::Prepare,
                    &[
                        HostArray::read(&image.data),
                        HostArray::read(&dims),
                        HostArray::read(&params),
                        HostArray::write(&mut prepared, 1),
                    ],
                    image.data.len(),
                )?;
                // SAFETY: prepare writes every sensor element and completed successfully.
                self.prepared = Some(unsafe { initialized(prepared) });
                self.preparation = edits.raw.clone();
            }
        } else {
            self.prepared = None;
        }
        let row_bytes = width.checked_mul(16).context("Output row size overflow")?;
        // This bounds each launch's addressing; it does not cap the full image.
        let rows_per_tile = (64 * 1024 * 1024 / row_bytes).max(1).min(height);
        let lattice: &[f32] = if edits.tone.mapper == ToneMapper::AgxSdr {
            crate::agx::lattice()
        } else {
            &[0.]
        };
        if edits.tone.mapper == ToneMapper::AgxSdr && !self.lattice_advised {
            advise_owned(lattice, true);
            self.lattice_advised = true;
        }
        let mut pixels = uninitialized::<[f32; 4]>(count)?;
        for y in (0..height).step_by(rows_per_tile) {
            let rows = rows_per_tile.min(height - y);
            let tile_count = pixel_count(width, rows, 1)?;
            ensure!(
                tile_count <= u32::MAX as usize / 4,
                "Output tile exceeds GPU addressing; use CPU"
            );
            params[46] = region[1] + region[3] * y as f32 / height as f32;
            params[48] = region[3] * rows as f32 / height as f32;
            dims[20] = rows as u32;
            let prepared = self.prepared.take();
            let source = prepared.as_deref().unwrap_or(&image.data);
            let result = self.launch(
                Program::Render(mosaic),
                &[
                    HostArray::read(source),
                    HostArray::read(&dims),
                    HostArray::read(&params),
                    HostArray::read(&pipeline.curve),
                    HostArray::read(lattice),
                    HostArray::read(&brushes),
                    HostArray::read(&index),
                    HostArray::write(&mut pixels[y * width..y * width + tile_count], 4),
                ],
                tile_count,
            );
            self.prepared = prepared;
            result?;
        }
        // SAFETY: every tile writes all four channels of every pixel; every launch
        // completed before this allocation becomes a Vec of initialized pixels.
        let pixels = unsafe { initialized(pixels) };
        Ok(Rendered {
            width,
            height,
            pixels,
        })
    }
}

#[cfg(test)]
mod tests {
    use cubecl::{
        Compiler, CubeTask,
        codegen::InfoBuilder,
        cuda::{CudaCompiler, CudaRuntime},
        prelude::*,
    };
    use cudarc::{
        driver::{CudaContext, LaunchConfig, PushKernelArg, sys::CUdevice_attribute::*},
        nvrtc::{CompileOptions, compile_ptx_with_opts},
    };

    #[cube(launch_unchecked)]
    fn probe(input: &Array<f32>, output: &mut Array<f32>) {
        let i = ABSOLUTE_POS;
        if i < input.len() {
            output[i] = input[i] * 2. + 1.;
        }
    }

    #[test]
    #[ignore = "requires CUDA coherent system-memory access"]
    fn cuda_reads_and_writes_ordinary_rust_allocations() {
        let context = CudaContext::new(0).unwrap();
        for attr in [
            CU_DEVICE_ATTRIBUTE_INTEGRATED,
            CU_DEVICE_ATTRIBUTE_PAGEABLE_MEMORY_ACCESS,
            CU_DEVICE_ATTRIBUTE_PAGEABLE_MEMORY_ACCESS_USES_HOST_PAGE_TABLES,
            CU_DEVICE_ATTRIBUTE_CONCURRENT_MANAGED_ACCESS,
        ] {
            assert_eq!(context.attribute(attr).unwrap(), 1);
        }
        let client = CudaRuntime::client(&Default::default());
        let settings = KernelSettings::default().cube_dim(CubeDim::new_1d(256));
        let arg = ArrayCompilationArg { inplace: None };
        let kernel = probe::Probe::<CudaRuntime>::new(settings, client, arg.clone(), arg);
        let task = KernelTask::<CudaCompiler, _>::new(kernel);
        let mut options = <CudaCompiler as Compiler>::CompilationOptions::default();
        options.supports_features.fast_math = true;
        let compiled = task
            .compile(
                &mut CudaCompiler::default(),
                &options,
                ExecutionMode::Unchecked,
                AddressType::U32.unsigned_type(),
            )
            .unwrap();
        let major = context
            .attribute(CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR)
            .unwrap();
        let minor = context
            .attribute(CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR)
            .unwrap();
        let ptx = compile_ptx_with_opts(
            &compiled.source,
            CompileOptions {
                options: vec![format!("--gpu-architecture=sm_{major}{minor}a")],
                include_paths: vec![
                    cubecl::cuda::install::include_path().display().to_string(),
                    cubecl::cuda::install::cccl_include_path()
                        .display()
                        .to_string(),
                ],
                ..Default::default()
            },
        )
        .unwrap();
        let module = context.load_module(ptx).unwrap();
        let function = module.load_function(&compiled.entrypoint_name).unwrap();
        let input = vec![0.25f32, -0.5, 0., 100.];
        let mut output = vec![0f32; input.len()];
        let mut info = InfoBuilder::default();
        for _ in 0..2 {
            info.metadata
                .register_array(input.len() as u64, input.len() as u64, AddressType::U32);
        }
        let info = info.finish(AddressType::U32).data;
        let input_pointer = input.as_ptr() as usize as u64;
        let output_pointer = output.as_mut_ptr() as usize as u64;
        let info_pointer = info.as_ptr() as usize as u64;
        let stream = context.default_stream();
        let mut launch = stream.launch_builder(&function);
        launch
            .arg(&input_pointer)
            .arg(&output_pointer)
            .arg(&info_pointer);
        // SAFETY: coherent host access was queried above. All naturally aligned
        // allocations outlive stream synchronization; the kernel bounds its index.
        unsafe {
            launch
                .launch(LaunchConfig {
                    grid_dim: (1, 1, 1),
                    block_dim: (256, 1, 1),
                    shared_mem_bytes: 0,
                })
                .unwrap();
        }
        stream.synchronize().unwrap();
        assert_eq!(output, [1.5, 0., 1., 201.]);
        assert_eq!(input, [0.25, -0.5, 0., 100.]);
    }
}
