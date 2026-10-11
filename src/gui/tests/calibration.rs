use super::*;

fn editor() -> (Editor, mpsc::Sender<Reply>, egui::Context) {
    let ctx = egui::Context::default();
    let mut app = Editor::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        None,
        None,
        Backend::Cpu,
        false,
    );
    let (responses, receive) = mpsc::channel();
    app.rx = receive;
    app.load_generation = 4;
    (app, responses, ctx)
}
fn reply(previous: Edits, revision: u64) -> Reply {
    Reply::Calibrated {
        load_id: 4,
        revision,
        previous,
        sample: crate::calibration::Sample {
            xy: [0.4, 0.38],
            pixels: 1024,
        },
    }
}
#[test]
fn displaying_illuminant_controls_does_not_quantize_or_change_the_recipe() {
    use crate::calibration::Illuminant;
    for illuminant in [
        Illuminant::Legacy,
        Illuminant::Camera,
        Illuminant::Custom {
            xy: [0.42779413, 0.4136972],
        },
        Illuminant::Custom { xy: [0.52, 0.29] },
        Illuminant::Daylight { kelvin: 6504.4 },
        Illuminant::BlackBody { kelvin: 2856.5 },
    ] {
        let (mut app, _, ctx) = editor();
        app.image = Some(Arc::new(
            SensorImage::from_rgb(4, 4, vec![0.2; 48]).unwrap(),
        ));
        app.edits.scene.illuminant = illuminant;
        app.edits.scene.calibration = [1.04, 0.97, 1.12];
        let before = app.edits.clone();
        for i in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(500., 700.))),
                    time: Some(i as f64),
                    ..Default::default()
                },
                |ui| app.calibration_controls(ui),
            );
            output.textures_delta.clear();
            assert_eq!(app.edits, before, "Opening controls changed {illuminant:?}");
        }
    }
}
#[test]
fn calibration_is_one_undoable_step_and_stale_results_preserve_redo() {
    let (mut app, responses, ctx) = editor();
    let original = app.edits.clone();
    app.busy = Some(Busy::Calibrate(4));
    app.zoom = 2.;
    app.changed();
    responses.send(reply(original.clone(), 0)).unwrap();
    app.poll(&ctx);
    let calibrated = app.edits.clone();
    assert_ne!(calibrated, original);
    assert_eq!(app.history.undo_len(), 1);
    assert_eq!(app.busy, None);
    let revision = app.history.revision();
    app.busy = Some(Busy::Calibrate(4));
    app.undo();
    assert_eq!(app.edits, original);
    responses.send(reply(calibrated.clone(), revision)).unwrap();
    app.poll(&ctx);
    assert_eq!(app.edits, original);
    assert!(app.history.can_redo());
    assert_eq!(app.busy, None);
    app.redo();
    assert_eq!(app.edits, calibrated);
    // Returning to the same recipe through undo/redo still invalidates the request.
    app.busy = Some(Busy::Calibrate(4));
    responses.send(reply(calibrated.clone(), revision)).unwrap();
    app.poll(&ctx);
    assert_eq!(app.edits, calibrated);
    assert_eq!(app.history.undo_len(), 1);
    assert!(app.status.contains("discarded"));
}
#[cfg(debug_assertions)]
#[test]
fn box_drag_samples_canvas_coordinates_at_zoom_without_painting_or_panning() {
    for reverse in [false, true] {
        let (mut app, _, ctx) = editor();
        let (requests, receiver) = mpsc::channel();
        app.tx = requests;
        app.image = Some(Arc::new(
            SensorImage::from_rgb(400, 300, vec![0.2; 400 * 300 * 3]).unwrap(),
        ));
        app.tool = Tool::Calibration;
        app.zoom = 2.;
        app.center = [0.6, 0.4];
        app.edits.geometry.crop = [0.1, 0.2, 0.8, 0.7];
        let frame = |app: &mut Editor, time, events| {
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
        };
        let rect = frame(&mut app, 0., vec![]);
        let region = app.viewport;
        let at = |x, y| {
            Pos2::new(
                rect.left() + x * rect.width(),
                rect.top() + y * rect.height(),
            )
        };
        let (start, end) = if reverse {
            (at(0.7, 0.8), at(0.2, 0.3))
        } else {
            (at(0.2, 0.3), at(0.7, 0.8))
        };
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let previous = app.edits.clone();
        let center = app.center;
        frame(
            &mut app,
            0.1,
            vec![egui::Event::PointerMoved(start), button(start, true)],
        );
        frame(&mut app, 0.2, vec![egui::Event::PointerMoved(end)]);
        frame(&mut app, 0.3, vec![button(end, false)]);
        let request = receiver
            .try_iter()
            .find_map(|work| match work {
                Work::Calibrate { region, .. } => Some(region),
                _ => None,
            })
            .expect("Drag did not start estimation");
        let expected = [
            region[0] + 0.2 * region[2],
            region[1] + 0.3 * region[3],
            0.5 * region[2],
            0.5 * region[3],
        ];
        for i in 0..4 {
            assert!(
                (request[i] - expected[i]).abs() < 1e-5,
                "{request:?} vs {expected:?}"
            );
        }
        assert_eq!(app.edits, previous);
        assert_eq!(app.center, center);
        assert!(app.mask.is_empty());
        assert_eq!(app.tool, Tool::View);
    }
}
