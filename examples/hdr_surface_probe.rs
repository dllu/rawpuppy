//! Query a real native surface and present a linear float pattern when advertised.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use eframe::wgpu;
use rawpuppy::{
    display::hdr::{Frame, SurfaceChoice, Texture},
    pipeline::Rendered,
};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    require_hdr: bool,
}
struct Native {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    texture: Arc<Texture>,
}
struct App {
    native: Option<Native>,
    report: Option<serde_json::Value>,
    failure: Option<String>,
    frames: usize,
    started: Instant,
}
impl App {
    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("Rawpuppy HDR surface probe")
                    .with_inner_size(winit::dpi::PhysicalSize::new(132, 64)),
            )?,
        );
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
        let caps = surface.get_capabilities(&adapter);
        let hdr_info = surface.display_hdr_info(&adapter);
        let formats: Vec<_> = caps
            .format_capabilities
            .iter()
            .map(|f| {
                serde_json::json!({
                    "format": format!("{:?}", f.format),
                    "color_spaces": format!("{:?}", f.color_spaces),
                })
            })
            .collect();
        let choice = SurfaceChoice::extended_linear(&caps);
        self.report = Some(serde_json::json!({
            "adapter": format!("{:?}", adapter.get_info()),
            "format_capabilities": formats,
            "display_hdr_info": format!("{hdr_info:?}"),
            "tone_map_headroom": hdr_info.tone_map_headroom(),
            "extended_linear_surface_available": choice.is_some(),
            "native_frames_presented": 0,
            "scope": "surface capability and signal presentation, not physical-monitor colorimetry or editor integration",
        }));
        let Some(choice) = choice else {
            event_loop.exit();
            return Ok(());
        };
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: choice.format,
            color_space: choice.color_space,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        let image = Rendered {
            width: 4,
            height: 1,
            pixels: vec![
                [-0.125, 0.5, 2., 1.],
                [4., 2., 1., 1.],
                [0.18, 0.18, 0.18, 1.],
                [0.5, 0.5, 0.5, 1.],
            ],
        };
        let texture = Texture::upload(&device, &queue, &Frame::from_rendered(&image, 1.)?)?;
        self.report.as_mut().unwrap()["selected"] = serde_json::json!({
            "format": format!("{:?}", choice.format),
            "color_space": format!("{:?}", choice.color_space),
            "source_pixels": image.pixels,
        });
        window.request_redraw();
        self.native = Some(Native {
            window,
            surface,
            device,
            queue,
            config,
            texture,
        });
        Ok(())
    }
    fn render(&mut self) -> Result<()> {
        let native = self.native.as_ref().context("Missing HDR surface")?;
        let frame = match native.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated => {
                native.surface.configure(&native.device, &native.config);
                return Ok(());
            }
            other => anyhow::bail!("HDR surface acquisition failed: {other:?}"),
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = native.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            native.texture.paint(&mut pass);
        }
        native.queue.submit([encoder.finish()]);
        native.window.pre_present_notify();
        native.queue.present(frame);
        self.frames += 1;
        self.report.as_mut().unwrap()["native_frames_presented"] = self.frames.into();
        Ok(())
    }
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.failure = Some(format!("{error:#}"));
        event_loop.exit();
    }
}
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.report.is_none()
            && let Err(error) = self.initialize(event_loop)
        {
            self.fail(event_loop, error);
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if self.native.is_none() {
            return;
        }
        match event {
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.render() {
                    self.fail(event_loop, error);
                } else if self.frames >= 3 {
                    event_loop.exit();
                } else if let Some(native) = &self.native {
                    native.window.request_redraw();
                }
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(native) = &mut self.native {
                    native.config.width = size.width.max(1);
                    native.config.height = size.height.max(1);
                    native.surface.configure(&native.device, &native.config);
                }
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.started.elapsed() > Duration::from_secs(15) {
            self.fail(event_loop, anyhow::anyhow!("HDR surface probe timed out"));
        }
        event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(100),
        ));
    }
}
fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(path) = &args.output {
        ensure!(!path.exists(), "Choose a fresh result path");
    }
    let mut app = App {
        native: None,
        report: None,
        failure: None,
        frames: 0,
        started: Instant::now(),
    };
    EventLoop::new()?.run_app(&mut app)?;
    ensure!(
        app.failure.is_none(),
        "{}",
        app.failure.as_deref().unwrap_or_default()
    );
    let report = app.report.context("HDR probe did not inspect a surface")?;
    let result = serde_json::to_string_pretty(&report)? + "\n";
    println!("{result}");
    if let Some(path) = args.output {
        std::fs::write(path, result)?;
    }
    if args.require_hdr {
        ensure!(
            app.frames >= 3,
            "The native surface did not advertise and present extended-linear HDR"
        );
    }
    Ok(())
}
