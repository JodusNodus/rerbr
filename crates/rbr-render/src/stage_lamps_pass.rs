//! The stage's lamps' bulbs (`stage_lamps.wgsl`, ours): a soft disc facing the eye at each lamp, added to the frame,
//! tested against the stage's depth without writing it, as bright as the lamps are lit. Drawn inside the stage pass
//! after the marks (`renderer::Renderer::draw_scene`).

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;

use crate::renderer::DEPTH_FORMAT;
use crate::stage_lamps::StageLamp;

const SHADER: &str = include_str!("stage_lamps.wgsl");

/// A street lamp's bulb's radius, metres (ours: the bright part of a bowl about 0.4 m across); a smaller lamp's as much smaller as its
/// sprite ([`crate::stage_lamps::scale`]).
const BULB_RADIUS: f32 = 0.15;
/// How far down its axis from the sprite's peak the bulb sits, in bulb radii: the peak is the housing's edge.
const BULB_DROP: f32 = 1.0;

/// The bulbs' view (`View` in `stage_lamps.wgsl`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    view_projection: [[f32; 4]; 4],
    eye: [f32; 4],
    colour: [f32; 4],
    fog: [f32; 4],
    screen: [f32; 4],
}

/// The frame's look of the bulbs ([`BulbPass::draw`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Glow {
    /// The lamps' colour times how lit they are; nothing is drawn when it is black.
    pub colour: Vec3,
    /// The renderer's units per cd/m2.
    pub pre_exposure: f32,
    /// The stage's linear fog's start and end, metres, when it is on.
    pub fog: Option<[f32; 2]>,
}

/// The pipeline, the view uniform and one bulb per lamp.
pub(crate) struct BulbPass {
    pipeline: wgpu::RenderPipeline,
    view: wgpu::Buffer,
    frame: wgpu::BindGroup,
    bulbs: wgpu::Buffer,
    count: u32,
}

impl BulbPass {
    /// The pass for `lamps`, for colour targets of `format`.
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat, lamps: &[StageLamp]) -> BulbPass {
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stage lamps"),
            bind_group_layouts: &[Some(&frame_layout)],
            immediate_size: 0,
        });
        const ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x4];
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
                entry_point: Some("vs_bulb"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 4 * 4,
                    step_mode: wgpu::VertexStepMode::Instance,
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
                entry_point: Some("fs_bulb"),
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
        let floats: Vec<[f32; 4]> = lamps
            .iter()
            .map(|lamp| {
                let radius = BULB_RADIUS * crate::stage_lamps::scale(lamp);
                (lamp.position + lamp.axis * radius * BULB_DROP).extend(radius).to_array()
            })
            .collect();
        let bulbs = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("stage lamps"),
            contents: bytemuck::cast_slice(&floats),
            usage: wgpu::BufferUsages::VERTEX,
        });
        BulbPass { pipeline, view, frame, bulbs, count: lamps.len() as u32 }
    }

    /// Draws the bulbs with `view_projection` (stage coordinates) into a target `height` pixels tall, as `glow` has
    /// them.
    pub(crate) fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        queue: &wgpu::Queue,
        view_projection: Mat4,
        height: u32,
        glow: Glow,
    ) {
        if glow.colour == Vec3::ZERO || self.count == 0 {
            return;
        }
        let [start, end] = glow.fog.unwrap_or_default();
        let view = ViewUniform {
            view_projection: view_projection.to_cols_array_2d(),
            eye: crate::camera::eye(view_projection).extend(glow.pre_exposure).to_array(),
            colour: glow.colour.extend(1.0).to_array(),
            fog: Vec4::new(start, end, f32::from(u8::from(glow.fog.is_some())), 0.0).to_array(),
            screen: [height.max(1) as f32, 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.view, 0, bytemuck::bytes_of(&view));
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.frame, &[]);
        pass.set_vertex_buffer(0, self.bulbs.slice(..));
        pass.draw(0..6, 0..self.count);
    }
}

#[cfg(test)]
crate::shader_check::wgsl_layout!(
    the_stage_lamps_view_keeps_the_shaders_layout,
    SHADER,
    "View",
    ViewUniform { view_projection, eye, colour, fog, screen }
);
