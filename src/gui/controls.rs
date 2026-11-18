//! Photo controls and coalesced parameter gestures.
pub(super) use super::sliders::{TrackScale, angle_slider, parameter_slider, slider};
use super::sliders::{exposure_slider, linear_slider};
use super::*;
use crate::config::Module;

#[derive(Clone, Copy)]
pub(super) enum Section {
    Framing,
    Light,
    Color,
    Detail,
    Sensor,
    Tone,
    Split,
    Retouch,
    Ai,
    Output,
}
impl Section {
    fn label(self) -> &'static str {
        match self {
            Self::Framing => "Crop & geometry",
            Self::Light => "Light",
            Self::Color => "Color calibration",
            Self::Detail => "Detail",
            Self::Sensor => "Sensor",
            Self::Tone => "Tone mapping",
            Self::Split => "Split toning",
            Self::Retouch => "Retouch",
            Self::Ai => "AI removal & corner fill",
            Self::Output => "Output & display",
        }
    }
}

#[derive(Default)]
pub(super) struct Sidebar {
    heights: [Option<f32>; 10],
    overrides: [Option<bool>; 10],
    open: [bool; 10],
    last_width: Option<f32>,
    resizing: bool,
}
impl Sidebar {
    pub(super) fn reset_measurements(&mut self) {
        self.heights = [None; 10];
        self.last_width = None;
        self.resizing = false;
    }

    pub(super) fn shows_ai(&self) -> bool {
        self.open[Section::Ai as usize]
    }

    fn plan(&mut self, height: f32, font_size: f32, expand: bool, framing: bool) {
        let scale = font_size / 18.;
        let estimated = [650., 230., 360., 200., 260., 310., 180., 280., 480., 250.];
        // Reserve all headers plus the reset button, including spacing and margins.
        let mut remaining = height - (10. * (font_size + 20.) + font_size + 22.);
        self.open =
            std::array::from_fn(|i| self.overrides[i].unwrap_or(i == Section::Light as usize));
        if framing {
            self.open[Section::Framing as usize] = true;
        }
        for (i, estimate) in estimated.iter().enumerate() {
            if self.open[i] {
                remaining -= self.heights[i].unwrap_or(estimate * scale);
            }
        }
        if expand {
            for section in [
                Section::Tone,
                Section::Color,
                Section::Detail,
                Section::Sensor,
                Section::Framing,
                Section::Split,
                Section::Retouch,
                Section::Ai,
                Section::Output,
            ] {
                let i = section as usize;
                let height = self.heights[i].unwrap_or(estimated[i] * scale);
                if !self.open[i] && self.overrides[i].is_none() && height + 8. <= remaining {
                    self.open[i] = true;
                    remaining -= height;
                }
            }
        }
    }

    fn show<R>(
        &mut self,
        section: Section,
        ui: &mut egui::Ui,
        body: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::CollapsingResponse<R> {
        let i = section as usize;
        let response = egui::CollapsingHeader::new(section.label())
            .open(Some(self.open[i]))
            .show(ui, body);
        if response.header_response.clicked() {
            self.overrides[i] = Some(!self.open[i]);
            ui.ctx().request_repaint();
        }
        if response.fully_open()
            && let Some(body) = &response.body_response
        {
            self.heights[i] = Some(body.rect.height() + ui.spacing().item_spacing.y);
        }
        response
    }
}

impl Editor {
    pub(super) fn controls(&mut self, ui: &mut egui::Ui) {
        let source_image = self.image.clone();
        let mut sidebar = std::mem::take(&mut self.sidebar);
        let panel = egui::Panel::right("edits")
            .default_size(self.preferences.config.ui.sidebar_width)
            .min_size(300.)
            .max_size(720_f32.min(ui.available_width() * 0.6))
            .resizable(true)
            .frame(egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(12.))
            .show(ui, |ui| {
                ui.add_space(12.);
                if let Some(image) = &self.image {
                    ui.add(egui::Label::new(egui::RichText::new(&image.metadata.model).heading().strong()).wrap()).on_hover_text(&image.metadata.model);
                    ui.label(
                        egui::RichText::new(format!(
                            "{} × {} · {:.1} MP",
                            image.metadata.width,
                            image.metadata.height,
                            image.metadata.width as f64 * image.metadata.height as f64 / 1e6
                        ))
                        .small()
                        .color(Color32::GRAY),
                    );
                } else {
                    ui.label(egui::RichText::new("Develop").heading().strong());
                }
                if !self.histogram.is_empty() {
                    let (rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 64.),
                        egui::Sense::hover(),
                    );
                    let points: Vec<_> = self
                        .histogram
                        .iter()
                        .enumerate()
                        .map(|(i, v)| {
                            Pos2::new(
                                rect.left() + i as f32 / 127. * rect.width(),
                                rect.bottom() - v * rect.height(),
                            )
                        })
                        .collect();
                    ui.painter().add(egui::Shape::line(
                        points,
                        egui::Stroke::new(1.2, Color32::from_gray(165)),
                    ));
                }
                ui.separator();
                let previous = self.edits.clone();
                sidebar.plan(ui.available_height(), self.preferences.config.ui.font_size, self.preferences.config.ui.expand_sections, self.framing.panel_open);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_enabled_ui(self.image.is_some() && !self.compare && self.busy.is_none(), |ui| {
                        sidebar.show(Section::Light, ui, |ui| {
                                exposure_slider(ui, &mut self.edits.scene.exposure);
                                egui::CollapsingHeader::new("Vignetting").show(ui, |ui| {
                                slider(
                                    ui,
                                    &mut self.edits.scene.vignette[0],
                                    -3.0..=3.0,
                                    "Vignette",
                                    " EV",
                                );
                                slider(
                                    ui,
                                    &mut self.edits.scene.vignette[1],
                                    -3.0..=3.0,
                                    "Vignette edge",
                                    " EV",
                                );
                                });
                                egui::CollapsingHeader::new("Graduated filter").default_open(self.tool == Tool::Graduated).show(ui, |ui| {
                                slider(
                                    ui,
                                    &mut self.edits.scene.graduated.exposure,
                                    -4.0..=4.0,
                                    "Graduated ND",
                                    " EV",
                                );
                                if ui.selectable_label(self.tool == Tool::Graduated, "Draw graduated filter").clicked() {
                                    self.toggle_tool(Tool::Graduated);
                                }
                                if self.edits.scene.graduated.exposure != 0. {
                                    slider(
                                        ui,
                                        &mut self.edits.scene.graduated.angle,
                                        -180.0..=180.0,
                                        "Direction",
                                        "°",
                                    );
                                    slider(
                                        ui,
                                        &mut self.edits.scene.graduated.center[1],
                                        0.0..=1.0,
                                        "Position",
                                        "",
                                    );
                                    slider(
                                        ui,
                                        &mut self.edits.scene.graduated.width,
                                        0.01..=1.0,
                                        "Transition",
                                        "",
                                    );
                                }
                                });
                                self.module_defaults_button(ui, Module::Light);
                            });
                        sidebar.show(Section::Tone, ui, |ui| {
                            self.tone_controls(ui);
                        });
                        sidebar.show(Section::Color, ui, |ui| {
                                self.calibration_controls(ui);
                                egui::CollapsingHeader::new("Channel mixer").show(ui, |ui| {
                                    for i in 0..3 {
                                        ui.horizontal(|ui| {
                                            ui.label(["R", "G", "B"][i]);
                                            for j in 0..3 {
                                                ui.add(
                                                    egui::DragValue::new(
                                                        &mut self.edits.scene.mixer[i][j],
                                                    )
                                                    .speed(0.01)
                                                    .range(-3.0..=3.0),
                                                );
                                            }
                                        });
                                    }
                                });
                            });
                        let framing_panel = sidebar.show(Section::Framing, ui, |ui| {
                            self.framing_controls(ui);
                            egui::CollapsingHeader::new("Camera lens corrections").show(ui, |ui| {
                            if let Some(name) = source_image.as_ref().and_then(|i| i.metadata.lens_model.as_ref()) {
                                ui.label(egui::RichText::new(name).small().color(Color32::GRAY));
                            }
                            let profile = source_image.as_ref().and_then(|i| i.metadata.lens_profile.as_ref());
                            let available = profile.is_some_and(|p| p.distortion.is_some() || p.vignette.is_some() || p.has_chromatic_aberration());
                            let mut enabled = self.edits.lens.mode != LensMode::Off;
                            if ui.add_enabled(available, egui::Checkbox::new(&mut enabled, "Camera lens corrections")).changed() {
                                self.edits.lens = if enabled {
                                    LensEdits { mode: LensMode::EmbeddedV1, distortion: profile.is_some_and(|p| p.distortion.is_some()), vignette: profile.is_some_and(|p| p.vignette.is_some()), chromatic_aberration: profile.is_some_and(|p| p.has_chromatic_aberration()), auto_frame: true }
                                } else { LensEdits::default() };
                            }
                            if enabled {
                                ui.horizontal(|ui| {
                                    ui.add_enabled(profile.is_some_and(|p| p.distortion.is_some()), egui::Checkbox::new(&mut self.edits.lens.distortion, "Distortion"));
                                    ui.add_enabled(profile.is_some_and(|p| p.vignette.is_some()), egui::Checkbox::new(&mut self.edits.lens.vignette, "Vignetting"));
                                });
                                ui.add_enabled(profile.is_some_and(|p| p.has_chromatic_aberration()), egui::Checkbox::new(&mut self.edits.lens.chromatic_aberration, "Chromatic aberration"));
                                ui.checkbox(&mut self.edits.lens.auto_frame, "Avoid camera correction gaps");
                            }
                            if let Some(error) = source_image.as_ref().and_then(|i| i.metadata.lens_profile_error.as_ref()) {
                                ui.label(egui::RichText::new("Camera correction data unavailable").small()).on_hover_text(error);
                            }
                            });
                            egui::CollapsingHeader::new("Advanced geometry").show(ui, |ui| {
                            linear_slider(
                                ui,
                                &mut self.edits.geometry.pitch,
                                -65.0..=65.0,
                                -84.99..=84.99,
                                "Vertical",
                                "°",
                            );
                            linear_slider(
                                ui,
                                &mut self.edits.geometry.yaw,
                                -65.0..=65.0,
                                -84.99..=84.99,
                                "Horizontal",
                                "°",
                            );
                            linear_slider(
                                ui,
                                &mut self.edits.geometry.field_of_view,
                                10.0..=120.0,
                                0.01..=174.99,
                                "Field of view",
                                "°",
                            );
                            linear_slider(ui, &mut self.edits.geometry.scale, 0.5..=3.0, 0.01..=32., "Scale", "×");
                            slider(ui, &mut self.edits.geometry.shift[0], -1.0..=1.0, "Shift X", "");
                            slider(ui, &mut self.edits.geometry.shift[1], -1.0..=1.0, "Shift Y", "");
                            slider(
                                ui,
                                &mut self.edits.geometry.distortion[0],
                                -0.5..=0.5,
                                "Distortion",
                                "",
                            );
                            slider(
                                ui,
                                &mut self.edits.geometry.distortion[1],
                                -0.5..=0.5,
                                "Distortion edge",
                                "",
                            );
                            slider(
                                ui,
                                &mut self.edits.geometry.chromatic_aberration[0],
                                -0.02..=0.02,
                                "Red fringe",
                                "",
                            );
                            slider(
                                ui,
                                &mut self.edits.geometry.chromatic_aberration[1],
                                -0.02..=0.02,
                                "Blue fringe",
                                "",
                            );
                            #[cfg(feature="sam3")]
                            ui.checkbox(&mut self.perspective_mask,"Architecture mask (SAM 3)");
                            });
                        });
                        if framing_panel.header_response.clicked() { self.framing.panel_open = false; }
                        sidebar.show(Section::Detail, ui, |ui| {
                            slider(ui, &mut self.edits.detail.sharpen, 0. ..=1., "Sharpen", "");
                            slider(ui, &mut self.edits.detail.local_contrast, 0. ..=1., "Local contrast", "");
                            ui.label(egui::RichText::new("View at 100% to judge fine detail.").small().color(Color32::GRAY));
                            self.module_defaults_button(ui, Module::Detail);
                        });
                        sidebar.show(Section::Sensor, ui, |ui| {
                            egui::ComboBox::from_label("Reconstruction")
                                .selected_text(match self.edits.raw.reconstruction {
                                    Reconstruction::Mhc => "Standard",
                                    Reconstruction::RawNindV1 => "Joint AI (experimental)",
                                }).show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.edits.raw.reconstruction,
                                                        Reconstruction::Mhc, "Standard");
                                    #[cfg(feature = "raw-ml")]
                                    {
                                        let supported = source_image.as_ref().is_some_and(|image|
                                            image.cfa.as_ref().is_some_and(|c| c.width == 2 && c.height == 2));
                                        let installed = crate::models::raw_model_path().is_ok_and(|p| p.join("manifest.json").is_file());
                                        ui.add_enabled_ui(supported && installed, |ui| {
                                            ui.selectable_value(&mut self.edits.raw.reconstruction,
                                                Reconstruction::RawNindV1, "Joint AI (experimental)");
                                        });
                                    }
                                });
                            if self.edits.raw.reconstruction == Reconstruction::RawNindV1 {
                                ui.label("The first pass prepares the photo; later adjustments reuse it.");
                            }
                            ui.checkbox(&mut self.edits.raw.hot_pixels, "Correct hot pixels");
                            ui.add_enabled_ui(source_image.as_ref().is_some_and(|image| image.raw_integer), |ui| {
                                ui.checkbox(&mut self.edits.raw.recover_highlights, "Recover clipped highlights");
                            });
                            if self.edits.raw.reconstruction == Reconstruction::Mhc { linear_slider(
                                ui,
                                &mut self.edits.raw.denoise,
                                0.0..=0.05,
                                0.0..=1.0,
                                "Sensor denoise",
                                "",
                            ); }
                            self.module_defaults_button(ui, Module::Sensor);
                        });
                        sidebar.show(Section::Split, ui, |ui| {
                            slider(
                                ui,
                                &mut self.edits.display.split_strength,
                                0.0..=1.0,
                                "Strength",
                                "",
                            );
                            ui.horizontal(|ui| {
                                ui.label("Shadows");
                                ui.color_edit_button_rgb(&mut self.edits.display.shadows);
                            });
                            ui.horizontal(|ui| {
                                ui.label("Highlights");
                                ui.color_edit_button_rgb(&mut self.edits.display.highlights);
                            });
                        });
                        sidebar.show(Section::Retouch, ui, |ui| {
                            ui.horizontal(|ui| {
                                for (tool, label) in [(Tool::Clone, "Clone"), (Tool::Heal, "Heal")] {
                                    if ui.selectable_label(self.tool == tool, label).clicked() {
                                        self.toggle_tool(tool);
                                    }
                                }
                            });
                            slider(ui, &mut self.radius, 0.002..=0.15, "Brush size", "");
                            slider(ui, &mut self.feather, 0.0..=1.0, "Feather", "");
                            ui.label(format!("{} strokes", self.edits.display.retouch.len()));
                            if ui.small_button("Clear retouch").clicked() {
                                self.edits.display.retouch.clear();
                            }
                        });
                        sidebar.show(Section::Ai, ui, |ui| {
                            let stale = crate::synthesis::recipe_hash(&self.edits).is_ok_and(|h| {
                                self.edits
                                    .display
                                    .synthesis
                                    .iter()
                                    .any(|f| f.recipe_sha256 != h || f.source_color_revision!=self.image.as_ref().map_or(0,|i|i.metadata.color_revision))
                            });
                            ui.add_enabled_ui(cfg!(feature = "moebius"), |ui| {
                                if ui.selectable_label(self.tool == Tool::Mask, "Paint area").clicked() {
                                    self.toggle_tool(Tool::Mask);
                                }
                                slider(ui, &mut self.radius, 0.002..=0.15, "Brush size", "");
                                ui.horizontal(|ui| {
                                    ui.label("Sampling steps");
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.add(egui::DragValue::new(&mut self.ai_steps).range(2..=50).speed(1.));
                                    });
                                });
                                ui.spacing_mut().slider_width = ui.available_width();
                                ui.add(egui::Slider::new(&mut self.ai_steps, 2..=50).show_value(false));
                                ui.horizontal(|ui| {
                                    ui.label("Seed");
                                    ui.add(egui::DragValue::new(&mut self.ai_seed));
                                });
                                if ui
                                    .add_enabled(
                                        !self.mask.is_empty(),
                                        egui::Button::new("Generate painted area"),
                                    )
                                    .clicked()
                                {
                                    self.generate(false, false);
                                }
                                if ui.button("Fill geometric corners").clicked() {
                                    self.generate(true, false);
                                }
                                if stale {
                                    ui.label("Image processing changed; update the fills.");
                                }
                                if !self.edits.display.synthesis.is_empty()
                                    && ui
                                        .button(if stale {
                                            "Update generated fills"
                                        } else {
                                            "Regenerate fills"
                                        })
                                        .clicked()
                                {
                                    self.generate(false, true);
                                }
                            });
                            if !cfg!(feature = "moebius") {
                                ui.label(
                                    egui::RichText::new(
                                        "AI generation is unavailable in this build.",
                                    )
                                    .small(),
                                );
                            }
                            ui.label(format!(
                                "{} generated layers",
                                self.edits.display.synthesis.len()
                            ));
                            if ui.small_button("Clear selection").clicked() {
                                self.mask.clear();
                            }
                            if ui.small_button("Clear generated fills").clicked() {
                                self.edits.display.synthesis.clear();
                            }
                        });
                        sidebar.show(Section::Output, ui, |ui| {
                        egui::ComboBox::from_label("Export color")
                            .selected_text(format!("{:?}", self.space))
                            .show_ui(ui, |ui| {
                                for space in [
                                    OutputSpace::Srgb,
                                    OutputSpace::DisplayP3,
                                    OutputSpace::AdobeRgb,
                                    OutputSpace::Rec2020,
                                    OutputSpace::LinearSrgb,
                                ] {
                                    ui.selectable_value(
                                        &mut self.space,
                                        space,
                                        format!("{space:?}"),
                                    );
                                }
                            });
                        if ui.add_enabled(self.hdr_white_scale.is_none(), egui::Button::new("Choose display profile…")).clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("ICC profile", &["icc", "icm"])
                                .pick_file()
                        {
                            self.profile = Some(path);
                            self.changed();
                        }
                        if self.profile.is_some() && ui.small_button("Use automatic display colour").clicked() {self.profile=None;}
                        let display_label = self.display_resolved.label.as_str();
                        let display_label = if self.hdr_requested && self.hdr_white_scale.is_none() && self.profile.is_none() {
                            "Automatic: SDR surface (HDR unavailable)"
                        } else { display_label };
                        #[cfg(target_os = "linux")]
                        let display_label = if self.wayland_surface.as_ref().is_some_and(|surface| surface.tagged()) {
                            if self.hdr_requested { "Automatic: managed sRGB (HDR unavailable)" }
                            else { "Automatic: compositor-managed sRGB surface" }
                        } else { display_label };
                        ui.label(
                            egui::RichText::new(display_label)
                            .small()
                            .color(Color32::GRAY),
                        );
                        });
                        if ui.small_button("Reset all edits").clicked() {
                            self.edits = source_image.as_ref().map_or_else(Edits::default, |i| self.preferences.config.defaults.for_image(i));
                            self.sync_framing_aspect();
                        }
                    });
                });
                if previous.lens != self.edits.lens && let (Some(camera), Some(image)) = (self.edits.geometry.camera, &source_image) {
                    let m = &image.metadata;
                    let frame = crate::lens::Correction::compile(m.lens_profile.as_ref(), &self.edits.lens, m.width, m.height).map_or(1., |c| c.frame_scale);
                    if let Some(fov) = crate::perspective::camera_fov(camera, m.width, m.height, frame).filter(|f| *f < 175.) { self.edits.geometry.field_of_view = fov; }
                }
                if previous != self.edits {
                    if ui.input(|i| i.pointer.primary_down()) {
                        self.history.begin_gesture(previous);
                        self.changed();
                    } else {
                        self.remember(previous);
                    }
                }
                if !ui.input(|i| i.pointer.primary_down()) && !self.framing.dragging() {self.finish_edit_gesture();}
            });
        let width = panel.response.rect.width();
        if sidebar
            .last_width
            .is_some_and(|last| (width - last).abs() > 0.5)
        {
            sidebar.heights = [None; 10];
        }
        if ui.input(|i| i.pointer.primary_down())
            && sidebar
                .last_width
                .is_some_and(|last| (width - last).abs() > 0.5)
        {
            sidebar.resizing = true;
        }
        if sidebar.resizing && !ui.input(|i| i.pointer.primary_down()) {
            sidebar.resizing = false;
            let mut config = self.preferences.config.clone();
            config.ui.sidebar_width = width.clamp(300., 720.);
            if let Err(error) = self.preferences.save(config) {
                self.error = Some(format!("{error:#}"));
            }
        }
        sidebar.last_width = Some(width);
        self.sidebar = sidebar;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_budget_expands_sections_and_keeps_manual_choices() {
        let mut sidebar = Sidebar::default();
        sidebar.plan(600., 18., true, false);
        assert!(sidebar.open[Section::Light as usize]);
        assert!(!sidebar.open[Section::Color as usize]);
        let compact = sidebar.open.iter().filter(|v| **v).count();
        sidebar.plan(1800., 18., true, false);
        assert!(sidebar.open[Section::Color as usize]);
        assert!(sidebar.open[Section::Detail as usize]);
        assert!(sidebar.open.iter().filter(|v| **v).count() > compact);
        sidebar.overrides[Section::Color as usize] = Some(false);
        sidebar.overrides[Section::Sensor as usize] = Some(true);
        sidebar.plan(2400., 18., true, false);
        assert!(!sidebar.open[Section::Color as usize]);
        sidebar.plan(600., 18., true, false);
        assert!(sidebar.open[Section::Sensor as usize]);
        sidebar.plan(2400., 18., false, true);
        assert!(sidebar.open[Section::Framing as usize]);
        assert!(!sidebar.open[Section::Detail as usize]);
    }
}
