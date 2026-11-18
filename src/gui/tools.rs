//! Commands and interaction feedback shared by the canvas and sidebar.
use super::*;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) enum Tool {
    View,
    Clone,
    Heal,
    Mask,
    Calibration,
    Agx(crate::agx::Pick),
    Crop,
    Perspective,
    Straighten,
    Graduated,
}

impl Tool {
    pub(super) fn is_brush(self) -> bool {
        matches!(self, Self::Clone | Self::Heal)
    }

    pub(super) fn full_canvas(self) -> bool {
        matches!(self, Self::Crop | Self::Perspective)
    }

    fn one_shot(self) -> bool {
        matches!(
            self,
            Self::Calibration | Self::Agx(_) | Self::Straighten | Self::Graduated
        )
    }

    fn name(self) -> &'static str {
        use crate::agx::Pick;
        match self {
            Self::View => "Pan",
            Self::Crop => "Crop",
            Self::Perspective => "Perspective guides",
            Self::Straighten => "Straighten",
            Self::Graduated => "Draw graduated filter",
            Self::Calibration => "Pick neutral colour",
            Self::Agx(Pick::Both) => "Pick white and black points",
            Self::Agx(Pick::White) => "Pick white point",
            Self::Agx(Pick::Black) => "Pick black point",
            Self::Agx(Pick::PivotExposure) => "Pick pivot exposure",
            Self::Agx(Pick::PivotAndOutput) => "Pick pivot and output",
            Self::Clone => "Clone",
            Self::Heal => "Heal",
            Self::Mask => "Paint AI selection",
        }
    }

    fn instructions(self, source_set: bool) -> &'static str {
        match self {
            Self::View => "",
            Self::Crop => "Drag an edge to resize or drag inside to move.",
            Self::Perspective => "Drag to draw or edit a guide. Right-click to remove.",
            Self::Straighten => "Drag along a horizon or a vertical edge.",
            Self::Graduated => "Drag from the unchanged side toward the filtered side.",
            Self::Calibration => "Draw a rectangle over a neutral surface.",
            Self::Agx(_) => "Draw a rectangle over the area to measure.",
            Self::Clone | Self::Heal if source_set => {
                "Paint the target. Alt-click to move the source."
            }
            Self::Clone | Self::Heal => "Alt-click a clean area to set the source.",
            Self::Mask => "Paint over the object and its edges.",
        }
    }
}

impl Editor {
    pub(super) fn set_tool(&mut self, tool: Tool) {
        if self.tool == tool && (!self.compare || tool == Tool::View) {
            return;
        }
        let framing_changed = self.tool.full_canvas() != tool.full_canvas();
        self.finish_edit_gesture();
        self.framing.cancel_drag();
        self.adjustment_drag = None;
        self.calibration_box = None;
        self.stroke_recorded = false;
        self.brush_stroke = BrushStroke::default();
        self.tool = tool;
        self.framing.panel_open = matches!(tool, Tool::Crop | Tool::Perspective | Tool::Straighten);
        if tool != Tool::View {
            self.set_compare(false);
        }
        if framing_changed {
            self.zoom = 1.;
            self.center = [0.5; 2];
            self.changed();
        }
    }

    pub(super) fn toggle_tool(&mut self, tool: Tool) {
        if self.image.is_some() {
            if self.tool == tool {
                self.set_tool(Tool::View);
            } else if self.busy.is_none() {
                self.set_tool(tool);
            }
        }
    }

    pub(super) fn set_compare(&mut self, compare: bool) {
        if self.compare != compare {
            if compare {
                self.set_tool(Tool::View);
            }
            self.compare = compare;
            self.changed();
        }
    }

    pub(super) fn canvas_shortcuts(&mut self, ui: &egui::Ui) {
        let ctx = ui.ctx();
        if self.image.is_none()
            || ctx.text_edit_focused()
            || egui::Popup::is_any_open(ctx)
            || self.preferences.open
            || self.error.is_some()
            || self.pending.is_some()
        {
            return;
        }
        let plain = |key| {
            ctx.input_mut(|input| {
                let pressed = input.events.iter().any(|event| {
                    matches!(event, egui::Event::Key {
                    key: pressed_key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } if *pressed_key == key && modifiers.is_none())
                });
                pressed && input.consume_key(egui::Modifiers::NONE, key)
            })
        };
        if plain(egui::Key::Escape) {
            self.set_compare(false);
            self.set_tool(Tool::View);
            return;
        }
        if self.tool != Tool::View
            && !self.tool.one_shot()
            && !ctx.egui_wants_keyboard_input()
            && plain(egui::Key::Enter)
        {
            self.set_tool(Tool::View);
            return;
        }
        if plain(egui::Key::F) {
            self.zoom = 1.;
            self.center = [0.5; 2];
            self.changed();
        }
        if plain(egui::Key::Num1) {
            self.actual_pixels(ui);
        }
        if plain(egui::Key::G) {
            self.framing.show_grid = !self.framing.show_grid;
        }
        if self.busy.is_none() {
            if plain(egui::Key::B) {
                self.set_compare(!self.compare);
            }
            for (key, tool) in [
                (egui::Key::C, Tool::Crop),
                (egui::Key::S, Tool::Straighten),
                (egui::Key::P, Tool::Perspective),
            ] {
                if plain(key) {
                    self.toggle_tool(tool);
                }
            }
        }
    }

    pub(super) fn canvas_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(self.busy.is_none(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (tool, label, hint) in [
                        (Tool::View, "Pan", "Drag to pan · Esc exits any tool"),
                        (Tool::Crop, "Crop", "Resize or move the crop · C"),
                        (Tool::Straighten, "Straighten", "Draw a level line · S"),
                        (
                            Tool::Perspective,
                            "Guides",
                            "Draw or edit perspective guides · P",
                        ),
                    ] {
                        if ui
                            .selectable_label(!self.compare && self.tool == tool, label)
                            .on_hover_text(hint)
                            .clicked()
                        {
                            self.set_compare(false);
                            self.toggle_tool(tool);
                        }
                    }
                });
            });
            ui.separator();
            if ui
                .small_button("Fit")
                .on_hover_text("Fit to screen · F")
                .clicked()
            {
                self.zoom = 1.;
                self.center = [0.5; 2];
                self.changed();
            }
            if ui
                .small_button("100%")
                .on_hover_text("Actual pixels · 1")
                .clicked()
            {
                self.actual_pixels(ui);
            }
            if ui
                .add_enabled(
                    self.busy.is_none(),
                    egui::Button::selectable(self.compare, "Before"),
                )
                .on_hover_text("Compare with the unedited tones · B")
                .clicked()
            {
                self.set_compare(!self.compare);
            }
            ui.menu_button(
                if self.framing.show_grid {
                    "Grid: on"
                } else {
                    "Grid"
                },
                |ui| {
                    self.grid_controls(ui);
                },
            );
        });
        if self.compare || self.tool != Tool::View {
            self.canvas_mode_bar(ui);
        }
    }

    fn canvas_mode_bar(&mut self, ui: &mut egui::Ui) {
        let (title, instruction, exit) = if self.compare {
            (
                "Before",
                "Showing the photo before tonal edits.",
                "Back to edits · Esc",
            )
        } else {
            (
                self.tool.name(),
                self.tool.instructions(self.clone_source.is_some()),
                if self.tool.one_shot() {
                    "Cancel · Esc"
                } else {
                    "Done · Esc"
                },
            )
        };
        egui::Frame::new()
            .fill(Color32::from_rgb(46, 38, 33))
            .stroke(egui::Stroke::new(1., ACCENT.gamma_multiply(0.7)))
            .corner_radius(5.)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(title).strong().color(ACCENT));
                    if ui.small_button(exit).clicked() {
                        self.set_compare(false);
                        self.set_tool(Tool::View);
                    }
                    ui.label(egui::RichText::new(instruction).small());
                });
            });
    }
}
