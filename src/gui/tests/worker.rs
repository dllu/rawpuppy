use super::*;
#[test]
fn malformed_raw_does_not_stop_the_editor_worker() {
    let directory = tempfile::tempdir().unwrap();
    let invalid = directory.path().join("invalid-cfa.dng");
    let mut encoder =
        tiff::encoder::TiffEncoder::new(std::fs::File::create(&invalid).unwrap()).unwrap();
    let mut output = encoder
        .new_image::<tiff::encoder::colortype::Gray16>(32, 32)
        .unwrap();
    let tags = output.encoder();
    tags.write_tag(tiff::tags::Tag::Make, "Rawpuppy").unwrap();
    tags.write_tag(tiff::tags::Tag::Model, "Invalid CFA Test")
        .unwrap();
    tags.write_tag(tiff::tags::Tag::PhotometricInterpretation, 32803u16)
        .unwrap();
    tags.write_tag(tiff::tags::Tag::Unknown(50706), &[1u8, 4, 0, 0][..])
        .unwrap();
    tags.write_tag(tiff::tags::Tag::Unknown(33421), &[2u16, 2][..])
        .unwrap();
    tags.write_tag(tiff::tags::Tag::Unknown(33422), &[0u8, 1, 1][..])
        .unwrap();
    tags.write_tag(tiff::tags::Tag::Unknown(50714), 512u32)
        .unwrap();
    tags.write_tag(tiff::tags::Tag::Unknown(50717), 15360u32)
        .unwrap();
    tags.write_tag(
        tiff::tags::Tag::Unknown(50721),
        &[1f32, 0., 0., 0., 1., 0., 0., 0., 1.][..],
    )
    .unwrap();
    // Three CFA entries contradict the declared 2×2 repeat dimensions and
    // currently trigger the dependency's unknown-CFA-size panic.
    tags.write_tag(
        tiff::tags::Tag::Unknown(50728),
        &[const { tiff::encoder::Rational { n: 1, d: 1 } }; 3][..],
    )
    .unwrap();
    output.write_data(&[4000u16; 32 * 32]).unwrap();
    let original_invalid = std::fs::read(&invalid).unwrap();
    let metadata_error = crate::input::color_revision(&invalid).unwrap_err();
    assert!(format!("{metadata_error:#}").contains("Unknown CFA size"));
    let valid = directory.path().join("valid.png");
    image::RgbImage::from_pixel(2, 2, image::Rgb([40, 100, 180]))
        .save(&valid)
        .unwrap();
    let original_valid = std::fs::read(&valid).unwrap();
    let (send, requests) = mpsc::channel();
    let (responses, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        super::worker(requests, responses, egui::Context::default(), Backend::Cpu)
    });
    send.send(Work::Open {
        id: 1,
        path: invalid.clone(),
    })
    .unwrap();
    send.send(Work::Open {
        id: 2,
        path: valid.clone(),
    })
    .unwrap();
    let reply = receive
        .recv_timeout(std::time::Duration::from_secs(3))
        .expect("Decoder failure disconnected the editor worker");
    let Reply::Error { scope, message } = reply else {
        panic!("Malformed RAW must report an error");
    };
    assert_eq!(scope, ErrorScope::Load(1));
    assert!(message.contains("Decoding"));
    let Reply::Opened {
        id,
        path,
        image,
        edits,
    } = receive
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
    else {
        panic!("Worker did not recover for the next photograph");
    };
    assert_eq!(id, 2);
    assert_eq!(path, valid);
    send.send(Work::Render {
        id: 3,
        image,
        edits,
        region: [0., 0., 1., 1.],
        size: [2, 2],
        profile: None,
        hdr: false,
    })
    .unwrap();
    let Reply::Preview {
        id, size, pixels, ..
    } = receive
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
    else {
        panic!("Recovered worker could not render the next photograph");
    };
    assert_eq!(id, 3);
    assert_eq!(size, [2, 2]);
    let PreviewPixels::Sdr(bytes) = pixels else {
        panic!("Expected SDR preview");
    };
    assert_eq!(bytes.len(), 16);
    assert!(bytes.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    drop(send);
    worker.join().unwrap();
    assert_eq!(std::fs::read(invalid).unwrap(), original_invalid);
    assert_eq!(std::fs::read(valid).unwrap(), original_valid);
}
#[test]
fn failed_operations_replace_progress_without_clearing_unrelated_work() {
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
    app.generation = 9;
    let cases = vec![
        (ErrorScope::Load(4), Some(Busy::Load(4)), "Opening photo…"),
        (ErrorScope::Export(4), Some(Busy::Export(4)), "Exporting…"),
        (ErrorScope::Save(4), None, "Saving edits…"),
        (ErrorScope::Preview(9), None, "Rendering preview…"),
        (
            ErrorScope::Perspective(4),
            Some(Busy::Perspective(4)),
            "Analyzing perspective…",
        ),
    ];
    #[cfg(feature = "moebius")]
    let cases = cases.into_iter().chain([(
        ErrorScope::Generate(4),
        Some(Busy::Generate(4)),
        "Generating local fill…",
    )]);
    for (scope, busy, progress) in cases {
        app.busy = busy;
        app.status = progress.into();
        responses
            .send(Reply::Error {
                scope,
                message: "Owned operation failed".into(),
            })
            .unwrap();
        app.poll(&ctx);
        assert_eq!(app.busy, None);
        assert_ne!(
            app.status, progress,
            "Completed failure still reports progress"
        );
        assert_eq!(app.error.as_deref(), Some("Owned operation failed"));
    }
    app.load_generation = 5;
    for scope in [ErrorScope::Save(4), ErrorScope::Export(4)] {
        app.busy = Some(Busy::Load(5));
        app.status = "Opening newer photo…".into();
        responses
            .send(Reply::Error {
                scope,
                message: "Older destination failed".into(),
            })
            .unwrap();
        app.poll(&ctx);
        assert_eq!(app.busy, Some(Busy::Load(5)));
        assert_eq!(app.status, "Opening newer photo…");
        assert_eq!(app.error.as_deref(), Some("Older destination failed"));
    }
}
#[test]
fn automatic_perspective_is_one_undo_step_and_stale_results_do_not_change_edits() {
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
    let report = || {
        let blank = crate::pipeline::Rendered {
            width: 32,
            height: 32,
            pixels: vec![[0.2, 0.2, 0.2, 1.]; 32 * 32],
        };
        let mut report = crate::perspective::analyze(
            &blank,
            None,
            &crate::edits::GeometryEdits::default(),
            crate::perspective::Mode::Vertical,
        )
        .unwrap();
        report.solution = Some(crate::edits::GeometryEdits {
            pitch: -15.,
            shift: [0., 0.2],
            ..Default::default()
        });
        Box::new(report)
    };
    let original = app.edits.clone();
    app.busy = Some(Busy::Perspective(4));
    responses
        .send(Reply::Perspective {
            load_id: 4,
            revision: 0,
            previous: original.clone(),
            report: report(),
            apply: true,
        })
        .unwrap();
    app.poll(&ctx);
    let corrected = app.edits.clone();
    assert_ne!(corrected, original);
    assert_eq!(app.history.undo_len(), 1);
    assert_eq!(app.busy, None);
    app.undo();
    assert_eq!(app.edits, original);
    app.redo();
    assert_eq!(app.edits, corrected);
    // A result for an earlier photo cannot finish the newer photo's work.
    app.busy = Some(Busy::Perspective(4));
    responses
        .send(Reply::Perspective {
            load_id: 3,
            revision: 0,
            previous: corrected.clone(),
            report: report(),
            apply: true,
        })
        .unwrap();
    app.poll(&ctx);
    assert_eq!(app.edits, corrected);
    assert_eq!(app.busy, Some(Busy::Perspective(4)));
    app.edits.geometry.rotation = 3.;
    let changed = app.edits.clone();
    let history = app.history.undo_len();
    responses
        .send(Reply::Perspective {
            load_id: 4,
            revision: 0,
            previous: corrected,
            report: report(),
            apply: true,
        })
        .unwrap();
    app.poll(&ctx);
    assert_eq!(app.edits, changed);
    assert_eq!(app.history.undo_len(), history);
    assert_eq!(app.busy, None);
}

#[test]
fn cancelling_save_continuation_keeps_a_later_open_request_pending() {
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
    app.tx = send;
    app.rx = receive;
    app.image = Some(Arc::new(
        SensorImage::from_rgb(2, 2, vec![0.2; 12]).unwrap(),
    ));
    app.path = Some(PathBuf::from("owned-photo.png"));
    app.edits.scene.exposure = 0.5;
    app.pending = Some(Pending::Close);
    app.close_after_save = true;
    app.save();
    let Work::Save {
        load_id,
        path,
        edits,
    } = requests.try_recv().unwrap()
    else {
        panic!("Expected saved snapshot");
    };
    app.cancel_pending();
    let next = PathBuf::from("next-owned-photo.png");
    app.request_open(next.clone());
    responses
        .send(Reply::Saved {
            load_id,
            path,
            edits,
        })
        .unwrap();
    app.poll(&ctx);
    assert!(matches!(&app.pending,Some(Pending::Open(path)) if path==&next));
    assert!(
        requests.try_recv().is_err(),
        "Delayed save implicitly approved a new open request"
    );
    // A fresh explicit Save decision must still continue once its matching
    // current snapshot is durable.
    app.edits.scene.exposure = 0.7;
    app.close_after_save = true;
    app.save();
    let Work::Save {
        load_id,
        path,
        edits,
    } = requests.try_recv().unwrap()
    else {
        panic!("Expected current saved snapshot");
    };
    responses
        .send(Reply::Saved {
            load_id,
            path,
            edits,
        })
        .unwrap();
    app.poll(&ctx);
    assert!(matches!(requests.try_recv().unwrap(),Work::Open{path,..} if path==next));
    assert!(app.pending.is_none() && !app.close_after_save);
}
#[cfg(feature = "moebius")]
#[test]
fn regeneration_removes_obsolete_corner_fills_without_loading_assets_or_a_model() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("photo.png");
    image::RgbImage::from_pixel(2, 2, image::Rgb([40, 50, 60]))
        .save(&original)
        .unwrap();
    let bytes = std::fs::read(&original).unwrap();
    let source = Arc::new(SensorImage::open(&original).unwrap());
    let mut edits = Edits::for_image(&source);
    let hash = "0".repeat(64);
    edits
        .display
        .synthesis
        .push(crate::synthesis::GeneratedFill {
            region: [0., 0., 0.25, 0.25],
            dabs: vec![],
            fill_gaps: true,
            feather: 0.,
            harmonization: Default::default(),
            steps: 20,
            seed: 42,
            asset: format!("{hash}.exr"),
            sha256: hash.clone(),
            source_sha256: hash.clone(),
            source_color_revision: 0,
            recipe_sha256: hash,
            sampling: Default::default(),
            model: "moebius-scene-2026-v1".into(),
        });
    let expected_hash = crate::synthesis::recipe_hash(&edits).unwrap();
    let (send, requests) = mpsc::channel();
    let (responses, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        super::worker(requests, responses, egui::Context::default(), Backend::Cpu)
    });
    // The current unrotated canvas has no gaps. Its obsolete corner asset
    // deliberately does not exist, so replanning must use the base photo.
    send.send(Work::Generate {
        load_id: 1,
        revision: 0,
        path: original.clone(),
        image: source,
        edits,
        dabs: vec![],
        gaps: false,
        steps: 20,
        seed: 42,
        regenerate: true,
    })
    .unwrap();
    let reply = receive
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    drop(send);
    worker.join().unwrap();
    let Reply::Generated {
        load_id,
        recipe_hash,
        fills,
        replace,
        ..
    } = reply
    else {
        panic!("Gap-free regeneration must remove obsolete fills");
    };
    assert_eq!(load_id, 1);
    assert_eq!(recipe_hash, expected_hash);
    assert!(replace && fills.is_empty());
    assert!(!crate::synthesis::asset_directory(&original).exists());
    assert!(!sidecar::path_for(&original).exists());
    assert_eq!(std::fs::read(original).unwrap(), bytes);
}
#[cfg(feature = "moebius")]
#[test]
fn invalid_generation_finishes_only_its_own_activity() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("photo.png");
    image::RgbImage::from_pixel(2, 2, image::Rgb([40, 50, 60]))
        .save(&original)
        .unwrap();
    let source = Arc::new(SensorImage::open(&original).unwrap());
    let bytes = std::fs::read(&original).unwrap();
    let (send, requests) = mpsc::channel();
    let (responses, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        super::worker(requests, responses, egui::Context::default(), Backend::Cpu)
    });
    // An empty painted selection is rejected before any model is loaded.
    send.send(Work::Generate {
        load_id: 1,
        revision: 0,
        path: original.clone(),
        image: source,
        edits: Edits::default(),
        dabs: vec![],
        gaps: false,
        steps: 20,
        seed: 42,
        regenerate: false,
    })
    .unwrap();
    let Reply::Error { scope, .. } = receive
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
    else {
        panic!("Empty selection must fail before inference");
    };
    assert_eq!(scope, ErrorScope::Generate(1));
    assert!(!scope.applies(2, 3));
    let mut exporting = Some(Busy::Export(1));
    scope.finish_busy(&mut exporting);
    assert_eq!(exporting, Some(Busy::Export(1)));
    let mut generating = Some(Busy::Generate(1));
    scope.finish_busy(&mut generating);
    assert_eq!(generating, None);
    assert_eq!(std::fs::read(&original).unwrap(), bytes);
    drop(send);
    worker.join().unwrap();
}
#[test]
fn durable_failures_preserve_newer_activity_and_the_worker_recovers() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("photo.png");
    image::RgbImage::from_pixel(2, 2, image::Rgb([40, 50, 60]))
        .save(&original)
        .unwrap();
    let original_bytes = std::fs::read(&original).unwrap();
    let image = Arc::new(SensorImage::open(&original).unwrap());
    let failed_save = directory.path().join("blocked.xmp");
    let failed_export = directory.path().join("blocked.png");
    std::fs::create_dir(&failed_save).unwrap();
    std::fs::create_dir(&failed_export).unwrap();
    let (send, requests) = mpsc::channel();
    let (responses, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        super::worker(requests, responses, egui::Context::default(), Backend::Cpu)
    });
    let mut edits = Edits::default();
    edits.tone.mapper = ToneMapper::Linear;
    send.send(Work::Save {
        load_id: 1,
        path: failed_save.clone(),
        edits: edits.clone(),
    })
    .unwrap();
    send.send(Work::Export {
        load_id: 1,
        original: original.clone(),
        path: failed_export.clone(),
        image: image.clone(),
        edits: edits.clone(),
        space: OutputSpace::Srgb,
    })
    .unwrap();
    send.send(Work::Open {
        id: 2,
        path: original.clone(),
    })
    .unwrap();
    for (expected, destination) in [
        (ErrorScope::Save(1), &failed_save),
        (ErrorScope::Export(1), &failed_export),
    ] {
        let Reply::Error { scope, message } = receive
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap()
        else {
            panic!("Blocked durable destination should fail");
        };
        assert_eq!(scope, expected);
        assert!(
            scope.applies(2, 3),
            "Durable failure must still be reported"
        );
        assert!(
            message.contains(destination.to_str().unwrap()),
            "Failure must identify its destination: {message}"
        );
        let mut busy = Some(Busy::Load(2));
        scope.finish_busy(&mut busy);
        assert_eq!(
            busy,
            Some(Busy::Load(2)),
            "Old I/O cleared the new document's activity"
        );
        if scope == ErrorScope::Save(1) {
            let mut exporting = Some(Busy::Export(1));
            scope.finish_busy(&mut exporting);
            assert_eq!(
                exporting,
                Some(Busy::Export(1)),
                "Saving never owned the export's busy state"
            );
        }
    }
    let Reply::Opened { id, .. } = receive
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
    else {
        panic!("Worker did not open the next document");
    };
    assert_eq!(id, 2);
    let mut busy = Some(Busy::Export(2));
    ErrorScope::Preview(3).finish_busy(&mut busy);
    assert_eq!(
        busy,
        Some(Busy::Export(2)),
        "Preview failure cannot finish an export"
    );
    let saved = sidecar::path_for(&original);
    let exported = directory.path().join("result.png");
    send.send(Work::Save {
        load_id: 2,
        path: saved.clone(),
        edits: edits.clone(),
    })
    .unwrap();
    send.send(Work::Export {
        load_id: 2,
        original: original.clone(),
        path: exported.clone(),
        image,
        edits: edits.clone(),
        space: OutputSpace::Srgb,
    })
    .unwrap();
    let Reply::Saved {
        load_id,
        path,
        edits: snapshot,
    } = receive
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
    else {
        panic!("Worker did not save the current document");
    };
    assert_eq!(load_id, 2);
    assert_eq!(path, saved);
    assert_eq!(snapshot, edits);
    assert_eq!(sidecar::load(&saved).unwrap(), edits);
    let Reply::Exported { load_id, path } = receive
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
    else {
        panic!("Worker did not export the current document");
    };
    assert_eq!(load_id, 2);
    assert_eq!(path, exported);
    ErrorScope::Export(load_id).finish_busy(&mut busy);
    assert_eq!(busy, None);
    assert_eq!(image::image_dimensions(exported).unwrap(), (2, 2));
    assert_eq!(std::fs::read(&original).unwrap(), original_bytes);
    drop(send);
    worker.join().unwrap();
}
#[test]
fn failed_old_preview_is_scoped_and_the_worker_still_renders_the_new_request() {
    let (send, requests) = mpsc::channel();
    let (responses, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        super::worker(requests, responses, egui::Context::default(), Backend::Cpu)
    });
    let image = Arc::new(SensorImage::from_rgb(2, 2, vec![0.2; 12]).unwrap());
    let invalid = Edits {
        version: 999,
        ..Edits::default()
    };
    send.send(Work::Render {
        id: 3,
        image: image.clone(),
        edits: invalid,
        region: [0., 0., 1., 1.],
        size: [2, 2],
        profile: None,
        hdr: false,
    })
    .unwrap();
    let Reply::Error { scope, .. } = receive
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
    else {
        panic!("Invalid recipe should produce a scoped failure");
    };
    assert!(
        !scope.applies(4, 5),
        "Superseded preview failure would alter the new request"
    );
    let mut valid = Edits::default();
    valid.tone.mapper = ToneMapper::Linear;
    send.send(Work::Render {
        id: 5,
        image,
        edits: valid,
        region: [0., 0., 1., 1.],
        size: [2, 2],
        profile: None,
        hdr: false,
    })
    .unwrap();
    let Reply::Preview {
        id,
        pixels: PreviewPixels::Sdr(rgba),
        ..
    } = receive
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
    else {
        panic!("Worker did not recover for the current preview");
    };
    assert_eq!(id, 5);
    assert_eq!(rgba.len(), 16);
    assert!(rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    drop(send);
    worker.join().unwrap();
}
#[test]
fn failed_old_load_is_identified_before_a_new_document_completes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("photo.png");
    image::RgbImage::from_pixel(2, 2, image::Rgb([40, 50, 60]))
        .save(&path)
        .unwrap();
    let (send, requests) = mpsc::channel();
    let (responses, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        super::worker(requests, responses, egui::Context::default(), Backend::Cpu)
    });
    send.send(Work::Open {
        id: 1,
        path: directory.path().join("missing.raf"),
    })
    .unwrap();
    let Reply::Error { scope, .. } = receive
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
    else {
        panic!("Missing original should fail")
    };
    assert!(!scope.applies(2, 3));
    send.send(Work::Open {
        id: 2,
        path: path.clone(),
    })
    .unwrap();
    let Reply::Opened {
        id, path: opened, ..
    } = receive
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
    else {
        panic!("Next document did not open")
    };
    assert_eq!(id, 2);
    assert_eq!(opened, path);
    drop(send);
    worker.join().unwrap();
}
