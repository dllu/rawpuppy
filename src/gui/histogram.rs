//! RGB distribution of the visible preview in sRGB coordinates.
use super::*;

const BINS: usize = 128;
const CHANNEL_COLORS: [[u8; 3]; 3] = [[214, 39, 41], [40, 180, 65], [35, 72, 224]];

pub(super) struct Histogram(Box<[[f32; BINS]; 3]>);

impl Histogram {
    pub(super) fn from_pixels(pixels: &[[f32; 4]]) -> Self {
        let mut bins = Box::new([[0_f32; BINS]; 3]);
        for &[r, g, b, alpha] in pixels {
            if alpha < 0.5 {
                continue;
            }
            for (channel, value) in bins.iter_mut().zip([r, g, b]) {
                if !value.is_nan() {
                    let bin = (color::srgb_encode(value.clamp(0., 1.)) * BINS as f32) as usize;
                    channel[bin.min(BINS - 1)] += 1.;
                }
            }
        }

        // A shared scale keeps a peak of the same area equally tall in every channel.
        let peak = bins.iter().flatten().copied().fold(1., f32::max);
        for count in bins.iter_mut().flatten() {
            *count = (*count / peak).sqrt();
        }
        Self(bins)
    }

    pub(super) fn show(&self, ui: &mut egui::Ui) {
        let (outer, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 64.), egui::Sense::hover());
        response.on_hover_text("Red, green, and blue · preview in sRGB");
        let painter = ui.painter().with_clip_rect(outer);
        painter.rect_filled(outer, 4., ui.visuals().extreme_bg_color);
        let rect = outer.shrink(2.);

        for (bins, [r, g, b]) in self.0.iter().zip(CHANNEL_COLORS) {
            let fill = Color32::from_rgb_additive(r / 5, g / 5, b / 5);
            let outline = Color32::from_rgb_additive(r, g, b);
            let mut mesh = egui::Mesh::default();
            let mut edge = Vec::with_capacity(BINS);
            for (i, &height) in bins.iter().enumerate() {
                let x = rect.left() + rect.width() * i as f32 / (BINS - 1) as f32;
                let y = rect.bottom() - height * rect.height();
                let point = Pos2::new(x, y);
                edge.push(point);
                mesh.colored_vertex(point, fill);
                mesh.colored_vertex(Pos2::new(x, rect.bottom()), fill);
                if i > 0 {
                    let i = (2 * i) as u32;
                    mesh.add_triangle(i - 2, i - 1, i);
                    mesh.add_triangle(i, i - 1, i + 1);
                }
            }
            painter.add(egui::Shape::mesh(mesh));
            painter.add(egui::Shape::line(edge, egui::Stroke::new(1., outline)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_peaks_share_a_scale_and_ignore_geometric_gaps() {
        let pixels = [
            [1., 0.25, -0.1, 1.],
            [1., 0.25, 1.5, 1.],
            [1., 0.75, 1.5, 0.8],
            [1., 0.25, 0., 1.],
            [0., 0., 0., 0.3],
        ];
        let Histogram(bins) = Histogram::from_pixels(&pixels);
        let sample = |v| (color::srgb_encode(v) * BINS as f32) as usize;
        assert_eq!(bins[0][BINS - 1], 1.);
        assert_eq!(bins[0][0], 0.);
        assert_eq!(bins[1][sample(0.25)], 0.75_f32.sqrt());
        assert_eq!(bins[1][sample(0.75)], 0.5);
        assert_eq!(bins[2][0], 0.5_f32.sqrt());
        assert_eq!(bins[2][BINS - 1], 0.5_f32.sqrt());
    }
}
