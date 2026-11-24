//! The interface a launcher or the command line calls: convert one route of the install into a BTB stage. The course
//! (`route`), the collision (`collision`, `surface`) and the drawn world are read, put in the stage's world (`frame`)
//! and written by `stage`. Nothing here prints, reads the environment or exits the process.
use crate::install::{Install, Route};
use crate::{
    collision, frame, material, mesh, objects, pssg::Pssg, route, shade, solid, stage, surface, texture, trees, vista,
};
use rbr_ac_tracks::btb::{
    control::Control,
    geometry::{Grid, Triangle},
    road::Line,
    stage::CELL_LIMIT,
};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::rc::Rc;

pub use stage::Report;

pub struct Options {
    /// The stage folder's name; [`default_name`] when `None`.
    pub name: Option<String>,
    /// The folder the stage goes in; the player's tracks folder when `None`.
    pub out: Option<PathBuf>,
    pub replace: bool,
    pub draw_corridor: f64,
    pub collision_corridor: f64,
    pub lod: f64,
    pub control: Control,
}

impl Options {
    pub fn new(control: Control) -> Options {
        Options {
            name: None,
            out: None,
            replace: false,
            // An area's terrain is a few million triangles: all of it is drawn.
            draw_corridor: 5000.,
            collision_corridor: 75.,
            lod: 1000.,
            control,
        }
    }
}

/// The stage folder of a route when none is given: `DR_greece_ghymno_rally_01_route_0`.
pub fn default_name(route: &Route) -> String {
    format!("DR_{}_{}_route_{}", route.location, route.area, route.index)
}

/// Converts the route `id` (`location/area/route_<n>`) of `install`.
pub fn import(install: &Install, id: &str, options: &Options) -> Result<Report, String> {
    let route = install.route(id)?;
    if !route.is_rally() && !route.is_rallycross() && route.area != "pikes_peak" {
        return Err(format!("{id}: only rally stages, rallycross and Pikes Peak convert (not the frontend)"));
    }
    let control = &options.control;
    control.say(&format!("reading {}", route.id()));
    let (length, line, start, finish, splits, circuit) = if route.is_rallycross() {
        let (lap, line, start, finish, splits) = lap_line(&route)?;
        control.say(&format!("lap {:.0} m on a {:.0} m closed road line", lap.length, line.length()));
        (lap.length, line, start, finish, splits, true)
    } else {
        let course = route::course(&route)?;
        let (line, start, finish, splits) = centre_line(&route, &course)?;
        control.say(&format!("course {:.0} m on a {:.0} m road line", course.length(), line.length()));
        (course.length(), line, start, finish, splits, false)
    };
    control.check()?;
    let faces = collision::read(&route)?;
    let solids = solid_props(&route, control)?;
    let judged = if circuit { (0., line.length()) } else { (start, finish) };
    let (collision, surface_counts) = collision_triangles(&faces, solids, &line, judged.0, judged.1)?;
    control.say(&format!("{} collision triangles", collision.len()));
    drop(faces);
    control.check()?;
    let (mut materials, mut drawn, layer) = terrain(&route, control)?;
    control.say(&format!("{} drawn terrain triangles in {} materials", drawn.len(), materials.len()));
    let terrain_triangles = drawn.len();
    let instanced = props(&route, &line, control, &mut materials, &mut drawn)?;
    control.say(&format!(
        "{} baked prop and tree triangles, {} instanced models in {} placements",
        drawn.len() - terrain_triangles,
        instanced.models.len(),
        instanced.placed.iter().map(|p| p.matrices.len()).sum::<usize>()
    ));
    let grade = crate::grade::Grade::day(&route);
    let input = stage::Input {
        source: route.dir.clone(),
        route: route.id(),
        title: route::title(&route),
        country: route::country(&route),
        place: route::place(&route),
        length,
        line,
        start,
        finish,
        splits,
        circuit,
        materials,
        drawn,
        layer,
        instanced,
        collision,
        collision_source: "track.jpk",
        surface_counts,
        lighting: crate::lighting::Lighting::day(&route).map(|l| grade.as_ref().map_or(l, |g| l.graded(g.lift))),
        grade: grade.map(|g| g.png),
        grass: crate::grass::read(route.area_dir())?,
    };
    let out = match &options.out {
        Some(out) => out.clone(),
        None => rbr_btb::player::tracks().ok_or("no player tracks folder; pass --out")?,
    };
    let mut stage_options = stage::Options {
        out,
        name: options.name.clone().unwrap_or_else(|| default_name(&route)),
        draw_corridor: options.draw_corridor,
        collision_corridor: options.collision_corridor,
        lod: options.lod,
        replace: options.replace,
        control: options.control.clone(),
    };
    match stage::convert(&input, &stage_options) {
        // Dense collision (Panzerplatte's kerbs and walls) can pass a cell's vertex limit: once more, narrower.
        Err(e) if e.starts_with(CELL_LIMIT) => {
            stage_options.collision_corridor *= 0.5;
            control.say(&format!("{e}; collision narrowed to {:.0} m", stage_options.collision_corridor));
            stage::convert(&input, &stage_options)
        }
        done => done,
    }
}

/// Farthest a gate's crossing may lie from the road line, metres.
const GATE_OFF_ROAD: f64 = 8.;

/// The road's centre line in the stage's world (the AI track's, shared by a route's halves), with the arc lengths of
/// its start, finish and splits: each gate's crossing projected onto it. A line running the other way is reversed.
fn centre_line(route: &Route, course: &route::Course) -> Result<(Line, f64, f64, Vec<f64>), String> {
    let road = route::road(route)?;
    let mut points: Vec<[f64; 3]> = road.iter().map(|s| frame::world(s.centre())).collect();
    let mut width: Vec<f64> = road.iter().map(|s| f64::from(s.width)).collect();
    let gate = |distance: f64| {
        course.gates.iter().min_by(|a, b| (a.distance - distance).abs().total_cmp(&(b.distance - distance).abs()))
    };
    let at = |line: &Line, distance: f64| -> Result<f64, String> {
        let g = gate(distance).ok_or("course without gates")?;
        let (s, off) = rbr_ac_tracks::assetto::ai::project(line, frame::world(g.crossing));
        if off > GATE_OFF_ROAD {
            return Err(format!("the gate at {distance:.0} m lies {off:.1} m off the AI road"));
        }
        Ok(s)
    };
    let mut line = Line::new(points.clone(), width.clone());
    if at(&line, course.finish)? < at(&line, course.start)? {
        points.reverse();
        width.reverse();
        line = Line::new(points, width);
    }
    let (start, finish) = (at(&line, course.start)?, at(&line, course.finish)?);
    let splits = course.splits.iter().map(|&d| at(&line, d)).collect::<Result<Vec<_>, _>>()?;
    if finish - start < 100. || splits.iter().any(|&s| s <= start || s >= finish) {
        return Err(format!("the course does not run along the AI road ({start:.0} to {finish:.0} m)"));
    }
    Ok((line, start, finish, splits))
}

/// A rallycross lap as a ring (the first point repeated at the end) in the stage's world: the race branch of the AI
/// road, starting just behind the spawn (3.7 m behind the lap line), where the lap is timed. Returns the lap, the ring, its `start` and `finish`
/// (one lap on from it, arc lengths) and the time splits' arc lengths from the pole.
fn lap_line(route: &Route) -> Result<(route::Lap, Line, f64, f64, Vec<f64>), String> {
    let lap = route::lap(route)?;
    let road = route::lap_road(route, lap.line)?;
    let mut points: Vec<[f64; 3]> = road.iter().map(|s| frame::world(s.centre())).collect();
    let mut width: Vec<f64> = road.iter().map(|s| f64::from(s.width)).collect();
    points.push(points[0]);
    width.push(width[0]);
    let ring = Line::new(points, width);
    if (ring.length() - lap.length).abs() > lap.length * 0.03 {
        return Err(format!("the AI loop is {:.0} m, the lap {:.0} m", ring.length(), lap.length));
    }
    let on = |p: [f32; 3]| -> Result<f64, String> {
        let (s, off) = rbr_ac_tracks::assetto::ai::project(&ring, frame::world(p));
        if off > GATE_OFF_ROAD {
            return Err(format!("a lap marker lies {off:.1} m off the AI road"));
        }
        Ok(s)
    };
    // The car stands where rally stages' grids do, 3.7 m behind the line, and the lap is timed from there (`stage`).
    let start = (on(lap.line)? - stage::GRID_BEHIND - stage::SEAM_BEHIND).rem_euclid(ring.length());
    let along = |s: f64| (s - start).rem_euclid(ring.length());
    let splits = lap.splits.iter().map(|&p| on(p).map(along)).collect::<Result<Vec<_>, _>>()?;
    let finish = start + ring.length();
    Ok((lap, ring, start, finish, splits))
}

/// The hulls of the route's solid props (`objects::Shape::solid`) placed at their instances, as collision triangles with
/// no surface yet; none where the route has no `objects.ens`.
fn solid_props(route: &Route, control: &Control) -> Result<Vec<Triangle>, String> {
    if !route.dir.join("objects.ens").is_file() {
        return Ok(Vec::new());
    }
    let layout = objects::read(route)?;
    let mut out = Vec::new();
    let (mut solid, mut small, mut soft) = (0, 0, 0);
    for instance in &layout.instances {
        for shape in &layout.entities[instance.entity].shapes {
            let hull = if shape.solid() { solid::triangles(shape, &instance.matrix) } else { Vec::new() };
            match (shape.solid(), hull.is_empty()) {
                (true, false) => solid += 1,
                (true, true) => small += 1,
                (false, _) => soft += 1,
            }
            out.extend(hull);
        }
    }
    control.say(&format!(
        "{solid} solid hulls ({} triangles), {small} lower than {} m, {soft} that give way or are vegetation",
        out.len(),
        solid::MIN_RISE
    ));
    Ok(out)
}

/// The collision in the stage's world with RBR ids, and the count of each code's triangles. `DEFA` takes the road
/// surface of the stage's class, judged from the codes under the course; an unknown code is reported.
fn collision_triangles(
    faces: &[collision::Face],
    solids: Vec<Triangle>,
    line: &Line,
    start: f64,
    finish: f64,
) -> Result<(Vec<Triangle>, BTreeMap<String, usize>), String> {
    let mut counts = BTreeMap::<String, usize>::new();
    let mut unknown = BTreeMap::<String, usize>::new();
    let mut triangles: Vec<Triangle> = faces
        .iter()
        .map(|f| {
            let code = f.code();
            *counts.entry(code.clone()).or_default() += 1;
            let material = match surface::rbr(&code) {
                Some(surface::Surface::Id(id)) => Some(u32::from(id)),
                Some(surface::Surface::Default) => None,
                None => {
                    *unknown.entry(code).or_default() += 1;
                    None
                }
            };
            // The file's winding is counter-clockwise about +y (right handed); the BTB mesh's is the other way.
            let [a, b, c] = f.pos.map(frame::world);
            Triangle { pos: [a, c, b], uv: [[0.; 2]; 3], normal: [[0.; 3]; 3], material }
        })
        .collect();
    if !unknown.is_empty() {
        return Err(format!("surface codes without an RBR surface: {unknown:?}"));
    }
    let physics = {
        let grid = Grid::new(triangles.iter().collect(), 4.);
        let mut along = Vec::new();
        let mut s = start;
        while s < finish {
            let p = line.sample(s).0;
            if let Some(id) = grid.ground(p[0], p[1], p[2]).and_then(|(_, t)| t.material) {
                along.push((id as u8, 2.5));
            }
            s += 2.5;
        }
        surface::physics(along)
    };
    // The solid props, after the road's surface is judged (a trunk is no road).
    triangles.extend(solids);
    let default = u32::from(surface::default_for(physics));
    for t in &mut triangles {
        t.material.get_or_insert(default);
    }
    Ok((triangles, counts))
}

/// The terrain's materials, triangles and the layers' corners of each.
type Terrain = (Vec<stage::Material>, Vec<Triangle>, Vec<stage::Corners>);

/// The drawn terrain in the stage's world: the route's `routesplit.pssg` where the area has one (Greece), and the area's
/// `tracksplit.pssg` (its vista there, all of it elsewhere); the ground is shaded by `shade` (layers, macro colour and lightmap per pixel), the rest drawn with its first layer's texture.
fn terrain(route: &Route, control: &Control) -> Result<Terrain, String> {
    let mut library = texture::Library::day(route);
    let files: Vec<_> = [route.dir.join("routesplit.pssg"), route.area_dir().join("tracksplit.pssg")]
        .into_iter()
        .filter(|p| p.is_file())
        .collect();
    let (mut materials, mut triangles, mut corners) = (Vec::new(), Vec::new(), Vec::new());
    let mut shader = shade::Shader::default();
    for (file, path) in files.iter().enumerate() {
        control.check()?;
        control.say(&format!("reading {}", path.file_name().unwrap_or_default().to_string_lossy()));
        let pssg = Pssg::read(path)?;
        let sources = material::read(&pssg);
        let mut index = HashMap::<(String, u8), u32>::new();
        let shaded = shader.shade(mesh::terrain(&pssg)?, &sources, &mut library);
        for (level, m) in decal_levels(shaded.meshes) {
            let slot = *index.entry((m.material.clone(), level)).or_insert_with(|| {
                let layers = shaded.layers.get(&m.material).cloned();
                // A shaded material's fallback is its first layer, which `stage` takes from the layers.
                let dds = if layers.is_some() {
                    None
                } else {
                    let source = sources.get(&m.material);
                    match source.filter(|s| material::is_vista(s)).and_then(|s| vista::diffuse(s, &mut library)) {
                        Some(dds) => Some(dds),
                        None => source.and_then(material::diffuse).and_then(|r| library.get(r)).map(|t| {
                            if level == 0 || level == FADES {
                                t.dds()
                            } else {
                                faded(&t, level)
                            }
                        }),
                    }
                };
                // Decals lie on the ground they mark: drawn with their alpha.
                let transparent = m.node.starts_with("DECAL");
                materials.push(stage::Material {
                    name: format!("{file}:{}:{level}", m.material),
                    dds,
                    transparent,
                    range: None,
                    layers,
                    grows: !transparent,
                    ..stage::Material::default()
                });
                materials.len() as u32 - 1
            });
            // A shaded material draws its atlas (UV set 0) and takes the layers' coordinates and weights beside it.
            let layers = shaded.layers.get(&m.material).map(|l| l.count);
            for &[a, b, c] in &m.triangles {
                // Counter-clockwise in DiRT Rally's right-handed frame; the BTB mesh's winding the other way.
                let at = [a, c, b].map(|i| i as usize);
                let atlas = m.uv2.as_ref().filter(|_| layers.is_some());
                triangles.push(Triangle {
                    pos: at.map(|i| frame::world(m.positions[i])),
                    uv: at.map(|i| atlas.map_or_else(|| m.uv.get(i), |uv2| uv2.get(i)).copied().unwrap_or_default()),
                    normal: at.map(|i| m.normals.get(i).map_or([0., 0., 1.], |&n| frame::world_dir(n))),
                    material: Some(slot),
                });
                corners.push(stage::Corners {
                    uv: at.map(|i| m.uv.get(i).copied().unwrap_or_default()),
                    weights: at
                        .map(|i| shade::weights(m.colour.as_ref().map_or([0; 4], |c| c[i]), layers.unwrap_or(1))),
                });
            }
        }
    }
    Ok((materials, triangles, corners))
}

/// Steps a decal's vertex alpha is drawn in.
const FADES: u8 = 4;

/// `meshes` with each decal split by the fade of its triangles: the decal shaders (`decal_ao_vc.fx`) multiply the
/// texture's alpha by the vertex `Color`'s alpha (a tyre strip fades along and across, mean 0.33; splats 0.95), which
/// BTB vertices do not hold. A triangle takes the level (1..=`FADES`) nearest its mean alpha, drawn with the texture's
/// alpha scaled to it ([`faded`]); the nearly clear ones are left out. Everything else is level 0, as it was.
fn decal_levels(meshes: Vec<mesh::Mesh>) -> Vec<(u8, mesh::Mesh)> {
    let mut out = Vec::new();
    for m in meshes {
        let Some(colour) = m.colour.as_ref().filter(|_| m.node.starts_with("DECAL")) else {
            out.push((0, m));
            continue;
        };
        let mut by_level = vec![Vec::new(); usize::from(FADES)];
        for t in &m.triangles {
            let alpha = t.iter().map(|&i| f32::from(colour[i as usize][3])).sum::<f32>() / 765.;
            let level = (alpha * f32::from(FADES)).round() as usize;
            if level > 0 {
                by_level[level - 1].push(*t);
            }
        }
        for (i, triangles) in by_level.into_iter().enumerate().filter(|(_, t)| !t.is_empty()) {
            out.push((i as u8 + 1, mesh::Mesh { triangles, ..m.clone() }));
        }
    }
    out
}

/// A decal texture with its alpha scaled to `level` of [`FADES`].
fn faded(texture: &texture::Texture, level: u8) -> Vec<u8> {
    let mut rgba = texture.rgba();
    let scale = f32::from(level) / f32::from(FADES);
    rgba.chunks_exact_mut(4).for_each(|p| p[3] = (f32::from(p[3]) * scale).round() as u8);
    rbr_ac_tracks::btb::textures::encode(true, texture.width, texture.height, rgba)
}

/// A prop is drawn as far as this many times its size: a metre-wide rock to 100 m, a tree of ten metres to a kilometre.
const PROP_REACH: f32 = 100.;
/// A model of `t` triangles is drawn no farther than this over the square root of `t`: dense trees stand out to a few
/// hundred metres, a rock to a kilometre. An instanced model costs its triangles in every view, not in memory.
const PROP_COST: f64 = 60_000.;
/// The draw ranges a prop takes, metres, the least that holds its size ([`PROP_REACH`]) and its cost
/// ([`PROP_COST`]); materials are split by them.
const PROP_RANGES: [f64; 8] = [100., 150., 225., 340., 500., 750., 1000., 3000.];
/// The distant forest: `trees.bin`'s `*_dist_*` types are tiles of 500 m of far trees (about a hundred triangles each),
/// drawn far past where the single trees are.
const DIST_REACH: f64 = 3000.;
/// A tree's far level, its billboard (4 triangles from the game's own 2-triangle far level), is drawn from the model's
/// range to [`DIST_REACH`] for the trees within this far of the road, metres.
const BILLBOARD_REACH: f64 = 1000.;
/// The most triangles a stage writes (terrain, single props, far levels and each instanced model once) on the sample
/// routes: the viewer holds every one in memory (the instances only their transforms).
#[cfg(test)]
const DRAWN_BUDGET: usize = 6_000_000;

/// Whether a shader draws its diffuse texture's alpha (cut-out leaves, wire fences and branches of `Object_Cov` and
/// `Object_2SidedCov`, blended signs; the other props' alpha is a gloss or emission mask: `Object_Emis`, `Object_Norm`
/// and `Object_Stones` carry DXT5 with varying alpha too). Trunks and `foliage_environment` are opaque.
fn blended(shader: &str) -> bool {
    let s = shader.to_lowercase();
    s.contains("blend")
        || s.contains("cov.fx")
        || ((s.starts_with("foliage") || s.starts_with("facing_foliage") || s.starts_with("treesheet"))
            && !s.contains("trunk"))
}

/// One of the three files props are drawn from, read once.
struct Source {
    types: mesh::Types,
    materials: BTreeMap<String, material::Material>,
    /// Each type's longest side in its own space, metres, and its triangles.
    sizes: Vec<(f32, usize)>,
}

impl Source {
    fn read(path: &std::path::Path) -> Result<Source, String> {
        let pssg = Pssg::read(path)?;
        let types = mesh::types(&pssg)?;
        let sizes = types
            .meshes
            .iter()
            .map(|meshes| {
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                for p in meshes.iter().flat_map(|m| &m.positions) {
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
                let side = (0..3).map(|k| (hi[k] - lo[k]).max(0.)).fold(0., f32::max);
                (side, meshes.iter().map(|m| m.triangles.len()).sum())
            })
            .collect();
        Ok(Source { types, materials: material::read(&pssg), sizes })
    }
}

/// The placements of one type drawn over one range, mirrored or not: instanced once there are two.
struct Group {
    source: Rc<Source>,
    file: String,
    at: usize,
    range: f64,
    mirrored: bool,
    /// Each placement's matrix and the turn its cards take ([`facing`]).
    places: Vec<(mesh::Mat, [f32; 2])>,
}

/// Props and trees gathered into the drawn triangles and the instances.
struct Placer<'a> {
    route: &'a Route,
    line: &'a Line,
    library: texture::Library,
    sources: HashMap<String, Option<Rc<Source>>>,
    /// `(file, material id, range in, range out)` -> index in the stage's materials.
    index: HashMap<(String, String, u64, u64), u32>,
    /// By file, type, range and mirroring.
    groups: BTreeMap<(String, usize, u64, bool), Group>,
    lod_in: BTreeMap<u32, f64>,
    placed: usize,
    unresolved: BTreeMap<String, usize>,
}

impl Placer<'_> {
    /// The index of `material` of `source`, drawn from `lod.0` to `lod.1` metres, added to `materials` the first time.
    fn slot(
        &mut self,
        (file, source): (&str, &Source),
        material: &str,
        lod: (f64, f64),
        materials: &mut Vec<stage::Material>,
    ) -> u32 {
        let key = (file.to_owned(), material.to_owned(), lod.0 as u64, lod.1 as u64);
        if let Some(&slot) = self.index.get(&key) {
            return slot;
        }
        let found = source.materials.get(material);
        let texture = found.and_then(material::diffuse).and_then(|r| self.library.get(r));
        let transparent = found.is_some_and(|f| blended(&f.shader))
            && texture.as_ref().is_some_and(|t| t.format == texture::Format::Dxt5);
        // Solid props take their normal and specular maps; cut-outs (leaves, wire) keep the flat card.
        let mut solid = |input: &str| {
            found.filter(|_| !transparent).and_then(|f| f.textures.get(input)).and_then(|r| self.library.get(r))
        };
        let (bumped, shiny) = (solid("TNormalMap"), solid("TSpecularMap"));
        materials.push(stage::Material {
            name: format!("{file}:{material}@{}-{}", lod.0, lod.1),
            dds: texture.map(|t| t.dds()),
            transparent,
            range: Some(lod.1),
            normal: bumped.as_ref().map(|t| t.normal_dds()),
            specular: shiny.as_ref().map(|t| t.specular_dds()),
            ..stage::Material::default()
        });
        let slot = materials.len() as u32 - 1;
        if lod.0 > 0. {
            self.lod_in.insert(slot, lod.0);
        }
        self.index.insert(key, slot);
        slot
    }

    /// Draws the type `key` of `file` at `matrix`; one far from the road for its size is left out. A type's models are
    /// gathered by range and drawn by `finish`; its far billboards and the `*_dist_*` tiles are written here.
    fn place(
        &mut self,
        file: &str,
        key: &str,
        matrix: &mesh::Mat,
        materials: &mut Vec<stage::Material>,
        out: &mut Vec<Triangle>,
    ) -> Result<(), String> {
        if !self.sources.contains_key(file) {
            let dir =
                if file.starts_with("route_") { self.route.dir.clone() } else { self.route.area_dir().to_owned() };
            let path = dir.join(file);
            let source = if path.is_file() { Some(Rc::new(Source::read(&path)?)) } else { None };
            self.sources.insert(file.to_owned(), source);
        }
        let Some(source) = self.sources.get(file).and_then(Clone::clone) else {
            *self.unresolved.entry(format!("{file} (missing)")).or_default() += 1;
            return Ok(());
        };
        let Some(&at) = source.types.index.get(key) else {
            *self.unresolved.entry(key.to_owned()).or_default() += 1;
            return Ok(());
        };
        let meshes = &source.types.meshes[at];
        let scale = (0..3).map(|i| matrix[i][..3].iter().map(|v| v * v).sum::<f32>().sqrt()).fold(0., f32::max);
        let (side, triangles) = source.sizes[at];
        let size = side * scale;
        let tile = key.contains("_dist_");
        let reach = if tile {
            DIST_REACH
        } else {
            f64::from(size * PROP_REACH).min(PROP_COST / (triangles.max(1) as f64).sqrt())
        };
        let range = PROP_RANGES.iter().copied().find(|&r| r >= reach).unwrap_or(DIST_REACH);
        let here = frame::world([matrix[3][0], matrix[3][1], matrix[3][2]]);
        let (along, distance) = rbr_ac_tracks::assetto::ai::project(self.line, here);
        if meshes.is_empty() {
            return Ok(());
        }
        let (road, forward) = self.line.sample(along);
        let sprite = source.types.sprites[at].as_ref();
        // A tree's far level is a baked quad pair facing the road (4 triangles each: the model's own turn is the
        // placement's, the billboard's the road's), drawn from the model's range on; the tiles stay models, drawn from
        // 0, with their billboard past 8 m from the road.
        if let Some(sprite) = sprite.filter(|_| if tile { distance > 8. } else { distance <= BILLBOARD_REACH }) {
            let lod = if tile { (0., range) } else { (range, DIST_REACH) };
            let slot = self.slot((file, &source), &sprite.material, lod, materials);
            out.extend(billboard(sprite, matrix, (road, forward), slot));
            if tile {
                self.placed += 1;
                return Ok(());
            }
        }
        if distance > range + f64::from(size) {
            return Ok(());
        }
        let face = facing(matrix, road, forward);
        if tile {
            let triangles = bake(meshes, matrix, face, |m| self.slot((file, &source), m, (0., range), materials));
            self.placed += meshes.len();
            out.extend(triangles);
            return Ok(());
        }
        let mirrored = mesh::determinant(matrix) < 0.;
        let group = self.groups.entry((file.to_owned(), at, range as u64, mirrored)).or_insert_with(|| Group {
            source: source.clone(),
            file: file.to_owned(),
            at,
            range,
            mirrored,
            places: Vec::new(),
        });
        group.places.push((*matrix, face));
        Ok(())
    }

    /// The gathered models: one drawn as instances where a type holds two placements of a range, else baked.
    fn finish(mut self, materials: &mut Vec<stage::Material>, out: &mut Vec<Triangle>) -> stage::Instanced {
        let mut instanced = stage::Instanced::default();
        for group in std::mem::take(&mut self.groups).into_values() {
            let meshes = &group.source.types.meshes[group.at];
            let lod = (0., group.range);
            if let [(matrix, face)] = group.places[..] {
                out.extend(bake(meshes, &matrix, face, |m| self.slot((&group.file, &group.source), m, lod, materials)));
                self.placed += meshes.len();
                continue;
            }
            let mut placed =
                stage::Placed { models: Vec::new(), matrices: group.places.iter().map(|(m, _)| xform(m)).collect() };
            for m in meshes {
                let slot = self.slot((&group.file, &group.source), &m.material, lod, materials);
                placed.models.push(instanced.models.len());
                instanced.models.push(stage::Model { triangles: model(m, group.mirrored), material: slot });
            }
            self.placed += meshes.len() * group.places.len();
            instanced.placed.push(placed);
        }
        instanced.lod_in = std::mem::take(&mut self.lod_in);
        instanced
    }
}

/// `matrix` (DiRT Rally's, row vectors) as the `.xform` of a model written in the BTB mesh's axes: `F M F` with
/// `F = diag(1, 1, -1)` ([`frame::world`] and the mesh's `(x, height, y)` swap leave `(x, y, -z)`), its translation
/// `F t`.
fn xform(matrix: &mesh::Mat) -> [f32; 16] {
    let flip = |k: usize| if k == 2 { -1. } else { 1. };
    std::array::from_fn(|n| {
        let (i, j) = (n / 4, n % 4);
        matrix[i][j] * flip(j) * if i < 3 { flip(i) } else { 1. }
    })
}

/// The triangles of `meshes` placed at `matrix` in the stage's world, the cards turned to `face`; `slot` gives a
/// material's index.
fn bake(meshes: &[mesh::Mesh], matrix: &mesh::Mat, face: [f32; 2], mut slot: impl FnMut(&str) -> u32) -> Vec<Triangle> {
    let mirrored = mesh::determinant(matrix) < 0.;
    let mut out = Vec::new();
    for m in meshes {
        let slot = slot(&m.material);
        if is_cards(m) {
            out.extend(cards(m, matrix, face, slot));
            continue;
        }
        out.extend(m.triangles.iter().map(|&[a, b, c]| {
            // Counter-clockwise in DiRT Rally's frame, the BTB mesh's winding the other way; a mirrored
            // placement turned it already.
            let corners = if mirrored { [a, b, c] } else { [a, c, b] }.map(|i| i as usize);
            Triangle {
                pos: corners.map(|i| frame::world(mesh::point(matrix, m.positions[i]))),
                uv: corners.map(|i| m.uv.get(i).copied().unwrap_or_default()),
                normal: corners
                    .map(|i| m.normals.get(i).map_or([0., 0., 1.], |&n| frame::world_dir(mesh::direction(matrix, n)))),
                material: Some(slot),
            }
        }));
    }
    out
}

/// `m` in its own space for a model placed by [`xform`]: wound as [`bake`] winds it (a mirrored placement flips the
/// winding again, so its model is wound the other way). Cards cannot turn to the road per placement: each is two
/// upright cards crossed at 90 degrees about the type's vertical axis, drawn from both sides.
fn model(m: &mesh::Mesh, mirrored: bool) -> Vec<Triangle> {
    let triangle =
        |corners: [usize; 3], pos: &dyn Fn(usize) -> [f32; 3], normal: &dyn Fn(usize) -> [f32; 3]| Triangle {
            pos: corners.map(|i| frame::world(pos(i))),
            uv: corners.map(|i| m.uv.get(i).copied().unwrap_or_default()),
            normal: corners.map(normal),
            material: None,
        };
    if !is_cards(m) {
        let normal = |i: usize| m.normals.get(i).map_or([0., 0., 1.], |&n| frame::world_dir(n));
        return m
            .triangles
            .iter()
            .map(|&[a, b, c]| {
                let corners = if mirrored { [a, b, c] } else { [a, c, b] }.map(|i| i as usize);
                triangle(corners, &|i| m.positions[i], &normal)
            })
            .collect();
    }
    let offsets = m.uv2.as_deref().unwrap_or_default();
    let up = |_| [0., 1., 0.];
    let mut out = Vec::new();
    for plane in [0, 2] {
        let pos = |i: usize| {
            let (c, o) = (m.positions[i], offsets[i]);
            let mut p = [c[0], c[1] + o[1], c[2]];
            p[plane] += o[0];
            p
        };
        for &[a, b, c] in &m.triangles {
            let corners = [a, b, c].map(|i| i as usize);
            out.push(triangle(corners, &pos, &up));
            out.push(triangle([corners[0], corners[2], corners[1]], &pos, &up));
        }
    }
    out
}

/// DiRT Rally's plan direction of a stage's plan vector (x, -z of DiRT Rally's), unit.
fn plan_direction(plan: [f64; 2]) -> [f32; 2] {
    let len = plan[0].hypot(plan[1]).max(1e-6);
    [(plan[0] / len) as f32, (-plan[1] / len) as f32]
}

/// The direction (in DiRT Rally's plan, unit) a camera-facing sprite at `matrix`'s origin is turned to: between the
/// road point `road` and back along the road (`forward` reversed) the way the driver comes, so it is seen well from the
/// side and from ahead.
fn facing(matrix: &mesh::Mat, road: [f64; 3], forward: [f64; 3]) -> [f32; 2] {
    let at = mesh::point(matrix, [0.; 3]);
    let to = plan_direction([road[0] - f64::from(at[0]), road[1] + f64::from(at[2])]);
    let back = plan_direction([-forward[0], -forward[1]]);
    let sum = [to[0] + back[0], to[1] + back[1]];
    let len = sum[0].hypot(sum[1]);
    if len < 0.3 {
        to
    } else {
        [sum[0] / len, sum[1] / len]
    }
}

/// Whether `m` is camera-facing cards: every triangle's corners at one point, their offsets in the second `ST`
/// stream. `facing_foliage` leaves (olives, birches, bushes) and the `*_facing` trees (`treesheet_foliage_norm`, a
/// whole tree on one card; the `*_dist_*` tiles share that shader with real geometry).
fn is_cards(m: &mesh::Mesh) -> bool {
    let point = |t: &[u32; 3]| {
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-3 && (a[k] - c[k]).abs() < 1e-3)
    };
    m.uv2.as_ref().is_some_and(|u| u.len() == m.positions.len())
        && !m.triangles.is_empty()
        && m.triangles.iter().all(point)
}

/// Camera-facing cards ([`is_cards`]) as placed ones: the file keeps each card's four vertices at its centre with their
/// offsets (right, up, metres of the type's own space) in the second `ST` stream, and the game's shader turns the card
/// to the camera. Upright cards turned to `face` (see [`facing`]).
fn cards(m: &mesh::Mesh, matrix: &mesh::Mat, face: [f32; 2], slot: u32) -> Vec<Triangle> {
    let norm = |i: usize| matrix[i][..3].iter().map(|v| v * v).sum::<f32>().sqrt();
    let (across, up) = (norm(0).max(norm(2)), norm(1));
    let right = [face[1], -face[0]];
    let offsets = m.uv2.as_deref().unwrap_or_default();
    let corner = |i: usize| {
        let (c, o) = (mesh::point(matrix, m.positions[i]), offsets[i]);
        [c[0] + right[0] * o[0] * across, c[1] + o[1] * up, c[2] + right[1] * o[0] * across]
    };
    let normal = frame::world_dir([face[0] * 0.5, 0.5, face[1] * 0.5]);
    m.triangles
        .iter()
        .map(|&[a, b, c]| {
            let (oa, ob, oc) = (offsets[a as usize], offsets[b as usize], offsets[c as usize]);
            // Counter-clockwise seen from the viewer in DiRT Rally's frame, `(a, c, b)` for the BTB mesh.
            let ccw = (ob[0] - oa[0]) * (oc[1] - oa[1]) - (ob[1] - oa[1]) * (oc[0] - oa[0]) > 0.;
            let corners = if ccw { [a, c, b] } else { [a, b, c] }.map(|i| i as usize);
            Triangle {
                pos: corners.map(|i| frame::world(corner(i))),
                uv: corners.map(|i| m.uv.get(i).copied().unwrap_or_default()),
                normal: [normal; 3],
                material: Some(slot),
            }
        })
        .collect()
}

/// A tree's far level as it stands at `matrix`: two upright quads (4 triangles in place of the model's thousands), one
/// turned to face the road point `road` (the driver is on the road), one across it facing back along `forward`, the
/// way the driver comes, so a tree ahead is not seen edge on. Each shows the atlas view that matches its turn.
fn billboard(
    sprite: &mesh::Sprite,
    matrix: &mesh::Mat,
    (road, forward): ([f64; 3], [f64; 3]),
    material: u32,
) -> Vec<Triangle> {
    let norm = |i: usize| matrix[i][..3].iter().map(|v| v * v).sum::<f32>().sqrt();
    let (across, up) = (norm(0).max(norm(2)), norm(1));
    let centre = mesh::point(matrix, sprite.centre);
    let to_road = plan_direction([road[0] - f64::from(centre[0]), road[1] + f64::from(centre[2])]);
    let width = (sprite.uv[2] - sprite.uv[0]).max(f32::EPSILON);
    let quad = |toward: [f32; 2]| {
        let right = [toward[1], -toward[0]];
        // The view of the atlas whose turn is the tree's own turn against the viewer.
        let turn = (toward[1].atan2(toward[0]) - matrix[0][2].atan2(matrix[0][0])) / std::f32::consts::TAU;
        let view = (turn * sprite.views as f32).round().rem_euclid(sprite.views as f32);
        let corner = |sx: f32, sy: f32| {
            let at = [
                centre[0] + right[0] * sx * sprite.half[0] * across,
                centre[1] + sy * sprite.half[1] * up,
                centre[2] + right[1] * sx * sprite.half[0] * across,
            ];
            let uv = [
                sprite.uv[if sx < 0. { 0 } else { 2 }] + (view - (sprite.uv[0] / width).round()) * width,
                sprite.uv[if sy > 0. { 1 } else { 3 }],
            ];
            (frame::world(at), uv)
        };
        let normal = frame::world_dir([toward[0] * 0.5, 0.5, toward[1] * 0.5]);
        let [tl, bl, tr, br] = [corner(-1., 1.), corner(-1., -1.), corner(1., 1.), corner(1., -1.)];
        // Counter-clockwise in DiRT Rally's frame, so `(a, c, b)` for the BTB mesh.
        [[tl, tr, bl], [br, bl, tr]].map(move |t| Triangle {
            pos: t.map(|c| c.0),
            uv: t.map(|c| c.1),
            normal: [normal; 3],
            material: Some(material),
        })
    };
    let back = plan_direction([-forward[0], -forward[1]]);
    // A tree whose road-facing quad already looks along the road needs no second one.
    let along = (to_road[0] * back[0] + to_road[1] * back[1]).abs() > 0.9;
    let mut out = quad(to_road).to_vec();
    if !along {
        out.extend(quad(back));
    }
    out
}

/// The props `objects.ens` places and the trees `trees.bin` does, added to the drawn triangles (the single ones and the
/// far levels) and the instances: drawn only (no collision), the intact `default` models at their first level, within
/// reach of the road for their size.
fn props(
    route: &Route,
    line: &Line,
    control: &Control,
    materials: &mut Vec<stage::Material>,
    drawn: &mut Vec<Triangle>,
) -> Result<stage::Instanced, String> {
    let mut placer = Placer {
        route,
        line,
        library: texture::Library::day(route),
        sources: HashMap::new(),
        index: HashMap::new(),
        groups: BTreeMap::new(),
        lod_in: BTreeMap::new(),
        placed: 0,
        unresolved: BTreeMap::new(),
    };
    let has_trees = route.dir.join("trees.bin").is_file();
    if route.dir.join("objects.ens").is_file() {
        control.say("reading objects.ens");
        let layout = objects::read(route)?;
        for (n, instance) in layout.instances.iter().enumerate() {
            if n % 1024 == 0 {
                control.check()?;
            }
            for (file, node) in &layout.entities[instance.entity].parts {
                // The physical trees and bushes stand where `trees.bin` draws the same ones (11252 of 11252 on Greece
                // route_0 within 1.5 m, 14163 of 14166 on Finland's): drawn once.
                if file == "trees.pssg" && has_trees {
                    continue;
                }
                placer.place(file, node, &instance.matrix, materials, drawn)?;
            }
        }
        control.say(&format!(
            "{} props placed, {} instances without a model",
            layout.instances.len(),
            layout.skipped.values().sum::<usize>()
        ));
    }
    if has_trees {
        control.say("reading trees.bin");
        let placed = trees::read(route)?;
        for (n, item) in placed.items.iter().enumerate() {
            if n % 1024 == 0 {
                control.check()?;
            }
            let key = format!("{} Root", placed.types[item.kind].name);
            placer.place("trees.pssg", &key, &item.matrix, materials, drawn)?;
        }
        control.say(&format!("{} trees placed", placed.items.len()));
    }
    if !placer.unresolved.is_empty() {
        control.say(&format!(
            "{} models not found: {:?}",
            placer.unresolved.values().sum::<usize>(),
            placer.unresolved
        ));
    }
    Ok(placer.finish(materials, drawn))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_billboard_stands_upright_across_the_line_to_the_road() {
        let sprite = mesh::Sprite {
            material: "lod".into(),
            centre: [0., 10., 0.],
            half: [4., 10.],
            uv: [0.25, 0.01, 0.375, 1.],
            views: 8,
        };
        let identity = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];
        // The road lies along the stage's +x from a tree at the origin: DiRT Rally's +x.
        let quad = billboard(&sprite, &identity, ([30., 0., 0.], [1., 0., 0.]), 7);
        assert_eq!(quad.len(), 2);
        for t in &quad {
            assert_eq!(t.material, Some(7));
            for (p, uv) in t.pos.iter().zip(t.uv) {
                // In the plane across the road (x = 0), 8 m wide, from the ground to 20 m, in the atlas's first view (the tree faces the road).
                assert!(p[0].abs() < 1e-9 && (p[1].abs() - 4.).abs() < 1e-9 && (0. ..=20.).contains(&p[2]));
                assert!((0. ..=0.125).contains(&uv[0]) && (0.01..=1.).contains(&uv[1]), "{uv:?}");
            }
        }
        // Along a road that bends away, a second quad faces back along it.
        assert_eq!(billboard(&sprite, &identity, ([30., 0., 0.], [0., 1., 0.]), 7).len(), 4);
        let ys: Vec<f64> = quad.iter().flat_map(|t| t.pos.map(|p| p[1].abs())).collect();
        assert!(ys.iter().all(|y| (y - 4.).abs() < 1e-9));
    }

    #[test]
    fn a_card_opens_from_its_centre_towards_the_viewer() {
        let identity = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];
        let m = mesh::Mesh {
            positions: vec![[0., 5., 0.]; 4],
            uv: vec![[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
            uv2: Some(vec![[-1., 1.], [1., 1.], [1., -1.], [-1., -1.]]),
            triangles: vec![[0, 1, 2], [2, 3, 0]],
            ..mesh::Mesh::default()
        };
        // Facing DiRT Rally's +x: the card spans its z (the stage's y), 2 m wide and 2 m high around (0, 5, 0).
        let tris = cards(&m, &identity, [1., 0.], 3);
        assert_eq!(tris.len(), 2);
        for p in tris.iter().flat_map(|t| t.pos) {
            assert!(p[0].abs() < 1e-9 && (p[1].abs() - 1.).abs() < 1e-9 && (p[2] - 5.).abs() == 1.);
        }
        // The BTB mesh winds clockwise to its front: the first triangle's normal points away from a viewer at +x.
        let [a, b, c] = tris[0].pos;
        let n = [(b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1])];
        assert!(n[0] < 0., "{n:?}");
    }

    #[test]
    fn a_placed_model_is_the_baked_triangles() {
        let m = mesh::Mesh {
            positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 2., 0.5], [0.3, 1., -1.]],
            normals: vec![[0., 1., 0.], [0.6, 0.8, 0.], [0., 0.6, 0.8], [0., 0., 1.]],
            uv: vec![[0., 0.], [1., 0.], [0., 1.], [1., 1.]],
            triangles: vec![[0, 1, 2], [1, 3, 2]],
            ..mesh::Mesh::default()
        };
        // A turn about y with a scale and a shift, and the same with x mirrored.
        let (c, s) = (0.8f32.cos(), 0.8f32.sin());
        for flip in [1., -1.] {
            let matrix = [
                [c * 1.3 * flip, 0., -s * 1.3 * flip, 0.],
                [0., 1.3, 0., 0.],
                [s * 1.3, 0., c * 1.3, 0.],
                [412.5, 33.25, -871.75, 1.],
            ];
            let baked = bake(std::slice::from_ref(&m), &matrix, [1., 0.], |_| 5);
            let local = model(&m, flip < 0.);
            let placed = xform(&matrix);
            assert_eq!((baked.len(), local.len()), (2, 2));
            for (b, l) in baked.iter().zip(&local) {
                for k in 0..3 {
                    // The model's world axes are the mesh's `(x, height, y)`; the placement works on those.
                    let at = rbr_btb::scene::place_point(
                        &placed,
                        [l.pos[k][0] as f32, l.pos[k][2] as f32, l.pos[k][1] as f32],
                    );
                    let world = [f64::from(at[0]), f64::from(at[2]), f64::from(at[1])];
                    assert!((0..3).all(|i| (world[i] - b.pos[k][i]).abs() < 1e-3), "{world:?} {:?}", b.pos[k]);
                    let n = rbr_btb::scene::place_normal(&placed, [l.normal[k][0], l.normal[k][2], l.normal[k][1]]);
                    assert!((0..3).all(|i| (n[[0, 2, 1][i]] - b.normal[k][i]).abs() < 1e-3));
                    assert_eq!(l.uv[k], b.uv[k]);
                }
            }
        }
    }

    #[test]
    fn cards_in_a_model_are_crossed_pairs_seen_from_both_sides() {
        let m = mesh::Mesh {
            positions: vec![[0., 5., 0.]; 4],
            uv: vec![[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
            uv2: Some(vec![[-1., 1.], [1., 1.], [1., -1.], [-1., -1.]]),
            triangles: vec![[0, 1, 2], [2, 3, 0]],
            ..mesh::Mesh::default()
        };
        assert_eq!(model(&m, false).len(), 8);
        let planes: Vec<usize> =
            model(&m, false).iter().map(|t| usize::from(t.pos.iter().any(|p| p[0].abs() > 0.5))).collect();
        assert_eq!(planes.iter().sum::<usize>(), 4);
    }

    #[test]
    fn the_sample_routes_draw_within_the_budget() {
        let Some(game) = std::env::var_os("RBR_DIRT_RALLY") else { return };
        let install = Install::at(std::path::Path::new(&game)).unwrap();
        let out = std::env::temp_dir().join("rbr-dirt-rally-budget-test");
        let mut options = Options::new(Control::new(|_: &str| {}, Default::default()));
        (options.out, options.replace) = (Some(out.clone()), true);
        let report = import(&install, "finland/ouninpohja_rally_01/route_0", &options).unwrap();
        let _ = std::fs::remove_dir_all(out);
        assert!(report.drawn_triangles <= DRAWN_BUDGET, "{} written triangles", report.drawn_triangles);
    }
}
