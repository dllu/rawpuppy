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

mod calibration;
mod canvas;
mod controls;
mod curve;
mod display_state;
mod document;
mod framing;
mod history;
#[cfg(test)]
mod tests;
mod toolbar;
mod worker;
#[cfg(test)]
use canvas::primary_stroke;
use canvas::{BrushStroke, Tool};
use curve::curve_editor;
use worker::{Busy, ErrorScope, PreviewPixels, Reply, Work, worker};
#[cfg(debug_assertions)]
mod probe;
use history::History;

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
    history: History,
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
    texture_region: [f32; 4],
    preview_size: [usize; 2],
    compare: bool,
    framing: framing::Framing,
    tool: Tool,
    clone_source: Option<[f32; 2]>,
    radius: f32,
    feather: f32,
    pending: Option<Pending>,
    close_after_save: bool,
    stroke_recorded: bool,
    brush_stroke: BrushStroke,
    brush_offset: [f32; 2],
    fit_scale: f32,
    mask: Vec<crate::synthesis::MaskDab>,
    ai_steps: usize,
    ai_seed: i64,
    perspective_status: Option<String>,
    perspective_mask: bool,
    calibration_box: Option<[[f32; 2]; 2]>,
    calibration_status: Option<String>,
}

impl Editor {
    pub(super) fn new(
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
            history: History::default(),
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
            texture_region: [0., 0., 1., 1.],
            preview_size: [1200, 900],
            compare: false,
            framing: framing::Framing::default(),
            tool: Tool::View,
            clone_source: None,
            radius: 0.025,
            feather: 0.6,
            pending: None,
            close_after_save: false,
            stroke_recorded: false,
            brush_stroke: BrushStroke::default(),
            brush_offset: [0.; 2],
            fit_scale: 1.,
            mask: vec![],
            ai_steps: 20,
            ai_seed: 0,
            perspective_status: None,
            perspective_mask: true,
            calibration_box: None,
            calibration_status: None,
        };
        if let Some(path) = input {
            app.open(path);
        }
        app
    }
    pub(super) fn dirty(&self) -> bool {
        self.edits != self.saved
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
            if ctx.input(|i| i.key_pressed(egui::Key::C)) {
                self.set_tool(if self.tool == Tool::Crop {
                    Tool::View
                } else {
                    Tool::Crop
                });
            }
            if ctx.input(|i| i.key_pressed(egui::Key::P)) {
                self.set_tool(if self.tool == Tool::Perspective {
                    Tool::View
                } else {
                    Tool::Perspective
                });
            }
            if matches!(self.tool, Tool::Crop | Tool::Perspective)
                && ctx
                    .input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape))
            {
                self.set_tool(Tool::View);
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
                            self.cancel_pending();
                        }
                    });
                });
        }
    }
}
