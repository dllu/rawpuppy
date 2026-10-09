use anyhow::{Result, ensure};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::NSView;
use objc2_core_graphics::{CGColorSpace, kCGColorSpaceSRGB};
use objc2_quartz_core::{CALayer, CAMetalLayer};
use raw_window_handle::RawWindowHandle;

fn tag(layer: &CALayer, space: Option<&CGColorSpace>) -> usize {
    let mut count = 0;
    if let Some(metal) = layer.downcast_ref::<CAMetalLayer>() {
        metal.setColorspace(space);
        count += 1;
    }
    // SAFETY: this traverses the owned view's layer tree on the UI thread.
    if let Some(children) = unsafe { layer.sublayers() } {
        for child in children {
            count += tag(&child, space);
        }
    }
    count
}
pub fn set_surface(handle: RawWindowHandle, managed: bool) -> Result<usize> {
    ensure!(
        MainThreadMarker::new().is_some(),
        "Metal surface color changes require the UI thread"
    );
    let RawWindowHandle::AppKit(handle) = handle else {
        return Ok(0);
    };
    // SAFETY: the handle comes from the live owned winit window on the UI thread;
    // retaining the NSView keeps it alive throughout the layer traversal.
    let view = unsafe { Retained::<NSView>::retain(handle.ns_view.as_ptr().cast()).unwrap() };
    let space = if managed {
        CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
    } else {
        None
    };
    Ok(view
        .layer()
        .map_or(0, |layer| tag(&layer, space.as_deref())))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metal_layer_tags_srgb_and_can_disable_matching_for_manual_icc() {
        let layer = CAMetalLayer::new();
        let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).unwrap();
        assert_eq!(tag(&layer, Some(&space)), 1);
        assert!(layer.colorspace().is_some());
        assert_eq!(tag(&layer, None), 1);
        assert!(layer.colorspace().is_none());
    }
}
