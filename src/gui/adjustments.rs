//! Small on-photo adjustments share canvas coordinates, cancellation and history.
use super::framing::Grid;
use super::*;

impl Editor {
    pub(super) fn grid_controls(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.framing.show_grid, "Show grid (G)");
        egui::ComboBox::from_label("Grid style")
            .selected_text(self.framing.grid.label())
            .show_ui(ui, |ui| {
                for grid in [
                    Grid::Thirds,
                    Grid::Golden,
                    Grid::Diagonals,
                    Grid::Center,
                    Grid::Architecture,
                ] {
                    if ui
                        .selectable_value(&mut self.framing.grid, grid, grid.label())
                        .changed()
                    {
                        self.framing.show_grid = true;
                    }
                }
            });
    }

    pub(super) fn adjustment_overlay(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        rect: Rect,
        region: [f32; 4],
    ) {
        if !matches!(self.tool, Tool::Straighten | Tool::Graduated)
            || self.compare
            || self.busy.is_some()
        {
            self.adjustment_drag = None;
            return;
        }
        let at = |p: Pos2| {
            [
                region[0] + (p.x - rect.left()) / rect.width() * region[2],
                region[1] + (p.y - rect.top()) / rect.height() * region[3],
            ]
        };
        let screen = |p: [f32; 2]| {
            Pos2::new(
                rect.left() + (p[0] - region[0]) / region[2] * rect.width(),
                rect.top() + (p[1] - region[1]) / region[3] * rect.height(),
            )
        };
        let primary = !ui.input(|i| i.pointer.middle_down());
        if primary
            && response.is_pointer_button_down_on()
            && ui.input(|i| i.pointer.primary_pressed())
        {
            let p = at(ui.input(|i| i.pointer.press_origin()).unwrap());
            self.adjustment_drag = Some([p, p]);
        }
        if primary
            && let Some(line) = &mut self.adjustment_drag
            && let Some(p) = ui.input(|i| i.pointer.interact_pos())
        {
            line[1] = at(p);
        }
        let line = self.adjustment_drag.or_else(|| {
            if self.tool != Tool::Graduated {
                return None;
            }
            let g = &self.edits.scene.graduated;
            let crop = self.edits.geometry.crop;
            let (s, c) = g.angle.to_radians().sin_cos();
            Some([-1., 1.].map(|sign| {
                [
                    crop[0] + (g.center[0] + sign * s * g.width * 0.5) * crop[2],
                    crop[1] + (g.center[1] + sign * c * g.width * 0.5) * crop[3],
                ]
            }))
        });
        if let Some(line) = line {
            let [a, b] = line.map(screen);
            let painter = ui.painter().with_clip_rect(rect);
            painter.line_segment([a, b], egui::Stroke::new(3., Color32::BLACK));
            painter.line_segment([a, b], egui::Stroke::new(1.5, ACCENT));
            for p in [a, b] {
                painter.circle_filled(p, 4., ACCENT);
            }
            let label = if self.tool == Tool::Straighten {
                let m = &self.image.as_ref().unwrap().metadata;
                crate::geometry::level_angle(line[0], line[1], m.height as f32 / m.width as f32)
                    .map(|angle| format!("{angle:+.2}°"))
            } else {
                Some("unchanged → filtered".into())
            };
            if let Some(label) = label {
                painter.text(
                    (a + b.to_vec2()) * 0.5 + Vec2::new(0., -12.),
                    egui::Align2::CENTER_BOTTOM,
                    label,
                    egui::FontId::proportional(13.),
                    Color32::WHITE,
                );
            }
        }
        if !ui.input(|i| i.pointer.primary_down())
            && let Some(line) = self.adjustment_drag.take()
        {
            if screen(line[0]).distance(screen(line[1])) < 12. {
                return;
            }
            self.finish_edit_gesture();
            let previous = self.edits.clone();
            if self.tool == Tool::Straighten {
                let m = &self.image.as_ref().unwrap().metadata;
                if let Some(angle) =
                    crate::geometry::level_angle(line[0], line[1], m.height as f32 / m.width as f32)
                {
                    self.edits.geometry.rotation =
                        crate::geometry::wrap_rotation(self.edits.geometry.rotation + angle);
                }
            } else {
                let crop = self.edits.geometry.crop;
                let points = line.map(|p| [(p[0] - crop[0]) / crop[2], (p[1] - crop[1]) / crop[3]]);
                let dx = points[1][0] - points[0][0];
                let dy = points[1][1] - points[0][1];
                let g = &mut self.edits.scene.graduated;
                g.center = std::array::from_fn(|i| (points[0][i] + points[1][i]) * 0.5);
                g.width = dx.hypot(dy).max(0.001);
                g.angle = dx.atan2(dy).to_degrees();
                if g.exposure == 0. {
                    g.exposure = -1.;
                }
            }
            if self.edits != previous {
                self.remember(previous);
            }
        }
    }
}
