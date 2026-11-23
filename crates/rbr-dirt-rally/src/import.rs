//! The interface a launcher or the command line calls: convert one route of the install into a BTB stage. The course
//! (`route`), the collision (`collision`, `surface`) and the drawn world are read, put in the stage's world (`frame`)
//! and written by `stage`. Nothing here prints, reads the environment or exits the process.
use crate::install::{Install, Route};
use crate::{collision, frame, material, mesh, objects, pssg::Pssg, route, stage, surface, texture, trees};
use rbr_ac_tracks::btb::{
    control::Control,
    stage::CELL_LIMIT,
    geometry::{Grid, Triangle},
    road::Line,
};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

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
    if !route.is_rally() {
        return Err(format!("{id}: only rally stages convert (not rallycross, Pikes Peak or the frontend)"));
    }
    let control = &options.control;
    control.say(&format!("reading {}", route.id()));
    let course = route::course(&route)?;
    let (line, start, finish, splits) = centre_line(&route, &course)?;
    control.say(&format!("course {:.0} m on a {:.0} m road line", course.length(), line.length()));
    control.check()?;
    let faces = collision::read(&route)?;
    let (collision, surface_counts) = collision_triangles(&faces, &line, start, finish)?;
    control.say(&format!("{} collision triangles", collision.len()));
    drop(faces);
    control.check()?;
    let (mut materials, mut drawn) = terrain(&route, control)?;
    control.say(&format!("{} drawn terrain triangles in {} materials", drawn.len(), materials.len()));
    let terrain_triangles = drawn.len();
    props(&route, &line, control, &mut materials, &mut drawn)?;
    control.say(&format!("{} drawn prop and tree triangles", drawn.len() - terrain_triangles));
    let input = stage::Input {
        source: route.dir.clone(),
        route: route.id(),
        title: route::title(&route),
        country: route::country(&route),
        length: course.length(),
        line,
        start,
        finish,
        splits,
        materials,
        drawn,
        collision,
        collision_source: "track.jpk",
        surface_counts,
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

/// The collision in the stage's world with RBR ids, and the count of each code's triangles. `DEFA` takes the road
/// surface of the stage's class, judged from the codes under the course; an unknown code is reported.
fn collision_triangles(
    faces: &[collision::Face],
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
    let default = u32::from(surface::default_for(physics));
    for t in &mut triangles {
        t.material.get_or_insert(default);
    }
    Ok((triangles, counts))
}

/// The drawn terrain in the stage's world: the route's `routesplit.pssg` where the area has one (Greece), and the area's
/// `tracksplit.pssg` (its vista there, all of it elsewhere), each material drawn with its first layer's texture.
fn terrain(route: &Route, control: &Control) -> Result<(Vec<stage::Material>, Vec<Triangle>), String> {
    let mut library = texture::Library::day(route);
    let files: Vec<_> = [route.dir.join("routesplit.pssg"), route.area_dir().join("tracksplit.pssg")]
        .into_iter()
        .filter(|p| p.is_file())
        .collect();
    let (mut materials, mut triangles) = (Vec::new(), Vec::new());
    for (file, path) in files.iter().enumerate() {
        control.check()?;
        control.say(&format!("reading {}", path.file_name().unwrap_or_default().to_string_lossy()));
        let pssg = Pssg::read(path)?;
        let sources = material::read(&pssg);
        let mut index = HashMap::<String, u32>::new();
        for m in mesh::terrain(&pssg)? {
            let at = *index.entry(m.material.clone()).or_insert_with(|| {
                let dds =
                    sources.get(&m.material).and_then(material::diffuse).and_then(|r| library.get(r)).map(|t| t.dds());
                // Decals lie on the ground they mark: drawn with their alpha.
                let transparent = m.node.starts_with("DECAL");
                materials.push(stage::Material {
                    name: format!("{file}:{}", m.material),
                    dds,
                    transparent,
                    range: None,
                });
                materials.len() as u32 - 1
            });
            triangles.extend(m.triangles.iter().map(|&[a, b, c]| {
                // Counter-clockwise in DiRT Rally's right-handed frame; the BTB mesh's winding the other way.
                let corners = [a, c, b].map(|i| i as usize);
                Triangle {
                    pos: corners.map(|i| frame::world(m.positions[i])),
                    uv: corners.map(|i| m.uv.get(i).copied().unwrap_or_default()),
                    normal: corners.map(|i| m.normals.get(i).map_or([0., 0., 1.], |&n| frame::world_dir(n))),
                    material: Some(at),
                }
            }));
        }
    }
    Ok((materials, triangles))
}

/// A prop is drawn as far as this many times its size: a metre-wide rock to 100 m, a tree of ten metres to a kilometre.
const PROP_REACH: f32 = 100.;
/// A model of `t` triangles is drawn no farther than this over the square root of `t`: dense trees stand out to a few
/// hundred metres, a rock to a kilometre. Without it Finland's forest is 48 M triangles.
const PROP_COST: f64 = 20_000.;
/// The draw ranges a prop takes, metres, the least that holds its size ([`PROP_REACH`]) and its cost
/// ([`PROP_COST`]); materials are split by them.
const PROP_RANGES: [f64; 8] = [100., 150., 225., 340., 500., 750., 1000., 3000.];
/// The distant forest: `trees.bin`'s `*_dist_*` types are tiles of 500 m of far trees (about a hundred triangles each),
/// drawn far past where the single trees are.
const DIST_REACH: f64 = 3000.;

/// Whether a shader draws its diffuse texture's alpha (cut-out leaves and blended signs; the other props' alpha is a
/// gloss or emission mask: `Object_Emis`, `Object_Norm` and `Object_Stones` carry DXT5 with varying alpha too). Trunks
/// and `foliage_environment` are opaque.
fn blended(shader: &str) -> bool {
    let s = shader.to_lowercase();
    s.contains("blend")
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

/// Props and trees gathered into the drawn triangles.
struct Placer<'a> {
    route: &'a Route,
    line: &'a Line,
    library: texture::Library,
    sources: HashMap<String, Option<Source>>,
    /// `(file, material id, range)` -> index in the stage's materials.
    index: HashMap<(String, String, u64), u32>,
    placed: usize,
    unresolved: BTreeMap<String, usize>,
}

impl Placer<'_> {
    /// Draws the type `key` of `file` at `matrix`; one far from the road for its size is left out.
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
            let source = if path.is_file() { Some(Source::read(&path)?) } else { None };
            self.sources.insert(file.to_owned(), source);
        }
        let Some(source) = self.sources.get(file).and_then(Option::as_ref) else {
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
        let reach = if key.contains("_dist_") {
            DIST_REACH
        } else {
            f64::from(size * PROP_REACH).min(PROP_COST / (triangles.max(1) as f64).sqrt())
        };
        let range = PROP_RANGES.iter().copied().find(|&r| r >= reach).unwrap_or(DIST_REACH);
        let here = frame::world([matrix[3][0], matrix[3][1], matrix[3][2]]);
        if meshes.is_empty() || rbr_ac_tracks::assetto::ai::project(self.line, here).1 > range + f64::from(size) {
            return Ok(());
        }
        let mirrored = mesh::determinant(matrix) < 0.;
        for m in meshes {
            let slot = match self.index.get(&(file.to_owned(), m.material.clone(), range as u64)) {
                Some(&slot) => slot,
                None => {
                    let found = source.materials.get(&m.material);
                    let texture = found.and_then(material::diffuse).and_then(|r| self.library.get(r));
                    let transparent = found.is_some_and(|f| blended(&f.shader))
                        && texture.as_ref().is_some_and(|t| t.format == texture::Format::Dxt5);
                    let name = format!("{file}:{}@{range}", m.material);
                    materials.push(stage::Material {
                        name,
                        dds: texture.map(|t| t.dds()),
                        transparent,
                        range: Some(range),
                    });
                    self.index.insert((file.to_owned(), m.material.clone(), range as u64), materials.len() as u32 - 1);
                    materials.len() as u32 - 1
                }
            };
            out.extend(m.triangles.iter().map(|&[a, b, c]| {
                // Counter-clockwise in DiRT Rally's frame, the BTB mesh's winding the other way; a mirrored
                // placement turned it already.
                let corners = if mirrored { [a, b, c] } else { [a, c, b] }.map(|i| i as usize);
                Triangle {
                    pos: corners.map(|i| frame::world(mesh::point(matrix, m.positions[i]))),
                    uv: corners.map(|i| m.uv.get(i).copied().unwrap_or_default()),
                    normal: corners.map(|i| {
                        m.normals.get(i).map_or([0., 0., 1.], |&n| frame::world_dir(mesh::direction(matrix, n)))
                    }),
                    material: Some(slot),
                }
            }));
            self.placed += 1;
        }
        Ok(())
    }
}

/// The props `objects.ens` places and the trees `trees.bin` does, added to the drawn triangles: drawn only (no
/// collision), the intact `default` models at their first level, within reach of the road for their size.
fn props(
    route: &Route,
    line: &Line,
    control: &Control,
    materials: &mut Vec<stage::Material>,
    drawn: &mut Vec<Triangle>,
) -> Result<(), String> {
    let mut placer = Placer {
        route,
        line,
        library: texture::Library::day(route),
        sources: HashMap::new(),
        index: HashMap::new(),
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
    Ok(())
}
