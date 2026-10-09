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
