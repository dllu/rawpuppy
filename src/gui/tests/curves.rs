use super::*;
fn frame(
    ctx: &egui::Context,
    points: &mut Vec<[f32; 2]>,
    time: f64,
    events: Vec<egui::Event>,
) -> Rect {
    let mut rectangle = Rect::NOTHING;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400., 300.))),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| rectangle = curve_editor(ui, points),
    );
    output.textures_delta.clear();
    rectangle
}
fn button(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}
#[test]
fn a_curve_drag_keeps_its_original_point_across_other_handles() {
    let ctx = egui::Context::default();
    let mut points = vec![[0., 0.], [0.25, 0.25], [0.75, 0.75], [1., 1.]];
    let rect = frame(&ctx, &mut points, 0., vec![]);
    let at = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + rect.width() * x,
            rect.bottom() - rect.height() * y,
        )
    };
    let start = at(0.25, 0.25);
    frame(
        &ctx,
        &mut points,
        0.1,
        vec![egui::Event::PointerMoved(start), button(start, true)],
    );
    frame(
        &ctx,
        &mut points,
        0.2,
        vec![egui::Event::PointerMoved(at(0.25, 0.35))],
    );
    assert!((points[1][1] - 0.35).abs() < 1e-5);
    let end = at(0.76, 0.6);
    frame(&ctx, &mut points, 0.3, vec![egui::Event::PointerMoved(end)]);
    assert!(
        (points[1][1] - 0.6).abs() < 1e-5,
        "Drag changed a different point: {points:?}"
    );
    assert_eq!(points[2], [0.75, 0.75]);
    frame(&ctx, &mut points, 0.4, vec![button(end, false)]);
    let second = at(0.75, 0.75);
    frame(
        &ctx,
        &mut points,
        0.5,
        vec![egui::Event::PointerMoved(second), button(second, true)],
    );
    frame(
        &ctx,
        &mut points,
        0.6,
        vec![egui::Event::PointerMoved(at(0.75, 0.85))],
    );
    assert!((points[2][1] - 0.85).abs() < 1e-5);
    assert!((points[1][1] - 0.6).abs() < 1e-5);
    assert_eq!(points[0], [0., 0.]);
    assert_eq!(points[3], [1., 1.]);
}
