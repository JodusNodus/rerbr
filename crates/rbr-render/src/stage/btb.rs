//! BTB (Bob's Track Builder) stages: [`rbr_btb::scene::Scene`] as a [`Stage`]. A stage prepared by
//! `rbr_btb::stage::prepare` (its folder's `btb.ini` names the BTB folder), or named `<BTB folder>/track-41_M` directly
//! (the RSF plugin plays BTB stages as map 41), draws from the track folder instead of `Maps/`. What each effect and
//! technique becomes, and what is missing: `notes/btb.md` "Drawing".
//!
//! There is no instancing: every placement is expanded into world-space vertices, grouped by 128 m cells, `LODIn` and
//! `LODOut` (a mesh each, drawn between those distances: [`Lod::Band`]) so that the views cull them by their bounds.

use super::mesh::{Bounds, Draw, DrawState, Layer, Lod, Mesh, Vertex};
use super::{
    Address, Anchor, Error, LoadReport, Pose, Selection, Stage, StageTexture, TextureName, TextureRef, TextureSet,
};
use crate::atmosphere::Atmosphere;
use glam::{Vec3, Vec3Swizzles};
use rbr_btb::scene::{self, Alpha, Scene};
use rbr_formats::stage_files;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// The BTB track folder stage `name` plays: a prepared folder's `btb.ini` source (`rbr_btb::stage::prepare`), or the
/// track folder itself (`<folder>/track-41_M`, for development).
fn source(name: &str) -> Option<PathBuf> {
    let folder = stage_files::folder(name)?;
    rbr_btb::stage::source(folder).or_else(|| rbr_btb::track::is_track_folder(folder).then(|| folder.to_owned()))
}

/// Whether stage `name` is a BTB stage's.
pub(super) fn is_btb(name: &str) -> bool {
    source(name).is_some()
}

/// The `ambient` of every BTB material (`materials.ini`, 0.4 in all folders): the light's floor.
const AMBIENT: f32 = 0.4;
/// Side of the cells small placements are grouped by, metres.
const CELL: f32 = 128.0;
/// A placement wider than this (half extent, metres) is a mesh of its own: the terrain tiles.
const SOLO: f32 = 150.0;
/// Distances of [`Lod::Band`] are kept to this step, metres.
const STEP: f32 = 50.0;
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
            let first = texture(&material.diffuse);
            let second = if lerp { texture(&material.second) } else { None };
            // Two passes only where the texture is translucent; a cut-out one is alpha tested, an opaque one solid.
            let alpha = match (material.technique.transparent, &material.diffuse) {
                (false, _) => Alpha::Opaque,
                (true, None) => Alpha::Translucent,
                (true, Some(file)) => *alphas.entry(file).or_insert_with(|| scene.texture_alpha(file)),
            };
            let state = |alpha_ref, blend| DrawState {
                textures: [first, second],
                shadow: None,
                address: [Address::Wrap; 2],
                second_texture: second.is_some(),
                alpha_ref,
                mip_bias: false,
                cull_back: false,
                blend,
                depth_test: true,
                depth_write: !blend,
                shade: None,
                specular: None,
            };
            let draws = match alpha {
                Alpha::Translucent => {
                    vec![(Layer::Blended, state(None, true)), (Layer::Solid, state(Some(SOLID_REF), false))]
                }
                Alpha::Cutout => vec![(Layer::Solid, state(Some(CUTOUT_REF), false))],
                Alpha::Opaque => vec![(Layer::Solid, state(None, false))],
            };
            Some(Plan { draws, lerp, up: material.technique.up, uv_lerp: material.effect.uv_slots().lerp })
        })
        .collect();

    let sun = scene.sun_direction.map_or(Vec3::new(0.6, -0.6, 0.5).normalize(), Vec3::from).normalize_or_zero();
    let to_sun = -sun;
    let step = scene.driveline.len().div_ceil(DOME_POINTS).max(1);
    let dome_points: Vec<Vec3> = scene.driveline.iter().step_by(step).map(|&p| Vec3::from(p)).collect();
    let mut groups = Groups::default();
    for object in scene.objects.iter().filter(|o| o.visible) {
        let Some(plan) = &plans[object.mesh] else { continue };
        let mesh = &scene.meshes[object.mesh];
        let (low, high) = local_bounds(mesh);
        for placement in &object.placements {
            let (centre, half) = world_bounds(placement, low, high);
            let solo = half.max_element() > SOLO;
            let cell = if solo {
                Cell::Solo(groups.solos.next())
            } else {
                Cell::At((centre.x / CELL).floor() as i32, (centre.z / CELL).floor() as i32)
            };
            let reach = if solo { half.length() } else { 0.0 };
            let lod = band(object.lod_in, object.lod_out + reach);
            let vertices = world_vertices(mesh, plan, placement, to_sun);
            let dome = solo && roofs(&vertices, &mesh.indices, &dome_points);
            for &(layer, state) in &plan.draws {
                let layer = if dome { Layer::Sky } else { layer };
                groups.add(layer, cell, lod, state, &vertices, &mesh.indices, centre - half, centre + half);
            }
        }
    }
    let mut meshes = groups.finish();
    meshes.sort_by_key(|mesh| mesh.layer);
    let (pieces, triangles) =
        (meshes.len(), meshes.iter().flat_map(|m| &m.draws).map(|d| d.indices.len() / 3).sum::<usize>());
    if std::env::var_os("RBR_BTB_STATS").is_some() {
        let vertices: usize = meshes.iter().map(|m| m.vertices.len()).sum();
        let domes = meshes.iter().filter(|m| m.layer == Layer::Sky).count();
        eprintln!(
            "btb: {} objects -> {pieces} meshes ({domes} sky), {vertices} vertices, {triangles} triangles",
            scene.objects.len()
        );
    }

    let sky = selection.sky.and_then(|sky| sky.section(name));
    let mut atmosphere = Atmosphere::load(maps, name, selection.weather.as_str(), sky.as_deref());
    if scene.sun_direction.is_some() {
        atmosphere.sun_direction = sun;
    }
    let (yaw, position) = (scene.spawn.yaw, Vec3::from(scene.spawn.position));
    let start = Some(Pose { position, forward: Vec3::new(yaw.sin(), 0.0, yaw.cos()) });
    Ok(Stage {
        name: name.to_owned(),
        selection,
        meshes,
        interactive: Vec::new(),
        texture_names,
        textures: BTreeMap::new(),
        start_digits: None,
        snow_ground: false,
        collision: None,
        start,
        atmosphere,
        driveline: scene.driveline.iter().map(|&p| Vec3::from(p)).collect(),
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
}

/// The mesh's vertices in the world for `placement`: lit by the sun as the effects do, `(ambient + N.L)` on the
/// texture, in the vertex colour (the renderer's own light replaces it); the lerp weight in the alpha.
fn world_vertices(mesh: &scene::Mesh, plan: &Plan, placement: &[f32; 16], to_sun: Vec3) -> Vec<Vertex> {
    let uv = |set: Option<usize>, i: usize| {
        set.and_then(|s| mesh.uvs.get(s)).and_then(|uvs| uvs.get(i)).copied().unwrap_or([0.0; 2])
    };
    (0..mesh.positions.len())
        .map(|i| {
            let normal = if plan.up { [0.0, 1.0, 0.0] } else { scene::place_normal(placement, mesh.normals[i]) };
            let light = (AMBIENT + Vec3::from(normal).dot(to_sun).max(0.0)).min(1.0);
            let shade = (light * 255.0).round() as u8;
            let weight = if plan.lerp { mesh.blend.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0) } else { 1.0 };
            Vertex {
                position: scene::place_point(placement, mesh.positions[i]),
                color: [shade, shade, shade, (weight * 255.0).round() as u8],
                uv0: uv(Some(0), i),
                uv1: if plan.lerp { uv(plan.uv_lerp, i) } else { [0.0; 2] },
                uv2: [0.0; 2],
                normal,
                specular: [0.0; 3],
                occlusion: 0.0,
                sky_dir: 0,
                bounce: [0; 2],
                wet: 0.0,
                edge: 0.0,
            }
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

/// The draw distances as a level: none for the whole range.
fn band(near: f32, far: f32) -> Lod {
    let near = (near / STEP).floor() * STEP;
    let far = (far / STEP).ceil() * STEP;
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

/// The meshes being built.
#[derive(Default)]
struct Groups {
    by_key: HashMap<(Layer, Cell, Lod), usize>,
    groups: Vec<Group>,
    solos: Counter,
}

struct Group {
    layer: Layer,
    lod: Lod,
    vertices: Vec<Vertex>,
    draws: Vec<(DrawState, Vec<u32>)>,
    low: Vec3,
    high: Vec3,
}

impl Groups {
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        layer: Layer,
        cell: Cell,
        lod: Lod,
        state: DrawState,
        vertices: &[Vertex],
        indices: &[u32],
        low: Vec3,
        high: Vec3,
    ) {
        let at = *self.by_key.entry((layer, cell, lod)).or_insert_with(|| {
            self.groups.push(Group {
                layer,
                lod,
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
                let bounds = (group.layer != Layer::Sky).then(|| Bounds {
                    center: (group.low + group.high) / 2.0,
                    half_extent: (group.high - group.low) / 2.0,
                });
                Mesh {
                    layer: group.layer,
                    anchor: Anchor::World,
                    bounds,
                    cell: bounds,
                    vertices: group.vertices,
                    indices,
                    draws,
                }
            })
            .collect()
    }
}
