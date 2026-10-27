//! Main document toolbar.
use super::*;

impl Editor {
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("toolbar")
            .min_size(60.)
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin::symmetric(14, 12)),
            )
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new("rawpuppy")
                            .size(22.)
                            .strong()
                            .color(ACCENT),
                    );
                    ui.add_space(14.);
                    if ui.button("Open…").clicked() {
                        self.choose_photo();
                    }
                    ui.add_enabled_ui(self.image.is_some() && self.busy.is_none(), |ui| {
                        if ui.button("Save edits").clicked() {
                            self.save();
                        }
                        if ui.button("Export…").clicked() {
                            self.export();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(self.history.can_undo(), egui::Button::new("Undo"))
                            .clicked()
                        {
                            self.undo();
                        }
                        if ui
                            .add_enabled(self.history.can_redo(), egui::Button::new("Redo"))
                            .clicked()
                        {
                            self.redo();
                        }
                    });
                    if ui.button("Preferences…").clicked() {
                        self.preferences.draft = self.preferences.config.ui.clone();
                        self.preferences.open = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(12.);
                        if let Some(path) = &self.path {
                            ui.add(
                                egui::Label::new(
                                    path.file_name().unwrap_or_default().to_string_lossy(),
                                )
                                .truncate(),
                            );
                        }
                    });
                });
            });
        egui::Panel::bottom("status")
            .exact_size((self.preferences.config.ui.font_size * 0.85 + 14.).max(34.))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(14.);
                    if self.busy.is_some() {
                        ui.spinner();
                    }
                    let shortcuts =
                        "S straighten  ·  G grid  ·  B compare  ·  F fit  ·  1 actual pixels";
                    let help_width = ui
                        .painter()
                        .layout_no_wrap(
                            shortcuts.into(),
                            egui::TextStyle::Small.resolve(ui.style()),
                            Color32::GRAY,
                        )
                        .size()
                        .x;
                    let show_help = ui.available_width() > help_width + 400.;
                    let status_width =
                        ui.available_width() - if show_help { help_width + 30. } else { 12. };
                    ui.add_sized(
                        Vec2::new(status_width.max(1.), ui.available_height()),
                        egui::Label::new(
                            egui::RichText::new(&self.status)
                                .small()
                                .color(Color32::GRAY),
                        )
                        .truncate()
                        .halign(egui::Align::Min),
                    );
                    if show_help {
                        ui.label(egui::RichText::new(shortcuts).small().color(Color32::GRAY));
                    }
                });
            });
    }
}
