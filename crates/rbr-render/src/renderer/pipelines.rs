//! The stage shader and the render pipelines of the stage draws, the sky and the car's shadow map.

use super::enhance::Enhance;
use super::sun_mask::NORMALS;
use super::textures::{lit_class, DrawFeatures};
use super::uniforms::frame_index;
use super::DEPTH_FORMAT;
use crate::coords::FRONT_FACE;
use crate::stage::{Anchor, DrawState, Layer, Vertex};
use std::collections::HashMap;
use std::sync::{mpsc, Arc, Condvar, Mutex, OnceLock};

pub(crate) const SHADER: &str = concat!(
    include_str!("../shader.wgsl"),
    include_str!("../terrain_detail.wgsl"),
    include_str!("../albedo.wgsl"),
    include_str!("../lamps.wgsl"),
    include_str!("../night.wgsl"),
    include_str!("../water.wgsl"),
    include_str!("../ssr.wgsl"),
    include_str!("../cabin.wgsl"),
    include_str!("../sky_lut.wgsl"),
    include_str!("../mirrors.wgsl"),
    include_str!("../sun_mask.wgsl"),
    include_str!("../cloud_volume.wgsl"),
    include_str!("../volume_shadow.wgsl"),
    include_str!("../motion.wgsl"),
    include_str!("../grass_card.wgsl"),
    include_str!("../grass_cards.wgsl")
);

/// The pipeline state of a draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct PipelineKey {
    cull_back: bool,
    pub(super) blend: bool,
    depth_test: bool,
    pub(super) depth_write: bool,
    /// The alpha test (`ALPHA_TEST` in `shader.wgsl`); without it the shader is built with no `discard`.
    alpha_test: bool,
    /// A mirror's glass is among its faces (`MIRRORS` in `mirrors.wgsl`): only those draws carry the mirrors' code,
    /// which slows the rest of the stage's shading by a millisecond at 1440p on an M-series Mac.
    mirror: bool,
    /// The draw's lit class ([`lit_class`], `LIT` in `shader.wgsl`), 0 for the stage's: each pipeline carries only its
    /// class's code, and only lit draws (the car's) the cabin light's (`CABIN` in `cabin.wgsl`). The shader's code for
    /// the classes a pipeline never draws still held registers on an M1 Pro, where it slowed every pixel.
    lit: u32,
    /// For a lit draw, whether the frame's view is in the car (`INSIDE` in `shader.wgsl`): the cockpit's pipelines
    /// carry no outside view's light, the others no cabin's. `None` decides at run time (`frame.reflection.y`).
    inside: Option<bool>,
    /// The draw's features (`KNOWN` in `shader.wgsl`): each pipeline carries only the code of the ones it draws,
    /// which its mode uniform says as well. `None` (the prepass) decides at run time.
    features: Option<DrawFeatures>,
    /// For a lit draw, whether its faces are all the cabin's interior set (`CABIN_SET` in `shader.wgsl`), or none are:
    /// the interior's pipelines carry no fog, the others no cabin light. `None` decides per fragment.
    cabin_set: Option<bool>,
    /// The draw's frame uniform ([`frame_index`](super::uniforms::frame_index), `FRAME_KIND` in `shader.wgsl`): the
    /// stage's, the clouds' or the sky faces'. `None` (the prepass) decides at run time.
    frame: Option<usize>,
    /// A sky draw, drawn behind the rest at depth 0 where the depth is still the clear one (`Renderer::draw_scene`); the
    /// backdrop's (the world-anchored sky faces') at their own depth.
    sky: bool,
    pass: DrawPass,
    /// The car's lamps light the stage's draws (`LAMPS` in `shader.wgsl`): only the frames where they are lit take the
    /// pipelines with the lamps' code, which slows the rest of the stage's shading by a millisecond at 1080p on an
    /// M-series Mac.
    lamps: bool,
    /// The grass made on the GPU (`crate::grass::made`): its cards from their buffer (`vs_grass`), no vertex buffer.
    pub(super) made: bool,
    /// An instanced BTB mesh (`stage::placed`): `vs_main_placed` or `vs_clouded_placed`, with the placements' buffer
    /// ([`placed_layout`]) after the vertices'.
    pub(super) placed: bool,
}

/// Where a pipeline draws in the main view's pass. The alpha-tested opaque draws ([`prepassed`]) first lay their depth
/// with a cheap shader that only cuts the texture's alpha out, then shade where their depth is still the one laid: so
/// only the nearest of the layers of foliage over a pixel runs the full shader. The frame is the same as drawing them
/// once in order (to the pixel on 66_E and 33_N).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum DrawPass {
    /// Drawn once, in order (the reflection cube draws every draw so).
    Main,
    /// The depth only, before every other draw: `fs_prepass`, no colour written.
    DepthPrepass,
    /// The full shader where the depth equals the prepass's, writing no depth. The alpha test, since a cut-out texel
    /// lying at the depth laid by another draw (coincident cards) passes Equal too: without its `discard`, which of
    /// the tied fragments a pixel kept changed from run to run on Metal. Both pipelines give the same depth through
    /// `vs_main`'s `@invariant` position.
    AfterPrepass,
    /// The car's lit draws in the main view with the sun's mask (`SUN_MASK`): drawn once in order as `Main`, over the depth
    /// their prepass laid a little behind them ([`PREPASS_BIAS`]), so that coincident faces keep the order they have
    /// without it (the first drawn wins), which `AfterPrepass`'s Equal test would turn around.
    CarMain,
}

/// How far behind its draw the car's prepass lays its depth, in units of the depth format's least step (reverse-Z, so
/// negative): the contact shading's depth, not seen otherwise.
const PREPASS_BIAS: i32 = -256;

impl PipelineKey {
    /// A draw's, with `mirror` when it holds a mirror's glass and `sky` for the sky's.
    pub(super) fn of(state: &DrawState, mirror: bool, sky: bool) -> PipelineKey {
        PipelineKey {
            cull_back: state.cull_back,
            blend: state.blend,
            depth_test: state.depth_test,
            depth_write: state.depth_write,
            alpha_test: state.alpha_ref.is_some(),
            mirror,
            lit: lit_class(state.shade.as_ref()),
            inside: None,
            features: None,
            cabin_set: None,
            frame: None,
            sky,
            pass: DrawPass::Main,
            lamps: false,
            made: false,
            placed: false,
        }
    }

    /// The pipeline drawing an instanced mesh ([`PipelineKey::placed`]).
    pub(super) fn placed(self, placed: bool) -> PipelineKey {
        PipelineKey { placed, ..self }
    }

    /// The pipeline drawing the grass made on the GPU ([`PipelineKey::made`]).
    pub(super) fn made(self, made: bool) -> PipelineKey {
        PipelineKey { made, ..self }
    }

    pub(super) fn in_pass(self, pass: DrawPass) -> PipelineKey {
        // The prepass only cuts the texture's alpha out.
        let laying = pass == DrawPass::DepthPrepass;
        let (features, frame) = (self.features.filter(|_| !laying), self.frame.filter(|_| !laying));
        PipelineKey { pass, features, frame, ..self }
    }

    /// The lit draw's pipeline for faces all of the cabin's interior set or none of it ([`PipelineKey::cabin_set`]).
    pub(super) fn in_cabin(self, cabin_set: Option<bool>) -> PipelineKey {
        PipelineKey { cabin_set: cabin_set.filter(|_| self.lit != 0), ..self }
    }

    /// The pipeline for a draw with frame uniform `frame`.
    pub(super) fn framed(self, frame: usize) -> PipelineKey {
        PipelineKey { frame: Some(frame), ..self }
    }

    /// The pipeline for a draw with `features`.
    pub(super) fn drawing(self, features: DrawFeatures) -> PipelineKey {
        PipelineKey { features: Some(features), ..self }
    }

    /// The lit draw's pipeline for a view in the car or outside it.
    pub(super) fn viewed(self, inside: bool) -> PipelineKey {
        PipelineKey { inside: Some(inside), ..self }
    }

    /// Whether all the draw's faces are the cabin's interior set's ([`PipelineKey::cabin_set`]).
    pub(super) fn cabin(&self) -> bool {
        self.cabin_set == Some(true)
    }

    /// Whether the draw has a pipeline per view ([`PipelineKey::inside`]): the lit draws.
    pub(super) fn takes_views(&self) -> bool {
        self.lit != 0
    }

    /// The pipeline with the lamps' code.
    pub(super) fn with_lamps(self) -> PipelineKey {
        PipelineKey { lamps: true, ..self }
    }

    /// Whether the draw has a pipeline with the lamps' code ([`PipelineKey::lamps`]): the stage's unlit draws.
    pub(super) fn takes_lamps(&self) -> bool {
        self.lit == 0 && !self.sky
    }
}

/// An opaque alpha-tested stage draw that writes depth: the main view lays its depth first ([`DrawPass`]); with the
/// sun's mask (`sun_mask`) every opaque one, whose depth the mask is worked out from. The lit draws are left out, since
/// their alpha is not the texture's alone, but for the car's reflecting ones.
pub(super) fn prepassed(state: &DrawState, lighting: bool, cabin: bool) -> bool {
    let opaque = state.depth_test && state.depth_write;
    match &state.shade {
        // The car's opaque draws too (not the characters', whose shade reflects nothing), in the outside views
        // (`GpuDraw::prepassed`): the contact shading then sees the car, its arches and the ground under it. In the
        // in-car view the cabin's interior set's ([`PipelineKey::cabin`]), for its own contact shading.
        Some(shade) => lighting && opaque && !state.blend && (shade.fresnel_max > 0.0 || cabin),
        None => (cutout(state) || (lighting && !state.blend)) && opaque,
    }
}

/// An opaque alpha-tested draw (`cutout` in `shader.wgsl`): the object lanes' foliage, fences and spectators.
pub(crate) fn cutout(state: &DrawState) -> bool {
    !state.blend && state.alpha_ref.is_some()
}

/// What the stage's pipelines are built from: the shader, its layout and the colour format drawn in.
pub(super) struct Context<'a> {
    pub(super) device: &'a wgpu::Device,
    pub(super) format: wgpu::TextureFormat,
    pub(super) shader: &'a wgpu::ShaderModule,
    pub(super) layout: &'a wgpu::PipelineLayout,
    /// The layout with the grass's cards after the stage's groups ([`PipelineKey::made`]).
    pub(super) made_layout: &'a wgpu::PipelineLayout,
    /// The stage draws' vertex entry point ([`vertex_entry`]).
    pub(super) vertex: &'static str,
    /// [`Enhance::water`] or [`Enhance::wet`] (`WATER_PLUS` in `shader.wgsl`).
    pub(super) water_plus: bool,
    /// [`Enhance::cabin`] (`CABIN` in `shader.wgsl`).
    pub(super) cabin: bool,
    /// [`Enhance::grass`], with the stage's grass field (`GRASS` in `shader.wgsl`).
    pub(super) grass: bool,
    /// [`Enhance::lighting`] (`LIGHTING` in `sky_lut.wgsl`, `shader.wgsl`): the sky's tables, the reflection probes,
    /// and the draws after the prepass reading the sun's shadow and the contact shading from their mask (`SUN_MASK`,
    /// `renderer::sun_mask`).
    pub(super) lighting: bool,
    /// [`DebugView`] (`DEBUG_VIEW` in `cabin.wgsl`): 0 draws the shaded colour, as without the flag.
    pub(super) debug_view: u32,
    /// The surroundings cube's mip levels past [`REFLECTION_SIZE`](super::reflection::REFLECTION_SIZE)'s: 1 at
    /// [`Detail::Ultra`](super::Detail::Ultra) (`CUBE_SHIFT`).
    pub(super) cube_shift: u32,
    /// The colour targets' alpha the draws write ([`alpha_writes`]).
    pub(super) taa: bool,
}

/// The colour a draw writes into the frame: all of it, but under the temporal anti-aliasing only the car's blended
/// draws (its glass) write the alpha, which the resolve takes as its mask of what moves with the car over what moves
/// with the stage behind it (`taa.wgsl`); the frame's alpha is otherwise read by nothing.
pub(super) fn alpha_writes(taa: bool, car_blend: bool) -> wgpu::ColorWrites {
    match taa && !car_blend {
        true => wgpu::ColorWrites::COLOR,
        false => wgpu::ColorWrites::ALL,
    }
}

/// The stage draws' vertex entry point: with our lighting ([`Enhance::lighting`]) the one that also gives the
/// clouds' shadow. Every pipeline over the stage's vertices takes the same one, so their depths agree.
pub(super) fn vertex_entry(enhance: Enhance) -> &'static str {
    if enhance.lighting {
        "vs_clouded"
    } else {
        "vs_main"
    }
}

/// A stage pipeline made on a worker thread ([`Pool`]): usable once [`Pool::finish`] has returned.
#[derive(Clone)]
pub(super) struct Pipe(Arc<OnceLock<wgpu::RenderPipeline>>);

impl std::ops::Deref for Pipe {
    type Target = wgpu::RenderPipeline;

    fn deref(&self) -> &wgpu::RenderPipeline {
        self.0.get().expect("the stage's pipelines are finished before the first frame")
    }
}

impl PartialEq for Pipe {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl Eq for Pipe {}

impl PartialOrd for Pipe {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Pipe {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (**self).cmp(&**other)
    }
}

/// What a worker needs of a [`Context`], owned.
struct Owned {
    device: wgpu::Device,
    format: wgpu::TextureFormat,
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    made_layout: wgpu::PipelineLayout,
    vertex: &'static str,
    water_plus: bool,
    cabin: bool,
    grass: bool,
    lighting: bool,
    debug_view: u32,
    cube_shift: u32,
    taa: bool,
}

impl Owned {
    fn create(&self, key: PipelineKey) -> wgpu::RenderPipeline {
        Context {
            device: &self.device,
            format: self.format,
            shader: &self.shader,
            layout: &self.layout,
            made_layout: &self.made_layout,
            vertex: self.vertex,
            water_plus: self.water_plus,
            cabin: self.cabin,
            grass: self.grass,
            lighting: self.lighting,
            debug_view: self.debug_view,
            cube_shift: self.cube_shift,
            taa: self.taa,
        }
        .pipeline(key)
    }
}

/// The stage's pipelines, each made once on a pool of threads while the load goes on: the driver's compile of one
/// takes about a second on a cold shader cache, and 40 of them in a row took 35 s. `RBR_PIPELINE_THREADS` sets the
/// threads (0 or 1: made in place, one after another).
pub(super) struct Pool {
    made: HashMap<PipelineKey, Pipe>,
    jobs: Option<mpsc::Sender<(PipelineKey, Pipe)>>,
    owned: Arc<Owned>,
    pending: Arc<(Mutex<usize>, Condvar)>,
}

/// Counts a job done when dropped, so a worker that panics does not leave [`Pool::finish`] waiting.
struct Done(Arc<(Mutex<usize>, Condvar)>);

impl Drop for Done {
    fn drop(&mut self) {
        *self.0 .0.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
        self.0 .1.notify_all();
    }
}

impl Pool {
    pub(super) fn new(context: &Context) -> Pool {
        let owned = Arc::new(Owned {
            device: context.device.clone(),
            format: context.format,
            shader: context.shader.clone(),
            layout: context.layout.clone(),
            made_layout: context.made_layout.clone(),
            vertex: context.vertex,
            water_plus: context.water_plus,
            cabin: context.cabin,
            grass: context.grass,
            lighting: context.lighting,
            debug_view: context.debug_view,
            cube_shift: context.cube_shift,
            taa: context.taa,
        });
        let threads = std::env::var("RBR_PIPELINE_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()).min(4));
        let pending = Arc::new((Mutex::new(0), Condvar::new()));
        let jobs = (threads > 1).then(|| {
            let (sender, receiver) = mpsc::channel::<(PipelineKey, Pipe)>();
            let receiver = Arc::new(Mutex::new(receiver));
            for _ in 0..threads {
                let (receiver, owned, pending) = (receiver.clone(), owned.clone(), pending.clone());
                std::thread::spawn(move || loop {
                    let next = receiver.lock().unwrap_or_else(|e| e.into_inner()).recv();
                    let Ok((key, pipe)) = next else { break };
                    let _done = Done(pending.clone());
                    let _ = pipe.0.set(owned.create(key));
                });
            }
            sender
        });
        Pool { made: HashMap::new(), jobs, owned, pending }
    }

    /// The pipeline of `key`, queued to the pool when it is the first asked for.
    pub(super) fn get(&mut self, key: PipelineKey) -> Pipe {
        let Pool { made, jobs, owned, pending } = self;
        made.entry(key)
            .or_insert_with(|| {
                let pipe = Pipe(Arc::new(OnceLock::new()));
                match jobs {
                    Some(jobs) => {
                        *pending.0.lock().unwrap() += 1;
                        jobs.send((key, pipe.clone())).expect("the pool's threads outlive it");
                    }
                    None => {
                        let _ = pipe.0.set(owned.create(key));
                    }
                }
                pipe
            })
            .clone()
    }

    /// Waits for every queued pipeline, and ends the threads.
    pub(super) fn finish(&mut self) {
        self.jobs = None;
        let (count, done) = &*self.pending;
        let mut count = count.lock().unwrap_or_else(|e| e.into_inner());
        while *count > 0 {
            count = done.wait(count).unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl Context<'_> {
    /// The fragment entry point that shades the draws, taking the clouds' shadow when the vertex gives it.
    fn shading(&self) -> &'static str {
        if self.vertex == "vs_clouded" {
            "fs_clouded"
        } else {
            "fs_main"
        }
    }

    pub(super) fn pipeline(&self, key: PipelineKey) -> wgpu::RenderPipeline {
        let t = rbr_btb::phases::start();
        let pipeline = self.create_pipeline(key);
        rbr_btb::phases::add("pipeline creation (CPU)", t);
        pipeline
    }

    fn create_pipeline(&self, key: PipelineKey) -> wgpu::RenderPipeline {
        let (entry, depth_write, depth_compare, alpha_test, write_mask) = match key.pass {
            DrawPass::Main => (
                self.shading(),
                key.depth_write,
                wgpu::CompareFunction::Greater,
                key.alpha_test,
                alpha_writes(self.taa, key.lit != 0 && key.blend),
            ),
            DrawPass::CarMain => (
                self.shading(),
                key.depth_write,
                wgpu::CompareFunction::Greater,
                key.alpha_test,
                alpha_writes(self.taa, key.lit != 0 && key.blend),
            ),
            DrawPass::DepthPrepass => {
                let normals = if self.lighting { wgpu::ColorWrites::COLOR } else { wgpu::ColorWrites::empty() };
                ("fs_prepass", true, wgpu::CompareFunction::Greater, key.alpha_test, normals)
            }
            DrawPass::AfterPrepass => {
                let write_mask = alpha_writes(self.taa, key.lit != 0 && key.blend);
                (self.shading(), false, wgpu::CompareFunction::Equal, key.alpha_test, write_mask)
            }
        };
        // The backdrop lays its depth over what lies farther (`Renderer::draw_scene`); a sky dome, drawn at depth 0, where
        // the depth is still the clear one.
        let backdrop = key.sky && key.frame == Some(frame_index(Anchor::World, Layer::Sky));
        let depth_write = depth_write || backdrop;
        let depth_compare = match key {
            _ if backdrop => wgpu::CompareFunction::GreaterEqual,
            PipelineKey { sky: true, .. } => wgpu::CompareFunction::Equal,
            PipelineKey { depth_test: true, .. } => depth_compare,
            _ => wgpu::CompareFunction::Always,
        };
        let features = key.features.unwrap_or_default();
        let placed_buffers = [Some(vertex_layout()), Some(placed_layout()), Some(probe_layout())];
        let (layout, vertex, buffers) = match (key.made, key.placed) {
            (true, _) => {
                (self.made_layout, if self.vertex == "vs_clouded" { "vs_grass_clouded" } else { "vs_grass" }, &[][..])
            }
            (false, true) => (
                self.layout,
                if self.vertex == "vs_clouded" { "vs_clouded_placed" } else { "vs_main_placed" },
                &placed_buffers[..],
            ),
            (false, false) => (self.layout, self.vertex, &[Some(vertex_layout())][..]),
        };
        self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stage"),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: self.shader,
                entry_point: Some(vertex),
                compilation_options: Default::default(),
                buffers,
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: FRONT_FACE,
                cull_mode: key.cull_back.then_some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(depth_write),
                depth_compare: Some(depth_compare),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState {
                    constant: if key.pass == DrawPass::DepthPrepass && key.lit != 0 { PREPASS_BIAS } else { 0 },
                    ..Default::default()
                },
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: self.shader,
                entry_point: Some(entry),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[
                        ("ALPHA_TEST", f64::from(u8::from(alpha_test))),
                        ("WATER_PLUS", f64::from(u8::from(self.water_plus))),
                        ("CABIN", f64::from(u8::from(self.cabin && key.lit != 0))),
                        ("GRASS", f64::from(u8::from(self.grass))),
                        ("LIGHTING", f64::from(u8::from(self.lighting))),
                        ("DEBUG_VIEW", f64::from(self.debug_view)),
                        ("MIRRORS", f64::from(u8::from(key.mirror))),
                        ("LAMPS", f64::from(u8::from(key.lamps))),
                        ("LIT", f64::from(key.lit)),
                        ("INSIDE", key.inside.map_or(2.0, |inside| f64::from(u8::from(inside)))),
                        ("CABIN_SET", key.cabin_set.map_or(2.0, |set| f64::from(u8::from(set)))),
                        ("FRAME_KIND", key.frame.map_or(3.0, |frame| frame as f64)),
                        ("KNOWN", f64::from(u8::from(key.features.is_some()))),
                        ("DRAW_WATER", f64::from(u8::from(features.water))),
                        ("DRAW_SPECULAR", f64::from(u8::from(features.specular))),
                        ("DRAW_SECOND", f64::from(u8::from(features.second_texture))),
                        ("DRAW_DETAIL", f64::from(u8::from(features.detail))),
                        ("DRAW_BUMP", f64::from(u8::from(features.bump))),
                        ("DRAW_HIGHLIGHT", f64::from(u8::from(features.highlight))),
                        ("DRAW_SHEEN", f64::from(u8::from(features.sheen))),
                        ("DRAW_LAYERS", f64::from(u8::from(features.layers))),
                        (
                            "SUN_MASK",
                            f64::from(u8::from(
                                self.lighting
                                    && matches!(key.pass, DrawPass::AfterPrepass | DrawPass::CarMain)
                                    && !key.sky,
                            )),
                        ),
                        ("CUBE_SHIFT", f64::from(self.cube_shift)),
                    ],
                    ..Default::default()
                },
                targets: &[Some(wgpu::ColorTargetState {
                    // The mask's prepass draws the normals into their own, smaller target.
                    format: if self.lighting && key.pass == DrawPass::DepthPrepass { NORMALS } else { self.format },
                    blend: key.blend.then_some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// The sky gradient: one triangle over the screen, behind everything (where the depth is still the clear one, no
    /// depth write).
    pub(super) fn sky_pipeline(&self, layout: &wgpu::PipelineLayout) -> wgpu::RenderPipeline {
        self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: self.shader,
                entry_point: Some("vs_sky"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Equal),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: self.shader,
                entry_point: Some("fs_sky"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[("LIGHTING", f64::from(u8::from(self.lighting)))],
                    ..Default::default()
                },
                targets: &[Some(wgpu::ColorTargetState {
                    write_mask: alpha_writes(self.taa, false),
                    ..self.format.into()
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// The car shadow map's draws into the layer of vertex `entry`: depth only, both faces (thin parts cast from
    /// either side), with a slope bias against the car shading itself.
    pub(super) fn shadow_pipeline(&self, layout: &wgpu::PipelineLayout, entry: &str) -> wgpu::RenderPipeline {
        self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("car shadow"),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: self.shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState { constant: 0, slope_scale: 2.0, clamp: 0.0 },
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        })
    }
}

/// The stage's bind group layouts: the frame's (its uniform, the car's shadow map and its sampler, the reflection cube
/// and its sampler, the sun's cascades), the car shadow map draws' (the frame uniform alone), and the draws' own
/// ([`bind_group`](super::textures::bind_group)).
pub(super) fn layouts(device: &wgpu::Device) -> [wgpu::BindGroupLayout; 3] {
    let frame_entry = wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("frame"),
        entries: &[
            frame_entry,
            texture_entry(
                1,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
            ),
            // The comparison sampler and the sun's cascades are read by the vertex stage too (the cabin's shade,
            // `cabin_shade` in `cabin.wgsl`).
            wgpu::BindGroupLayoutEntry {
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ..texture_entry(2, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison))
            },
            texture_entry(
                3,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::Cube,
                    multisampled: false,
                },
            ),
            texture_entry(4, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            wgpu::BindGroupLayoutEntry {
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ..texture_entry(
                    5,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                )
            },
            // The physically based sky's tables (`sky_lut.wgsl`): the sky view, the aerial table, the sky cube and the
            // sky's light.
            texture_entry(6, TEXTURE),
            texture_entry(
                8,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D3,
                    multisampled: false,
                },
            ),
            texture_entry(
                9,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::Cube,
                    multisampled: false,
                },
            ),
            texture_entry(
                10,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ), // The surroundings cube (`Reflection::surroundings`).
            texture_entry(
                11,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::Cube,
                    multisampled: false,
                },
            ),
            // The mirrors' images and views (`mirrors.wgsl`).
            texture_entry(
                12,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
            ),
            texture_entry(
                13,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
            // The ground's light seen from above and each grid cell's tile (`renderer::bounce::Ground`).
            texture_entry(
                14,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
            ),
            texture_entry(
                15,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
            // The reflection probes and their grid (`renderer::probes`).
            texture_entry(
                16,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::CubeArray,
                    multisampled: false,
                },
            ),
            texture_entry(
                17,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
            // The car's shadow map's depth range per block (`renderer::shadows::ShadowRange`).
            texture_entry(
                18,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
            ),
            // The volumetric clouds' shadow map and its sampler (`renderer::cloud_volume`, `cloud_shadow` in
            // `cloud_volume.wgsl`): read by the vertex stage (`vs_clouded`), so both stages may see them.
            wgpu::BindGroupLayoutEntry {
                binding: 19,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: TEXTURE,
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 20,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("shadow frame"),
        entries: &[frame_entry],
    });
    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("texture"),
        entries: &[
            texture_entry(0, TEXTURE),
            texture_entry(1, TEXTURE),
            texture_entry(2, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            // The mode is read by the vertex stage too (the sun's cascades leave out the grass, `vs_sun`).
            wgpu::BindGroupLayoutEntry {
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ..texture_entry(
                    3,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                )
            },
            texture_entry(4, TEXTURE),
            texture_entry(5, TEXTURE),
            texture_entry(6, TEXTURE),
            texture_entry(7, TEXTURE),
            // A multilayer material's mask, four details and diffuse (`layered` in `shader.wgsl`).
            texture_entry(8, TEXTURE),
            texture_entry(9, TEXTURE),
            texture_entry(10, TEXTURE),
            texture_entry(11, TEXTURE),
            texture_entry(12, TEXTURE),
            texture_entry(13, TEXTURE),
        ],
    });
    [frame_layout, shadow_layout, texture_layout]
}

/// A filterable float 2D texture binding.
pub(crate) const TEXTURE: wgpu::BindingType = wgpu::BindingType::Texture {
    sample_type: wgpu::TextureSampleType::Float { filterable: true },
    view_dimension: wgpu::TextureViewDimension::D2,
    multisampled: false,
};

/// A binding of `ty` that the fragment stage reads.
pub(crate) fn texture_entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None }
}

/// The instance-rate vertex buffer of the instanced BTB meshes ([`Instance`](crate::stage::placed::Instance)): the
/// matrix's three rows and the light (`InstanceIn` in `shader.wgsl`), locations 11 to 14.
pub(crate) fn placed_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![11 => Float32x4, 12 => Float32x4, 13 => Float32x4, 14 => Float32x4];
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<crate::stage::placed::Instance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRIBUTES,
    }
}

/// The probes of the instanced meshes, a stage pack's [`Vertex`]es read once per instance: their occlusion, sky direction
/// and bounce, which lie together (`probe` in `InstanceIn`, location 15).
pub(crate) fn probe_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 1] = [wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Uint32x4,
        offset: std::mem::offset_of!(Vertex, occlusion) as u64,
        shader_location: 15,
    }];
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRIBUTES,
    }
}

const _: () = {
    use std::mem::offset_of;
    assert!(offset_of!(Vertex, sky_dir) == offset_of!(Vertex, occlusion) + 4);
    assert!(offset_of!(Vertex, bounce) == offset_of!(Vertex, occlusion) + 8);
};

/// Vertex buffer layout of [`Vertex`]: position, colour, texture 0, 1 and 2 coordinates, normal.
pub(crate) fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 11] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Unorm8x4,
        2 => Float32x2,
        3 => Float32x2,
        4 => Float32x2,
        5 => Float32x3,
        6 => Float32x3,
        7 => Float32,
        8 => Uint32,
        9 => Uint32x2,
        10 => Float32x2,
    ];
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
}
