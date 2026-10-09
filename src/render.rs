//! Backend selection, conservative CPU fallback, and one rendering interface for CLI and GUI.
use crate::{
    edits::Edits,
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
        }
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn set_document(&mut self, path: std::path::PathBuf) {
        self.layers = Some(crate::synthesis::Layers::new(path));
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
    pub fn generate_fill(
        &mut self,
        image: Arc<SensorImage>,
        edits: &Edits,
        region: [f32; 4],
        dabs: Vec<crate::synthesis::MaskDab>,
        fill_gaps: bool,
        settings: &crate::moebius::Sampling,
    ) -> Result<crate::synthesis::GeneratedFill> {
        ensure_region(region)?;
        let (w, h) = Pipeline::compile(&image, edits)?.dimensions(None);
        let color_revision = image.metadata.color_revision;
        let mut context_edits = edits.clone();
        let hash = crate::synthesis::recipe_hash(edits)?;
        context_edits.display.synthesis.retain(|fill| {
            fill.recipe_sha256 == hash && fill.source_color_revision == color_revision
        });
        let context = self.render_region(image, &context_edits, region, 512, 512)?;
        let mask =
            crate::synthesis::context_mask(&context, region, &dabs, fill_gaps, h as f32 / w as f32);
        anyhow::ensure!(
            mask.iter().any(|v| *v > 0.),
            "No painted pixels or geometric gaps in this region"
        );
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
        Ok(crate::synthesis::GeneratedFill {
            region,
            dabs,
            fill_gaps,
            steps: settings.steps,
            seed: settings.seed,
            asset,
            sha256,
            source_sha256: source,
            source_color_revision: color_revision,
            recipe_sha256: crate::synthesis::recipe_hash(edits)?,
            model: "moebius-scene-2026-v1".into(),
        })
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
