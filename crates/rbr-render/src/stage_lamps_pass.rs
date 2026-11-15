//! The stage's lamps' sprites drawn as light (`stage_lamps.wgsl`, ours): each sprite texture's triangles added to the
//! frame, tested against the stage's depth without writing it, as bright as the lamps are lit. Drawn inside the stage
//! pass after the marks (`renderer::Renderer::draw_scene`).

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;

use crate::renderer::DEPTH_FORMAT;
use crate::stage_lamps::Sprite;

const SHADER: &str = include_str!("stage_lamps.wgsl");

/// The sprites' view (`View` in `stage_lamps.wgsl`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    view_projection: [[f32; 4]; 4],
    eye: [f32; 4],
    colour: [f32; 4],
    fog: [f32; 4],
}

/// The frame's look of the sprites ([`GlowPass::draw`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Glow {
    /// The lamps' colour times how lit they are; nothing is drawn when it is black.
    pub colour: Vec3,
    /// The renderer's units per cd/m2.
    pub pre_exposure: f32,
    /// The stage's linear fog's start and end, metres, when it is on.
    pub fog: Option<[f32; 2]>,
}

struct Drawn {
    texture: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    count: u32,
}

/// The pipeline, the view uniform and each sprite texture's triangles.
pub(crate) struct GlowPass {
    pipeline: wgpu::RenderPipeline,
    view: wgpu::Buffer,
    frame: wgpu::BindGroup,
    sprites: Vec<Drawn>,
}

impl GlowPass {
    /// The pass for `sprites`, for colour targets of `format`.
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        sprites: &[Sprite],
    ) -> GlowPass {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stage lamps"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let view = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stage lamps view"),
            size: size_of::<ViewUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stage lamps frame"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stage lamps texture"),
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stage lamps"),
            bind_group_layouts: &[Some(&frame_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        const ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let keep = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stage lamps"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_sprite"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 5 * 4,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &ATTRIBUTES,
                })],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_sprite"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState { color: add, alpha: keep }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let frame = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stage lamps frame"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: view.as_entire_binding() }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("stage lamps"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let sprites = sprites
            .iter()
            .filter(|sprite| !sprite.corners.is_empty())
            .map(|sprite| {
                let image = mipmapped(device, queue, sprite).create_view(&Default::default());
                let texture = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("stage lamps texture"),
                    layout: &texture_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&image) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                    ],
                });
                let floats: Vec<f32> =
                    sprite.corners.iter().flat_map(|(p, uv)| [p[0], p[1], p[2], uv[0], uv[1]]).collect();
                let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("stage lamps"),
                    contents: bytemuck::cast_slice(&floats),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                Drawn { texture, vertices, count: sprite.corners.len() as u32 }
            })
            .collect();
        GlowPass { pipeline, view, frame, sprites }
    }

    /// Draws the sprites with `view_projection` (stage coordinates) as `glow` has them.
    pub(crate) fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        queue: &wgpu::Queue,
        view_projection: Mat4,
        glow: Glow,
    ) {
        if glow.colour == Vec3::ZERO || self.sprites.is_empty() {
            return;
        }
        let [start, end] = glow.fog.unwrap_or_default();
        let view = ViewUniform {
            view_projection: view_projection.to_cols_array_2d(),
            eye: crate::camera::eye(view_projection).extend(glow.pre_exposure).to_array(),
            colour: glow.colour.extend(1.0).to_array(),
            fog: Vec4::new(start, end, f32::from(u8::from(glow.fog.is_some())), 0.0).to_array(),
        };
        queue.write_buffer(&self.view, 0, bytemuck::bytes_of(&view));
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.frame, &[]);
        for sprite in &self.sprites {
            pass.set_bind_group(1, &sprite.texture, &[]);
            pass.set_vertex_buffer(0, sprite.vertices.slice(..));
            pass.draw(0..sprite.count, 0..1);
        }
    }
}

/// The sprite's texture with its mip chain (box-filtered), so that far sprites do not shimmer.
fn mipmapped(device: &wgpu::Device, queue: &wgpu::Queue, sprite: &Sprite) -> wgpu::Texture {
    let mut levels = vec![(sprite.width, sprite.height, sprite.rgba.clone())];
    while let Some((w, h, texels)) = levels.last().filter(|(w, h, _)| *w > 1 || *h > 1) {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let at = |x: u32, y: u32, c: usize| u32::from(texels[((y.min(h - 1) * w + x.min(w - 1)) * 4) as usize + c]);
        let next = (0..nh)
            .flat_map(|y| (0..nw).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                (0..4).map(move |c| {
                    ((at(2 * x, 2 * y, c)
                        + at(2 * x + 1, 2 * y, c)
                        + at(2 * x, 2 * y + 1, c)
                        + at(2 * x + 1, 2 * y + 1, c))
                        / 4) as u8
                })
            })
            .collect();
        levels.push((nw, nh, next));
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("stage lamp sprite"),
        size: wgpu::Extent3d { width: sprite.width, height: sprite.height, depth_or_array_layers: 1 },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, (w, h, texels)) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            texels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(*h) },
            wgpu::Extent3d { width: *w, height: *h, depth_or_array_layers: 1 },
        );
    }
    texture
}
