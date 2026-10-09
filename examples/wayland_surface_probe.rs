//! Exercise native source tagging and owned-proxy teardown in an isolated compositor.
#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use eframe::egui;
    use rawpuppy::display::WaylandSurface;
    use std::{
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };
    struct Probe {
        surface: Option<WaylandSurface>,
        started: Instant,
        closed: bool,
        report: Arc<Mutex<Option<Result<bool, String>>>>,
    }
    impl eframe::App for Probe {
        fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
            let ctx = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| {
                ui.label("sRGB surface protocol probe");
                ui.horizontal(|ui| {
                    for color in [
                        egui::Color32::RED,
                        egui::Color32::GREEN,
                        egui::Color32::BLUE,
                        egui::Color32::WHITE,
                        egui::Color32::GRAY,
                    ] {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(80., 80.), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 0., color);
                    }
                });
            });
            if self.closed {
                return;
            }
            let result = self.surface.as_mut().unwrap().poll();
            match result {
                Err(error) => {
                    eprintln!("protocol_probe_error={error}");
                    *self.report.lock().unwrap() = Some(Err(error.to_string()));
                    self.closed = true;
                }
                Ok(()) if !self.surface.as_ref().unwrap().pending() => {
                    println!("surface_tagged={}", self.surface.as_ref().unwrap().tagged());
                    *self.report.lock().unwrap() =
                        Some(Ok(self.surface.as_ref().unwrap().tagged()));
                    self.closed = true;
                }
                _ => {}
            }
            if self.started.elapsed() > Duration::from_secs(5) {
                *self.report.lock().unwrap() = Some(Err("Probe timed out".into()));
                self.closed = true;
            }
            if self.closed {
                // Drop our protocol objects while Winit still owns a live window.
                self.surface = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint_after(Duration::from_millis(20));
            }
        }
    }
    let report = Arc::new(Mutex::new(None));
    let app_report = report.clone();
    eframe::run_native(
        "Rawpuppy surface probe",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([480., 180.]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let window = cc.winit_window().ok_or("Missing native window")?.clone();
            let surface = WaylandSurface::bind(window)?.ok_or("Probe requires Wayland")?;
            Ok(Box::new(Probe {
                surface: Some(surface),
                started: Instant::now(),
                closed: false,
                report: app_report,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let tagged = report
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| anyhow::anyhow!("Probe did not finish"))?
        .map_err(anyhow::Error::msg)?;
    let expect_legacy = std::env::args().any(|arg| arg == "--legacy");
    anyhow::ensure!(
        tagged != expect_legacy,
        "Unexpected compositor source-tag state"
    );
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This probe requires Linux Wayland");
}
