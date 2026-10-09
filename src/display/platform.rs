use super::{Desktop, Request, Resolved};
use anyhow::Result;

pub fn discover(request: &Request) -> Result<Resolved> {
    match request.desktop {
        Desktop::X11 =>
        {
            #[cfg(target_os = "linux")]
            if let Some(icc) = linux::x11(request)? {
                return Ok(icc);
            }
        }
        Desktop::Wayland => {
            #[cfg(target_os = "linux")]
            return linux::wayland(request);
        }
        Desktop::Mac => {
            return Ok(Resolved {
                icc: None,
                label: "Automatic: ColorSync sRGB surface".into(),
            });
        }
        Desktop::Windows(hwnd) => {
            #[cfg(target_os = "windows")]
            if let Some(path) = windows::profile(hwnd)? {
                return Ok(Resolved {
                    icc: Some(super::Icc::from_bytes(std::fs::read(&path)?)?),
                    label: format!(
                        "Automatic: {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                });
            }
            #[cfg(not(target_os = "windows"))]
            let _ = hwnd;
        }
        Desktop::Other => {}
    }
    Ok(Resolved::default())
}

pub fn set_surface_encoding(
    handle: raw_window_handle::RawWindowHandle,
    managed: bool,
) -> Result<usize> {
    #[cfg(target_os = "macos")]
    return mac::set_surface(handle, managed);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (handle, managed);
        Ok(0)
    }
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub(super) fn wayland_managed() -> bool {
    linux::wayland_managed()
}
