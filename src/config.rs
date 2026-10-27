//! Editor preferences and portable, photo-independent module defaults.
use crate::{
    edits::{DetailEdits, Edits},
    input::SensorImage,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub ui: UiSettings,
    pub defaults: ModuleDefaults,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiSettings {
    /// Logical points, independent of monitor scale.
    pub font_size: f32,
    pub sidebar_width: f32,
    pub expand_sections: bool,
}
impl Default for UiSettings {
    fn default() -> Self {
        Self {
            font_size: 18.,
            sidebar_width: 420.,
            expand_sections: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModuleDefaults {
    pub light: LightDefaults,
    pub color: ColorDefaults,
    pub detail: DetailDefaults,
    pub sensor: SensorDefaults,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LightDefaults {
    pub exposure: f32,
    pub vignette: [f32; 2],
}
impl Default for LightDefaults {
    fn default() -> Self {
        Self {
            exposure: 0.7,
            vignette: [0.; 2],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorDefaults {
    pub saturation: f32,
}
impl Default for ColorDefaults {
    fn default() -> Self {
        Self { saturation: 1.05 }
    }
}

// Keep new-photo defaults separate from DetailEdits' historical, neutral defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DetailDefaults {
    pub sharpen: f32,
    pub local_contrast: f32,
}
impl Default for DetailDefaults {
    fn default() -> Self {
        Self {
            sharpen: 0.2,
            local_contrast: 0.,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SensorDefaults {
    pub hot_pixels: bool,
    pub denoise: f32,
    /// Omitted means use the image-aware default (on for integer RAW input).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recover_highlights: Option<bool>,
}

#[derive(Clone, Copy)]
pub enum Module {
    Light,
    Color,
    Detail,
    Sensor,
}
impl Module {
    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Color => "Color",
            Self::Detail => "Detail",
            Self::Sensor => "Sensor",
        }
    }
}

impl ModuleDefaults {
    pub fn for_image(&self, image: &SensorImage) -> Edits {
        let mut edits = Edits::for_image(image);
        self.apply(&mut edits);
        edits
    }

    fn apply(&self, edits: &mut Edits) {
        edits.scene.exposure = self.light.exposure;
        edits.scene.vignette = self.light.vignette;
        edits.tone.saturation = self.color.saturation;
        edits.detail = DetailEdits {
            sharpen: self.detail.sharpen,
            local_contrast: self.detail.local_contrast,
        };
        edits.raw.hot_pixels = self.sensor.hot_pixels;
        edits.raw.denoise = self.sensor.denoise;
        if let Some(enabled) = self.sensor.recover_highlights {
            edits.raw.recover_highlights = enabled;
        }
    }

    /// Never copy image-specific calibration, geometry, filters, strokes or fills.
    pub fn capture(&mut self, module: Module, edits: &Edits) {
        match module {
            Module::Light => {
                self.light = LightDefaults {
                    exposure: edits.scene.exposure,
                    vignette: edits.scene.vignette,
                }
            }
            Module::Color => self.color.saturation = edits.tone.saturation,
            Module::Detail => {
                self.detail = DetailDefaults {
                    sharpen: edits.detail.sharpen,
                    local_contrast: edits.detail.local_contrast,
                }
            }
            Module::Sensor => {
                self.sensor = SensorDefaults {
                    hot_pixels: edits.raw.hot_pixels,
                    denoise: edits.raw.denoise,
                    recover_highlights: Some(edits.raw.recover_highlights),
                }
            }
        }
    }
}

impl Config {
    pub fn path() -> Result<PathBuf> {
        Ok(dirs::config_dir()
            .context("Could not locate the user configuration directory")?
            .join("rawpuppy")
            .join("config.toml"))
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let config = Self::default();
                config.save_to(path)?;
                return Ok(config);
            }
            Err(error) => return Err(error).with_context(|| format!("Reading {}", path.display())),
        };
        let config: Self =
            toml::from_str(&text).with_context(|| format!("Parsing {}", path.display()))?;
        config
            .validate()
            .with_context(|| format!("Invalid configuration in {}", path.display()))?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            (12. ..=32.).contains(&self.ui.font_size),
            "Font size must be between 12 and 32 points"
        );
        ensure!(
            (300. ..=720.).contains(&self.ui.sidebar_width),
            "Sidebar width must be between 300 and 720 points"
        );
        let mut edits = Edits::default();
        self.defaults.apply(&mut edits);
        edits.validate().context("Invalid module defaults")
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent).with_context(|| format!("Creating {}", parent.display()))?;
        let text = toml::to_string_pretty(self).context("Encoding preferences")?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(
            b"# Rawpuppy preferences. Module defaults apply to photos without a sidecar.\n",
        )?;
        temp.write_all(text.as_bytes())?;
        temp.as_file().sync_all()?;
        temp.persist(path)
            .with_context(|| format!("Saving {}", path.display()))?;
        Ok(())
    }
}
