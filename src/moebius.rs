//! Native Rust Moebius sampling over prepared, verified inference graphs.
use crate::{color, models, pipeline::Rendered};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, sync::Mutex};
use tch::{CModule, Device, Kind, Tensor};

static SAMPLER_LOCK: Mutex<()> = Mutex::new(());

pub use crate::ml_runtime::InferenceDevice;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Sampling {
    pub steps: usize,
    pub seed: i64,
    pub guidance: f64,
    pub strength: f64,
    pub noise_offset: f64,
}
impl Default for Sampling {
    fn default() -> Self {
        Self {
            steps: 20,
            seed: 0,
            guidance: 2.,
            strength: 1.,
            noise_offset: 0.0357,
        }
    }
}
impl Sampling {
    pub fn parameters(&self) -> crate::synthesis::SamplingParameters {
        crate::synthesis::SamplingParameters {
            guidance: self.guidance,
            strength: self.strength,
            noise_offset: self.noise_offset,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (2..=1000).contains(&self.steps),
            "Diffusion steps must be between 2 and 1000"
        );
        self.parameters().validate(self.steps)
    }
}

#[derive(Deserialize)]
struct Manifest {
    version: u32,
    size: usize,
    scaling_factor: f64,
    source_hashes: BTreeMap<String, String>,
    modules: BTreeMap<String, Graph>,
}
#[derive(Deserialize)]
struct Graph {
    file: String,
    sha256: String,
    max_abs_error: f64,
}

pub struct Moebius {
    encoder: CModule,
    decoder: CModule,
    denoiser: CModule,
    device: Device,
    scaling: f64,
    alphas: Vec<f32>,
}
impl Moebius {
    pub fn open(directory: &Path, preference: InferenceDevice) -> Result<Self> {
        let manifest: Manifest = serde_json::from_slice(
            &std::fs::read(directory.join("manifest.json"))
                .context("Moebius graphs are not prepared; run tools/export_moebius.py")?,
        )?;
        ensure!(
            manifest.version == 1 && manifest.size == 512,
            "Unsupported Moebius graph manifest"
        );
        ensure!(
            manifest.scaling_factor.is_finite() && (manifest.scaling_factor - 0.13025).abs() < 1e-6,
            "Unexpected Moebius VAE scale"
        );
        for (file, hash) in [
            (
                "ft_places2/diffusion_pytorch_model.bin",
                models::MOEBIUS_WEIGHTS_SHA256,
            ),
            (
                "vae/diffusion_pytorch_model.bin",
                models::MOEBIUS_VAE_SHA256,
            ),
            ("vae/config.json", models::MOEBIUS_VAE_CONFIG_SHA256),
        ] {
            ensure!(
                manifest.source_hashes.get(file).is_some_and(|v| v == hash),
                "Moebius source checkpoint identity differs"
            );
        }
        let device = preference.resolve()?;
        let load = |name: &str| -> Result<CModule> {
            let graph = manifest
                .modules
                .get(name)
                .context("Missing Moebius graph")?;
            ensure!(
                graph.file == format!("{name}.pt"),
                "Unexpected Moebius graph filename"
            );
            ensure!(
                graph.max_abs_error.is_finite() && graph.max_abs_error <= 0.0001,
                "Moebius graph failed its numerical preparation check"
            );
            let path = directory.join(&graph.file);
            ensure!(
                models::sha256(&path)? == graph.sha256,
                "Moebius graph checksum mismatch: {name}"
            );
            let mut module = CModule::load_on_device(&path, device)?;
            module.set_eval();
            Ok(module)
        };
        // Match the authors' CPU-created, FP32 scaled-linear beta schedule exactly.
        let beta = Tensor::linspace(
            0.00085f64.sqrt(),
            0.012f64.sqrt(),
            1000,
            (Kind::Float, Device::Cpu),
        )
        .square();
        let alpha = (Tensor::ones_like(&beta) - beta).cumprod(0, Kind::Float);
        let mut alphas = vec![0f32; 1000];
        alpha.copy_data(&mut alphas, 1000);
        Ok(Self {
            encoder: load("encoder")?,
            decoder: load("decoder")?,
            denoiser: load("denoiser")?,
            device,
            scaling: manifest.scaling_factor,
            alphas,
        })
    }
    pub fn device(&self) -> Device {
        self.device
    }

    fn encode(&self, input: &Tensor) -> Result<Tensor> {
        let moments = self.encoder.forward_ts(&[input])?;
        ensure!(
            moments.size() == [1, 8, 64, 64],
            "Invalid Moebius encoder output"
        );
        let mean = moments.narrow(1, 0, 4);
        let std = moments.narrow(1, 4, 4);
        Ok((&mean + std * mean.randn_like()) * self.scaling)
    }

    /// Generate exactly one bounded context region, preserving every unmasked sample.
    pub fn inpaint_512(
        &self,
        image: &Rendered,
        mask: &[f32],
        settings: &Sampling,
    ) -> Result<Rendered> {
        const N: usize = 512 * 512;
        settings.validate()?;
        ensure!(
            image.width == 512 && image.height == 512 && image.pixels.len() == N && mask.len() == N,
            "Moebius requires a 512×512 context and matching mask"
        );
        ensure!(
            image.pixels.iter().flatten().all(|v| v.is_finite()),
            "Nonfinite Moebius input"
        );
        ensure!(
            mask.iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "Invalid Moebius mask"
        );
        if !mask.iter().any(|v| *v > 0.) {
            return Ok(Rendered {
                width: 512,
                height: 512,
                pixels: image.pixels.clone(),
            });
        }
        with_seeded_sampler(settings.seed, || self.sample_512(image, mask, settings))
    }

    fn sample_512(&self, image: &Rendered, mask: &[f32], settings: &Sampling) -> Result<Rendered> {
        const N: usize = 512 * 512;
        // Graphs were frozen and numerically verified during preparation. Runtime
        // profiling/fusion can alter those numerics and adds a long first-call stall.
        tch::jit::f_set_graph_executor_optimize(false)?;
        let mut rgb = vec![0f32; 3 * N];
        for (i, pixel) in image.pixels.iter().enumerate() {
            for c in 0..3 {
                rgb[c * N + i] = color::srgb_encode(pixel[c]).clamp(0., 1.) * 2. - 1.;
            }
        }
        let rgb = Tensor::from_slice(&rgb)
            .reshape([1, 3, 512, 512])
            .to(self.device);
        let binary: Vec<f32> = mask.iter().map(|v| if *v > 0. { 1. } else { 0. }).collect();
        let mask_tensor = Tensor::from_slice(&binary)
            .reshape([1, 1, 512, 512])
            .to(self.device);
        let latents = self.encode(&rgb)?;
        let masked_latents =
            self.encode(&(&rgb * (Tensor::ones_like(&mask_tensor) - &mask_tensor)))?;
        let latent_mask = mask_tensor.upsample_nearest2d([64, 64], None, None);
        let noise = latents.randn_like()
            + Tensor::randn([1, 4, 1, 1], (Kind::Float, self.device)) * settings.noise_offset;
        let schedule = timesteps(settings);
        let first = schedule[0];
        let mut sample = if settings.strength < 1. {
            &latents * (self.alphas[first].sqrt() as f64)
                + noise * ((1. - self.alphas[first]).sqrt() as f64)
        } else {
            noise
        };
        let ids: Vec<i64> = (10..20).chain(0..10).collect();
        let ids = Tensor::from_slice(&ids).reshape([2, 10]).to(self.device);
        let duplicated_mask = Tensor::cat(&[&latent_mask, &latent_mask], 0);
        let duplicated_context = Tensor::cat(&[&masked_latents, &masked_latents], 0);
        let stride = 1000 / settings.steps;
        for timestep in schedule {
            let duplicate = Tensor::cat(&[&sample, &sample], 0);
            let input = Tensor::cat(&[&duplicate, &duplicated_mask, &duplicated_context], 1);
            let time = Tensor::from_slice(&[timestep as i64]).to(self.device);
            let prediction = self.denoiser.forward_ts(&[&input, &time, &ids])?;
            ensure!(
                prediction.size() == [2, 4, 64, 64],
                "Invalid Moebius denoiser output"
            );
            let unconditioned = prediction.narrow(0, 0, 1);
            let conditioned = prediction.narrow(0, 1, 1);
            let epsilon = &unconditioned + (&conditioned - &unconditioned) * settings.guidance;
            let alpha = self.alphas[timestep];
            let previous = if timestep >= stride {
                self.alphas[timestep - stride]
            } else {
                1.
            };
            let original =
                (&sample - &epsilon * ((1. - alpha).sqrt() as f64)) / (alpha.sqrt() as f64);
            sample =
                original * (previous.sqrt() as f64) + epsilon * ((1. - previous).sqrt() as f64);
        }
        let output = self.decoder.forward_ts(&[sample / self.scaling])?;
        ensure!(
            output.size() == [1, 3, 512, 512],
            "Invalid Moebius decoder output"
        );
        let output = ((output + 1.) / 2.)
            .clamp(0., 1.)
            .to(Device::Cpu)
            .contiguous();
        let mut data = vec![0f32; 3 * N];
        output.copy_data(&mut data, 3 * N);
        ensure!(
            data.iter().all(|v| v.is_finite()),
            "Moebius produced nonfinite output"
        );
        let mut pixels = image.pixels.clone();
        for (i, pixel) in pixels.iter_mut().enumerate() {
            if mask[i] <= 0. {
                continue;
            }
            for c in 0..3 {
                let generated = color::srgb_decode(data[c * N + i]);
                pixel[c] += mask[i] * (generated - pixel[c]);
            }
            pixel[3] += mask[i] * (1. - pixel[3]);
        }
        Ok(Rendered {
            width: 512,
            height: 512,
            pixels,
        })
    }
}

fn with_seeded_sampler<T>(seed: i64, sample: impl FnOnce() -> Result<T>) -> Result<T> {
    // LibTorch's RNG is process-local and global; serialize our seeded sampler calls.
    let _lock = SAMPLER_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Diffusion sampler lock poisoned"))?;
    // tch's convenient tensor operators unwrap recoverable LibTorch errors.
    // Catch their Rust panics before the guard is dropped, keeping the photo
    // worker alive and preventing the process-wide RNG mutex from poisoning.
    // Model weights and input slices are immutable during inference; all
    // temporary tensors and the thread-local gradient guard unwind here.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _no_grad = tch::no_grad_guard();
        tch::manual_seed(seed);
        sample()
    }))
    .unwrap_or_else(|payload| {
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("Unknown tensor operation failure");
        Err(anyhow::anyhow!("Moebius sampling failed: {message}"))
    })
}

fn timesteps(settings: &Sampling) -> Vec<usize> {
    let stride = 1000 / settings.steps;
    let start = settings.steps - (settings.steps as f64 * settings.strength).floor() as usize;
    (0..settings.steps)
        .rev()
        .skip(start)
        .map(|t| t * stride)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_tensor_failure_returns_an_error_and_the_next_seeded_call_recovers() {
        let random = || {
            with_seeded_sampler(127, || {
                Ok(Vec::<f32>::try_from(Tensor::randn(
                    [8],
                    (Kind::Float, Device::Cpu),
                ))?)
            })
            .unwrap()
        };
        let before = random();
        let failure = std::panic::catch_unwind(|| {
            with_seeded_sampler(999, || {
                // A recoverable real LibTorch error without exhausting memory
                // or touching CUDA: two values cannot be reshaped into three.
                let _invalid = Tensor::from_slice(&[1f32, 2.]).reshape([3]);
                Ok(())
            })
        });
        if failure.is_err() {
            SAMPLER_LOCK.clear_poison();
        }
        let failure =
            failure.expect("Tensor panic escaped the sampler and would stop the photo worker");
        assert!(failure.unwrap_err().to_string().contains("invalid"));
        assert!(!SAMPLER_LOCK.is_poisoned());
        assert_eq!(before, random(), "The next call did not reset its seed");
        let leaf = Tensor::from_slice(&[1f32]).set_requires_grad(true);
        assert!(
            (&leaf + 1.).requires_grad(),
            "No-grad state leaked out of the sampler"
        );
    }
    #[test]
    fn scheduler_matches_leading_ddim_with_img2img_strength() {
        assert_eq!(
            timesteps(&Sampling::default()),
            (0..20).rev().map(|t| t * 50).collect::<Vec<_>>()
        );
        assert_eq!(
            timesteps(&Sampling {
                strength: 0.99,
                ..Default::default()
            }),
            (0..19).rev().map(|t| t * 50).collect::<Vec<_>>()
        );
        let s = Sampling {
            steps: 10,
            strength: 1.,
            ..Default::default()
        };
        assert_eq!(
            timesteps(&s),
            vec![900, 800, 700, 600, 500, 400, 300, 200, 100, 0]
        );
    }
}
