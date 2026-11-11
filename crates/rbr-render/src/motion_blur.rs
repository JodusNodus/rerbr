// Motion blur along each pixel's screen motion (ours, experimental), an opt-in pass (`Enhance::motion_blur`,
// `--enhance blur`) that runs only with the temporal anti-aliasing: the resolve writes into [`MotionBlur::target`]
// instead of the post pass's scene, and this pass gathers the scene along each pixel's blur vector into it. The blur
// vector is the pixel's velocity in uv per frame (the motion target's where something moving was drawn, else the
// point from the depth carried by the resolve's `view_to_prev`, as `previous_uv` in `taa.wgsl` finds it) times
// [`SHUTTER`] over the frame time, centred on the pixel and capped at [`MOST`] of the output height. Eleven taps
// along it, offset by the pixel's interleaved gradient noise advanced each frame, weighted so a pixel that does not
// move neither blurs nor bleeds into the moving road beside it.

use bytemuck::{Pod, Zeroable};

use crate::post::SCENE_FORMAT;
use crate::renderer::{depth_binding, texture_entry, TEXTURE};

pub(crate) const SHADER: &str = include_str!("motion_blur.wgsl");
/// The exposure time each frame's motion blurs over, seconds.
pub(crate) const SHUTTER: f32 = 1.0 / 120.0;
/// Past this frame time the blur has no strength, seconds.
const LONGEST_DT: f32 = 0.1;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    /// The velocity's scale into the blur vector ([`SHUTTER`] over the frame time), 0 without strength.
    scale: f32,
    /// The frame's number, for the gather's noise.
    frame: f32,
    pad: [f32; 2],
}

pub(crate) struct MotionBlur {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    /// The resolve's target while the blur is on (the output's size), which the blur gathers into the post pass's scene.
    target: wgpu::TextureView,
    group: wgpu::BindGroup,
    /// The last main-view frame's time ([`Renderer::set_time`](crate::renderer::Renderer::set_time)); `None` before
    /// the first.
    previous: Option<f32>,
    /// Frames drawn (the gather's noise advances each frame).
    frame: u32,
}

impl MotionBlur {
    /// The blur over the resolve's output of `size` (the output's), reading the temporal anti-aliasing's `uniform`
    /// (its `view_to_prev` and sizes), the frame's `motion` and `depth` (the render's size).
    pub(crate) fn new(
        device: &wgpu::Device,
        size: (u32, u32),
        uniform: &wgpu::Buffer,
        motion: &wgpu::TextureView,
        depth: &wgpu::TextureView,
    ) -> MotionBlur {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("motion blur"),
            entries: &[
                texture_entry(
                    0,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                texture_entry(1, TEXTURE),
                texture_entry(2, depth_binding()),
                texture_entry(3, TEXTURE),
                texture_entry(4, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                texture_entry(
                    5,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("motion blur"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("motion blur"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("motion blur"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_full"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_blur"),
                compilation_options: Default::default(),
                targets: &[Some(SCENE_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("motion blur"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("motion blur"),
            size: size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let target = target(device, size);
        let group = group(device, &layout, &sampler, &params, uniform, &target, motion, depth);
        MotionBlur { layout, pipeline, sampler, params, target, group, previous: None, frame: 0 }
    }

    /// The resolve's target while the blur is on: the temporal anti-aliasing writes into it instead of the post
    /// pass's scene.
    pub(crate) fn target(&self) -> &wgpu::TextureView {
        &self.target
    }

    /// For resolved frames of `size` (the output's, as the post pass's scene), reading the temporal anti-aliasing's
    /// `uniform` and the frame's `motion` and `depth`.
    pub(crate) fn resize(
        &mut self,
        device: &wgpu::Device,
        size: (u32, u32),
        uniform: &wgpu::Buffer,
        motion: &wgpu::TextureView,
        depth: &wgpu::TextureView,
    ) {
        self.target = target(device, size);
        self.group = group(device, &self.layout, &self.sampler, &self.params, uniform, &self.target, motion, depth);
        // A new target must not carry the old frame's motion: the strength restarts with no previous frame.
        self.previous = None;
    }

    /// Gathers [`MotionBlur::target`] along each pixel's blur vector into `scene` (the post pass's): `time` is the
    /// frame's ([`Renderer::set_time`](crate::renderer::Renderer::set_time)), worked out on the frame's first view
    /// (`first`); the frame's other views (a triple screen's) reuse the same strength and noise, so every view blurs
    /// alike. On a frame whose history reset (a camera cut: `params.z` in the resolve's uniform) the shader takes
    /// the strength as 0.
    pub(crate) fn draw(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        scene: &wgpu::TextureView,
        time: f32,
        first: bool,
    ) {
        if first {
            let dt = self.previous.map_or(0.0, |previous| time - previous);
            self.previous = Some(time);
            self.frame = self.frame.wrapping_add(1);
            let scale = match !(0.0 < dt && dt <= LONGEST_DT) {
                true => 0.0,
                false => SHUTTER / dt,
            };
            queue.write_buffer(
                &self.params,
                0,
                bytemuck::bytes_of(&Params { scale, frame: self.frame as f32, pad: [0.0; 2] }),
            );
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("motion blur"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: scene,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// The resolve's target of `size` ([`SCENE_FORMAT`]: the post pass's scene's format).
fn target(device: &wgpu::Device, (width, height): (u32, u32)) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("motion blur scene"),
            size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SCENE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// The bind group reading the temporal anti-aliasing's `uniform`, the resolve's `target` (as the scene), the frame's
/// `motion` and `depth`, and the blur's own `params`.
#[allow(clippy::too_many_arguments)]
fn group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    params: &wgpu::Buffer,
    uniform: &wgpu::Buffer,
    target: &wgpu::TextureView,
    motion: &wgpu::TextureView,
    depth: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("motion blur"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(target) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(depth) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(motion) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 5, resource: params.as_entire_binding() },
        ],
    })
}
