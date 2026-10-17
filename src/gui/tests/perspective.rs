use super::*;

#[test]
fn showing_controls_preserves_automatic_poses_outside_the_slider_drag_range() {
    let ctx = egui::Context::default();
    let mut pitch = -52.5;
    let mut yaw = 51.;
    let mut fov = 12.5;
    let mut scale = 0.28;
    for frame in 0..3 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600., 400.))),
                time: Some(frame as f64),
                ..Default::default()
            },
            |ui| {
                controls::slider(ui, &mut pitch, -45. ..=45., "Vertical", "°");
                controls::slider(ui, &mut yaw, -45. ..=45., "Horizontal", "°");
                controls::slider(ui, &mut fov, 15. ..=120., "Field of view", "°");
                controls::slider(ui, &mut scale, 0.5..=3., "Scale", "×");
            },
        );
        output.textures_delta.clear();
        assert_eq!([pitch, yaw, fov, scale], [-52.5, 51., 12.5, 0.28]);
    }
}

#[test]
fn fine_rotation_preserves_estimates_and_arrow_keys_step_by_a_tenth() {
    let ctx = egui::Context::default();
    let mut angle = -0.7281773;
    let mut id = None;
    for frame in 0..3 {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600., 400.))),
                time: Some(frame as f64),
                ..Default::default()
            },
            |ui| {
                let r = controls::angle_slider(ui, &mut angle, -5. ..=5., "Rotation");
                id = Some(r.id);
            },
        );
        out.textures_delta.clear();
        assert_eq!(angle, -0.7281773);
    }
    angle = 0.;
    ctx.memory_mut(|m| m.request_focus(id.unwrap()));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600., 400.))),
            time: Some(4.),
            events: vec![egui::Event::Key {
                key: egui::Key::ArrowRight,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ui| {
            controls::angle_slider(ui, &mut angle, -5. ..=5., "Rotation");
        },
    );
    out.textures_delta.clear();
    assert!((angle - 0.1).abs() < 0.00001, "{angle}");
}
