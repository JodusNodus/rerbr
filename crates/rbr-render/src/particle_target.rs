//! The soft particles at half the frame's size (renderer addition, `Enhance::particles`): on a frame of
//! [`REDUCE_FROM`] lines or more, the lit soft billboards (`fs_soft`) are drawn into a target of half its width and
//! height and laid over the frame (`particle_target.wgsl`). They are fill-bound: a puff of spray or dust near the eye
//! covers much of a 4K frame, many deep. Their light and the frame's transmittance through them add up as drawing them
//! over the frame does, so the order of the draws before and after them holds. In rain the mist's layer is drawn before
//! the temporal resolve, which lays it over the frame it writes (`fs_taa_mist`): laid after it, it cost a pass over the
//! whole frame (0.45 ms at 4K).

use crate::effects::Effects;
use crate::particles::gpu::Pass;
use crate::post::SCENE_FORMAT;
use crate::renderer::{depth_binding, DEPTH_FORMAT};
use glam::{Mat4, Vec3};

/// The frame's height from which the particles are drawn at half its size: 4K and up, where the half is still about
/// 1080 lines. A smaller frame draws them at its own size, as before.
pub(crate) const REDUCE_FROM: u32 = 1800;

pub(crate) const SHADER: &str = concat!(include_str!("particle_target.wgsl"), include_str!("particle_layer.wgsl"));

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

/// A frame's pixels `[x, y, width, height]`.
pub(crate) type Rect = [u32; 4];

/// The pixels of a frame of `size` that billboards with `corners` (stage space, four each) cover in
/// `view_projection`, a few more about them; the whole frame when one reaches behind the eye, `None` when none is in
/// view. Only those pixels are reduced, drawn and laid over: a few puffs far off cost little.
pub(crate) fn cover(corners: &[Vec3], view_projection: Mat4, size: (u32, u32)) -> Option<Rect> {
    const MARGIN: f32 = 4.0;
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for &corner in corners {
        let clip = view_projection * corner.extend(1.0);
        if clip.w <= 1e-3 {
            return Some([0, 0, size.0, size.1]);
        }
        let (x, y) = (clip.x / clip.w, clip.y / clip.w);
        lo = [lo[0].min(x), lo[1].min(-y)];
        hi = [hi[0].max(x), hi[1].max(-y)];
    }
    let pixels = |ndc: f32, side: u32, margin: f32| ((ndc * 0.5 + 0.5) * side as f32 + margin).clamp(0.0, side as f32);
    let (x0, x1) = (pixels(lo[0], size.0, -MARGIN).floor(), pixels(hi[0], size.0, MARGIN).ceil());
    let (y0, y1) = (pixels(lo[1], size.1, -MARGIN).floor(), pixels(hi[1], size.1, MARGIN).ceil());
    (x1 > x0 && y1 > y0).then_some([x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32])
}

/// `rect` of the frame in the reduced target's pixels, a texel more about it (the composite's taps).
fn halved([x, y, w, h]: Rect, half: (u32, u32)) -> Rect {
    let (x0, y0) = ((x / 2).saturating_sub(1), (y / 2).saturating_sub(1));
    let (x1, y1) = (((x + w).div_ceil(2) + 1).min(half.0), ((y + h).div_ceil(2) + 1).min(half.1));
    [x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0)]
}

/// A pass over `scene` that reads the frame's `depth`, as the rain and the particles draw in.
pub(crate) fn effects_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    scene: &wgpu::TextureView,
    depth: &wgpu::TextureView,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("effects over the resolve"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: scene,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: None,
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

fn scissor(pass: &mut wgpu::RenderPass<'_>, [x, y, w, h]: Rect) {
    pass.set_scissor_rect(x, y, w, h);
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

    /// Its own size.
    fn half(&self) -> (u32, u32) {
        reduced(self.size).unwrap_or(self.size)
    }

    /// Draws the rain and the particles over `scene` (with the frame's `depth`, in `view_projection`): the mist
    /// and the soft billboards (with the lamps' code when `lamps`) here, each laid over in its turn, the streaks and
    /// the waves at the frame's size between them. `false` when there is neither (nothing drawn: the caller draws
    /// the rest as before).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw(
        &self,
        (queue, encoder): (&wgpu::Queue, &mut wgpu::CommandEncoder),
        [scene, depth]: [&wgpu::TextureView; 2],
        effects: &Effects,
        particles: Option<&Pass>,
        lamps: bool,
        view_projection: Mat4,
        mist_laid: bool,
    ) -> bool {
        let mist = effects.rain() > 0.0;
        // Only the pixels the billboards cover; the mist covers the whole frame.
        let whole = [0, 0, self.size.0, self.size.1];
        let billboards =
            particles.filter(|p| p.soft_billboards()).and_then(|p| cover(p.corners(), view_projection, self.size));
        let Some(rect) = (if mist { Some(whole) } else { billboards }) else {
            return false;
        };
        if !mist_laid {
            effects.write_reduced(queue, self.half());
            self.reduce_depth(encoder, rect);
            if mist {
                self.fill(encoder, whole, |pass| effects.draw_mist(pass, true, &self.effects));
                self.lay(encoder, scene, whole);
            }
        }
        let mut pass = effects_pass(encoder, scene, depth);
        effects.draw_drops(&mut pass);
        particles.inspect(|p| p.draw_waves(&mut pass));
        drop(pass);
        if let (Some(rect), Some(particles)) = (billboards, particles) {
            self.fill(encoder, rect, |pass| particles.draw_soft_reduced(pass, &self.effects, lamps));
            self.lay(encoder, scene, rect);
        }
        true
    }

    /// Before the temporal resolve, in rain: the depth reduced and the mist's layer drawn, which the resolve lays over
    /// the frame ([`ParticleTarget::layer_views`]); then [`ParticleTarget::draw`] with `mist_laid`.
    pub(crate) fn draw_mist(&self, (queue, encoder): (&wgpu::Queue, &mut wgpu::CommandEncoder), effects: &Effects) {
        let whole = [0, 0, self.size.0, self.size.1];
        effects.write_reduced(queue, self.half());
        self.reduce_depth(encoder, whole);
        self.fill(encoder, whole, |pass| effects.draw_mist(pass, true, &self.effects));
    }

    /// The reduced depth and the layer's colour, for the resolve's bind group (`fs_taa_mist`).
    pub(crate) fn layer_views(&self) -> [&wgpu::TextureView; 2] {
        [&self.depth, &self.colour]
    }

    /// Reduces the frame's depth into the target's within `rect` (of the frame), for the [`ParticleTarget::layer`]s
    /// after it.
    fn reduce_depth(&self, encoder: &mut wgpu::CommandEncoder, rect: Rect) {
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
        scissor(&mut pass, halved(rect, self.half()));
        pass.set_pipeline(&self.reduce);
        pass.set_bind_group(0, &self.full, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Opens the reduced pass for `draw` (with [`ParticleTarget::effects`] and pipelines that keep the transmittance
    /// in alpha) within `rect` of the frame (which the depth was reduced over).
    fn fill(&self, encoder: &mut wgpu::CommandEncoder, rect: Rect, draw: impl FnOnce(&mut wgpu::RenderPass<'_>)) {
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
        scissor(&mut pass, halved(rect, self.half()));
        draw(&mut pass);
    }

    /// Lays the layer [`ParticleTarget::fill`] drew over `scene` within `rect`, as drawing it there would.
    fn lay(&self, encoder: &mut wgpu::CommandEncoder, scene: &wgpu::TextureView, rect: Rect) {
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
        scissor(&mut pass, rect);
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

    #[test]
    fn the_cover_holds_the_billboards_and_a_margin() {
        let size = (3840, 2160);
        let quad =
            |x: f32, y: f32| [-1.0, 1.0].map(|dx| [-1.0, 1.0].map(|dy| Vec3::new(x + dx * 0.1, y + dy * 0.1, 0.5)));
        let corners: Vec<Vec3> = quad(0.0, 0.0).into_iter().flatten().collect();
        // Clip space is the stage's here: x and y -0.1 to 0.1 of NDC.
        assert_eq!(cover(&corners, Mat4::IDENTITY, size), Some([1724, 968, 392, 224]));
        assert_eq!(cover(&[], Mat4::IDENTITY, size), None);
        let behind = [Vec3::ZERO];
        let flip =
            Mat4::from_cols_array(&[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.0]);
        assert_eq!(cover(&behind, flip, size), Some([0, 0, 3840, 2160]));
        let off = quad(5.0, 0.0).into_iter().flatten().collect::<Vec<_>>();
        assert_eq!(cover(&off, Mat4::IDENTITY, size), None);
        assert_eq!(halved([1724, 968, 392, 224], (1920, 1080)), [861, 483, 198, 114]);
        assert_eq!(halved([0, 0, 3840, 2160], (1920, 1080)), [0, 0, 1920, 1080]);
    }
}
