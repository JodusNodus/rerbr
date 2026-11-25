//! Ground-truth ambient occlusion in place of the contact shading ([`Enhance::gtao`](super::Enhance::gtao); ours, a
//! proof of concept, `gtao.wgsl`): half the frame's size, from the depth the prepass laid, resolved over its own history
//! (reprojected by the previous frame's matrix, kept within the neighbours' range), into the texture the stage's
//! draws read as their contact shading ([`SunMask`](super::sun_mask::SunMask)): the visibility, and the view distance
//! it stands at for the stage's bilateral upsample.

use super::pipelines::{texture_entry, TEXTURE};
use crate::gpu_util::{self, UNIFORM};
use bytemuck::{bytes_of, Pod, Zeroable};
use glam::Mat4;
use std::cell::Cell;

pub(super) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;
/// The occlusion's radius in metres, and the power its visibility is raised to (`RERBR_GTAO_RADIUS`,
/// `RERBR_GTAO_POWER` replace them for tuning).
const RADIUS: f32 = 1.5;
const POWER: f32 = 1.5;
/// The frame numbers the noise cycles through.
const PHASES: u32 = 16;

/// `GtaoRun` in `gtao.wgsl`.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct Run {
    previous: [[f32; 4]; 4],
    run: [f32; 4],
}

pub(super) struct Gtao {
    raw_pipeline: wgpu::RenderPipeline,
    resolve_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    targets: Targets,
    /// The last frame's clip matrix, `None` before the first and after a resize.
    previous: Cell<Option<Mat4>>,
    frame: Cell<u32>,
    radius: f32,
    power: f32,
}

struct Targets {
    raw: wgpu::TextureView,
    /// What the stage reads, and its copy the next frame's resolve reads.
    contact: wgpu::Texture,
    contact_view: wgpu::TextureView,
    history: wgpu::Texture,
    /// The raw pass's (the history in the raw's place: a texture is not read where it is drawn) and the resolve's.
    groups: [wgpu::BindGroup; 2],
    size: (u32, u32),
}

impl Gtao {
    /// The passes over `frame_layout` and the sun mask's depth group `depth_layout`, at half of `size`.
    pub(super) fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        [frame_layout, depth_layout]: [&wgpu::BindGroupLayout; 2],
        size: (u32, u32),
    ) -> Gtao {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gtao"),
            entries: &[texture_entry(14, TEXTURE), texture_entry(15, TEXTURE), texture_entry(16, UNIFORM)],
        });
        let layouts = [frame_layout, &layout, depth_layout];
        let targets = [Some(FORMAT.into())];
        let pipeline =
            |entry| gpu_util::full_screen_pipeline(device, entry, shader, ("vs_sky", entry), &layouts, &targets, &[]);
        let uniform = gpu_util::uniform_buffer(device, "gtao", std::mem::size_of::<Run>());
        let env = |name: &str, default: f32| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default);
        Gtao {
            raw_pipeline: pipeline("fs_gtao"),
            resolve_pipeline: pipeline("fs_gtao_resolve"),
            targets: Targets::new(device, &layout, &uniform, size),
            layout,
            uniform,
            previous: Cell::new(None),
            frame: Cell::new(0),
            radius: env("RERBR_GTAO_RADIUS", RADIUS),
            power: env("RERBR_GTAO_POWER", POWER),
        }
    }

    pub(super) fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        self.targets = Targets::new(device, &self.layout, &self.uniform, size);
        self.previous.set(None);
    }

    /// The texture the stage's draws read.
    pub(super) fn contact(&self) -> &wgpu::TextureView {
        &self.targets.contact_view
    }

    /// Works the frame's occlusion out of the depth, as the contact shading is (`depth` the sun mask's group).
    pub(super) fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        [frame, depth]: [&wgpu::BindGroup; 2],
        view_projection: Mat4,
    ) {
        let previous = self.previous.replace(Some(view_projection));
        let n = self.frame.get();
        self.frame.set(n.wrapping_add(1));
        let run = Run {
            previous: previous.unwrap_or(view_projection).to_cols_array_2d(),
            run: [(n % PHASES + 1) as f32, self.radius, self.power, f32::from(u8::from(previous.is_none()))],
        };
        queue.write_buffer(&self.uniform, 0, bytes_of(&run));
        let targets = &self.targets;
        let groups = [[frame, &targets.groups[0], depth], [frame, &targets.groups[1], depth]];
        gpu_util::full_screen_pass(encoder, "gtao", &targets.raw, gpu_util::CLEAR, &self.raw_pipeline, &groups[0]);
        gpu_util::full_screen_pass(
            encoder,
            "gtao resolve",
            &targets.contact_view,
            gpu_util::CLEAR,
            &self.resolve_pipeline,
            &groups[1],
        );
        let (width, height) = targets.size;
        encoder.copy_texture_to_texture(
            targets.contact.as_image_copy(),
            targets.history.as_image_copy(),
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
    }
}

impl Targets {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, uniform: &wgpu::Buffer, size: (u32, u32)) -> Targets {
        let size = (size.0.div_ceil(2).max(1), size.1.div_ceil(2).max(1));
        let texture = |label, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage,
                view_formats: &[],
            })
        };
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let raw = texture("gtao raw", attachment).create_view(&Default::default());
        let contact = texture("gtao", attachment | wgpu::TextureUsages::COPY_SRC);
        let history = texture("gtao history", wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        let contact_view = contact.create_view(&Default::default());
        let history_view = history.create_view(&Default::default());
        let group = |read: &wgpu::TextureView| {
            let entry = |binding, resource| wgpu::BindGroupEntry { binding, resource };
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gtao"),
                layout,
                entries: &[
                    entry(14, wgpu::BindingResource::TextureView(read)),
                    entry(15, wgpu::BindingResource::TextureView(&history_view)),
                    entry(16, uniform.as_entire_binding()),
                ],
            })
        };
        let groups = [group(&history_view), group(&raw)];
        Targets { raw, contact, contact_view, history, groups, size }
    }
}
