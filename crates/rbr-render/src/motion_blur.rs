//! Motion blur along each pixel's screen motion (ours, experimental), an opt-in pass (`Enhance::motion_blur`,
//! `--enhance blur`) that runs only with the temporal anti-aliasing: the resolve writes into [`MotionBlur::target`]
//! instead of the post pass's scene, and this pass gathers the scene along each pixel's blur vector into it. The blur
//! vector is the pixel's velocity in uv per frame (the motion target's where something moving was drawn, else the
//! point from the depth carried by the resolve's `view_to_prev`, as `previous_uv` in `taa.wgsl` finds it) times
//! [`SHUTTER`] over the frame time, centred on the pixel and capped at [`MOST`] of the output height. Eleven taps
//! along it, offset by the pixel's interleaved gradient noise advanced each frame, weighted so a pixel that does not
//! move neither blurs nor bleeds into the moving road beside it.

use bytemuck::{Pod, Zeroable};

use crate::gpu_util::{self, CLEAR, FILTERING, UNIFORM};
use crate::post::SCENE_FORMAT;
use crate::renderer::{depth_binding, TEXTURE};

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
    /// The resolve's target while the blur is on (the output's size), which the blur gathers into the post pass's
    /// scene.
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
        let layout =
            gpu_util::layout(device, "motion blur", &[UNIFORM, TEXTURE, depth_binding(), TEXTURE, FILTERING, UNIFORM]);
        let module = gpu_util::shader(device, "motion blur", SHADER);
        let targets = [Some(SCENE_FORMAT.into())];
        let pipeline = gpu_util::full_screen_pipeline(
            device,
            "motion blur",
            &module,
            ("vs_full", "fs_blur"),
            &[&layout],
            &targets,
            &[],
        );
        let sampler = gpu_util::linear_clamp(device, "motion blur");
        let params = gpu_util::uniform_buffer(device, "motion blur", size_of::<Params>());
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
        self.bind(device, uniform, motion, depth);
        // A new target must not carry the old frame's motion: the strength restarts with no previous frame.
        self.previous = None;
    }

    /// Reads the drawn view's temporal anti-aliasing's `uniform` (each view of a triple screen has its own, with its
    /// own `view_to_prev` and history reset), the frame's `motion` and `depth`.
    pub(crate) fn bind(
        &mut self,
        device: &wgpu::Device,
        uniform: &wgpu::Buffer,
        motion: &wgpu::TextureView,
        depth: &wgpu::TextureView,
    ) {
        self.group = group(device, &self.layout, &self.sampler, &self.params, uniform, &self.target, motion, depth);
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
        gpu_util::full_screen_pass(encoder, "motion blur", scene, CLEAR, &self.pipeline, &[&self.group]);
    }
}

/// The resolve's target of `size` ([`SCENE_FORMAT`]: the post pass's scene's format).
fn target(device: &wgpu::Device, size: (u32, u32)) -> wgpu::TextureView {
    let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    gpu_util::target(device, "motion blur scene", size, SCENE_FORMAT, usage)
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
    use wgpu::BindingResource::{Sampler, TextureView};
    let resources = [
        uniform.as_entire_binding(),
        TextureView(target),
        TextureView(depth),
        TextureView(motion),
        Sampler(sampler),
        params.as_entire_binding(),
    ];
    gpu_util::bind_group(device, "motion blur", layout, &resources)
}
