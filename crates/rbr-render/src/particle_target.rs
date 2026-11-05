//! The soft particles at half the frame's size (renderer addition, `Enhance::particles`): on a frame of
//! [`REDUCE_FROM`] lines or more, the lit soft billboards (`fs_soft`) are drawn into a target of half its width and
//! height and laid over the frame (`particle_target.wgsl`). They are fill-bound: a puff of spray or dust near the eye
//! covers much of a 4K frame, many deep. Their light and the frame's transmittance through them add up as drawing them
//! over the frame does, so the order of the draws before and after them holds.

use crate::post::SCENE_FORMAT;
use crate::renderer::{depth_binding, DEPTH_FORMAT};

/// The frame's height from which the particles are drawn at half its size: 4K and up, where the half is still about
/// 1080 lines. A smaller frame draws them at its own size, as before.
pub(crate) const REDUCE_FROM: u32 = 1800;

pub(crate) const SHADER: &str = include_str!("particle_target.wgsl");

/// What the frame's transmittance through the particles starts as: no particle, the frame unchanged.
const CLEAR: wgpu::Color = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };

/// The reduced target, its depth, and the passes that fill and lay them.
pub(crate) struct ParticleTarget {
    size: (u32, u32),
    colour: wgpu::TextureView,
    depth: wgpu::TextureView,
    /// The effects' bind group over the reduced depth (`Effects::reduced_group`).
    effects: wgpu::BindGroup,
    group: wgpu::BindGroup,
    /// The full-size depth alone, for the pass that writes the reduced one.
    full: wgpu::BindGroup,
    reduce: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
}

/// The size of the particles' target for a frame of `size`, `None` when they are drawn at the frame's own.
pub(crate) fn reduced(size: (u32, u32)) -> Option<(u32, u32)> {
    (size.1 >= REDUCE_FROM).then(|| (size.0.div_ceil(2), size.1.div_ceil(2)))
}

impl ParticleTarget {
    /// For the frame's `depth` of `size` (one [`reduced`] takes); `effects` makes the effects' bind group over the
    /// reduced depth.
    pub(crate) fn new(
        device: &wgpu::Device,
        size: (u32, u32),
        depth: &wgpu::TextureView,
        effects: impl FnOnce(&wgpu::TextureView) -> wgpu::BindGroup,
    ) -> Option<ParticleTarget> {
        let half = reduced(size)?;
        let texture = |label, format, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: half.0, height: half.1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: usage | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let colour = texture("particles' target", SCENE_FORMAT, attachment);
        let reduced_depth = texture("particles' depth", DEPTH_FORMAT, attachment);
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty,
            count: None,
        };
        let float = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particles' target"),
            entries: &[entry(0, depth_binding()), entry(1, depth_binding()), entry(2, float)],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particles' target"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(depth) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&reduced_depth) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&colour) },
            ],
        });
        let full_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particles' depth"),
            entries: &[entry(0, depth_binding())],
        });
        let full = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particles' depth"),
            layout: &full_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(depth) }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("particles' target"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = |label, layout, fragment, target: Option<Option<wgpu::ColorTargetState>>, depth| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_full"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: target.as_slice(),
                }),
                primitive: Default::default(),
                depth_stencil: depth,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let reduce = pipeline(
            "particles' depth",
            &full_layout,
            "fs_depth",
            None,
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
        );
        // The frame times the transmittance plus the particles' light; its alpha is left (nothing after reads it).
        let over = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::SrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let composite = pipeline(
            "particles' composite",
            &layout,
            "fs_composite",
            Some(Some(wgpu::ColorTargetState {
                format: SCENE_FORMAT,
                blend: Some(wgpu::BlendState { color: over, alpha: wgpu::BlendComponent::REPLACE }),
                write_mask: wgpu::ColorWrites::COLOR,
            })),
            None,
        );
        Some(ParticleTarget {
            size,
            effects: effects(&reduced_depth),
            colour,
            depth: reduced_depth,
            group,
            full,
            reduce,
            composite,
        })
    }

    /// The frame's size it was made for.
    pub(crate) fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The effects' bind group over the reduced depth.
    pub(crate) fn effects(&self) -> &wgpu::BindGroup {
        &self.effects
    }

    /// Reduces the frame's depth into the target's, for the [`ParticleTarget::layer`]s after it.
    pub(crate) fn reduce_depth(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("particles' depth"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.reduce);
        pass.set_bind_group(0, &self.full, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Opens the reduced pass for `draw` (with [`ParticleTarget::effects`] and pipelines that keep the transmittance
    /// in alpha) and lays its result over `scene`, as drawing it there would.
    pub(crate) fn layer(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &wgpu::TextureView,
        draw: impl FnOnce(&mut wgpu::RenderPass<'_>),
    ) {
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("particles at half size"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.colour,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(CLEAR), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: None,
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            draw(&mut pass);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("particles' composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: scene,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.composite);
        pass.set_bind_group(0, &self.group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_tall_frames_are_reduced() {
        assert_eq!(reduced((1920, 1080)), None);
        assert_eq!(reduced((2560, 1440)), None);
        assert_eq!(reduced((3840, 2160)), Some((1920, 1080)));
        assert_eq!(reduced((3841, 2161)), Some((1921, 1081)));
    }
}
