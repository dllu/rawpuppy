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

pub struct Renderer {
    backend: Backend,
    initialized: bool,
    #[cfg(feature = "gpu")]
    gpu: Option<crate::gpu::GpuRenderer>,
    label: String,
}
impl Renderer {
    pub fn new(backend: Backend) -> Self {
        Self {
            backend,
            initialized: false,
            #[cfg(feature = "gpu")]
            gpu: None,
            label: "CPU".into(),
        }
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    fn initialize(&mut self) -> Result<()> {
        if self.initialized || self.backend == Backend::Cpu {
            return Ok(());
        }
        self.initialized = true;
        #[cfg(feature = "gpu")]
        {
            let result = std::panic::catch_unwind(|| crate::gpu::GpuRenderer::new(self.backend))
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
}
