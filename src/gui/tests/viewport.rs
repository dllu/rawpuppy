//! Check the painted image while asynchronous viewport renders are still pending.
use super::*;

fn frame(
    app: &mut Editor,
    ctx: &egui::Context,
    time: f64,
    events: Vec<egui::Event>,
) -> egui::epaint::Mesh {
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
    let texture = app.texture.as_ref().unwrap().id();
    output
        .shapes
        .into_iter()
        .find_map(|shape| match shape.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some((*mesh).clone()),
            _ => None,
        })
        .expect("Photograph was not painted")
}

fn scale(mesh: &egui::epaint::Mesh) -> Vec2 {
    let first = mesh.vertices[0];
    let last = mesh.vertices[3];
    (last.pos - first.pos) / (last.uv - first.uv)
}

#[test]
fn scrolling_scales_pending_previews_uniformly_in_the_same_frame() {
    for (width, height, crop) in [
        (320, 480, [0., 0., 1., 1.]),
        (480, 320, [0., 0., 1., 1.]),
        (480, 320, [0.1, 0.1, 0.5, 0.8]),
    ] {
        let ctx = egui::Context::default();
        let mut app = Editor::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            None,
            None,
            Backend::Cpu,
            false,
        );
        // Capture requests, withholding replies to reproduce a slow RAW render.
        let (requests, receive) = mpsc::channel();
        app.tx = requests;
        app.image = Some(Arc::new(
            SensorImage::from_rgb(width, height, vec![0.2; width * height * 3]).unwrap(),
        ));
        app.edits.geometry.crop = crop;
        app.texture = Some(ctx.load_texture(
            "pending-preview",
            egui::ColorImage::filled([48, 32], Color32::WHITE),
            egui::TextureOptions::LINEAR,
        ));
        let initial = scale(&frame(&mut app, &ctx, 0., vec![]));
        let pointer = app.probe_photo_rect.unwrap().center();
        let zoomed = frame(
            &mut app,
            &ctx,
            0.1,
            vec![
                egui::Event::PointerMoved(pointer),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0., 150.),
                    modifiers: egui::Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                },
            ],
        );
        assert!(app.zoom > 1.);
        let ratio = scale(&zoomed) / initial;
        assert!(
            (ratio.x - app.zoom).abs() < 1e-4 && (ratio.y - app.zoom).abs() < 1e-4,
            "Pending preview stretched: scale={ratio:?}, zoom={}",
            app.zoom
        );
        let rect = app.probe_photo_rect.unwrap();
        let region = app.viewport;
        let source_scale = Vec2::new(
            rect.width() / (region[2] * width as f32 * crop[2]),
            rect.height() / (region[3] * height as f32 * crop[3]),
        );
        assert!((source_scale.x - source_scale.y).abs() < 1e-4);
        assert!((source_scale.x - app.fit_scale * app.zoom).abs() < 1e-4);
        let (id, requested_region) = receive
            .try_iter()
            .filter_map(|request| match request {
                Work::Render { id, region, .. } => Some((id, region)),
                _ => None,
            })
            .last()
            .unwrap();
        assert_eq!(id, app.generation);
        assert_eq!(requested_region, region);
        // Continue scrolling until both canvas edges clip the photo.
        for i in 2..=6 {
            let mesh = frame(
                &mut app,
                &ctx,
                i as f64 * 0.1,
                vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0., 150.),
                    modifiers: egui::Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                }],
            );
            let ratio = scale(&mesh) / initial;
            assert!((ratio.x - app.zoom).abs() < 1e-4);
            assert!((ratio.y - app.zoom).abs() < 1e-4);
        }
    }
}

#[test]
fn zooming_out_keeps_a_rendered_subregion_at_its_image_position() {
    let ctx = egui::Context::default();
    let mut app = Editor::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        None,
        None,
        Backend::Cpu,
        false,
    );
    let (requests, _receive) = mpsc::channel();
    app.tx = requests;
    let (replies, receive) = mpsc::channel();
    app.rx = receive;
    app.image = Some(Arc::new(
        SensorImage::from_rgb(480, 320, vec![0.2; 480 * 320 * 3]).unwrap(),
    ));
    let rendered_region = [0.25, 0.25, 0.5, 0.5];
    replies
        .send(Reply::Preview {
            id: app.generation,
            region: rendered_region,
            size: [16, 16],
            pixels: PreviewPixels::Sdr(vec![255; 16 * 16 * 4]),
            histogram: vec![],
            elapsed: 0.,
            backend: "fixture".into(),
            managed_display: true,
            reconstruction_progress: None,
        })
        .unwrap();
    app.poll(&ctx);
    assert_eq!(app.texture_region, rendered_region);
    app.zoom = 3.;
    frame(&mut app, &ctx, 0., vec![]);
    app.zoom = 1.;
    let mesh = frame(&mut app, &ctx, 0.1, vec![]);
    let rect = app.probe_photo_rect.unwrap();
    // The existing center-quarter preview occupies only the center quarter;
    // it must not expand to fill the full photograph while its replacement renders.
    let first = mesh.vertices[0];
    let last = mesh.vertices[3];
    assert_eq!(first.uv, Pos2::ZERO);
    assert_eq!(last.uv, Pos2::new(1., 1.));
    assert!((first.pos - (rect.min + rect.size() * 0.25)).length() < 1e-4);
    assert!((last.pos - (rect.min + rect.size() * 0.75)).length() < 1e-4);
    assert!((scale(&mesh).x / 240. - scale(&mesh).y / 160.).abs() < 1e-4);
}
