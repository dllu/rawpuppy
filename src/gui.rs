//! Native single-photo editor. I/O and photo rendering never run on the UI thread.
use crate::{
    color::{self, OutputSpace},
    display,
    edits::{Edits, LensEdits, LensMode, Reconstruction, Retouch, RetouchMode, ToneMapper},
    export,
    input::SensorImage,
    render::{Backend, Renderer},
    sidecar,
};
use anyhow::{Result, ensure};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use raw_window_handle::HasWindowHandle;
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    time::Instant,
};

const ACCENT: Color32 = Color32::from_rgb(225, 150, 100);

#[cfg(debug_assertions)]
mod probe;

#[derive(Clone)]
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
        profile: Option<display::Icc>,
        hdr: bool,
    },
    Save {
        load_id: u64,
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
        load_id: u64,
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
        pixels: PreviewPixels,
        histogram: Vec<f32>,
        elapsed: f64,
        backend: String,
        managed_display: bool,
        reconstruction_progress: Option<[usize; 2]>,
    },
    Saved {
        load_id: u64,
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
    Exported {
        load_id: u64,
        path: PathBuf,
    },
    Error {
        scope: ErrorScope,
        message: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ErrorScope {
    Load(u64),
    Preview(u64),
    Save(u64),
    Export(u64),
    #[cfg(feature = "moebius")]
    Generate(u64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Busy {
    Load(u64),
    Export(u64),
    #[cfg(feature = "moebius")]
    Generate(u64),
}
impl ErrorScope {
    fn applies(self, load: u64, preview: u64) -> bool {
        match self {
            Self::Load(id) => id == load,
            Self::Preview(id) => id == preview,
            // Durable I/O failures still need to name the failed destination,
            // even after the user changes photographs.
            Self::Save(_) | Self::Export(_) => true,
            #[cfg(feature = "moebius")]
            Self::Generate(id) => id == load,
        }
    }
    fn finish_busy(self, busy: &mut Option<Busy>) {
        let finished = match self {
            Self::Load(id) => Some(Busy::Load(id)),
            Self::Export(id) => Some(Busy::Export(id)),
            #[cfg(feature = "moebius")]
            Self::Generate(id) => Some(Busy::Generate(id)),
            Self::Preview(_) | Self::Save(_) => None,
        };
        if finished.is_some() && *busy == finished {
            *busy = None;
        }
    }
}
impl Work {
    fn error_scope(&self) -> ErrorScope {
        match self {
            Self::Open { id, .. } => ErrorScope::Load(*id),
            Self::Render { id, .. } => ErrorScope::Preview(*id),
            #[cfg(feature = "moebius")]
            Self::Generate { load_id, .. } => ErrorScope::Generate(*load_id),
            Self::Save { load_id, .. } => ErrorScope::Save(*load_id),
            Self::Export { load_id, .. } => ErrorScope::Export(*load_id),
        }
    }
}

enum PreviewPixels {
    Sdr(Vec<u8>),
    Hdr(display::hdr::Frame),
}

fn worker(rx: mpsc::Receiver<Work>, tx: mpsc::Sender<Reply>, ctx: egui::Context, backend: Backend) {
    let mut renderer = Renderer::new(backend);
    let mut display_encoder = display::Encoder::default();
    let mut last_preview = None;
    loop {
        let mut work = if renderer.reconstruction_pending() {
            match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(work) => work,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let Some(work) = last_preview.clone() else {
                        continue;
                    };
                    work
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(work) => work,
                Err(_) => break,
            }
        };
        // Coalesce only preview requests; durable save/export operations are always executed.
        while matches!(work, Work::Render { .. }) {
            match rx.try_recv() {
                Ok(next) => work = next,
                Err(_) => break,
            }
        }
        if matches!(work, Work::Render { .. }) {
            last_preview = Some(work.clone());
        }
        let scope = work.error_scope();
        let destination = match &work {
            Work::Save { path, .. } => Some(format!("Saving edits to {}", path.display())),
            Work::Export { path, .. } => Some(format!("Exporting {}", path.display())),
            _ => None,
        };
        let result: Result<Reply> = (|| match work {
            Work::Open { id, path } => {
                renderer.cancel_reconstruction();
                last_preview = None;
                let image = Arc::new(SensorImage::open(&path)?);
                let edits = sidecar::load_for_default(&path, Edits::for_image(&image))?;
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
                hdr,
            } => {
                let start = Instant::now();
                let mut preview_edits = edits.clone();
                let hash = crate::synthesis::recipe_hash(&edits)?;
                preview_edits.display.synthesis.retain(|fill| {
                    fill.recipe_sha256 == hash
                        && fill.source_color_revision == image.metadata.color_revision
                });
                let (rendered, reconstruction_progress) = renderer.render_preview_region(
                    image,
                    &preview_edits,
                    region,
                    size[0],
                    size[1],
                )?;
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
                let pixels = if hdr {
                    ensure!(
                        profile.is_none(),
                        "HDR previews require automatic compositor color management"
                    );
                    PreviewPixels::Hdr(display::hdr::Frame::from_rendered(&rendered, 1.)?)
                } else {
                    PreviewPixels::Sdr(display_encoder.encode(&rendered, profile.as_ref())?)
                };
                Ok(Reply::Preview {
                    id,
                    size,
                    pixels,
                    histogram,
                    elapsed: start.elapsed().as_secs_f64(),
                    backend: renderer.label().into(),
                    managed_display: profile.is_none(),
                    reconstruction_progress,
                })
            }
            Work::Save {
                load_id,
                path,
                edits,
            } => {
                sidecar::save(&path, &edits)?;
                Ok(Reply::Saved {
                    load_id,
                    path,
                    edits,
                })
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
                let (w, h) = renderer.dimensions(image.clone(), &edits, None)?;
                let wants_gaps =
                    gaps || regenerate && edits.display.synthesis.iter().any(|fill| fill.fill_gaps);
                let gap_regions = if wants_gaps {
                    let mut base = edits.clone();
                    base.display.synthesis.clear();
                    renderer.gap_contexts(image.clone(), &base)?
                } else {
                    vec![]
                };
                let jobs = if regenerate {
                    crate::synthesis::regeneration_contexts(
                        &edits.display.synthesis,
                        w,
                        h,
                        &gap_regions,
                    )?
                } else if gaps {
                    gap_regions
                        .into_iter()
                        .map(|region| crate::synthesis::FillContext {
                            region,
                            dabs: vec![],
                            fill_gaps: true,
                        })
                        .collect()
                } else {
                    vec![crate::synthesis::FillContext {
                        region: crate::synthesis::brush_context(&dabs, w, h)?,
                        dabs,
                        fill_gaps: false,
                    }]
                };
                if !regenerate {
                    ensure!(!jobs.is_empty(), "No geometric gaps to fill");
                }
                let mut fills = Vec::new();
                let mut current = edits.clone();
                if regenerate {
                    current.display.synthesis.clear();
                }
                for job in jobs {
                    let settings = crate::moebius::Sampling {
                        steps,
                        seed,
                        ..Default::default()
                    };
                    if let Some(fill) = renderer.generate_fill(
                        image.clone(),
                        &current,
                        job.region,
                        job.dabs,
                        job.fill_gaps,
                        &settings,
                    )? {
                        current.display.synthesis.push(fill.clone());
                        fills.push(fill);
                    }
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
                load_id,
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
                Ok(Reply::Exported { load_id, path })
            }
        })();
        let reply = result.unwrap_or_else(|e| {
            let error = if let Some(destination) = destination {
                e.context(destination)
            } else {
                e
            };
            Reply::Error {
                scope,
                message: format!("{error:#}"),
            }
        });
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

#[derive(Default)]
struct BrushStroke {
    previous: Option<[f32; 2]>,
}
impl BrushStroke {
    fn points(
        &mut self,
        uv: [f32; 2],
        press: Option<[f32; 2]>,
        radius: f32,
        aspect: f32,
    ) -> Vec<[f32; 2]> {
        if press.is_some() {
            self.previous = None;
        }
        let first = self.previous.is_none();
        let start = self.previous.unwrap_or(press.unwrap_or(uv));
        let delta = [
            uv[0] as f64 - start[0] as f64,
            uv[1] as f64 - start[1] as f64,
        ];
        let distance = delta[0].hypot(delta[1] * aspect as f64);
        let spacing = radius as f64 * 0.2;
        if !first && distance < spacing {
            return vec![];
        }
        self.previous = Some(uv);
        // Clip captured drags to the canvas plus a brush radius. Pointer travel
        // outside a narrow photo must not create a huge list of invisible dabs.
        let mut enter = 0f64;
        let mut leave = 1f64;
        for axis in 0..2 {
            let scale = if axis == 0 { 1. } else { aspect as f64 };
            let p = start[axis] as f64 * scale;
            let d = delta[axis] * scale;
            let min = -(radius as f64);
            let max = scale + radius as f64;
            if d == 0. {
                if p < min || p > max {
                    return vec![];
                }
            } else {
                let a = (min - p) / d;
                let b = (max - p) / d;
                enter = enter.max(a.min(b));
                leave = leave.min(a.max(b));
                if enter > leave {
                    return vec![];
                }
            }
        }
        let point = |t: f64| std::array::from_fn(|i| (start[i] as f64 + t * delta[i]) as f32);
        let mut points = Vec::new();
        if first || enter > 0. {
            points.push(point(enter));
        }
        let steps = ((leave - enter) * distance / spacing).ceil() as usize;
        for i in 1..=steps {
            points.push(point(enter + (leave - enter) * i as f64 / steps as f64));
        }
        points
    }
}
enum Pending {
    Close,
    Open(PathBuf),
}

pub fn run(
    input: Option<PathBuf>,
    profile: Option<PathBuf>,
    backend: Backend,
    hdr: bool,
) -> Result<()> {
    ensure!(
        !hdr || profile.is_none(),
        "HDR preview uses Automatic display colour; remove --display-profile"
    );
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440., 960.])
            .with_min_inner_size([800., 550.]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::WgpuConfiguration {
            prefer_extended_linear_hdr: hdr,
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "Rawpuppy",
        options,
        Box::new(move |cc| Ok(Box::new(Editor::new(cc, input, profile, backend, hdr)))),
    )
    .map_err(|e| anyhow::anyhow!("Opening native editor: {e}"))
}

struct Editor {
    #[cfg(debug_assertions)]
    probe: Option<probe::Probe>,
    #[cfg(debug_assertions)]
    probe_photo_rect: Option<Rect>,
    tx: mpsc::Sender<Work>,
    rx: mpsc::Receiver<Reply>,
    image: Option<Arc<SensorImage>>,
    path: Option<PathBuf>,
    edits: Edits,
    saved: Edits,
    undo: Vec<Edits>,
    redo: Vec<Edits>,
    texture: Option<egui::TextureHandle>,
    hdr_texture: Option<Arc<display::hdr::Texture>>,
    presentation: Option<eframe::egui_wgpu::RenderState>,
    hdr_requested: bool,
    hdr_white_scale: Option<f32>,
    histogram: Vec<f32>,
    generation: u64,
    load_generation: u64,
    preview_pending: bool,
    busy: Option<Busy>,
    status: String,
    error: Option<String>,
    profile: Option<PathBuf>,
    display_tx: mpsc::Sender<display::Request>,
    display_rx: mpsc::Receiver<(display::Request, Result<display::Resolved, String>)>,
    display_request: Option<display::Request>,
    display_polled: Instant,
    display_resolved: display::Resolved,
    #[cfg(target_os = "macos")]
    surface_encoding: Option<bool>,
    #[cfg(target_os = "macos")]
    present_managed: bool,
    #[cfg(target_os = "linux")]
    wayland_surface: Option<display::WaylandSurface>,
    #[cfg(target_os = "linux")]
    wayland_surface_checked: bool,
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
    brush_stroke: BrushStroke,
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
        hdr_requested: bool,
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
        let (display_tx, display_work_rx) = mpsc::channel::<display::Request>();
        let (display_reply_tx, display_rx) = mpsc::channel();
        let display_ctx = cc.egui_ctx.clone();
        std::thread::Builder::new()
            .name("display-profile-worker".into())
            .spawn(move || {
                while let Ok(mut request) = display_work_rx.recv() {
                    while let Ok(next) = display_work_rx.try_recv() {
                        request = next;
                    }
                    let result = display::discover(&request).map_err(|e| e.to_string());
                    if display_reply_tx.send((request, result)).is_err() {
                        break;
                    }
                    display_ctx.request_repaint();
                }
            })
            .expect("Starting display profile worker");
        let ctx = cc.egui_ctx.clone();
        std::thread::Builder::new()
            .name("photo-worker".into())
            .spawn(move || worker(work_rx, reply_tx, ctx, backend))
            .expect("Starting photo worker");
        let mut app = Self {
            #[cfg(debug_assertions)]
            probe: probe::Probe::from_env(),
            #[cfg(debug_assertions)]
            probe_photo_rect: None,
            tx,
            rx,
            image: None,
            path: None,
            edits: Edits::default(),
            saved: Edits::default(),
            undo: vec![],
            redo: vec![],
            texture: None,
            hdr_texture: None,
            presentation: cc.wgpu_render_state.clone(),
            hdr_requested,
            hdr_white_scale: cc
                .wgpu_render_state
                .as_ref()
                .filter(|state| {
                    state.target_format == eframe::wgpu::TextureFormat::Rgba16Float
                        && state.target_color_space
                            == eframe::wgpu::SurfaceColorSpace::ExtendedSrgbLinear
                })
                .map(|_| 1.),
            histogram: vec![],
            generation: 0,
            load_generation: 0,
            preview_pending: false,
            busy: None,
            status: "Open a photograph to begin".into(),
            error: None,
            profile,
            display_tx,
            display_rx,
            display_request: None,
            display_polled: Instant::now(),
            display_resolved: display::Resolved::default(),
            #[cfg(target_os = "macos")]
            surface_encoding: None,
            #[cfg(target_os = "macos")]
            present_managed: true,
            #[cfg(target_os = "linux")]
            wayland_surface: None,
            #[cfg(target_os = "linux")]
            wayland_surface_checked: false,
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
            brush_stroke: BrushStroke::default(),
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
    fn refresh_display(&mut self, frame: &eframe::Frame, ctx: &egui::Context) {
        if self.hdr_white_scale.is_some() {
            let desktop = frame
                .winit_window()
                .and_then(|w| w.window_handle().ok())
                .map_or(display::Desktop::Other, |h| display::desktop(h.as_raw()));
            let state = self.presentation.as_ref().unwrap();
            let scale =
                display::hdr::reference_white_scale(desktop, &state.display_hdr_info.read());
            state.renderer.write().set_hdr_white_scale(scale);
            if let Some(texture) = &self.hdr_texture
                && let Err(error) = texture.set_white_scale(&state.queue, scale)
            {
                self.error = Some(error.to_string());
            }
            if self.hdr_white_scale != Some(scale) {
                self.hdr_white_scale = Some(scale);
                ctx.request_repaint();
            }
            self.display_resolved = display::Resolved {
                icc: None,
                label: "Automatic: extended-linear HDR surface".into(),
            };
            ctx.request_repaint_after(std::time::Duration::from_secs(2));
            return;
        }
        let mut request = display::Request {
            custom: self.profile.clone(),
            ..Default::default()
        };
        if let Some(window) = frame.winit_window() {
            if let Ok(handle) = window.window_handle() {
                request.desktop = display::desktop(handle.as_raw());
            }
            request.monitor = window.current_monitor().map(|monitor| {
                let p = monitor.position();
                let s = monitor.size();
                display::Monitor {
                    name: monitor.name(),
                    rect: [p.x, p.y, s.width as i32, s.height as i32],
                }
            });
        }
        if self.display_request.as_ref() != Some(&request)
            || self.display_polled.elapsed().as_secs() >= 2
        {
            self.display_request = Some(request.clone());
            self.display_polled = Instant::now();
            let _ = self.display_tx.send(request);
        }
        while let Ok((request, result)) = self.display_rx.try_recv() {
            if self.display_request.as_ref() != Some(&request) {
                continue;
            }
            let resolved = match result {
                Ok(resolved) => resolved,
                Err(error) => {
                    self.error = Some(error);
                    display::Resolved::default()
                }
            };
            if resolved != self.display_resolved {
                self.display_resolved = resolved;
                self.changed();
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(2));
        #[cfg(target_os = "linux")]
        if !self.wayland_surface_checked
            && let Some(window) = frame.winit_window()
            && let Ok(handle) = window.window_handle()
            && matches!(
                handle.as_raw(),
                raw_window_handle::RawWindowHandle::Wayland(_)
            )
        {
            self.wayland_surface_checked = true;
            match display::WaylandSurface::bind(window.clone()) {
                Ok(surface) => self.wayland_surface = surface,
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        #[cfg(target_os = "linux")]
        if let Some(surface) = &mut self.wayland_surface {
            if let Err(error) = surface.poll() {
                self.error = Some(error.to_string());
            }
            if surface.pending() {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
        #[cfg(target_os = "macos")]
        if self.surface_encoding != Some(self.present_managed)
            && let Some(window) = frame.winit_window()
            && let Ok(handle) = window.window_handle()
        {
            match display::set_surface_encoding(handle.as_raw(), self.present_managed) {
                Ok(count) if count > 0 => self.surface_encoding = Some(self.present_managed),
                Err(error) => self.error = Some(error.to_string()),
                _ => {}
            }
        }
    }
    fn send(&mut self, work: Work) {
        if self.tx.send(work).is_err() {
            self.error = Some("Photo worker stopped".into());
            self.busy = None;
        }
    }
    fn open(&mut self, path: PathBuf) {
        self.load_generation = self.load_generation.wrapping_add(1);
        self.generation = self.generation.wrapping_add(1);
        self.busy = Some(Busy::Load(self.load_generation));
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
        if matches!(self.busy, Some(Busy::Load(_))) {
            return;
        }
        if let Some(path) = &self.path {
            self.status = "Saving edits…".into();
            self.send(Work::Save {
                load_id: self.load_generation,
                path: sidecar::path_for(path),
                edits: self.edits.clone(),
            });
        }
    }
    fn generate(&mut self, gaps: bool, regenerate: bool) {
        if self.busy.is_some() {
            return;
        }
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
            self.busy = Some(Busy::Generate(self.load_generation));
            self.status = "Generating local fill…".into();
            self.send(work);
        }
        #[cfg(not(feature = "moebius"))]
        let _ = (gaps, regenerate);
    }
    fn export(&mut self) {
        if self.busy.is_some() {
            return;
        }
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
                load_id: self.load_generation,
                original: original.clone(),
                path,
                image: image.clone(),
                edits: self.edits.clone(),
                space,
            };
            self.busy = Some(Busy::Export(self.load_generation));
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
                    self.hdr_texture = None;
                    self.histogram.clear();
                    self.zoom = 1.;
                    self.center = [0.5; 2];
                    self.clone_source = None;
                    self.brush_stroke = BrushStroke::default();
                    self.stroke_recorded = false;
                    self.mask.clear();
                    self.busy = None;
                    self.changed();
                    self.status = "Ready".into();
                }
                Reply::Preview {
                    id,
                    size,
                    pixels,
                    histogram,
                    elapsed,
                    backend,
                    managed_display,
                    reconstruction_progress,
                } if id == self.generation => {
                    #[cfg(target_os = "macos")]
                    {
                        self.present_managed = managed_display;
                    }
                    #[cfg(not(target_os = "macos"))]
                    let _ = managed_display;
                    match pixels {
                        PreviewPixels::Sdr(rgba) => {
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
                            self.hdr_texture = None;
                        }
                        PreviewPixels::Hdr(hdr) => {
                            let state = self.presentation.as_ref().unwrap();
                            match display::hdr::Texture::upload(&state.device, &state.queue, &hdr) {
                                Ok(texture) => {
                                    self.hdr_texture = Some(texture);
                                    self.texture = None;
                                }
                                Err(error) => {
                                    self.error = Some(error.to_string());
                                }
                            }
                        }
                    }
                    self.histogram = histogram;
                    if self.busy.is_none() {
                        self.status = if let Some([done, total]) = reconstruction_progress {
                            if total == 0 {
                                "Preparing Joint AI · Standard preview".into()
                            } else {
                                format!("Preparing Joint AI {done}/{total} · Standard preview")
                            }
                        } else {
                            format!("Preview {:.0} ms · {backend}", elapsed * 1000.)
                        };
                    }
                }
                Reply::Saved {
                    load_id,
                    path,
                    edits,
                } if load_id == self.load_generation
                    && self
                        .path
                        .as_ref()
                        .is_some_and(|p| sidecar::path_for(p) == path) =>
                {
                    self.saved = edits;
                    if self.busy.is_none() {
                        self.status = "Edits saved".into();
                    }
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
                    ErrorScope::Generate(load_id).finish_busy(&mut self.busy);
                    if crate::synthesis::recipe_hash(&self.edits).ok().as_ref()
                        == Some(&recipe_hash)
                    {
                        let previous = self.edits.clone();
                        let empty = fills.is_empty();
                        if replace {
                            self.edits.display.synthesis.clear();
                        }
                        self.edits.display.synthesis.extend(fills);
                        self.remember(previous);
                        self.mask.clear();
                        self.tool = Tool::View;
                        self.status = if empty && replace {
                            "No geometric gaps remain; save edits to keep the update".into()
                        } else if empty {
                            "No geometric gaps remain".into()
                        } else {
                            "Generated fill ready; save edits to keep it".into()
                        };
                    }
                }
                Reply::Exported { load_id, path } if self.busy == Some(Busy::Export(load_id)) => {
                    self.busy = None;
                    self.status = format!(
                        "Exported {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    );
                }
                Reply::Error { scope, message }
                    if scope.applies(self.load_generation, self.generation) =>
                {
                    self.error = Some(message);
                    scope.finish_busy(&mut self.busy);
                    if matches!(scope, ErrorScope::Save(id) if id == self.load_generation) {
                        self.close_after_save = false;
                    }
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
                ui.add_enabled_ui(self.image.is_some() && self.busy.is_none(), |ui| {
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
                    if self.busy.is_some() {
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
        let source_image = self.image.clone();
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
                    ui.add_enabled_ui(self.image.is_some() && !self.compare && self.busy.is_none(), |ui| {
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
                            if let Some(name) = source_image.as_ref().and_then(|i| i.metadata.lens_model.as_ref()) {
                                ui.label(egui::RichText::new(name).small().color(Color32::GRAY));
                            }
                            let profile = source_image.as_ref().and_then(|i| i.metadata.lens_profile.as_ref());
                            let available = profile.is_some_and(|p| p.distortion.is_some() || p.vignette.is_some() || p.has_chromatic_aberration());
                            let mut enabled = self.edits.lens.mode != LensMode::Off;
                            if ui.add_enabled(available, egui::Checkbox::new(&mut enabled, "Camera lens corrections")).changed() {
                                self.edits.lens = if enabled {
                                    LensEdits { mode: LensMode::EmbeddedV1, distortion: profile.is_some_and(|p| p.distortion.is_some()), vignette: profile.is_some_and(|p| p.vignette.is_some()), chromatic_aberration: profile.is_some_and(|p| p.has_chromatic_aberration()), auto_frame: true }
                                } else { LensEdits::default() };
                            }
                            if enabled {
                                ui.horizontal(|ui| {
                                    ui.add_enabled(profile.is_some_and(|p| p.distortion.is_some()), egui::Checkbox::new(&mut self.edits.lens.distortion, "Distortion"));
                                    ui.add_enabled(profile.is_some_and(|p| p.vignette.is_some()), egui::Checkbox::new(&mut self.edits.lens.vignette, "Vignetting"));
                                });
                                ui.add_enabled(profile.is_some_and(|p| p.has_chromatic_aberration()), egui::Checkbox::new(&mut self.edits.lens.chromatic_aberration, "Chromatic aberration"));
                                ui.checkbox(&mut self.edits.lens.auto_frame, "Avoid camera correction gaps");
                            }
                            if let Some(error) = source_image.as_ref().and_then(|i| i.metadata.lens_profile_error.as_ref()) {
                                ui.label(egui::RichText::new("Camera correction data unavailable").small()).on_hover_text(error);
                            }
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
                            egui::ComboBox::from_label("Reconstruction")
                                .selected_text(match self.edits.raw.reconstruction {
                                    Reconstruction::Mhc => "Standard",
                                    Reconstruction::RawNindV1 => "Joint AI (experimental)",
                                }).show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.edits.raw.reconstruction,
                                                        Reconstruction::Mhc, "Standard");
                                    #[cfg(feature = "raw-ml")]
                                    {
                                        let supported = source_image.as_ref().is_some_and(|image|
                                            image.cfa.as_ref().is_some_and(|c| c.width == 2 && c.height == 2));
                                        let installed = crate::models::raw_model_path().is_ok_and(|p| p.join("manifest.json").is_file());
                                        ui.add_enabled_ui(supported && installed, |ui| {
                                            ui.selectable_value(&mut self.edits.raw.reconstruction,
                                                Reconstruction::RawNindV1, "Joint AI (experimental)");
                                        });
                                    }
                                });
                            if self.edits.raw.reconstruction == Reconstruction::RawNindV1 {
                                ui.label("The first pass prepares the photo; later adjustments reuse it.");
                            }
                            ui.checkbox(&mut self.edits.raw.hot_pixels, "Correct hot pixels");
                            if self.edits.raw.reconstruction == Reconstruction::Mhc { slider(
                                ui,
                                &mut self.edits.raw.denoise,
                                0.0..=0.05,
                                "Sensor denoise",
                                "",
                            ); }
                        });
                        egui::CollapsingHeader::new("Tone").show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.selectable_value(
                                    &mut self.edits.tone.mapper,
                                    ToneMapper::AgxSdr,
                                    "AgX",
                                );
                                ui.selectable_value(
                                    &mut self.edits.tone.mapper,
                                    ToneMapper::Linear,
                                    "Linear / HDR",
                                );
                            });
                            if self.edits.tone.mapper == ToneMapper::Agx {
                                ui.label("This recipe keeps the older AgX approximation. Select AgX to update its rendering.");
                            }
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
                                    .any(|f| f.recipe_sha256 != h || f.source_color_revision!=self.image.as_ref().map_or(0,|i|i.metadata.color_revision))
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
                                    ui.label("Image processing changed; update the fills.");
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
                        if ui.add_enabled(self.hdr_white_scale.is_none(), egui::Button::new("Choose display profile…")).clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("ICC profile", &["icc", "icm"])
                                .pick_file()
                        {
                            self.profile = Some(path);
                            self.changed();
                        }
                        if self.profile.is_some() && ui.small_button("Use automatic display colour").clicked() {self.profile=None;}
                        let display_label = self.display_resolved.label.as_str();
                        let display_label = if self.hdr_requested && self.hdr_white_scale.is_none() && self.profile.is_none() {
                            "Automatic: SDR surface (HDR unavailable)"
                        } else { display_label };
                        #[cfg(target_os = "linux")]
                        let display_label = if self.wayland_surface.as_ref().is_some_and(|surface| surface.tagged()) {
                            if self.hdr_requested { "Automatic: managed sRGB (HDR unavailable)" }
                            else { "Automatic: compositor-managed sRGB surface" }
                        } else { display_label };
                        ui.label(
                            egui::RichText::new(display_label)
                            .small()
                            .color(Color32::GRAY),
                        );
                        if ui.small_button("Reset all edits").clicked() {
                            self.edits = source_image.as_ref().map_or_else(Edits::default, |i| Edits::for_image(i));
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
                #[cfg(debug_assertions)]
                {
                    self.probe_photo_rect = Some(rect);
                }
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
                if let Some(texture) = &self.hdr_texture {
                    ui.painter().add(texture.callback(rect));
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
                let press = ui.input(|i| {
                    i.pointer
                        .primary_pressed()
                        .then(|| i.pointer.press_origin())
                        .flatten()
                });
                let press = press.map(|pos| {
                    [
                        region[0] + (pos.x - rect.left()) / rect.width() * region[2],
                        region[1] + (pos.y - rect.top()) / rect.height() * region[3],
                    ]
                });
                let mut painting = false;
                if self.tool == Tool::Mask
                    && !self.compare
                    && self.busy.is_none()
                    && primary_stroke(&response)
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let uv = [
                        region[0] + (pos.x - rect.left()) / rect.width() * region[2],
                        region[1] + (pos.y - rect.top()) / rect.height() * region[3],
                    ];
                    painting = true;
                    self.mask.extend(
                        self.brush_stroke
                            .points(uv, press, self.radius, h / w)
                            .into_iter()
                            .map(|center| crate::synthesis::MaskDab {
                                center,
                                radius: self.radius,
                            }),
                    );
                }
                if matches!(self.tool, Tool::Clone | Tool::Heal)
                    && !self.compare
                    && self.busy.is_none()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let uv = [
                        region[0] + (pos.x - rect.left()) / rect.width() * region[2],
                        region[1] + (pos.y - rect.top()) / rect.height() * region[3],
                    ];
                    if ui.input(|i| i.modifiers.alt)
                        && response.clicked_by(egui::PointerButton::Primary)
                    {
                        self.clone_source = Some(uv);
                    } else if !ui.input(|i| i.modifiers.alt)
                        && primary_stroke(&response)
                        && let Some(source) = self.clone_source
                    {
                        painting = true;
                        let points = self.brush_stroke.points(uv, press, self.radius, h / w);
                        if let Some(start) = points.first() {
                            if !self.stroke_recorded {
                                self.undo.push(self.edits.clone());
                                self.redo.clear();
                                self.stroke_recorded = true;
                                self.brush_offset = [source[0] - start[0], source[1] - start[1]];
                            }
                            self.edits
                                .display
                                .retouch
                                .extend(points.into_iter().map(|target| Retouch {
                                    source: [
                                        target[0] + self.brush_offset[0],
                                        target[1] + self.brush_offset[1],
                                    ],
                                    target,
                                    // The cursor uses the cropped output width; saved
                                    // retouch radii use the full corrected canvas width.
                                    radius: self.radius * self.edits.geometry.crop[2],
                                    feather: self.feather,
                                    opacity: 1.,
                                    mode: if self.tool == Tool::Heal {
                                        RetouchMode::Heal
                                    } else {
                                        RetouchMode::Clone
                                    },
                                }));
                            self.changed();
                        }
                    }
                }
                if !painting || !ui.input(|i| i.pointer.primary_down()) {
                    self.stroke_recorded = false;
                    self.brush_stroke = BrushStroke::default();
                }
                if self.preview_pending && self.busy.is_none() {
                    let edits = if self.compare {
                        Edits {
                            geometry: self.edits.geometry.clone(),
                            lens: self.edits.lens.clone(),
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
                        profile: self.display_resolved.icc.clone(),
                        hdr: self.hdr_white_scale.is_some(),
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
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.refresh_display(frame, &ctx);
        #[cfg(debug_assertions)]
        if let Some(probe) = &mut self.probe
            && let Err(error) = probe.tick(
                &ctx,
                self.presentation.as_ref(),
                self.hdr_white_scale,
                self.probe_photo_rect,
                self.texture.is_some() || self.hdr_texture.is_some(),
            )
        {
            self.error = Some(error.to_string());
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
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

fn primary_stroke(response: &egui::Response) -> bool {
    !response.ctx.input(|i| i.pointer.middle_down())
        && (response.clicked_by(egui::PointerButton::Primary)
            || response.dragged_by(egui::PointerButton::Primary)
            || response.drag_stopped_by(egui::PointerButton::Primary)
            || (response.is_pointer_button_down_on()
                && response.ctx.input(|i| i.pointer.primary_down())))
}

#[cfg(test)]
mod brush_interaction_tests {
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
                assert_eq!(app.undo.len(), 2, "Undo must retain whole gestures");
            }
        }
    }
}

fn curve_editor(ui: &mut egui::Ui, points: &mut Vec<[f32; 2]>) -> Rect {
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
    let dragged_point = response.id.with("dragged-curve-point");
    if response.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        let x = ((origin.x - rect.left()) / rect.width()).clamp(0., 1.);
        let nearest = points
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| (a[0] - x).abs().total_cmp(&(b[0] - x).abs()))
            .map(|(i, _)| i)
            .unwrap();
        ui.data_mut(|data| data.insert_temp(dragged_point, nearest));
    }
    if response.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
        ui.data_mut(|data| data.remove::<usize>(dragged_point));
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
        && let Some(nearest) = ui
            .data(|data| data.get_temp::<usize>(dragged_point))
            .filter(|index| *index < points.len())
    {
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
        ui.data_mut(|data| data.remove::<usize>(dragged_point));
    }
    rect
}

#[cfg(test)]
mod curve_interaction_tests {
    use super::*;
    fn frame(
        ctx: &egui::Context,
        points: &mut Vec<[f32; 2]>,
        time: f64,
        events: Vec<egui::Event>,
    ) -> Rect {
        let mut rectangle = Rect::NOTHING;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400., 300.))),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| rectangle = curve_editor(ui, points),
        );
        output.textures_delta.clear();
        rectangle
    }
    fn button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }
    #[test]
    fn a_curve_drag_keeps_its_original_point_across_other_handles() {
        let ctx = egui::Context::default();
        let mut points = vec![[0., 0.], [0.25, 0.25], [0.75, 0.75], [1., 1.]];
        let rect = frame(&ctx, &mut points, 0., vec![]);
        let at = |x: f32, y: f32| {
            Pos2::new(
                rect.left() + rect.width() * x,
                rect.bottom() - rect.height() * y,
            )
        };
        let start = at(0.25, 0.25);
        frame(
            &ctx,
            &mut points,
            0.1,
            vec![egui::Event::PointerMoved(start), button(start, true)],
        );
        frame(
            &ctx,
            &mut points,
            0.2,
            vec![egui::Event::PointerMoved(at(0.25, 0.35))],
        );
        assert!((points[1][1] - 0.35).abs() < 1e-5);
        let end = at(0.76, 0.6);
        frame(&ctx, &mut points, 0.3, vec![egui::Event::PointerMoved(end)]);
        assert!(
            (points[1][1] - 0.6).abs() < 1e-5,
            "Drag changed a different point: {points:?}"
        );
        assert_eq!(points[2], [0.75, 0.75]);
        frame(&ctx, &mut points, 0.4, vec![button(end, false)]);
        let second = at(0.75, 0.75);
        frame(
            &ctx,
            &mut points,
            0.5,
            vec![egui::Event::PointerMoved(second), button(second, true)],
        );
        frame(
            &ctx,
            &mut points,
            0.6,
            vec![egui::Event::PointerMoved(at(0.75, 0.85))],
        );
        assert!((points[2][1] - 0.85).abs() < 1e-5);
        assert!((points[1][1] - 0.6).abs() < 1e-5);
        assert_eq!(points[0], [0., 0.]);
        assert_eq!(points[3], [1., 1.]);
    }
}

#[cfg(test)]
mod error_scope_tests {
    use super::*;
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
}
