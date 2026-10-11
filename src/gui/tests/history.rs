use super::*;

#[test]
fn unfinished_parameter_gestures_are_one_undo_step_and_cannot_reappear_on_release() {
    let ctx = egui::Context::default();
    let mut app = Editor::new(
        &eframe::CreationContext::_new_kittest(ctx),
        None,
        None,
        Backend::Cpu,
        false,
    );
    let initial = app.edits.clone();
    app.history.begin_gesture(initial.clone());
    app.edits.scene.exposure = 1.;
    app.changed();
    app.history.begin_gesture(app.edits.clone());
    app.edits.scene.exposure = 2.;
    app.changed();
    let finished = app.edits.clone();
    app.undo();
    assert_eq!(app.edits, initial);
    assert!(app.history.can_redo());
    app.finish_edit_gesture();
    assert_eq!(app.edits, initial);
    assert!(app.history.can_redo());
    app.redo();
    assert_eq!(app.edits, finished);
    assert_eq!(app.history.undo_len(), 1);
}

#[test]
fn document_history_is_bounded_for_parameter_changes_and_brush_strokes() {
    let mut history = History::default();
    let mut edits = Edits::default();
    for i in 0..250 {
        let previous = edits.clone();
        edits.scene.exposure = i as f32;
        if i % 2 == 0 {
            history.record(previous, &edits);
        } else {
            history.begin_stroke(previous);
        }
    }
    assert_eq!(history.undo_len(), 200);
    for _ in 0..200 {
        assert!(history.undo(&mut edits));
    }
    assert!(!history.undo(&mut edits));
    assert_eq!(edits.scene.exposure, 49.);
}
