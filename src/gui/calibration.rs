//! Illuminant controls and rectangular neutral-patch selection.
use super::*;
use crate::calibration::{self, Illuminant};

impl Editor {
    pub(super) fn calibration_controls(&mut self, ui: &mut egui::Ui) {
        let Some(image) = self.image.clone() else {
            return;
        };
        if image.metadata.camera_to_xyz.is_none() {
            ui.label("This image has no illuminant colour profile.");
            return;
        }
        let mut illuminant = self.edits.scene.illuminant;
        let xy = illuminant.xy(&image.metadata).unwrap_or(calibration::D65);
        let kelvin = calibration::temperature(xy)
            .unwrap_or(6500.)
            .clamp(1000., 25000.);
        egui::ComboBox::from_label("Illuminant")
            .selected_text(match illuminant {
                Illuminant::Legacy => "Saved camera calibration",
                Illuminant::Camera => "As shot",
                Illuminant::Daylight { .. } => "Daylight",
                Illuminant::BlackBody { .. } => "Black body",
                Illuminant::Custom { .. } => "Custom colour",
            })
            .show_ui(ui, |ui| {
                if illuminant.is_legacy() {
                    ui.selectable_value(
                        &mut illuminant,
                        Illuminant::Legacy,
                        "Saved camera calibration",
                    );
                }
                ui.add_enabled_ui(
                    Illuminant::Camera
                        .xy(&image.metadata)
                        .and_then(calibration::adaptation)
                        .is_ok(),
                    |ui| {
                        ui.selectable_value(&mut illuminant, Illuminant::Camera, "As shot");
                    },
                );
                if ui
                    .selectable_label(
                        matches!(illuminant, Illuminant::Daylight { .. }),
                        "Daylight",
                    )
                    .clicked()
                {
                    illuminant = Illuminant::Daylight {
                        kelvin: kelvin.max(4000.),
                    };
                }
                if ui
                    .selectable_label(
                        matches!(illuminant, Illuminant::BlackBody { .. }),
                        "Black body",
                    )
                    .clicked()
                {
                    illuminant = Illuminant::BlackBody { kelvin };
                }
                if ui
                    .selectable_label(
                        matches!(illuminant, Illuminant::Custom { .. }),
                        "Custom colour",
                    )
                    .clicked()
                {
                    illuminant = Illuminant::Custom { xy };
                }
            });
        let mut temperature = match illuminant {
            Illuminant::Daylight { kelvin } | Illuminant::BlackBody { kelvin } => kelvin,
            _ => kelvin,
        };
        let daylight = matches!(illuminant, Illuminant::Daylight { .. });
        if ui.add(egui::Slider::new(&mut temperature, if daylight {4000.} else {1000.}..=25000.)
            .logarithmic(true).custom_formatter(|v, _| format!("{v:.0}")).text("Temperature").suffix(" K"))
            .on_hover_text("Colour of the light source. Moving this control selects a spectral illuminant; custom colours may lie away from its locus.").changed() {
            illuminant = if daylight { Illuminant::Daylight { kelvin: temperature } } else { Illuminant::BlackBody { kelvin: temperature } };
        }
        let current_xy = illuminant.xy(&image.metadata).unwrap_or(xy);
        let mut swatch = calibration::swatch(current_xy).unwrap();
        let displayed_swatch = swatch;
        ui.horizontal(|ui| {
            ui.label("Illuminant colour");
            // egui's popup can round-trip RGB through HSV on opening. Ignore its
            // float noise so inspecting the swatch cannot create an undo step.
            if ui.color_edit_button_rgb(&mut swatch).on_hover_text("Choose the colour of the light source, or sample an area that should be neutral.").changed()
                && (0..3).any(|i| (swatch[i] - displayed_swatch[i]).abs() > 1e-6) {
                match calibration::chromaticity(color::apply(color::SRGB_TO_XYZ, swatch))
                    .and_then(|xy| {calibration::adaptation(xy)?; Ok(xy)}) {
                    Ok(xy) => illuminant = Illuminant::Custom { xy },
                    Err(_) => self.calibration_status = Some("Choose a light-source colour with nonzero brightness".into()),
                }
            }
        });
        if let Illuminant::Custom { xy } = &mut illuminant {
            let old = *xy;
            ui.horizontal(|ui| {
                for (name, v) in ["CIE x", "y"].into_iter().zip(xy.iter_mut()) {
                    ui.label(name);
                    ui.add(egui::DragValue::new(v).speed(0.0005).range(0.001..=0.85));
                }
            });
            if calibration::adaptation(*xy).is_err() {
                *xy = old;
            }
        }
        if illuminant != self.edits.scene.illuminant {
            if self.edits.scene.illuminant.is_legacy() {
                self.edits.scene.calibration = [1.; 3];
            }
            self.edits.scene.illuminant = illuminant;
            self.calibration_status = None;
        }
        ui.horizontal(|ui| {
            if ui.button("Estimate whole image").on_hover_text("Gray-world estimate: assume the image's mean colour should be neutral. Strongly coloured scenes can bias it.").clicked() {
                self.calibration_box = None;
                self.tool = Tool::View;
                self.estimate_illuminant([0., 0., 1., 1.]);
            }
            if ui.selectable_label(self.tool == Tool::Calibration, "Draw neutral box").clicked() {
                self.tool = if self.tool == Tool::Calibration {Tool::View} else {Tool::Calibration};
                self.calibration_box = None;
            }
        });
        if self.tool == Tool::Calibration {
            ui.label(egui::RichText::new("Drag over a neutral surface. Esc cancels.").small());
        }
        if let Some(status) = &self.calibration_status {
            ui.label(egui::RichText::new(status).small());
        }
        egui::CollapsingHeader::new("Fine channel gains").show(ui, |ui| {
            for (value, name) in self
                .edits
                .scene
                .calibration
                .iter_mut()
                .zip(["Red", "Green", "Blue"])
            {
                ui.add(egui::Slider::new(value, 0.25..=4.).text(name).suffix("×"));
            }
        });
    }
    pub(super) fn estimate_illuminant(&mut self, region: [f32; 4]) {
        if self.busy.is_some() {
            return;
        }
        self.finish_edit_gesture();
        if let Some(image) = self.image.clone() {
            self.busy = Some(Busy::Calibrate(self.load_generation));
            self.status = "Estimating illuminant from scene colour…".into();
            self.send(Work::Calibrate {
                load_id: self.load_generation,
                revision: self.history.revision(),
                image,
                edits: self.edits.clone(),
                region,
            });
        }
    }
    pub(super) fn calibration_selection(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
        region: [f32; 4],
        press: Option<[f32; 2]>,
    ) {
        if self.tool == Tool::Calibration && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.tool = Tool::View;
            self.calibration_box = None;
        }
        if self.tool == Tool::Calibration
            && !self.compare
            && self.busy.is_none()
            && !ui.input(|i| i.pointer.middle_down())
        {
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            if let Some(start) = press
                && response.is_pointer_button_down_on()
            {
                self.calibration_box = Some([start, start]);
            }
            if let Some(pos) = response.interact_pointer_pos()
                && let Some(points) = &mut self.calibration_box
            {
                points[1] = [
                    region[0]
                        + (pos.x - rect.left()).clamp(0., rect.width()) / rect.width() * region[2],
                    region[1]
                        + (pos.y - rect.top()).clamp(0., rect.height()) / rect.height() * region[3],
                ];
            }
            if response.drag_stopped_by(egui::PointerButton::Primary) {
                if let Some(points) = self.calibration_box {
                    let selection = sampling_region(points);
                    if selection[2] > 0. && selection[3] > 0. {
                        self.estimate_illuminant(selection);
                    }
                }
                self.tool = Tool::View;
            }
        }
        if let Some(points) = self.calibration_box {
            let [a, b] = points.map(|uv| {
                Pos2::new(
                    rect.left() + (uv[0] - region[0]) / region[2] * rect.width(),
                    rect.top() + (uv[1] - region[1]) / region[3] * rect.height(),
                )
            });
            ui.painter().with_clip_rect(rect).rect_stroke(
                Rect::from_two_pos(a, b),
                0.,
                egui::Stroke::new(1.5, ACCENT),
                egui::StrokeKind::Inside,
            );
        }
    }
}
pub(super) fn sampling_region(points: [[f32; 2]; 2]) -> [f32; 4] {
    let [a, b] = points.map(|uv| uv.map(|v| v.clamp(0., 1.)));
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        (a[0] - b[0]).abs(),
        (a[1] - b[1]).abs(),
    ]
}
