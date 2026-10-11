//! Tone-curve widget with stable drag handles.
use super::*;

pub(super) fn curve_editor(ui: &mut egui::Ui, points: &mut Vec<[f32; 2]>) -> Rect {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 150.),
        egui::Sense::click_and_drag(),
    );
    ui.painter().rect_filled(rect, 4., Color32::from_gray(20));
    let lut = color::curve_lut(points, 128);
    let line: Vec<_> = lut
        .iter()
        .enumerate()
        .map(|(i, y)| {
            Pos2::new(
                rect.left() + i as f32 / 127. * rect.width(),
                rect.bottom() - y * rect.height(),
            )
        })
        .collect();
    ui.painter()
        .add(egui::Shape::line(line, egui::Stroke::new(1.5, ACCENT)));
    for p in points.iter() {
        ui.painter().circle_filled(
            Pos2::new(
                rect.left() + p[0] * rect.width(),
                rect.bottom() - p[1] * rect.height(),
            ),
            3.,
            Color32::WHITE,
        );
    }
    let dragged_point = response.id.with("dragged-curve-point");
    if response.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        let x = ((origin.x - rect.left()) / rect.width()).clamp(0., 1.);
        let nearest = points
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| (a[0] - x).abs().total_cmp(&(b[0] - x).abs()))
            .map(|(i, _)| i)
            .unwrap();
        ui.data_mut(|data| data.insert_temp(dragged_point, nearest));
    }
    if response.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
        ui.data_mut(|data| data.remove::<usize>(dragged_point));
    }
    if response.double_clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let x = ((pos.x - rect.left()) / rect.width()).clamp(0.01, 0.99);
        let i = points.partition_point(|p| p[0] < x);
        if points.iter().all(|p| (p[0] - x).abs() > 0.01) {
            let y = ((rect.bottom() - pos.y) / rect.height()).clamp(points[i - 1][1], points[i][1]);
            points.insert(i, [x, y]);
        }
    } else if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some(nearest) = ui
            .data(|data| data.get_temp::<usize>(dragged_point))
            .filter(|index| *index < points.len())
    {
        let lower = if nearest == 0 {
            0.
        } else {
            points[nearest - 1][1]
        };
        let upper = if nearest == points.len() - 1 {
            1.
        } else {
            points[nearest + 1][1]
        };
        points[nearest][1] = ((rect.bottom() - pos.y) / rect.height()).clamp(lower, upper);
    }
    ui.label(
        egui::RichText::new("Double-click to add a point; drag to shape.")
            .small()
            .color(Color32::GRAY),
    );
    if ui.small_button("Reset curve").clicked() {
        *points = vec![[0., 0.], [1., 1.]];
        ui.data_mut(|data| data.remove::<usize>(dragged_point));
    }
    rect
}
