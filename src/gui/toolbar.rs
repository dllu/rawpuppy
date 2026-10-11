//! Main document toolbar.
use super::*;

impl Editor {
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("toolbar").exact_size(60.).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.add_space(14.);
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
                    if ui
                        .button(if self.dirty() {
                            "Save edits •"
                        } else {
                            "Save edits"
                        })
                        .clicked()
                    {
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
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(12.);
                    if let Some(path) = &self.path {
                        ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                    }
                });
            });
        });
        egui::Panel::bottom("status")
            .exact_size(34.)
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(14.);
                    if self.busy.is_some() {
                        ui.spinner();
                    }
                    ui.label(
                        egui::RichText::new(&self.status)
                            .small()
                            .color(Color32::GRAY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(12.);
                        ui.label(
                            egui::RichText::new(
                                "B compare  ·  F fit  ·  1 actual pixels  ·  ⌘/Ctrl S save",
                            )
                            .small()
                            .color(Color32::GRAY),
                        );
                    });
                });
            });
    }
}
