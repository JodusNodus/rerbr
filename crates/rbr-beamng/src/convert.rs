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
/// Plan tile size of the written objects, metres.
const TILE: f64 = 100.0;

#[derive(Debug, Clone)]
pub struct Options {
    /// The mission folder in the zip (`gameplay/missions/<level>/timeTrial/<mission>`).
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
    /// Drawn objects whose material's triangles average under this area (m²) are drawn only to [`Options::detail_lod`].
    pub detail_area: f64,
    pub detail_lod: f64,
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
            detail_area: 0.02,
            detail_lod: 400.0,
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

    fn holds(&self, t: &Triangle) -> bool {
        t.pos
            .iter()
            .any(|p| self.cells.contains(&((p[0] / self.cell).floor() as i64, (p[1] / self.cell).floor() as i64)))
    }
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
    let race = Race::parse(&archive.read_text(&format!("{}/race.race.json", options.mission))?)?;
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
    let reaches: std::collections::BTreeSet<u64> =
        options.reach.iter().map(|r| r.1).chain([options.draw_corridor]).map(f64::to_bits).collect();
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
    for st in scene.statics.iter().filter(|s| shown(s)) {
        let path = st.cdae_path();
        if !shapes.contains_key(&path) {
            let Some(entry) = archive.find(&path).map(str::to_owned) else {
                *skipped.entry(format!("{path} (missing)")).or_default() += 1;
                continue;
            };
            shapes
                .insert(path.clone(), crate::cdae::parse(&archive.read(&entry)?).map_err(|e| format!("{path}: {e}"))?);
        }
        let shape = &shapes[&path];
        let solid = st.collision_type.as_deref().is_none_or(|c| !c.eq_ignore_ascii_case("none"));
        let has_collision = shape.details.iter().any(|d| d.size < 0.0);
        let visible_collides = st.collision_type.as_deref().is_some_and(|c| c.to_lowercase().starts_with("visible"));
        let col_detail =
            if visible_collides { Some(Detail::Visible) } else { has_collision.then_some(Detail::Collision) };
        if let Some(detail) = col_detail.filter(|_| solid) {
            for part in world::parts(shape, st, detail) {
                for t in part.triangles.iter().filter(|t| col_corridor.holds(t)) {
                    let name = t.material.and_then(|m| part.materials.get(m as usize)).cloned().unwrap_or_default();
                    let ground = mats.get(&name.to_lowercase()).and_then(|m| m.ground.clone());
                    let ground = ground.map(|g| grounds.get(&g.to_uppercase()).cloned().unwrap_or(g));
                    let id = surface(ground.as_deref(), t.normal()[2].abs() < 0.3);
                    let entry = surfaces.entry(format!("{name} ({})", ground.unwrap_or_default())).or_insert((id, 0));
                    entry.1 += 1;
                    add(col.entry((tile_of(t), id)).or_default(), frame, t, false);
                    col_triangles.push(*t);
                }
            }
        }
        let skip = st.hidden || options.skip.iter().any(|k| path.to_lowercase().contains(&k.to_lowercase()));
        if skip {
            *skipped.entry(path).or_default() += 1;
            continue;
        }
        let corridor = &draw_corridors[&rule(&options.reach, &path).unwrap_or(options.draw_corridor).to_bits()];
        let lod = rule(&options.lod, &path).map_or(0, |m| m.round().max(1.0) as u32);
        for part in world::parts(shape, st, Detail::Visible) {
            for t in part.triangles.iter().filter(|t| corridor.holds(t)) {
                *per_shape.entry(path.clone()).or_default() += 1;
                let name = t.material.and_then(|m| part.materials.get(m as usize)).cloned().unwrap_or_default();
                let b = drawn.entry((tile_of(t), name.to_lowercase(), lod));
                add(b.or_insert_with(|| Builder { weld: true, ..Builder::default() }), frame, t, true);
            }
        }
    }
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

    let folder_name = options.name.clone().unwrap_or_else(|| mission_name(&mut archive, &options.mission));
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
    let mut by_name: Vec<&String> = drawn.keys().map(|(_, m, _)| m).collect();
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
    let mut put = |zw: &mut zip::ZipWriter<std::fs::File>, file: &str, b: &Builder, material: &str, entries: &str| {
        let x = xwrite::write(&XMesh {
            positions: &b.positions,
            normals: &b.normals,
            uvs: &b.uvs,
            triangles: &b.triangles,
            material,
        });
        zw.start_file(format!("Objects/{file}.x"), deflate).map_err(zerr)?;
        zw.write_all(&x).map_err(|e| e.to_string())?;
        let _ = write!(objects, "\n[Object_{count}]\nfile={file}\nxform=identity\ncloneCount=1\n{entries}");
        count += 1;
        Ok::<(), String>(())
    };
    for (((tx, ty), id), b) in &col {
        let entries =
            "LODIn=0\nVisible=0\nDrawInstanced=0\nMoveable=0\nShadowCaster=0\nCollisionModel=0\nCollisionOnly=1\n";
        put(&mut zw, &format!("col_{tx}_{ty}_{id}"), b, &format!("collision\\surface_{id}"), entries)?;
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
        put(&mut zw, &format!("draw_{tx}_{ty}_{index}"), b, &material_of[name], &entries)?;
    }
    zw.finish().map_err(zerr)?;
    say(format!("objects: {count} ({detail_objects} drawn to {} m only)", options.detail_lod));

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

/// The mission's name from its `info.json`, else its folder name.
fn mission_name(archive: &mut Archive, mission: &str) -> String {
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
