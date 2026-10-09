//! Device selection shared by optional native learning models.
use anyhow::{Result, ensure};
use tch::Device;

#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum InferenceDevice {
    #[default]
    Auto,
    Cpu,
    Cuda,
    Mps,
}

impl InferenceDevice {
    pub(crate) fn resolve(self) -> Result<Device> {
        #[cfg(target_os = "linux")]
        if !matches!(self, Self::Cpu) {
            // Retain LibTorch's optional CUDA registration library for the
            // process lifetime; linkers can otherwise discard its initializer.
            static CUDA: std::sync::OnceLock<Option<libloading::Library>> =
                std::sync::OnceLock::new();
            CUDA.get_or_init(|| {
                // SAFETY: use the configured runtime's library through the
                // system loader, retaining it while tensors/modules can exist.
                unsafe { libloading::Library::new("libtorch_cuda.so").ok() }
            });
        }
        match self {
            Self::Auto if tch::Cuda::is_available() => Ok(Device::Cuda(0)),
            Self::Auto if tch::utils::has_mps() => Ok(Device::Mps),
            Self::Auto | Self::Cpu => Ok(Device::Cpu),
            Self::Cuda => {
                ensure!(
                    tch::Cuda::is_available(),
                    "CUDA inference device unavailable"
                );
                Ok(Device::Cuda(0))
            }
            Self::Mps => {
                ensure!(tch::utils::has_mps(), "Metal inference device unavailable");
                Ok(Device::Mps)
            }
        }
    }
}
