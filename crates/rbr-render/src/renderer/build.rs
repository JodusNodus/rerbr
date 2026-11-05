//! Building a [`Renderer`]: uploading the stage and making its pipelines and bind groups.

use super::enhance::{AntiAliasing, Detail, Enhance};
use super::gpu::Gpu;
use super::meshes::mesh_bounds;
use super::meshes::{transformed_vertices, GpuDraw, GpuMesh, InteractiveGpu, Packing};
use super::pipelines::{cutout, layouts, prepassed, vertex_entry, Context, DrawPass, PipelineKey};
use super::reflection::{reflects, Reflection, ScreenReflection};
use super::shadows::shadow_map;
use super::sun_mask::SunMask;
use super::textures::dds_texture;
use super::textures::{
    bind_group, bind_key, depth_view, luminance_texture, solid_texture, Glow, Samplers, Views, CLEAR, WHITE,
};
use super::uniforms::{frame_index, FrameBinding, FrameUniform};
use super::uniforms::{LampUniform, MAX_LAMPS};
use super::Renderer;
use super::SHADER;
use super::{bounce, cloud_volume, meshes, mirrors, motion, occlusion, pipelines, probes, reflection, shadows};
use crate::dials;
use crate::effects::Effects;
use crate::foliage;
use crate::parallel::in_parallel;
use crate::post::{Post, SCENE_FORMAT};
use crate::puddles;
use crate::skylight::SkyLight;
use crate::stage::{Anchor, Draw, Image, Layer, Lod, Mesh, Stage, Weather};
use crate::start_board::{self, StartBoard, StartLamp};
use crate::sun::{self, SunShadow};
use crate::terrain_detail;
use crate::uploads::Uploads;
use glam::{Mat4, Vec3};
use rbr_formats::dds::{self};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

impl Renderer {
    /// Uploads `stage` and builds the pipelines and bind groups its draws use, for colour targets of
    /// `format`, with the additions in `enhance`.
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat, stage: &Stage, enhance: Enhance) -> Renderer {
        Renderer::with_progress(gpu, format, stage, enhance, &mut |_| {})
    }

    /// [`Renderer::new`], telling `progress` the share of the stage's meshes made so far, 0 to 1: they take most of
    /// the time (3.2 s of 3.3 s on Semetin, M3 Pro).
    pub fn with_progress(
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        stage: &Stage,
        enhance: Enhance,
        progress: &mut dyn FnMut(f32),
    ) -> Renderer {
        let device = gpu.device.clone();
        let queue = gpu.queue.clone();
        let output_format = format;
        let format = SCENE_FORMAT;

        let [frame_layout, shadow_layout, texture_layout] = layouts(&device);
        let (shadow_view, shadow_layers, shadow_sampler) = shadow_map(&device);

        // A stage drawn at a time of day (`relight`, `Atmosphere::night`) needs the normals the baked draws do not
        // store, and at night the headlights' shadows use the sun's first cascade.
        let timed = stage.atmosphere.relight.is_some() || stage.atmosphere.night;
        let quality = sun::Quality::of(&enhance);
        let sun = (enhance.lighting || timed).then(|| SunShadow::new(&device, &texture_layout, quality));
        let sun_range = sun.as_ref().map(|sun| shadows::ShadowRange::sun(&device, &queue, &sun.view, quality));
        let sun_view = sun.as_ref().map_or_else(|| sun::placeholder(&device), |sun| sun.view.clone());
        let taa = enhance.anti_aliasing == AntiAliasing::Taa;
        let ultra = enhance.detail == Detail::Ultra;
        let surroundings_side = if ultra { reflection::ULTRA_SIZE } else { reflection::REFLECTION_SIZE };
        let reflection = Reflection::new(&device, format, reflection::REFLECTION_SIZE);
        let surroundings = Reflection::new(&device, format, surroundings_side);
        let probes = probes::Probes::new(&device, format, &stage.driveline, enhance.lighting, surroundings_side);
        let skylight = SkyLight::new(&device);
        let cube_view = |cube: &Reflection| {
            cube.cube.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            })
        };
        let (reflection_view, surroundings_view) = (cube_view(&reflection), cube_view(&surroundings));
        let reflection_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("car reflection"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let mirrors = mirrors::Mirrors::new(&device, format, enhance.mirrors, enhance.windscreen);
        let ground_light = bounce::Ground::new(&device, stage, enhance.lighting);
        let world_box = (stage.meshes.iter().filter(|m| m.anchor == Anchor::World).flat_map(|m| &m.vertices))
            .map(|v| Vec3::from(v.position))
            .filter(|&p| crate::coords::on_stage(p))
            .fold(None, |b: Option<(Vec3, Vec3)>, p| Some(b.map_or((p, p), |(low, high)| (low.min(p), high.max(p)))));
        let shadow_range = shadows::ShadowRange::new(&device, &shadow_view);
        // The volumetric clouds' shadow map ([`cloud_volume::shadow_target`]), read by every draw.
        let (cloud_shadow_view, cloud_shadow_sampler) = cloud_volume::shadow_target(&device);
        let frames = [(); 3].map(|_| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frame"),
                size: std::mem::size_of::<FrameUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("frame"),
                layout: &frame_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&shadow_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&shadow_sampler) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&reflection_view) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&reflection_sampler) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&sun_view) },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::TextureView(&skylight.sky_view),
                    },
                    wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(&skylight.aerial) },
                    wgpu::BindGroupEntry {
                        binding: 9,
                        resource: wgpu::BindingResource::TextureView(&skylight.cube_view),
                    },
                    wgpu::BindGroupEntry { binding: 10, resource: skylight.light.as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 11,
                        resource: wgpu::BindingResource::TextureView(&surroundings_view),
                    },
                    wgpu::BindGroupEntry { binding: 12, resource: wgpu::BindingResource::TextureView(&mirrors.view) },
                    wgpu::BindGroupEntry { binding: 13, resource: mirrors.uniform.as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 14,
                        resource: wgpu::BindingResource::TextureView(&ground_light.view),
                    },
                    wgpu::BindGroupEntry { binding: 15, resource: ground_light.cells.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 16, resource: wgpu::BindingResource::TextureView(&probes.view) },
                    wgpu::BindGroupEntry { binding: 17, resource: probes.buffer.as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 18,
                        resource: wgpu::BindingResource::TextureView(&shadow_range.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 19,
                        resource: wgpu::BindingResource::TextureView(&cloud_shadow_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 20,
                        resource: wgpu::BindingResource::Sampler(&cloud_shadow_sampler),
                    },
                ],
            });
            FrameBinding { buffer, bind_group }
        });
        let shadow_frame = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow frame"),
            layout: &shadow_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: frames[0].buffer.as_entire_binding() }],
        });

        let samplers = Samplers::new(&device, if enhance.anisotropy { 16 } else { 1 });
        let mut dial_targets = BTreeMap::new();
        let mut dial_bases: Vec<(&str, &[u8], wgpu::TextureView)> = Vec::new();
        let mut glows = BTreeMap::new();
        // `RERBR_NO_TERRAIN_DETAIL` leaves the terrain detail out (A/B timing; `terrain_detail`). Its windows over the
        // machine's threads: one after another they took 0.5 s of the load on 580.
        let detail = if std::env::var_os("RERBR_NO_TERRAIN_DETAIL").is_none() {
            let dds: Vec<_> = (stage.textures.iter())
                .filter_map(|(&key, texture)| match texture.image.as_ref()? {
                    Image::Dds(bytes) => Some((key, bytes)),
                    _ => None,
                })
                .collect();
            let windows =
                in_parallel(&dds, |&(key, bytes)| Some((key, terrain_detail::window(&dds::parse(bytes).ok()?)?)));
            windows.into_iter().flatten().collect()
        } else {
            BTreeMap::new()
        };
        let mut views = Views {
            loaded: stage
                .textures
                .iter()
                .filter_map(|(&key, texture)| {
                    let texture = match texture.image.as_ref()? {
                        Image::Dds(bytes) => dds_texture(&device, &queue, &dds::parse(bytes).ok()?),
                        Image::Solid(texel) => solid_texture(&device, &queue, *texel),
                        Image::Shadow { width, height, pixels } => {
                            luminance_texture(&device, &queue, *width, *height, pixels)
                        }
                        Image::Dial { base, glow, .. } => {
                            if let Some(mask) = glow.as_deref().and_then(|bytes| dds::parse(bytes).ok()) {
                                // DXT1 has no alpha to read (its texels are opaque).
                                let view = dds_texture(&device, &queue, &mask).create_view(&Default::default());
                                glows.insert(key, Glow { view, alpha: mask.format != dds::Format::Dxt1, day: false });
                            }
                            // The textures of one file with one base (a face's, each drawn the same) share a target,
                            // drawn on by the first.
                            let shared = (dial_bases.iter()).find(|(r, b, _)| *r == texture.request && *b == base);
                            if let Some((_, _, view)) = shared {
                                return Some((key, view.clone()));
                            }
                            let target =
                                dials::gpu::Target::new(&device, dds_texture(&device, &queue, &dds::parse(base).ok()?));
                            let view = target.view();
                            dial_bases.push((&texture.request, base, view.clone()));
                            dial_targets.insert(key, target);
                            return Some((key, view));
                        }
                    };
                    Some((key, texture.create_view(&Default::default())))
                })
                .collect(),
            white: solid_texture(&device, &queue, WHITE).create_view(&Default::default()),
            clear: solid_texture(&device, &queue, CLEAR).create_view(&Default::default()),
            glows,
            detail,
            banks: terrain_detail::bank_shares(&stage.meshes),
        };
        let start_board = stage.start_board().and_then(|texture| {
            let Some(Image::Dds(bytes)) = stage.textures.get(&texture)?.image.as_ref() else { return None };
            let placeholder = start_board::Rgba::decode(&dds::parse(bytes).ok()?);
            let digits =
                stage.start_digits.as_deref().and_then(|b| dds::parse(b).ok()).map(|d| start_board::Rgba::decode(&d));
            let board = StartBoard::new(&device, &queue, placeholder, digits);
            views.loaded.insert(texture, board.face.view());
            views.glows.insert(texture, Glow { view: board.face.glow_view(), alpha: false, day: false });
            Some(board)
        });
        let start_lamps = start_board::LAMPS.map(|name| {
            stage
                .textures_ending(name)
                .filter_map(|texture| {
                    let Some(Image::Dds(bytes)) = stage.textures.get(&texture)?.image.as_ref() else { return None };
                    let lamp = StartLamp::new(&device, &queue, start_board::Rgba::decode(&dds::parse(bytes).ok()?));
                    views.loaded.insert(texture, lamp.face.view());
                    views.glows.insert(texture, Glow { view: lamp.face.glow_view(), alpha: false, day: true });
                    Some(lamp)
                })
                .collect::<Vec<_>>()
        });

        let mut bind_groups = HashMap::new();
        let mut pipelines = HashMap::new();
        let mut meshes = Vec::new();
        let mut packing = Packing::default();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stage"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let mask_layout = SunMask::layout(&device);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stage"),
            bind_group_layouts: &[Some(&frame_layout), Some(&texture_layout), Some(&mask_layout)],
            immediate_size: 0,
        });
        let context = Context {
            device: &device,
            format,
            shader: &shader,
            layout: &layout,
            vertex: vertex_entry(enhance),
            water_plus: enhance.water || enhance.wet,
            cabin: enhance.cabin,
            grass: enhance.grass,
            lighting: enhance.lighting,
            debug_view: enhance.debug_view.code(),
            cube_shift: u32::from(ultra),
            taa,
        };
        // A draw of a mesh whose indices start at `first_index` in the shared buffer, with its pipelines and bind group.
        let mut gpu_draw = |draw: &Draw,
                            first_index: u32,
                            casts_shadow: bool,
                            mirror: bool,
                            (anchor, layer): (Anchor, Layer),
                            cabin_set,
                            wet_road: bool| {
            let bind = bind_key(&draw.state, &views);
            let pipeline = PipelineKey::of(&draw.state, mirror, layer == Layer::Sky)
                .drawing(bind.features())
                .in_cabin(cabin_set)
                .framed(frame_index(anchor, layer));
            let viewed = pipeline.takes_views().then(|| {
                [false, true].map(|inside| {
                    let key = pipeline.viewed(inside);
                    pipelines.entry(key).or_insert_with(|| context.pipeline(key)).clone()
                })
            });
            let mut pipeline_in = |pass: DrawPass| {
                let key = pipeline.in_pass(pass);
                pipelines.entry(key).or_insert_with(|| context.pipeline(key)).clone()
            };
            // A lit draw is only drawn with its views' pipelines: its `main` is the outside one, not a third to compile.
            let main = viewed.as_ref().map_or_else(|| pipeline_in(DrawPass::Main), |[outside, _]| outside.clone());
            // The car's lit draws are shaded after the prepass by their outside view's pipeline.
            let prepassed = prepassed(&draw.state, context.lighting, pipeline.cabin()).then(|| {
                let lit = pipeline.takes_views();
                [DrawPass::DepthPrepass, DrawPass::AfterPrepass].map(|pass| {
                    let key = if lit && pass == DrawPass::AfterPrepass {
                        pipeline.viewed(false).in_pass(DrawPass::CarMain)
                    } else {
                        pipeline.in_pass(pass)
                    };
                    pipelines.entry(key).or_insert_with(|| context.pipeline(key)).clone()
                })
            });
            let lamps = pipeline.takes_lamps().then(|| {
                [DrawPass::Main, DrawPass::AfterPrepass].map(|pass| {
                    let key = pipeline.with_lamps().in_pass(pass);
                    pipelines.entry(key).or_insert_with(|| context.pipeline(key)).clone()
                })
            });
            let group = bind_groups
                .entry(bind)
                .or_insert_with(|| bind_group(&device, &texture_layout, bind, &views, &samplers))
                .clone();
            GpuDraw {
                pipeline,
                main,
                prepassed,
                lamps,
                views: viewed,
                group,
                casts_shadow,
                bind,
                wet_road,
                lod: draw.lod,
                indices: first_index + draw.indices.start as u32..first_index + draw.indices.end as u32,
            }
        };

        let mut slots = Vec::with_capacity(stage.meshes.len());
        let cut_out =
            if enhance.lighting { foliage::cut_out(stage, |draw| cutout(&draw.state)) } else { HashMap::new() };
        let plant = |draw: &Draw| cut_out.get(&draw.state.textures[0]?).filter(|_| cutout(&draw.state));
        let ground = stage.atmosphere.relight.map(|_| crate::relight::Ground::of(stage));
        // The puddles show in wet weather only (`wet_plus`).
        let heights = (enhance.wet && matches!(stage.selection.weather, Weather::Wet))
            .then(|| puddles::Heights::of(stage.meshes.iter().map(|mesh| (mesh, &mesh.vertices[..]))));
        // The interactive props go after the solid meshes and before the blended ones: the car's glass, which writes
        // no depth, is drawn over them as over the rest of the stage. Drawn after it, a sign behind the car showed
        // through its windows untinted, as if in the cabin.
        let first_blended = stage.meshes.iter().position(|mesh| mesh.layer == Layer::Blended);
        let (solid, blended) = stage.meshes.split_at(first_blended.unwrap_or(stage.meshes.len()));
        let mut interactive_slots = HashMap::with_capacity(stage.interactive.len());
        let over = crate::sky_dome::Over::of(&stage.meshes, &stage.driveline);
        // A stage mesh's vertices as drawn, when they are not its own: the sun's light needs the normals the baked stage
        // draws do not store, the sun's shadows the plants. The grass's tile meshes (`Lod::Close`) have none to light:
        // `Renderer::stream_grass` gives them their cards' vertices. Their zeroed ones, all in one place, made grouping
        // the plants into trees quadratic: 5 s of the load on 580.
        let lit = enhance.lighting || enhance.wet || timed;
        let prepare = |mesh: &Mesh| {
            let grass = mesh.draws.iter().all(|draw| draw.lod == Lod::Close);
            (lit && !grass).then(|| {
                let mut vertices = sun::with_normals(mesh);
                // The fences carry their own light (`fences::FENCE`).
                if mesh.anchor == Anchor::World && !crate::fences::is_fence(mesh) {
                    if let Some(ground) = &ground {
                        vertices = ground.on_foliage(vertices, mesh);
                    }
                    vertices = foliage::with_plants(vertices, mesh, plant);
                    if let Some(heights) = &heights {
                        vertices = puddles::with_lowness(vertices, mesh, heights);
                    }
                }
                vertices
            })
        };
        // Made over the load's threads 1024 meshes at a time, bounding the vertices held; the uploads stay in order.
        // One after another they took 1.2 s of the load on 580.
        let mut prepared = stage.meshes.chunks(1024).flat_map(|part| in_parallel(part, prepare));
        let meshes_and_props = solid.iter().map(Some).chain([None]).chain(blended.iter().map(Some));
        for (done, mesh) in meshes_and_props.enumerate() {
            progress(done as f32 / (stage.meshes.len() + 1) as f32);
            let Some(mesh) = mesh else {
                for instance in &stage.interactive {
                    let mesh = instance.mesh.as_ref();
                    if mesh.draws.is_empty() {
                        continue;
                    }
                    let placement = Mat4::from_cols_array(&instance.placement);
                    let vertices = transformed_vertices(&mesh.vertices, placement);
                    let bounds = mesh_bounds(&vertices);
                    let (slot, first_index) = packing.add(&device, &vertices, &mesh.indices, false);
                    let draws = mesh
                        .draws
                        .iter()
                        .map(|draw| {
                            gpu_draw(draw, first_index, false, false, (Anchor::World, Layer::Solid), None, false)
                        })
                        .collect();
                    interactive_slots.insert(instance.handle, meshes.len());
                    meshes.push(GpuMesh {
                        bounds: bounds.map(|b| (b.center, b.half_extent)),
                        cell: instance.cell.map(|b| (b.center, b.half_extent)),
                        visible: true,
                        outside: None,
                        reflected: false,
                        anchor: Anchor::World,
                        layer: Layer::Solid,
                        slot,
                        draws,
                        interactive: Some(InteractiveGpu {
                            placement: instance.placement,
                            local_mesh: Arc::clone(&instance.mesh),
                            current_pose: None,
                        }),
                        in_car: None,
                        car: false,
                        sky_dome: false,
                        backdrop_bounds: None,
                    });
                }
                continue;
            };
            let drawn = prepared.next().expect("one per stage mesh");
            // Under the physically based sky a sky dome is left out: the sky is ours (`crate::sky_dome`).
            let sky_dome = over.as_ref().is_some_and(|o| o.roofed_by(mesh));
            if mesh.draws.is_empty() || (sky_dome && enhance.lighting) {
                slots.push(None);
                continue;
            }
            slots.push(Some(meshes.len()));
            let backdrop = mesh.anchor == Anchor::World && mesh.layer == Layer::Sky;
            let drawn = drawn.as_deref().unwrap_or(&mesh.vertices);
            let (slot, first_index) = packing.add(&device, drawn, &mesh.indices, backdrop);
            // A mirror's glass (`mirrors::mark`): a lit draw with a marked corner.
            let mirror = |draw: &Draw| {
                enhance.mirrors != crate::mirrors::Rate::Off
                    && draw.state.shade.is_some()
                    && mesh.indices[draw.indices.clone()].iter().any(|&i| mesh.vertices[i as usize].specular[0] >= 1.0)
            };
            // The ground whose surface is road (`puddles::mark_roads`), with the wet film.
            let wet_road = |draw: &Draw| {
                enhance.wet
                    && draw.state.shade.is_none()
                    && draw.state.mip_bias
                    && mesh.indices[draw.indices.clone()].iter().any(|&i| mesh.vertices[i as usize].wet > 0.0)
            };
            // Whether all of a draw's vertices are the cabin's interior set's (`car::cabin::INTERIOR` and up), or none
            // are: `None` when some are.
            let cabin_set = |draw: &Draw| {
                let indices = &mesh.indices[draw.indices.clone()];
                let interior = |i: &u32| mesh.vertices[*i as usize].specular[1] >= crate::car::cabin::INTERIOR;
                match (indices.iter().all(interior), indices.iter().any(interior)) {
                    (true, _) => Some(true),
                    (_, false) => Some(false),
                    _ => None,
                }
            };
            let draws = mesh
                .draws
                .iter()
                .map(|draw| {
                    let casts_shadow = draw.state.shade.is_some() && draw.state.depth_write;
                    let placed = (mesh.anchor, mesh.layer);
                    gpu_draw(draw, first_index, casts_shadow, mirror(draw), placed, cabin_set(draw), wet_road(draw))
                })
                .collect();
            meshes.push(GpuMesh {
                bounds: mesh.bounds.map(|b| (b.center, b.half_extent)),
                cell: mesh.cell.map(|b| (b.center, b.half_extent)),
                visible: true,
                outside: None,
                reflected: false,
                anchor: mesh.anchor,
                layer: mesh.layer,
                slot,
                draws,
                interactive: None,
                in_car: None,
                car: false,
                sky_dome,
                backdrop_bounds: mesh_bounds(&mesh.vertices)
                    .filter(|_| backdrop && mesh.bounds.is_none())
                    .map(|b| (b.center, b.half_extent)),
            });
        }

        let buffers = packing.finish(&device);
        let ground_light = enhance.lighting.then(|| {
            let start = std::time::Instant::now();
            let tiles = occlusion::bake(&device, &queue, &texture_layout, &meshes, &buffers, &stage.driveline);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            eprintln!("sky occlusion: {tiles} tiles in {:.1} s", start.elapsed().as_secs_f32());
            let toward = -stage.atmosphere.sun_direction;
            bounce::bake(&device, &queue, &texture_layout, &meshes, &buffers, &ground_light, toward);
            ground_light
        });
        let depth_size = (1, 1);
        let depth_view = depth_view(&device, depth_size);
        let cloud_volume = cloud_volume::CloudVolume::new(
            &context,
            &queue,
            &frame_layout,
            (&depth_view, depth_size),
            enhance.lighting,
            &frames[0].buffer,
            cloud_shadow_view,
        );
        let sky = context.sky_pipeline(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky"),
            bind_group_layouts: &[Some(&frame_layout), Some(cloud_volume.sky_layout())],
            immediate_size: 0,
        }));
        let shadow_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("car shadow"),
            bind_group_layouts: &[Some(&shadow_layout)],
            immediate_size: 0,
        });
        let shadow_pipelines =
            ["vs_shadow", "vs_ground"].map(|entry| context.shadow_pipeline(&shadow_pipeline_layout, entry));
        let sun_mask = SunMask::new(
            &context,
            (mask_layout, &frame_layout),
            &depth_view,
            sun_range.as_ref().map(|range| &range.view),
            depth_size,
            quality,
        );
        let draws = || meshes.iter().flat_map(|mesh| &mesh.draws);
        let wetness = stage.selection.weather.wetness();
        let rays = enhance.reflections.rays().filter(|_| draws().any(|draw| reflects(draw, wetness)));
        let screen_reflection = rays.map(|rays| {
            ScreenReflection::new(
                &device,
                (&shader, vertex_entry(enhance), "fs_reflect", rays, pipelines::alpha_writes(taa, false)),
                [&frame_layout, &texture_layout],
                format,
                depth_size,
                &depth_view,
                ultra,
            )
        });
        let post = Post::new(
            &device,
            output_format,
            depth_size,
            &depth_view,
            enhance.lighting,
            taa,
            enhance.debug_view.code(),
        );
        let light_shafts = enhance.lighting.then(|| {
            let clouds = cloud_volume.shadow_map();
            crate::light_shafts::LightShafts::new(&device, &post.scene, depth_size, &depth_view, &sun_view, clouds)
        });
        let effects = enhance.particles.then(|| Effects::new(&device, format, &depth_view, &sun_view));
        let motion = taa.then(|| motion::Motion::new(&device, &shader, &shadow_layout, buffers.len()));
        let taa = motion.as_ref().map(|m| crate::taa::Taa::new(&device, depth_size, depth_size, &depth_view, m.view()));
        Renderer {
            uploads: Uploads::new(&device),
            device,
            queue,
            format: output_format,
            enhance,
            post,
            taa,
            motion,
            light_shafts,
            effects,
            sun,
            cascades: None,
            world_box,
            world: None,
            world_drawn: None,
            far_drawn: None,
            time: 0.0,
            air_frame: 0,
            lamps: [LampUniform::default(); MAX_LAMPS],
            lamp_matrix: None,
            profiler: None,
            start_board,
            start_lamps,
            dial_targets,
            dial_draws: Vec::new(),
            dial_pass: None,
            buffers,
            frames,
            casters: meshes::casters(&meshes),
            meshes,
            slots,
            interactive_slots,
            grass: None,
            atmosphere: stage.atmosphere.clone(),
            ground_light,
            wetness,
            car_dirt: Default::default(),
            cabin_sun: None,
            car_body: None,
            cabin_mask: None,
            outside_view: false,
            screen_reflection,
            skid_marks: None,
            car_shadow: None,
            sky,
            depth_size,
            output_size: depth_size,
            render_scale: 1.0,
            depth_view,
            sun_mask,
            cloud_volume,
            shadow_params: std::cell::Cell::new([0.0; 4]),
            shadow_layers,
            shadow_pipelines,
            shadow_frame,
            shadow_range,
            sun_range,
            reflection,
            surroundings,
            probes,
            skylight,
            fog_ground: enhance.lighting.then(|| crate::height_fog::Ground::of(&stage.meshes)).flatten(),
            particles: None,
            glass: None,
            mirrors,
        }
    }
}
