use super::*;
use crate::edits::{GuideAxis, PerspectiveGuide};

fn editor() -> (Editor, egui::Context, mpsc::Receiver<Work>) {
    let ctx = egui::Context::default();
    let mut app = Editor::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        None,
        None,
        Backend::Cpu,
        false,
    );
    let (send, receive) = mpsc::channel();
    app.tx = send;
    app.image = Some(Arc::new(
        SensorImage::from_rgb(600, 400, vec![0.2; 600 * 400 * 3]).unwrap(),
    ));
    (app, ctx, receive)
}
fn frame(app: &mut Editor, ctx: &egui::Context, time: f64, events: Vec<egui::Event>) -> Rect {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 700.))),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| {
            app.controls(ui);
            app.canvas(ui);
        },
    );
    output.textures_delta.clear();
    app.probe_photo_rect.unwrap()
}
fn button(pos: Pos2, pressed: bool, button: egui::PointerButton) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button,
        modifiers: egui::Modifiers::NONE,
    }
}
fn drag(app: &mut Editor, ctx: &egui::Context, time: f64, start: Pos2, end: Pos2) {
    frame(
        app,
        ctx,
        time,
        vec![
            egui::Event::PointerMoved(start),
            button(start, true, egui::PointerButton::Primary),
        ],
    );
    frame(app, ctx, time + 0.1, vec![egui::Event::PointerMoved(end)]);
    frame(
        app,
        ctx,
        time + 0.2,
        vec![button(end, false, egui::PointerButton::Primary)],
    );
}
#[test]
fn dragging_crop_edges_is_one_undo_step_and_previews_the_uncropped_canvas() {
    let (mut app, ctx, requests) = editor();
    app.edits.geometry.crop = [0.2, 0.2, 0.6, 0.6];
    let before = app.edits.clone();
    app.set_tool(Tool::Crop);
    let rect = frame(&mut app, &ctx, 0., vec![]);
    let Work::Render { edits, .. } = requests.try_recv().unwrap() else {
        panic!("Missing full-canvas preview");
    };
    assert_eq!(
        edits, before,
        "Framing must retain the photo's retouch/layer coordinate system"
    );
    let at = |x, y| rect.min + Vec2::new(x * rect.width(), y * rect.height());
    drag(&mut app, &ctx, 0.1, at(0.8, 0.8), at(0.7, 0.65));
    let after = app.edits.clone();
    for (a, b) in after.geometry.crop.into_iter().zip([0.2, 0.2, 0.5, 0.45]) {
        assert!((a - b).abs() < 1e-5);
    }
    assert_eq!(app.history.undo_len(), 1);
    app.undo();
    assert_eq!(app.edits, before);
    app.redo();
    assert_eq!(app.edits, after);
    assert!(
        requests
            .try_iter()
            .filter_map(|w| if let Work::Render { edits, region, .. } = w {
                Some((edits, region))
            } else {
                None
            })
            .all(|(e, r)| {
                let c = e.geometry.crop;
                let visible = [
                    c[0] + r[0] * c[2],
                    c[1] + r[1] * c[3],
                    r[2] * c[2],
                    r[3] * c[3],
                ];
                visible
                    .into_iter()
                    .zip([0., 0., 1., 1.])
                    .all(|(a, b)| (a - b).abs() < 1e-5)
            })
    );
    app.set_tool(Tool::View);
    frame(&mut app, &ctx, 0.5, vec![]);
    let crop = requests
        .try_iter()
        .find_map(|w| {
            if let Work::Render { edits, .. } = w {
                Some(edits.geometry.crop)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(crop, after.geometry.crop);
}
#[test]
fn aspect_locked_corner_and_edge_drags_stay_inside_the_canvas() {
    for handle in 0..9 {
        for delta in [[-0.9, -0.9], [-0.1, 0.05], [0.1, 0.1], [0.9, 0.9]] {
            let crop =
                super::super::framing::resize_crop([0.2, 0.2, 0.6, 0.6], delta, handle, Some(1.));
            assert!(
                crop[0] >= 0.
                    && crop[1] >= 0.
                    && crop[2] > 0.
                    && crop[3] > 0.
                    && crop[0] + crop[2] <= 1.000001
                    && crop[1] + crop[3] <= 1.000001,
                "{crop:?}"
            );
            assert!((crop[2] / crop[3] - 1.).abs() < 1e-5, "{crop:?}");
        }
    }
}
#[test]
fn perspective_segments_can_be_drawn_edited_deleted_and_restored_on_a_corrected_view() {
    let (mut app, ctx, _) = editor();
    app.edits.geometry.pitch = 12.;
    app.edits.geometry.rotation = 3.;
    app.set_tool(Tool::Perspective);
    let rect = frame(&mut app, &ctx, 0., vec![]);
    let at = |x, y| rect.min + Vec2::new(x * rect.width(), y * rect.height());
    let a = at(0.3, 0.15);
    let b = at(0.34, 0.85);
    drag(&mut app, &ctx, 0.1, a, b);
    assert_eq!(app.edits.geometry.guides.len(), 1);
    assert_eq!(app.history.undo_len(), 1);
    let guide = app.edits.geometry.guides[0].clone();
    let geometry = crate::geometry::Geometry::compile(&app.edits.geometry, 600, 400).unwrap();
    let expected = geometry.map([0.34, 0.85], 1).unwrap();
    for (actual, expected) in guide.end.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-5);
    }
    let end = at(0.45, 0.75);
    drag(&mut app, &ctx, 0.4, b, end);
    assert_ne!(app.edits.geometry.guides[0], guide);
    assert_eq!(app.history.undo_len(), 2);
    let midpoint = a + (end - a) * 0.5;
    frame(
        &mut app,
        &ctx,
        0.7,
        vec![
            egui::Event::PointerMoved(midpoint),
            button(midpoint, true, egui::PointerButton::Secondary),
        ],
    );
    frame(
        &mut app,
        &ctx,
        0.8,
        vec![button(midpoint, false, egui::PointerButton::Secondary)],
    );
    assert!(app.edits.geometry.guides.is_empty());
    app.undo();
    assert_eq!(app.edits.geometry.guides.len(), 1);
    app.undo();
    assert_eq!(app.edits.geometry.guides[0], guide);
    app.undo();
    assert!(app.edits.geometry.guides.is_empty());
}
#[test]
fn guide_correction_and_auto_crop_arrive_as_one_transaction_and_stale_results_are_discarded() {
    let (mut app, ctx, _) = editor();
    app.load_generation = 4;
    let truth = crate::edits::GeometryEdits {
        pitch: 15.,
        ..Default::default()
    };
    let geometry = crate::geometry::Geometry::compile(&truth, 600, 400).unwrap();
    app.edits.geometry.guides = [0.25, 0.75]
        .map(|x| PerspectiveGuide {
            start: geometry.map([x, 0.1], 1).unwrap(),
            end: geometry.map([x, 0.9], 1).unwrap(),
            axis: GuideAxis::Vertical,
            enabled: true,
        })
        .to_vec();
    let before = app.edits.clone();
    let (send, receive) = mpsc::channel();
    let (reply_tx, reply_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        super::super::worker(receive, reply_tx, egui::Context::default(), Backend::Cpu)
    });
    send.send(Work::Perspective {
        load_id: 4,
        revision: 0,
        image: app.image.clone().unwrap(),
        edits: before.clone(),
        mode: crate::perspective::Mode::Vertical,
        provider: crate::perspective::MaskProvider::Classical,
        manual: true,
        apply: true,
        crop: Some(crate::crop::Constraint::Aspect(1.)),
    })
    .unwrap();
    let reply = reply_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    let (replies, rx) = mpsc::channel();
    app.rx = rx;
    app.busy = Some(Busy::Perspective(4));
    replies.send(reply).unwrap();
    app.poll(&ctx);
    assert_eq!(app.busy, None);
    assert_eq!(app.history.undo_len(), 1);
    assert!((app.edits.geometry.pitch - 15.).abs() < 0.001);
    let c = app.edits.geometry.crop;
    assert!((c[2] * 600. / (c[3] * 400.) - 1.).abs() < 1e-5);
    assert_eq!(app.edits.geometry.guides, before.geometry.guides);
    let after = app.edits.clone();
    app.undo();
    assert_eq!(app.edits, before);
    app.redo();
    assert_eq!(app.edits, after);
    app.busy = Some(Busy::AutoCrop(4));
    replies
        .send(Reply::AutoCropped {
            load_id: 4,
            revision: 0,
            previous: before,
            crop: [0., 0., 1., 1.],
            aspect: None,
        })
        .unwrap();
    app.poll(&ctx);
    assert_eq!(app.edits, after);
    assert_eq!(app.busy, None);
    assert!(app.status.contains("discarded"));
    drop(send);
    worker.join().unwrap();
}
