//! BTB (Bob's Track Builder) stages: [`rbr_btb::scene::Scene`] as a [`Stage`]. A stage prepared by
//! `rbr_btb::stage::prepare` (its folder's `btb.ini` names the BTB folder), or named `<BTB folder>/track-<n>_<L>` directly
//! (for development), draws from the track folder instead of `Maps/`. What each effect and
//! technique becomes, and what is missing: `notes/btb.md` "Drawing".
//!
//! Every placement is expanded into world-space vertices, grouped by 128 m cells, `LODIn` and `LODOut` (a mesh each,
//! drawn between those distances: [`Lod::Band`]) so that the views cull them by their bounds, except the objects that
//! ask to be instanced (`ReRbrInstanced=1`, [`instanced`]): their mesh is stored once and each placement is a transform
//! ([`placed`]), grouped the same way.

use super::mesh::{Draw, DrawState, Highlight, Layer, Layers, Light, Lod, Mesh, NormalMap, Sheen, Surface, Vertex};
use super::placed::{self, bounds_and_cell, Instance, AMBIENT};
use super::{
    Address, Anchor, Error, GrassFx, GrassOccluder, LoadReport, Pose, Selection, Sky, Stage, StageTexture, TextureName,
    TextureRef, TextureSet,
};
use crate::atmosphere::Atmosphere;
use glam::{Vec3, Vec3Swizzles};
use rbr_btb::scene::{self, Alpha, Scene};
use rbr_formats::stage_files;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// The BTB track folder stage `name` plays: a prepared folder's `btb.ini` source (`rbr_btb::stage::prepare`), or the
/// track folder itself (`<folder>/track-<n>_<L>`, for development).
fn source(name: &str) -> Option<PathBuf> {
    let folder = stage_files::folder(name)?;
    rbr_btb::stage::source(folder).or_else(|| rbr_btb::track::is_track_folder(folder).then(|| folder.to_owned()))
}

/// Whether stage `name` is a BTB stage's.
pub(super) fn is_btb(name: &str) -> bool {
    source(name).is_some()
}

/// The BTB stage folder of stage `name` and its `track.ini`, when it is a BTB stage's.
pub(super) fn track(name: &str) -> Option<rbr_btb::track::Track> {
    rbr_btb::track::Track::open(&source(name)?).ok()
}

/// Side of the cells small placements are grouped by, metres.
const CELL: f32 = 128.0;
/// A placement wider than this (half extent, metres) is a mesh of its own: the terrain tiles.
const SOLO: f32 = 150.0;
/// Distances of [`Lod::Band`] are kept to this step, metres.
const STEP: f32 = 50.0;
/// How much farther than the source's `LODIn` and `LODOut` a band reaches (ours). A mesh is a 128 m cell of placements,
/// so it shows all at once: AC conversions' cells came in 60 to 100 px tall at 1080p at their 1000 m cap and authored
/// 250 to 700 m. Half as far again costs 0.2 to 0.7 ms a frame there (twice, up to 2 ms); both ends, so a stage's
/// detailed and plain versions of a placement still meet.
const REACH: f32 = 1.5;
/// The alpha test of a transparent material's second pass (`AlphaRef` 200, `GREATEREQUAL`): texels at or under this
/// are dropped.
const SOLID_REF: u8 = 199;
/// The alpha test of a cut-out texture's one draw (ours): the half-covered contour of the original's blended fringe.
/// Blended, the fringe wrote no depth, and the temporal anti-aliasing carried it by what lay behind: plants ghosted.
const CUTOUT_REF: u8 = 127;
/// How far over the driveline an author's own sky dome passes, metres: a mesh with faces that far over each of
/// [`DOME_POINTS`] points of it is drawn as the sky ([`Layer::Sky`]).
const DOME_CLEARANCE: f32 = 50.0;
/// The driveline points a dome must pass over.
const DOME_POINTS: usize = 5;
/// How far, metres, a converted stage's drawn triangle may lie off its collision ground and still be on it
/// ([`on_ground`]).
const ON_GROUND: f32 = 0.25;
/// A multilayer material's textures and numbers for the shader. In world space each detail's offset is the stage origin's
/// share of `(x, z) * scale` (AC's `x` is the stage's plus the origin's, its `z` the stage's negated, less the origin's), kept
/// as the fraction the texture repeats at so that the shader's `f32` holds it.
fn layered(l: &scene::Layers, texture: &mut impl FnMut(&Option<String>) -> Option<TextureRef>) -> Layers {
    let mut named = |name: &String| texture(&Some(name.clone())).expect("a name gives a texture");
    let offset = |i: usize| match l.world {
        Some([x, z]) => {
            [(x * f64::from(l.scale[i][0])).rem_euclid(1.0), (-z * f64::from(l.scale[i][1])).rem_euclid(1.0)]
                .map(|o| o as f32)
        }
        None => [0.0; 2],
    };
    Layers {
        diffuse: named(&l.diffuse),
        mask: named(&l.mask),
        details: std::array::from_fn(|i| named(&l.details[i])),
        world: l.world.is_some(),
        magic: l.magic,
        scale: l.scale,
        offset: std::array::from_fn(offset),
    }
}

/// The fewest placements of an object that `RBR_BTB_INSTANCE=auto` draws as instances.
const AUTO_MIN: usize = 8;
/// The triangles of a placement [`on_ground`] samples, at most.
const GROUND_SAMPLES: usize = 64;

/// Loads the BTB stage `name`: geometry, draws, texture names (no textures yet: [`load_textures`]), start, driveline
/// and sun.
pub(super) fn load(maps: &Path, name: &str, selection: Selection) -> Result<Stage, Error> {
    let folder = &source(name).ok_or_else(|| Error::Btb(format!("{name}: no BTB folder")))?;
    let scene = Scene::load(folder).map_err(Error::Btb)?;
    let mut report = LoadReport::default();

    // The textures the draws sample: a diffuse one and, for the lerp, the second.
    let mut refs: HashMap<String, TextureRef> = HashMap::new();
    let mut texture_names = BTreeMap::new();
    let mut texture = |file: &Option<String>| -> Option<TextureRef> {
        let file = file.as_ref()?;
        let next = TextureRef::stage(refs.len() as u32);
        let texture = *refs.entry(file.clone()).or_insert(next);
        texture_names.entry(texture).or_insert_with(|| TextureName {
            track: folder.to_string_lossy().into_owned(),
            file_name: file.clone(),
            ground: false,
        });
        Some(texture)
    };

    // A converted stage's grass atlas, where it ships one: a stage texture like the draws'.
    let mut grass_fx = scene.grass.clone().map(|settings| {
        let texture = texture(&settings.texture);
        GrassFx { settings, texture, occluders: Vec::new() }
    });

    // One plan per mesh: the draws it makes, with their layers.
    let mut alphas: HashMap<&str, Alpha> = HashMap::new();
    let plans: Vec<Option<Plan>> = scene
        .meshes
        .iter()
        .map(|mesh| {
            let material = &scene.materials[mesh.material?];
            if material.effect.sky || mesh.indices.is_empty() {
                return None;
            }
            let lerp = material.effect.vert_lerp && material.second.is_some() && !mesh.blend.is_empty();
            // A multilayer material drawn per pixel (`ReRbrLayer*`) keeps the bake of the sum as its first texture, which
            // the grass and the ground detail read.
            let layers = material.layers.as_ref().filter(|_| !material.technique.transparent);
            let first = texture(&material.diffuse);
            let second = if lerp { texture(&material.second) } else { None };
            // Two passes only where the texture is translucent; a cut-out one is alpha tested, an opaque one solid.
            let alpha = match (material.technique.transparent, &material.diffuse) {
                (false, _) => Alpha::Opaque,
                (true, None) => Alpha::Translucent,
                (true, Some(file)) => *alphas.entry(file).or_insert_with(|| scene.texture_alpha(file)),
            };
            let slots = material.uv_slots();
            let has = |slot: Option<usize>| slot.filter(|&s| s < mesh.uvs.len());
            // The normal map on `uv0` or, where the lerp does not hold it, `uv1`.
            let uv_bump = has(slots.bump).filter(|&s| s == 0 || !lerp);
            let normal = uv_bump.filter(|_| material.effect.bump && !material.technique.up).and_then(|s| {
                let texture = texture(&material.normal)?;
                Some(NormalMap { texture, uv1: s != 0, doubled: !material.first_uv, scale: material.bump_scale })
            });
            // The highlight on the opaque draws only: a cut-out's `specular` marks our plants (`foliage`).
            let colour = material.specular_color;
            let opaque = matches!(alpha, Alpha::Opaque);
            let uv_specular = has(slots.specular).filter(|_| material.effect.specular && opaque);
            let highlight = uv_specular.filter(|_| colour.iter().any(|&c| c > 0.0)).map(|_| Highlight {
                colour,
                power: material.specular_power.max(1.0),
                map: texture(&material.specular),
            });
            let light = Light(material.light);
            // The sheen on the opaque draws only (a road is no cut-out).
            let sheen = material.sheen.filter(|_| opaque).map(|roughness| Sheen { roughness });
            let layers = layers.map(|l| layered(l, &mut texture));
            let surface = (normal.is_some()
                || highlight.is_some()
                || sheen.is_some()
                || layers.is_some()
                || light != Light::NONE)
                .then_some(Surface { normal, highlight, sheen, light, layers });
            let state = |alpha_ref, blend| DrawState {
                textures: [first, second],
                shadow: None,
                address: [Address::Wrap; 2],
                second_texture: second.is_some(),
                alpha_ref,
                mip_bias: false,
                ground: false,
                grass: match material.grass {
                    scene::Grass::Grows(profile) => Some(profile),
                    _ => None,
                },
                cull_back: false,
                blend,
                depth_test: true,
                depth_write: !blend,
                shade: None,
                specular: None,
                surface,
            };
            let draws = match alpha {
                Alpha::Translucent => {
                    vec![(Layer::Blended, state(None, true)), (Layer::Solid, state(Some(SOLID_REF), false))]
                }
                Alpha::Cutout => vec![(Layer::Solid, state(Some(CUTOUT_REF), false))],
                Alpha::Opaque => vec![(Layer::Solid, state(None, false))],
            };
            let uv_bump = uv_bump.filter(|&s| normal.is_some() && s != 0);
            let uv_specular = uv_specular.filter(|_| highlight.is_some());
            let hides = match material.grass {
                scene::Grass::Hides => Some(None),
                scene::Grass::HidesAlpha => Some(first),
                _ => None,
            };
            Some(Plan { draws, lerp, up: material.technique.up, uv_lerp: slots.lerp, uv_bump, uv_specular, hides })
        })
        .collect();

    // A converted stage (`rbr-ac-tracks`: Assetto Corsa; BeamNG's converter alike) collides with meshes of their own (`CollisionOnly`) and
    // draws none of them: its ground is what lies on them ([`on_ground`]), and its surfaces tell its road.
    let converted = scene.objects.iter().any(|o| o.collides && !o.visible);
    let loaded = converted.then(|| super::collision(maps, name, selection)).flatten();
    let sun = scene.sun_direction.map_or(Vec3::new(0.6, -0.6, 0.5).normalize(), Vec3::from).normalize_or_zero();
    let to_sun = -sun;
    let step = scene.driveline.len().div_ceil(DOME_POINTS).max(1);
    let dome_points: Vec<Vec3> = scene.driveline.iter().step_by(step).map(|&p| Vec3::from(p)).collect();
    let mut groups = Groups::default();
    let mut placed = placed::Builder::default();
    let auto = std::env::var("RBR_BTB_INSTANCE").is_ok_and(|v| v == "auto");
    for object in scene.objects.iter().filter(|o| o.visible) {
        let Some(plan) = &plans[object.mesh] else { continue };
        let mesh = &scene.meshes[object.mesh];
        let (low, high) = local_bounds(mesh);
        let instancing = instanced(object, plan, auto);
        for placement in &object.placements {
            let (centre, half) = world_bounds(placement, low, high);
            let solo = half.max_element() > SOLO;
            let reach = if solo { half.length() } else { 0.0 };
            let lod = band(object.lod_in, object.lod_out + reach);
            if instancing && !solo {
                // Forced on an object that did not ask (development), a converted stage's ground stays baked.
                let on = || {
                    let vertices = world_vertices(mesh, plan, placement, to_sun);
                    loaded.as_ref().is_some_and(|collision| on_ground(&vertices, &mesh.indices, collision))
                };
                if object.instanced || !on() {
                    let model = placed.model(object.mesh, || placed::Model {
                        vertices: model_vertices(mesh, plan),
                        indices: mesh.indices.clone(),
                    });
                    let cell = ((centre.x / CELL).floor() as i32, (centre.z / CELL).floor() as i32);
                    let key = (model, cell, lod, object.sun_cascades);
                    placed.add(
                        key,
                        &plan.draws,
                        Instance::new(placement, to_sun, plan.up),
                        (centre - half, centre + half),
                    );
                    continue;
                }
            }
            let cell = if solo {
                Cell::Solo(groups.solos.next())
            } else {
                Cell::At((centre.x / CELL).floor() as i32, (centre.z / CELL).floor() as i32)
            };
            let vertices = world_vertices(mesh, plan, placement, to_sun);
            if let (Some(texture), Some(fx)) = (plan.hides, grass_fx.as_mut()) {
                fx.occluders.extend(mesh.indices.chunks_exact(3).map(|t| {
                    let v = [t[0], t[1], t[2]].map(|i| &vertices[i as usize]);
                    GrassOccluder { p: v.map(|v| Vec3::from(v.position)), uv: v.map(|v| v.uv0), texture }
                }));
            }
            let dome = solo && roofs(&vertices, &mesh.indices, &dome_points);
            let collides = match &loaded {
                Some(collision) => on_ground(&vertices, &mesh.indices, collision),
                None => object.collides && !converted,
            };
            for &(layer, state) in &plan.draws {
                let layer = if dome { Layer::Sky } else { layer };
                // The opaque faces of the collision ground are the near ground: the terrain detail, the wet film and
                // the grass field take them.
                let ground = collides && !dome && !state.blend && state.alpha_ref.is_none();
                let state = DrawState { ground, ..state };
                let key = (layer, cell, lod, object.sun_cascades);
                groups.add(key, state, &vertices, &mesh.indices, centre - half, centre + half);
            }
        }
    }
    let mut meshes = groups.finish();
    meshes.sort_by_key(|mesh| mesh.layer);
    let placed = placed.finish();
    let (pieces, triangles) =
        (meshes.len(), meshes.iter().flat_map(|m| &m.draws).map(|d| d.indices.len() / 3).sum::<usize>());
    if std::env::var_os("RBR_BTB_STATS").is_some() {
        let vertices: usize = meshes.iter().map(|m| m.vertices.len()).sum();
        let domes = meshes.iter().filter(|m| m.layer == Layer::Sky).count();
        let surfaces = meshes.iter().flat_map(|m| &m.draws).filter_map(|d| d.state.surface.map(|s| (d, s)));
        let (bumped, highlit, sheened) = surfaces.fold((0, 0, 0), |(b, h, r), (d, s)| {
            let n = d.indices.len() / 3;
            (
                b + n * usize::from(s.normal.is_some()),
                h + n * usize::from(s.highlight.is_some()),
                r + n * usize::from(s.sheen.is_some()),
            )
        });
        eprintln!(
            "btb: {} objects -> {pieces} meshes ({domes} sky), {vertices} vertices, {triangles} triangles \
             ({bumped} with a normal map, {highlit} with a highlight, {sheened} with a sheen)",
            scene.objects.len()
        );
    }
    if std::env::var_os("RBR_BTB_STATS").is_some() && !placed.groups.is_empty() {
        eprintln!(
            "btb: instanced {} models ({} vertices), {} placements in {} groups, {} triangles",
            placed.models.len(),
            placed.models.iter().map(|m| m.vertices.len()).sum::<usize>(),
            placed.instances.len(),
            placed.groups.len(),
            placed.triangle_count()
        );
    }
    let driveline: Vec<Vec3> = scene.driveline.iter().map(|&p| Vec3::from(p)).collect();
    let roads = crate::puddles::roads(&meshes, &driveline);
    let collision = super::mark_wet_roads(&mut meshes, maps, name, selection, Some(&roads), loaded);

    // The sky is the sky map's (`rbr_btb::stage::SKY_MAP`): its section by the row's own words, in the retail
    // `TrackSettings.ini`; without a row, its variant M's pick by the weather.
    let sky = selection.sky.and_then(Sky::named);
    let mut atmosphere = Atmosphere::load(maps, rbr_btb::stage::SKY_STAGE, selection.weather.as_str(), sky.as_deref());
    if scene.sun_direction.is_some() {
        atmosphere.sun_direction = sun;
    }
    if let Some(air) = scene.air {
        atmosphere.haze_hue = air.fog_color.map(Vec3::from);
        atmosphere.aerosol = air.aerosol;
        atmosphere.mist = air.mist;
        atmosphere.sky_tint = air.sky_tint;
        atmosphere.north = air.north.map(Vec3::from);
    }
    let (yaw, position) = (scene.spawn.yaw, Vec3::from(scene.spawn.position));
    let start = Some(Pose { position, forward: Vec3::new(yaw.sin(), 0.0, yaw.cos()) });
    Ok(Stage {
        name: name.to_owned(),
        selection,
        meshes,
        placed,
        interactive: Vec::new(),
        texture_names,
        textures: BTreeMap::new(),
        start_digits: None,
        snow_ground: false,
        grass_fx,
        collision,
        start,
        atmosphere,
        driveline,
        roads: Some(roads),
        report: {
            report.missing_textures = Vec::new();
            report
        },
    })
}

/// Reads the textures the draws name from the track's `Textures.zip`.
pub(super) fn load_textures(stage: &mut Stage) -> Result<(), Error> {
    let folder = source(&stage.name).ok_or_else(|| Error::Btb(format!("{}: no BTB folder", stage.name)))?;
    let files = scene::TextureFiles::open(&folder).map_err(Error::Btb)?;
    let names: Vec<_> = stage.texture_names.iter().map(|(&texture, name)| (texture, name.file_name.clone())).collect();
    let images = crate::parallel::in_parallel(&names, |(_, file)| {
        files.read(file).and_then(|bytes| super::textures::decode(TextureSet::Stage, bytes))
    });
    let mut missing = Vec::new();
    for ((texture, file), image) in names.into_iter().zip(images) {
        if image.is_none() {
            missing.push(file.clone());
        }
        stage.textures.insert(texture, StageTexture { request: file, image });
    }
    stage.report.missing_textures = missing;
    Ok(())
}

/// How a mesh is drawn.
struct Plan {
    draws: Vec<(Layer, DrawState)>,
    /// The vertex alpha is the lerp weight, and `uv1` the lerp UV set.
    lerp: bool,
    /// `Up` technique: every normal is +y.
    up: bool,
    uv_lerp: Option<usize>,
    /// The normal map's UV set where it is not set 0, in `uv1`.
    uv_bump: Option<usize>,
    /// The highlight's map's UV set, in `specular`'s uv, with strength 1.
    uv_specular: Option<usize>,
    /// The grass grows nowhere under the mesh ([`GrassOccluder`]): `Some(texture)` under its opaque texels alone.
    hides: Option<Option<TextureRef>>,
}

/// Whether the placements of `object` are drawn as instances ([`placed`]): when it asks (`ReRbrInstanced`) or, to compare
/// the two (`RBR_BTB_INSTANCE=auto`), when it has at least [`AUTO_MIN`]; and it has a few placements, is no
/// collision ground and neither grows nor hides the grass (which are read in the world).
fn instanced(object: &scene::Object, plan: &Plan, auto: bool) -> bool {
    let grows = plan.draws.iter().any(|(_, state)| state.grass.is_some());
    let asks = object.instanced || (auto && object.placements.len() >= AUTO_MIN);
    asks && object.placements.len() >= 2 && !object.collides && !grows && plan.hides.is_none()
}

/// One vertex of `mesh`: the plan's UV sets and lerp weight, with `position`, `normal` and the light's `shade`.
fn vertex(mesh: &scene::Mesh, plan: &Plan, i: usize, position: [f32; 3], normal: [f32; 3], shade: u8) -> Vertex {
    let uv = |set: Option<usize>, i: usize| {
        set.and_then(|s| mesh.uvs.get(s)).and_then(|uvs| uvs.get(i)).copied().unwrap_or([0.0; 2])
    };
    let weight = if plan.lerp { mesh.blend.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0) } else { 1.0 };
    Vertex {
        position,
        color: [shade, shade, shade, (weight * 255.0).round() as u8],
        uv0: uv(Some(0), i),
        uv1: if plan.lerp { uv(plan.uv_lerp, i) } else { plan.uv_bump.map_or([0.0; 2], |s| uv(Some(s), i)) },
        uv2: [0.0; 2],
        normal,
        specular: plan.uv_specular.map_or([0.0; 3], |s| {
            let [u, v] = uv(Some(s), i);
            [u, v, 1.0]
        }),
        occlusion: 0.0,
        sky_dir: 0,
        bounce: [0; 2],
        wet: 0.0,
        edge: 0.0,
    }
}

/// The mesh's vertices in the world for `placement`: lit by the sun as the effects do, `(ambient + N.L)` on the
/// texture, in the vertex colour (the renderer's own light replaces it); the lerp weight in the alpha.
fn world_vertices(mesh: &scene::Mesh, plan: &Plan, placement: &[f32; 16], to_sun: Vec3) -> Vec<Vertex> {
    (0..mesh.positions.len())
        .map(|i| {
            let normal = if plan.up { [0.0, 1.0, 0.0] } else { scene::place_normal(placement, mesh.normals[i]) };
            let light = (AMBIENT + Vec3::from(normal).dot(to_sun).max(0.0)).min(1.0);
            let shade = (light * 255.0).round() as u8;
            vertex(mesh, plan, i, scene::place_point(placement, mesh.positions[i]), normal, shade)
        })
        .collect()
}

/// The mesh's vertices in its own space, for [`Instance::place`] to place and light.
fn model_vertices(mesh: &scene::Mesh, plan: &Plan) -> Vec<Vertex> {
    (0..mesh.positions.len())
        .map(|i| {
            let normal = if plan.up { [0.0, 1.0, 0.0] } else { mesh.normals[i] };
            vertex(mesh, plan, i, mesh.positions[i], normal, 255)
        })
        .collect()
}

/// Whether the triangles pass [`DOME_CLEARANCE`] over each of `points` (stage x and z): an author's sky dome, which,
/// drawn as the stage, roofed it for the sun's shadows and the sky's occlusion (Cueva Santa, Savo).
fn roofs(vertices: &[Vertex], indices: &[u32], points: &[Vec3]) -> bool {
    let at = |i: u32| Vec3::from(vertices[i as usize].position);
    let over = |p: &Vec3| {
        indices.chunks_exact(3).any(|t| {
            crate::sky_dome::height_over(&[at(t[0]), at(t[1]), at(t[2])], p.xz())
                .is_some_and(|y| y > p.y + DOME_CLEARANCE)
        })
    };
    !points.is_empty() && points.iter().all(over)
}

/// Whether most of the triangles (up to [`GROUND_SAMPLES`] of them, evenly spread) lie on `collision`: within
/// [`ON_GROUND`] of the collision surface under their centres.
fn on_ground(vertices: &[Vertex], indices: &[u32], collision: &crate::particles::ground::Ground) -> bool {
    let triangles = indices.len() / 3;
    let step = triangles.div_ceil(GROUND_SAMPLES).max(1);
    let (mut on, mut sampled) = (0, 0);
    for t in indices.chunks_exact(3).step_by(step) {
        let [x, y, z] = [0, 1, 2].map(|c| t.iter().map(|&i| vertices[i as usize].position[c]).sum::<f32>() / 3.0);
        sampled += 1;
        on += usize::from(collision.below([x, z, y]).is_some_and(|hit| (hit.z - y).abs() < ON_GROUND));
    }
    sampled > 0 && 2 * on > sampled
}

fn local_bounds(mesh: &scene::Mesh) -> (Vec3, Vec3) {
    mesh.positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(low, high), &p| {
        (low.min(Vec3::from(p)), high.max(Vec3::from(p)))
    })
}

/// Centre and half extent of the placed box.
fn world_bounds(placement: &[f32; 16], low: Vec3, high: Vec3) -> (Vec3, Vec3) {
    let mut corners = (0..8).map(|i| {
        let pick = |bit: usize| if i & bit == 0 { low } else { high };
        Vec3::from(scene::place_point(placement, [pick(1).x, pick(2).y, pick(4).z]))
    });
    let first = corners.next().unwrap_or_default();
    let (min, max) = corners.fold((first, first), |(min, max), c| (min.min(c), max.max(c)));
    ((min + max) / 2.0, (max - min) / 2.0)
}

/// The draw distances as a level, [`REACH`] times the source's: none for the whole range.
fn band(near: f32, far: f32) -> Lod {
    let near = (REACH * near / STEP).floor() * STEP;
    let far = (REACH * far / STEP).ceil() * STEP;
    if near <= 0.0 && far >= f32::from(u16::MAX) {
        Lod::Always
    } else {
        Lod::Band { near: near.clamp(0.0, 65535.0) as u16, far: far.clamp(0.0, 65535.0) as u16 }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Cell {
    At(i32, i32),
    Solo(u32),
}

#[derive(Default)]
struct Counter(u32);

impl Counter {
    fn next(&mut self) -> u32 {
        self.0 += 1;
        self.0
    }
}

/// The meshes being built, one per layer, cell, band and whether it casts into the sun's cascades.
#[derive(Default)]
struct Groups {
    by_key: HashMap<(Layer, Cell, Lod, bool), usize>,
    groups: Vec<Group>,
    solos: Counter,
}

struct Group {
    layer: Layer,
    lod: Lod,
    sun_cascades: bool,
    vertices: Vec<Vertex>,
    draws: Vec<(DrawState, Vec<u32>)>,
    low: Vec3,
    high: Vec3,
}

impl Groups {
    fn add(
        &mut self,
        key: (Layer, Cell, Lod, bool),
        state: DrawState,
        vertices: &[Vertex],
        indices: &[u32],
        low: Vec3,
        high: Vec3,
    ) {
        let (layer, _, lod, sun_cascades) = key;
        let at = *self.by_key.entry(key).or_insert_with(|| {
            self.groups.push(Group {
                layer,
                lod,
                sun_cascades,
                vertices: Vec::new(),
                draws: Vec::new(),
                low: Vec3::splat(f32::MAX),
                high: Vec3::splat(f32::MIN),
            });
            self.groups.len() - 1
        });
        let group = &mut self.groups[at];
        let base = group.vertices.len() as u32;
        group.vertices.extend_from_slice(vertices);
        group.low = group.low.min(low);
        group.high = group.high.max(high);
        let slot = match group.draws.iter().position(|(s, _)| *s == state) {
            Some(slot) => slot,
            None => {
                group.draws.push((state, Vec::new()));
                group.draws.len() - 1
            }
        };
        group.draws[slot].1.extend(indices.iter().map(|&i| base + i));
    }

    fn finish(self) -> Vec<Mesh> {
        self.groups
            .into_iter()
            .map(|group| {
                let mut indices = Vec::new();
                let mut draws = Vec::new();
                for (state, list) in group.draws {
                    let first = indices.len();
                    indices.extend(list);
                    draws.push(Draw { indices: first..indices.len(), state, lod: group.lod });
                }
                // The sky has no bounds: always drawn.
                let (bounds, cell) = if group.layer == Layer::Sky {
                    (None, None)
                } else {
                    let (bounds, cell) = bounds_and_cell(group.low, group.high, group.lod);
                    (Some(bounds), cell)
                };
                Mesh {
                    layer: group.layer,
                    anchor: Anchor::World,
                    bounds,
                    cell,
                    vertices: group.vertices,
                    indices,
                    draws,
                    sun_cascades: group.sun_cascades,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objects_out_of_the_sun_cascades_get_meshes_of_their_own() {
        let state = DrawState {
            textures: [None; 2],
            shadow: None,
            address: [Address::Wrap; 2],
            second_texture: false,
            alpha_ref: Some(CUTOUT_REF),
            mip_bias: false,
            ground: false,
            grass: None,
            cull_back: false,
            blend: false,
            depth_test: true,
            depth_write: true,
            shade: None,
            specular: None,
            surface: None,
        };
        let vertices = [bytemuck::Zeroable::zeroed(); 3];
        let lod = band(0.0, 500.0);
        let mut groups = Groups::default();
        // Two placements in one cell and band, each way, and one casting placement in the next cell.
        for (cell, casts) in [(0, true), (0, false), (0, true), (0, false), (1, true)] {
            let key = (Layer::Solid, Cell::At(cell, 0), lod, casts);
            groups.add(key, state, &vertices, &[0, 1, 2], Vec3::ZERO, Vec3::ONE);
        }
        let meshes = groups.finish();
        let casting: Vec<(bool, usize)> = meshes.iter().map(|m| (m.sun_cascades, m.indices.len() / 3)).collect();
        assert_eq!(casting, [(true, 2), (false, 2), (true, 1)]);
    }

    fn state() -> DrawState {
        DrawState {
            textures: [None; 2],
            shadow: None,
            address: [Address::Wrap; 2],
            second_texture: false,
            alpha_ref: None,
            mip_bias: false,
            ground: false,
            grass: None,
            cull_back: false,
            blend: false,
            depth_test: true,
            depth_write: true,
            shade: None,
            specular: None,
            surface: None,
        }
    }

    fn plan(up: bool, lerp: bool) -> Plan {
        Plan {
            draws: vec![(Layer::Solid, state())],
            lerp,
            up,
            uv_lerp: Some(1),
            uv_bump: None,
            uv_specular: Some(1),
            hides: None,
        }
    }

    fn mesh() -> scene::Mesh {
        scene::Mesh {
            name: "tree".into(),
            positions: vec![[0.0, 0.0, 0.0], [1.0, 2.0, 0.5], [-1.5, 3.0, 2.0], [0.2, -1.0, 4.0]],
            normals: vec![[0.0, 1.0, 0.0], [0.6, 0.0, 0.8], [-0.3, 0.9, 0.3], [0.0, 0.0, -1.0]],
            tangents: Vec::new(),
            binormals: Vec::new(),
            uvs: vec![vec![[0.0, 0.0], [1.0, 0.0], [0.5, 1.0], [0.25, 0.75]], vec![[0.1, 0.2]; 4]],
            blend: vec![0.0, 0.3, 1.0, 0.7],
            indices: vec![0, 1, 2, 0, 2, 3],
            material: Some(0),
        }
    }

    /// A placement turned, scaled and moved: row-major, `p * M`.
    fn turned() -> [f32; 16] {
        let (s, c) = 0.7f32.sin_cos();
        [2.0 * c, 0.0, -2.0 * s, 0.0, 0.0, 1.5, 0.0, 0.0, 2.0 * s, 0.0, 2.0 * c, 0.0, 130.5, 12.0, -77.25, 1.0]
    }

    #[test]
    fn a_model_under_an_instance_is_the_placements_expanded_vertices() {
        let to_sun = Vec3::new(0.3, 0.8, -0.5).normalize();
        for (up, lerp) in [(false, false), (true, false), (false, true)] {
            let (mesh, plan) = (mesh(), plan(up, lerp));
            let placement = turned();
            let baked = world_vertices(&mesh, &plan, &placement, to_sun);
            let instance = Instance::new(&placement, to_sun, plan.up);
            for (model, baked) in model_vertices(&mesh, &plan).iter().zip(&baked) {
                let placed = instance.place(model);
                assert_eq!(placed, *baked, "up {up}, lerp {lerp}");
            }
        }
    }

    #[test]
    fn instanced_groups_have_the_bounds_of_the_expanded_meshes() {
        let (mesh, plan) = (mesh(), plan(false, false));
        let (low, high) = local_bounds(&mesh);
        let to_sun = Vec3::Y;
        let lod = band(0.0, 300.0);
        let (mut groups, mut builder) = (Groups::default(), placed::Builder::default());
        let model = builder
            .model(0, || placed::Model { vertices: model_vertices(&mesh, &plan), indices: mesh.indices.clone() });
        for x in [10.0, 20.0, 140.0] {
            let placement = [turned()[..12].to_vec(), vec![x, 3.0, 5.0, 1.0]].concat().try_into().unwrap();
            let (centre, half) = world_bounds(&placement, low, high);
            let cell = ((centre.x / CELL).floor() as i32, (centre.z / CELL).floor() as i32);
            let vertices = world_vertices(&mesh, &plan, &placement, to_sun);
            groups.add(
                (Layer::Solid, Cell::At(cell.0, cell.1), lod, true),
                state(),
                &vertices,
                &mesh.indices,
                centre - half,
                centre + half,
            );
            builder.add(
                (model, cell, lod, true),
                &plan.draws,
                Instance::new(&placement, to_sun, false),
                (centre - half, centre + half),
            );
        }
        let (baked, placed) = (groups.finish(), builder.finish());
        assert_eq!(baked.len(), placed.groups.len());
        for (mesh, group) in baked.iter().zip(&placed.groups) {
            assert_eq!((mesh.bounds, mesh.cell), (Some(group.bounds), group.cell));
            assert_eq!(mesh.indices.len() / 3, placed.models[group.model].indices.len() / 3 * group.instances.len());
        }
        assert_eq!(placed.instances.len(), 3);
    }

    #[test]
    fn a_world_layer_keeps_the_origins_share_of_each_detail_as_a_fraction() {
        let names = |n: &str| n.to_owned();
        let layers = scene::Layers {
            diffuse: names("d"),
            mask: names("m"),
            details: ["a", "b", "c", "d"].map(names),
            scale: [[0.02, 0.02], [0.5, 0.25], [0.0, 0.0], [4.0, 8.0]],
            magic: 1.5,
            world: Some([-231.0, -563.0]),
        };
        let mut next = 0;
        let mut texture = |file: &Option<String>| {
            file.as_ref()?;
            next += 1;
            Some(TextureRef::stage(next))
        };
        let world = layered(&layers, &mut texture);
        // AC's x is the stage's plus the origin's, its z the stage's negated less the origin's.
        let near = |a: f32, b: f32| (a - b).abs() < 1e-5;
        assert!(near(world.offset[0][0], 0.38) && near(world.offset[0][1], 0.26), "{:?}", world.offset[0]);
        assert!(near(world.offset[1][0], 0.5) && near(world.offset[1][1], 0.75), "{:?}", world.offset[1]);
        assert_eq!((world.offset[2], world.offset[3]), ([0.0; 2], [0.0; 2]));
        let uv = layered(&scene::Layers { world: None, ..layers }, &mut texture);
        assert!(uv.offset.iter().all(|o| *o == [0.0; 2]) && !uv.world);
    }
}
