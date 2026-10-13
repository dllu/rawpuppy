//! Pinhole intrinsics from physical dimensions; equivalent EXIF is a fallback.
use crate::{edits::CameraIntrinsics, input::SensorImage};

pub fn camera_for_image(image: &SensorImage) -> Option<CameraIntrinsics> {
    let m = &image.metadata;
    let focal_length_mm = m.focal_length_mm.filter(|v| v.is_finite() && *v > 0.)?;
    // Published GFX dimensions, not a guessed sensor size based on make alone.
    let sensor_mm = match (m.make.to_ascii_uppercase().as_str(), m.model.as_str()) {
        ("FUJIFILM", "GFX 100S" | "GFX100S" | "GFX 100" | "GFX100") => [43.8, 32.9],
        _ => {
            let equivalent = m.focal_length_35mm.filter(|v| v.is_finite() && *v > 0.)?;
            let diagonal = 43.27 * focal_length_mm / equivalent;
            let aspect = m.width.min(m.height) as f32 / m.width.max(m.height) as f32;
            let width = diagonal / (1. + aspect * aspect).sqrt();
            [width, width * aspect]
        }
    };
    Some(CameraIntrinsics {
        focal_length_mm,
        sensor_mm,
    })
}

pub fn camera_fov(
    camera: CameraIntrinsics,
    width: usize,
    height: usize,
    frame_scale: f32,
) -> Option<f32> {
    if width == 0
        || height == 0
        || !frame_scale.is_finite()
        || frame_scale <= 0.
        || !camera.focal_length_mm.is_finite()
        || camera.focal_length_mm <= 0.
        || camera.sensor_mm.iter().any(|v| !v.is_finite() || *v <= 0.)
    {
        return None;
    }
    let sensor_width = camera.sensor_mm[usize::from(width < height)];
    Some((2. * (sensor_width * frame_scale / (2. * camera.focal_length_mm)).atan()).to_degrees())
}
