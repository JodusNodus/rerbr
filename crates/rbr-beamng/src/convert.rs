//! A level and one of its time-trial missions as a BTB stage folder (`<out>/RX_CONTENT/Tracks/<name>/`, the files
//! `rbr_btb` reads, `notes/btb.md`), so the BTB path plays it unchanged.
//!
//! - Frame: BeamNG is z up, right handed; the stage frame is Direct3D's, y up: `(x - ox, z, y - oy)` with `(ox, oy)` the
//!   course's plan centre in whole metres. Torque's faces wind clockwise seen from their front in its right-handed
//!   frame; the axis swap mirrors that to the winding BTB's files have (their ground faces have `(b - a) x (c - a)`
//!   pointing up in the file frame), so the corners keep their order.
//! - Physics: the level's collision meshes (the shapes' `Collision-*` details) within the collision corridor of the
//!   driveline, one object per 100 m tile and surface, `Visible=0`, `CollisionOnly=1`, each surface its own material
//!   whose `mat.ini` map is that RBR surface id in all 256 cells ([`surface`]).
//! - Drawn: the visible detail of every shown shape not in a hidden group or left out by `skip`, within the draw
//!   corridor, one object per tile and material; drawing never collides (`CollisionModel=-1`).
//! - Driveline: the course ([`crate::course`]) from 50 m before the start, its heights on the collision; pace notes
//!   the start, splits, finish and end of stage and the notes [`crate::pacenotes`] reads from its shape.

use crate::course::{self, Course};
use crate::level::{self, Scene};
use crate::materials::{self, Material};
use crate::pacenotes;
use crate::race::Race;
use crate::textures;
use crate::world::{self, Detail, Grid, Triangle};
use crate::xwrite::{self, XMesh};
use crate::zipfs::Archive;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Pace note tags (`notes/formats.md` "dls"): start, finish, split, end of stage.
const START: u32 = 0x15;
const FINISH: u32 = 0x16;
const SPLIT: u32 = 0x17;
const END: u32 = 0x18;
/// The driveline begins this far before the start position.
const LEAD: f64 = 50.0;
/// How far from the eye a forest item is drawn unless a `--lod` rule says otherwise, metres.
const FOREST_LOD: f64 = 500.0;
/// Forest items within this plan distance of the driveline are drawn in full detail up close, metres.
const FOREST_NEAR: f64 = 15.0;
/// Forest kinds lower than this (metres) are plants: drawn in their least detail, only within PLANT_CORRIDOR of the
/// driveline, up to PLANT_LOD from the eye.
const PLANT_HEIGHT: f64 = 3.0;
const PLANT_CORRIDOR: f64 = 30.0;
const PLANT_LOD: f64 = 150.0;
/// How far from the eye a forest item is drawn in full detail, metres.
const FOREST_DETAIL: f64 = 150.0;
/// Plan tile size of the written objects, metres.
const TILE: f64 = 100.0;

#[derive(Debug, Clone)]
pub struct Options {
    /// The mission folder in the zip (`gameplay/missions/<level>/timeTrial/<mission>`), or a race file
    /// (`levels/<level>/quickrace/<race>.race.json`), or `road` (`road:reverse`): along the level's longest drivable
    /// road from its first node (its last).
    pub mission: String,
    /// The stage folder's name; default the mission's.
    pub name: Option<String>,
    /// The root the `RX_CONTENT/Tracks/<name>/` folder goes under.
    pub out: PathBuf,
    /// Shapes whose path contains any of these (case-insensitive) are not drawn.
    pub skip: Vec<String>,
    /// Plan distance from the driveline within which shapes are drawn, metres.
    pub draw_corridor: f64,
    /// `(text, metres)`: shapes whose path contains the text (case-insensitive) are drawn within that plan distance
    /// of the driveline instead of [`Options::draw_corridor`]; the last match wins.
    pub reach: Vec<(String, f64)>,
    /// `(text, metres)`: shapes whose path contains the text are drawn only up to that distance from the eye
    /// (`LODOut`); the last match wins.
    pub lod: Vec<(String, f64)>,
    /// Plan distance within which collision is kept, metres.
    pub collision_corridor: f64,
    /// Plan distance from the driveline within which forest items are drawn (a `--reach` rule overrides), metres.
    pub forest_corridor: f64,
    /// Re-mesh the road band's collision (`remesh`): on unless `--no-remesh`.
    pub remesh_road: bool,
    /// Draw the shapes with `isRenderEnabled` false too (a level that sets it on all it draws).
    pub draw_disabled: bool,
    /// The most forest triangles drawn: items nearest the driveline first.
    pub forest_budget: usize,
    /// Drawn objects whose material's triangles average under this area (m²) are drawn only to [`Options::detail_lod`].
    pub detail_area: f64,
    pub detail_lod: f64,
    /// Shapes whose path contains any of these hold trees that [`Options::retail`]'s replace ([`crate::trees::split`]).
    pub trees: Vec<String>,
    /// The retail trees (`--trees-from`); `None` leaves the level's trees as they are.
    pub retail: Option<crate::trees::Library>,
}

/// The value of the last rule whose text `path` contains (case-insensitive).
fn rule(rules: &[(String, f64)], path: &str) -> Option<f64> {
    let path = path.to_lowercase();
    rules.iter().rev().find(|(k, _)| path.contains(&k.to_lowercase())).map(|r| r.1)
}

impl Options {
    pub fn new(mission: &str, out: &Path) -> Options {
        Options {
            mission: mission.trim_end_matches('/').to_owned(),
            name: None,
            out: out.to_owned(),
            skip: Vec::new(),
            draw_corridor: 150.0,
            reach: Vec::new(),
            lod: Vec::new(),
            collision_corridor: 60.0,
            forest_corridor: 80.0,
            forest_budget: 6_000_000,
            draw_disabled: false,
            remesh_road: true,
            detail_area: 0.02,
            detail_lod: 400.0,
            trees: Vec::new(),
            retail: None,
        }
    }
}

/// What a conversion did.
#[derive(Debug, Default)]
pub struct Report {
    pub folder: PathBuf,
    pub lines: Vec<String>,
}

/// The RBR surface id for a BeamNG ground model (resolved through the level's own models to a built-in one), and
/// whether the face is a wall. By static friction against BeamNG's asphalt (0.98) and NGP's against tarmac fine clean
/// (1.30), `notes/beamng.md` "Surfaces": asphalt tarmac fine clean (37); kerbs (0.96) tarmac medium clean (65, 1.275);
/// cobbles and rock (0.93) tarmac coarse clean (69, 1.25); a metal tread (1.0) tarmac fine clean; grass (0.61, 3-5 cm)
/// grass soft (113, 0.70, 3 cm); gravel and sand (0.6-0.69, 10 cm) gravel fine deeper (1, 0.8725, 2.5 cm); dirt
/// (0.68-0.70) dirt hard (133, 0.84), mud dirt soft (149). A wall (steep) without a road-like model is a metal barrier
/// (64); a flat face without a known model BeamNG's default asphalt.
pub fn surface(ground: Option<&str>, steep: bool) -> u8 {
    let g = ground.unwrap_or("").to_uppercase();
    let has = |k: &str| g.contains(k);
    if has("ASPHALT") {
        37
    } else if has("RUMBLE") || has("CONCRETE") {
        65
    } else if has("METAL") && !steep {
        37
    } else if has("COBBLE") || has("ROCK") || has("STONE") {
        69
    } else if has("GRASS") || has("FOLIAGE") || has("MOSS") {
        113
    } else if has("GRAVEL") || has("SAND") {
        1
    } else if has("MUD") {
        149
    } else if has("DIRT") || has("SOIL") {
        133
    } else if steep {
        64
    } else {
        37
    }
}

/// Plan cells within a distance of the driveline.
struct Corridor {
    cell: f64,
    cells: std::collections::HashSet<(i64, i64)>,
}

impl Corridor {
    fn new(line: &[[f64; 3]], reach: f64) -> Corridor {
        // Cells a tenth of the reach (at least 5 m) keep a wide corridor's set small.
        let cell = (reach / 10.0).max(5.0);
        let r = (reach / cell).ceil() as i64;
        let mut cells = std::collections::HashSet::new();
        for p in line.iter().step_by(2) {
            let (cx, cy) = ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64);
            for dx in -r..=r {
                for dy in -r..=r {
                    if dx * dx + dy * dy <= r * r {
                        cells.insert((cx + dx, cy + dy));
                    }
                }
            }
        }
        Corridor { cell, cells }
    }

    fn holds_point(&self, p: [f64; 3]) -> bool {
        self.cells.contains(&((p[0] / self.cell).floor() as i64, (p[1] / self.cell).floor() as i64))
    }

    fn holds(&self, t: &Triangle) -> bool {
        t.pos
            .iter()
            .any(|p| self.cells.contains(&((p[0] / self.cell).floor() as i64, (p[1] / self.cell).floor() as i64)))
    }
}

/// A forest object's key: shape path, full detail, `LODIn`, `LODOut`.
type ForestKey = (String, bool, u32, u32);

/// A forest item up for drawing.
struct Candidate {
    /// Distance from the driveline over its height (a tall tree stands out from further): lower first.
    rank: f64,
    /// Its least detail: keys and triangles.
    plain: (Vec<ForestKey>, usize),
    /// Full detail up close and the least beyond, for an item by the road.
    full: Option<(Vec<ForestKey>, usize)>,
    placement: [f32; 16],
    /// Drawn, and whether in full detail.
    taken: Option<bool>,
}

/// One object being built: deduplicated vertices and triangles.
#[derive(Default)]
struct Builder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    triangles: Vec<[u32; 3]>,
    seen: HashMap<[i32; 8], u32>,
    area: f64,
    weld: bool,
}

impl Builder {
    fn vertex(&mut self, p: [f32; 3], n: [f32; 3], uv: [f32; 2]) -> u32 {
        // Drawn corners within 1 mm, 1/256 of a normal and 1/4096 of a texture share a vertex (the level's meshes
        // repeat each corner per triangle: 18.0 M corners became 8.5 M vertices); collision keeps exact corners.
        let q = |v: f32, s: f32| if self.weld { (v * s).round() as i32 } else { v.to_bits() as i32 };
        let key = [
            q(p[0], 1000.0),
            q(p[1], 1000.0),
            q(p[2], 1000.0),
            q(n[0], 256.0),
            q(n[1], 256.0),
            q(n[2], 256.0),
            q(uv[0], 4096.0),
            q(uv[1], 4096.0),
        ];
        *self.seen.entry(key).or_insert_with(|| {
            self.positions.push(p);
            self.normals.push(n);
            self.uvs.push(uv);
            self.positions.len() as u32 - 1
        })
    }
}

/// The frame change: BeamNG `(x, y, z)` to the stage's `(x - ox, z, y - oy)`.
#[derive(Clone, Copy)]
struct Frame {
    origin: [f64; 2],
}

impl Frame {
    fn point(&self, p: [f64; 3]) -> [f32; 3] {
        [(p[0] - self.origin[0]) as f32, p[2] as f32, (p[1] - self.origin[1]) as f32]
    }

    /// A BeamNG transform (row-major 3x4) as a row-major Direct3D placement of geometry in the stage's axes: the
    /// linear part with y and z swapped on both sides, transposed; the translation `position` in the stage frame.
    fn placement(&self, m: &[[f64; 4]; 3], position: [f64; 3]) -> [f32; 16] {
        const SWAP: [usize; 3] = [0, 2, 1];
        let t = self.point(position);
        let mut out = [0.0f32; 16];
        for row in 0..3 {
            for col in 0..3 {
                out[row * 4 + col] = m[SWAP[col]][SWAP[row]] as f32;
            }
        }
        out[12..15].copy_from_slice(&t);
        out[15] = 1.0;
        out
    }

    fn normal(n: [f32; 3]) -> [f32; 3] {
        [n[0], n[2], n[1]]
    }
}

/// Adds a triangle to `b` in the stage frame.
fn add(b: &mut Builder, frame: Frame, t: &Triangle, normals: bool) {
    let corner = |b: &mut Builder, k: usize| {
        let n = if normals { Frame::normal(t.normal[k]) } else { [0.0, 1.0, 0.0] };
        let uv = if normals { t.uv[k] } else { [0.0, 0.0] };
        b.vertex(frame.point(t.pos[k]), n, uv)
    };
    let (a, c, d) = (corner(b, 0), corner(b, 1), corner(b, 2));
    if a != c && c != d && a != d {
        b.triangles.push([a, c, d]);
        let n = t.normal();
        let [p, q, r] = t.pos;
        let u = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
        let v = [r[0] - p[0], r[1] - p[1], r[2] - p[2]];
        let cross = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        b.area += 0.5 * (cross[0] * n[0] + cross[1] * n[1] + cross[2] * n[2]).abs();
    }
}

fn tile_of(t: &Triangle) -> (i64, i64) {
    let c = |k: usize| (t.pos[0][k] + t.pos[1][k] + t.pos[2][k]) / 3.0;
    ((c(0) / TILE).floor() as i64, (c(1) / TILE).floor() as i64)
}

/// A name safe as a file name and an ini section: ASCII letters, digits, `_`, `-` and `.`.
fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || "_-.".contains(c) { c } else { '_' }).collect()
}

/// Converts the level in `zip` per `options`.
pub fn convert(zip: &Path, options: &Options) -> Result<Report, String> {
    let mut archive = Archive::open(zip)?;
    let level = level::names(&archive).into_iter().next().ok_or("no level (levels/<name>/main/) in the zip")?;
    let scene = Scene::read(&mut archive, &level)?;
    let mats = materials::read(&mut archive, &level)?;
    let grounds = materials::ground_models(&mut archive, &level)?;
    let race = match options.mission.as_str() {
        m @ ("road" | "road:reverse") => {
            let points =
                course::longest_road(&scene, m.ends_with("reverse")).ok_or("no drivable DecalRoad in the level")?;
            Race::along(&points)?
        }
        m => Race::parse(&archive.read_text(&race_file(m))?)?,
    };
    let course = course::course(&scene, &race)?;
    let mut report = Report::default();
    let mut say = |s: String| {
        eprintln!("beamng-stage: {s}");
        report.lines.push(s);
    };
    say(format!(
        "level {level}, road {} (worst path node {:.1} m off), course {:.0} m, {} splits",
        course.road,
        course.fit,
        course.line.length(),
        course.splits.len()
    ));

    // The plan origin: the course's centre in whole metres.
    let ext = |k: usize| {
        let v = course.line.points.iter().map(|p| p[k]);
        (v.clone().fold(f64::MAX, f64::min) + v.fold(f64::MIN, f64::max)) * 0.5
    };
    let frame = Frame { origin: [ext(0).round(), ext(1).round()] };

    let shown = |s: &level::Static| !scene.in_hidden_group(&s.parent);
    let reaches: std::collections::BTreeSet<u64> = options
        .reach
        .iter()
        .map(|r| r.1)
        .chain([options.draw_corridor, options.forest_corridor])
        .map(f64::to_bits)
        .collect();
    let draw_corridors: HashMap<u64, Corridor> =
        reaches.into_iter().map(|r| (r, Corridor::new(&course.line.points, f64::from_bits(r)))).collect();
    let col_corridor = Corridor::new(&course.line.points, options.collision_corridor);

    // Collision: per tile and surface.
    let mut col: BTreeMap<((i64, i64), u8), Builder> = BTreeMap::new();
    let mut col_triangles: Vec<Triangle> = Vec::new();
    let mut surfaces: BTreeMap<String, (u8, usize)> = BTreeMap::new();
    // Per tile, material and `LODOut` (whole metres, 0 for none given).
    let mut drawn: BTreeMap<((i64, i64), String, u32), Builder> = BTreeMap::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_shape: BTreeMap<String, usize> = BTreeMap::new();
    let mut shapes: HashMap<String, crate::cdae::Shape> = HashMap::new();
    // Retail trees per kind and `LODOut`: their placements.
    let mut planted: BTreeMap<(usize, u32), Vec<[f32; 16]>> = BTreeMap::new();
    let mut replaced = crate::trees::Split::default();
    // The road band's collision, re-meshed from the level's (`remesh`), its materials by name.
    let by_band = Corridor::new(&course.line.points, crate::remesh::REACH);
    let mut names: Vec<String> = Vec::new();
    let road_band = if options.remesh_road {
        let mut level: Vec<Triangle> = Vec::new();
        for st in scene.statics.iter().filter(|s| shown(s)) {
            let path = st.cdae_path();
            if !shapes.contains_key(&path) {
                let Some(shape) = load_shape(&mut archive, st) else { continue };
                shapes.insert(path.clone(), shape.map_err(|e| format!("{path}: {e}"))?);
            }
            let Some(detail) = collision_detail(st, &shapes[&path]) else { continue };
            for part in world::parts(&shapes[&path], st, detail) {
                for t in part.triangles.iter().filter(|t| by_band.holds(t)) {
                    let name = t.material.and_then(|m| part.materials.get(m as usize)).cloned().unwrap_or_default();
                    let index = names.iter().position(|n| *n == name).unwrap_or_else(|| {
                        names.push(name);
                        names.len() - 1
                    });
                    level.push(Triangle { material: Some(index as u32), ..*t });
                }
            }
        }
        let surface = crate::remesh::build(&course.line, &Grid::new(level.iter().collect(), 4.0));
        say(format!(
            "road re-mesh: {} triangles in {} sections, {:.1}% of {} samples off the road",
            surface.triangles.len(),
            surface.written,
            surface.missing as f64 * 100.0 / surface.samples.max(1) as f64,
            surface.samples
        ));
        Some(surface)
    } else {
        None
    };
    let mut replaced_faces = 0usize;
    let mut collide = |t: &Triangle, name: &str| {
        let ground = mats.get(&name.to_lowercase()).and_then(|m| m.ground.clone());
        let ground = ground.map(|g| grounds.get(&g.to_uppercase()).cloned().unwrap_or(g));
        let id = surface(ground.as_deref(), t.normal()[2].abs() < 0.3);
        let entry = surfaces.entry(format!("{name} ({})", ground.unwrap_or_default())).or_insert((id, 0));
        entry.1 += 1;
        add(col.entry((tile_of(t), id)).or_default(), frame, t, false);
        col_triangles.push(*t);
    };
    for st in scene.statics.iter().filter(|s| shown(s)) {
        let path = st.cdae_path();
        if !shapes.contains_key(&path) {
            let Some(shape) = load_shape(&mut archive, st) else {
                *skipped.entry(format!("{path} (missing)")).or_default() += 1;
                continue;
            };
            shapes.insert(path.clone(), shape.map_err(|e| format!("{path}: {e}"))?);
        }
        let shape = &shapes[&path];
        if let Some(detail) = collision_detail(st, shape) {
            for part in world::parts(shape, st, detail) {
                for t in part.triangles.iter().filter(|t| col_corridor.holds(t)) {
                    let name = t.material.and_then(|m| part.materials.get(m as usize)).cloned().unwrap_or_default();
                    // Under the road band: replaced; across its edge: its corners within the band on it.
                    let mut t = *t;
                    if let Some(band) = &road_band {
                        if t.normal()[2].abs() > 0.85 && band.replaces(&t) {
                            replaced_faces += 1;
                            continue;
                        }
                        for p in &mut t.pos {
                            p[2] = band.settled(*p).unwrap_or(p[2]);
                        }
                    }
                    collide(&t, &name);
                }
            }
        }
        let skip = st.hidden
            || (st.render_disabled && !options.draw_disabled)
            || options.skip.iter().any(|k| path.to_lowercase().contains(&k.to_lowercase()));
        if skip {
            *skipped.entry(path).or_default() += 1;
            continue;
        }
        let corridor = &draw_corridors[&rule(&options.reach, &path).unwrap_or(options.draw_corridor).to_bits()];
        let lod = rule(&options.lod, &path).map_or(0, |m| m.round().max(1.0) as u32);
        let trees = options.trees.iter().any(|k| path.to_lowercase().contains(&k.to_lowercase()));
        let retail = options.retail.as_ref().filter(|_| trees);
        for part in world::parts(shape, st, Detail::Visible) {
            let material =
                |t: &Triangle| t.material.and_then(|m| part.materials.get(m as usize)).cloned().unwrap_or_default();
            let mut kept: Vec<&Triangle> = part.triangles.iter().filter(|t| corridor.holds(t)).collect();
            if let Some(library) = retail {
                let see_through =
                    |t: &Triangle| mats.get(&material(t).to_lowercase()).is_some_and(|m| m.translucent || m.alpha_test);
                let cut: Vec<bool> = kept.iter().map(|t| see_through(t)).collect();
                let positions: Vec<[[f64; 3]; 3]> = kept.iter().map(|t| t.pos).collect();
                let split = crate::trees::split(&positions, &cut, library.spacing, library.lowest());
                for spot in &split.spots {
                    let (kind, matrix) =
                        library.place(frame.point(spot.foot), spot.height, crate::trees::seed_of(spot.foot));
                    planted.entry((kind, lod)).or_default().push(matrix);
                }
                kept = split.kept.iter().map(|&i| kept[i]).collect();
                replaced.spots.extend(split.spots);
                replaced.from_cards += split.from_cards;
                replaced.cards += split.cards;
            }
            for t in kept {
                *per_shape.entry(path.clone()).or_default() += 1;
                let name = material(t);
                let b = drawn.entry((tile_of(t), name.to_lowercase(), lod));
                add(b.or_insert_with(|| Builder { weld: true, ..Builder::default() }), frame, t, true);
            }
        }
    }
    if let Some(band) = &road_band {
        for t in &band.triangles {
            collide(t, t.material.map_or("", |m| names[m as usize].as_str()));
        }
        say(format!("road re-mesh: {replaced_faces} level faces under it replaced"));
    }
    // The level's forest: each kind's drawn parts once per detail (its most and its least detailed level), in the
    // stage's axes about its origin, per material. An item within its draw corridor is a placement of the least
    // detailed kind out to its `LODOut`; one within FOREST_NEAR of the driveline is drawn in full detail up to
    // FOREST_DETAIL from the eye and in the least beyond.
    let mut forest_kinds: BTreeMap<(String, bool), Vec<(String, Builder)>> = BTreeMap::new();
    let mut forest_placed: BTreeMap<ForestKey, Vec<[f32; 16]>> = BTreeMap::new();
    let near = Corridor::new(&course.line.points, FOREST_NEAR);
    let plants = Corridor::new(&course.line.points, PLANT_CORRIDOR);
    let mut candidates: Vec<Candidate> = Vec::new();
    for item in crate::forest::items(&mut archive, &level)? {
        let path = item.cdae_path();
        if options.skip.iter().any(|k| path.to_lowercase().contains(&k.to_lowercase())) {
            continue;
        }
        let corridor = &draw_corridors[&rule(&options.reach, &path).unwrap_or(options.forest_corridor).to_bits()];
        if !corridor.holds_point(item.position) {
            continue;
        }
        if !forest_kinds.contains_key(&(path.clone(), true)) {
            let at_origin = level::Static::placed(&item.shape, None, None, None);
            let shape = match load_shape(&mut archive, &item) {
                Some(Ok(shape)) => Some(shape),
                Some(Err(e)) => {
                    *skipped.entry(format!("{path} ({e})")).or_default() += 1;
                    None
                }
                None => {
                    *skipped.entry(format!("{path} (missing)")).or_default() += 1;
                    None
                }
            };
            for (full, detail) in [(true, Detail::Visible), (false, Detail::Lowest)] {
                let parts = shape.as_ref().map(|s| world::parts(s, &at_origin, detail)).unwrap_or_default();
                let mut by_material: BTreeMap<String, Builder> = BTreeMap::new();
                for part in &parts {
                    for t in &part.triangles {
                        let name = t.material.and_then(|m| part.materials.get(m as usize)).cloned().unwrap_or_default();
                        let b = by_material
                            .entry(name.to_lowercase())
                            .or_insert_with(|| Builder { weld: true, ..Builder::default() });
                        add(b, Frame { origin: [0.0, 0.0] }, t, true);
                    }
                }
                forest_kinds.insert((path.clone(), full), by_material.into_iter().collect());
            }
        }
        if forest_kinds[&(path.clone(), false)].is_empty() {
            continue;
        }
        // Low plants (bushes, ferns) only by the road and up close.
        let height = forest_kinds[&(path.clone(), true)]
            .iter()
            .flat_map(|(_, b)| b.positions.iter().map(|p| p[1]))
            .fold(0.0f32, f32::max) as f64
            * item.scale[2];
        let low = height < PLANT_HEIGHT;
        if low && !plants.holds_point(item.position) {
            continue;
        }
        let lod = rule(&options.lod, &path).unwrap_or(if low { PLANT_LOD } else { FOREST_LOD }).round().max(1.0) as u32;
        let placement = frame.placement(&item.transform(), item.position);
        let (detail, cut) = (FOREST_DETAIL as u32, (FOREST_DETAIL as u32).min(lod));
        let tris =
            |full: bool| -> usize { forest_kinds[&(path.clone(), full)].iter().map(|(_, b)| b.triangles.len()).sum() };
        let plain = (vec![(path.clone(), false, 0, lod)], tris(false));
        let full = (!low && near.holds_point(item.position)).then(|| {
            let mut k = vec![(path.clone(), true, 0, cut)];
            if lod > detail {
                k.push((path.clone(), false, detail, lod));
            }
            let cost = k.iter().map(|k| tris(k.1)).sum::<usize>();
            (k, cost)
        });
        candidates.push(Candidate {
            rank: course.line.project(item.position).1 / height.max(1.0),
            plain,
            full,
            placement,
            taken: None,
        });
    }
    // Within the triangle budget, nearest the driveline for their height first: the least detail for most of the
    // budget, then full detail for the items by the road, then the least detail for more items.
    candidates.sort_by(|a, b| a.rank.total_cmp(&b.rank));
    let budget = options.forest_budget;
    let mut spent = 0usize;
    let fill = |candidates: &mut [Candidate], spent: &mut usize, upto: usize| {
        for c in candidates.iter_mut().filter(|c| c.taken.is_none()) {
            if *spent + c.plain.1 <= upto {
                *spent += c.plain.1;
                c.taken = Some(false);
            }
        }
    };
    fill(&mut candidates, &mut spent, budget * 6 / 10);
    for c in candidates.iter_mut().filter(|c| c.taken == Some(false)) {
        if let Some((_, cost)) = &c.full {
            if spent - c.plain.1 + cost <= budget {
                spent = spent - c.plain.1 + cost;
                c.taken = Some(true);
            }
        }
    }
    fill(&mut candidates, &mut spent, budget);
    let (mut dropped, mut full_items) = (0usize, 0usize);
    for c in candidates {
        let keys = match c.taken {
            None => {
                dropped += 1;
                continue;
            }
            Some(true) => {
                full_items += 1;
                c.full.map(|f| f.0).unwrap_or_default()
            }
            Some(false) => c.plain.0,
        };
        for k in keys {
            forest_placed.entry(k).or_default().push(c.placement);
        }
    }
    say(format!("forest: {full_items} items in full detail up close, {dropped} left out (budget {budget} triangles)"));

    for (name, (id, n)) in &surfaces {
        say(format!("collision {name}: surface {id}, {n} triangles"));
    }
    for (path, n) in &per_shape {
        say(format!("drawn {path}: {n} triangles"));
    }
    for (path, n) in &skipped {
        say(format!("not drawn: {path} x{n}"));
    }
    let up = col.values().flat_map(|b| b.triangles.iter().map(move |t| (b, t))).filter(|(b, t)| {
        let p = |i: u32| b.positions[i as usize];
        let (a, c, d) = (p(t[0]), p(t[1]), p(t[2]));
        let u = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let v = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
        u[2] * v[0] - u[0] * v[2] > 0.0
    });
    let col_count: usize = col.values().map(|b| b.triangles.len()).sum();
    say(format!("collision: {col_count} triangles in {} objects, {} facing up", col.len(), up.count()));
    let draw_count: usize = drawn.values().map(|b| b.triangles.len()).sum();
    let draw_vertices: usize = drawn.values().map(|b| b.positions.len()).sum();
    say(format!("drawn: {draw_count} triangles, {draw_vertices} vertices in {} objects", drawn.len()));

    // Driveline heights on the collision.
    let grid = Grid::new(col_triangles.iter().collect(), 8.0);
    let ground = |p: [f64; 3]| grid.ground(p[0], p[1], p[2]).map_or(p[2], |(h, _)| h);
    let stage = staged(&course);
    let mut off = 0usize;
    let line: Vec<[f64; 3]> = stage
        .line
        .points
        .iter()
        .map(|&p| {
            let h = ground(p);
            if (h - p[2]).abs() > 1.0 {
                off += 1;
            }
            [p[0], p[1], h]
        })
        .collect();
    say(format!(
        "driveline: {} points, {off} more than 1 m off the collision height (taken from the collision)",
        line.len()
    ));

    let folder_name = options.name.clone().unwrap_or_else(|| mission_name(&mut archive, &level, &options.mission));
    let folder = options.out.join("RX_CONTENT").join("Tracks").join(safe(&folder_name));
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;

    // Textures and materials.
    let mut textures: Vec<textures::Texture> = Vec::new();
    let mut sections = String::new();
    let mut names = Vec::new();
    let mut maps = String::new();
    let mut map_count = 0;
    let mut used_files: HashMap<String, usize> = HashMap::new();
    let mut unique = |base: &str| {
        let n = used_files.entry(base.to_lowercase()).or_default();
        *n += 1;
        if *n == 1 {
            base.to_owned()
        } else {
            format!("{base}_{n}")
        }
    };
    let mut ids: Vec<u8> = col.keys().map(|(_, id)| *id).collect();
    ids.sort();
    ids.dedup();
    for id in &ids {
        let file = format!("surface_{id}.dds");
        textures.push(textures::Texture { file: file.clone(), bytes: textures::flat([0.5; 4]), how: "surface".into() });
        let section = format!("collision\\surface_{id}");
        write_material(&mut sections, &section, &file, false);
        names.push(section);
        let _ = writeln!(maps, "[MAP{map_count}]\ntexture={file}");
        for row in 0..16 {
            let _ = writeln!(maps, "data{row}={}", vec![id.to_string(); 16].join(" "));
        }
        map_count += 1;
    }
    let mut hows: BTreeMap<String, usize> = BTreeMap::new();
    // Cut-outs made, by colour and opacity entry: materials sharing both share the file.
    let mut cut_outs: HashMap<(Option<String>, String), textures::Texture> = HashMap::new();
    let mut material_of: HashMap<String, String> = HashMap::new();
    let mut by_name: Vec<&String> =
        drawn.keys().map(|(_, m, _)| m).chain(forest_kinds.values().flatten().map(|(m, _)| m)).collect();
    by_name.sort();
    by_name.dedup();
    for name in by_name {
        let m = mats.get(name).cloned().unwrap_or_else(|| Material { name: name.clone(), ..Material::default() });
        let entry = m.color_map.as_deref().and_then(|p| textures::find(&archive, &level, p));
        let stem = entry
            .as_deref()
            .and_then(|e| e.rsplit('/').next())
            .map_or_else(|| safe(name), |f| safe(f.rsplit_once('.').map_or(f, |(s, _)| s)));
        let color = m.color.unwrap_or([0.5, 0.5, 0.5, 1.0]);
        let see_through = m.translucent || m.alpha_test;
        let opacity =
            m.opacity_map.as_deref().filter(|_| see_through).and_then(|p| textures::find(&archive, &level, p));
        let tex = match opacity {
            Some(o) => match cut_outs.get(&(entry.clone(), o.clone())) {
                Some(t) => t.clone(),
                None => {
                    let t = textures::make_cut_out(&mut archive, entry.as_deref(), &o, &unique(&stem), color);
                    cut_outs.insert((entry.clone(), o), t.clone());
                    t
                }
            },
            None => textures::make(&mut archive, entry.as_deref(), &unique(&stem), color),
        };
        *hows.entry(tex.how.clone()).or_default() += 1;
        if tex.how.starts_with("flat") {
            say(format!("texture {}: {name} (colour map {:?})", tex.how, m.color_map));
        }
        let section = format!("beamng\\{}", safe(name));
        write_material(&mut sections, &section, &tex.file, see_through);
        names.push(section.clone());
        material_of.insert(name.clone(), section);
        if !textures.iter().any(|t| t.file == tex.file) {
            textures.push(tex);
        }
    }
    for (how, n) in &hows {
        say(format!("textures {how}: {n}"));
    }
    // The retail kinds' textures, as they are, and their materials (`retail\\<file>`).
    let kinds = options.retail.as_ref().map_or(&[][..], |l| &l.kinds[..]);
    let mut retail_material: HashMap<String, String> = HashMap::new();
    for part in kinds.iter().flat_map(|k| &k.parts) {
        let file = format!("retail_{}", safe(&part.texture));
        if retail_material.contains_key(&file) {
            continue;
        }
        let section = format!("retail\\{}", safe(&part.texture));
        write_material(&mut sections, &section, &file, true);
        names.push(section.clone());
        textures.push(textures::Texture { file: file.clone(), bytes: part.image.clone(), how: "retail".into() });
        retail_material.insert(file, section);
    }

    // Objects.
    let mut objects = String::new();
    let mut count = 0usize;
    let objects_zip = folder.join("Objects.zip");
    let mut zw = zip::ZipWriter::new(
        std::fs::File::create(&objects_zip).map_err(|e| format!("{}: {e}", objects_zip.display()))?,
    );
    let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let zerr = |e: zip::result::ZipError| format!("Objects.zip: {e}");
    let identity: Vec<u8> = [1f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    zw.start_file("Objects/identity.xform", deflate).map_err(zerr)?;
    zw.write_all(&identity).map_err(|e| e.to_string())?;
    let mut put = |zw: &mut zip::ZipWriter<std::fs::File>,
                   file: &str,
                   b: &Builder,
                   material: &str,
                   (xform, clones): (&str, usize),
                   entries: &str| {
        let x = xwrite::write(&XMesh {
            positions: &b.positions,
            normals: &b.normals,
            uvs: &b.uvs,
            triangles: &b.triangles,
            material,
        });
        zw.start_file(format!("Objects/{file}.x"), deflate).map_err(zerr)?;
        zw.write_all(&x).map_err(|e| e.to_string())?;
        let _ = write!(objects, "\n[Object_{count}]\nfile={file}\nxform={xform}\ncloneCount={clones}\n{entries}");
        count += 1;
        Ok::<(), String>(())
    };
    for (((tx, ty), id), b) in &col {
        let entries =
            "LODIn=0\nVisible=0\nDrawInstanced=0\nMoveable=0\nShadowCaster=0\nCollisionModel=0\nCollisionOnly=1\n";
        put(&mut zw, &format!("col_{tx}_{ty}_{id}"), b, &format!("collision\\surface_{id}"), ("identity", 1), entries)?;
    }
    let mut detail_objects = 0;
    for (index, (((tx, ty), name, lod), b)) in drawn.iter().enumerate() {
        let small = b.area / (b.triangles.len().max(1) as f64) < options.detail_area;
        let lod = if *lod > 0 {
            format!("LODOut={lod}\n")
        } else if small {
            detail_objects += 1;
            format!("LODOut={}\n", options.detail_lod)
        } else {
            String::new()
        };
        let entries =
            format!("LODIn=0\n{lod}Visible=1\nDrawInstanced=0\nMoveable=0\nShadowCaster=1\nCollisionModel=-1\n");
        put(&mut zw, &format!("draw_{tx}_{ty}_{index}"), b, &material_of[name], ("identity", 1), &entries)?;
    }
    // Retail trees: per kind and `LODOut`, one placement file, and an object per part of the kind.
    let mut retail_triangles = 0;
    for ((kind, lod), placements) in &planted {
        let xform = format!("trees_{kind}_{lod}");
        zw.start_file(format!("Objects/{xform}.xform"), deflate).map_err(zerr)?;
        zw.write_all(&placements.iter().flatten().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>())
            .map_err(|e| e.to_string())?;
        let lod = if *lod > 0 { format!("LODOut={lod}\n") } else { String::new() };
        let entries =
            format!("LODIn=0\n{lod}Visible=1\nDrawInstanced=0\nMoveable=0\nShadowCaster=1\nCollisionModel=-1\n");
        for (p, part) in kinds[*kind].parts.iter().enumerate() {
            let b = Builder {
                positions: part.positions.clone(),
                normals: vec![[0.0, 1.0, 0.0]; part.positions.len()],
                uvs: part.uvs.clone(),
                triangles: part.triangles.clone(),
                ..Builder::default()
            };
            let material = &retail_material[&format!("retail_{}", safe(&part.texture))];
            put(&mut zw, &format!("{xform}_{p}"), &b, material, (&xform, placements.len()), &entries)?;
            retail_triangles += part.triangles.len() * placements.len();
        }
    }
    // The forest: per kind, detail and distance band, one placement file and an object per material of the kind.
    let (mut forest_items, mut forest_triangles) = (0, 0);
    let mut forest_weight: HashMap<String, usize> = HashMap::new();
    for (k, ((path, full, lod_in, lod_out), placements)) in forest_placed.iter().enumerate() {
        let xform = format!("forest_{k}");
        zw.start_file(format!("Objects/{xform}.xform"), deflate).map_err(zerr)?;
        zw.write_all(&placements.iter().flatten().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>())
            .map_err(|e| e.to_string())?;
        let entries = format!(
            "LODIn={lod_in}\nLODOut={lod_out}\nVisible=1\nDrawInstanced=0\nMoveable=0\nShadowCaster=1\nCollisionModel=-1\n"
        );
        for (p, (name, b)) in forest_kinds[&(path.clone(), *full)].iter().enumerate() {
            put(&mut zw, &format!("{xform}_{p}"), b, &material_of[name], (&xform, placements.len()), &entries)?;
            forest_triangles += b.triangles.len() * placements.len();
        }
        if *lod_in == 0 {
            forest_items += placements.len();
        }
        let n: usize = forest_kinds[&(path.clone(), *full)].iter().map(|(_, b)| b.triangles.len()).sum();
        *forest_weight.entry(path.rsplit('/').next().unwrap_or(path).to_owned()).or_default() += n * placements.len();
    }
    zw.finish().map_err(zerr)?;
    say(format!("objects: {count} ({detail_objects} drawn to {} m only)", options.detail_lod));
    say(format!(
        "forest: {forest_items} items of {} kinds within the draw corridor, {forest_triangles} triangles",
        forest_kinds.values().filter(|k| !k.is_empty()).count()
    ));
    let mut heaviest: Vec<(String, usize)> = forest_weight.into_iter().collect();
    heaviest.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let heaviest: Vec<String> = heaviest.iter().take(6).map(|(k, n)| format!("{k} {n}")).collect();
    say(format!("forest's heaviest kinds (triangles): {}", heaviest.join(", ")));
    if let Some(library) = &options.retail {
        let per_kind: Vec<String> = (0..kinds.len())
            .map(|k| {
                let n: usize = planted.iter().filter(|((kk, _), _)| *kk == k).map(|(_, p)| p.len()).sum();
                format!("{:.1} m x{n}", kinds[k].height)
            })
            .collect();
        say(format!(
            "retail trees from {}: {} kinds of its {} trees, spacing {:.1} m; {} placed ({} along {} tree-line cards), {retail_triangles} triangles; by kind {}",
            library.stage,
            kinds.len(),
            library.trees,
            library.spacing,
            replaced.spots.len(),
            replaced.from_cards,
            replaced.cards,
            per_kind.join(", ")
        ));
    }

    let textures_zip = folder.join("Textures.zip");
    let mut tw = zip::ZipWriter::new(
        std::fs::File::create(&textures_zip).map_err(|e| format!("{}: {e}", textures_zip.display()))?,
    );
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for t in &textures {
        tw.start_file(format!("Textures/{}", t.file), stored).map_err(|e| format!("Textures.zip: {e}"))?;
        tw.write_all(&t.bytes).map_err(|e| e.to_string())?;
    }
    tw.finish().map_err(|e| format!("Textures.zip: {e}"))?;

    // The ini files.
    let write = |file: &str, text: String| std::fs::write(folder.join(file), text).map_err(|e| format!("{file}: {e}"));
    write("objectlist.ini", format!("[OBJECTS]\ncount={count}\n{objects}"))?;
    let mut list = format!("[MATERIALS]\ncount={}\n", names.len());
    for (i, n) in names.iter().enumerate() {
        let _ = writeln!(list, "MAT_{i}={n}");
    }
    write("materials.ini", format!("{list}\n{sections}"))?;
    write("mat.ini", format!("[MAPS]\ncount={map_count}\n{maps}"))?;
    let length = stage.finish + 15.0;
    write(
        "track.ini",
        format!(
            "[INFO]\nname = {folder_name}\nphysics = tarmac\nauthor = converted from the BeamNG level {level}\nversion = \ndate = \ncomment = Local conversion of the player's own copy; not for distribution\nlength = {:.1}\n",
            stage.finish / 1000.0
        ),
    )?;
    write("settings.ini", "[SETTINGS]\nsunDirection\t= -0.843359,-0.503774,0.186968\n".to_owned())?;
    let mut dl = format!("[DRIVELINE]\ncount={}\n", line.len());
    let at = Line3::new(&line);
    for (i, p) in line.iter().enumerate() {
        let q = frame_k(frame, *p);
        let t = at.tangent(i);
        let _ = writeln!(dl, "K{i}={}, {}, {}, {}, {}, {}, {}, 0", q[0], q[1], q[2], t[0], t[1], t[2], at.at[i]);
    }
    write("driveline.ini", dl)?;
    let mut notes = vec![(START, 0, stage.start)];
    notes.extend(stage.splits.iter().map(|&s| (SPLIT, 0, s)));
    notes.push((FINISH, 0, stage.finish));
    notes.push((END, 0, length.min(at.at.last().copied().unwrap_or(length))));
    let called = pacenotes::notes(&line, &at.at, stage.start, stage.finish);
    notes.extend(called.iter().map(|n| (n.tag, n.flags, n.distance)));
    notes.sort_by(|a, b| a.2.total_cmp(&b.2));
    let mut pn = format!("[PACENOTES]\ncount={}\n", notes.len());
    for (i, (tag, flags, d)) in notes.iter().enumerate() {
        let _ = write!(pn, "\n[P{i}]\ntype={tag}\ndistance={d:.3}\nflag={flags}\n");
    }
    let mut kinds: BTreeMap<u32, usize> = BTreeMap::new();
    for n in &called {
        *kinds.entry(n.tag).or_default() += 1;
    }
    say(format!("pace notes: {} ({kinds:?} by tag)", called.len()));
    write("pacenotes.ini", pn)?;
    // Spawn: the race's start position on the collision, facing along the course.
    let start = [course.start[0], course.start[1], ground(course.start)];
    let s = frame.point(start);
    let yaw = course.heading[0].atan2(course.heading[1]);
    let mut spawn = String::from("[SPAWN]\n");
    for i in 0..16 {
        let v = match i {
            12 => f64::from(s[0]),
            13 => f64::from(s[1]),
            14 => f64::from(s[2]),
            0 | 5 | 10 => 1.0,
            _ => 0.0,
        };
        let _ = writeln!(spawn, "m{i:02}={v}");
    }
    let _ = write!(spawn, "angleX=0\nangleY=0\nangleZ={yaw}\n");
    write("spawn.ini", spawn)?;
    say(format!(
        "stage: start {:.0} m, finish {:.0} m, splits {:?}; origin ({}, {}); folder {}",
        stage.start,
        stage.finish,
        stage.splits.iter().map(|s| s.round()).collect::<Vec<_>>(),
        frame.origin[0],
        frame.origin[1],
        folder.display()
    ));
    report.folder = folder;
    Ok(report)
}

fn write_material(out: &mut String, section: &str, texture: &str, transparent: bool) {
    let technique = if transparent { "Transparent" } else { "Default" };
    let _ = write!(
        out,
        "[{section}]\neffectName=BTB_Diffuse\ntechniqueName={technique}\nambient=0.4,0.4,0.4,1.0\nSpecularPower=4\nSpecularColor=0,0,0\ndiffuseTexture={texture}\n\n"
    );
}

/// The shape a static draws: BeamNG's built `.cdae`, else (none, or one this reader does not read) the `.dae` source
/// a level may ship instead; `None` when neither is in the zip.
fn load_shape(archive: &mut Archive, st: &level::Static) -> Option<Result<crate::cdae::Shape, String>> {
    let path = st.cdae_path();
    let dae = st.shape.trim_start_matches('/');
    let built = archive.find(&path).map(str::to_owned).map(|e| archive.read(&e).and_then(|b| crate::cdae::parse(&b)));
    if matches!(built, Some(Ok(_))) || dae == path {
        return built;
    }
    match archive.find(dae).map(str::to_owned) {
        Some(e) => Some(archive.read(&e).and_then(|b| crate::dae::parse(&String::from_utf8_lossy(&b)))),
        None => built,
    }
}

/// The detail a placed shape collides with: its visible one for a `Visible Mesh` collision type, else its collision
/// levels when it has any; `None` for a shape set not to collide (`collisionType` `None`).
fn collision_detail(st: &level::Static, shape: &crate::cdae::Shape) -> Option<Detail> {
    let solid = st.collision_type.as_deref().is_none_or(|c| !c.eq_ignore_ascii_case("none"));
    let has_collision = shape.details.iter().any(|d| d.size < 0.0);
    let visible_collides = st.collision_type.as_deref().is_some_and(|c| c.to_lowercase().starts_with("visible"));
    let detail = if visible_collides { Some(Detail::Visible) } else { has_collision.then_some(Detail::Collision) };
    detail.filter(|_| solid)
}

/// The race file of a mission folder, or the race file itself.
pub fn race_file(mission: &str) -> String {
    let mission = mission.trim_end_matches('/');
    if mission.ends_with(".race.json") {
        mission.to_owned()
    } else {
        format!("{mission}/race.race.json")
    }
}

/// The mission's name from its `info.json`, else its folder name; a race file's level and file name.
fn mission_name(archive: &mut Archive, level: &str, mission: &str) -> String {
    if mission.starts_with("road") {
        return format!("{level}_{}", mission.replace(':', "_"));
    }
    if let Some(race) = mission.strip_suffix(".race.json") {
        let mut parts = race.rsplit('/');
        let stem = parts.next().unwrap_or(race);
        let level = parts.nth(1).unwrap_or_default();
        return format!("{level}_{stem}");
    }
    let folder = mission.rsplit('/').next().unwrap_or(mission).to_owned();
    archive
        .read_text(&format!("{mission}/info.json"))
        .ok()
        .and_then(|t| crate::json::parse(&t).ok())
        .and_then(|j| j.get("name").and_then(|n| n.str()).map(str::to_owned))
        .filter(|n| !n.trim().is_empty() && !n.starts_with("missions."))
        .unwrap_or(folder)
}

/// The driveline points in RBR's frame: `(x - ox, y - oy, z)`.
fn frame_k(frame: Frame, p: [f64; 3]) -> [f32; 3] {
    [(p[0] - frame.origin[0]) as f32, (p[1] - frame.origin[1]) as f32, p[2] as f32]
}

/// Arc lengths and tangents of a polyline.
struct Line3 {
    points: Vec<[f64; 3]>,
    at: Vec<f64>,
}

impl Line3 {
    fn new(points: &[[f64; 3]]) -> Line3 {
        let line = crate::road::Line::new(points.to_vec(), Vec::new());
        Line3 { points: line.points, at: line.at }
    }

    fn tangent(&self, i: usize) -> [f64; 3] {
        let a = self.points[i.saturating_sub(1)];
        let b = self.points[(i + 1).min(self.points.len() - 1)];
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-9);
        d.map(|v| v / l)
    }
}

/// The driveline and its note distances.
struct Staged {
    line: crate::road::Line,
    start: f64,
    finish: f64,
    splits: Vec<f64>,
}

/// The course with its lead-in. A loop's driveline starts [`LEAD`] m before the start (wrapping round) and ends 10 m
/// short of its first point, the finish 30 m before its end; an open course's starts at the start and its finish is
/// 30 m before its end.
fn staged(course: &Course) -> Staged {
    let mut staged = staged_all(course);
    staged.splits = two_splits(&staged.splits, staged.start, staged.finish);
    staged
}

/// The two splits a stage times (the HUD, `EndRace` and the results carry two, as every retail stage has): of
/// `splits`, the pair nearest a third and two thirds of the way from `start` to `finish`; all of them when one or
/// two; a third and two thirds when the race has none (a quick race's start and finish only).
fn two_splits(splits: &[f64], start: f64, finish: f64) -> Vec<f64> {
    let at = |share: f64| start + (finish - start) * share;
    if splits.is_empty() {
        return vec![at(1.0 / 3.0), at(2.0 / 3.0)];
    }
    let pairs = (0..splits.len()).flat_map(|i| (i + 1..splits.len()).map(move |j| (splits[i], splits[j])));
    pairs
        .min_by(|a, b| {
            let off = |(x, y): (f64, f64)| (x - at(1.0 / 3.0)).abs() + (y - at(2.0 / 3.0)).abs();
            off(*a).total_cmp(&off(*b))
        })
        .map_or_else(|| splits.to_vec(), |(x, y)| vec![x, y])
}

fn staged_all(course: &Course) -> Staged {
    let c = &course.line;
    let total = c.length();
    if course.closed {
        // The course is start .. start + total; the loop closes LOOP_GAP later.
        let full = total + course::LOOP_GAP;
        let ring = c;
        // Driveline: start - LEAD .. start + full - LEAD - 10.
        let pts: Vec<[f64; 3]> = (0..)
            .map(|k| k as f64 * course::STEP)
            .take_while(|d| *d <= full - 10.0)
            .map(|d| {
                let s = (d - LEAD).rem_euclid(full);
                if s <= total {
                    ring.sample(s).0
                } else {
                    // Across the gap: straight from the course's end back to its start.
                    let t = (s - total) / course::LOOP_GAP;
                    let (a, b) = (c.points[c.points.len() - 1], c.points[0]);
                    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
                }
            })
            .collect();
        let line = crate::road::Line::new(pts, Vec::new());
        Staged {
            start: LEAD,
            finish: line.length() - 30.0,
            splits: course.splits.iter().map(|s| s + LEAD).collect(),
            line,
        }
    } else {
        let line = c.stretch(0.0, total, true, course::STEP);
        Staged { start: 5.0, finish: line.length() - 30.0, splits: course.splits.clone(), line }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_splits_nearest_the_thirds() {
        assert_eq!(two_splits(&[4000.0, 7200.0, 13500.0, 17000.0], 0.0, 21000.0), [7200.0, 13500.0]);
        assert_eq!(two_splits(&[100.0, 200.0, 300.0], 0.0, 300.0), [100.0, 200.0]);
        assert_eq!(two_splits(&[500.0], 0.0, 900.0), [500.0]);
        assert_eq!(two_splits(&[], 0.0, 900.0), [300.0, 600.0]);
    }

    #[test]
    fn ground_models_map_to_surfaces() {
        assert_eq!(surface(Some("ASPHALT"), false), 37);
        assert_eq!(surface(Some("RUMBLE_STRIP"), false), 65);
        assert_eq!(surface(Some("COBBLESTONE"), false), 69);
        assert_eq!(surface(Some("GRASS"), false), 113);
        assert_eq!(surface(Some("GRAVEL"), false), 1);
        assert_eq!(surface(Some("DIRT_DUSTY"), false), 133);
        assert_eq!(surface(Some("METAL_TREAD"), false), 37);
        assert_eq!(surface(Some("METAL_TREAD"), true), 64);
        assert_eq!(surface(Some("SOFT_COLLISION_GENERAL"), true), 64);
        assert_eq!(surface(None, true), 64);
        assert_eq!(surface(None, false), 37);
    }

    #[test]
    fn the_frame_swaps_and_keeps_faces_up() {
        let frame = Frame { origin: [100.0, 200.0] };
        assert_eq!(frame.point([101.0, 202.0, 3.0]), [1.0, 3.0, 2.0]);
        // A BeamNG ground face winds clockwise seen from above; in the file frame its corners keep their order.
        let t = Triangle {
            pos: [[100.0, 200.0, 0.0], [100.0, 201.0, 0.0], [101.0, 200.0, 0.0]],
            uv: [[0.0; 2]; 3],
            normal: [[0.0, 0.0, 1.0]; 3],
            material: None,
        };
        assert!(t.normal()[2] < 0.0);
        let mut b = Builder::default();
        add(&mut b, frame, &t, true);
        let p = |i: u32| b.positions[i as usize];
        let [i, j, k] = b.triangles[0];
        let (a, c, d) = (p(i), p(j), p(k));
        let (u, v) = ([c[0] - a[0], c[1] - a[1], c[2] - a[2]], [d[0] - a[0], d[1] - a[1], d[2] - a[2]]);
        assert!(u[2] * v[0] - u[0] * v[2] > 0.0);
        assert_eq!(b.normals[0], [0.0, 1.0, 0.0]);
    }
}
