//! Photo controls and coalesced parameter gestures.
use super::*;

impl Editor {
    pub(super) fn controls(&mut self, ui: &mut egui::Ui) {
        let source_image = self.image.clone();
        egui::Panel::right("edits")
            .exact_size(310.)
            .resizable(false)
            .show(ui, |ui| {
                ui.add_space(12.);
                if let Some(image) = &self.image {
                    ui.label(
                        egui::RichText::new(&image.metadata.model)
                            .size(17.)
                            .strong(),
                    );
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
                    ui.label(egui::RichText::new("Develop").size(17.).strong());
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
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_enabled_ui(self.image.is_some() && !self.compare && self.busy.is_none(), |ui| {
                        egui::CollapsingHeader::new("Light")
                            .default_open(true)
                            .show(ui, |ui| {
                                slider(
                                    ui,
                                    &mut self.edits.scene.exposure,
                                    -8.0..=8.0,
                                    "Exposure",
                                    " EV",
                                );
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
                                slider(
                                    ui,
                                    &mut self.edits.scene.graduated.exposure,
                                    -4.0..=4.0,
                                    "Graduated ND",
                                    " EV",
                                );
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
                        egui::CollapsingHeader::new("Color calibration")
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("Relative to the camera white point")
                                        .small()
                                        .color(Color32::GRAY),
                                );
                                for (i, name) in ["Red", "Green", "Blue"].into_iter().enumerate() {
                                    slider(
                                        ui,
                                        &mut self.edits.scene.calibration[i],
                                        0.25..=4.0,
                                        name,
                                        "×",
                                    );
                                }
                                slider(
                                    ui,
                                    &mut self.edits.tone.saturation,
                                    0.0..=2.0,
                                    "Saturation",
                                    "×",
                                );
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
                        egui::CollapsingHeader::new("Geometry & lens").show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui.button("Auto vertical").on_hover_text("Align reliable vertical architecture; preserve the view's horizontal perspective").clicked() {self.auto_perspective(crate::perspective::Mode::Vertical);}
                                if ui.button("Auto both").on_hover_text("Align one unambiguous façade's vertical and horizontal directions").clicked() {self.auto_perspective(crate::perspective::Mode::Both);}
                            });
                            #[cfg(feature="sam3")]
                            ui.checkbox(&mut self.perspective_mask,"Architecture mask (SAM 3)").on_hover_text("Use the installed local model to exclude trees and people. Without a model, automatic correction uses the classical detector.");
                            if let Some(message)=&self.perspective_status {ui.label(egui::RichText::new(message).small().color(Color32::GRAY));}
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
                            slider(
                                ui,
                                &mut self.edits.geometry.rotation,
                                -180.0..=180.0,
                                "Rotate",
                                "°",
                            );
                            slider(
                                ui,
                                &mut self.edits.geometry.pitch,
                                -65.0..=65.0,
                                "Vertical",
                                "°",
                            );
                            slider(
                                ui,
                                &mut self.edits.geometry.yaw,
                                -65.0..=65.0,
                                "Horizontal",
                                "°",
                            );
                            slider(
                                ui,
                                &mut self.edits.geometry.field_of_view,
                                10.0..=120.0,
                                "Field of view",
                                "°",
                            );
                            slider(ui, &mut self.edits.geometry.scale, 0.5..=3.0, "Scale", "×");
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
                            ui.checkbox(&mut self.grid, "Alignment grid");
                            ui.label("Crop");
                            let c = &mut self.edits.geometry.crop;
                            ui.horizontal(|ui| {
                                ui.label("x");
                                ui.add(
                                    egui::DragValue::new(&mut c[0])
                                        .speed(0.005)
                                        .range(0.0..=0.99),
                                );
                                ui.label("y");
                                ui.add(
                                    egui::DragValue::new(&mut c[1])
                                        .speed(0.005)
                                        .range(0.0..=0.99),
                                );
                            });
                            c[2] = c[2].min(1. - c[0]);
                            c[3] = c[3].min(1. - c[1]);
                            let max_width = 1. - c[0];
                            let max_height = 1. - c[1];
                            ui.horizontal(|ui| {
                                ui.label("w");
                                ui.add(
                                    egui::DragValue::new(&mut c[2])
                                        .speed(0.005)
                                        .range(0.01..=max_width),
                                );
                                ui.label("h");
                                ui.add(
                                    egui::DragValue::new(&mut c[3])
                                        .speed(0.005)
                                        .range(0.01..=max_height),
                                );
                            });
                            if ui.small_button("Reset crop").clicked() {
                                *c = [0., 0., 1., 1.];
                            }
                        });
                        egui::CollapsingHeader::new("Sensor & detail").show(ui, |ui| {
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
                            if self.edits.raw.reconstruction == Reconstruction::Mhc { slider(
                                ui,
                                &mut self.edits.raw.denoise,
                                0.0..=0.05,
                                "Sensor denoise",
                                "",
                            ); }
                        });
                        egui::CollapsingHeader::new("Tone").show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.selectable_value(
                                    &mut self.edits.tone.mapper,
                                    ToneMapper::AgxSdr,
                                    "AgX",
                                );
                                ui.selectable_value(
                                    &mut self.edits.tone.mapper,
                                    ToneMapper::Linear,
                                    "Linear / HDR",
                                );
                            });
                            if self.edits.tone.mapper == ToneMapper::Agx {
                                ui.label("This recipe keeps the older AgX approximation. Select AgX to update its rendering.");
                            }
                            curve_editor(ui, &mut self.edits.display.curve);
                        });
                        egui::CollapsingHeader::new("Split toning").show(ui, |ui| {
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
                        egui::CollapsingHeader::new("Retouch").show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut self.tool, Tool::View, "Pan");
                                ui.selectable_value(&mut self.tool, Tool::Clone, "Clone");
                                ui.selectable_value(&mut self.tool, Tool::Heal, "Heal");
                            });
                            ui.label(
                                egui::RichText::new("Alt-click a source, then paint a target.")
                                    .small(),
                            );
                            slider(ui, &mut self.radius, 0.002..=0.15, "Brush size", "");
                            slider(ui, &mut self.feather, 0.0..=1.0, "Feather", "");
                            ui.label(format!("{} strokes", self.edits.display.retouch.len()));
                            if ui.small_button("Clear retouch").clicked() {
                                self.edits.display.retouch.clear();
                            }
                        });
                        egui::CollapsingHeader::new("AI removal & corner fill").show(ui, |ui| {
                            let stale = crate::synthesis::recipe_hash(&self.edits).is_ok_and(|h| {
                                self.edits
                                    .display
                                    .synthesis
                                    .iter()
                                    .any(|f| f.recipe_sha256 != h || f.source_color_revision!=self.image.as_ref().map_or(0,|i|i.metadata.color_revision))
                            });
                            ui.add_enabled_ui(cfg!(feature = "moebius"), |ui| {
                                ui.horizontal(|ui| {
                                    ui.selectable_value(&mut self.tool, Tool::Mask, "Paint area")
                                        .on_hover_text("Cover the entire object, including its edges and shadows. Only painted pixels are replaced.");
                                    ui.selectable_value(&mut self.tool, Tool::View, "Pan");
                                });
                                slider(ui, &mut self.radius, 0.002..=0.15, "Brush size", "");
                                ui.add(
                                    egui::Slider::new(&mut self.ai_steps, 2..=50)
                                        .text("Sampling steps"),
                                );
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
                        ui.separator();
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
                        if ui.small_button("Reset all edits").clicked() {
                            self.edits = source_image.as_ref().map_or_else(Edits::default, |i| Edits::for_image(i));
                        }
                    });
                });
                if previous != self.edits {
                    if ui.input(|i| i.pointer.primary_down()) {
                        self.history.begin_gesture(previous);
                        self.changed();
                    } else {
                        self.remember(previous);
                    }
                }
                if !ui.input(|i| i.pointer.primary_down()) {self.finish_edit_gesture();}
            });
    }
}

fn slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    label: &str,
    suffix: &str,
) {
    ui.add(egui::Slider::new(value, range).text(label).suffix(suffix));
}
