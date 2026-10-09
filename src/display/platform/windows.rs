use anyhow::Result;
use std::{os::windows::ffi::OsStringExt, path::PathBuf};
use windows_sys::Win32::Graphics::Gdi::{
    CreateDCW, DeleteDC, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFOEXW,
    MonitorFromWindow,
};
use windows_sys::Win32::UI::ColorSystem::GetICMProfileW;

pub fn profile(hwnd: isize) -> Result<Option<PathBuf>> {
    // SAFETY: Win32 validates opaque window/monitor handles; structures and
    // buffers are initialized with their documented sizes and stay live for calls.
    unsafe {
        let monitor = MonitorFromWindow(hwnd as _, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFOEXW = std::mem::zeroed();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(monitor, (&mut info as *mut MONITORINFOEXW).cast()) == 0 {
            return Ok(None);
        }
        let driver: Vec<u16> = "DISPLAY\0".encode_utf16().collect();
        let dc = CreateDCW(
            driver.as_ptr(),
            info.szDevice.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
        );
        if dc.is_null() {
            return Ok(None);
        }
        let mut length = 0;
        GetICMProfileW(dc, &mut length, std::ptr::null_mut());
        if length == 0 {
            DeleteDC(dc);
            return Ok(None);
        }
        let mut path = vec![0u16; length as usize];
        let success = GetICMProfileW(dc, &mut length, path.as_mut_ptr());
        DeleteDC(dc);
        if success == 0 {
            return Ok(None);
        }
        let length = path.iter().position(|v| *v == 0).unwrap_or(path.len());
        Ok(Some(std::ffi::OsString::from_wide(&path[..length]).into()))
    }
}
