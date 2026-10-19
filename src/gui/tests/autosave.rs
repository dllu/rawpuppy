use super::*;
fn editor() -> (
    Editor,
    egui::Context,
    mpsc::Receiver<Work>,
    mpsc::Sender<Reply>,
) {
    let ctx = egui::Context::default();
    let mut app = Editor::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        None,
        None,
        Backend::Cpu,
        false,
    );
    let (send, requests) = mpsc::channel();
    let (responses, receive) = mpsc::channel();
    app.save_tx = send;
    app.rx = receive;
    app.image = Some(Arc::new(
        SensorImage::from_rgb(4, 3, vec![0.2; 36]).unwrap(),
    ));
    app.path = Some(PathBuf::from("owned-test.raf"));
    (app, ctx, requests, responses)
}
fn tick(app: &mut Editor, ctx: &egui::Context, time: f64) {
    let mut out = ctx.run_ui(
        egui::RawInput {
            time: Some(time),
            ..Default::default()
        },
        |ui| app.autosave_tick(ui.ctx()),
    );
    out.textures_delta.clear();
}
fn saved(reply: &mpsc::Sender<Reply>, work: Work) -> Edits {
    let Work::Save {
        load_id,
        path,
        edits,
    } = work
    else {
        panic!("Expected sidecar snapshot");
    };
    reply
        .send(Reply::Saved {
            load_id,
            path,
            edits: edits.clone(),
        })
        .unwrap();
    edits
}
#[test]
fn autosave_debounces_coalesces_and_saves_undo_without_blocking_rendering() {
    let (mut app, ctx, requests, responses) = editor();
    tick(&mut app, &ctx, 0.);
    assert!(requests.try_recv().is_err());
    let original = app.edits.clone();
    app.edits.scene.exposure = 0.5;
    app.remember(original.clone());
    tick(&mut app, &ctx, 0.1);
    let half = app.edits.clone();
    app.edits.scene.exposure = 1.;
    app.remember(half);
    tick(&mut app, &ctx, 0.3);
    tick(&mut app, &ctx, 0.6);
    assert!(requests.try_recv().is_err());
    app.busy = Some(Busy::Export(0));
    tick(&mut app, &ctx, 0.8);
    let first = requests.try_recv().unwrap();
    app.edits.scene.exposure = 2.;
    app.remember({
        let mut old = app.edits.clone();
        old.scene.exposure = 1.;
        old
    });
    tick(&mut app, &ctx, 0.9);
    tick(&mut app, &ctx, 1.5);
    assert!(requests.try_recv().is_err());
    assert_eq!(saved(&responses, first).scene.exposure, 1.);
    app.poll(&ctx);
    assert!(app.dirty());
    assert_eq!(app.busy, Some(Busy::Export(0)));
    tick(&mut app, &ctx, 1.6);
    assert_eq!(
        saved(&responses, requests.try_recv().unwrap())
            .scene
            .exposure,
        2.
    );
    app.poll(&ctx);
    assert!(!app.dirty());
    app.undo();
    tick(&mut app, &ctx, 2.);
    tick(&mut app, &ctx, 2.5);
    assert_eq!(
        saved(&responses, requests.try_recv().unwrap())
            .scene
            .exposure,
        1.
    );
    app.poll(&ctx);
    assert!(!app.dirty());
    app.changed();
    tick(&mut app, &ctx, 3.);
    tick(&mut app, &ctx, 4.);
    assert!(requests.try_recv().is_err());
}
#[test]
fn a_failed_autosave_stays_dirty_and_manual_retry_can_recover() {
    let (mut app, ctx, requests, responses) = editor();
    app.edits.scene.exposure = 1.;
    tick(&mut app, &ctx, 0.);
    tick(&mut app, &ctx, 1.);
    let Work::Save { load_id, .. } = requests.try_recv().unwrap() else {
        panic!();
    };
    responses
        .send(Reply::Error {
            scope: ErrorScope::Save(load_id),
            message: "Disk full".into(),
        })
        .unwrap();
    app.poll(&ctx);
    assert!(app.dirty() && app.autosave.failed);
    tick(&mut app, &ctx, 2.);
    assert!(requests.try_recv().is_err());
    app.save();
    saved(&responses, requests.try_recv().unwrap());
    app.poll(&ctx);
    assert!(!app.dirty());
}
#[test]
fn serial_writer_is_atomic_and_keeps_original_and_darktable_sidecar_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("photo.raf");
    std::fs::write(&original, b"immutable original").unwrap();
    let darktable = original.with_extension("raf.xmp");
    std::fs::write(&darktable, b"darktable metadata").unwrap();
    let path = sidecar::path_for(&original);
    let (send, receive) = mpsc::channel();
    let (reply, results) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        crate::gui::autosave::save_worker(receive, reply, egui::Context::default())
    });
    for exposure in [0.5, 1., -0.25] {
        let mut edits = Edits::default();
        edits.scene.exposure = exposure;
        send.send(Work::Save {
            load_id: 1,
            path: path.clone(),
            edits,
        })
        .unwrap();
    }
    drop(send);
    writer.join().unwrap();
    assert_eq!(
        results
            .try_iter()
            .filter(|r| matches!(r, Reply::Saved { .. }))
            .count(),
        3
    );
    assert_eq!(sidecar::load(&path).unwrap().scene.exposure, -0.25);
    assert_eq!(std::fs::read(original).unwrap(), b"immutable original");
    assert_eq!(std::fs::read(darktable).unwrap(), b"darktable metadata");
}
#[test]
fn selectable_buttons_do_not_resize_or_move_neighbours_on_hover_or_selection() {
    let (app, ctx, _, _) = editor();
    drop(app);
    let mut baseline = None;
    for n in 0..5 {
        let events = if n == 0 {
            vec![]
        } else {
            vec![egui::Event::PointerMoved(Pos2::new(35., 20.))]
        };
        let mut rects = None;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700., 300.))),
                time: Some(n as f64),
                events,
                ..Default::default()
            },
            |ui| {
                ui.horizontal(|ui| {
                    let first = ui.selectable_label(n >= 3, "Perspective");
                    let second = ui.button("Neighbour");
                    rects = Some([first.rect, second.rect]);
                });
            },
        );
        out.textures_delta.clear();
        let rects = rects.unwrap();
        if let Some(baseline) = baseline {
            assert_eq!(rects, baseline);
        } else {
            baseline = Some(rects);
        }
    }
}
