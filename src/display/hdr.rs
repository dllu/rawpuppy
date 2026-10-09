//! Display-linear sRGB presentation for an explicitly extended-linear float target.
use crate::{input::pixel_count, pipeline::Rendered};
use anyhow::{Context, Result, ensure};
use eframe::{egui, egui_wgpu, wgpu};
use half::f16;
use std::sync::Arc;

/// Map relative display white to the explicitly selected extended-linear signal.
/// Windows scRGB uses 80-nit units; Apple EDR is relative to system SDR white.
/// The verified Vulkan Wayland WSI convention declares 80-nit units/203-nit white.
pub fn reference_white_scale(desktop: super::Desktop, info: &wgpu::DisplayHdrInfo) -> f32 {
    match desktop {
        super::Desktop::Wayland => 203. / 80.,
        super::Desktop::Windows(_) => info
            .luminance
            .and_then(|v| v.sdr_white_nits)
            .filter(|v| v.is_finite() && *v > 0.)
            .map_or(1., |v| v / 80.),
        _ => 1.,
    }
}

/// Texture format alone does not identify an HDR signal or its transfer function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceChoice {
    pub format: wgpu::TextureFormat,
    pub color_space: wgpu::SurfaceColorSpace,
}
impl SurfaceChoice {
    pub fn extended_linear(capabilities: &wgpu::SurfaceCapabilities) -> Option<Self> {
        capabilities
            .color_spaces(wgpu::TextureFormat::Rgba16Float)
            .contains(wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR)
            .then_some(Self {
                format: wgpu::TextureFormat::Rgba16Float,
                color_space: wgpu::SurfaceColorSpace::ExtendedSrgbLinear,
            })
    }
}

/// Premultiply before texture filtering. Preserve extended RGB within FP16's range.
pub struct Frame {
    width: u32,
    height: u32,
    pixels: Vec<[f16; 4]>,
}
impl Frame {
    /// `white_scale` maps the photo's relative white into the surface's signal units.
    /// Native scRGB luminance conventions vary by desktop; this is display policy,
    /// independent of edits and exported scene values.
    pub fn from_rendered(image: &Rendered, white_scale: f32) -> Result<Self> {
        ensure!(
            white_scale.is_finite() && white_scale > 0.,
            "Invalid HDR reference-white scale"
        );
        ensure!(image.width > 0 && image.height > 0, "Empty HDR preview");
        ensure!(
            image.pixels.len() == pixel_count(image.width, image.height, 1)?,
            "Invalid HDR preview raster"
        );
        ensure!(
            image.pixels.iter().flatten().all(|v| v.is_finite()),
            "Nonfinite HDR preview"
        );
        Ok(Self {
            width: image.width.try_into()?,
            height: image.height.try_into()?,
            pixels: image
                .pixels
                .iter()
                .map(|p| {
                    let alpha = p[3].clamp(0., 1.);
                    std::array::from_fn(|c| {
                        let value = if c == 3 {
                            alpha as f64
                        } else {
                            p[c] as f64 * alpha as f64 * white_scale as f64
                        };
                        let maximum = f16::MAX.to_f64();
                        f16::from_f64(value.clamp(-maximum, maximum))
                    })
                })
                .collect(),
        })
    }
    pub fn pixels(&self) -> &[[f16; 4]] {
        &self.pixels
    }
    pub fn size(&self) -> [u32; 2] {
        [self.width, self.height]
    }
}

/// One bounded preview texture and a linear premultiplied-alpha quad pipeline.
pub struct Texture {
    pipeline: wgpu::RenderPipeline,
    binding: wgpu::BindGroup,
    _texture: wgpu::Texture,
    pub size: [u32; 2],
}
impl Texture {
    pub fn upload(device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) -> Result<Arc<Self>> {
        let limit = device.limits().max_texture_dimension_2d;
        ensure!(
            frame.width <= limit && frame.height <= limit,
            "HDR preview exceeds the presentation device's texture dimensions"
        );
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Rawpuppy HDR preview"),
            size: wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            bytemuck::cast_slice(&frame.pixels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(
                    frame
                        .width
                        .checked_mul(8)
                        .context("HDR row size overflow")?,
                ),
                rows_per_image: Some(frame.height),
            },
            texture.size(),
        );
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("HDR preview binding"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("HDR preview pixels"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &texture.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("hdr.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("HDR preview pipeline"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("HDR preview quad"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Ok(Arc::new(Self {
            pipeline,
            binding,
            _texture: texture,
            size: [frame.width, frame.height],
        }))
    }
    /// The attachment must be Rgba16Float carrying extended-linear sRGB.
    pub fn paint(&self, pass: &mut wgpu::RenderPass<'static>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.binding, &[]);
        pass.draw(0..3, 0..1);
    }
    pub fn callback(self: &Arc<Self>, rect: egui::Rect) -> egui::PaintCallback {
        egui_wgpu::Callback::new_paint_callback(rect, Quad(self.clone()))
    }
}

struct Quad(Arc<Texture>);
impl egui_wgpu::CallbackTrait for Quad {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        _resources: &egui_wgpu::CallbackResources,
    ) {
        self.0.paint(pass);
    }
}
