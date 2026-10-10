//! Backend selection, conservative CPU fallback, and one rendering interface for CLI and GUI.
use crate::{
    edits::{Edits, Reconstruction},
    input::SensorImage,
    pipeline::{Pipeline, Rendered},
};
use anyhow::Result;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Backend {
    #[default]
    Auto,
    Cpu,
    Vulkan,
    Metal,
    Cuda,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum CudaMemoryMode {
    #[default]
    Auto,
    Copy,
    System,
}

pub struct Renderer {
    backend: Backend,
    #[cfg(feature = "gpu")]
    cuda_memory: CudaMemoryMode,
    initialized: bool,
    #[cfg(feature = "gpu")]
    gpu: Option<crate::gpu::GpuRenderer>,
    label: String,
    layers: Option<crate::synthesis::Layers>,
    #[cfg(feature = "moebius")]
    moebius: Option<crate::moebius::Moebius>,
    #[cfg(feature = "raw-ml")]
    raw_service: Option<crate::raw_background::Service>,
    #[cfg(feature = "raw-ml")]
    raw_job: Option<crate::raw_background::Job>,
    #[cfg(feature = "raw-ml")]
    reconstructed: Option<(Arc<SensorImage>, bool, Arc<SensorImage>)>,
}
impl Renderer {
    pub fn new(backend: Backend) -> Self {
        Self::with_cuda_memory(backend, CudaMemoryMode::Auto)
    }
    pub fn with_cuda_memory(backend: Backend, cuda_memory: CudaMemoryMode) -> Self {
        #[cfg(not(feature = "gpu"))]
        let _ = cuda_memory;
        Self {
            backend,
            #[cfg(feature = "gpu")]
            cuda_memory,
            initialized: false,
            #[cfg(feature = "gpu")]
            gpu: None,
            label: "CPU".into(),
            layers: None,
            #[cfg(feature = "moebius")]
            moebius: None,
            #[cfg(feature = "raw-ml")]
            raw_service: None,
            #[cfg(feature = "raw-ml")]
            raw_job: None,
            #[cfg(feature = "raw-ml")]
            reconstructed: None,
        }
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    fn prepare_source(
        &mut self,
        image: Arc<SensorImage>,
        edits: &Edits,
    ) -> Result<Arc<SensorImage>> {
        if image.reconstruction == Reconstruction::RawNindV1 {
            anyhow::ensure!(
                edits.raw.reconstruction == image.reconstruction,
                "Prepared reconstruction and recipe disagree"
            );
            return Ok(image);
        }
        #[cfg(feature = "raw-ml")]
        if self
            .reconstructed
            .as_ref()
            .is_some_and(|(original, _, _)| !Arc::ptr_eq(original, &image))
        {
            self.reconstructed = None;
        }
        if edits.raw.reconstruction == Reconstruction::Mhc {
            #[cfg(feature = "raw-ml")]
            {
                self.raw_job = None;
            }
            return Ok(image);
        }
        #[cfg(feature = "raw-ml")]
        {
            if let Some((original, hot, prepared)) = &self.reconstructed
                && Arc::ptr_eq(original, &image)
                && *hot == edits.raw.hot_pixels
            {
                self.raw_job = None;
                return Ok(prepared.clone());
            }
            self.request_raw_job(image.clone(), edits.raw.hot_pixels)?;
            let job = self.raw_job.take().unwrap();
            let prepared = job.wait()?;
            self.reconstructed = Some((image, edits.raw.hot_pixels, prepared.clone()));
            Ok(prepared)
        }
        #[cfg(not(feature = "raw-ml"))]
        anyhow::bail!("This recipe uses joint AI reconstruction; build with the raw-ml feature")
    }
    #[cfg(feature = "raw-ml")]
    fn request_raw_job(&mut self, image: Arc<SensorImage>, hot: bool) -> Result<()> {
        if self
            .raw_job
            .as_ref()
            .is_some_and(|job| job.matches(&image, hot))
        {
            return Ok(());
        }
        self.raw_job = None;
        if self.raw_service.is_none() {
            self.raw_service = Some(crate::raw_background::Service::open(
                crate::models::raw_model_path()?,
            )?);
        }
        self.raw_job = Some(self.raw_service.as_ref().unwrap().request(image, hot)?);
        Ok(())
    }
    pub fn cancel_reconstruction(&mut self) {
        #[cfg(feature = "raw-ml")]
        {
            self.raw_job = None;
        }
    }
    pub fn reconstruction_pending(&self) -> bool {
        #[cfg(feature = "raw-ml")]
        {
            self.raw_job.is_some()
        }
        #[cfg(not(feature = "raw-ml"))]
        {
            false
        }
    }

    /// Standard preview while joint preparation runs; saved output never uses
    /// this temporary path, and neural synthesis layers are withheld until ready.
    pub fn render_preview_region(
        &mut self,
        image: Arc<SensorImage>,
        edits: &Edits,
        region: [f32; 4],
        width: usize,
        height: usize,
    ) -> Result<(Rendered, Option<[usize; 2]>)> {
        if image.reconstruction == Reconstruction::RawNindV1 {
            self.cancel_reconstruction();
            return Ok((
                self.render_region(image, edits, region, width, height)?,
                None,
            ));
        }
        if edits.raw.reconstruction == Reconstruction::Mhc {
            self.cancel_reconstruction();
            return Ok((
                self.render_region(image, edits, region, width, height)?,
                None,
            ));
        }
        #[cfg(feature = "raw-ml")]
        {
            if let Some((original, hot, prepared)) = &self.reconstructed
                && Arc::ptr_eq(original, &image)
                && *hot == edits.raw.hot_pixels
            {
                self.raw_job = None;
                return Ok((
                    self.render_region(prepared.clone(), edits, region, width, height)?,
                    None,
                ));
            }
            self.request_raw_job(image.clone(), edits.raw.hot_pixels)?;
            let polled = self.raw_job.as_ref().unwrap().poll();
            let prepared = match polled {
                Ok(value) => value,
                Err(error) => {
                    self.raw_job = None;
                    return Err(error);
                }
            };
            if let Some(prepared) = prepared {
                self.raw_job = None;
                self.reconstructed = Some((image, edits.raw.hot_pixels, prepared.clone()));
                return Ok((
                    self.render_region(prepared, edits, region, width, height)?,
                    None,
                ));
            }
            let progress = self.raw_job.as_ref().unwrap().progress();
            let mut temporary = edits.clone();
            temporary.raw.reconstruction = Reconstruction::Mhc;
            temporary.raw.denoise = 0.;
            temporary.display.synthesis.clear();
            let rendered =
                self.render_prepared_before_synthesis(image, &temporary, region, width, height)?;
            Ok((rendered, Some(progress)))
        }
        #[cfg(not(feature = "raw-ml"))]
        anyhow::bail!("This recipe uses joint AI reconstruction; build with the raw-ml feature")
    }
    pub fn dimensions(
        &mut self,
        image: Arc<SensorImage>,
        edits: &Edits,
        max_edge: Option<usize>,
    ) -> Result<(usize, usize)> {
        let image = self.prepare_source(image, edits)?;
        Ok(Pipeline::compile(&image, edits)?.dimensions(max_edge))
    }
    pub fn set_document(&mut self, path: std::path::PathBuf) {
        self.layers = Some(crate::synthesis::Layers::new(path));
    }
    fn layer_coverage<'a>(
        &mut self,
        edits: &'a Edits,
        region: [f32; 4],
        aspect: f32,
    ) -> Result<Option<crate::synthesis::LayerCoverage<'a>>> {
        if edits.display.synthesis.is_empty() {
            return Ok(None);
        }
        Ok(Some(
            self.layers
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("Saved fills need the original document path"))?
                .coverage(edits, region, aspect)?,
        ))
    }
    /// Plan corners using sparse interior samples and exact output perimeter pixels.
    pub fn gap_contexts(
        &mut self,
        image: Arc<SensorImage>,
        edits: &Edits,
    ) -> Result<Vec<[f32; 4]>> {
        let image = self.prepare_source(image, edits)?;
        let pipeline = Pipeline::compile(&image, edits)?;
        let (w, h) = pipeline.dimensions(None);
        let coverage = self.layer_coverage(edits, [0., 0., 1., 1.], h as f32 / w as f32)?;
        Ok(crate::synthesis::gap_contexts(w, h, |uv| {
            let alpha = pipeline.sample(uv)[3];
            coverage
                .as_ref()
                .map_or(alpha, |layers| layers.alpha(uv, alpha))
        }))
    }
    /// Keep output-edge gaps represented when a bounded inference crop misses them.
    pub fn cover_gap_mask(
        &mut self,
        image: Arc<SensorImage>,
        edits: &Edits,
        region: [f32; 4],
        mask: &mut [f32],
        size: [usize; 2],
    ) -> Result<()> {
        let image = self.prepare_source(image, edits)?;
        let pipeline = Pipeline::compile(&image, edits)?;
        let (w, h) = pipeline.dimensions(None);
        let coverage = self.layer_coverage(edits, region, h as f32 / w as f32)?;
        crate::synthesis::cover_boundary_gaps(mask, size, region, [w, h], |uv| {
            let alpha = pipeline.sample(uv)[3];
            coverage
                .as_ref()
                .map_or(alpha, |layers| layers.alpha(uv, alpha))
        })
    }
    fn initialize(&mut self) -> Result<()> {
        if self.initialized || self.backend == Backend::Cpu {
            return Ok(());
        }
        self.initialized = true;
        #[cfg(feature = "gpu")]
        {
            let result = std::panic::catch_unwind(|| {
                crate::gpu::GpuRenderer::with_cuda_memory(self.backend, self.cuda_memory)
            })
            .map_err(|_| anyhow::anyhow!("GPU initialization failed"))?;
            match result {
                Ok(gpu) => {
                    self.label = gpu.name().into();
                    self.gpu = Some(gpu);
                }
                Err(e) if self.backend == Backend::Auto => self.label = format!("CPU ({e})"),
                Err(e) => {
                    self.initialized = false;
                    return Err(e);
                }
            }
            Ok(())
        }
        #[cfg(not(feature = "gpu"))]
        {
            if self.backend != Backend::Auto {
                self.initialized = false;
                anyhow::bail!("GPU backends require the gpu Cargo feature");
            }
            Ok(())
        }
    }
    pub fn render(
        &mut self,
        image: Arc<SensorImage>,
        edits: &Edits,
        max_edge: Option<usize>,
    ) -> Result<Rendered> {
        let image = self.prepare_source(image, edits)?;
        let (w, h) = Pipeline::compile(&image, edits)?.dimensions(max_edge);
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
        let mut rendered = self.render_before_synthesis(image, edits, region, width, height)?;
        if !edits.display.synthesis.is_empty() {
            self.layers
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("Saved fills need the original document path"))?
                .apply(edits, &mut rendered, region)?;
        }
        Ok(rendered)
    }
    pub fn render_before_synthesis(
        &mut self,
        image: Arc<SensorImage>,
        edits: &Edits,
        region: [f32; 4],
        width: usize,
        height: usize,
    ) -> Result<Rendered> {
        let image = self.prepare_source(image, edits)?;
        self.render_prepared_before_synthesis(image, edits, region, width, height)
    }
    fn render_prepared_before_synthesis(
        &mut self,
        image: Arc<SensorImage>,
        edits: &Edits,
        region: [f32; 4],
        width: usize,
        height: usize,
    ) -> Result<Rendered> {
        if let Err(e) = self.initialize() {
            if self.backend != Backend::Auto {
                return Err(e);
            }
            self.label = format!("CPU ({e})");
        }
        #[cfg(feature = "gpu")]
        if let Some(gpu) = &mut self.gpu {
            match gpu.render_region(image.clone(), edits, region, width, height) {
                Ok(image) => {
                    self.label = gpu.name().into();
                    return Ok(image);
                }
                Err(e) if self.backend == Backend::Auto => self.label = format!("CPU ({e})"),
                Err(e) => return Err(e),
            }
        }
        Pipeline::compile(&image, edits)?.render_region(region, width, height)
    }

    #[cfg(feature = "moebius")]
    /// Generate current canvas targets, or skip a geometric region already covered.
    pub fn generate_fill(
        &mut self,
        image: Arc<SensorImage>,
        edits: &Edits,
        region: [f32; 4],
        dabs: Vec<crate::synthesis::MaskDab>,
        fill_gaps: bool,
        settings: &crate::moebius::Sampling,
    ) -> Result<Option<crate::synthesis::GeneratedFill>> {
        ensure_region(region)?;
        settings.validate()?;
        let image = self.prepare_source(image, edits)?;
        let (w, h) = Pipeline::compile(&image, edits)?.dimensions(None);
        let color_revision = image.metadata.color_revision;
        let mut context_edits = edits.clone();
        let hash = crate::synthesis::recipe_hash(edits)?;
        context_edits.display.synthesis.retain(|fill| {
            fill.recipe_sha256 == hash && fill.source_color_revision == color_revision
        });
        let context = self.render_region(image.clone(), &context_edits, region, 512, 512)?;
        let mut mask =
            crate::synthesis::context_mask(&context, region, &dabs, fill_gaps, h as f32 / w as f32);
        let mut targets = mask.clone();
        crate::synthesis::clip_mask_to_canvas(&mut targets, [512, 512], region)?;
        if fill_gaps {
            self.cover_gap_mask(image, &context_edits, region, &mut targets, [512, 512])?;
        }
        if !targets.iter().any(|v| *v > 0.) && fill_gaps {
            return Ok(None);
        }
        anyhow::ensure!(
            targets.iter().any(|v| *v > 0.),
            "No painted pixels or geometric gaps in this region"
        );
        for (mask, target) in mask.iter_mut().zip(targets) {
            *mask = mask.max(target);
        }
        if self.moebius.is_none() {
            let path = crate::models::cache_dir()?.join("models/moebius/torchscript");
            self.moebius = Some(crate::moebius::Moebius::open(
                &path,
                crate::moebius::InferenceDevice::Auto,
            )?);
        }
        let mut generated = self
            .moebius
            .as_ref()
            .unwrap()
            .inpaint_512(&context, &mask, settings)?;
        // Exact target membership is evaluated at output resolution when applying
        // the layer. Keep context opaque so subpixel gaps and brush boundaries
        // can interpolate known/generated colors without leaving alpha holes.
        for pixel in &mut generated.pixels {
            pixel[3] = 1.;
        }
        let layers = self
            .layers
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Set the original path before generating a fill"))?;
        let source = layers.source_hash()?.to_owned();
        let (asset, sha256) = layers.store(&generated)?;
        Ok(Some(crate::synthesis::GeneratedFill {
            region,
            dabs,
            fill_gaps,
            feather: 0.15,
            steps: settings.steps,
            seed: settings.seed,
            sampling: settings.parameters(),
            asset,
            sha256,
            source_sha256: source,
            source_color_revision: color_revision,
            recipe_sha256: crate::synthesis::recipe_hash(edits)?,
            model: "moebius-scene-2026-v1".into(),
        }))
    }
}

#[cfg(feature = "moebius")]
fn ensure_region(region: [f32; 4]) -> Result<()> {
    anyhow::ensure!(
        region.iter().all(|v| v.is_finite()) && region[2] > 0. && region[3] > 0.,
        "Invalid synthesis context"
    );
    Ok(())
}
