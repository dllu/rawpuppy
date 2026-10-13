//! Versioned, source-independent edit recipe. Coordinates are normalized; sizes use usize.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Edits {
    pub version: u32,
    pub raw: RawEdits,
    #[serde(skip_serializing_if = "LensEdits::is_default")]
    pub lens: LensEdits,
    pub geometry: GeometryEdits,
    pub scene: SceneEdits,
    pub tone: ToneEdits,
    pub display: DisplayEdits,
}

impl Default for Edits {
    fn default() -> Self {
        Self {
            version: 1,
            raw: RawEdits::default(),
            lens: LensEdits::default(),
            geometry: GeometryEdits::default(),
            scene: SceneEdits::default(),
            tone: ToneEdits::default(),
            display: DisplayEdits::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LensMode {
    #[default]
    Off,
    EmbeddedV1,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LensEdits {
    pub mode: LensMode,
    pub distortion: bool,
    pub vignette: bool,
    /// Missing in historical recipes, which retain their original channel maps.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub chromatic_aberration: bool,
    /// Preserve the largest rectangular field without camera-induced gaps.
    pub auto_frame: bool,
}
impl Default for LensEdits {
    fn default() -> Self {
        Self {
            mode: LensMode::Off,
            distortion: true,
            vignette: true,
            chromatic_aberration: false,
            auto_frame: true,
        }
    }
}
impl LensEdits {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct RawEdits {
    #[serde(skip_serializing_if = "Reconstruction::is_standard")]
    pub reconstruction: Reconstruction,
    pub hot_pixels: bool,
    /// Standard deviation in normalized sensor units. Zero disables denoising.
    pub denoise: f32,
    /// Missing in historical recipes, preserving their original pixels/hashes.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub recover_highlights: bool,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Reconstruction {
    #[default]
    Mhc,
    RawNindV1,
}
impl Reconstruction {
    pub fn is_standard(&self) -> bool {
        *self == Self::Mhc
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct GeometryEdits {
    /// Output-to-input radial coefficients: r' = r (1 + k1 r² + k2 r⁴).
    pub distortion: [f32; 2],
    /// Relative radial red and blue scaling for lateral chromatic aberration.
    pub chromatic_aberration: [f32; 2],
    pub yaw: f32,
    pub pitch: f32,
    pub rotation: f32,
    /// Horizontal field of view in degrees for the pinhole model.
    pub field_of_view: f32,
    /// x, y, width, height in the corrected canvas.
    pub crop: [f32; 4],
    pub scale: f32,
    /// Translation in the corrected canvas, composed into the pinhole map.
    #[serde(skip_serializing_if = "zero_shift")]
    pub shift: [f32; 2],
    /// Camera calibration and editable evidence; the derived FOV drives rendering.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraIntrinsics>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<PerspectiveGuide>,
    /// Physical output width/height, independent of the source pixel aspect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop_aspect: Option<f32>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CameraIntrinsics {
    pub focal_length_mm: f32,
    /// Landscape sensor dimensions in millimeters.
    pub sensor_mm: [f32; 2],
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum GuideAxis {
    Vertical,
    Horizontal,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PerspectiveGuide {
    /// Normalized lens-corrected image coordinates, before perspective and crop.
    pub start: [f32; 2],
    pub end: [f32; 2],
    pub axis: GuideAxis,
    pub enabled: bool,
}

fn zero_shift(value: &[f32; 2]) -> bool {
    *value == [0.; 2]
}

impl Default for GeometryEdits {
    fn default() -> Self {
        Self {
            distortion: [0.0; 2],
            chromatic_aberration: [0.0; 2],
            yaw: 0.0,
            pitch: 0.0,
            rotation: 0.0,
            field_of_view: 50.0,
            crop: [0.0, 0.0, 1.0, 1.0],
            scale: 1.0,
            shift: [0.; 2],
            camera: None,
            guides: vec![],
            crop_aspect: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SceneEdits {
    pub exposure: f32,
    #[serde(skip_serializing_if = "crate::calibration::Illuminant::is_legacy")]
    pub illuminant: crate::calibration::Illuminant,
    /// Relative sensor gains, applied with the camera calibration in one matrix.
    pub calibration: [f32; 3],
    /// A scene-linear RGB mixer, in the same calibration module.
    pub mixer: [[f32; 3]; 3],
    /// Radial gain in EV at r=1, quadratic and quartic terms.
    pub vignette: [f32; 2],
    pub graduated: GraduatedFilter,
}

impl Default for SceneEdits {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            illuminant: Default::default(),
            calibration: [1.0; 3],
            mixer: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            vignette: [0.0; 2],
            graduated: GraduatedFilter::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct GraduatedFilter {
    pub exposure: f32,
    pub angle: f32,
    pub center: [f32; 2],
    pub width: f32,
}

impl Default for GraduatedFilter {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            angle: 0.0,
            center: [0.5, 0.5],
            width: 0.3,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToneMapper {
    /// Historical analytic approximation, preserving old sidecars and saved fills.
    Agx,
    #[default]
    #[serde(rename = "agx_sdr_v1")]
    AgxSdr,
    Linear,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ToneEdits {
    pub mapper: ToneMapper,
    pub saturation: f32,
}
impl Default for ToneEdits {
    fn default() -> Self {
        Self {
            mapper: ToneMapper::AgxSdr,
            saturation: 1.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct DisplayEdits {
    /// Monotone points in perceptual sRGB coordinates.
    pub curve: Vec<[f32; 2]>,
    pub shadows: [f32; 3],
    pub highlights: [f32; 3],
    pub split_strength: f32,
    pub retouch: Vec<Retouch>,
    pub synthesis: Vec<crate::synthesis::GeneratedFill>,
}
impl Default for DisplayEdits {
    fn default() -> Self {
        Self {
            curve: vec![[0.0, 0.0], [1.0, 1.0]],
            shadows: [1.0; 3],
            highlights: [1.0; 3],
            split_strength: 0.0,
            retouch: vec![],
            synthesis: vec![],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetouchMode {
    #[default]
    Clone,
    Heal,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Retouch {
    pub source: [f32; 2],
    pub target: [f32; 2],
    /// Radius as a fraction of corrected canvas width before cropping.
    pub radius: f32,
    pub feather: f32,
    pub opacity: f32,
    pub mode: RetouchMode,
}

impl Edits {
    /// New integer RAW documents use highlight recovery and available camera corrections. Deserialized recipes keep
    /// their explicit/default-off state so historical rendering and hashes survive.
    pub fn for_image(image: &crate::input::SensorImage) -> Self {
        let mut out = Self::default();
        if crate::calibration::Illuminant::Camera
            .xy(&image.metadata)
            .and_then(crate::calibration::adaptation)
            .is_ok()
        {
            out.scene.illuminant = crate::calibration::Illuminant::Camera;
        }
        out.raw.recover_highlights = image.raw_integer;
        if let Some(profile) = &image.metadata.lens_profile
            && (profile.distortion.is_some()
                || profile.vignette.is_some()
                || profile.has_chromatic_aberration())
        {
            out.lens = LensEdits {
                mode: LensMode::EmbeddedV1,
                distortion: profile.distortion.is_some(),
                vignette: profile.vignette.is_some(),
                chromatic_aberration: profile.has_chromatic_aberration(),
                auto_frame: true,
            };
        }
        if let Some(equivalent) = image.metadata.focal_length_35mm {
            let frame_scale = crate::lens::Correction::compile(
                image.metadata.lens_profile.as_ref(),
                &out.lens,
                image.metadata.width,
                image.metadata.height,
            )
            .map_or(1., |c| c.frame_scale);
            if let Some(fov) = crate::perspective::horizontal_fov(
                equivalent,
                image.metadata.width,
                image.metadata.height,
                frame_scale,
            ) {
                out.geometry.field_of_view = fov;
            }
        }
        if let Some(camera) = crate::perspective::camera_for_image(image) {
            let frame_scale = crate::lens::Correction::compile(
                image.metadata.lens_profile.as_ref(),
                &out.lens,
                image.metadata.width,
                image.metadata.height,
            )
            .map_or(1., |c| c.frame_scale);
            if let Some(fov) = crate::perspective::camera_fov(
                camera,
                image.metadata.width,
                image.metadata.height,
                frame_scale,
            ) {
                out.geometry.camera = Some(camera);
                out.geometry.field_of_view = fov;
            }
        }
        out
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "Unsupported edit recipe version {}",
            self.version
        );
        let json = serde_json::to_value(self)?;
        fn no_null(v: &serde_json::Value) -> bool {
            match v {
                serde_json::Value::Null => false,
                serde_json::Value::Array(a) => a.iter().all(no_null),
                serde_json::Value::Object(a) => a.values().all(no_null),
                _ => true,
            }
        }
        ensure!(no_null(&json), "All edit values must be finite");
        let g = &self.geometry;
        if let Some(camera) = g.camera {
            ensure!(
                camera.focal_length_mm > 0. && camera.sensor_mm.iter().all(|v| *v > 0.),
                "Camera dimensions and focal length must be positive"
            );
        }
        if let Some(aspect) = g.crop_aspect {
            ensure!(aspect > 0., "Crop aspect ratio must be positive");
        }
        ensure!(
            g.guides.iter().all(|line| (line.end[0] - line.start[0])
                .hypot(line.end[1] - line.start[1])
                > 1e-6),
            "Perspective guides must have distinct endpoints"
        );
        ensure!(
            g.field_of_view > 0.0 && g.field_of_view < 175.0,
            "Field of view must be between 0 and 175 degrees"
        );
        ensure!(
            g.yaw.abs() < 85.0 && g.pitch.abs() < 85.0,
            "Perspective angles must be less than 85 degrees"
        );
        ensure!(g.scale > 0.0, "Geometry scale must be positive");
        ensure!(
            g.crop[0] >= 0.0
                && g.crop[1] >= 0.0
                && g.crop[2] > 0.0
                && g.crop[3] > 0.0
                && g.crop[0] + g.crop[2] <= 1.000001
                && g.crop[1] + g.crop[3] <= 1.000001,
            "Crop must be a nonempty rectangle inside [0,1]"
        );
        ensure!(
            self.raw.denoise >= 0.0 && self.raw.denoise <= 1.0,
            "Denoise sigma must be in [0,1]"
        );
        ensure!(
            self.scene.exposure.abs() <= 32.0,
            "Exposure must be within ±32 EV"
        );
        ensure!(
            self.scene.calibration.iter().all(|x| *x > 0.0),
            "Calibration gains must be positive"
        );
        self.scene.illuminant.validate()?;
        ensure!(
            self.scene.graduated.width > 0.0,
            "Graduated filter width must be positive"
        );
        ensure!(
            self.tone.saturation >= 0.0,
            "Saturation must be nonnegative"
        );
        ensure!(
            self.display.split_strength >= 0.0 && self.display.split_strength <= 1.0,
            "Split strength must be in [0,1]"
        );
        let c = &self.display.curve;
        ensure!(
            c.len() >= 2 && c[0][0] == 0.0 && c[c.len() - 1][0] == 1.0,
            "Curve must span [0,1]"
        );
        ensure!(
            c.iter().all(|p| p.iter().all(|x| (0.0..=1.0).contains(x)))
                && c.windows(2)
                    .all(|p| p[0][0] < p[1][0] && p[0][1] <= p[1][1]),
            "Curve must be monotone with unique x positions"
        );
        for r in &self.display.retouch {
            ensure!(
                r.radius > 0.0
                    && (0.0..=1.0).contains(&r.feather)
                    && (0.0..=1.0).contains(&r.opacity),
                "Invalid retouch brush"
            );
        }
        for fill in &self.display.synthesis {
            fill.validate()?;
        }
        Ok(())
    }
}
