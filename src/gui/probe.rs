//! Explicit debug-build native frame verification; inactive without the test environment variable.
use anyhow::{Context, Result, ensure};
use eframe::{
    egui,
    egui_wgpu::{RenderState, capture::FloatCapture},
};
use std::{
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

pub(super) struct Probe {
    output: PathBuf,
    capture: FloatCapture,
    requested: bool,
    done: bool,
    started: Instant,
}
impl Probe {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("RAWPUPPY_TEST_GUI_REPORT").map(|path| Self {
            output: path.into(),
            capture: FloatCapture::default(),
            requested: false,
            done: false,
            started: Instant::now(),
        })
    }
    pub fn tick(
        &mut self,
        ctx: &egui::Context,
        state: Option<&RenderState>,
        white_scale: Option<f32>,
        photo_rect: Option<egui::Rect>,
        ready: bool,
    ) -> Result<()> {
        if self.done {
            return Ok(());
        }
        ensure!(
            self.started.elapsed() < Duration::from_secs(30),
            "Native editor probe timed out"
        );
        if ready && !self.requested && photo_rect.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                self.capture.clone(),
            )));
            self.requested = true;
        }
        let screenshot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot {
                    user_data, image, ..
                } if user_data
                    .data
                    .as_ref()
                    .is_some_and(|d| d.is::<FloatCapture>()) =>
                {
                    Some(image.clone())
                }
                _ => None,
            })
        });
        let Some(screenshot) = screenshot else {
            return Ok(());
        };
        let state = state.context("Missing presentation state")?;
        let floats = self.capture.0.lock().unwrap().take();
        let mut report = serde_json::json!({
            "format": format!("{:?}", state.target_format),
            "color_space": format!("{:?}", state.target_color_space),
            "hdr_active": white_scale.is_some(),
            "white_scale": white_scale,
            "screenshot_size": screenshot.size,
            "scope": "actual editor frame, before compositor output mapping; no physical-monitor measurement",
        });
        if let Some(image) = floats {
            let rectangle = photo_rect.context("Missing photo rectangle")?;
            let ppp = ctx.pixels_per_point();
            let samples: Vec<_> = [0.125, 0.375, 0.625, 0.875]
                .into_iter()
                .map(|u| {
                    let x = ((rectangle.left() + rectangle.width() * u) * ppp).round() as usize;
                    let y = (rectangle.center().y * ppp).round() as usize;
                    image.pixels
                        [y.min(image.size[1] - 1) * image.size[0] + x.min(image.size[0] - 1)]
                })
                .collect();
            let maximum = image
                .pixels
                .iter()
                .flat_map(|p| p[..3].iter())
                .copied()
                .fold(f32::NEG_INFINITY, f32::max);
            let minimum = image
                .pixels
                .iter()
                .flat_map(|p| p[..3].iter())
                .copied()
                .fold(f32::INFINITY, f32::min);
            report["float_signal"] = serde_json::json!({"size":image.size,"minimum_rgb":minimum,"maximum_rgb":maximum,"photo_samples":samples});
        } else {
            ensure!(
                white_scale.is_none(),
                "HDR frame capture failed to provide float pixels"
            );
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.output)?;
        file.write_all((serde_json::to_string_pretty(&report)? + "\n").as_bytes())?;
        self.done = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        Ok(())
    }
}
