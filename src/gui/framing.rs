//! Direct manipulation of corrected crops and lens-corrected perspective evidence.
use super::*;
use crate::edits::{CameraIntrinsics, GuideAxis, PerspectiveGuide};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Grid {
    #[default]
    None,
    Thirds,
    Golden,
    Diagonals,
    Center,
    Architecture,
}
impl Grid {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Thirds => "Rule of thirds",
            Self::Golden => "Golden ratio",
            Self::Diagonals => "Diagonals",
            Self::Center => "Center",
            Self::Architecture => "Architecture",
        }
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Aspect {
    #[default]
    Free,
    Original,
    Square,
    ThreeTwo,
    FourThree,
    FiveFour,
    SevenFive,
    SixteenNine,
    Wide,
    Custom,
}
impl Aspect {
    fn label(self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Original => "Original",
            Self::Square => "1:1",
            Self::ThreeTwo => "3:2",
            Self::FourThree => "4:3",
            Self::FiveFour => "5:4",
            Self::SevenFive => "7:5",
            Self::SixteenNine => "16:9",
            Self::Wide => "2.39:1",
            Self::Custom => "Custom",
        }
    }
    fn ratio(self, width: usize, height: usize) -> Option<f32> {
        match self {
            Self::Free => None,
            Self::Original => Some(width as f32 / height as f32),
            Self::Square => Some(1.),
            Self::ThreeTwo => Some(1.5),
            Self::FourThree => Some(4. / 3.),
            Self::FiveFour => Some(1.25),
            Self::SevenFive => Some(1.4),
            Self::SixteenNine => Some(16. / 9.),
            Self::Wide => Some(2.39),
            Self::Custom => None,
        }
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum AutoCrop {
    #[default]
    Off,
    Area,
    Aspect,
}

pub(super) struct Framing {
    pub grid: Grid,
    pub show_grid: bool,
    pub render_crop: [f32; 4],
    pub panel_open: bool,
    aspect: Aspect,
    portrait: bool,
    auto_crop: AutoCrop,
    pub selected: Option<usize>,
    axis: GuideAxis,
    crop_drag: Option<CropDrag>,
    guide_drag: Option<GuideDrag>,
}
impl Default for Framing {
    fn default() -> Self {
        Self {
            grid: Grid::Thirds,
            show_grid: false,
            render_crop: [0., 0., 1., 1.],
            panel_open: false,
            aspect: Aspect::Free,
            portrait: false,
            auto_crop: AutoCrop::Off,
            selected: None,
            axis: GuideAxis::Vertical,
            crop_drag: None,
            guide_drag: None,
        }
    }
}
impl Framing {
    pub fn cancel_drag(&mut self) {
        self.crop_drag = None;
        self.guide_drag = None;
    }
    pub fn sync_aspect(&mut self, ratio: Option<f32>, width: usize, height: usize) {
        let Some(ratio) = ratio else {
            self.aspect = Aspect::Free;
            self.portrait = false;
            return;
        };
        self.aspect = Aspect::Custom;
        self.portrait = false;
        for aspect in [
            Aspect::Original,
            Aspect::Square,
            Aspect::ThreeTwo,
            Aspect::FourThree,
            Aspect::FiveFour,
            Aspect::SevenFive,
            Aspect::SixteenNine,
            Aspect::Wide,
        ] {
            let value = aspect.ratio(width, height).unwrap();
            if (value - ratio).abs() < 1e-5 {
                self.aspect = aspect;
                self.portrait = false;
                return;
            }
            if (1. / value - ratio).abs() < 1e-5 {
                self.aspect = aspect;
                self.portrait = true;
                return;
            }
        }
    }
    pub fn dragging(&self) -> bool {
        self.crop_drag.is_some() || self.guide_drag.is_some()
    }
    pub fn crop_constraint(&self, aspect: Option<f32>) -> Option<crate::crop::Constraint> {
        match self.auto_crop {
            AutoCrop::Off => None,
            AutoCrop::Area => Some(crate::crop::Constraint::LargestArea),
            AutoCrop::Aspect => aspect.map(crate::crop::Constraint::Aspect),
        }
    }
}
struct CropDrag {
    handle: usize,
    start: [f32; 2],
    crop: [f32; 4],
}
struct GuideDrag {
    index: Option<usize>,
    endpoint: Option<usize>,
    start: [f32; 2],
    original: PerspectiveGuide,
}

impl Editor {
    pub(super) fn sync_framing_aspect(&mut self) {
        if let Some(image) = &self.image {
            self.framing.sync_aspect(
                self.edits.geometry.crop_aspect,
                image.metadata.width,
                image.metadata.height,
            );
        }
    }
    pub(super) fn set_tool(&mut self, tool: Tool) {
        if self.tool != tool {
            let was_full_canvas = matches!(self.tool, Tool::Crop | Tool::Perspective);
            self.finish_edit_gesture();
            self.framing.crop_drag = None;
            self.framing.guide_drag = None;
            self.adjustment_drag = None;
            self.tool = tool;
            self.framing.panel_open =
                matches!(tool, Tool::Crop | Tool::Perspective | Tool::Straighten);
            if matches!(tool, Tool::Crop | Tool::Perspective) || was_full_canvas {
                self.zoom = 1.;
                self.center = [0.5; 2];
            }
            self.changed();
        }
    }
    pub(super) fn framing_controls(&mut self, ui: &mut egui::Ui) {
        let Some(image) = self.image.clone() else {
            return;
        };
        super::controls::angle_slider(ui, &mut self.edits.geometry.rotation, -5. ..=5., "Rotation");
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.tool == Tool::Straighten, "Draw level line")
                .clicked()
            {
                self.set_tool(if self.tool == Tool::Straighten {
                    Tool::View
                } else {
                    Tool::Straighten
                });
            }
            if ui.small_button("−90°").clicked() {
                self.edits.geometry.rotation =
                    crate::geometry::wrap_rotation(self.edits.geometry.rotation - 90.);
            }
            if ui.small_button("+90°").clicked() {
                self.edits.geometry.rotation =
                    crate::geometry::wrap_rotation(self.edits.geometry.rotation + 90.);
            }
        });
        if self.tool == Tool::Straighten {
            ui.label(
                egui::RichText::new("Draw along a horizontal or vertical edge. Esc cancels.")
                    .small(),
            );
        }
        self.camera_controls(ui, &image);
        ui.horizontal(|ui| {
            if ui.button("Auto vertical").clicked() {
                self.auto_perspective(crate::perspective::Mode::Vertical);
            }
            if ui.button("Auto both").clicked() {
                self.auto_perspective(crate::perspective::Mode::Both);
            }
        });
        ui.horizontal(|ui| {
            if ui
                .button("Find guides")
                .on_hover_text("Detect lines to review before applying a correction")
                .clicked()
            {
                self.start_perspective(crate::perspective::Mode::Both, false, false);
            }
            if ui
                .selectable_label(self.tool == Tool::Perspective, "Edit guides")
                .clicked()
            {
                self.set_tool(if self.tool == Tool::Perspective {
                    Tool::View
                } else {
                    Tool::Perspective
                });
            }
        });
        if self.tool == Tool::Perspective {
            ui.label(
                egui::RichText::new("Blue: vertical · amber: horizontal · gray: excluded").small(),
            );
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.framing.axis, GuideAxis::Vertical, "Draw vertical");
                ui.selectable_value(
                    &mut self.framing.axis,
                    GuideAxis::Horizontal,
                    "Draw horizontal",
                );
            });
            ui.label(
                egui::RichText::new(
                    "Drag to draw · drag endpoints to edit · right-click to remove",
                )
                .small(),
            );
            if let Some(index) = self
                .framing
                .selected
                .filter(|i| *i < self.edits.geometry.guides.len())
            {
                let guide = &mut self.edits.geometry.guides[index];
                ui.horizontal(|ui| {
                    ui.checkbox(&mut guide.enabled, "Use line");
                    ui.selectable_value(&mut guide.axis, GuideAxis::Vertical, "Vertical");
                    ui.selectable_value(&mut guide.axis, GuideAxis::Horizontal, "Horizontal");
                });
                if ui.small_button("Remove selected line").clicked()
                    || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
                {
                    self.finish_edit_gesture();
                    self.framing.guide_drag = None;
                    self.edits.geometry.guides.remove(index);
                    self.framing.selected = None;
                }
            }
            ui.horizontal(|ui| {
                if ui.button("Apply vertical").clicked() {
                    self.start_perspective(crate::perspective::Mode::Vertical, true, true);
                }
                if ui.button("Apply both").clicked() {
                    self.start_perspective(crate::perspective::Mode::Both, true, true);
                }
            });
        }
        let guides = &self.edits.geometry.guides;
        if !guides.is_empty() {
            ui.label(
                egui::RichText::new(format!(
                    "{} vertical · {} horizontal · {} excluded",
                    guides
                        .iter()
                        .filter(|g| g.enabled && g.axis == GuideAxis::Vertical)
                        .count(),
                    guides
                        .iter()
                        .filter(|g| g.enabled && g.axis == GuideAxis::Horizontal)
                        .count(),
                    guides.iter().filter(|g| !g.enabled).count()
                ))
                .small(),
            );
            if ui.small_button("Clear guides").clicked() {
                self.edits.geometry.guides.clear();
                self.framing.selected = None;
            }
        }
        if let Some(message) = &self.perspective_status {
            ui.label(egui::RichText::new(message).small().color(Color32::GRAY));
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.tool == Tool::Crop, "Crop on image")
                .clicked()
            {
                self.set_tool(if self.tool == Tool::Crop {
                    Tool::View
                } else {
                    Tool::Crop
                });
            }
            if ui.small_button("Reset crop").clicked() {
                self.edits.geometry.crop = [0., 0., 1., 1.];
                self.edits.geometry.crop_aspect = None;
                self.framing.aspect = Aspect::Free;
            }
        });
        if self.tool == Tool::Crop {
            ui.label(egui::RichText::new("Drag edges to resize · drag inside to move").small());
        }
        let old = (self.framing.aspect, self.framing.portrait);
        egui::ComboBox::from_label("Aspect ratio")
            .selected_text(self.framing.aspect.label())
            .show_ui(ui, |ui| {
                for aspect in [
                    Aspect::Free,
                    Aspect::Original,
                    Aspect::Square,
                    Aspect::ThreeTwo,
                    Aspect::FourThree,
                    Aspect::FiveFour,
                    Aspect::SevenFive,
                    Aspect::SixteenNine,
                    Aspect::Wide,
                ] {
                    ui.selectable_value(&mut self.framing.aspect, aspect, aspect.label());
                }
            });
        if self.framing.aspect != Aspect::Free && self.framing.aspect != Aspect::Square {
            ui.checkbox(&mut self.framing.portrait, "Swap width / height");
        }
        if old != (self.framing.aspect, self.framing.portrait) {
            let ratio = if self.framing.aspect == Aspect::Custom {
                self.edits.geometry.crop_aspect.map(|r| 1. / r)
            } else {
                self.framing
                    .aspect
                    .ratio(image.metadata.width, image.metadata.height)
                    .map(|r| if self.framing.portrait { 1. / r } else { r })
            };
            self.edits.geometry.crop_aspect = ratio;
            if ratio.is_none() && self.framing.auto_crop == AutoCrop::Aspect {
                self.framing.auto_crop = AutoCrop::Off;
            }
            if let Some(r) = ratio {
                self.edits.geometry.crop = fit_aspect(
                    self.edits.geometry.crop,
                    r * image.metadata.height as f32 / image.metadata.width as f32,
                );
            }
        }
        if let Some(ratio) = self.edits.geometry.crop_aspect {
            ui.label(egui::RichText::new(format!("Locked to {ratio:.3}:1")).small());
        }
        self.grid_controls(ui);
        ui.horizontal(|ui| {
            if ui.button("Largest area").clicked() {
                self.auto_crop(crate::crop::Constraint::LargestArea);
            }
            if ui
                .add_enabled(
                    self.edits.geometry.crop_aspect.is_some(),
                    egui::Button::new("Fit aspect"),
                )
                .clicked()
            {
                self.auto_crop(crate::crop::Constraint::Aspect(
                    self.edits.geometry.crop_aspect.unwrap(),
                ));
            }
        });
        egui::ComboBox::from_label("After perspective")
            .selected_text(match self.framing.auto_crop {
                AutoCrop::Off => "Keep crop",
                AutoCrop::Area => "Crop largest area",
                AutoCrop::Aspect => "Crop to aspect ratio",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.framing.auto_crop, AutoCrop::Off, "Keep crop");
                ui.selectable_value(
                    &mut self.framing.auto_crop,
                    AutoCrop::Area,
                    "Crop largest area",
                );
                ui.add_enabled_ui(self.edits.geometry.crop_aspect.is_some(), |ui| {
                    ui.selectable_value(
                        &mut self.framing.auto_crop,
                        AutoCrop::Aspect,
                        "Crop to aspect ratio",
                    );
                });
            });
    }

    fn camera_controls(&mut self, ui: &mut egui::Ui, image: &SensorImage) {
        let m = &image.metadata;
        let detected = crate::perspective::camera_for_image(image);
        let mut camera = self.edits.geometry.camera.or(detected).unwrap_or_else(|| {
            let sensor_mm = [36., 24.];
            let width = sensor_mm[usize::from(m.width < m.height)];
            CameraIntrinsics {
                sensor_mm,
                focal_length_mm: width
                    / (2. * (self.edits.geometry.field_of_view.to_radians() * 0.5).tan()),
            }
        });
        let previous = camera;
        ui.horizontal(|ui| {
            ui.label("Focal length");
            ui.add(
                egui::DragValue::new(&mut camera.focal_length_mm)
                    .speed(0.5)
                    .range(1. ..=5000.)
                    .suffix(" mm"),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Sensor");
            for dimension in &mut camera.sensor_mm {
                ui.add(
                    egui::DragValue::new(dimension)
                        .speed(0.1)
                        .range(1. ..=500.)
                        .suffix(" mm"),
                );
            }
        });
        if detected.is_none() && self.edits.geometry.camera.is_none() {
            ui.label(
                egui::RichText::new("Set the sensor dimensions for your camera.")
                    .small()
                    .color(Color32::GRAY),
            );
        }
        if camera != previous {
            let frame = crate::lens::Correction::compile(
                m.lens_profile.as_ref(),
                &self.edits.lens,
                m.width,
                m.height,
            )
            .map_or(1., |c| c.frame_scale);
            if let Some(fov) = crate::perspective::camera_fov(camera, m.width, m.height, frame)
                .filter(|f| *f < 175.)
            {
                self.edits.geometry.camera = Some(camera);
                self.edits.geometry.field_of_view = fov;
            }
        }
    }

    pub(super) fn framing_overlay(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        rect: Rect,
        region: [f32; 4],
    ) {
        if (self.compare || self.busy.is_some()) && self.framing.dragging() {
            self.framing.cancel_drag();
            self.finish_edit_gesture();
        }
        if self.tool == Tool::Crop && !self.compare && self.busy.is_none() {
            self.crop_overlay(ui, response, rect, region);
        } else {
            let crop = to_screen_crop(rect, region, self.edits.geometry.crop);
            if self.framing.show_grid && !self.compare {
                draw_grid(ui.painter().with_clip_rect(rect), crop, self.framing.grid);
            }
            if self.tool == Tool::Perspective && self.edits.geometry.crop != [0., 0., 1., 1.] {
                ui.painter().with_clip_rect(rect).rect_stroke(
                    crop,
                    0.,
                    egui::Stroke::new(1., ACCENT.gamma_multiply(0.6)),
                    egui::StrokeKind::Inside,
                );
            }
        }
        if self.tool == Tool::Perspective && !self.compare && self.busy.is_none() {
            self.guide_overlay(ui, response, rect, region);
        }
    }

    fn crop_overlay(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        rect: Rect,
        region: [f32; 4],
    ) {
        let at = |p: Pos2| from_screen(rect, region, p);
        if ui.input(|i| i.pointer.primary_pressed()) && response.is_pointer_button_down_on() {
            let p = ui.input(|i| i.pointer.press_origin()).unwrap();
            let crop_rect = to_screen_crop(rect, region, self.edits.geometry.crop);
            if let Some(handle) = crop_hit(crop_rect, p) {
                self.history.begin_gesture(self.edits.clone());
                self.framing.crop_drag = Some(CropDrag {
                    handle,
                    start: at(p),
                    crop: self.edits.geometry.crop,
                });
            }
        }
        if let (Some(drag), Some(p)) = (&self.framing.crop_drag, response.interact_pointer_pos()) {
            let point = at(p);
            let m = &self.image.as_ref().unwrap().metadata;
            let ratio = self
                .edits
                .geometry
                .crop_aspect
                .map(|r| r * m.height as f32 / m.width as f32);
            let crop = resize_crop(
                drag.crop,
                [point[0] - drag.start[0], point[1] - drag.start[1]],
                drag.handle,
                ratio,
            );
            if crop != self.edits.geometry.crop {
                self.edits.geometry.crop = crop;
                self.changed();
            }
        }
        if !ui.input(|i| i.pointer.primary_down()) && self.framing.crop_drag.take().is_some() {
            self.finish_edit_gesture();
        }
        let crop = to_screen_crop(rect, region, self.edits.geometry.crop);
        let painter = ui.painter().with_clip_rect(rect);
        for shade in [
            Rect::from_min_max(rect.min, Pos2::new(rect.right(), crop.top())),
            Rect::from_min_max(Pos2::new(rect.left(), crop.bottom()), rect.max),
            Rect::from_min_max(Pos2::new(rect.left(), crop.top()), crop.left_bottom()),
            Rect::from_min_max(crop.right_top(), Pos2::new(rect.right(), crop.bottom())),
        ] {
            if shade.is_positive() {
                painter.rect_filled(shade.intersect(rect), 0., Color32::from_black_alpha(135));
            }
        }
        if self.framing.show_grid {
            draw_grid(painter.clone(), crop, self.framing.grid);
        }
        painter.rect_stroke(
            crop,
            0.,
            egui::Stroke::new(1.5, ACCENT),
            egui::StrokeKind::Inside,
        );
        for point in crop_handles(crop) {
            painter.rect_filled(
                Rect::from_center_size(point, Vec2::splat(7.)),
                1.,
                Color32::WHITE,
            );
        }
        if response.hovered()
            && let Some(p) = response.hover_pos()
            && let Some(handle) = crop_hit(crop, p)
        {
            ui.ctx().set_cursor_icon(match handle {
                0 | 4 => egui::CursorIcon::ResizeNwSe,
                2 | 6 => egui::CursorIcon::ResizeNeSw,
                1 | 5 => egui::CursorIcon::ResizeVertical,
                3 | 7 => egui::CursorIcon::ResizeHorizontal,
                _ => egui::CursorIcon::Move,
            });
        }
    }

    fn guide_overlay(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        rect: Rect,
        region: [f32; 4],
    ) {
        let m = &self.image.as_ref().unwrap().metadata;
        let Ok(geometry) =
            crate::geometry::Geometry::compile(&self.edits.geometry, m.width, m.height)
        else {
            return;
        };
        let Ok(inverse) = color::inverse(geometry.homography) else {
            return;
        };
        let screen = |p| project(inverse, p).map(|p| to_screen(rect, region, p));
        let source = |p: Pos2| project(geometry.homography, from_screen(rect, region, p));
        let hit = response
            .hover_pos()
            .and_then(|p| guide_hit(&self.edits.geometry.guides, p, &screen));
        if response.clicked_by(egui::PointerButton::Secondary)
            && let Some((index, _)) = hit
        {
            self.finish_edit_gesture();
            self.framing.guide_drag = None;
            let previous = self.edits.clone();
            self.edits.geometry.guides.remove(index);
            self.framing.selected = None;
            self.remember(previous);
        }
        if ui.input(|i| i.pointer.primary_pressed())
            && response.is_pointer_button_down_on()
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
            && let Some(start) = source(p)
        {
            let hit = guide_hit(&self.edits.geometry.guides, p, &screen);
            self.framing.selected = hit.map(|h| h.0);
            let original = hit.map_or(
                PerspectiveGuide {
                    start,
                    end: start,
                    axis: self.framing.axis,
                    enabled: true,
                },
                |h| self.edits.geometry.guides[h.0].clone(),
            );
            self.history.begin_gesture(self.edits.clone());
            self.framing.guide_drag = Some(GuideDrag {
                index: hit.map(|h| h.0),
                endpoint: hit.and_then(|h| h.1),
                start,
                original,
            });
        }
        if let Some(p) = response.interact_pointer_pos().and_then(source)
            && let Some(drag) = &mut self.framing.guide_drag
        {
            let mut guide = drag.original.clone();
            if drag.index.is_none() {
                guide.end = p;
            } else if let Some(endpoint) = drag.endpoint {
                if endpoint == 0 {
                    guide.start = p;
                } else {
                    guide.end = p;
                }
            } else {
                for (axis, value) in p.into_iter().enumerate() {
                    guide.start[axis] += value - drag.start[axis];
                    guide.end[axis] += value - drag.start[axis];
                }
            }
            let long = screen(guide.start)
                .zip(screen(guide.end))
                .is_some_and(|(a, b)| a.distance(b) >= 8.);
            if long {
                if let Some(index) = drag.index {
                    self.edits.geometry.guides[index] = guide;
                } else {
                    let index = self.edits.geometry.guides.len();
                    self.edits.geometry.guides.push(guide);
                    drag.index = Some(index);
                    drag.endpoint = Some(1);
                    self.framing.selected = Some(index);
                }
                self.changed();
            }
        }
        if !ui.input(|i| i.pointer.primary_down()) && self.framing.guide_drag.take().is_some() {
            self.finish_edit_gesture();
        }
        let painter = ui.painter().with_clip_rect(rect);
        for (i, guide) in self.edits.geometry.guides.iter().enumerate() {
            if let Some((a, b)) = screen(guide.start).zip(screen(guide.end)) {
                let selected = self.framing.selected == Some(i);
                let color = if !guide.enabled {
                    Color32::from_gray(105)
                } else if guide.axis == GuideAxis::Vertical {
                    Color32::from_rgb(110, 200, 245)
                } else {
                    Color32::from_rgb(245, 185, 95)
                };
                painter.line_segment(
                    [a, b],
                    egui::Stroke::new(if selected { 3. } else { 1.5 }, color),
                );
                if selected {
                    for point in [a, b] {
                        painter.circle_filled(point, 4., Color32::WHITE);
                        painter.circle_stroke(point, 5., egui::Stroke::new(1., color));
                    }
                }
            }
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(if hit.is_some() {
                egui::CursorIcon::Grab
            } else {
                egui::CursorIcon::Crosshair
            });
        }
    }
}

fn project(matrix: color::Matrix, p: [f32; 2]) -> Option<[f32; 2]> {
    let p = color::apply(matrix, [p[0], p[1], 1.]);
    (p[2] > 1e-6 && p.iter().all(|v| v.is_finite())).then(|| [p[0] / p[2], p[1] / p[2]])
}
fn from_screen(rect: Rect, region: [f32; 4], p: Pos2) -> [f32; 2] {
    [
        region[0] + (p.x - rect.left()) / rect.width() * region[2],
        region[1] + (p.y - rect.top()) / rect.height() * region[3],
    ]
}
fn to_screen(rect: Rect, region: [f32; 4], p: [f32; 2]) -> Pos2 {
    Pos2::new(
        rect.left() + (p[0] - region[0]) / region[2] * rect.width(),
        rect.top() + (p[1] - region[1]) / region[3] * rect.height(),
    )
}
fn to_screen_crop(rect: Rect, region: [f32; 4], crop: [f32; 4]) -> Rect {
    Rect::from_min_max(
        to_screen(rect, region, [crop[0], crop[1]]),
        to_screen(rect, region, [crop[0] + crop[2], crop[1] + crop[3]]),
    )
}
fn guide_hit(
    guides: &[PerspectiveGuide],
    p: Pos2,
    screen: &impl Fn([f32; 2]) -> Option<Pos2>,
) -> Option<(usize, Option<usize>)> {
    guides
        .iter()
        .enumerate()
        .filter_map(|(i, g)| {
            let (a, b) = screen(g.start).zip(screen(g.end))?;
            let endpoint = if p.distance(a) < 9. {
                Some(0)
            } else if p.distance(b) < 9. {
                Some(1)
            } else {
                None
            };
            let vector = b - a;
            let t = ((p - a).dot(vector) / vector.length_sq().max(1.)).clamp(0., 1.);
            let distance = p.distance(a + vector * t);
            (endpoint.is_some() || distance < 7.).then_some((distance, i, endpoint))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, i, e)| (i, e))
}
fn crop_handles(r: Rect) -> [Pos2; 8] {
    [
        r.left_top(),
        r.center_top(),
        r.right_top(),
        r.right_center(),
        r.right_bottom(),
        r.center_bottom(),
        r.left_bottom(),
        r.left_center(),
    ]
}
fn crop_hit(r: Rect, p: Pos2) -> Option<usize> {
    crop_handles(r)
        .iter()
        .position(|h| h.distance(p) < 10.)
        .or_else(|| {
            if (p.x - r.left()).abs() < 8. && (r.top()..=r.bottom()).contains(&p.y) {
                Some(7)
            } else if (p.x - r.right()).abs() < 8. && (r.top()..=r.bottom()).contains(&p.y) {
                Some(3)
            } else if (p.y - r.top()).abs() < 8. && (r.left()..=r.right()).contains(&p.x) {
                Some(1)
            } else if (p.y - r.bottom()).abs() < 8. && (r.left()..=r.right()).contains(&p.x) {
                Some(5)
            } else {
                r.contains(p).then_some(8)
            }
        })
}
pub(super) fn fit_aspect(c: [f32; 4], ratio: f32) -> [f32; 4] {
    let width = c[2].min(c[3] * ratio);
    let height = width / ratio;
    [
        c[0] + (c[2] - width) * 0.5,
        c[1] + (c[3] - height) * 0.5,
        width,
        height,
    ]
}
pub(super) fn resize_crop(
    c: [f32; 4],
    delta: [f32; 2],
    handle: usize,
    ratio: Option<f32>,
) -> [f32; 4] {
    if handle == 8 {
        return [
            (c[0] + delta[0]).clamp(0., 1. - c[2]),
            (c[1] + delta[1]).clamp(0., 1. - c[3]),
            c[2],
            c[3],
        ];
    }
    let left = matches!(handle, 0 | 6 | 7);
    let right = matches!(handle, 2..=4);
    let top = matches!(handle, 0..=2);
    let bottom = matches!(handle, 4..=6);
    let mut x0 = c[0];
    let mut y0 = c[1];
    let mut x1 = c[0] + c[2];
    let mut y1 = c[1] + c[3];
    let min_width = c[2].min(1e-4);
    let min_height = c[3].min(1e-4);
    if left {
        x0 = (x0 + delta[0]).clamp(0., (x1 - min_width).max(0.));
    }
    if right {
        x1 = (x1 + delta[0]).clamp((x0 + min_width).min(1.), 1.);
    }
    if top {
        y0 = (y0 + delta[1]).clamp(0., (y1 - min_height).max(0.));
    }
    if bottom {
        y1 = (y1 + delta[1]).clamp((y0 + min_height).min(1.), 1.);
    }
    if let Some(r) = ratio {
        let corner = (left || right) && (top || bottom);
        let mut width = x1 - x0;
        let mut height = y1 - y0;
        if corner {
            let anchor = [
                if left { c[0] + c[2] } else { c[0] },
                if top { c[1] + c[3] } else { c[1] },
            ];
            width = width
                .max(height * r)
                .min(if left { anchor[0] } else { 1. - anchor[0] })
                .min((if top { anchor[1] } else { 1. - anchor[1] }) * r);
            height = width / r;
            x0 = if left { anchor[0] - width } else { anchor[0] };
            y0 = if top { anchor[1] - height } else { anchor[1] };
        } else if left || right {
            let center = c[1] + c[3] * 0.5;
            width = width.min(2. * center.min(1. - center) * r);
            height = width / r;
            x0 = if left { c[0] + c[2] - width } else { c[0] };
            y0 = center - height * 0.5;
        } else {
            let center = c[0] + c[2] * 0.5;
            height = height.min(2. * center.min(1. - center) / r);
            width = height * r;
            x0 = center - width * 0.5;
            y0 = if top { c[1] + c[3] - height } else { c[1] };
        }
        return [x0.max(0.), y0.max(0.), width, height];
    }
    [x0.max(0.), y0.max(0.), x1 - x0, y1 - y0]
}
fn draw_grid(painter: egui::Painter, rect: Rect, grid: Grid) {
    let stroke = egui::Stroke::new(1., Color32::from_white_alpha(110));
    let fractions: &[f32] = match grid {
        Grid::None | Grid::Diagonals => &[],
        Grid::Thirds => &[1. / 3., 2. / 3.],
        Grid::Golden => &[0.381966, 0.618034],
        Grid::Center => &[0.5],
        Grid::Architecture => &[1. / 6., 2. / 6., 0.5, 4. / 6., 5. / 6.],
    };
    for &t in fractions {
        painter.line_segment(
            [
                Pos2::new(rect.left() + t * rect.width(), rect.top()),
                Pos2::new(rect.left() + t * rect.width(), rect.bottom()),
            ],
            stroke,
        );
        painter.line_segment(
            [
                Pos2::new(rect.left(), rect.top() + t * rect.height()),
                Pos2::new(rect.right(), rect.top() + t * rect.height()),
            ],
            stroke,
        );
    }
    if grid == Grid::Diagonals {
        painter.line_segment([rect.left_top(), rect.right_bottom()], stroke);
        painter.line_segment([rect.right_top(), rect.left_bottom()], stroke);
    }
}
