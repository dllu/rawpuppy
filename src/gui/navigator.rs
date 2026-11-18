//! A bounded overview of the active canvas and its window at the current zoom.
use super::*;
use crate::{edits::DetailEdits, synthesis::GeneratedFill};
use std::sync::Weak;

const LONG_EDGE: f32 = 256.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Overview {
    pub(super) frame: [f32; 4],
    pub(super) size: [usize; 2],
}
impl Overview {
    pub(super) fn new(frame: [f32; 4], dimensions: [f32; 2]) -> Self {
        let scale = LONG_EDGE / dimensions[0].max(dimensions[1]);
        Self {
            frame,
            size: dimensions.map(|v| (v * scale).round().max(1.) as usize),
        }
    }
}

pub(super) fn relative_region(parent: [f32; 4], region: [f32; 4]) -> [f32; 4] {
    [
        (region[0] - parent[0]) / parent[2],
        (region[1] - parent[1]) / parent[3],
        region[2] / parent[2],
        region[3] / parent[3],
    ]
}

pub(super) struct Thumbnail {
    view: Overview,
    pixels: Vec<u8>,
}

pub(super) struct CachedThumbnail {
    source: Weak<SensorImage>,
    recipe: String,
    detail: DetailEdits,
    fills: Vec<GeneratedFill>,
    profile: Option<[u8; 32]>,
    prepared: bool,
    thumbnail: Arc<Thumbnail>,
}

impl CachedThumbnail {
    #[expect(clippy::too_many_arguments)]
    pub(super) fn request(
        cache: &mut Option<Self>,
        view: Overview,
        renderer: &mut Renderer,
        encoder: &mut display::Encoder,
        source: Arc<SensorImage>,
        edits: &Edits,
        recipe: String,
        profile: Option<&display::Icc>,
        prepared: bool,
    ) -> Result<Arc<Thumbnail>> {
        let identity = Arc::downgrade(&source);
        let profile_digest = profile.map(|icc| icc.digest);
        if let Some(entry) = cache
            && entry.source.ptr_eq(&identity)
            && entry.thumbnail.view == view
            && entry.recipe == recipe
            && entry.detail == edits.detail
            && entry.fills == edits.display.synthesis
            && entry.profile == profile_digest
            && entry.prepared == prepared
        {
            return Ok(entry.thumbnail.clone());
        }
        let (rendered, progress) = renderer.render_preview_region(
            source,
            edits,
            relative_region(edits.geometry.crop, view.frame),
            view.size[0],
            view.size[1],
        )?;
        let thumbnail = Arc::new(Thumbnail {
            view,
            pixels: encoder.encode(&rendered, profile)?,
        });
        *cache = Some(Self {
            source: identity,
            recipe,
            detail: edits.detail.clone(),
            fills: edits.display.synthesis.clone(),
            profile: profile_digest,
            prepared: progress.is_none(),
            thumbnail: thumbnail.clone(),
        });
        Ok(thumbnail)
    }
}

#[derive(Default)]
pub(super) struct Navigator {
    pub(super) open: bool,
    thumbnail: Option<Arc<Thumbnail>>,
    texture: Option<egui::TextureHandle>,
    drag_offset: Vec2,
}

impl Navigator {
    pub(super) fn update(&mut self, ctx: &egui::Context, next: Option<Arc<Thumbnail>>) {
        let Some(next) = next else {
            return;
        };
        if self
            .thumbnail
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, &next))
        {
            return;
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(next.view.size, &next.pixels);
        if let Some(texture) = &mut self.texture {
            texture.set(image, egui::TextureOptions::LINEAR);
        } else {
            self.texture = Some(ctx.load_texture("navigator", image, egui::TextureOptions::LINEAR));
        }
        self.thumbnail = Some(next);
    }
}

impl Editor {
    pub(super) fn navigator_controls(&mut self, ui: &mut egui::Ui) {
        let was_open = self.navigator.open;
        let caption = if self.zoom == 1. {
            "Navigator · Fit".into()
        } else {
            let scale = self.fit_scale * self.zoom * ui.ctx().pixels_per_point();
            format!("Navigator · {:.0}%", 100. * scale)
        };
        let response = egui::CollapsingHeader::new(caption)
            .id_salt("navigator")
            .default_open(true)
            .show(ui, |ui| self.navigator_view(ui));
        self.navigator.open = response.body_returned.is_some();
        if self.navigator.open && !was_open {
            self.preview_pending = true;
        }
    }

    fn navigator_view(&mut self, ui: &mut egui::Ui) {
        let Some(image) = &self.image else { return };
        let frame = self.framing.render_crop;
        let dimensions = Vec2::new(
            image.metadata.width as f32 * frame[2],
            image.metadata.height as f32 * frame[3],
        );
        let row_size = Vec2::new(ui.available_width(), 112.);
        let (row, _) = ui.allocate_exact_size(row_size, egui::Sense::hover());
        let size = dimensions * (row.width() / dimensions.x).min(row.height() / dimensions.y);
        let rect = Rect::from_center_size(row.center(), size);
        let response = ui.interact(
            rect,
            ui.id().with("overview"),
            egui::Sense::click_and_drag(),
        );
        let [x, y, w, h] = self.viewport;
        let window = Rect::from_min_size(rect.min + Vec2::new(x, y) * size, Vec2::new(w, h) * size);
        let position = |p: Pos2| (p - rect.min) / size;
        let primary_down = ui.input(|i| i.pointer.primary_down());
        if response.is_pointer_button_down_on()
            && let Some(press) = ui.input(|i| {
                i.pointer
                    .primary_pressed()
                    .then(|| i.pointer.press_origin())
                    .flatten()
            })
        {
            self.navigator.drag_offset = if window.contains(press) {
                Vec2::from(self.center) - position(press)
            } else {
                Vec2::ZERO
            };
        }
        if (response.clicked_by(egui::PointerButton::Primary)
            || response.dragged_by(egui::PointerButton::Primary)
            || (response.is_pointer_button_down_on() && primary_down))
            && let Some(p) = response.interact_pointer_pos()
        {
            self.center = (position(p) + self.navigator.drag_offset)
                .clamp(Vec2::ZERO, Vec2::splat(1.))
                .into();
        }
        if let Some(p) = response.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.1 {
                let anchor = position(p).into();
                if !window.contains(p) || self.zoom == 1. {
                    self.center = anchor;
                }
                self.zoom_towards(anchor, scroll);
            }
        }

        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 2., ui.visuals().extreme_bg_color);
        if let Some(thumbnail) = &self.navigator.thumbnail
            && let Some(texture) = &self.navigator.texture
        {
            // A previous thumbnail can remain visible while the worker refreshes it.
            // Keep the shared world coordinates even when entering or leaving Crop.
            if let Some((destination, uv)) =
                super::canvas::preview_placement(rect, frame, thumbnail.view.frame)
            {
                painter.image(texture.id(), destination, uv, Color32::WHITE);
            }
        }
        painter.rect_stroke(
            window.intersect(rect),
            0.,
            egui::Stroke::new(3., Color32::BLACK),
            egui::StrokeKind::Inside,
        );
        painter.rect_stroke(
            window.intersect(rect),
            0.,
            egui::Stroke::new(1.25, ACCENT),
            egui::StrokeKind::Inside,
        );
        response
            .on_hover_cursor(if primary_down {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            })
            .on_hover_text("Click or drag to pan · scroll to zoom");
    }
}
