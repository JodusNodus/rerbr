//! The interface a launcher or the command line calls: convert one route of the install into a BTB stage. The course
//! (`route`), the collision (`collision`, `surface`) and the drawn world are read, put in the stage's world (`frame`)
//! and written by `stage`. Nothing here prints, reads the environment or exits the process.
use crate::install::{Install, Route};
use crate::{collision, frame, material, mesh, pssg::Pssg, route, shade, stage, surface, texture};
use rbr_ac_tracks::btb::{
    control::Control,
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
    let (materials, drawn) = terrain(&route, control)?;
    control.say(&format!("{} drawn terrain triangles in {} materials", drawn.len(), materials.len()));
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
    let stage_options = stage::Options {
        out,
        name: options.name.clone().unwrap_or_else(|| default_name(&route)),
        draw_corridor: options.draw_corridor,
        collision_corridor: options.collision_corridor,
        lod: options.lod,
        replace: options.replace,
        control: options.control.clone(),
    };
    stage::convert(&input, &stage_options)
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
/// `tracksplit.pssg` (its vista there, all of it elsewhere); the ground is shaded by `shade` (layers, macro colour and lightmap per pixel), the rest drawn with its first layer's texture.
fn terrain(route: &Route, control: &Control) -> Result<(Vec<stage::Material>, Vec<Triangle>), String> {
    let mut library = texture::Library::day(route);
    let files: Vec<_> = [route.dir.join("routesplit.pssg"), route.area_dir().join("tracksplit.pssg")]
        .into_iter()
        .filter(|p| p.is_file())
        .collect();
    let (mut materials, mut triangles) = (Vec::new(), Vec::new());
    let mut shader = shade::Shader::default();
    for (file, path) in files.iter().enumerate() {
        control.check()?;
        control.say(&format!("reading {}", path.file_name().unwrap_or_default().to_string_lossy()));
        let pssg = Pssg::read(path)?;
        let sources = material::read(&pssg);
        let mut index = HashMap::<String, u32>::new();
        let shaded = shader.shade(mesh::terrain(&pssg)?, &sources, &mut library);
        for m in shaded.meshes {
            let at = *index.entry(m.material.clone()).or_insert_with(|| {
                let layers = shaded.layers.get(&m.material).cloned();
                // A shaded material's fallback is its first layer, which `stage` takes from the layers.
                let dds = if layers.is_some() {
                    None
                } else {
                    sources.get(&m.material).and_then(material::diffuse).and_then(|r| library.get(r)).map(|t| t.dds())
                };
                // Decals lie on the ground they mark: drawn with their alpha.
                let transparent = m.node.starts_with("DECAL");
                materials.push(stage::Material { name: format!("{file}:{}", m.material), dds, transparent, layers });
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
