use eframe::wgpu;
use rawpuppy::{
    display::hdr::{Frame, SurfaceChoice, Texture},
    pipeline::Rendered,
};

fn image() -> Rendered {
    Rendered {
        width: 4,
        height: 1,
        pixels: vec![
            [-0.125, 0.5, 2., 1.],
            [4., 2., 1., 1.],
            [2., 4., 8., 0.5],
            [1., 1., 1., 0.],
        ],
    }
}

#[test]
fn float_preview_retains_extended_rgb_and_premultiplies_before_filtering() {
    let source = image();
    let original = source.pixels.clone();
    let frame = Frame::from_rendered(&source, 1.).unwrap();
    assert_eq!(frame.size(), [4, 1]);
    assert_eq!(frame.pixels()[0].map(|v| v.to_f32()), [-0.125, 0.5, 2., 1.]);
    assert_eq!(frame.pixels()[1].map(|v| v.to_f32()), [4., 2., 1., 1.]);
    assert_eq!(frame.pixels()[2].map(|v| v.to_f32()), [1., 2., 4., 0.5]);
    assert_eq!(frame.pixels()[3].map(|v| v.to_f32()), [0.; 4]);
    assert_eq!(source.pixels, original);
    let mut invalid = image();
    invalid.pixels[0][0] = f32::NAN;
    assert!(Frame::from_rendered(&invalid, 1.).is_err());
    invalid.pixels.clear();
    assert!(Frame::from_rendered(&invalid, 1.).is_err());
    let scaled = Frame::from_rendered(&source, 2.).unwrap();
    assert_eq!(scaled.pixels()[0].map(|v| v.to_f32()), [-0.25, 1., 4., 1.]);
    assert_eq!(scaled.pixels()[2].map(|v| v.to_f32()), [2., 4., 8., 0.5]);
    assert!(Frame::from_rendered(&source, 0.).is_err());
}

#[test]
fn hdr_selection_requires_the_advertised_float_format_and_linear_color_space() {
    let mut caps = wgpu::SurfaceCapabilities::default();
    caps.formats.push(wgpu::TextureFormat::Rgba16Float);
    assert_eq!(SurfaceChoice::extended_linear(&caps), None);
    caps.format_capabilities
        .push(wgpu::SurfaceFormatCapabilities {
            format: wgpu::TextureFormat::Rgba16Float,
            color_spaces: wgpu::SurfaceColorSpaces::SRGB | wgpu::SurfaceColorSpaces::EXTENDED_SRGB,
        });
    assert_eq!(SurfaceChoice::extended_linear(&caps), None);
    caps.format_capabilities[0].color_spaces |= wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR;
    let choice = SurfaceChoice::extended_linear(&caps).unwrap();
    assert_eq!(choice.format, wgpu::TextureFormat::Rgba16Float);
    assert_eq!(
        choice.color_space,
        wgpu::SurfaceColorSpace::ExtendedSrgbLinear
    );
    assert!(choice.color_space.is_hdr());
    // Missing advisory luminance does not mean an SDR surface.
    assert_eq!(wgpu::DisplayHdrInfo::default().tone_map_headroom(), None);
}

#[test]
fn reference_white_uses_native_signal_units_without_guessing_unknown_windows_values() {
    use rawpuppy::display::{Desktop, hdr::reference_white_scale};
    let mut info = wgpu::DisplayHdrInfo::default();
    assert_eq!(reference_white_scale(Desktop::Mac, &info), 1.);
    assert_eq!(reference_white_scale(Desktop::Windows(1), &info), 1.);
    assert_eq!(reference_white_scale(Desktop::Wayland, &info), 203. / 80.);
    info.luminance = Some(wgpu::DisplayLuminance {
        sdr_white_nits: Some(160.),
        ..Default::default()
    });
    assert_eq!(reference_white_scale(Desktop::Windows(1), &info), 2.);
    assert_eq!(reference_white_scale(Desktop::Mac, &info), 1.);
    info.luminance.as_mut().unwrap().sdr_white_nits = Some(f32::NAN);
    assert_eq!(reference_white_scale(Desktop::Windows(1), &info), 1.);
}

#[test]
#[ignore = "requires a Vulkan/Metal/DX12 presentation device"]
fn hdr_egui_white_and_coverage_match_the_linear_photo_signal() {
    use eframe::{egui, egui_wgpu};
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .unwrap();
        let scale = 203. / 80.;
        let photo = Rendered {
            width: 2,
            height: 1,
            pixels: vec![[1.; 4], [4., 4., 4., 1.]],
        };
        let texture = Texture::upload(
            &device,
            &queue,
            &Frame::from_rendered(&photo, scale).unwrap(),
        )
        .unwrap();
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(8., 8.),
                )),
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx();
                let painter = ctx.layer_painter(egui::LayerId::background());
                painter.rect_filled(
                    egui::Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(4., 4.)),
                    0.,
                    egui::Color32::WHITE,
                );
                painter.rect_filled(
                    egui::Rect::from_min_max(egui::pos2(0., 4.), egui::pos2(4., 8.)),
                    0.,
                    egui::Color32::from_white_alpha(128),
                );
                painter.add(texture.callback(egui::Rect::from_min_max(
                    egui::pos2(4., 0.),
                    egui::pos2(8., 8.),
                )));
            },
        );
        let jobs = ctx.tessellate(output.shapes, 1.);
        let mut renderer = egui_wgpu::Renderer::new_with_color_space(
            &device,
            wgpu::TextureFormat::Rgba16Float,
            wgpu::SurfaceColorSpace::ExtendedSrgbLinear,
            egui_wgpu::RendererOptions::default(),
        );
        renderer.set_hdr_white_scale(scale);
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                renderer.update_texture(&device, &queue, *id, delta);
            }
        }
        output.textures_delta.clear();
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("HDR GUI and photo"),
            size: wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256 * 8,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let commands = renderer.update_buffers(
            &device,
            &queue,
            &mut encoder,
            &jobs,
            &egui_wgpu::ScreenDescriptor {
                size_in_pixels: [8, 8],
                pixels_per_point: 1.,
            },
        );
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
            renderer.render(
                &mut pass,
                &jobs,
                &egui_wgpu::ScreenDescriptor {
                    size_in_pixels: [8, 8],
                    pixels_per_point: 1.,
                },
            );
        }
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(8),
                },
            },
            target.size(),
        );
        queue.submit(commands.into_iter().chain([encoder.finish()]));
        let (send, receive) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| send.send(r).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receive.recv().unwrap().unwrap();
        let bytes = buffer.slice(..).get_mapped_range().unwrap();
        let pixel = |x: usize, y: usize| {
            let values: &[half::f16] =
                bytemuck::cast_slice(&bytes[y * 256 + x * 8..y * 256 + x * 8 + 8]);
            values[0].to_f32()
        };
        assert!(
            (pixel(2, 2) - scale).abs() < 0.002,
            "GUI white {}",
            pixel(2, 2)
        );
        assert!(
            (pixel(4, 2) - pixel(2, 2)).abs() < 0.002,
            "photo/GUI white mismatch"
        );
        assert!(
            (pixel(7, 2) - 4. * scale).abs() < 0.01,
            "HDR photo peak {}",
            pixel(7, 2)
        );
        assert!(
            (pixel(2, 6) - scale * 128. / 255.).abs() < 0.003,
            "GUI alpha {}",
            pixel(2, 6)
        );
        drop(bytes);
        buffer.unmap();
    });
}

#[test]
#[ignore = "requires a Vulkan/Metal/DX12 presentation device"]
fn hdr_texture_paint_preserves_superwhite_negative_channels_and_linear_alpha() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .unwrap();
        let source = image();
        let photo =
            Texture::upload(&device, &queue, &Frame::from_rendered(&source, 1.).unwrap()).unwrap();
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("HDR readback"),
            size: wgpu::Extent3d {
                width: 4,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("HDR pixels"),
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.25,
                                g: 0.25,
                                b: 0.25,
                                a: 1.,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            photo.paint(&mut pass);
        }
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            target.size(),
        );
        queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap()
            });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receive.recv().unwrap().unwrap();
        let mapped = buffer.slice(..).get_mapped_range().unwrap();
        let pixels: &[[half::f16; 4]] = bytemuck::cast_slice(&mapped[..32]);
        let expected = [
            [-0.125, 0.5, 2., 1.],
            [4., 2., 1., 1.],
            [1.125, 2.125, 4.125, 1.],
            [0.25, 0.25, 0.25, 1.],
        ];
        for (actual, expected) in pixels.iter().zip(expected) {
            for (a, e) in actual.iter().zip(expected) {
                assert!((a.to_f32() - e).abs() < 0.002, "{actual:?} != {expected:?}");
            }
        }
        drop(mapped);
        buffer.unmap();
    });
}
