//! Preview viewport, painting, brush travel and actual-pixel navigation.
use super::*;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) enum Tool {
    View,
    Clone,
    Heal,
    Mask,
    Calibration,
}

#[derive(Default)]
pub(super) struct BrushStroke {
    previous: Option<[f32; 2]>,
}
impl BrushStroke {
    pub(super) fn points(
        &mut self,
        uv: [f32; 2],
        press: Option<[f32; 2]>,
        radius: f32,
        aspect: f32,
    ) -> Vec<[f32; 2]> {
        if press.is_some() {
            self.previous = None;
        }
        let first = self.previous.is_none();
        let start = self.previous.unwrap_or(press.unwrap_or(uv));
        let delta = [
            uv[0] as f64 - start[0] as f64,
            uv[1] as f64 - start[1] as f64,
        ];
        let distance = delta[0].hypot(delta[1] * aspect as f64);
        let spacing = radius as f64 * 0.2;
        if !first && distance < spacing {
            return vec![];
        }
        self.previous = Some(uv);
        // Clip captured drags to the canvas plus a brush radius. Pointer travel
        // outside a narrow photo must not create a huge list of invisible dabs.
        let mut enter = 0f64;
        let mut leave = 1f64;
        for axis in 0..2 {
            let scale = if axis == 0 { 1. } else { aspect as f64 };
            let p = start[axis] as f64 * scale;
            let d = delta[axis] * scale;
            let min = -(radius as f64);
            let max = scale + radius as f64;
            if d == 0. {
                if p < min || p > max {
                    return vec![];
                }
            } else {
                let a = (min - p) / d;
                let b = (max - p) / d;
                enter = enter.max(a.min(b));
                leave = leave.min(a.max(b));
                if enter > leave {
                    return vec![];
                }
            }
        }
        let point = |t: f64| std::array::from_fn(|i| (start[i] as f64 + t * delta[i]) as f32);
        let mut points = Vec::new();
        if first || enter > 0. {
            points.push(point(enter));
        }
        let steps = ((leave - enter) * distance / spacing).ceil() as usize;
        for i in 1..=steps {
            points.push(point(enter + (leave - enter) * i as f64 / steps as f64));
        }
        points
    }
}
impl Editor {
    pub(super) fn canvas(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(20, 22, 24))
                    .inner_margin(18.),
            )
            .show(ui, |ui| {
                if self.image.is_none() {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("One photograph. Your light.")
                                    .size(26.)
                                    .color(Color32::from_gray(195)),
                            );
                            ui.add_space(14.);
                            ui.label("Drop a RAW file here, or open a photograph.");
                            ui.add_space(18.);
                            if ui.button("Open photograph…").clicked() {
                                self.choose_photo();
                            }
                        });
                    });
                    return;
                }
                ui.horizontal(|ui| {
                    if ui.small_button("Fit").clicked() {
                        self.zoom = 1.;
                        self.center = [0.5; 2];
                        self.changed();
                    }
                    if ui.small_button("100%").clicked() {
                        self.actual_pixels(ui);
                    }
                    if ui.selectable_label(self.compare, "Before").clicked() {
                        self.compare = !self.compare;
                        self.changed();
                    }
                    ui.add_space(12.);
                    if matches!(self.tool, Tool::Clone | Tool::Heal) {
                        ui.label(
                            egui::RichText::new("Alt-click source · paint to retouch")
                                .color(ACCENT),
                        );
                    }
                });
                let image = self.image.as_ref().unwrap();
                let w = image.metadata.width as f32 * self.edits.geometry.crop[2];
                let h = image.metadata.height as f32 * self.edits.geometry.crop[3];
                let available = ui.available_size().max(Vec2::splat(1.));
                let fit = (available.x / w).min(available.y / h);
                self.fit_scale = fit;
                let full = Vec2::new(w, h) * fit * self.zoom;
                let visible = Vec2::new(full.x.min(available.x), full.y.min(available.y));
                let center = ui.available_rect_before_wrap().center();
                let rect = Rect::from_center_size(center, visible);
                let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                // Apply navigation before calculating the viewport sent to the worker.
                if response.hovered() {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll.abs() > 0.1 {
                        self.zoom = (self.zoom * (scroll * 0.002).exp()).max(1.);
                    }
                }
                if response.double_clicked() {
                    self.zoom = 1.;
                    self.center = [0.5; 2];
                }
                if response.dragged()
                    && (self.tool == Tool::View || ui.input(|i| i.pointer.middle_down()))
                {
                    let delta = ui.input(|i| i.pointer.delta());
                    self.center[0] -= delta.x / full.x;
                    self.center[1] -= delta.y / full.y;
                }
                let full = Vec2::new(w, h) * fit * self.zoom;
                let visible = Vec2::new(full.x.min(available.x), full.y.min(available.y));
                let rect = Rect::from_center_size(center, visible);
                #[cfg(debug_assertions)]
                {
                    self.probe_photo_rect = Some(rect);
                }
                let fraction = [visible.x / full.x, visible.y / full.y];
                for (i, f) in fraction.iter().enumerate() {
                    self.center[i] = self.center[i].clamp(f * 0.5, 1. - f * 0.5);
                }
                let region = [
                    self.center[0] - fraction[0] * 0.5,
                    self.center[1] - fraction[1] * 0.5,
                    fraction[0],
                    fraction[1],
                ];
                let scale = ui.ctx().pixels_per_point();
                let size = [
                    (visible.x * scale).round().clamp(1., 8192.) as usize,
                    (visible.y * scale).round().clamp(1., 8192.) as usize,
                ];
                if region != self.viewport || size != self.preview_size {
                    self.viewport = region;
                    self.preview_size = size;
                    self.changed();
                }
                // Checkerboard makes out-of-image perspective corners visible.
                let square = 16.;
                for y in 0..(visible.y / square).ceil() as usize {
                    for x in 0..(visible.x / square).ceil() as usize {
                        let r = Rect::from_min_size(
                            rect.min + Vec2::new(x as f32 * square, y as f32 * square),
                            Vec2::splat(square),
                        )
                        .intersect(rect);
                        ui.painter().rect_filled(
                            r,
                            0.,
                            Color32::from_gray(if (x + y) % 2 == 0 { 31 } else { 36 }),
                        );
                    }
                }
                // A pending preview covers its rendered region, not the new viewport.
                // Reproject and clip it so zoom/pan never stretches stale pixels.
                if let Some((photo_rect, uv)) = preview_placement(rect, region, self.texture_region)
                {
                    let painter = ui.painter().with_clip_rect(rect);
                    if let Some(texture) = &self.texture {
                        painter.image(texture.id(), photo_rect, uv, Color32::WHITE);
                    }
                    if let (Some(texture), Some(state)) = (&self.hdr_texture, &self.presentation) {
                        painter.add(texture.callback_with_uv(&state.device, photo_rect, uv));
                    }
                }
                if self.grid {
                    for i in 1..6 {
                        let t = i as f32 / 6.;
                        ui.painter().line_segment(
                            [
                                Pos2::new(rect.left() + t * rect.width(), rect.top()),
                                Pos2::new(rect.left() + t * rect.width(), rect.bottom()),
                            ],
                            egui::Stroke::new(1., Color32::from_white_alpha(80)),
                        );
                        ui.painter().line_segment(
                            [
                                Pos2::new(rect.left(), rect.top() + t * rect.height()),
                                Pos2::new(rect.right(), rect.top() + t * rect.height()),
                            ],
                            egui::Stroke::new(1., Color32::from_white_alpha(80)),
                        );
                    }
                }
                if response.hovered()
                    && matches!(self.tool, Tool::Clone | Tool::Heal | Tool::Mask)
                    && let Some(pos) = response.hover_pos()
                {
                    ui.painter().circle_stroke(
                        pos,
                        self.radius * w * fit * self.zoom,
                        egui::Stroke::new(1., ACCENT),
                    );
                }
                for dab in &self.mask {
                    let pos = Pos2::new(
                        rect.left() + (dab.center[0] - region[0]) / region[2] * rect.width(),
                        rect.top() + (dab.center[1] - region[1]) / region[3] * rect.height(),
                    );
                    if rect.contains(pos) {
                        ui.painter().circle_filled(
                            pos,
                            dab.radius * full.x,
                            Color32::from_rgba_unmultiplied(225, 150, 100, 75),
                        );
                    }
                }
                let press = ui.input(|i| {
                    i.pointer
                        .primary_pressed()
                        .then(|| i.pointer.press_origin())
                        .flatten()
                });
                let press = press.map(|pos| {
                    [
                        region[0] + (pos.x - rect.left()) / rect.width() * region[2],
                        region[1] + (pos.y - rect.top()) / rect.height() * region[3],
                    ]
                });
                let mut painting = false;
                self.calibration_selection(ui, &response, rect, region, press);
                if self.tool == Tool::Mask
                    && !self.compare
                    && self.busy.is_none()
                    && primary_stroke(&response)
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let uv = [
                        region[0] + (pos.x - rect.left()) / rect.width() * region[2],
                        region[1] + (pos.y - rect.top()) / rect.height() * region[3],
                    ];
                    painting = true;
                    self.mask.extend(
                        self.brush_stroke
                            .points(uv, press, self.radius, h / w)
                            .into_iter()
                            .map(|center| crate::synthesis::MaskDab {
                                center,
                                radius: self.radius,
                            }),
                    );
                }
                if matches!(self.tool, Tool::Clone | Tool::Heal)
                    && !self.compare
                    && self.busy.is_none()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let uv = [
                        region[0] + (pos.x - rect.left()) / rect.width() * region[2],
                        region[1] + (pos.y - rect.top()) / rect.height() * region[3],
                    ];
                    if ui.input(|i| i.modifiers.alt)
                        && response.clicked_by(egui::PointerButton::Primary)
                    {
                        self.clone_source = Some(uv);
                    } else if !ui.input(|i| i.modifiers.alt)
                        && primary_stroke(&response)
                        && let Some(source) = self.clone_source
                    {
                        painting = true;
                        let points = self.brush_stroke.points(uv, press, self.radius, h / w);
                        if let Some(start) = points.first() {
                            if !self.stroke_recorded {
                                self.history.begin_stroke(self.edits.clone());
                                self.stroke_recorded = true;
                                self.brush_offset = [source[0] - start[0], source[1] - start[1]];
                            }
                            self.edits
                                .display
                                .retouch
                                .extend(points.into_iter().map(|target| Retouch {
                                    source: [
                                        target[0] + self.brush_offset[0],
                                        target[1] + self.brush_offset[1],
                                    ],
                                    target,
                                    // The cursor uses the cropped output width; saved
                                    // retouch radii use the full corrected canvas width.
                                    radius: self.radius * self.edits.geometry.crop[2],
                                    feather: self.feather,
                                    opacity: 1.,
                                    mode: if self.tool == Tool::Heal {
                                        RetouchMode::Heal
                                    } else {
                                        RetouchMode::Clone
                                    },
                                }));
                            self.changed();
                        }
                    }
                }
                if !painting || !ui.input(|i| i.pointer.primary_down()) {
                    self.stroke_recorded = false;
                    self.brush_stroke = BrushStroke::default();
                }
                if self.preview_pending && self.busy.is_none() {
                    let edits = if self.compare {
                        Edits {
                            geometry: self.edits.geometry.clone(),
                            lens: self.edits.lens.clone(),
                            ..Edits::default()
                        }
                    } else {
                        self.edits.clone()
                    };
                    self.send(Work::Render {
                        id: self.generation,
                        image: self.image.as_ref().unwrap().clone(),
                        edits,
                        region: self.viewport,
                        size: self.preview_size,
                        profile: self.display_resolved.icc.clone(),
                        hdr: self.hdr_white_scale.is_some(),
                    });
                    self.preview_pending = false;
                }
            });
    }

    pub(super) fn actual_pixels(&mut self, ui: &egui::Ui) {
        if self.image.is_some() {
            self.zoom = (1. / (self.fit_scale * ui.ctx().pixels_per_point())).max(1.);
            self.changed();
        }
    }
}

fn preview_placement(rect: Rect, region: [f32; 4], texture: [f32; 4]) -> Option<(Rect, Rect)> {
    let view = Rect::from_min_size(
        Pos2::new(region[0], region[1]),
        Vec2::new(region[2], region[3]),
    );
    let source = Rect::from_min_size(
        Pos2::new(texture[0], texture[1]),
        Vec2::new(texture[2], texture[3]),
    );
    let overlap = view.intersect(source);
    if !overlap.is_positive() {
        return None;
    }
    let screen = |p: Pos2| rect.min + (p - view.min) / view.size() * rect.size();
    let uv = |p: Pos2| Pos2::ZERO + (p - source.min) / source.size();
    Some((
        Rect::from_min_max(screen(overlap.min), screen(overlap.max)),
        Rect::from_min_max(uv(overlap.min), uv(overlap.max)),
    ))
}

pub(super) fn primary_stroke(response: &egui::Response) -> bool {
    !response.ctx.input(|i| i.pointer.middle_down())
        && (response.clicked_by(egui::PointerButton::Primary)
            || response.dragged_by(egui::PointerButton::Primary)
            || response.drag_stopped_by(egui::PointerButton::Primary)
            || (response.is_pointer_button_down_on()
                && response.ctx.input(|i| i.pointer.primary_down())))
}
