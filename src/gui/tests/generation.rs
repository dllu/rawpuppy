//! Exercise the asynchronous reply protocol without loading an inference model.
use super::*;
use crate::synthesis::{GeneratedFill, MaskDab};

fn fill(edits: &Edits, seed: i64) -> GeneratedFill {
    let hash = "0".repeat(64);
    GeneratedFill {
        region: [0., 0., 0.5, 0.5],
        dabs: vec![MaskDab {
            center: [0.2, 0.2],
            radius: 0.1,
        }],
        fill_gaps: false,
        feather: 0.,
        harmonization: Default::default(),
        steps: 20,
        seed,
        sampling: Default::default(),
        asset: format!("{hash}.exr"),
        sha256: hash.clone(),
        source_sha256: hash,
        source_color_revision: 0,
        recipe_sha256: crate::synthesis::recipe_hash(edits).unwrap(),
        model: "history-fixture".into(),
    }
}
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
fn reply(previous: Edits, revision: u64, replace: bool) -> Reply {
    Reply::Generated {
        load_id: 4,
        revision,
        recipe_hash: crate::synthesis::recipe_hash(&previous).unwrap(),
        fills: vec![fill(&previous, 2)],
        previous: Box::new(previous),
        replace,
    }
}

#[test]
fn ai_append_and_regeneration_are_single_undoable_steps_and_view_changes_are_allowed() {
    for replace in [false, true] {
        let (mut app, responses, ctx) = editor();
        app.edits.display.synthesis.push(fill(&app.edits, 1));
        let previous = app.edits.clone();
        let revision = app.history.revision();
        app.busy = Some(Busy::Generate(4));
        app.zoom = 2.;
        app.changed(); // Preview revision is not document history.
        responses
            .send(reply(previous.clone(), revision, replace))
            .unwrap();
        app.poll(&ctx);
        let generated = app.edits.clone();
        assert_ne!(generated, previous);
        assert_eq!(app.history.undo_len(), 1);
        assert_eq!(
            generated.display.synthesis.len(),
            if replace { 1 } else { 2 }
        );
        app.undo();
        assert_eq!(app.edits, previous);
        app.redo();
        assert_eq!(app.edits, generated);
    }
}

#[test]
fn an_ai_reply_after_undo_or_undo_redo_does_not_reapply_fills_or_destroy_redo() {
    for redo in [false, true] {
        let (mut app, responses, ctx) = editor();
        let original = app.edits.clone();
        app.edits.display.synthesis.push(fill(&app.edits, 1));
        app.remember(original.clone());
        let requested = app.edits.clone();
        let revision = app.history.revision();
        app.busy = Some(Busy::Generate(4));
        app.undo();
        if redo {
            app.redo();
        }
        assert_eq!(
            crate::synthesis::recipe_hash(&app.edits).unwrap(),
            crate::synthesis::recipe_hash(&requested).unwrap()
        );
        let before = app.edits.clone();
        let undo = app.history.undo_len();
        let can_redo = app.history.can_redo();
        responses.send(reply(requested, revision, true)).unwrap();
        app.poll(&ctx);
        assert_eq!(app.edits, before);
        assert_eq!(app.history.undo_len(), undo);
        assert_eq!(app.history.can_redo(), can_redo);
        assert_eq!(app.busy, None);
        assert!(app.status.contains("discarded"));
        if !redo {
            app.redo();
            assert_eq!(app.edits.display.synthesis.len(), 1);
        }
    }
}

#[test]
fn an_ai_reply_checks_existing_layers_even_when_the_base_recipe_hash_is_unchanged() {
    let (mut app, responses, ctx) = editor();
    let requested = app.edits.clone();
    let revision = app.history.revision();
    app.edits.display.synthesis.push(fill(&app.edits, 1)); // Full snapshot protects unrecorded changes too.
    let before = app.edits.clone();
    app.busy = Some(Busy::Generate(4));
    responses.send(reply(requested, revision, false)).unwrap();
    app.poll(&ctx);
    assert_eq!(app.edits, before);
    assert_eq!(app.history.undo_len(), 0);
}
