//! Pinned local model catalog. Models are separate from immutable photographs and recipes.
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const LAMA_SHA256: &str = "1faef5301d78db7dda502fe59966957ec4b79dd64e16f03ed96913c7a4eb68d6";
pub const LAMA_URL: &str = "https://huggingface.co/Carve/LaMa-ONNX/resolve/c3c0c9e468934d62e79c329e35d82dd09ff8c444/lama_fp32.onnx";
pub const MOEBIUS_WEIGHTS_URL: &str = "https://huggingface.co/hustvl/Moebius/resolve/cd01f47fb648219d3fa605806c5ce00b713faa5e/ft_places2/diffusion_pytorch_model.bin";
pub const MOEBIUS_WEIGHTS_SHA256: &str =
    "6525afb888e55f9b5c74fa0a5d19ca0762d720d6c716fb0f8422fbeb6868a09a";
pub const MOEBIUS_VAE_URL: &str = "https://huggingface.co/hustvl/PixelHacker/resolve/012fd343158936a265b8a0ee38a791a7a2841f45/vae/diffusion_pytorch_model.bin";
pub const MOEBIUS_VAE_SHA256: &str =
    "a59d7ea697f2942d22002dc3469e8c53db807a6b78f7f5ec03bd4c1f70f98efe";
pub const MOEBIUS_VAE_CONFIG_URL: &str = "https://huggingface.co/hustvl/PixelHacker/resolve/012fd343158936a265b8a0ee38a791a7a2841f45/vae/config.json";
pub const MOEBIUS_VAE_CONFIG_SHA256: &str =
    "caeb94d8607dbd24acd13719c6823e2525509de8a21eb6b56cf93d5267c04694";

pub fn cache_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path).join("rawpuppy"));
    }
    #[cfg(target_os = "windows")]
    if let Some(path) = std::env::var_os("LOCALAPPDATA") {
        return Ok(PathBuf::from(path).join("rawpuppy"));
    }
    let home =
        std::env::var_os("HOME").context("Cannot locate the model cache; set XDG_CACHE_HOME")?;
    #[cfg(target_os = "macos")]
    let root = PathBuf::from(home).join("Library/Caches");
    #[cfg(not(target_os = "macos"))]
    let root = PathBuf::from(home).join(".cache");
    Ok(root.join("rawpuppy"))
}
pub fn lama_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join("models/lama_fp32.onnx"))
}
pub fn sha256(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
pub fn verify_lama(path: &Path) -> Result<()> {
    ensure!(
        sha256(path)? == LAMA_SHA256,
        "LaMa model checksum differs from the pinned Apache-2.0 release"
    );
    Ok(())
}
pub fn fetch_lama() -> Result<PathBuf> {
    let path = lama_path()?;
    fetch_pinned(&path, LAMA_URL, LAMA_SHA256)?;
    Ok(path)
}
pub fn fetch_moebius() -> Result<PathBuf> {
    let root = cache_dir()?.join("models/moebius");
    for (name, url, hash) in [
        (
            "ft_places2/diffusion_pytorch_model.bin",
            MOEBIUS_WEIGHTS_URL,
            MOEBIUS_WEIGHTS_SHA256,
        ),
        (
            "vae/diffusion_pytorch_model.bin",
            MOEBIUS_VAE_URL,
            MOEBIUS_VAE_SHA256,
        ),
        (
            "vae/config.json",
            MOEBIUS_VAE_CONFIG_URL,
            MOEBIUS_VAE_CONFIG_SHA256,
        ),
    ] {
        fetch_pinned(&root.join(name), url, hash)?;
    }
    Ok(root)
}
fn fetch_pinned(path: &Path, url: &str, hash: &str) -> Result<()> {
    if path.is_file() {
        ensure!(
            sha256(path)? == hash,
            "Cached model artifact checksum differs from the pinned release: {}",
            path.display()
        );
        return Ok(());
    }
    let parent = path.parent().unwrap();
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut response = ureq::get(url)
        .call()
        .context("Downloading the pinned local model artifact")?;
    std::io::copy(&mut response.body_mut().as_reader(), &mut temporary)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    ensure!(
        sha256(temporary.path())? == hash,
        "Downloaded model artifact checksum differs from the pinned release"
    );
    temporary.persist(path)?;
    Ok(())
}
