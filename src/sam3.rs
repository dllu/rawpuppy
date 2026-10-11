//! Optional local SAM 3 architecture/vegetation masks; no photograph leaves the process.
use crate::{color, ml_runtime::InferenceDevice, models, perspective::Mask, pipeline::Rendered};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tch::{CModule, Device, Kind, Tensor};

pub const REVISION: &str = "3c879f39826c281e95690f02c7821c4de09afae7";
pub const WEIGHTS_SHA256: &str = "6d06f0a5f84e435071fe6603e61d0b4cc7b40e0d39d487cfd4d67d8cc11cc14a";
#[derive(Deserialize)]
struct Manifest {
    version: u32,
    revision: String,
    portable_device: bool,
    prompts: Vec<String>,
    input_edge: usize,
    mean: [f32; 3],
    std: [f32; 3],
    weights_sha256: String,
    graph_sha256: String,
    checks: Vec<Check>,
}
#[derive(Deserialize)]
struct Check {
    max_absolute_error: f64,
    threshold_disagreements: usize,
}

pub fn directory() -> Result<PathBuf> {
    Ok(models::cache_dir()?.join("models/sam3/architecture-v1"))
}

pub struct Model {
    module: CModule,
    device: Device,
    mean: [f32; 3],
    std: [f32; 3],
}
impl Model {
    pub fn device(&self) -> Device {
        self.device
    }
    pub fn open(path: &Path, preference: InferenceDevice) -> Result<Self> {
        let m: Manifest = serde_json::from_slice(
            &std::fs::read(path.join("manifest.json"))
                .context("Prepare the local SAM 3 model with tools/export_sam3.py")?,
        )?;
        ensure!(
            m.version == 1
                && m.portable_device
                && m.revision == REVISION
                && m.prompts == ["building", "wall", "tree", "person"]
                && m.input_edge == 1008
                && m.weights_sha256 == WEIGHTS_SHA256
                && m.checks.len() >= 3
                && m.checks.iter().all(|c| c.max_absolute_error.is_finite()
                    && c.max_absolute_error <= 0.001
                    && c.threshold_disagreements == 0)
                && m.mean.iter().all(|x| x.is_finite())
                && m.std.iter().all(|x| x.is_finite() && *x > 0.),
            "Unverified SAM 3 architecture graph manifest"
        );
        let graph = path.join("architecture.pt");
        ensure!(
            models::sha256(&graph)? == m.graph_sha256,
            "SAM 3 graph checksum mismatch"
        );
        ensure!(
            path.join("LICENSE.txt").is_file() && path.join("NOTICE.txt").is_file(),
            "SAM 3 license and attribution are required"
        );
        let device = preference.resolve()?;
        let mut module = CModule::load_on_device(graph, device)?;
        module.set_eval();
        Ok(Self {
            module,
            device,
            mean: m.mean,
            std: m.std,
        })
    }
    pub fn mask(&self, image: &Rendered) -> Result<Mask> {
        ensure!(
            image.width > 0
                && image.height > 0
                && image.width <= 2048
                && image.height <= 2048
                && image.pixels.len() == image.width * image.height,
            "SAM 3 expects a bounded analysis preview"
        );
        let bytes: Vec<u8> = image
            .pixels
            .iter()
            .flat_map(|p| {
                [p[0], p[1], p[2]]
                    .map(|x| (color::srgb_encode(x).clamp(0., 1.) * 255.).round() as u8)
            })
            .collect();
        let rgb =
            image::RgbImage::from_raw(image.width as u32, image.height as u32, bytes).unwrap();
        let resized =
            image::imageops::resize(&rgb, 1008, 1008, image::imageops::FilterType::Triangle);
        let count = 1008 * 1008;
        let mut planar = vec![0.; count * 3];
        for (i, p) in resized.pixels().enumerate() {
            for c in 0..3 {
                planar[c * count + i] = (f32::from(p[c]) / 255. - self.mean[c]) / self.std[c];
            }
        }
        let input = Tensor::from_slice(&planar)
            .view([1, 3, 1008, 1008])
            .to_device(self.device);
        let _guard = tch::no_grad_guard();
        let output = self.module.forward_ts(&[input])?;
        let shape = output.size();
        ensure!(
            shape.len() == 3
                && shape[0] == 4
                && shape[1] > 0
                && shape[2] > 0
                && shape[1] <= 1008
                && shape[2] <= 1008,
            "Unexpected SAM 3 mask output"
        );
        let output = output
            .unsqueeze(0)
            .upsample_bilinear2d([image.height as i64, image.width as i64], false, None, None)
            .to_device(Device::Cpu)
            .to_kind(Kind::Float)
            .contiguous();
        let n = image.pixels.len();
        let mut data = vec![0_f32; n * 4];
        output.copy_data(&mut data, n * 4);
        ensure!(
            data.iter().all(|x| x.is_finite() && (0. ..=1.).contains(x)),
            "SAM 3 returned invalid probabilities"
        );
        let mut values = vec![0.; n];
        for y in 0..image.height {
            for x in 0..image.width {
                let mut positive = false;
                let mut excluded = false;
                // Accommodate a two-pixel approximate façade boundary, but expand
                // excluded tree/person masks too. Never derive lines from masks.
                for yy in y.saturating_sub(2)..=(y + 2).min(image.height - 1) {
                    for xx in x.saturating_sub(2)..=(x + 2).min(image.width - 1) {
                        let i = yy * image.width + xx;
                        positive |= data[i].max(data[n + i]) >= 0.5;
                        excluded |= data[2 * n + i].max(data[3 * n + i]) >= 0.5;
                    }
                }
                values[y * image.width + x] = if positive && !excluded { 1. } else { 0. };
            }
        }
        Ok(Mask {
            width: image.width,
            height: image.height,
            values,
        })
    }
}

pub fn install(path: &Path) -> Result<PathBuf> {
    Model::open(path, InferenceDevice::Cpu)?;
    let destination = directory()?;
    if destination.exists() {
        Model::open(&destination, InferenceDevice::Cpu)?;
        ensure!(
            models::sha256(&destination.join("architecture.pt"))?
                == models::sha256(&path.join("architecture.pt"))?,
            "A different SAM 3 graph is already installed"
        );
        return Ok(destination);
    }
    let parent = destination.parent().context("Missing SAM 3 cache parent")?;
    std::fs::create_dir_all(parent)?;
    let temporary = tempfile::tempdir_in(parent)?;
    for name in [
        "architecture.pt",
        "manifest.json",
        "LICENSE.txt",
        "NOTICE.txt",
    ] {
        std::fs::copy(path.join(name), temporary.path().join(name))?;
    }
    Model::open(temporary.path(), InferenceDevice::Cpu)?;
    std::fs::rename(temporary.path(), &destination)?;
    Ok(destination)
}
