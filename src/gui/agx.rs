use super::{controls::parameter_slider, *};
use crate::agx::{
    AgxEdits, BLACK_EV, BasePrimaries, Curve, PIVOT_MARGIN, Pick, Primaries, WHITE_EV,
};
use std::ops::RangeInclusive;

impl Editor {
    pub(super) fn tone_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.edits.tone.mapper, ToneMapper::AgxSdr, "AgX");
            ui.selectable_value(
                &mut self.edits.tone.mapper,
                ToneMapper::Linear,
                "Linear / HDR",
            );
        });
        if self.edits.tone.mapper == ToneMapper::Agx {
            ui.label("This recipe uses legacy AgX. Select AgX to use the adjustable version.");
        }
        if self.edits.tone.mapper == ToneMapper::AgxSdr {
            self.agx_controls(ui);
        } else {
            if matches!(self.tool, Tool::Agx(_)) {
                self.tool = Tool::View;
                self.calibration_box = None;
            }
            control(
                ui,
                &mut self.edits.tone.saturation,
                0. ..=2.,
                "Saturation",
                "",
            );
        }
        egui::CollapsingHeader::new("Display curve").show(ui, |ui| {
            curve_editor(ui, &mut self.edits.display.curve);
        });
    }

    fn agx_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (label, pick) in [
                ("Auto levels", Pick::Both),
                ("Auto white", Pick::White),
                ("Auto black", Pick::Black),
            ] {
                if ui
                    .button(label)
                    .on_hover_text("Measure scene exposure across the current crop")
                    .clicked()
                {
                    if matches!(self.tool, Tool::Agx(_)) {
                        self.tool = Tool::View;
                        self.calibration_box = None;
                    }
                    self.estimate_agx([0., 0., 1., 1.], pick);
                }
            }
            ui.menu_button("Sample area", |ui| {
                for (label, pick) in [
                    ("White and black points", Pick::Both),
                    ("White point", Pick::White),
                    ("Black point", Pick::Black),
                    ("Pivot exposure", Pick::PivotExposure),
                    ("Pivot and target output", Pick::PivotAndOutput),
                ] {
                    if ui.button(label).clicked() {
                        self.set_tool(Tool::Agx(pick));
                        self.calibration_box = None;
                        ui.close();
                    }
                }
            });
        });
        if matches!(self.tool, Tool::Agx(_)) {
            ui.label(
                egui::RichText::new("Drag a rectangle over the photo to sample it. Esc cancels.")
                    .small(),
            );
        }
        let tone = &mut self.edits.tone;
        let agx = &mut tone.agx;
        control(ui, &mut agx.white_ev, WHITE_EV, "White point", " EV");
        control(ui, &mut agx.black_ev, BLACK_EV, "Black point", " EV");
        agx.clamp_pivot();
        control(ui, &mut agx.contrast, 0.1..=8., "Contrast", "");
        control(ui, &mut agx.toe_power, 0.5..=8., "Toe power", "");
        control(ui, &mut agx.shoulder_power, 0.5..=8., "Shoulder power", "");
        egui::CollapsingHeader::new("Curve preview").show(ui, |ui| plot_curve(ui, agx));
        egui::CollapsingHeader::new("Advanced curve").show(ui, |ui| {
            control(ui, &mut agx.range_margin, 0. ..=1., "Auto range margin", "").on_hover_text(
                "Expands the measured range in stops around mid-gray. 0.1 adds 10%.",
            );
            control(
                ui,
                &mut agx.pivot_ev,
                (agx.black_ev + PIVOT_MARGIN)..=(agx.white_ev - PIVOT_MARGIN),
                "Pivot exposure",
                " EV",
            );
            control(
                ui,
                &mut agx.pivot_output,
                (agx.target_black + 0.001)..=(agx.target_white - 0.001),
                "Pivot output",
                "",
            );
            control(ui, &mut agx.toe_start, 0. ..=0.95, "Toe start", "");
            control(
                ui,
                &mut agx.shoulder_start,
                0. ..=0.95,
                "Shoulder start",
                "",
            );
            control(
                ui,
                &mut agx.target_black,
                0. ..=0.98_f32.min(agx.pivot_output - 0.001),
                "Target black",
                "",
            );
            control(
                ui,
                &mut agx.target_white,
                (agx.pivot_output + 0.001).max(0.02)..=1.,
                "Target white",
                "",
            );
            ui.checkbox(&mut agx.auto_gamma, "Keep pivot on the diagonal");
            if !agx.auto_gamma {
                control(ui, &mut agx.gamma, 0.5..=8., "Curve gamma", "");
            }
        });
        egui::CollapsingHeader::new("Look").show(ui, |ui| {
            let agx = &mut self.edits.tone.agx;
            control(ui, &mut agx.slope, 0. ..=2., "Slope", "");
            control(ui, &mut agx.lift, -0.5..=0.5, "Lift", "");
            control(ui, &mut agx.brightness, 0.25..=4., "Brightness", "");
            control(ui, &mut agx.preserve_hue, 0. ..=1., "Preserve hue", "");
            self.saturation_control(ui);
        });
        egui::CollapsingHeader::new("Primaries")
            .show(ui, |ui| primaries(ui, &mut self.edits.tone.agx.primaries));
        if ui.small_button("Reset AgX").clicked() {
            self.edits.tone.agx = AgxEdits::default();
            self.edits.tone.saturation = self.preferences.config.defaults.color.saturation;
        }
    }

    fn saturation_control(&mut self, ui: &mut egui::Ui) {
        control(
            ui,
            &mut self.edits.tone.saturation,
            0. ..=2.,
            "Saturation",
            "×",
        );
        self.module_defaults_button(ui, crate::config::Module::Color);
    }

    pub(super) fn estimate_agx(&mut self, region: [f32; 4], pick: Pick) {
        if self.busy.is_some() || self.edits.tone.mapper != ToneMapper::AgxSdr {
            return;
        }
        self.finish_edit_gesture();
        if let Some(image) = self.image.clone() {
            self.busy = Some(Busy::AgxLevels(self.load_generation));
            self.status = "Measuring scene exposure for AgX…".into();
            self.send(Work::AgxLevels {
                load_id: self.load_generation,
                revision: self.history.revision(),
                image,
                edits: self.edits.clone(),
                region,
                pick,
            });
        }
    }
}

fn control(
    ui: &mut egui::Ui,
    v: &mut f32,
    range: RangeInclusive<f32>,
    label: &str,
    unit: &str,
) -> egui::Response {
    let response = parameter_slider(ui, v, range.clone(), range.clone(), label, unit, false);
    if response.changed() {
        *v = v.clamp(*range.start(), *range.end());
    }
    response
}

fn plot_curve(ui: &mut egui::Ui, agx: &AgxEdits) {
    let curve = Curve::new(agx);
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 105.), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3., ui.visuals().extreme_bg_color);
    let range = agx.white_ev - agx.black_ev;
    let point = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + x * rect.width(),
            rect.bottom() - y.powf(1. / 2.2) * rect.height(),
        )
    };
    let gray = -agx.black_ev / range;
    painter.line_segment(
        [point(gray, 0.), point(gray, 1.)],
        egui::Stroke::new(1., ui.visuals().weak_text_color()),
    );
    let points: Vec<_> = (0..=96)
        .map(|i| {
            let u = i as f32 / 96.;
            point(u, curve.at(agx.black_ev + u * range))
        })
        .collect();
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, ACCENT)));
    painter.circle_filled(
        point(
            (agx.pivot_ev - agx.black_ev) / range,
            curve.at(agx.pivot_ev),
        ),
        3.,
        ACCENT,
    );
    ui.horizontal(|ui| {
        ui.label(format!("{:+.2} EV", agx.black_ev));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(format!("{:+.2} EV", agx.white_ev));
        });
    });
}

fn primaries(ui: &mut egui::Ui, p: &mut Primaries) {
    if ui.small_button("Reset primaries").clicked() {
        *p = Primaries::default();
    }
    ui.checkbox(&mut p.disabled, "Disable primary adjustments");
    ui.add_enabled_ui(!p.disabled, |ui| {
        use BasePrimaries::*;
        egui::ComboBox::from_label("Base")
            .selected_text(match p.base {
                Rec2020 => "Rec.2020",
                DisplayP3 => "Display P3",
                Srgb => "sRGB",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut p.base, Rec2020, "Rec.2020");
                ui.selectable_value(&mut p.base, DisplayP3, "Display P3");
                ui.selectable_value(&mut p.base, Srgb, "sRGB");
            });
        egui::CollapsingHeader::new("Before tone mapping").show(ui, |ui| {
            for (i, name) in ["Red", "Green", "Blue"].iter().enumerate() {
                control(
                    ui,
                    &mut p.attenuation[i],
                    0. ..=0.75,
                    &format!("{name} attenuation"),
                    "",
                );
                control(
                    ui,
                    &mut p.rotation[i],
                    -30. ..=30.,
                    &format!("{name} rotation"),
                    "°",
                );
            }
        });
        egui::CollapsingHeader::new("After tone mapping").show(ui, |ui| {
            ui.checkbox(&mut p.reverse_all, "Reverse input adjustments");
            if !p.reverse_all {
                if ui.small_button("Copy from input").clicked() {
                    p.purity = p.attenuation;
                    p.reversal = p.rotation;
                    p.purity_master = 1.;
                    p.rotation_master = 1.;
                }
                control(
                    ui,
                    &mut p.purity_master,
                    0. ..=1.,
                    "Master purity boost",
                    "",
                );
                control(
                    ui,
                    &mut p.rotation_master,
                    0. ..=1.,
                    "Master rotation reversal",
                    "",
                );
                for (i, name) in ["Red", "Green", "Blue"].iter().enumerate() {
                    control(
                        ui,
                        &mut p.purity[i],
                        0. ..=0.75,
                        &format!("{name} purity boost"),
                        "",
                    );
                    control(
                        ui,
                        &mut p.reversal[i],
                        -30. ..=30.,
                        &format!("{name} reverse rotation"),
                        "°",
                    );
                }
            }
        });
    });
}
