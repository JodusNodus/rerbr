//! One route to a BTB stage folder: drawn tiles, collision, driveline, timing, calls, spawn, `track.ini`. The input is
//! already in the stage's world (z up, `frame::world`) and in the BTB mesh's winding; the files are those
//! `rbr_ac_tracks`' AC conversion writes, through its shared mesh, texture and pacenote writers.
use crate::surface;
use rbr_ac_tracks::assetto::material::{self, TextureBank};
use rbr_ac_tracks::btb::{
    control::Control,
    geometry::{Grid, Triangle},
    mesh::{self as xwrite, XMesh},
    pacenotes,
    road::Line,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// The file a conversion leaves in its stage's folder: what it was made from. It marks the stage as one of ours.
pub const CONVERSION_FILE: &str = "dr-conversion.json";

/// A drawn material: its `materials.ini` section and the DDS it draws.
#[derive(Default)]
pub struct Material {
    pub name: String,
    /// The diffuse texture as a DDS file; `None` draws a flat grey.
    pub dds: Option<Vec<u8>>,
    /// Drawn with alpha (foliage, decals).
    pub transparent: bool,
    /// How far it is drawn, metres; `None` for the stage's default (`lod` for alpha, ReRBR's far plane otherwise).
    /// Props are small: they are drawn as far as their size holds.
    pub range: Option<f64>,
    /// The ground the renderer sums per pixel (`ReRbrLayer*`): `dds` stays the fallback of a reader without them.
    pub layers: Option<crate::shade::Layers>,
    /// The normal map (`TNormalMap`) and the specular map (`TSpecularMap`) of a prop, as DDS files.
    pub normal: Option<Vec<u8>>,
    pub specular: Option<Vec<u8>>,
}

impl Material {
    /// The section's bump and highlight: the maps go into the bank, the highlight is ReRBR's own constant.
    fn extras(&self, bank: &mut TextureBank) -> Result<material::Extras, String> {
        let mut put = |dds: &Option<Vec<u8>>| dds.as_ref().map(|d| bank.put(d.clone(), "dds")).transpose();
        let (normal, specular) = (put(&self.normal)?, put(&self.specular)?);
        let highlight = specular.is_some().then_some(HIGHLIGHT);
        Ok(material::Extras { normal, specular, highlight, ..material::Extras::default() })
    }
}

/// A shaded terrain triangle's corners: the coordinates its layers tile on and its vertex weights (R, G, B of the second
/// to fourth layers).
#[derive(Clone, Copy, Default)]
pub struct Corners {
    pub uv: [[f32; 2]; 3],
    pub weights: [[u8; 3]; 3],
}

/// `(SpecularColor, SpecularPower)` of a prop with a specular map: the maps hold the intensity, the shader's own
/// constants are not on disk.
const HIGHLIGHT: (f32, f32) = (0.5, 40.);

/// A prop or tree's mesh in its own space, written once as a model (`Objects/dr_<n>.x`) and drawn by its placements. The
/// triangles are in the stage's world axes (as [`Input::drawn`]) about the model's origin; `material` is the index
/// into `materials`.
pub struct Model {
    pub triangles: Vec<Triangle>,
    pub material: u32,
}

/// The placements of models that share them: one `.xform` of row-major Direct3D matrices (`rbr_btb::scene::place_point`),
/// in the BTB mesh's axes about the stage's world origin (the stage subtracts its own origin), one object per model.
pub struct Placed {
    pub models: Vec<usize>,
    pub matrices: Vec<[f32; 16]>,
}

/// Props and trees drawn as instances (`ReRbrInstanced=1`).
#[derive(Default)]
pub struct Instanced {
    pub models: Vec<Model>,
    pub placed: Vec<Placed>,
    /// Materials drawn only from this far, metres (a far level over the model's range); the others from 0.
    pub lod_in: BTreeMap<u32, f64>,
}

/// A route ready to write, in the stage's world.
pub struct Input {
    /// The route folder it comes from, and its id (`greece/ghymno_rally_01/route_0`).
    pub source: PathBuf,
    pub route: String,
    pub title: String,
    pub country: Option<&'static str>,
    /// Latitude and longitude of the venue, `track.ini`'s own keys, where the country's default is far off.
    pub place: Option<[f64; 2]>,
    /// The road's centre line with its width, covering the start and finish with their margins.
    pub line: Line,
    /// The timed length the game gives it, metres.
    pub length: f64,
    /// Arc lengths on `line`.
    pub start: f64,
    pub finish: f64,
    pub splits: Vec<f64>,
    /// A rallycross lap: `line` is a closed ring that starts at the lap line (the spawn), `start..finish` is
    /// one lap of it and `splits` are arc lengths from its start.
    pub circuit: bool,
    pub materials: Vec<Material>,
    /// Drawn triangles, `material` the index into `materials`.
    pub drawn: Vec<Triangle>,
    /// The layers' own coordinates and weights of the first drawn triangles (the terrain's), for the materials with
    /// [`Material::layers`]: their `Triangle::uv` is the atlas.
    pub layer: Vec<Corners>,
    /// The props and trees drawn as instances.
    pub instanced: Instanced,
    /// Collision triangles, `material` the RBR surface id ([`surface::Surface::Default`] already resolved).
    pub collision: Vec<Triangle>,
    /// Where the surfaces came from, for the conversion file (`"track.jpk"`, or the drawn terrain's fallback).
    pub collision_source: &'static str,
    /// Surface counts by source code, for the conversion file.
    pub surface_counts: BTreeMap<String, usize>,
}

pub struct Options {
    pub out: PathBuf,
    pub name: String,
    pub draw_corridor: f64,
    pub collision_corridor: f64,
    pub lod: f64,
    pub replace: bool,
    pub control: Control,
}

#[derive(Debug)]
pub struct Report {
    pub folder: PathBuf,
    pub length: f64,
    pub physics: &'static str,
    /// Triangles written (baked tiles, and each instanced model once).
    pub drawn_triangles: usize,
    /// Triangles the instances draw when all are in view.
    pub instanced_triangles: usize,
    pub collision_triangles: usize,
    pub textures: usize,
    pub max_cell_vertices: usize,
}

/// Writes `input` to `options.out/options.name`, through a staging folder: a failed or cancelled conversion leaves
/// nothing, a replaced one is swapped in whole.
pub fn convert(input: &Input, options: &Options) -> Result<Report, String> {
    let name = Path::new(&options.name);
    if options.name.is_empty() || name.components().count() != 1 || name.file_name().is_none() {
        return Err("--name must be one folder name".into());
    }
    for (key, value) in [
        ("draw corridor", options.draw_corridor),
        ("collision corridor", options.collision_corridor),
        ("LOD", options.lod),
    ] {
        if !value.is_finite() || !(1. ..=5000.).contains(&value) {
            return Err(format!("{key} must be between 1 and 5000 m"));
        }
    }
    let parent = &options.out;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let destination = parent.join(&options.name);
    if destination.exists() && (!options.replace || !destination.join(CONVERSION_FILE).is_file()) {
        return Err(format!("{} exists; --replace only replaces stages this converter made", destination.display()));
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let staging = parent.join(format!(".dr-stage-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let mut report = match build(input, options, &staging) {
        Ok(report) => report,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    if destination.exists() {
        let backup = parent.join(format!(".dr-backup-{}-{stamp}", std::process::id()));
        std::fs::rename(&destination, &backup).map_err(|e| e.to_string())?;
        if let Err(e) = std::fs::rename(&staging, &destination) {
            let _ = std::fs::rename(&backup, &destination);
            return Err(e.to_string());
        }
        std::fs::remove_dir_all(backup).map_err(|e| e.to_string())?;
    } else {
        std::fs::rename(&staging, &destination).map_err(|e| e.to_string())?;
    }
    report.folder = destination;
    Ok(report)
}

/// Plan cells within `radius` of the line, `cell` metres square; a triangle is in when its plan box touches one.
pub struct Corridor {
    cells: HashSet<(i64, i64)>,
    cell: f64,
}

impl Corridor {
    pub fn new(line: &Line, radius: f64, cell: f64) -> Corridor {
        let r = (radius / cell).ceil() as i64 + 1;
        let mut cells = HashSet::new();
        for i in 0..=(line.length() / (cell * 0.5)).ceil() as usize {
            let p = line.sample((i as f64 * cell * 0.5).min(line.length())).0;
            let centre = ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64);
            for x in -r..=r {
                for y in -r..=r {
                    if x * x + y * y <= r * r {
                        cells.insert((centre.0 + x, centre.1 + y));
                    }
                }
            }
        }
        Corridor { cells, cell }
    }

    pub fn contains(&self, pos: &[[f64; 3]; 3]) -> bool {
        let c = |v: f64| (v / self.cell).floor() as i64;
        let range = |k: usize| {
            let (a, b) = pos.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[k]), b.max(p[k])));
            c(a)..=c(b)
        };
        range(0).any(|x| range(1).any(|y| self.cells.contains(&(x, y))))
    }
}

/// ReRBR's far plane, metres (`rbr_btb::circuit::FAR`'s reach for drawn scenery).
const FAR: f64 = 10_000.;
/// Side of the cells ReRBR merges drawn meshes into (its BTB stage's `CELL`), metres.
const SCENERY_CELL: f64 = 128.;
/// Side of the collision tiles, metres (as the AC converter's).
const COLLISION_TILE: f64 = 300.;

/// One mesh being gathered: deduplicated vertices in the BTB mesh's axes (x, height, y) about the origin.
#[derive(Default)]
struct Builder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    /// A shaded vertex's layer coordinates and weights ([`Corners`]), empty for the others.
    layer_uvs: Vec<[f32; 2]>,
    weights: Vec<[u8; 3]>,
    /// The whole layer units the layer coordinates may be shifted by ([`shade::Layers::period`]).
    period: f32,
    triangles: Vec<[u32; 3]>,
    vertices: HashMap<[u32; 11], u32>,
}

impl Builder {
    fn add(&mut self, t: &Triangle, layer: Option<&Corners>, origin: [f64; 2], visible: bool) {
        let index = std::array::from_fn(|k| {
            let p = t.pos[k];
            let position = [(p[0] - origin[0]) as f32, p[2] as f32, (p[1] - origin[1]) as f32];
            let (normal, uv) = if visible {
                ([t.normal[k][0], t.normal[k][2], t.normal[k][1]], t.uv[k])
            } else {
                ([0., 1., 0.], [0.; 2])
            };
            let (layer_uv, weights) = layer.map_or(([0.; 2], [0; 3]), |l| (l.uv[k], l.weights[k]));
            let key = [
                position[0].to_bits(),
                position[1].to_bits(),
                position[2].to_bits(),
                normal[0].to_bits(),
                normal[1].to_bits(),
                normal[2].to_bits(),
                uv[0].to_bits(),
                uv[1].to_bits(),
                layer_uv[0].to_bits(),
                layer_uv[1].to_bits(),
                u32::from_le_bytes([weights[0], weights[1], weights[2], 0]),
            ];
            *self.vertices.entry(key).or_insert_with(|| {
                self.positions.push(position);
                self.normals.push(normal);
                self.uvs.push(uv);
                if layer.is_some() {
                    self.layer_uvs.push(layer_uv);
                    self.weights.push(weights);
                }
                self.positions.len() as u32 - 1
            })
        });
        if index[0] != index[1] && index[1] != index[2] && index[0] != index[2] {
            self.triangles.push(index);
        }
    }

    /// The mesh as a `.x`; a shaded one carries the layers' coordinates as UV set 1 and its weights as sets 2 and 3.
    fn bytes(&self, material: &str) -> Vec<u8> {
        let period = self.period;
        // Shifted towards the origin so that `f32` holds the layers' texels in the far stretches of a long stage.
        let low = self.layer_uvs.iter().fold([f32::MAX; 2], |low, uv| [low[0].min(uv[0]), low[1].min(uv[1])]);
        let shift = low.map(|l| if period > 0. { (l / period).floor() * period } else { 0. });
        let layer_uvs: Vec<[f32; 2]> = self.layer_uvs.iter().map(|uv| [uv[0] - shift[0], uv[1] - shift[1]]).collect();
        let weight = |w: u8| f32::from(w) / 255.;
        let rg: Vec<[f32; 2]> = self.weights.iter().map(|w| [weight(w[0]), weight(w[1])]).collect();
        let b: Vec<[f32; 2]> = self.weights.iter().map(|w| [weight(w[2]), 0.]).collect();
        let shaded = !layer_uvs.is_empty();
        let more: &[&[[f32; 2]]] = if shaded { &[&rg, &b] } else { &[] };
        xwrite::write(&XMesh {
            positions: &self.positions,
            normals: &self.normals,
            uvs: &self.uvs,
            uvs2: shaded.then_some(&layer_uvs),
            more,
            triangles: &self.triangles,
            material,
        })
    }
}

/// `Objects.zip` and its `objectlist.ini`.
struct Objects {
    zip: zip::ZipWriter<std::fs::File>,
    ini: String,
    count: usize,
}

const IDENTITY: [f32; 16] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];

impl Objects {
    fn new(path: &Path) -> Result<Objects, String> {
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut objects = Objects { zip: zip::ZipWriter::new(file), ini: String::new(), count: 0 };
        let identity: Vec<u8> = IDENTITY.iter().flat_map(|v| v.to_le_bytes()).collect();
        objects.put("Objects/identity.xform", &identity)?;
        Ok(objects)
    }

    fn put(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        self.zip.start_file(name, options).map_err(|e| e.to_string())?;
        self.zip.write_all(bytes).map_err(|e| e.to_string())
    }

    /// One mesh with its `materials.ini` section and `objectlist.ini` keys.
    fn mesh(&mut self, builder: &Builder, material: &str, entries: &str) -> Result<(), String> {
        self.placed(builder, material, "identity", entries)
    }

    /// One mesh placed by the `.xform` `xform` (already written; `identity` is always there).
    fn placed(&mut self, builder: &Builder, material: &str, xform: &str, entries: &str) -> Result<(), String> {
        if builder.triangles.is_empty() {
            return Ok(());
        }
        let i = self.count;
        self.put(&format!("Objects/dr_{i}.x"), &builder.bytes(material))?;
        let _ = writeln!(
            self.ini,
            "\n[Object_{i}]\nfile=dr_{i}\nxform={xform}\ncloneCount=1\nDrawInstanced=0\nMoveable=0\n{entries}"
        );
        self.count += 1;
        Ok(())
    }

    fn finish(self) -> Result<String, String> {
        self.zip.finish().map_err(|e| e.to_string())?;
        Ok(format!("[OBJECTS]\ncount={}\n{}", self.count, self.ini))
    }
}

/// The ground under `p` within 3 m of its height, or beside it where a small gap lets the point through.
fn ground_near<'a>(ground: &Grid<'a>, p: [f64; 3]) -> Option<(f64, &'a Triangle)> {
    let near = |x: f64, y: f64| ground.ground(x, y, p[2]).filter(|(z, _)| (*z - p[2]).abs() < 3.);
    near(p[0], p[1]).or_else(|| {
        [0.1, 0.25, 0.5].into_iter().find_map(|r| {
            (0..8).find_map(|k| {
                let a = f64::from(k) * std::f64::consts::FRAC_PI_4;
                near(p[0] + r * a.cos(), p[1] + r * a.sin())
            })
        })
    })
}

/// The stage's `settings.ini`: the sun and the renderer's own air, north the stage's +z (as for AC's frame). The
/// bakes carry DiRT Rally's own ambient, so the clear sky's blue is kept only in part (`sky_tint`).
const SETTINGS_INI: &str =
    "[SETTINGS]\nsunDirection=-0.843359,-0.503774,0.186968\n\n[ReRbrAir]\nmist=0\nsky_tint=0.2\nnorth=0, 0, 1\n";

/// Where the car stands: the start grid's first slot, 3.7 m behind the start line on every rally route's grid.
pub(crate) const GRID_BEHIND: f64 = 3.7;
/// A circuit's car stands this far past the lap line (the ring's seam): on the seam itself a first step's progress can
/// come out just behind it, and the next step would count a lap.
pub(crate) const SEAM_BEHIND: f64 = 1.;

fn build(input: &Input, options: &Options, folder: &Path) -> Result<Report, String> {
    let control = &options.control;
    // The stage keeps 50 m before the start and 40 m past the finish, resampled every 2.5 m; a circuit is its lap from
    // the lap line, a closed ring (its last point repeats the first).
    let (from, to) = if input.circuit {
        (input.start, input.finish)
    } else {
        ((input.start - 50.).max(0.), (input.finish + 40.).min(input.line.length()))
    };
    let mut line = input.line.stretch(from, to - from, true, 2.5);
    if input.circuit {
        let (first, width) = (line.points[0], line.width[0]);
        let last = line.points.len() - 1;
        (line.points[last], line.width[last]) = (first, width);
        line = Line::new(std::mem::take(&mut line.points), std::mem::take(&mut line.width));
    }
    let (start, finish) = if input.circuit { (0., line.length()) } else { (input.start - from, input.finish - from) };
    let origin: [f64; 2] = std::array::from_fn(|k| {
        let (lo, hi) = line.points.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[k]), b.max(p[k])));
        ((lo + hi) * 0.5).round()
    });
    let draw = Corridor::new(&line, options.draw_corridor, 100.);
    let collide = Corridor::new(&line, options.collision_corridor, 25.);

    // Collision first: the driveline's heights, the stage's physics and the spawn come from it.
    let collision: Vec<&Triangle> = input.collision.iter().filter(|t| collide.contains(&t.pos)).collect();
    if collision.is_empty() {
        return Err("no collision near the course".into());
    }
    let ground = Grid::new(collision.clone(), 4.);
    let (mut missing, mut along) = (0, Vec::new());
    for i in 0..line.points.len() {
        match ground_near(&ground, line.points[i]) {
            Some((z, t)) => {
                line.points[i][2] = z;
                if let (Some(id), Some(next)) = (t.material, line.at.get(i + 1)) {
                    along.push((id as u8, next - line.at[i]));
                }
            }
            None => missing += 1,
        }
    }
    if missing > line.points.len() / 100 {
        return Err(format!("{missing}/{} driveline points have no collision within 3 m", line.points.len()));
    }
    let physics = surface::physics(along.iter().copied());
    control.say(&format!("physics={physics}; {missing}/{} driveline points without ground", line.points.len()));
    let (spawn, heading) = {
        let (p, tangent) = line.sample(if input.circuit { SEAM_BEHIND } else { (start - GRID_BEHIND).max(0.) });
        let (z, _) = ground_near(&ground, p).ok_or("no ground under the start position")?;
        ([p[0], p[1], z], tangent)
    };
    drop(ground);

    let mut objects = Objects::new(&folder.join("Objects.zip"))?;
    let mut bank = TextureBank::new(&folder.join("Textures.zip"))?;
    let (mut sections, mut names) = (String::new(), Vec::new());

    // Drawn: each material's triangles in the draw corridor, by ReRBR's scenery cell.
    let mut tiles = BTreeMap::<(usize, (i64, i64)), Builder>::new();
    for (i, t) in input.drawn.iter().enumerate().filter(|(_, t)| draw.contains(&t.pos)) {
        let cell =
            |k: usize| (((t.pos[0][k] + t.pos[1][k] + t.pos[2][k]) / 3. - origin[k]) / SCENERY_CELL).floor() as i64;
        let material = t.material.unwrap_or(0) as usize;
        let layers = input.materials[material].layers.as_ref();
        let tile = tiles
            .entry((material, (cell(0), cell(1))))
            .or_insert_with(|| Builder { period: layers.map_or(0., |l| l.period), ..Builder::default() });
        tile.add(t, layers.and(input.layer.get(i)), origin, true);
    }
    let mut section_of = BTreeMap::new();
    let mut drawn_triangles = 0;
    let (mut instanced_triangles, mut model_triangles) = (0, 0);
    for (i, placed) in input.instanced.placed.iter().enumerate() {
        control.check()?;
        let xform = format!("dr_x{i}");
        let bytes: Vec<u8> = placed
            .matrices
            .iter()
            .flat_map(|m| {
                let mut m = *m;
                (m[12], m[14]) = (m[12] - origin[0] as f32, m[14] - origin[1] as f32);
                m.map(f32::to_le_bytes)
            })
            .flatten()
            .collect();
        objects.put(&format!("Objects/{xform}.xform"), &bytes)?;
        for &at in &placed.models {
            let model = &input.instanced.models[at];
            let m = &input.materials[model.material as usize];
            let section = match section_of.get(&(model.material as usize)) {
                Some(section) => String::clone(section),
                None => {
                    let texture = match &m.dds {
                        Some(dds) => bank.put(dds.clone(), "dds")?,
                        None => bank.put(rbr_ac_tracks::btb::textures::flat([0.5, 0.5, 0.5, 1.]), "dds")?,
                    };
                    let name = format!("dr\\{}", model.material);
                    material::section_with(&mut sections, &name, &texture, m.transparent, &m.extras(&mut bank)?);
                    names.push(name.clone());
                    section_of.insert(model.material as usize, name.clone());
                    name
                }
            };
            let mut builder = Builder::default();
            for t in &model.triangles {
                builder.add(t, None, [0., 0.], true);
            }
            let lod = m.range.unwrap_or(options.lod);
            let entries =
                format!("LODIn=0\nLODOut={lod}\nVisible=1\nShadowCaster=1\nCollisionModel=-1\nReRbrInstanced=1\n");
            objects.placed(&builder, &section, &xform, &entries)?;
            model_triangles += builder.triangles.len();
            instanced_triangles += builder.triangles.len() * placed.matrices.len();
        }
    }
    drawn_triangles += model_triangles;
    for ((material, _), builder) in &tiles {
        control.check()?;
        let section = match section_of.get(material) {
            Some(section) => String::clone(section),
            None => {
                let m = &input.materials[*material];
                let texture = match (&m.dds, &m.layers) {
                    (Some(dds), _) => bank.put(dds.clone(), "dds")?,
                    (None, Some(l)) => bank.put(l.details[0].to_vec(), "dds")?,
                    (None, None) => bank.put(rbr_ac_tracks::btb::textures::flat([0.5, 0.5, 0.5, 1.]), "dds")?,
                };
                let name = format!("dr\\{material}");
                let (normal, bump_scale) = match m.layers.as_ref().and_then(|l| l.normal.as_ref()) {
                    Some((dds, repeat)) => (Some(bank.put(dds.to_vec(), "dds")?), Some(*repeat)),
                    None => (None, None),
                };
                let layers = match &m.layers {
                    Some(l) => Some(material::Layers {
                        diffuse: bank.put(l.diffuse.to_vec(), "dds")?,
                        mask: String::new(),
                        details: {
                            let mut details: [String; 4] = Default::default();
                            for (slot, dds) in details.iter_mut().zip(&l.details) {
                                *slot = bank.put(dds.to_vec(), "dds")?;
                            }
                            details
                        },
                        scale: l.scale,
                        magic: l.magic,
                        world: None,
                        uv: true,
                        blend: l.blend,
                    }),
                    None => None,
                };
                material::section_with(
                    &mut sections,
                    &name,
                    &texture,
                    m.transparent,
                    &material::Extras {
                        layers,
                        normal,
                        bump_scale,
                        bump_uv: m.layers.is_some(),
                        ..m.extras(&mut bank)?
                    },
                );
                names.push(name.clone());
                section_of.insert(*material, name.clone());
                name
            }
        };
        // Opaque ground is drawn as far as ReRBR sees, so the valley below and the mountains around stay in view.
        let m = &input.materials[*material];
        let lod = m.range.unwrap_or(if m.transparent { options.lod } else { FAR });
        let lod_in = input.instanced.lod_in.get(&(*material as u32)).copied().unwrap_or(0.);
        let entries = format!("LODIn={lod_in}\nLODOut={lod}\nVisible=1\nShadowCaster=1\nCollisionModel=-1\n");
        objects.mesh(builder, &section, &entries)?;
        drawn_triangles += builder.triangles.len();
    }
    drop(tiles);
    control.say(&format!("{drawn_triangles} drawn triangles in {} materials", section_of.len()));

    // Collision: by tile and surface, invisible, one flat texture per surface for `mat.ini`.
    let mut col = BTreeMap::<((i64, i64), u8), Builder>::new();
    for t in &collision {
        let tile = |k: usize| ((t.pos[0][k] + t.pos[1][k] + t.pos[2][k]) / COLLISION_TILE).floor() as i64;
        col.entry(((tile(0), tile(1)), t.material.unwrap_or(37) as u8)).or_default().add(t, None, origin, false);
    }
    let collision_triangles = col.values().map(|b| b.triangles.len()).sum();
    let ids: BTreeSet<u8> = col.keys().map(|(_, id)| *id).collect();
    let mut maps = String::new();
    for (index, id) in ids.iter().enumerate() {
        let texture = bank.put(rbr_ac_tracks::btb::textures::flat([f32::from(*id) / 255.; 4]), "dds")?;
        let name = format!("collision\\surface_{id}");
        material::section(&mut sections, &name, &texture, false);
        names.push(name);
        let _ = writeln!(maps, "[MAP{index}]\ntexture={texture}");
        for (key, surface) in ["data", "damp", "wet"].into_iter().zip(surface::weathers(*id)) {
            for row in 0..16 {
                let _ = writeln!(maps, "{key}{row}={}", vec![surface.to_string(); 16].join(" "));
            }
        }
    }
    for ((_, id), builder) in &col {
        let entries = "LODIn=0\nVisible=0\nShadowCaster=0\nCollisionModel=0\nCollisionOnly=1\n";
        objects.mesh(builder, &format!("collision\\surface_{id}"), entries)?;
    }
    drop(col);
    let textures = bank.count;
    bank.finish()?;

    let write = |file: &str, text: String| std::fs::write(folder.join(file), text).map_err(|e| format!("{file}: {e}"));
    write("objectlist.ini", objects.finish()?)?;
    let mut list = format!("[MATERIALS]\ncount={}\n", names.len());
    for (i, n) in names.iter().enumerate() {
        let _ = writeln!(list, "MAT_{i}={n}");
    }
    write("materials.ini", format!("{list}\n{sections}"))?;
    write("mat.ini", format!("[MAPS]\ncount={}\n{maps}", ids.len()))?;
    let country = input.country.map(|c| format!("countrycode={c}\n")).unwrap_or_default()
        + &input.place.map(|[lat, lon]| format!("latitude={lat:.4}\nlongitude={lon:.4}\n")).unwrap_or_default();
    write(
        "track.ini",
        format!(
            "[INFO]\nname={}\nphysics={physics}\nauthor=Codemasters (DiRT Rally)\nversion=DiRT Rally local conversion\nlength={:.3}\ncomment=Converted locally from {}\n{country}{}",
            input.title,
            input.length / 1000.,
            input.route,
            if input.circuit { "circuit=1\n" } else { "" }
        ),
    )?;
    write("settings.ini", SETTINGS_INI.to_owned())?;
    let mut dl = format!("[DRIVELINE]\ncount={}\n", line.points.len());
    let n = line.points.len();
    for (i, p) in line.points.iter().enumerate() {
        // A ring's last point repeats its first: neighbours wrap around the seam.
        let (a, b) = if input.circuit {
            (line.points[(i + n - 2) % (n - 1)], line.points[(i + 1) % (n - 1)])
        } else {
            (line.points[i.saturating_sub(1)], line.points[(i + 1).min(n - 1)])
        };
        let length = rbr_ac_tracks::assetto::ai::distance(a, b).max(1e-9);
        let t: [f64; 3] = std::array::from_fn(|k| (b[k] - a[k]) / length);
        let at = if input.circuit { line.at[i] } else { i as f64 * 2.5 };
        let _ = writeln!(
            dl,
            "K{i}={}, {}, {}, {}, {}, {}, {}, 0",
            p[0] - origin[0],
            p[1] - origin[1],
            p[2],
            t[0],
            t[1],
            t[2],
            at
        );
    }
    write("driveline.ini", dl)?;
    // Start, finish and the result marker 15 m past the finish; the splits; the corners graded from the line.
    // A circuit's start, finish and end keys lie far past the lap so the 1.02 timekeeper never fires them, and it has no
    // pacenotes.
    let far = rbr_btb::circuit::FAR;
    let mut notes = if input.circuit {
        vec![(0x15, 0, far), (0x16, 0, far + 10.), (0x18, 0, far + 20.)]
    } else {
        vec![(0x15, 0, start), (0x16, 0, finish), (0x18, 0, (finish + 15.).min(line.length()))]
    };
    notes.extend(input.splits.iter().map(|s| (0x17, 0, if input.circuit { *s } else { s - from })));
    let called = if input.circuit { Vec::new() } else { pacenotes::notes(&line.points, &line.at, start, finish) };
    notes.extend(called.iter().map(|n| (n.tag, n.flags, n.distance)));
    notes.sort_by(|a, b| a.2.total_cmp(&b.2));
    let mut pn = format!("[PACENOTES]\ncount={}\n", notes.len());
    for (i, (tag, flag, s)) in notes.iter().enumerate() {
        let _ = writeln!(pn, "\n[P{i}]\ntype={tag}\nflag={flag}\ndistance={s:.4}");
    }
    write("pacenotes.ini", pn)?;
    let mut sp = String::from("[SPAWN]\n");
    for i in 0..16 {
        let v = match i {
            0 | 5 | 10 => 1.,
            12 => spawn[0] - origin[0],
            13 => spawn[2],
            14 => spawn[1] - origin[1],
            _ => 0.,
        };
        let _ = writeln!(sp, "m{i:02}={v}");
    }
    let _ = writeln!(sp, "angleX=0\nangleY=0\nangleZ={}", heading[0].atan2(heading[1]));
    write("spawn.ini", sp)?;
    let provenance = serde_json::json!({
        "source": input.source, "route": input.route, "origin": origin, "physics": physics,
        "length_m": input.length, "line_length_m": input.finish - input.start, "start_m": start, "finish_m": finish,
        "drawn_triangles": drawn_triangles, "collision_triangles": collision_triangles,
        "collision_source": input.collision_source, "surface_triangles": input.surface_counts,
        "circuit": input.circuit, "textures": textures, "pacenotes": called.len(), "missing_ground_points": missing,
    });
    let provenance = serde_json::to_string_pretty(&provenance).map_err(|e| e.to_string())?;
    write(CONVERSION_FILE, provenance)?;
    control.say("validating BTB collision cells");
    let stats = rbr_ac_tracks::btb::stage::check_cells(folder)?;
    control.say(&format!("{} cells, largest {}/65536 vertices; {textures} textures", stats.cells, stats.max_vertices));
    Ok(Report {
        folder: folder.to_owned(),
        length: input.length,
        physics,
        drawn_triangles,
        instanced_triangles,
        collision_triangles,
        textures,
        max_cell_vertices: stats.max_vertices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicBool, Arc};

    /// A strip from `x0` to `x1` along y from -20 to 620 m, two triangles every 20 m.
    fn strip(x0: f64, x1: f64, material: u32) -> Vec<Triangle> {
        let t = |pos: [[f64; 3]; 3]| Triangle {
            pos,
            uv: [[0.; 2]; 3],
            normal: [[0., 0., 1.]; 3],
            material: Some(material),
        };
        // The BTB mesh's winding: clockwise seen from above.
        (0..32)
            .flat_map(|i| {
                let (y0, y1) = (f64::from(i) * 20. - 20., f64::from(i) * 20.);
                [t([[x0, y0, 0.], [x0, y1, 0.], [x1, y0, 0.]]), t([[x1, y0, 0.], [x0, y1, 0.], [x1, y1, 0.]])]
            })
            .collect()
    }

    #[test]
    fn a_prop_with_maps_is_a_bump_and_highlight_section() {
        let out = std::env::temp_dir().join(format!("rbr-dirt-rally-maps-{}.zip", std::process::id()));
        let mut bank = TextureBank::new(&out).unwrap();
        let flat = || Some(rbr_ac_tracks::btb::textures::flat([0.5; 4]));
        let m = Material { normal: flat(), specular: flat(), ..Material::default() };
        let mut ini = String::new();
        material::section_with(&mut ini, "p", "d.dds", false, &m.extras(&mut bank).unwrap());
        assert!(ini.contains("ReRbrEffect=BTB_DiffuseBumpSpecular\n") && ini.contains("SpecularPower=40.0\n"), "{ini}");
        assert!(ini.contains("normalTexture=") && ini.contains("specularTexture=") && !ini.contains("ReRbrBumpScale"));
        ini.clear();
        material::section_with(&mut ini, "q", "d.dds", false, &Material::default().extras(&mut bank).unwrap());
        assert!(!ini.contains("ReRbrEffect") && !ini.contains("normalTexture"), "{ini}");
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn a_straight_course_writes_a_stage_whose_cells_check() {
        let out = std::env::temp_dir().join(format!("rbr-dirt-rally-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let line = Line::new((0..=60).map(|i| [0., f64::from(i) * 10., 0.]).collect(), vec![6.; 61]);
        let mut collision = strip(-4., 4., 17);
        collision.extend(strip(-30., -4., 113));
        let input = Input {
            source: "route_0".into(),
            route: "test/test_rally_01/route_0".into(),
            title: "Test".into(),
            country: Some("GR"),
            place: Some([38.5, 22.5]),
            length: 480.,
            line,
            start: 60.,
            finish: 540.,
            circuit: false,
            splits: vec![300.],
            materials: vec![Material { name: "road".into(), ..Material::default() }],
            drawn: strip(-4., 4., 0),
            layer: Vec::new(),
            instanced: Instanced::default(),
            collision,
            collision_source: "test",
            surface_counts: BTreeMap::new(),
        };
        let options = Options {
            out: out.clone(),
            name: "DR_test".into(),
            draw_corridor: 300.,
            collision_corridor: 75.,
            lod: 1000.,
            replace: false,
            control: Control::new(|_| {}, Arc::new(AtomicBool::new(false))),
        };
        let report = convert(&input, &options).unwrap();
        assert_eq!(
            (report.physics, report.length, report.drawn_triangles, report.collision_triangles),
            ("gravel", 480., 64, 128)
        );
        let read = |file: &str| std::fs::read_to_string(report.folder.join(file)).unwrap();
        assert!(read("track.ini").contains("physics=gravel\n") && read("track.ini").contains("countrycode=GR\n"));
        assert!(read("track.ini").contains("latitude=38.5000\nlongitude=22.5000\n"));
        let notes = read("pacenotes.ini");
        assert!(
            notes.contains("type=21\nflag=0\ndistance=50.0000") && notes.contains("type=23\nflag=0\ndistance=290.0000")
        );
        assert!(read("mat.ini").contains("data0=17 17") && read("mat.ini").contains("wet0=115 115"));
        // A second conversion only replaces with --replace.
        assert!(convert(&input, &options).is_err());
        assert!(convert(&input, &Options { replace: true, ..options }).is_ok());
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn a_lap_writes_a_closed_ring_circuit() {
        let out = std::env::temp_dir().join(format!("rbr-dirt-rally-circuit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        // Up one strip and back down the next: 1240 m, the lap line (arc 0) at the foot of the first.
        let mut points: Vec<[f64; 3]> = (0..=60).map(|i| [0., f64::from(i) * 10., 0.]).collect();
        points.extend((0..=60).map(|i| [20., 600. - f64::from(i) * 10., 0.]));
        points.push(points[0]);
        let line = Line::new(points, vec![6.; 123]);
        let length = line.length();
        let collision = strip(-4., 24., 17);
        let input = Input {
            source: "route_0".into(),
            route: "test/test_rallycross/route_0".into(),
            title: "Test".into(),
            country: None,
            place: None,
            length,
            line,
            start: 10.,
            finish: 10. + length,
            circuit: true,
            splits: vec![300., 900.],
            materials: vec![Material { name: "road".into(), ..Material::default() }],
            drawn: strip(-4., 4., 0),
            layer: Vec::new(),
            instanced: Instanced::default(),
            collision,
            collision_source: "test",
            surface_counts: BTreeMap::new(),
        };
        let options = Options {
            out: out.clone(),
            name: "DR_circuit".into(),
            draw_corridor: 300.,
            collision_corridor: 75.,
            lod: 1000.,
            replace: false,
            control: Control::new(|_| {}, Arc::new(AtomicBool::new(false))),
        };
        let report = convert(&input, &options).unwrap();
        let read = |file: &str| std::fs::read_to_string(report.folder.join(file)).unwrap();
        assert!(read("track.ini").contains("circuit=1\n"));
        // The ring starts at the lap line, 10 m up the first strip, and ends where it began; the car stands 1 m past it.
        let keys: Vec<Vec<f64>> = read("driveline.ini")
            .lines()
            .filter(|l| l.starts_with('K'))
            .map(|l| l.split('=').nth(1).unwrap().split(',').map(|v| v.trim().parse().unwrap()).collect())
            .collect();
        let (first, last) = (&keys[0], keys.last().unwrap());
        assert_eq!(first[..3], last[..3]);
        assert!((last[6] - length).abs() < 0.01, "{}", last[6]);
        // The start, finish and end keys lie far past the lap, the splits along it from the line (as given); no calls.
        let notes = read("pacenotes.ini");
        for key in
            ["type=21\nflag=0\ndistance=40000", "type=22\nflag=0\ndistance=40010", "type=24\nflag=0\ndistance=40020"]
        {
            assert!(notes.contains(key), "{key}");
        }
        assert!(
            notes.contains("count=5") && notes.contains("distance=300.0000") && notes.contains("distance=900.0000")
        );
        let spawn = read("spawn.ini");
        assert!(spawn.contains("m12=-10\n") && spawn.contains("m14=-289\n") && spawn.contains("angleZ=0"), "{spawn}");
        let _ = std::fs::remove_dir_all(&out);
    }
}
