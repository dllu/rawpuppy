use super::*;
#[test]
fn captured_brush_travel_outside_the_canvas_does_not_stamp_invisible_paths() {
    for aspect in [1., 17. / 100_003.] {
        let mut stroke = BrushStroke::default();
        assert_eq!(
            stroke.points([0.5; 2], Some([0.5; 2]), 0.025, aspect).len(),
            1
        );
        let outbound = stroke.points([10_000_000.; 2], None, 0.025, aspect);
        assert!(!outbound.is_empty());
        assert!(
            outbound.len() < 500,
            "Off-canvas travel produced excessive dabs"
        );
        assert!(
            stroke
                .points([20_000_000.; 2], None, 0.025, aspect)
                .is_empty()
        );
        let inbound = stroke.points([0.5; 2], None, 0.025, aspect);
        assert!(!inbound.is_empty());
        assert!(inbound.len() < 500);
        for point in outbound.into_iter().chain(inbound) {
            assert!((-0.02501..=1.02501).contains(&point[0]));
            assert!((-0.02501..=aspect + 0.02501).contains(&(point[1] * aspect)));
        }
    }
}
fn frame(ctx: &egui::Context, time: f64, events: Vec<egui::Event>) -> (Rect, bool, bool) {
    let mut result = (Rect::NOTHING, false, false);
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400., 300.))),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(300., 150.), egui::Sense::click_and_drag());
            result = (
                rect,
                primary_stroke(&response),
                response.dragged_by(egui::PointerButton::Middle),
            );
        },
    );
    output.textures_delta.clear();
    result
}
fn button(pos: Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}
#[cfg(debug_assertions)]
fn canvas_frame(
    ctx: &egui::Context,
    app: &mut Editor,
    time: f64,
    events: Vec<egui::Event>,
) -> Rect {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(500., 400.))),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| app.canvas(ui),
    );
    output.textures_delta.clear();
    app.probe_photo_rect.unwrap()
}
#[test]
fn middle_button_pans_without_painting_and_primary_drag_paints() {
    let ctx = egui::Context::default();
    let (rect, _, _) = frame(&ctx, 0., vec![]);
    let start = rect.center();
    frame(
        &ctx,
        0.1,
        vec![
            egui::Event::PointerMoved(start),
            button(start, egui::PointerButton::Middle, true),
        ],
    );
    let end = start + Vec2::new(20., 10.);
    let (_, paint, pan) = frame(&ctx, 0.2, vec![egui::Event::PointerMoved(end)]);
    assert!(pan, "Middle-button drag must remain a pan gesture");
    assert!(!paint, "Pan gesture also painted a brush stroke");
    frame(
        &ctx,
        0.3,
        vec![button(end, egui::PointerButton::Middle, false)],
    );
    frame(
        &ctx,
        0.4,
        vec![
            egui::Event::PointerMoved(start),
            button(start, egui::PointerButton::Primary, true),
        ],
    );
    let (_, paint, pan) = frame(&ctx, 0.5, vec![egui::Event::PointerMoved(end)]);
    assert!(paint, "Primary drag must still paint");
    assert!(!pan);
}

#[cfg(debug_assertions)]
#[test]
fn cropped_clone_and_heal_match_the_visible_circular_brush() {
    use crate::pipeline::Pipeline;
    for (width, height, crop) in [
        (320, 240, [0., 0., 1., 1.]),
        (320, 240, [0.25, 0.2, 0.5, 0.6]),
        (240, 320, [0.3, 0.1, 0.25, 0.8]),
    ] {
        for mode in [Tool::Clone, Tool::Heal] {
            let ctx = egui::Context::default();
            let mut app = Editor::new(
                &eframe::CreationContext::_new_kittest(ctx.clone()),
                None,
                None,
                Backend::Cpu,
                false,
            );
            let pixels: Vec<_> = (0..width * height)
                .flat_map(|i| {
                    let x = (i % width) as f32 / width as f32;
                    let y = (i / width) as f32 / height as f32;
                    [x * x, y * y, 0.2]
                })
                .collect();
            let original = pixels.clone();
            let source = Arc::new(SensorImage::from_rgb(width, height, pixels).unwrap());
            app.image = Some(source.clone());
            app.edits.tone.mapper = ToneMapper::Linear;
            app.edits.geometry.crop = crop;
            let base_edits = app.edits.clone();
            app.tool = mode;
            app.radius = 0.1;
            app.feather = 0.;
            app.clone_source = Some([0.25; 2]);
            let rect = canvas_frame(&ctx, &mut app, 0., vec![]);
            let target = rect.center();
            canvas_frame(
                &ctx,
                &mut app,
                0.1,
                vec![
                    egui::Event::PointerMoved(target),
                    button(target, egui::PointerButton::Primary, true),
                ],
            );
            canvas_frame(
                &ctx,
                &mut app,
                0.2,
                vec![button(target, egui::PointerButton::Primary, false)],
            );
            assert_eq!(app.edits.display.retouch.len(), 1);
            let directory = tempfile::tempdir().unwrap();
            let recipe = directory.path().join("photo.png.rawpuppy.xmp");
            sidecar::save(&recipe, &app.edits).unwrap();
            let saved = sidecar::load(&recipe).unwrap();
            let painted = Pipeline::compile(&source, &saved).unwrap();
            let base = Pipeline::compile(&source, &base_edits).unwrap();
            // Sample physical circles just inside and outside the cursor.
            // This tests saved recipe behavior, not only radius arithmetic.
            for i in 0..8 {
                let (sin, cos) = (i as f32 * std::f32::consts::FRAC_PI_4).sin_cos();
                for (distance, changed) in [(0.095, true), (0.105, false)] {
                    let uv = [
                        0.5 + distance * cos,
                        0.5 + distance * sin * rect.width() / rect.height(),
                    ];
                    let before = base.sample(uv);
                    let after = painted.sample(uv);
                    let delta = (0..3)
                        .map(|c| (before[c] - after[c]).abs())
                        .fold(0., f32::max);
                    assert_eq!(
                        delta > 1e-5,
                        changed,
                        "Brush footprint differs from cursor: crop={crop:?}, uv={uv:?}, delta={delta}"
                    );
                }
            }
            assert_eq!(source.data, original);
        }
    }
}

#[cfg(debug_assertions)]
#[test]
fn fast_brush_drags_cover_the_path_and_separate_gestures_do_not_connect() {
    use crate::pipeline::{Pipeline, Rendered};
    for (tool, traffic) in [Tool::Clone, Tool::Heal, Tool::Mask]
        .into_iter()
        .flat_map(|tool| [0, 1, 2].map(|traffic| (tool, traffic)))
    {
        let ctx = egui::Context::default();
        let mut app = Editor::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            None,
            None,
            Backend::Cpu,
            false,
        );
        let pixels = (0..320 * 480)
            .flat_map(|i| {
                let x = (i % 320) as f32 / 320.;
                let y = (i / 320) as f32 / 480.;
                [x * x, y * y, 0.2]
            })
            .collect();
        let source = Arc::new(SensorImage::from_rgb(320, 480, pixels).unwrap());
        app.image = Some(source.clone());
        app.edits.tone.mapper = ToneMapper::Linear;
        app.edits.geometry.crop = [0.1, 0.1, 0.75, 0.8];
        let base_edits = app.edits.clone();
        app.tool = tool;
        app.radius = 0.025;
        app.feather = 0.;
        app.clone_source = Some([0.1; 2]);
        let rect = canvas_frame(&ctx, &mut app, 0., vec![]);
        let at = |uv: [f32; 2]| {
            Pos2::new(
                rect.left() + uv[0] * rect.width(),
                rect.top() + uv[1] * rect.height(),
            )
        };
        let start = at([0.35, 0.25]);
        let end = at([0.65, 0.75]);
        let mut press_events = vec![
            egui::Event::PointerMoved(start),
            button(start, egui::PointerButton::Primary, true),
        ];
        if traffic == 1 {
            press_events.push(egui::Event::PointerMoved(end));
        }
        canvas_frame(&ctx, &mut app, 0.1, press_events);
        let motion = if traffic == 2 {
            at([0.365, 0.275])
        } else {
            end
        };
        canvas_frame(&ctx, &mut app, 0.2, vec![egui::Event::PointerMoved(motion)]);
        canvas_frame(
            &ctx,
            &mut app,
            0.3,
            vec![button(end, egui::PointerButton::Primary, false)],
        );
        let second = at([0.15, 0.75]);
        canvas_frame(
            &ctx,
            &mut app,
            0.4,
            vec![
                egui::Event::PointerMoved(second),
                button(second, egui::PointerButton::Primary, true),
            ],
        );
        canvas_frame(
            &ctx,
            &mut app,
            0.5,
            vec![button(second, egui::PointerButton::Primary, false)],
        );
        let painted = Pipeline::compile(&source, &app.edits).unwrap();
        let base = Pipeline::compile(&source, &base_edits).unwrap();
        let selected = |uv: [f32; 2]| {
            if tool == Tool::Mask {
                let pixel = Rendered {
                    width: 1,
                    height: 1,
                    pixels: vec![[0., 0., 0., 1.]],
                };
                crate::synthesis::context_mask(
                    &pixel,
                    [uv[0] - 0.0005, uv[1] - 0.0005, 0.001, 0.001],
                    &app.mask,
                    false,
                    rect.height() / rect.width(),
                )[0] == 1.
            } else {
                let before = base.sample(uv);
                let after = painted.sample(uv);
                (0..3).any(|c| (before[c] - after[c]).abs() > 1e-5)
            }
        };
        for i in 0..=100 {
            let t = i as f32 / 100.;
            // Offset slightly from the centerline: a heal deliberately
            // preserves its center's local color.
            let uv = [0.358 + t * 0.3, 0.25 + t * 0.5];
            assert!(selected(uv), "Fast drag left a hole at {uv:?}");
        }
        assert!(!selected([0.4, 0.75]), "Separate strokes were connected");
        if tool != Tool::Mask {
            assert_eq!(app.history.undo_len(), 2, "Undo must retain whole gestures");
        }
    }
}
