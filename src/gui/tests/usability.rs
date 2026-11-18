use super::*;

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

fn slider_frame(ctx: &egui::Context, value: &mut f32, time: f64, events: Vec<egui::Event>) -> Rect {
    let mut rect = Rect::NOTHING;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(420., 300.))),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| {
            rect = sliders::exposure_slider(ui, value).rect;
        },
    );
    output.textures_delta.clear();
    rect
}

fn pointer(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn exposure_track_uses_sidebar_width_and_a_small_drag_is_a_small_correction() {
    let ctx = egui::Context::default();
    preferences::apply_style(&ctx, &Default::default());
    let mut exposure = 0.7;
    let rect = slider_frame(&ctx, &mut exposure, 0., vec![]);
    assert!(rect.width() >= 390., "{rect:?}");
    assert_eq!(exposure, 0.7);
    let start = Pos2::new(rect.center().x, rect.bottom() - 15.);
    slider_frame(
        &ctx,
        &mut exposure,
        0.1,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
    );
    let before = exposure;
    let end = start + Vec2::new(10., 0.);
    slider_frame(
        &ctx,
        &mut exposure,
        0.2,
        vec![egui::Event::PointerMoved(end)],
    );
    slider_frame(&ctx, &mut exposure, 0.3, vec![pointer(end, false)]);
    assert!(
        (0.05..=0.15).contains(&(exposure - before)),
        "10 px changed exposure by {} EV",
        exposure - before
    );
}

#[test]
fn exposure_numeric_input_accepts_values_beyond_the_track_without_clamping_on_redraw() {
    let ctx = egui::Context::default();
    preferences::apply_style(&ctx, &Default::default());
    let mut exposure = 0.7;
    let rect = slider_frame(&ctx, &mut exposure, 0., vec![]);
    let pos = Pos2::new(rect.right() - 35., rect.top() + 15.);
    slider_frame(
        &ctx,
        &mut exposure,
        0.1,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    slider_frame(&ctx, &mut exposure, 0.2, vec![pointer(pos, false)]);
    slider_frame(
        &ctx,
        &mut exposure,
        0.3,
        vec![egui::Event::Text("6.25".into())],
    );
    slider_frame(&ctx, &mut exposure, 0.4, vec![key(egui::Key::Enter)]);
    assert_eq!(exposure, 6.25);
    slider_frame(&ctx, &mut exposure, 0.5, vec![]);
    assert_eq!(exposure, 6.25);
}

#[test]
fn saving_a_module_default_from_the_ui_persists_without_changing_photo_history() {
    let ctx = egui::Context::default();
    let mut app = Editor::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        None,
        None,
        Backend::Cpu,
        false,
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rawpuppy/config.toml");
    app.preferences.path = Some(path.clone());
    let previous = app.edits.clone();
    app.edits.scene.exposure = 1.25;
    app.edits.scene.graduated.exposure = -1.;
    app.edits.geometry.rotation = 10.;
    app.remember(previous);
    let photo = app.edits.clone();
    let revision = app.history.revision();
    let mut draw = |time: f64, events: Vec<egui::Event>| {
        let mut rect = Rect::NOTHING;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600., 400.))),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                rect = ui
                    .scope(|ui| app.module_defaults_button(ui, crate::config::Module::Light))
                    .response
                    .rect;
            },
        );
        out.textures_delta.clear();
        rect
    };
    let pos = draw(0., vec![]).center();
    draw(
        0.1,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    draw(0.2, vec![pointer(pos, false)]);
    let config = crate::config::Config::load_from(&path).unwrap();
    assert_eq!(config.defaults.light.exposure, 1.25);
    assert_eq!(config.defaults, app.preferences.config.defaults);
    assert_eq!(app.edits, photo);
    assert_eq!(app.history.revision(), revision);
    assert!(app.dirty());
    assert!(app.error.is_none());
    let (send, requests) = mpsc::channel();
    app.tx = send;
    app.open(dir.path().join("next.png"));
    let Work::Open { defaults, .. } = requests.try_recv().unwrap() else {
        panic!("Missing photo request");
    };
    assert_eq!(defaults, config.defaults);
}

#[test]
fn escape_leaves_tools_even_after_a_canvas_button_has_focus() {
    let ctx = egui::Context::default();
    let mut app = Editor::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        None,
        None,
        Backend::Cpu,
        false,
    );
    app.image = Some(Arc::new(
        SensorImage::from_rgb(4, 4, vec![0.2; 48]).unwrap(),
    ));
    for tool in [
        Tool::Crop,
        Tool::Perspective,
        Tool::Straighten,
        Tool::Graduated,
        Tool::Clone,
        Tool::Heal,
        Tool::Mask,
        Tool::Calibration,
        Tool::Agx(crate::agx::Pick::Both),
    ] {
        app.set_tool(tool);
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![
                    key(egui::Key::Escape),
                    egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: false,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ui| {
                ctx.memory_mut(|m| m.request_focus(egui::Id::new("tool button")));
                app.canvas_shortcuts(ui);
            },
        );
        output.textures_delta.clear();
        assert_eq!(app.tool, Tool::View, "{tool:?}");
    }
}

#[test]
fn comparison_and_editing_tools_are_mutually_exclusive() {
    let ctx = egui::Context::default();
    let mut app = Editor::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        None,
        None,
        Backend::Cpu,
        false,
    );
    app.set_tool(Tool::Crop);
    app.set_compare(true);
    assert!(app.compare);
    assert_eq!(app.tool, Tool::View);
    app.set_tool(Tool::Clone);
    assert!(!app.compare);
    assert_eq!(app.tool, Tool::Clone);
}
