//! User preferences are separate from the current photo and its undo history.
use super::*;
use crate::config::{Config, Module, UiSettings};

pub(super) struct Preferences {
    pub config: Config,
    pub path: Option<PathBuf>,
    pub open: bool,
    pub draft: UiSettings,
}
impl Preferences {
    pub fn load() -> (Self, Option<String>) {
        // Widget/worker tests must never read or write the user's preferences.
        let path = if cfg!(test) {
            Ok(None)
        } else {
            Config::path().map(Some)
        };
        let (config, path, error) = match path {
            Ok(Some(path)) => match Config::load_from(&path) {
                Ok(config) => (config, Some(path), None),
                Err(error) => (Config::default(), Some(path), Some(format!("{error:#}"))),
            },
            Ok(None) => (Config::default(), None, None),
            Err(error) => (Config::default(), None, Some(format!("{error:#}"))),
        };
        (
            Self {
                draft: config.ui.clone(),
                config,
                path,
                open: false,
            },
            error,
        )
    }

    pub fn save(&mut self, config: Config) -> Result<()> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("User configuration directory is unavailable"))?;
        config.save_to(path)?;
        self.config = config;
        Ok(())
    }
}

pub(super) fn apply_style(ctx: &egui::Context, settings: &UiSettings) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    let size = settings.font_size;
    for (text_style, factor) in [
        (egui::TextStyle::Body, 1.),
        (egui::TextStyle::Button, 1.),
        (egui::TextStyle::Monospace, 1.),
        (egui::TextStyle::Small, 0.85),
        (egui::TextStyle::Heading, 1.25),
    ] {
        style.text_styles.get_mut(&text_style).unwrap().size = size * factor;
    }
    style.spacing.item_spacing = Vec2::new(10., 8.);
    style.spacing.button_padding = Vec2::new(12., 6.);
    style.spacing.interact_size.y = size + 12.;
    ctx.set_style_of(egui::Theme::Dark, style);
}

impl Editor {
    pub(super) fn module_defaults_button(&mut self, ui: &mut egui::Ui, module: Module) {
        let hint = match module {
            Module::Light => {
                "Save exposure and vignetting for new photos. Graduated filters stay with this photo."
            }
            Module::Color => "Use this saturation for new photos.",
            Module::Detail => "Save sharpening and local contrast for new photos.",
            Module::Sensor => {
                "Save hot-pixel correction, highlight recovery and denoising for new photos. Reconstruction is chosen per photo."
            }
        };
        let label = if matches!(module, Module::Color) {
            "Save saturation as default"
        } else {
            "Save as default"
        };
        if ui.small_button(label).on_hover_text(hint).clicked() {
            let mut config = self.preferences.config.clone();
            config.defaults.capture(module, &self.edits);
            match self.preferences.save(config) {
                Ok(()) => self.status = format!("{} defaults saved for new photos", module.label()),
                Err(error) => self.error = Some(format!("{error:#}")),
            }
        }
    }

    pub(super) fn preferences_window(&mut self, ctx: &egui::Context) {
        let mut open = self.preferences.open;
        egui::Window::new("Preferences").open(&mut open).resizable(false).default_width(430.).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Font size");
                ui.add(egui::DragValue::new(&mut self.preferences.draft.font_size).range(12. ..=32.).speed(0.25).suffix(" pt"));
            });
            ui.horizontal(|ui| {
                ui.label("Sidebar width");
                ui.add(egui::DragValue::new(&mut self.preferences.draft.sidebar_width).range(300. ..=720.).speed(1.).suffix(" pt"));
            });
            ui.checkbox(&mut self.preferences.draft.expand_sections, "Expand sections when there is room");
            ui.separator();
            ui.label("Save module defaults from Light, Tone, Detail or Sensor. They apply to photos without saved edits and to Reset all edits.");
            ui.label("Light saves exposure and vignetting. Tone saves saturation.");
            if let Some(path) = &self.preferences.path {
                ui.label(egui::RichText::new(path.display().to_string()).small().color(Color32::GRAY));
            }
            ui.horizontal(|ui| {
                if ui.button("Save preferences").clicked() {
                    let mut config = self.preferences.config.clone();
                    config.ui = self.preferences.draft.clone();
                    match self.preferences.save(config) {
                        Ok(()) => {
                            apply_style(ctx, &self.preferences.config.ui);
                            // Panel widths are normally retained by egui between frames.
                            ctx.data_mut(|d| d.remove::<egui::containers::panel::PanelState>(egui::Id::new("edits")));
                            self.sidebar.reset_measurements();
                            self.status = "Preferences saved".into();
                        }
                        Err(error) => self.error = Some(format!("{error:#}")),
                    }
                }
                if ui.button("Reset UI settings").clicked() {
                    self.preferences.draft = UiSettings::default();
                }
            });
        });
        self.preferences.open = open;
    }
}
