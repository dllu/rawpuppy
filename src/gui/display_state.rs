//! Native display profile and HDR surface changes.
use super::*;

impl Editor {
    pub(super) fn refresh_display(&mut self, frame: &eframe::Frame, ctx: &egui::Context) {
        if self.hdr_white_scale.is_some() {
            let desktop = frame
                .winit_window()
                .and_then(|w| w.window_handle().ok())
                .map_or(display::Desktop::Other, |h| display::desktop(h.as_raw()));
            let state = self.presentation.as_ref().unwrap();
            let scale =
                display::hdr::reference_white_scale(desktop, &state.display_hdr_info.read());
            state.renderer.write().set_hdr_white_scale(scale);
            if let Some(texture) = &self.hdr_texture
                && let Err(error) = texture.set_white_scale(&state.queue, scale)
            {
                self.error = Some(error.to_string());
            }
            if self.hdr_white_scale != Some(scale) {
                self.hdr_white_scale = Some(scale);
                ctx.request_repaint();
            }
            self.display_resolved = display::Resolved {
                icc: None,
                label: "Automatic: extended-linear HDR surface".into(),
            };
            ctx.request_repaint_after(std::time::Duration::from_secs(2));
            return;
        }
        let mut request = display::Request {
            custom: self.profile.clone(),
            ..Default::default()
        };
        if let Some(window) = frame.winit_window() {
            if let Ok(handle) = window.window_handle() {
                request.desktop = display::desktop(handle.as_raw());
            }
            request.monitor = window.current_monitor().map(|monitor| {
                let p = monitor.position();
                let s = monitor.size();
                display::Monitor {
                    name: monitor.name(),
                    rect: [p.x, p.y, s.width as i32, s.height as i32],
                }
            });
        }
        if self.display_request.as_ref() != Some(&request)
            || self.display_polled.elapsed().as_secs() >= 2
        {
            self.display_request = Some(request.clone());
            self.display_polled = Instant::now();
            let _ = self.display_tx.send(request);
        }
        while let Ok((request, result)) = self.display_rx.try_recv() {
            if self.display_request.as_ref() != Some(&request) {
                continue;
            }
            let resolved = match result {
                Ok(resolved) => resolved,
                Err(error) => {
                    self.error = Some(error);
                    display::Resolved::default()
                }
            };
            if resolved != self.display_resolved {
                self.display_resolved = resolved;
                self.changed();
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(2));
        #[cfg(target_os = "linux")]
        if !self.wayland_surface_checked
            && let Some(window) = frame.winit_window()
            && let Ok(handle) = window.window_handle()
            && matches!(
                handle.as_raw(),
                raw_window_handle::RawWindowHandle::Wayland(_)
            )
        {
            self.wayland_surface_checked = true;
            match display::WaylandSurface::bind(window.clone()) {
                Ok(surface) => self.wayland_surface = surface,
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        #[cfg(target_os = "linux")]
        if let Some(surface) = &mut self.wayland_surface {
            if let Err(error) = surface.poll() {
                self.error = Some(error.to_string());
            }
            if surface.pending() {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
        #[cfg(target_os = "macos")]
        if self.surface_encoding != Some(self.present_managed)
            && let Some(window) = frame.winit_window()
            && let Ok(handle) = window.window_handle()
        {
            match display::set_surface_encoding(handle.as_raw(), self.present_managed) {
                Ok(count) if count > 0 => self.surface_encoding = Some(self.present_managed),
                Err(error) => self.error = Some(error.to_string()),
                _ => {}
            }
        }
    }
}
