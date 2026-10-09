//! Display discovery is separate from photo processing; profiles never enter the edit recipe.
use crate::{color::OutputSpace, export, input::pixel_count, pipeline::Rendered};
use anyhow::{Context, Result, ensure};
use lcms2::{ColorSpaceSignature, Flags, Intent, PixelFormat, Profile, Transform};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc};

pub mod hdr;
mod platform;
#[cfg(target_os = "linux")]
mod wayland_surface;
#[cfg(target_os = "linux")]
pub use wayland_surface::WaylandSurface;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Desktop {
    X11,
    Wayland,
    Mac,
    Windows(isize),
    #[default]
    Other,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Monitor {
    pub name: Option<String>,
    pub rect: [i32; 4],
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub desktop: Desktop,
    pub monitor: Option<Monitor>,
    pub custom: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icc {
    pub bytes: Arc<[u8]>,
    pub digest: [u8; 32],
}
impl Icc {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let profile = Profile::new_icc(&bytes).context("Reading display ICC profile")?;
        ensure!(
            profile.color_space() == ColorSpaceSignature::RgbData,
            "Display profile must describe RGB"
        );
        let digest = Sha256::digest(&bytes).into();
        Ok(Self {
            bytes: bytes.into(),
            digest,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub icc: Option<Icc>,
    pub label: String,
}
impl Default for Resolved {
    fn default() -> Self {
        Self {
            icc: None,
            label: "Automatic: sRGB fallback".into(),
        }
    }
}
pub fn discover(request: &Request) -> Result<Resolved> {
    if let Some(path) = &request.custom {
        #[cfg(target_os = "linux")]
        if request.desktop == Desktop::Wayland && platform::wayland_managed() {
            anyhow::bail!(
                "This Wayland compositor manages display colour. Use Automatic; a legacy ICC override would apply the monitor conversion twice."
            );
        }
        let icc = Icc::from_bytes(
            std::fs::read(path)
                .with_context(|| format!("Opening display profile {}", path.display()))?,
        )?;
        return Ok(Resolved {
            icc: Some(icc),
            label: format!(
                "Custom: {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
        });
    }
    platform::discover(request)
}

/// Cache a transform on the photo worker and transform RGBA directly, retaining alpha.
#[derive(Default)]
pub struct Encoder {
    cached: Option<([u8; 32], RgbaTransform)>,
}
type RgbaTransform = Transform<[f32; 4], [u8; 4]>;
impl Encoder {
    pub fn encode(&mut self, image: &Rendered, profile: Option<&Icc>) -> Result<Vec<u8>> {
        ensure!(
            image.pixels.len() == pixel_count(image.width, image.height, 1)?,
            "Invalid display raster"
        );
        let Some(profile) = profile else {
            return Ok(export::rgba8(image, OutputSpace::Srgb));
        };
        if self
            .cached
            .as_ref()
            .is_none_or(|(key, _)| *key != profile.digest)
        {
            let input = export::profile(OutputSpace::LinearSrgb)?;
            let output = Profile::new_icc(&profile.bytes)?;
            let transform = Transform::new_flags(
                &input,
                PixelFormat::RGBA_FLT,
                &output,
                PixelFormat::RGBA_8,
                Intent::RelativeColorimetric,
                Flags::COPY_ALPHA,
            )?;
            self.cached = Some((profile.digest, transform));
        }
        let mut rgba = vec![0u8; pixel_count(image.width, image.height, 4)?];
        self.cached
            .as_ref()
            .unwrap()
            .1
            .transform_pixels(&image.pixels, bytemuck::cast_slice_mut(&mut rgba));
        Ok(rgba)
    }
}

pub fn desktop(handle: raw_window_handle::RawWindowHandle) -> Desktop {
    use raw_window_handle::RawWindowHandle as H;
    match handle {
        H::Xlib(_) | H::Xcb(_) => Desktop::X11,
        H::Wayland(_) => Desktop::Wayland,
        H::AppKit(_) => Desktop::Mac,
        H::Win32(h) => Desktop::Windows(h.hwnd.get()),
        _ => Desktop::Other,
    }
}

/// Configure the owned GUI surface on the UI thread, without changing monitor settings.
pub fn set_surface_encoding(
    handle: raw_window_handle::RawWindowHandle,
    managed: bool,
) -> Result<usize> {
    platform::set_surface_encoding(handle, managed)
}

pub fn overlap(a: [i32; 4], b: [i32; 4]) -> i64 {
    let dx = (i64::from(a[0]) + i64::from(a[2])).min(i64::from(b[0]) + i64::from(b[2]))
        - i64::from(a[0]).max(i64::from(b[0]));
    let dy = (i64::from(a[1]) + i64::from(a[3])).min(i64::from(b[1]) + i64::from(b[3]))
        - i64::from(a[1]).max(i64::from(b[1]));
    dx.max(0) * dy.max(0)
}
