//! wgpu drawing of a loaded [`Stage`]. Each draw's render state ([`DrawState`](crate::stage::DrawState)) maps to a
//! pipeline (culling, blending, depth) and a bind group (its textures, address modes and shader mode). Meshes are
//! uploaded once, and meshes outside the view frustum are skipped.  Textures are BC1 (`DXT1`), BC2 (`DXT3`) or BC3
//! (`DXT5`, community car models) as stored, so the device needs `TEXTURE_COMPRESSION_BC`. Colours are not converted to
//! sRGB, as the D3D9 fixed output did not.

use crate::atmosphere::Atmosphere;
use crate::dials;
use crate::effects::Effects;
use crate::glass;
use crate::lights::Lamp;
use crate::particles;
use crate::post::{Post, SCENE_FORMAT};
use crate::profile::Profiler;
use crate::skylight::{self, SkyLight};
use crate::stage::TextureRef;
use crate::start_board::{self, StartBoard, StartLamp};
use crate::sun::{self, Cascades, SunShadow};
use crate::uploads::Uploads;
use glam::{Mat4, Vec3, Vec4};
use rbr_formats::dds::{self, Format};
use std::collections::{BTreeMap, HashMap};

mod bounce;
mod build;
mod cloud_volume;
mod depth_pyramid;
mod enhance;
mod frame;
mod gpu;
mod meshes;
mod mirrors;
mod motion;
mod occlusion;
mod offscreen;
mod pipelines;
mod probes;
mod reflection;
mod shadows;
mod sun_mask;
mod textures;
mod uniforms;
mod views;

pub use cloud_volume::survey::CloudSurvey;
#[cfg(test)]
pub(crate) use depth_pyramid::SHADER as PYRAMID_SHADER;
pub use enhance::{AntiAliasing, DebugView, Detail, Enhance, Reflections};
pub use frame::FrameStats;
pub use gpu::{instance, request_gpu, Gpu, GpuError};
pub(crate) use meshes::mesh_bounds;
use meshes::{GpuMesh, MeshBuffers};
pub use offscreen::ReadbackError;
use pipelines::Context;
pub(crate) use pipelines::{cutout, texture_entry, vertex_layout, SHADER, TEXTURE};
#[cfg(test)]
pub(crate) use reflection::BLIT;
use reflection::{Reflection, ScreenReflection};
#[cfg(test)]
pub(crate) use shadows::RANGE_SHADER;
use sun_mask::SunMask;
pub(crate) use textures::dds_texture;
use uniforms::FrameBinding;
pub(crate) use uniforms::{LampUniform, MAX_LAMPS};
pub use views::FrameView;

pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const CLEAR_COLOR: wgpu::Color = wgpu::Color { r: 0.42, g: 0.58, b: 0.78, a: 1.0 };
/// The binding of the frame's depth.
pub(crate) fn depth_binding() -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Depth,
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled: false,
    }
}

/// Draws one stage into a colour target of `format`.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    enhance: Enhance,
    frames: [FrameBinding; 3],
    meshes: Vec<GpuMesh>,
    /// The meshes with a draw that casts the car's shadow, in order ([`meshes::casters`]).
    casters: Vec<usize>,
    /// The meshes' shared vertex and index buffers ([`Packing`]).
    buffers: Vec<MeshBuffers>,
    /// The GPU mesh of each stage mesh, `None` for a mesh with no draws (not uploaded).
    slots: Vec<Option<usize>>,
    /// Physics handle to the GPU mesh of each supported interactive visibility record.
    interactive_slots: HashMap<u32, usize>,
    /// The grass's tile meshes, filled about the eye ([`Renderer::set_grass`]).
    grass: Option<crate::grass::Ring>,
    atmosphere: Atmosphere,
    /// The ground's light seen from above ([`crate::bounce`], with [`Enhance::lighting`]); the stage's light off itself
    /// is drawn into its vertices at load.
    ground_light: Option<bounce::Ground>,
    /// How wet the stage's weather is ([`FrameUniform::wetness`]).
    wetness: f32,
    /// The car's dirt ([`Renderer::set_car_dirt`]).
    car_dirt: crate::dirt::CarDirt,
    /// How much sun gets into the cabin ([`Renderer::set_cabin_sun`]) and the car body's model space to stage space
    /// ([`Renderer::set_car_body`]).
    cabin_sun: Option<crate::car::cabin::SunIn>,
    car_body: Option<Mat4>,
    /// The car's shape the in-car view draws no particles in ([`Renderer::set_cabin_mask`]).
    cabin_mask: Option<crate::particles::cabin::CabinMask>,
    /// The camera is outside the car ([`Renderer::set_outside_view`]).
    outside_view: bool,
    /// The sky gradient, drawn first ([`Atmosphere::zenith`] overhead, the fog colour at the horizon).
    sky: wgpu::RenderPipeline,
    /// The size the frame is drawn at, and the size of the target the post pass writes
    /// ([`Renderer::set_render_scale`]).
    depth_size: (u32, u32),
    output_size: (u32, u32),
    /// The share of the target's width and height the frame is drawn at (1: all of it).
    render_scale: f32,
    depth_view: wgpu::TextureView,
    /// The sun's shadow over the main view, from its depth (with [`Enhance::lighting`]).
    sun_mask: SunMask,
    /// The volumetric clouds over the main view's sky (with [`Enhance::lighting`]).
    cloud_volume: cloud_volume::CloudVolume,
    /// The shadow map's parameters ([`cloud_volume::shadow_params`]) of the main view's frame being drawn: the map is
    /// drawn for the main eye, so the mirrors and the cubes read these rather than their own eye's. The frames before
    /// the main view's read the last frame's.
    shadow_params: std::cell::Cell<[f32; 4]>,
    /// The car's sun and contact shadows ([`SHADOW_SIZE`](shadows::SHADOW_SIZE), [`GROUND_REACH`](shadows::GROUND_REACH)): the map's two
    /// layers, the pipelines that draw into them, and the frame binding those read (the world-anchored frame's buffer
    /// without the map).
    shadow_layers: [wgpu::TextureView; 2],
    shadow_pipelines: [wgpu::RenderPipeline; 2],
    reflection: Reflection,
    /// The stage around the car to the old reach, for its light and the wet roads' and the water's reflections.
    surroundings: Reflection,
    /// The surroundings baked along the driveline in its place ([`Enhance::lighting`]).
    probes: probes::Probes,
    /// The physically based sky's tables ([`Enhance::lighting`]; bound in every frame, drawn only with it).
    skylight: SkyLight,
    /// The low ground the height fog settles on (`None` without [`Enhance::lighting`]).
    fog_ground: Option<crate::height_fog::Ground>,
    shadow_frame: wgpu::BindGroup,
    /// The car's shadow map's depth range per block, redrawn with it.
    shadow_range: shadows::ShadowRange,
    /// The sun's first cascade's depth range per block, redrawn with it (`None` without the cascades).
    sun_range: Option<shadows::ShadowRange>,
    /// `None` when no draw reflects ([`reflects`]).
    screen_reflection: Option<ScreenReflection>,
    post: Post,
    /// `None` unless [`AntiAliasing::Taa`] ([`crate::taa`]).
    taa: Option<crate::taa::Taa>,
    /// The motion of what moves of itself, for the temporal anti-aliasing (`None` without it).
    motion: Option<motion::Motion>,
    /// `None` unless [`Enhance::lighting`] ([`crate::light_shafts`]).
    light_shafts: Option<crate::light_shafts::LightShafts>,
    /// `None` unless [`Enhance::particles`].
    effects: Option<Effects>,
    /// The soft particles' target at half a tall frame's size ([`crate::particle_target`]).
    particle_target: Option<crate::particle_target::ParticleTarget>,
    /// `None` unless [`Enhance::lighting`].
    sun: Option<SunShadow>,
    /// The sun's cascades for the frame being drawn, `None` when there are none (no sun, or an overcast sky).
    cascades: Option<Cascades>,
    /// The low and high corners of the stage and its backdrop (their vertices [`crate::coords::on_stage`]); the world
    /// layer's matrix and texel for the frame ([`sun::world`], `None` without the cascades or under an overcast sky),
    /// and the matrix it was last drawn with.
    world_box: Option<(Vec3, Vec3)>,
    world: Option<(Mat4, f32)>,
    world_drawn: Option<Mat4>,
    /// What the cascades' far layer holds ([`sun::FarDrawn`]), drawn again only when it no longer serves the eye.
    far_drawn: Option<sun::FarDrawn>,
    /// What the second and third cascades hold ([`sun::CascadesDrawn`]).
    cascades_drawn: sun::CascadesDrawn,
    /// The skid marks ([`crate::skidmarks_pass`]), made by the first [`Renderer::set_skid_marks`] or
    /// [`Renderer::set_tyre_marks`].
    skid_marks: Option<crate::skidmarks_pass::SkidPass>,
    /// The game's car shadow decal ([`crate::car_shadow`]), made by the first [`Renderer::set_car_shadow`].
    car_shadow: Option<crate::skidmarks_pass::SkidPass>,
    /// The particle billboards ([`Renderer::set_particles`]), drawn last in the main view.
    particles: Option<particles::gpu::Pass>,
    /// The windscreen's drops ([`Renderer::set_glass`]), drawn after the particles.
    glass: Option<glass::gpu::Pass>,
    /// Seconds since the start ([`Renderer::set_time`]).
    time: f32,
    /// The light shafts' marches drawn (the jitter's frame, [`crate::light_shafts::Sun`]).
    air_frame: u32,
    /// The car's lit lamps at night ([`Renderer::set_lamps`]).
    lamps: [LampUniform; MAX_LAMPS],
    /// The headlights' shadow map's clip matrix for the frame being drawn, `None` without one.
    lamp_matrix: Option<Mat4>,
    /// `None` unless [`Renderer::enable_profiling`].
    profiler: Option<Profiler>,
    /// The start board's clock face ([`Renderer::set_start_clock`]), when the stage shows one.
    start_board: Option<StartBoard>,
    /// The start board's lamps, green then red ([`Renderer::set_start_lamps`]).
    start_lamps: [Vec<StartLamp>; 2],
    /// The dials' textures ([`Image::Dial`]), the draws [`Renderer::draw_dials`] left for the next frame, and their pass
    /// (made by the first).
    dial_targets: BTreeMap<TextureRef, dials::gpu::Target>,
    dial_draws: Vec<dials::gpu::Draw>,
    dial_pass: Option<dials::gpu::Pass>,
    /// The cockpit's mirrors ([`Enhance::mirrors`], `crate::mirrors`).
    mirrors: mirrors::Mirrors,
    /// The large writes, submitted before the frame's first reads ([`Renderer::render`]).
    uploads: Uploads,
    /// The view of a frame of several drawn last ([`Renderer::render_view`]), and the others' history, by view.
    view: usize,
    parked: Vec<Option<views::Parked>>,
}

impl Renderer {
    /// The particle billboards and the water waves' triangles of the next frames, with `sprites` (the pass is made on
    /// the first call).
    pub fn set_particles(
        &mut self,
        sprites: &particles::gpu::Sprites,
        billboards: &[particles::sim::Billboard],
        waves: &[particles::wave::WaveVertex],
    ) {
        let pass = self.particles.get_or_insert_with(|| {
            let effects = self.effects.as_ref().map(|effects| &effects.layout);
            particles::gpu::Pass::new(&self.device, &self.queue, SCENE_FORMAT, DEPTH_FORMAT, sprites, effects)
        });
        pass.set(&self.device, &mut self.uploads, billboards);
        pass.set_waves(&self.device, &mut self.uploads, waves);
    }

    /// The additions drawn.
    pub fn enhance(&self) -> Enhance {
        self.enhance
    }

    /// The windscreen's drops of the next frames (the pass is made on the first call).
    pub fn set_glass(&mut self, drops: &[glass::gpu::Drop], wipers: glass::gpu::Wipers) {
        let pass = self.glass.get_or_insert_with(|| {
            let sun_map = self.sun.as_ref().map_or_else(|| sun::placeholder(&self.device), |sun| sun.view.clone());
            let may_halve = self.enhance.detail == Detail::High;
            glass::gpu::Pass::new(&self.device, SCENE_FORMAT, DEPTH_FORMAT, &sun_map, may_halve)
        });
        pass.set(&self.device, &mut self.uploads, drops);
        pass.set_wipers(wipers);
    }

    /// Lays the car's shadow decal ([`crate::car_shadow::decal`]) on `probe`'s ground for the car's body frame `frame`
    /// (physics axes) and the section's `Car_Deep_Shadow_Alpha`, with `texture` (`car_dpshad2.tga`) on the first call.
    /// Under [`Enhance::contact`] the contact shadow stands in for it.
    pub fn set_car_shadow(
        &mut self,
        texture: &rbr_formats::tga::Image,
        probe: &crate::stage_probe::StageProbe,
        frame: Mat4,
    ) {
        if self.enhance.contact {
            return;
        }
        let corners = crate::car_shadow::decal(probe, frame, self.atmosphere.car_deep_shadow);
        let shadow = self.car_shadow.get_or_insert_with(|| {
            let image = dds::Dds {
                width: texture.width,
                height: texture.height,
                format: Format::Rgba8,
                mip_count: 1,
                data: &texture.rgba,
            };
            crate::skidmarks_pass::SkidPass::decal(&self.device, &self.queue, SCENE_FORMAT, &image)
        });
        shadow.set_corners(&self.device, &mut self.uploads, &corners);
    }

    /// Sets the skid marks to draw from `quads`, with `texture` (`skidmarks.dds`) on the first call.
    pub fn set_skid_marks(&mut self, texture: &dds::Dds, quads: &[crate::skidmarks::Quad]) {
        let marks = self.skid_marks.get_or_insert_with(|| {
            crate::skidmarks_pass::SkidPass::new(&self.device, &self.queue, SCENE_FORMAT, texture)
        });
        marks.set_quads(&self.device, &mut self.uploads, quads);
    }

    /// Sets our tyre marks to draw from `quads` (`Enhance::marks`, in place of the skid marks).
    pub fn set_tyre_marks(&mut self, quads: &[crate::tyre_marks::Quad]) {
        let marks = self
            .skid_marks
            .get_or_insert_with(|| crate::skidmarks_pass::SkidPass::tyres(&self.device, &self.queue, SCENE_FORMAT));
        marks.set_tyre_quads(&self.device, &mut self.uploads, quads);
    }

    /// Starts measuring each offscreen frame's stages ([`Renderer::profile`]), slowing the frames.
    pub fn enable_profiling(&mut self) {
        self.profiler = Some(Profiler::new(&self.device, &self.queue, false));
    }

    /// Starts feeding the performance log ([`crate::perf_log`]) the stages' CPU times and the passes' GPU times, which
    /// [`Renderer::end_frame`] reads back; whether the GPU's are timed here.
    pub fn enable_perf_log(&mut self) -> bool {
        let profiler = Profiler::new(&self.device, &self.queue, true);
        let timed = profiler.gpu_timed();
        self.profiler = Some(profiler);
        timed
    }

    /// Ends a frame the caller submitted ([`Renderer::render`] and the commands after it): reads its GPU times back.
    pub fn end_frame(&mut self) {
        if let Some(profiler) = &mut self.profiler {
            profiler.end_frame();
        }
    }

    /// Each measured stage's median milliseconds per frame, since [`Renderer::enable_profiling`] (offscreen frames).
    pub fn profile(&self) -> Vec<(&'static str, f64)> {
        self.profiler.as_ref().map_or_else(Vec::new, Profiler::medians)
    }

    /// Draws the frame at `scale` of the target's width and height (at most 1, the default), which the temporal
    /// anti-aliasing upscales (without it, the post pass stretches it): a cheaper frame for high refresh rates at high
    /// resolutions.
    pub fn set_render_scale(&mut self, scale: f32) {
        self.render_scale = scale.clamp(0.25, 1.0);
    }

    /// Sets the car's dirt, which `CarPaint` faces blend their dirt map in by (`crate::dirt`).
    pub fn set_car_dirt(&mut self, dirt: crate::dirt::CarDirt) {
        self.car_dirt = dirt;
    }

    /// Sets whether the camera is outside the car (the reflections' interior flag).
    pub fn set_outside_view(&mut self, outside: bool) {
        self.outside_view = outside;
    }

    /// Draws the dials' `draws` into their textures ([`Image::Dial`]) at the start of the next frame.
    pub fn draw_dials(&mut self, draws: Vec<dials::gpu::Draw>) {
        self.dial_draws.extend(draws);
    }

    /// Drops the dials' queued draws without drawing them and submits the frame's uploads: frames a video span skips
    /// ([`Dials::redraw`] repaints) and the hidden window's. Unsubmitted writes keep their staging memory, which ran the
    /// GPU out of memory 6 minutes into a skipped stretch.
    pub fn skip_frame(&mut self) {
        self.dial_draws.clear();
        self.queue.submit(self.uploads.take());
        let _ = self.device.poll(wgpu::PollType::Poll);
    }

    /// Sets the time of day the start board's clock shows, s after midnight (`None`: its placeholder face).
    pub fn set_start_clock(&mut self, seconds: Option<f32>) {
        if let Some(board) = &mut self.start_board {
            board.set(&self.queue, seconds);
        }
    }

    /// An HDR display's headroom over its SDR white (1: none; [`present::HDR_FORMAT`](crate::present::HDR_FORMAT)),
    /// up to which the frame's highlights show.
    pub fn set_headroom(&mut self, headroom: f32) {
        self.post.set_headroom(headroom);
    }

    /// Lights the start board's lamps by the race clock (`None`: no race, unlit; [`start_board::lamps`]).
    pub fn set_start_lamps(&mut self, race: Option<&crate::live::RaceHud>) {
        for (lamps, lit) in self.start_lamps.iter_mut().zip(start_board::lamps(race)) {
            for lamp in lamps {
                lamp.set(&self.queue, lit);
            }
        }
    }

    /// The light and haze from here on (a stage's clock, `relight::Clock`). The renderer's resources for a time of
    /// day are made with it only when the stage's atmosphere at [`Renderer::new`] had a relight or was night.
    /// Whether the frame reads the physically based sky's tables ([`Enhance::lighting`]).
    fn skylight_on(&self) -> bool {
        self.enhance.lighting
    }

    /// Whether the sun is hidden: under an overcast sky with the sky's tables ([`Atmosphere::overcast`]), else as the
    /// game under `UseFog`.
    pub(super) fn sunless(&self) -> bool {
        if self.skylight_on() {
            self.atmosphere.overcast()
        } else {
            self.atmosphere.use_fog
        }
    }

    /// Draws the sky's tables for the atmosphere when it changed, in a submission of their own, before the reflection
    /// cube's faces read them.
    fn update_skylight(&self, profiler: &mut Option<Profiler>) {
        let Some(sky) = self.skylight_on().then(|| skylight::sky_uniform(&self.atmosphere)).flatten() else {
            return;
        };
        if self.skylight.holds(&sky) {
            return;
        }
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let t = frame::mark(profiler, &mut encoder);
        self.skylight.update(&self.queue, &mut encoder, &sky);
        frame::span(profiler, &mut encoder, "sky tables", t);
        self.queue.submit([encoder.finish()]);
    }

    /// The day's light from the sky's tables ([`skylight::DayLight`]), when the frame reads them.
    fn day_light(&self) -> Option<skylight::DayLight> {
        self.skylight_on().then(|| self.skylight.day(&self.device)).flatten()
    }

    /// Whether the volumetric clouds are drawn in place of the game's cloud faces: with the sky's tables, by day and
    /// night.
    pub(super) fn cloud_volume_shown(&self) -> bool {
        self.cloud_volume.on() && self.day_light().is_some()
    }

    /// Whether the game's cloud faces are left out: under the volumetric clouds, and with a cover chosen over the
    /// section's ([`Atmosphere::clouds`]), whose faces would show the section's sky.
    pub(super) fn cloud_faces_hidden(&self) -> bool {
        self.cloud_volume_shown() || (self.atmosphere.clouds.is_some() && self.skylight_on())
    }

    /// Sets the time in seconds the moving parts of the frame are drawn at (water, rain, clouds, stars).
    pub fn set_time(&mut self, seconds: f32) {
        self.time = seconds;
    }

    /// The car's lit lamps, which light the stage at night and at dusk ([`Atmosphere::night`],
    /// [`crate::relight::Relight::lamps`]); the first [`MAX_LAMPS`].
    pub fn set_lamps(&mut self, lamps: &[Lamp]) {
        self.lamps = [LampUniform::default(); MAX_LAMPS];
        for (slot, lamp) in self.lamps.iter_mut().zip(lamps) {
            *slot = LampUniform::of(lamp);
        }
        let air = self.air_lamps();
        if let Some(effects) = &mut self.effects {
            effects.set_lamps(air);
        }
    }

    /// The lamps as they light the air: the dust, the spray, the rain and the fog take them as the stage does, all at
    /// night, the dusk's share at dusk and none by day (the game lights neither; by day the tail lamps lit the dust
    /// behind the car as a red streak).
    fn air_lamps(&self) -> [LampUniform; MAX_LAMPS] {
        let dark = if self.atmosphere.night { 1.0 } else { self.atmosphere.relight.map_or(0.0, |r| r.lamps) };
        self.lamps.map(|l| LampUniform { colour: (Vec4::from(l.colour) * dark).to_array(), ..l })
    }

    /// Starts the temporal anti-aliasing's history again with the next frame ([`crate::taa`]): for a cut it cannot
    /// tell from the view alone.
    pub fn reset_history(&mut self) {
        if let Some(taa) = &mut self.taa {
            taa.reset();
        }
        self.reset_parked();
    }

    /// The car body's model space to stage space this frame.
    pub fn set_car_body(&mut self, model_to_stage: Mat4) {
        self.car_body = Some(model_to_stage);
        for mesh in &mut self.meshes {
            if let Some(model) = mesh.in_car {
                mesh.bounds = Some(meshes::placed_box(model_to_stage, model));
            }
        }
    }

    /// The car's shape, model space, the in-car view draws no particles in (`particles::cabin`).
    pub fn set_cabin_mask(&mut self, mask: Option<crate::particles::cabin::CabinMask>) {
        self.cabin_mask = mask;
    }

    /// How much sun gets into the car's cabin by its direction ([`Enhance::cabin`], `crate::car::cabin::SunIn`), which
    /// the cabin light bounces off its sunlit faces.
    pub fn set_cabin_sun(&mut self, sun_in: Option<crate::car::cabin::SunIn>) {
        self.cabin_sun = sun_in;
    }

    /// The rain or snow ([`Enhance::particles`]; `None` for none).
    pub fn set_precipitation(&mut self, precipitation: Option<particles::Precipitation>) {
        if let Some(effects) = &mut self.effects {
            effects.set_precipitation(precipitation);
        }
    }

    /// The eye's velocity in stage space, metres per second, for the rain's streaks in frames that do not move on in
    /// time ([`Enhance::particles`]; a screenshot). Moving frames measure it.
    pub fn set_eye_velocity(&mut self, velocity: Vec3) {
        if let Some(effects) = &mut self.effects {
            effects.set_velocity(velocity);
        }
    }

    /// The wind uniform ([`FrameUniform::wind`]): x the time, z how hard rain lands on the wet roads
    /// ([`Enhance::particles`]).
    fn wind(&self) -> [f32; 4] {
        let rain = self.effects.as_ref().map_or(0.0, |effects| effects.rain());
        let landing = if self.wetness >= 1.0 { rain } else { 0.0 };
        let (_, start) = cloud_volume::drift(&self.atmosphere);
        [self.time, start[0], landing, start[1]]
    }
}
