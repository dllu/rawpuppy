//! Native single-photo editor. I/O and photo rendering never run on the UI thread.
use crate::{
    color::{self, OutputSpace},
    edits::{Edits, Retouch, RetouchMode, ToneMapper},
    export,
    input::SensorImage,
    render::{Backend, Renderer},
    sidecar,
};
use anyhow::{Result, ensure};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    time::Instant,
};

const ACCENT: Color32 = Color32::from_rgb(225, 150, 100);

enum Work {
    Open {
        id: u64,
        path: PathBuf,
    },
    Render {
        id: u64,
        image: Arc<SensorImage>,
        edits: Edits,
        region: [f32; 4],
        size: [usize; 2],
        profile: Option<PathBuf>,
    },
    Save {
        path: PathBuf,
        edits: Edits,
    },
    #[cfg(feature = "moebius")]
    Generate {
        load_id: u64,
        path: PathBuf,
        image: Arc<SensorImage>,
        edits: Edits,
        dabs: Vec<crate::synthesis::MaskDab>,
        gaps: bool,
        steps: usize,
        seed: i64,
        regenerate: bool,
    },
    Export {
        original: PathBuf,
        path: PathBuf,
        image: Arc<SensorImage>,
        edits: Edits,
        space: OutputSpace,
    },
}
enum Reply {
    Opened {
        id: u64,
        path: PathBuf,
        image: Arc<SensorImage>,
        edits: Edits,
    },
    Preview {
        id: u64,
        size: [usize; 2],
        rgba: Vec<u8>,
        histogram: Vec<f32>,
        elapsed: f64,
        backend: String,
    },
    Saved {
        path: PathBuf,
        edits: Edits,
    },
    #[cfg(feature = "moebius")]
    Generated {
        load_id: u64,
        recipe_hash: String,
        fills: Vec<crate::synthesis::GeneratedFill>,
        replace: bool,
    },
    Exported(PathBuf),
    Error(String),
}

fn worker(rx: mpsc::Receiver<Work>, tx: mpsc::Sender<Reply>, ctx: egui::Context, backend: Backend) {
    let mut renderer = Renderer::new(backend);
    while let Ok(mut work) = rx.recv() {
        // Coalesce only preview requests; durable save/export operations are always executed.
        while matches!(work, Work::Render { .. }) {
            match rx.try_recv() {
                Ok(next) => work = next,
                Err(_) => break,
            }
        }
        let result: Result<Reply> = (|| match work {
            Work::Open { id, path } => {
                let image = Arc::new(SensorImage::open(&path)?);
                let edits = sidecar::load_for(&path)?;
                renderer.set_document(path.clone());
                Ok(Reply::Opened {
                    id,
                    path,
                    image,
                    edits,
                })
            }
            Work::Render {
                id,
                image,
                edits,
                region,
                size,
                profile,
            } => {
                let start = Instant::now();
                let mut preview_edits = edits.clone();
                let hash = crate::synthesis::recipe_hash(&edits)?;
                preview_edits
                    .display
                    .synthesis
                    .retain(|fill| fill.recipe_sha256 == hash);
                let rendered =
                    renderer.render_region(image, &preview_edits, region, size[0], size[1])?;
                let mut histogram = vec![0f32; 128];
                for p in &rendered.pixels {
                    if p[3] < 0.5 {
                        continue;
                    }
                    let l = color::srgb_encode(color::luminance([p[0], p[1], p[2]]));
                    histogram[(l.clamp(0., 1.) * 127.) as usize] += 1.;
                }
                let peak = histogram.iter().copied().fold(1., f32::max);
                for v in &mut histogram {
                    *v = (*v / peak).sqrt();
                }
                let rgba = export::display_rgba8(&rendered, profile.as_deref())?;
                Ok(Reply::Preview {
                    id,
                    size,
                    rgba,
                    histogram,
                    elapsed: start.elapsed().as_secs_f64(),
                    backend: renderer.label().into(),
                })
            }
            Work::Save { path, edits } => {
                sidecar::save(&path, &edits)?;
                Ok(Reply::Saved { path, edits })
            }
            #[cfg(feature = "moebius")]
            Work::Generate {
                load_id,
                path,
                image,
                edits,
                dabs,
                gaps,
                steps,
                seed,
                regenerate,
            } => {
                renderer.set_document(path);
                let hash = crate::synthesis::recipe_hash(&edits)?;
                let (w, h) = crate::pipeline::Pipeline::compile(&image, &edits)?.dimensions(None);
                let mut jobs = Vec::new();
                if regenerate {
                    for fill in &edits.display.synthesis {
                        jobs.push((fill.region, fill.dabs.clone(), fill.fill_gaps, steps, seed));
                    }
                } else if gaps {
                    let mut base = edits.clone();
                    base.display.synthesis.clear();
                    let probe =
                        renderer.render_region(image.clone(), &base, [0., 0., 1., 1.], 128, 128)?;
                    for region in crate::synthesis::gap_contexts(&probe, w, h) {
                        jobs.push((region, vec![], true, steps, seed));
                    }
                } else {
                    jobs.push((
                        crate::synthesis::brush_context(&dabs, w, h)?,
                        dabs,
                        false,
                        steps,
                        seed,
                    ));
                }
                ensure!(!jobs.is_empty(), "No geometric gaps to fill");
                let mut fills = Vec::new();
                let mut current = edits.clone();
                if regenerate {
                    current.display.synthesis.clear();
                }
                for (region, dabs, gaps, steps, seed) in jobs {
                    let settings = crate::moebius::Sampling {
                        steps,
                        seed,
                        ..Default::default()
                    };
                    let fill = renderer.generate_fill(
                        image.clone(),
                        &current,
                        region,
                        dabs,
                        gaps,
                        &settings,
                    )?;
                    current.display.synthesis.push(fill.clone());
                    fills.push(fill);
                    ctx.request_repaint();
                }
                Ok(Reply::Generated {
                    load_id,
                    recipe_hash: hash,
                    fills,
                    replace: regenerate,
                })
            }
            Work::Export {
                original,
                path,
                image,
                edits,
                space,
            } => {
                ensure!(
                    path.canonicalize().ok().as_ref() != Some(&original.canonicalize()?),
                    "Export cannot overwrite the original"
                );
                let rendered = renderer.render(image, &edits, None)?;
                export::write(&path, &rendered, space, true)?;
                Ok(Reply::Exported(path))
            }
        })();
        let reply = result.unwrap_or_else(|e| Reply::Error(format!("{e:#}")));
        if tx.send(reply).is_err() {
            break;
        }
        ctx.request_repaint();
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Tool {
    View,
    Clone,
    Heal,
    Mask,
}
enum Pending {
    Close,
    Open(PathBuf),
}

pub fn run(input: Option<PathBuf>, profile: Option<PathBuf>, backend: Backend) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440., 960.])
            .with_min_inner_size([800., 550.]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "Rawpuppy",
        options,
        Box::new(move |cc| Ok(Box::new(Editor::new(cc, input, profile, backend)))),
    )
    .map_err(|e| anyhow::anyhow!("Opening native editor: {e}"))
}

struct Editor {
    tx: mpsc::Sender<Work>,
    rx: mpsc::Receiver<Reply>,
    image: Option<Arc<SensorImage>>,
    path: Option<PathBuf>,
    edits: Edits,
    saved: Edits,
    undo: Vec<Edits>,
    redo: Vec<Edits>,
    texture: Option<egui::TextureHandle>,
    histogram: Vec<f32>,
    generation: u64,
    load_generation: u64,
    preview_pending: bool,
    busy: bool,
    status: String,
    error: Option<String>,
    profile: Option<PathBuf>,
    space: OutputSpace,
    zoom: f32,
    center: [f32; 2],
    viewport: [f32; 4],
    preview_size: [usize; 2],
    compare: bool,
    grid: bool,
    tool: Tool,
    clone_source: Option<[f32; 2]>,
    radius: f32,
    feather: f32,
    pending: Option<Pending>,
    close_after_save: bool,
    stroke_recorded: bool,
    brush_offset: [f32; 2],
    edit_gesture: Option<Edits>,
    fit_scale: f32,
    mask: Vec<crate::synthesis::MaskDab>,
    ai_steps: usize,
    ai_seed: i64,
}

impl Editor {
    fn new(
        cc: &eframe::CreationContext<'_>,
        input: Option<PathBuf>,
        profile: Option<PathBuf>,
        backend: Backend,
    ) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(29, 31, 34);
        visuals.window_fill = Color32::from_rgb(35, 37, 40);
        visuals.extreme_bg_color = Color32::from_rgb(20, 22, 24);
        visuals.selection.bg_fill = ACCENT.gamma_multiply(0.4);
        visuals.widgets.active.bg_fill = ACCENT.gamma_multiply(0.6);
        cc.egui_ctx.set_visuals(visuals);
        let mut style = (*cc.egui_ctx.style_of(egui::Theme::Dark)).clone();
        style.spacing.item_spacing = Vec2::new(10., 9.);
        style.spacing.button_padding = Vec2::new(13., 7.);
        cc.egui_ctx.set_style_of(egui::Theme::Dark, style);
        let (tx, work_rx) = mpsc::channel();
        let (reply_tx, rx) = mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        std::thread::Builder::new()
            .name("photo-worker".into())
            .spawn(move || worker(work_rx, reply_tx, ctx, backend))
            .expect("Starting photo worker");
        let mut app = Self {
            tx,
            rx,
            image: None,
            path: None,
            edits: Edits::default(),
            saved: Edits::default(),
            undo: vec![],
            redo: vec![],
            texture: None,
            histogram: vec![],
            generation: 0,
            load_generation: 0,
            preview_pending: false,
            busy: false,
            status: "Open a photograph to begin".into(),
            error: None,
            profile,
            space: OutputSpace::Srgb,
            zoom: 1.,
            center: [0.5; 2],
            viewport: [0., 0., 1., 1.],
            preview_size: [1200, 900],
            compare: false,
            grid: false,
            tool: Tool::View,
            clone_source: None,
            radius: 0.025,
            feather: 0.6,
            pending: None,
            close_after_save: false,
            stroke_recorded: false,
            brush_offset: [0.; 2],
            edit_gesture: None,
            fit_scale: 1.,
            mask: vec![],
            ai_steps: 20,
            ai_seed: 0,
        };
        if let Some(path) = input {
            app.open(path);
        }
        app
    }
    fn dirty(&self) -> bool {
        self.edits != self.saved
    }
    fn send(&mut self, work: Work) {
        if self.tx.send(work).is_err() {
            self.error = Some("Photo worker stopped".into());
            self.busy = false;
        }
    }
    fn open(&mut self, path: PathBuf) {
        self.load_generation = self.load_generation.wrapping_add(1);
        self.generation = self.generation.wrapping_add(1);
        self.busy = true;
        self.status = "Decoding photograph…".into();
        self.send(Work::Open {
            id: self.load_generation,
            path,
        });
    }
    fn request_open(&mut self, path: PathBuf) {
        if self.dirty() {
            self.pending = Some(Pending::Open(path));
        } else {
            self.open(path);
        }
    }
    fn choose_photo(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Open photograph")
            .add_filter(
                "Photographs",
                &[
                    "raf", "dng", "nef", "cr2", "cr3", "arw", "orf", "rw2", "pef", "srw", "raw",
                    "jpg", "jpeg", "png", "tif", "tiff", "exr",
                ],
            )
            .pick_file()
        {
            self.request_open(path);
        }
    }
    fn remember(&mut self, previous: Edits) {
        if previous == self.edits {
            return;
        }
        self.undo.push(previous);
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.changed();
    }
    fn changed(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.preview_pending = true;
    }
    fn undo(&mut self) {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.edits, previous));
            self.changed();
        }
    }
    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.edits, next));
            self.changed();
        }
    }
    fn save(&mut self) {
        if let Some(path) = &self.path {
            self.status = "Saving edits…".into();
            self.send(Work::Save {
                path: sidecar::path_for(path),
                edits: self.edits.clone(),
            });
        }
    }
    fn generate(&mut self, gaps: bool, regenerate: bool) {
        #[cfg(feature = "moebius")]
        if let (Some(image), Some(path)) = (&self.image, &self.path) {
            let work = Work::Generate {
                load_id: self.load_generation,
                path: path.clone(),
                image: image.clone(),
                edits: self.edits.clone(),
                dabs: self.mask.clone(),
                gaps,
                steps: self.ai_steps,
                seed: self.ai_seed,
                regenerate,
            };
            self.busy = true;
            self.status = "Generating local fill…".into();
            self.send(work);
        }
        #[cfg(not(feature = "moebius"))]
        let _ = (gaps, regenerate);
    }
    fn export(&mut self) {
        let (Some(image), Some(original)) = (&self.image, &self.path) else {
            return;
        };
        let default = original.file_stem().unwrap_or_default().to_string_lossy();
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Export photograph")
            .set_file_name(format!("{default}.png"))
            .add_filter("16-bit PNG", &["png"])
            .add_filter("16-bit TIFF", &["tif", "tiff"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("Float EXR", &["exr"])
            .save_file()
        {
            let space = if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exr"))
            {
                OutputSpace::LinearSrgb
            } else {
                self.space
            };
            let work = Work::Export {
                original: original.clone(),
                path,
                image: image.clone(),
                edits: self.edits.clone(),
                space,
            };
            self.busy = true;
            self.status = "Exporting full resolution…".into();
            self.send(work);
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(reply) = self.rx.try_recv() {
            match reply {
                Reply::Opened {
                    id,
                    path,
                    image,
                    edits,
                } if id == self.load_generation => {
                    self.path = Some(path);
                    self.image = Some(image);
                    self.edits = edits.clone();
                    self.saved = edits;
                    self.undo.clear();
                    self.redo.clear();
                    self.texture = None;
                    self.histogram.clear();
                    self.zoom = 1.;
                    self.center = [0.5; 2];
                    self.clone_source = None;
                    self.mask.clear();
                    self.busy = false;
                    self.changed();
                    self.status = "Ready".into();
                }
                Reply::Preview {
                    id,
                    size,
                    rgba,
                    histogram,
                    elapsed,
                    backend,
                } if id == self.generation => {
                    let image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                    if let Some(texture) = &mut self.texture {
                        texture.set(image, egui::TextureOptions::LINEAR);
                    } else {
                        self.texture = Some(ctx.load_texture(
                            "photograph",
                            image,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                    self.histogram = histogram;
                    if !self.busy {
                        self.status = format!("Preview {:.0} ms · {backend}", elapsed * 1000.);
                    }
                }
                Reply::Saved { path, edits }
                    if self
                        .path
                        .as_ref()
                        .is_some_and(|p| sidecar::path_for(p) == path) =>
                {
                    self.saved = edits;
                    self.status = "Edits saved".into();
                    if self.close_after_save && !self.dirty() {
                        self.close_after_save = false;
                        self.perform_pending(ctx);
                    }
                }
                #[cfg(feature = "moebius")]
                Reply::Generated {
                    load_id,
                    recipe_hash,
                    fills,
                    replace,
                } if load_id == self.load_generation => {
                    self.busy = false;
                    if crate::synthesis::recipe_hash(&self.edits).ok().as_ref()
                        == Some(&recipe_hash)
                    {
                        let previous = self.edits.clone();
                        if replace {
                            self.edits.display.synthesis.clear();
                        }
                        self.edits.display.synthesis.extend(fills);
                        self.remember(previous);
                        self.mask.clear();
                        self.tool = Tool::View;
                        self.status = "Generated fill ready; save edits to keep it".into();
                    }
                }
                Reply::Exported(path) => {
                    self.busy = false;
                    self.status = format!(
                        "Exported {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    );
                }
                Reply::Error(message) => {
                    self.error = Some(message);
                    self.busy = false;
                    self.close_after_save = false;
                }
                _ => {}
            }
        }
    }
    fn perform_pending(&mut self, ctx: &egui::Context) {
        match self.pending.take() {
            Some(Pending::Close) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Some(Pending::Open(path)) => self.open(path),
            None => {}
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("toolbar").exact_size(60.).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.add_space(14.);
                ui.label(
                    egui::RichText::new("rawpuppy")
                        .size(22.)
                        .strong()
                        .color(ACCENT),
                );
                ui.add_space(14.);
                if ui.button("Open…").clicked() {
                    self.choose_photo();
                }
                ui.add_enabled_ui(self.image.is_some() && !self.busy, |ui| {
                    if ui
                        .button(if self.dirty() {
                            "Save edits •"
                        } else {
                            "Save edits"
                        })
                        .clicked()
                    {
                        self.save();
                    }
                    if ui.button("Export…").clicked() {
                        self.export();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                        .clicked()
                    {
                        self.undo();
                    }
                    if ui
                        .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                        .clicked()
                    {
                        self.redo();
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(12.);
                    if let Some(path) = &self.path {
                        ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                    }
                });
            });
        });
        egui::Panel::bottom("status")
            .exact_size(34.)
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(14.);
                    if self.busy {
                        ui.spinner();
                    }
                    ui.label(
                        egui::RichText::new(&self.status)
                            .small()
                            .color(Color32::GRAY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(12.);
                        ui.label(
                            egui::RichText::new(
                                "B compare  ·  F fit  ·  1 actual pixels  ·  ⌘/Ctrl S save",
                            )
                            .small()
                            .color(Color32::GRAY),
                        );
                    });
                });
            });
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("edits")
            .exact_size(310.)
            .resizable(false)
            .show(ui, |ui| {
                ui.add_space(12.);
                if let Some(image) = &self.image {
                    ui.label(
                        egui::RichText::new(&image.metadata.model)
                            .size(17.)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "{} × {} · {:.1} MP",
                            image.metadata.width,
                            image.metadata.height,
                            image.metadata.width as f64 * image.metadata.height as f64 / 1e6
                        ))
                        .small()
                        .color(Color32::GRAY),
                    );
                } else {
                    ui.label(egui::RichText::new("Develop").size(17.).strong());
                }
                if !self.histogram.is_empty() {
                    let (rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 64.),
                        egui::Sense::hover(),
                    );
                    let points: Vec<_> = self
                        .histogram
                        .iter()
                        .enumerate()
                        .map(|(i, v)| {
                            Pos2::new(
                                rect.left() + i as f32 / 127. * rect.width(),
                                rect.bottom() - v * rect.height(),
                            )
                        })
                        .collect();
                    ui.painter().add(egui::Shape::line(
                        points,
                        egui::Stroke::new(1.2, Color32::from_gray(165)),
                    ));
                }
                ui.separator();
                let previous = self.edits.clone();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_enabled_ui(self.image.is_some() && !self.compare && !self.busy, |ui| {
                        egui::CollapsingHeader::new("Light")
                            .default_open(true)
                            .show(ui, |ui| {
                                slider(
                                    ui,
                                    &mut self.edits.scene.exposure,
                                    -8.0..=8.0,
                                    "Exposure",
                                    " EV",
                                );
                                slider(
                                    ui,
                                    &mut self.edits.scene.vignette[0],
                                    -3.0..=3.0,
                                    "Vignette",
                                    " EV",
                                );
                                slider(
                                    ui,
                                    &mut self.edits.scene.vignette[1],
                                    -3.0..=3.0,
                                    "Vignette edge",
                                    " EV",
                                );
                                slider(
                                    ui,
                                    &mut self.edits.scene.graduated.exposure,
                                    -4.0..=4.0,
                                    "Graduated ND",
                                    " EV",
                                );
                                if self.edits.scene.graduated.exposure != 0. {
                                    slider(
                                        ui,
                                        &mut self.edits.scene.graduated.angle,
                                        -180.0..=180.0,
                                        "Direction",
                                        "°",
                                    );
                                    slider(
                                        ui,
                                        &mut self.edits.scene.graduated.center[1],
                                        0.0..=1.0,
                                        "Position",
                                        "",
                                    );
                                    slider(
                                        ui,
                                        &mut self.edits.scene.graduated.width,
                                        0.01..=1.0,
                                        "Transition",
                                        "",
                                    );
                                }
                            });
                        egui::CollapsingHeader::new("Color calibration")
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("Relative to the camera white point")
                                        .small()
                                        .color(Color32::GRAY),
                                );
                                for (i, name) in ["Red", "Green", "Blue"].into_iter().enumerate() {
                                    slider(
                                        ui,
                                        &mut self.edits.scene.calibration[i],
                                        0.25..=4.0,
                                        name,
                                        "×",
                                    );
                                }
                                slider(
                                    ui,
                                    &mut self.edits.tone.saturation,
                                    0.0..=2.0,
                                    "Saturation",
                                    "×",
                                );
                                egui::CollapsingHeader::new("Channel mixer").show(ui, |ui| {
                                    for i in 0..3 {
                                        ui.horizontal(|ui| {
                                            ui.label(["R", "G", "B"][i]);
                                            for j in 0..3 {
                                                ui.add(
                                                    egui::DragValue::new(
                                                        &mut self.edits.scene.mixer[i][j],
                                                    )
                                                    .speed(0.01)
                                                    .range(-3.0..=3.0),
                                                );
                                            }
                                        });
                                    }
                                });
                            });
                        egui::CollapsingHeader::new("Geometry & lens").show(ui, |ui| {
                            slider(
                                ui,
                                &mut self.edits.geometry.rotation,
                                -180.0..=180.0,
                                "Rotate",
                                "°",
                            );
                            slider(
                                ui,
                                &mut self.edits.geometry.pitch,
                                -65.0..=65.0,
                                "Vertical",
                                "°",
                            );
                            slider(
                                ui,
                                &mut self.edits.geometry.yaw,
                                -65.0..=65.0,
                                "Horizontal",
                                "°",
                            );
                            slider(
                                ui,
                                &mut self.edits.geometry.field_of_view,
                                10.0..=120.0,
                                "Field of view",
                                "°",
                            );
                            slider(ui, &mut self.edits.geometry.scale, 0.5..=3.0, "Scale", "×");
                            slider(
                                ui,
                                &mut self.edits.geometry.distortion[0],
                                -0.5..=0.5,
                                "Distortion",
                                "",
                            );
                            slider(
                                ui,
                                &mut self.edits.geometry.distortion[1],
                                -0.5..=0.5,
                                "Distortion edge",
                                "",
                            );
                            slider(
                                ui,
                                &mut self.edits.geometry.chromatic_aberration[0],
                                -0.02..=0.02,
                                "Red fringe",
                                "",
                            );
                            slider(
                                ui,
                                &mut self.edits.geometry.chromatic_aberration[1],
                                -0.02..=0.02,
                                "Blue fringe",
                                "",
                            );
                            ui.checkbox(&mut self.grid, "Alignment grid");
                            ui.label("Crop");
                            let c = &mut self.edits.geometry.crop;
                            ui.horizontal(|ui| {
                                ui.label("x");
                                ui.add(
                                    egui::DragValue::new(&mut c[0])
                                        .speed(0.005)
                                        .range(0.0..=0.99),
                                );
                                ui.label("y");
                                ui.add(
                                    egui::DragValue::new(&mut c[1])
                                        .speed(0.005)
                                        .range(0.0..=0.99),
                                );
                            });
                            c[2] = c[2].min(1. - c[0]);
                            c[3] = c[3].min(1. - c[1]);
                            let max_width = 1. - c[0];
                            let max_height = 1. - c[1];
                            ui.horizontal(|ui| {
                                ui.label("w");
                                ui.add(
                                    egui::DragValue::new(&mut c[2])
                                        .speed(0.005)
                                        .range(0.01..=max_width),
                                );
                                ui.label("h");
                                ui.add(
                                    egui::DragValue::new(&mut c[3])
                                        .speed(0.005)
                                        .range(0.01..=max_height),
                                );
                            });
                            if ui.small_button("Reset crop").clicked() {
                                *c = [0., 0., 1., 1.];
                            }
                        });
                        egui::CollapsingHeader::new("Sensor & detail").show(ui, |ui| {
                            ui.checkbox(&mut self.edits.raw.hot_pixels, "Correct hot pixels");
                            slider(
                                ui,
                                &mut self.edits.raw.denoise,
                                0.0..=0.05,
                                "Sensor denoise",
                                "",
                            );
                        });
                        egui::CollapsingHeader::new("Tone").show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.selectable_value(
                                    &mut self.edits.tone.mapper,
                                    ToneMapper::Agx,
                                    "AgX",
                                );
                                ui.selectable_value(
                                    &mut self.edits.tone.mapper,
                                    ToneMapper::Linear,
                                    "Linear / HDR",
                                );
                            });
                            curve_editor(ui, &mut self.edits.display.curve);
                        });
                        egui::CollapsingHeader::new("Split toning").show(ui, |ui| {
                            slider(
                                ui,
                                &mut self.edits.display.split_strength,
                                0.0..=1.0,
                                "Strength",
                                "",
                            );
                            ui.horizontal(|ui| {
                                ui.label("Shadows");
                                ui.color_edit_button_rgb(&mut self.edits.display.shadows);
                            });
                            ui.horizontal(|ui| {
                                ui.label("Highlights");
                                ui.color_edit_button_rgb(&mut self.edits.display.highlights);
                            });
                        });
                        egui::CollapsingHeader::new("Retouch").show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut self.tool, Tool::View, "Pan");
                                ui.selectable_value(&mut self.tool, Tool::Clone, "Clone");
                                ui.selectable_value(&mut self.tool, Tool::Heal, "Heal");
                            });
                            ui.label(
                                egui::RichText::new("Alt-click a source, then paint a target.")
                                    .small(),
                            );
                            slider(ui, &mut self.radius, 0.002..=0.15, "Brush size", "");
                            slider(ui, &mut self.feather, 0.0..=1.0, "Feather", "");
                            ui.label(format!("{} strokes", self.edits.display.retouch.len()));
                            if ui.small_button("Clear retouch").clicked() {
                                self.edits.display.retouch.clear();
                            }
                        });
                        egui::CollapsingHeader::new("AI removal & corner fill").show(ui, |ui| {
                            let stale = crate::synthesis::recipe_hash(&self.edits).is_ok_and(|h| {
                                self.edits
                                    .display
                                    .synthesis
                                    .iter()
                                    .any(|f| f.recipe_sha256 != h)
                            });
                            ui.add_enabled_ui(cfg!(feature = "moebius"), |ui| {
                                ui.horizontal(|ui| {
                                    ui.selectable_value(&mut self.tool, Tool::Mask, "Paint area");
                                    ui.selectable_value(&mut self.tool, Tool::View, "Pan");
                                });
                                slider(ui, &mut self.radius, 0.002..=0.15, "Brush size", "");
                                ui.add(
                                    egui::Slider::new(&mut self.ai_steps, 2..=50)
                                        .text("Sampling steps"),
                                );
                                ui.horizontal(|ui| {
                                    ui.label("Seed");
                                    ui.add(egui::DragValue::new(&mut self.ai_seed));
                                });
                                if ui
                                    .add_enabled(
                                        !self.mask.is_empty(),
                                        egui::Button::new("Generate painted area"),
                                    )
                                    .clicked()
                                {
                                    self.generate(false, false);
                                }
                                if ui.button("Fill geometric corners").clicked() {
                                    self.generate(true, false);
                                }
                                if stale {
                                    ui.label("Preceding edits changed; update the fills.");
                                }
                                if !self.edits.display.synthesis.is_empty()
                                    && ui
                                        .button(if stale {
                                            "Update generated fills"
                                        } else {
                                            "Regenerate fills"
                                        })
                                        .clicked()
                                {
                                    self.generate(false, true);
                                }
                            });
                            if !cfg!(feature = "moebius") {
                                ui.label(
                                    egui::RichText::new(
                                        "AI generation is unavailable in this build.",
                                    )
                                    .small(),
                                );
                            }
                            ui.label(format!(
                                "{} generated layers",
                                self.edits.display.synthesis.len()
                            ));
                            if ui.small_button("Clear selection").clicked() {
                                self.mask.clear();
                            }
                            if ui.small_button("Clear generated fills").clicked() {
                                self.edits.display.synthesis.clear();
                            }
                        });
                        ui.separator();
                        egui::ComboBox::from_label("Export color")
                            .selected_text(format!("{:?}", self.space))
                            .show_ui(ui, |ui| {
                                for space in [
                                    OutputSpace::Srgb,
                                    OutputSpace::DisplayP3,
                                    OutputSpace::AdobeRgb,
                                    OutputSpace::Rec2020,
                                    OutputSpace::LinearSrgb,
                                ] {
                                    ui.selectable_value(
                                        &mut self.space,
                                        space,
                                        format!("{space:?}"),
                                    );
                                }
                            });
                        if ui.button("Choose display profile…").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("ICC profile", &["icc", "icm"])
                                .pick_file()
                        {
                            self.profile = Some(path);
                            self.changed();
                        }
                        ui.label(
                            egui::RichText::new(if self.profile.is_some() {
                                "Display: custom ICC"
                            } else {
                                "Display: sRGB"
                            })
                            .small()
                            .color(Color32::GRAY),
                        );
                        if ui.small_button("Reset all edits").clicked() {
                            self.edits = Edits::default();
                        }
                    });
                });
                if previous != self.edits {
                    if ui.input(|i| i.pointer.primary_down()) {
                        if self.edit_gesture.is_none() {
                            self.edit_gesture = Some(previous);
                        }
                        self.changed();
                    } else {
                        self.remember(previous);
                    }
                }
                if !ui.input(|i| i.pointer.primary_down())
                    && let Some(previous) = self.edit_gesture.take()
                {
                    self.remember(previous);
                }
            });
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(20, 22, 24))
                    .inner_margin(18.),
            )
            .show(ui, |ui| {
                if self.image.is_none() {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("One photograph. Your light.")
                                    .size(26.)
                                    .color(Color32::from_gray(195)),
                            );
                            ui.add_space(14.);
                            ui.label("Drop a RAW file here, or open a photograph.");
                            ui.add_space(18.);
                            if ui.button("Open photograph…").clicked() {
                                self.choose_photo();
                            }
                        });
                    });
                    return;
                }
                ui.horizontal(|ui| {
                    if ui.small_button("Fit").clicked() {
                        self.zoom = 1.;
                        self.center = [0.5; 2];
                        self.changed();
                    }
                    if ui.small_button("100%").clicked() {
                        self.actual_pixels(ui);
                    }
                    if ui.selectable_label(self.compare, "Before").clicked() {
                        self.compare = !self.compare;
                        self.changed();
                    }
                    ui.add_space(12.);
                    if matches!(self.tool, Tool::Clone | Tool::Heal) {
                        ui.label(
                            egui::RichText::new("Alt-click source · paint to retouch")
                                .color(ACCENT),
                        );
                    }
                });
                let image = self.image.as_ref().unwrap();
                let w = image.metadata.width as f32 * self.edits.geometry.crop[2];
                let h = image.metadata.height as f32 * self.edits.geometry.crop[3];
                let available = ui.available_size().max(Vec2::splat(1.));
                let fit = (available.x / w).min(available.y / h);
                self.fit_scale = fit;
                let full = Vec2::new(w, h) * fit * self.zoom;
                let visible = Vec2::new(full.x.min(available.x), full.y.min(available.y));
                let center = ui.available_rect_before_wrap().center();
                let rect = Rect::from_center_size(center, visible);
                let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                let fraction = [visible.x / full.x, visible.y / full.y];
                for (i, f) in fraction.iter().enumerate() {
                    self.center[i] = self.center[i].clamp(f * 0.5, 1. - f * 0.5);
                }
                let region = [
                    self.center[0] - fraction[0] * 0.5,
                    self.center[1] - fraction[1] * 0.5,
                    fraction[0],
                    fraction[1],
                ];
                let scale = ui.ctx().pixels_per_point();
                let size = [
                    (visible.x * scale).round().clamp(1., 8192.) as usize,
                    (visible.y * scale).round().clamp(1., 8192.) as usize,
                ];
                if region != self.viewport || size != self.preview_size {
                    self.viewport = region;
                    self.preview_size = size;
                    self.changed();
                }
                // Checkerboard makes out-of-image perspective corners visible.
                let square = 16.;
                for y in 0..(visible.y / square).ceil() as usize {
                    for x in 0..(visible.x / square).ceil() as usize {
                        let r = Rect::from_min_size(
                            rect.min + Vec2::new(x as f32 * square, y as f32 * square),
                            Vec2::splat(square),
                        )
                        .intersect(rect);
                        ui.painter().rect_filled(
                            r,
                            0.,
                            Color32::from_gray(if (x + y) % 2 == 0 { 31 } else { 36 }),
                        );
                    }
                }
                if let Some(texture) = &self.texture {
                    ui.painter().image(
                        texture.id(),
                        rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                        Color32::WHITE,
                    );
                }
                if self.grid {
                    for i in 1..6 {
                        let t = i as f32 / 6.;
                        ui.painter().line_segment(
                            [
                                Pos2::new(rect.left() + t * rect.width(), rect.top()),
                                Pos2::new(rect.left() + t * rect.width(), rect.bottom()),
                            ],
                            egui::Stroke::new(1., Color32::from_white_alpha(80)),
                        );
                        ui.painter().line_segment(
                            [
                                Pos2::new(rect.left(), rect.top() + t * rect.height()),
                                Pos2::new(rect.right(), rect.top() + t * rect.height()),
                            ],
                            egui::Stroke::new(1., Color32::from_white_alpha(80)),
                        );
                    }
                }
                if response.hovered() {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll.abs() > 0.1 {
                        self.zoom = (self.zoom * (scroll * 0.002).exp()).max(1.);
                        self.changed();
                    }
                    if self.tool != Tool::View
                        && let Some(pos) = response.hover_pos()
                    {
                        ui.painter().circle_stroke(
                            pos,
                            self.radius * w * fit * self.zoom,
                            egui::Stroke::new(1., ACCENT),
                        );
                    }
                }
                if response.double_clicked() {
                    self.zoom = 1.;
                    self.center = [0.5; 2];
                    self.changed();
                }
                if response.dragged()
                    && (self.tool == Tool::View || ui.input(|i| i.pointer.middle_down()))
                {
                    let delta = ui.input(|i| i.pointer.delta());
                    self.center[0] -= delta.x / full.x;
                    self.center[1] -= delta.y / full.y;
                    self.changed();
                }
                for dab in &self.mask {
                    let pos = Pos2::new(
                        rect.left() + (dab.center[0] - region[0]) / region[2] * rect.width(),
                        rect.top() + (dab.center[1] - region[1]) / region[3] * rect.height(),
                    );
                    if rect.contains(pos) {
                        ui.painter().circle_filled(
                            pos,
                            dab.radius * full.x,
                            Color32::from_rgba_unmultiplied(225, 150, 100, 75),
                        );
                    }
                }
                if self.tool == Tool::Mask
                    && !self.compare
                    && !self.busy
                    && (response.clicked() || response.dragged())
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let uv = [
                        region[0] + (pos.x - rect.left()) / rect.width() * region[2],
                        region[1] + (pos.y - rect.top()) / rect.height() * region[3],
                    ];
                    if self.mask.last().is_none_or(|dab| {
                        ((dab.center[0] - uv[0]).powi(2) + (dab.center[1] - uv[1]).powi(2)).sqrt()
                            > self.radius * 0.2
                    }) {
                        self.mask.push(crate::synthesis::MaskDab {
                            center: uv,
                            radius: self.radius,
                        });
                    }
                }
                if matches!(self.tool, Tool::Clone | Tool::Heal)
                    && !self.compare
                    && !self.busy
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let uv = [
                        region[0] + (pos.x - rect.left()) / rect.width() * region[2],
                        region[1] + (pos.y - rect.top()) / rect.height() * region[3],
                    ];
                    if ui.input(|i| i.modifiers.alt) && response.clicked() {
                        self.clone_source = Some(uv);
                    } else if (response.clicked() || response.dragged())
                        && let Some(source) = self.clone_source
                    {
                        if !self.stroke_recorded {
                            self.undo.push(self.edits.clone());
                            self.redo.clear();
                            self.stroke_recorded = true;
                            self.brush_offset = [source[0] - uv[0], source[1] - uv[1]];
                        }
                        self.edits.display.retouch.push(Retouch {
                            source: [uv[0] + self.brush_offset[0], uv[1] + self.brush_offset[1]],
                            target: uv,
                            radius: self.radius,
                            feather: self.feather,
                            opacity: 1.,
                            mode: if self.tool == Tool::Heal {
                                RetouchMode::Heal
                            } else {
                                RetouchMode::Clone
                            },
                        });
                        self.changed();
                    }
                }
                if !ui.input(|i| i.pointer.primary_down()) {
                    self.stroke_recorded = false;
                }
                if self.preview_pending && !self.busy {
                    let edits = if self.compare {
                        Edits {
                            geometry: self.edits.geometry.clone(),
                            ..Edits::default()
                        }
                    } else {
                        self.edits.clone()
                    };
                    self.send(Work::Render {
                        id: self.generation,
                        image: self.image.as_ref().unwrap().clone(),
                        edits,
                        region: self.viewport,
                        size: self.preview_size,
                        profile: self.profile.clone(),
                    });
                    self.preview_pending = false;
                }
            });
    }

    fn actual_pixels(&mut self, ui: &egui::Ui) {
        if self.image.is_some() {
            self.zoom = (1. / (self.fit_scale * ui.ctx().pixels_per_point())).max(1.);
            self.changed();
        }
    }
}

impl eframe::App for Editor {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .next()
        });
        if let Some(path) = dropped {
            self.request_open(path);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O)) {
            self.choose_photo();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.save();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::E)) {
            self.export();
        }
        if ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            )
        }) {
            self.redo();
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
            self.undo();
        }
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(egui::Key::F)) {
                self.zoom = 1.;
                self.center = [0.5; 2];
                self.changed();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Num1)) {
                self.actual_pixels(ui);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::B)) {
                self.compare = !self.compare;
                self.changed();
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(Pending::Close);
        }
        self.toolbar(ui);
        self.controls(ui);
        self.canvas(ui);
        if let Some(message) = self.error.clone() {
            egui::Window::new("Could not complete operation")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(&ctx, |ui| {
                    ui.set_max_width(500.);
                    ui.label(message);
                    if ui.button("Dismiss").clicked() {
                        self.error = None;
                    }
                });
        }
        if self.pending.is_some() {
            egui::Window::new("Save your edits?")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(&ctx, |ui| {
                    ui.label("This photograph has unsaved changes.");
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            self.close_after_save = true;
                            self.save();
                        }
                        if ui.button("Discard").clicked() {
                            self.saved = self.edits.clone();
                            self.perform_pending(&ctx);
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending = None;
                        }
                    });
                });
        }
    }
}

fn slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    label: &str,
    suffix: &str,
) {
    ui.add(egui::Slider::new(value, range).text(label).suffix(suffix));
}

fn curve_editor(ui: &mut egui::Ui, points: &mut Vec<[f32; 2]>) {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 150.),
        egui::Sense::click_and_drag(),
    );
    ui.painter().rect_filled(rect, 4., Color32::from_gray(20));
    let lut = color::curve_lut(points, 128);
    let line: Vec<_> = lut
        .iter()
        .enumerate()
        .map(|(i, y)| {
            Pos2::new(
                rect.left() + i as f32 / 127. * rect.width(),
                rect.bottom() - y * rect.height(),
            )
        })
        .collect();
    ui.painter()
        .add(egui::Shape::line(line, egui::Stroke::new(1.5, ACCENT)));
    for p in points.iter() {
        ui.painter().circle_filled(
            Pos2::new(
                rect.left() + p[0] * rect.width(),
                rect.bottom() - p[1] * rect.height(),
            ),
            3.,
            Color32::WHITE,
        );
    }
    if response.double_clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let x = ((pos.x - rect.left()) / rect.width()).clamp(0.01, 0.99);
        let i = points.partition_point(|p| p[0] < x);
        if points.iter().all(|p| (p[0] - x).abs() > 0.01) {
            let y = ((rect.bottom() - pos.y) / rect.height()).clamp(points[i - 1][1], points[i][1]);
            points.insert(i, [x, y]);
        }
    } else if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let x = ((pos.x - rect.left()) / rect.width()).clamp(0., 1.);
        let nearest = points
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| (a[0] - x).abs().total_cmp(&(b[0] - x).abs()))
            .map(|(i, _)| i)
            .unwrap();
        let lower = if nearest == 0 {
            0.
        } else {
            points[nearest - 1][1]
        };
        let upper = if nearest == points.len() - 1 {
            1.
        } else {
            points[nearest + 1][1]
        };
        points[nearest][1] = ((rect.bottom() - pos.y) / rect.height()).clamp(lower, upper);
    }
    ui.label(
        egui::RichText::new("Double-click to add a point; drag to shape.")
            .small()
            .color(Color32::GRAY),
    );
    if ui.small_button("Reset curve").clicked() {
        *points = vec![[0., 0.], [1., 1.]];
    }
}
