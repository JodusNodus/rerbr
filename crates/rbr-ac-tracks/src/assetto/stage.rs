//! AC point-to-point course to an exported BTB stage. Mesh files use AC's Direct3D axes;
//! physics/driveline files use `(x,z,y)`, matching the BTB loader's axis swap.
use crate::assetto::{ai, csp, kn5, material, multilayer, trees};
use crate::btb::{
    control::Control,
    geometry::{Grid, Triangle},
    mesh::{self as xwrite, XMesh},
    pacenotes,
    road::Line,
    textures,
};
use kn5::{Matrix, IDENTITY};
use rbr_btb::circuit::FAR;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// Starts the error of a missing timing gate marker: a closed AI loop without gates is a circuit.
pub const MISSING_GATE: &str = "missing timing marker";

#[derive(Debug)]
pub struct Input {
    pub root: PathBuf,
    pub layout: String,
    pub models: Vec<(PathBuf, Matrix)>,
    pub markers: BTreeMap<String, Matrix>,
    pub line: Line,
    pub start: f64,
    pub finish: f64,
    pub splits: Vec<f64>,
    pub title: String,
    pub author: String,
    pub ai_source: PathBuf,
    pub reversed_ai: bool,
    /// Gates invented from a closed AI loop rather than read from the source.
    /// A closed AI loop played as ReRBR circuit: `start..finish` is one lap of `line`.
    pub circuit: bool,
    pub ai_extensions: [f64; 2],
    pub ai_trimmed_points: usize,
}

/// The ground under a driveline point within 3 m of its height. A collision mesh may have a gap a few centimetres
/// wide under the line (Nagao: 36 of 2 526 points, every one with ground within 0.3 m), so the nearest ground within
/// half a metre stands in for it.
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

const BACKWARDS: &str = "finish must follow start by at least 50 m on the AI line";
/// The same on a closed AI loop, which only a circuit can use.
pub const BACKWARDS_LOOP: &str = "finish must follow start by at least 50 m on the AI line, a closed loop";

fn relative(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value.replace('\\', "/"));
    if path.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) || value.contains(':') {
        return Err(format!("expected relative track path: {value}"));
    }
    Ok(path)
}

/// AC's Windows paths are case insensitive, including nested references.
pub(crate) fn source_path(root: &Path, value: &str) -> Result<PathBuf, String> {
    let mut path = root.to_owned();
    for component in relative(value)?.components() {
        let Component::Normal(name) = component else { continue };
        let exact = path.join(name);
        if exact.exists() {
            path = exact;
            continue;
        }
        let mut matches = std::fs::read_dir(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .filter_map(Result::ok)
            .filter(|e| e.file_name().eq_ignore_ascii_case(name));
        let found = matches.next().ok_or_else(|| format!("{}: missing {}", path.display(), name.to_string_lossy()))?;
        if matches.next().is_some() {
            return Err(format!("ambiguous case-insensitive path {}", exact.display()));
        }
        path = found.path();
    }
    Ok(path)
}

fn triplet(value: Option<&str>) -> Result<[f32; 3], String> {
    let Some(value) = value else {
        return Ok([0.; 3]);
    };
    let values = value
        .split(';')
        .next()
        .unwrap_or("")
        .split(',')
        .map(|v| v.trim().parse::<f32>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let out: [f32; 3] = values.try_into().map_err(|_| "expected three coordinates")?;
    if !out.iter().all(|v| v.is_finite()) {
        return Err("non-finite model placement".into());
    }
    Ok(out)
}

impl Input {
    pub fn read(root: &Path, layout: &str) -> Result<Self, String> {
        Self::read_with_ai(root, layout, None, false, false)
    }

    /// `circuit` makes a closed AI loop one lap of a ReRBR circuit, whatever the source's timing gates. An AI line
    /// that runs from the finish to the start (Akina's uphill layout) is reversed.
    pub fn read_with_ai(
        root: &Path,
        layout: &str,
        ai_file: Option<&Path>,
        reverse_ai: bool,
        circuit: bool,
    ) -> Result<Self, String> {
        let first = Self::read_as_given(root, layout, ai_file, reverse_ai, circuit);
        match &first {
            Err(e) if e == BACKWARDS && !reverse_ai => {
                Self::read_as_given(root, layout, ai_file, true, circuit).or(first)
            }
            _ => first,
        }
    }

    fn read_as_given(
        root: &Path,
        layout: &str,
        ai_file: Option<&Path>,
        reverse_ai: bool,
        circuit: bool,
    ) -> Result<Self, String> {
        let layout = if layout.is_empty() { "." } else { layout };
        if layout != "." && (relative(layout)?.components().count() != 1 || relative(layout)?.file_name().is_none()) {
            return Err("--layout must name one layout folder".into());
        }
        let root = root.canonicalize().map_err(|e| format!("{}: {e}", root.display()))?;
        let layout_path = source_path(&root, layout)?;
        let file = if layout == "." { "models.ini".to_owned() } else { format!("models_{layout}.ini") };
        let text = match source_path(&root, &file) {
            Ok(file) => std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?,
            Err(_) if layout == "." => {
                let base = root.file_name().unwrap().to_string_lossy();
                let model = source_path(&root, &format!("{base}.kn5"))?;
                format!("[MODEL_0]\nFILE={}\n", model.file_name().unwrap().to_string_lossy())
            }
            Err(e) => return Err(e),
        };
        let sections = rbr_formats::ini::parse(&text).map_err(|e| e.to_string())?;
        let mut models = Vec::new();
        let mut markers = BTreeMap::new();
        for section in sections.iter().filter(|s| s.name.to_uppercase().starts_with("MODEL_")) {
            let name = section.get("FILE").ok_or("MODEL section has no FILE")?.split(';').next().unwrap().trim();
            let path = source_path(&root, name)?;
            let placement = kn5::placement(triplet(section.get("POSITION"))?, triplet(section.get("ROTATION"))?);
            let model = kn5::read(&path, false, false, &placement)?;
            markers.extend(model.markers);
            models.push((path, placement));
        }
        if models.is_empty() {
            return Err("no models in models_<layout>.ini".into());
        }
        let ai_source = match ai_file {
            Some(path) => path.canonicalize().map_err(|e| e.to_string())?,
            None => source_path(&layout_path, "ai/fast_lane.ai")?,
        };
        let mut line = ai::read(&ai_source)?;
        if reverse_ai {
            line.points.reverse();
            line.width.reverse();
            line = Line::new(line.points, line.width);
        }
        let ai_trimmed_points = if circuit { 0 } else { ai::trim_terminal_return(&mut line) };
        let mut ai_extensions = [0.; 2];
        if circuit {
            line = ai::close_loop(&line)?;
        }
        for (i, name) in ["AC_AB_START", "AC_AB_FINISH"].into_iter().enumerate().filter(|_| !circuit) {
            if let (Some(a), Some(b)) = (markers.get(&format!("{name}_L")), markers.get(&format!("{name}_R"))) {
                let p = ai::world(std::array::from_fn(|k| (a[12 + k] + b[12 + k]) * 0.5));
                ai_extensions[i] = ai::cover_endpoint(&mut line, p, i == 0);
            }
        }
        let gate = |name: &str| -> Result<f64, String> {
            let a = markers.get(&format!("{name}_L")).ok_or_else(|| {
                format!("{MISSING_GATE} {name}_L; this converter requires point-to-point timing gates")
            })?;
            let b = markers.get(&format!("{name}_R")).ok_or_else(|| format!("{MISSING_GATE} {name}_R"))?;
            let p = ai::world(std::array::from_fn(|i| (a[12 + i] + b[12 + i]) * 0.5));
            let (s, error) = ai::project(&line, p);
            if error > 15. {
                return Err(format!("{name} is {error:.1} m off the AI line"));
            }
            Ok(s)
        };
        let (start, finish) = if circuit {
            // The lap line is the source's start gate when it lies on the loop, else the AI line's first point.
            let start = gate("AC_AB_START").unwrap_or(0.);
            (start, start + line.length())
        } else {
            (gate("AC_AB_START")?, gate("AC_AB_FINISH")?)
        };
        if finish <= start + 50. {
            // On a closed loop the gates say nothing about direction: it is a circuit, not a reversed line.
            return Err(if ai::close_loop(&line).is_ok() { BACKWARDS_LOOP } else { BACKWARDS }.into());
        }
        let mut splits = Vec::new();
        for i in (1..16).filter(|_| !circuit) {
            let key = format!("AC_TIME_{i}");
            if let (Some(a), Some(b)) = (markers.get(&format!("{key}_L")), markers.get(&format!("{key}_R"))) {
                // Older tracks keep zero-sized sector placeholders at the origin.
                if ai::distance(ai::world([a[12], a[13], a[14]]), ai::world([b[12], b[13], b[14]])) < 0.5 {
                    continue;
                }
                match gate(&key) {
                    Ok(s) if s > start + 20. && s < finish - 20. => splits.push(s),
                    _ => {}
                }
            }
        }
        splits.sort_by(f64::total_cmp);
        splits.dedup_by(|a, b| (*a - *b).abs() < 1.);
        if splits.is_empty() {
            splits = vec![start + (finish - start) / 3., start + 2. * (finish - start) / 3.];
        }
        let metadata_path = source_path(&root, &format!("ui/{layout}/ui_track.json")).ok();
        let metadata = metadata_path
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(t.trim_start_matches('\u{feff}')).ok());
        let fallback_title =
            if layout == "." { root.file_name().unwrap().to_string_lossy().into_owned() } else { layout.to_owned() };
        let title =
            metadata.as_ref().and_then(|m| m["name"].as_str()).unwrap_or(&fallback_title).replace(['\n', '\r'], " ");
        let author = metadata
            .as_ref()
            .and_then(|m| {
                m["author"]
                    .as_str()
                    .or_else(|| m["description"].as_str().filter(|s| s.to_uppercase().contains("JCRR")).map(|_| "JCRR"))
            })
            .unwrap_or("unknown")
            .replace(['\n', '\r'], " ");
        Ok(Self {
            root,
            layout: layout.into(),
            models,
            markers,
            line,
            start,
            finish,
            splits,
            title,
            author,
            ai_source,
            reversed_ai: reverse_ai,
            circuit,
            ai_extensions,
            ai_trimmed_points,
        })
    }
}

pub struct Options {
    pub out: PathBuf,
    pub name: String,
    pub draw_corridor: f64,
    pub collision_corridor: f64,
    pub lod: f64,
    pub replace: bool,
    pub remesh: bool,
    pub surface_overrides: BTreeMap<String, u8>,
    pub control: Control,
}

#[derive(Debug)]
pub struct Report {
    pub folder: PathBuf,
    pub length: f64,
    pub drawn_triangles: usize,
    pub collision_triangles: usize,
    pub textures: usize,
    pub missing_textures: usize,
    pub max_cell_vertices: usize,
}

/// Occupied plan cells, conservatively expanded from the sampled course. Triangles test their entire
/// bounding box, so long ground triangles crossing the road stay even when their centroid is far away.
struct Corridor {
    cells: HashSet<(i64, i64)>,
    cell: f64,
}
impl Corridor {
    fn new(line: &Line, radius: f64, cell: f64) -> Self {
        let mut cells = HashSet::new();
        let r = (radius / cell).ceil() as i64 + 1;
        for i in 0..=(line.length() / (cell * 0.5)).ceil() as usize {
            let p = line.sample((i as f64 * cell * 0.5).min(line.length())).0;
            let centre = ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64);
            for x in -r..=r {
                for y in -r..=r {
                    if ((x * x + y * y) as f64).sqrt() <= r as f64 {
                        cells.insert((centre.0 + x, centre.1 + y));
                    }
                }
            }
        }
        Self { cells, cell }
    }
    fn contains(&self, points: &[[f64; 3]]) -> bool {
        let lo = |k| (points.iter().map(|p| p[k]).fold(f64::MAX, f64::min) / self.cell).floor() as i64;
        let hi = |k| (points.iter().map(|p| p[k]).fold(f64::MIN, f64::max) / self.cell).floor() as i64;
        for x in lo(0)..=hi(0) {
            for y in lo(1)..=hi(1) {
                if self.cells.contains(&(x, y)) {
                    return true;
                }
            }
        }
        false
    }
}

#[derive(Default)]
struct Builder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    /// The source UVs of a per-tile mesh, whose `uvs` are the tile texture's: a second set for the normal map.
    uvs2: Option<Vec<[f32; 2]>>,
    triangles: Vec<[u32; 3]>,
    vertices: HashMap<[u32; 8], u32>,
    /// The `materials.ini` section it is drawn with.
    material: String,
}
impl Builder {
    /// The surface it draws, m².
    fn area(&self) -> f64 {
        let at = |i: u32| self.positions[i as usize].map(f64::from);
        self.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(at);
                let (u, v) = (
                    std::array::from_fn::<f64, 3, _>(|k| b[k] - a[k]),
                    std::array::from_fn::<f64, 3, _>(|k| c[k] - a[k]),
                );
                let cross = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                0.5 * cross.iter().map(|c| c * c).sum::<f64>().sqrt()
            })
            .sum()
    }
    fn add(&mut self, triangle: &Triangle, origin: [f64; 2], visible: bool) {
        let indices = std::array::from_fn(|k| {
            let p = triangle.pos[k];
            let position = [(p[0] - origin[0]) as f32, p[2] as f32, (p[1] - origin[1]) as f32];
            let normal = if visible {
                let n = triangle.normal[k];
                [n[0], n[2], n[1]]
            } else {
                [0., 1., 0.]
            };
            let uv = if visible { triangle.uv[k] } else { [0.; 2] };
            let key = [position[0], position[1], position[2], normal[0], normal[1], normal[2], uv[0], uv[1]]
                .map(f32::to_bits);
            *self.vertices.entry(key).or_insert_with(|| {
                let id = self.positions.len() as u32;
                self.positions.push(position);
                self.normals.push(normal);
                self.uvs.push(uv);
                id
            })
        });
        if indices[0] != indices[1] && indices[1] != indices[2] && indices[0] != indices[2] {
            self.triangles.push(indices);
        }
    }
    fn bytes(&self, material: &str) -> Vec<u8> {
        xwrite::write(&XMesh {
            positions: &self.positions,
            normals: &self.normals,
            uvs: &self.uvs,
            uvs2: self.uvs2.as_deref(),
            triangles: &self.triangles,
            material,
        })
    }
}

fn tile(t: &Triangle) -> (i64, i64) {
    let c = |k: usize| ((t.pos[0][k] + t.pos[1][k] + t.pos[2][k]) / 300.).floor() as i64;
    (c(0), c(1))
}

/// Side of the cells ReRBR merges drawn meshes into (`CELL` in its BTB stage), metres.
const SCENERY_CELL: f64 = 128.;
/// A mesh wider than this half extent is drawn alone and ReRBR adds its radius to the range itself.
const SOLO: f32 = 150.;

/// Scenery past the draw corridor is kept where its triangles are large for their distance from the course: a longest
/// side of at least this share of it (over a degree), the mountains, sea and tree lines AC draws to the horizon.
const FAR_SHARE: f64 = 1. / 50.;
/// Nothing past this distance from the course is kept, and far scenery is drawn this far: ReRBR's far plane (`coords::FAR`).
const FAR_REACH: f64 = 10_000.;
/// Side of the tiles far scenery is grouped in, metres: each is a mesh of its own (over [`SOLO`]).
const FAR_CELL: f64 = 1024.;

/// The course's plan points every 25 m, for the distance of far scenery ([`FAR_SHARE`]).
struct Far {
    points: Vec<[f64; 2]>,
    /// The draw corridor's radius: a triangle reaching less cannot be kept past it.
    corridor: f64,
}
impl Far {
    fn new(line: &Line, corridor: f64) -> Self {
        let n = (line.length() / 25.).ceil() as usize;
        let points =
            (0..=n).map(|i| line.sample((i as f64 * 25.).min(line.length())).0).map(|p| [p[0], p[1]]).collect();
        Self { points, corridor }
    }
    fn keeps(&self, pos: &[[f64; 3]; 3]) -> bool {
        let side =
            |a: [f64; 3], b: [f64; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        let longest = side(pos[0], pos[1]).max(side(pos[1], pos[2])).max(side(pos[2], pos[0]));
        let centre = [0, 1].map(|k| (pos[0][k] + pos[1][k] + pos[2][k]) / 3.);
        let reach = (longest / FAR_SHARE).min(FAR_REACH);
        if reach < self.corridor {
            return false;
        }
        self.points.iter().any(|p| (p[0] - centre[0]).hypot(p[1] - centre[1]) <= reach)
    }
    /// Whether a triangle's centre lies within reach of the course.
    fn near(&self, pos: &[[f64; 3]; 3]) -> bool {
        let centre = [0, 1].map(|k| (pos[0][k] + pos[1][k] + pos[2][k]) / 3.);
        self.points.iter().any(|p| (p[0] - centre[0]).hypot(p[1] - centre[1]) <= FAR_REACH)
    }
    /// Which of `rest` (a mesh's triangles the size test dropped) join `kept` (those it kept): the whole surface
    /// connected to them through shared vertices, within [`FAR_REACH`]. The size test alone keeps a mountain's large
    /// triangles and drops its small ones, leaving holes and floating crags; a ring or three around them still did.
    fn connected(&self, kept: &[Triangle], rest: &[Triangle]) -> Vec<bool> {
        let key = |p: &[f64; 3]| p.map(|c| (c * 1000.).round() as i64);
        let mut at: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
        for (i, t) in rest.iter().enumerate().filter(|(_, t)| self.near(&t.pos)) {
            for p in &t.pos {
                at.entry(key(p)).or_default().push(i);
            }
        }
        let mut joined = vec![false; rest.len()];
        let mut seen: HashSet<[i64; 3]> = HashSet::new();
        let mut queue: Vec<[i64; 3]> = kept.iter().flat_map(|t| t.pos.iter().map(key)).collect();
        while let Some(vertex) = queue.pop() {
            if !seen.insert(vertex) {
                continue;
            }
            for &i in at.get(&vertex).into_iter().flatten() {
                if !joined[i] {
                    joined[i] = true;
                    queue.extend(rest[i].pos.iter().map(key));
                }
            }
        }
        joined
    }
    fn tile(&self, t: &Triangle, origin: [f64; 2]) -> (i64, i64) {
        let c = |k: usize| (((t.pos[0][k] + t.pos[1][k] + t.pos[2][k]) / 3. - origin[k]) / FAR_CELL).floor() as i64;
        (c(0), c(1))
    }
}

/// The drawn tile of a triangle: ReRBR's cells, so a mesh is not merged with its neighbours.
fn scenery_tile(t: &Triangle, origin: [f64; 2]) -> (i64, i64) {
    let c = |k: usize| (((t.pos[0][k] + t.pos[1][k] + t.pos[2][k]) / 3. - origin[k]) / SCENERY_CELL).floor() as i64;
    (c(0), c(1))
}

/// The box of a mesh's vertices, `(min, max)`.
type Bounds = ([f32; 3], [f32; 3]);

fn bounds(positions: &[[f32; 3]]) -> Bounds {
    let first = positions.first().copied().unwrap_or_default();
    positions.iter().fold((first, first), |(low, high), p| {
        (std::array::from_fn(|k| low[k].min(p[k])), std::array::from_fn(|k| high[k].max(p[k])))
    })
}

/// Extra metres of `LODOut` (and fewer of `LODIn`) for each mesh. ReRBR measures the range from the centre of the box of
/// all meshes it merges (those whose box centres share a cell and a range); AC measures it from each object. Padding by
/// that box's half diagonal keeps an object drawn whenever it is within its source range. `meshes` are each box and range.
fn lod_padding(meshes: &[(Bounds, (u32, u32))]) -> Vec<f32> {
    let centre = |(low, high): &Bounds, k: usize| (low[k] + high[k]) / 2.;
    let half = |(low, high): &Bounds| std::array::from_fn::<f32, 3, _>(|k| (high[k] - low[k]) / 2.);
    let key = |(b, lod): &(Bounds, (u32, u32))| {
        let cell = |k: usize| (f64::from(centre(b, k)) / SCENERY_CELL).floor() as i64;
        (cell(0), cell(2), *lod)
    };
    let solo = |b: &Bounds| half(b).into_iter().fold(0., f32::max) > SOLO;
    let mut merged = HashMap::<_, Bounds>::new();
    for mesh in meshes.iter().filter(|(b, _)| !solo(b)) {
        let (low, high) = merged.entry(key(mesh)).or_insert(mesh.0);
        for k in 0..3 {
            low[k] = low[k].min(mesh.0 .0[k]);
            high[k] = high[k].max(mesh.0 .1[k]);
        }
    }
    meshes
        .iter()
        .map(
            |mesh| if solo(&mesh.0) { 0. } else { half(&merged[&key(mesh)]).iter().map(|h| h * h).sum::<f32>().sqrt() },
        )
        .collect()
}

struct Objects {
    zip: zip::ZipWriter<std::fs::File>,
    ini: String,
    count: usize,
}
impl Objects {
    fn new(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut out = Self { zip: zip::ZipWriter::new(file), ini: String::new(), count: 0 };
        out.put("Objects/identity.xform", &IDENTITY.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>())?;
        Ok(out)
    }
    fn put(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        self.zip.start_file(name, deflated()).map_err(|e| e.to_string())?;
        self.zip.write_all(bytes).map_err(|e| e.to_string())
    }
    /// Writes `(geometry, material, objectlist entries)` meshes in order. The bytes are built and deflated on every core,
    /// each as a one-entry archive that is copied in without recompressing: deflate is most of an import's time.
    fn meshes(&mut self, batch: Vec<(Builder, String, String)>) -> Result<(), String> {
        let batch: Vec<_> = batch.into_iter().filter(|(b, ..)| !b.triangles.is_empty()).collect();
        let (first, count) = (self.count, batch.len());
        let threads = std::thread::available_parallelism().map_or(4, usize::from).min(batch.len().max(1));
        let archives = std::thread::scope(|scope| {
            let batch = &batch;
            let workers: Vec<_> = (0..threads)
                .map(|t| {
                    scope.spawn(move || {
                        (t..batch.len())
                            .step_by(threads)
                            .map(|i| {
                                (i, archive(&format!("Objects/ac_{}.x", first + i), &batch[i].0.bytes(&batch[i].1)))
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            let mut all: Vec<_> = workers.into_iter().flat_map(|w| w.join().unwrap()).collect();
            all.sort_by_key(|(i, _)| *i);
            all.into_iter().map(|(_, archive)| archive)
        });
        for (i, (archive, (.., entries))) in archives.zip(batch).enumerate() {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(archive?)).map_err(|e| e.to_string())?;
            self.zip.raw_copy_file(archive.by_index_raw(0).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            let _ = writeln!(
                self.ini,
                "\n[Object_{}]\nfile=ac_{}\nxform=identity\ncloneCount=1\nDrawInstanced=0\nMoveable=0\n{entries}",
                first + i,
                first + i
            );
        }
        self.count += count;
        Ok(())
    }
    fn finish(self) -> Result<String, String> {
        self.zip.finish().map_err(|e| e.to_string())?;
        Ok(format!("[OBJECTS]\ncount={}\n{}", self.count, self.ini))
    }
}

fn deflated() -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
}

/// `bytes` as the only entry of a zip held in memory.
fn archive(name: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file(name, deflated()).map_err(|e| e.to_string())?;
    zip.write_all(bytes).map_err(|e| e.to_string())?;
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}

/// A drawn mesh's tile, source material and LOD range bits.
/// A drawn mesh's tile, material, source `LODIn` and `LODOut`, and whether it is far scenery ([`Far`]).
type DrawKey = ((i64, i64), usize, u32, u32, bool);

/// A tile's bake as a DDS file, and the builder drawn with it.
type Tile = (Vec<u8>, Builder);

/// Metres a baked texel of world-space multilayer ground covers at `distance` metres from the driveline.
fn texel_metres(distance: f64) -> f64 {
    0.03 + 0.003 * distance.max(0.)
}

/// The tile textures of the builders whose material is world-space multilayer (`multilayer::bake_tile`), as DXT1
/// files with the meshes drawn with them, baked on every core; `None` for the others, the materials the renderer sums per
/// pixel (`drawn_per_pixel`, [`material::Layers`]), a tile that does not fit and a tile whose material bake is already
/// as fine.
fn tile_bakes(
    model: &kn5::Model,
    builders: &[(DrawKey, Builder)],
    origin: [f64; 2],
    line: &Line,
    drawn_per_pixel: &BTreeSet<usize>,
) -> Vec<Option<Tile>> {
    let mut layers = BTreeMap::new();
    for ((_, mat, ..), _) in builders {
        layers
            .entry(*mat)
            .or_insert_with(|| multilayer::Layers::new(model, *mat).filter(|_| !drawn_per_pixel.contains(mat)));
    }
    // Far scenery keeps its material's own texture.
    let jobs: Vec<usize> =
        (0..builders.len()).filter(|&i| !builders[i].0 .4 && layers[&builders[i].0 .1].is_some()).collect();
    let threads = std::thread::available_parallelism().map_or(4, usize::from).min(jobs.len().max(1));
    let mut out: Vec<Option<Tile>> = (0..builders.len()).map(|_| None).collect();
    let done = std::thread::scope(|scope| {
        let (jobs, layers) = (&jobs, &layers);
        let workers: Vec<_> = (0..threads)
            .map(|t| {
                scope.spawn(move || {
                    let mut done = Vec::new();
                    for &i in jobs.iter().skip(t).step_by(threads) {
                        let ((_, mat, ..), b) = &builders[i];
                        // The mean distance of its vertices from the driveline sets the density.
                        let step = (b.positions.len() / 64).max(1);
                        let distances: Vec<f64> = b
                            .positions
                            .iter()
                            .step_by(step)
                            .map(|p| {
                                ai::project(line, [f64::from(p[0]) + origin[0], f64::from(p[2]) + origin[1], 0.]).1
                            })
                            .collect();
                        let distance = distances.iter().sum::<f64>() / distances.len().max(1) as f64;
                        let layers = layers[mat].as_ref().unwrap();
                        // A source fine enough keeps the material's single bake, which repeats with its UVs.
                        if texel_metres(distance) >= layers.texel {
                            continue;
                        }
                        // Back to AC's frame: the stage's (x, height, z) is AC's (x - origin, y, -z - origin).
                        let ac = |p: [f32; 3]| {
                            [f64::from(p[0]) + origin[0], f64::from(p[1]), -(f64::from(p[2]) + origin[1])]
                        };
                        let corners: Vec<multilayer::Corner> = b
                            .positions
                            .iter()
                            .zip(&b.uvs)
                            .map(|(p, uv)| multilayer::Corner { position: ac(*p), uv: uv.map(f64::from) })
                            .collect();
                        let bake = multilayer::bake_tile(layers, &corners, &b.triangles, texel_metres(distance));
                        done.push((
                            i,
                            bake.map(|bake| {
                                let (w, h, rgba) = bake.image;
                                let drawn = Builder {
                                    positions: bake.vertices.iter().map(|&v| b.positions[v as usize]).collect(),
                                    normals: bake.vertices.iter().map(|&v| b.normals[v as usize]).collect(),
                                    uvs: bake.uvs,
                                    uvs2: Some(bake.vertices.iter().map(|&v| b.uvs[v as usize]).collect()),
                                    triangles: bake.triangles,
                                    ..Builder::default()
                                };
                                (textures::encode(false, w, h, rgba), drawn)
                            }),
                        ));
                    }
                    done
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().unwrap()).collect::<Vec<_>>()
    });
    for (i, tile) in done {
        out[i] = tile;
    }
    out
}

/// The trees of CSP's lists inside the draw corridor, as the crossed cards of [`trees::prototype`]: their meshes, the
/// textures and materials they use are written to `objects`, `bank`, `materials` and `names`. Returns the trees and
/// triangles drawn.
fn tree_cards(
    trees: &[trees::Tree],
    root: &Path,
    draw: &Corridor,
    origin: [f64; 2],
    options: &Options,
    (objects, bank, materials, names): (&mut Objects, &mut material::TextureBank, &mut String, &mut Vec<String>),
) -> Result<(usize, usize), String> {
    // Each tree file's cards and the material drawn with them; `None` for a file that cannot be read.
    let mut prototypes = BTreeMap::<&str, Option<(trees::Prototype, String)>>::new();
    let mut builders = BTreeMap::<((i64, i64), String), Builder>::new();
    let mut placed = 0;
    let world = |x: f64, y: f64, z: f64| [x, -z, y];
    for tree in trees {
        let [x, y, z] = tree.position;
        if !draw.contains(&[world(x, y, z)]) {
            continue;
        }
        if !prototypes.contains_key(tree.bin.as_str()) {
            let made = source_path(root, &format!("extension/trees/{}", tree.bin))
                .and_then(|path| trees::prototype(&path))
                .and_then(|p| {
                    let (w, h, rgba) = p.atlas.clone();
                    let texture = bank.put(textures::encode(true, w, h, rgba), "dds")?;
                    let name = format!("ac\\tree_{}", prototypes.len());
                    material::section(materials, &name, &texture, true);
                    names.push(name.clone());
                    Ok((p, name))
                });
            if let Err(e) = &made {
                options.control.say(&format!("tree {} is not drawn: {e}", tree.bin));
            }
            prototypes.insert(tree.bin.as_str(), made.ok());
        }
        let Some((proto, material)) = &prototypes[tree.bin.as_str()] else { continue };
        let metres = tree.size * proto.height;
        let half = f64::from(proto.radius) * proto.width * tree.size * tree.width;
        let (low, high) = (y + f64::from(proto.low) * metres, y + f64::from(proto.high) * metres);
        let centre = Triangle { pos: [world(x, y, z); 3], uv: [[0.; 2]; 3], normal: [[0.; 3]; 3], material: None };
        let builder = builders.entry((scenery_tile(&centre, origin), material.clone())).or_default();
        for card in 0..trees::CARDS {
            let turn = tree.angle + std::f64::consts::PI * card as f64 / trees::CARDS as f64;
            let (dx, dz) = (turn.cos() * half, turn.sin() * half);
            let (u0, u1) = (card as f32 / trees::CARDS as f32, (card + 1) as f32 / trees::CARDS as f32);
            let corner = |side: f64, y: f64, u: f32, v: f32| (world(x + side * dx, y, z + side * dz), [u, v]);
            let [a, b, c, d] = [
                corner(-1., low, u0, 1.),
                corner(1., low, u1, 1.),
                corner(1., high, u1, 0.),
                corner(-1., high, u0, 0.),
            ];
            for [p, q, r] in [[a, b, c], [a, c, d]] {
                let up = [0., 0., 1.];
                let t = Triangle { pos: [p.0, q.0, r.0], uv: [p.1, q.1, r.1], normal: [up; 3], material: None };
                builder.add(&t, origin, true);
            }
        }
        placed += 1;
    }
    let builders: Vec<_> = builders.into_iter().map(|((_, material), b)| (material, b)).collect();
    let lod = (0, (options.lod as f32).to_bits());
    let boxes: Vec<_> = builders.iter().map(|(_, b)| (bounds(&b.positions), lod)).collect();
    let mut triangles = 0;
    let mut batch = Vec::new();
    for ((material, b), pad) in builders.into_iter().zip(lod_padding(&boxes)) {
        triangles += b.triangles.len();
        let entries =
            format!("LODIn=0\nLODOut={}\nVisible=1\nShadowCaster=1\nCollisionModel=-1\n", options.lod + f64::from(pad));
        batch.push((b, material, entries));
    }
    objects.meshes(batch)?;
    Ok((placed, triangles))
}

/// The file a conversion leaves in its stage's folder: what it was made from and how. It marks the stage as one of ours.
pub const CONVERSION_FILE: &str = "ac-conversion.json";

pub fn convert(input: &Input, options: &Options) -> Result<Report, String> {
    if options.name.is_empty()
        || relative(&options.name)?.components().count() != 1
        || relative(&options.name)?.file_name().is_none()
    {
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
    let parent = options.out.clone();
    std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let destination = parent.join(&options.name);
    if destination.exists() && (!options.replace || !destination.join(CONVERSION_FILE).is_file()) {
        return Err(format!(
            "{} exists; --replace only replaces stages generated by the AC converter",
            destination.display()
        ));
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let staging = parent.join(format!(".ac-stage-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let result = build(input, options, &staging);
    let mut report = match result {
        Ok(r) => r,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    if destination.exists() {
        let backup = parent.join(format!(".ac-backup-{}-{stamp}", std::process::id()));
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

struct Spawn {
    position: [f64; 3],
    tangent: [f64; 3],
    source: &'static str,
    before_start: f64,
}

/// The timing gate is not a grid position. Use AC's grid helper when it lies before the gate,
/// otherwise place the car five metres behind it. Helper heights often float above the road.
fn start_position(input: &Input, line: &Line, start: f64, ground: &Grid<'_>) -> Result<Spawn, String> {
    let (gate, forward) = line.sample(start);
    let behind = |p: [f64; 3]| (gate[0] - p[0]) * forward[0] + (gate[1] - p[1]) * forward[1];
    let on_ground = |mut p: [f64; 3], tangent, source| {
        let (z, _) = ground_near(ground, p)?;
        p[2] = z;
        Some(Spawn { position: p, tangent, source, before_start: behind(p) })
    };
    if let Some(marker) = input.markers.get("AC_START_0") {
        let mut p = ai::world([marker[12], marker[13], marker[14]]);
        let (s, error) = ai::project(line, p);
        let (road, tangent) = line.sample(s);
        p[2] = road[2];
        if error <= 5. && (2.0..=50.).contains(&behind(p)) {
            if let Some(spawn) = on_ground(p, tangent, "AC_START_0") {
                return Ok(spawn);
            }
        }
    }
    let (p, tangent) = if start >= 5. {
        line.sample(start - 5.)
    } else {
        let length = forward[0].hypot(forward[1]).max(1e-9);
        ([gate[0] - 5. * forward[0] / length, gate[1] - 5. * forward[1] / length, gate[2]], forward)
    };
    on_ground(p, tangent, "driveline_before_start").ok_or_else(|| "no road under the start position".into())
}

/// An AI recording can end at the finish gate. Leave enough grounded driveline beyond the result
/// marker for the controller to cross it before braking at the line's endpoint.
fn finish_runoff(line: &mut Line, finish: f64, ground: &Grid<'_>) -> Result<f64, String> {
    let original = line.length();
    if original >= finish + 25. {
        return Ok(0.);
    }
    let (end, tangent) = line.sample(original);
    let width = *line.width.last().unwrap_or(&6.);
    while line.length() < finish + 30. {
        let at = (line.length() + 2.5).min(finish + 30.);
        let mut p = std::array::from_fn(|k| end[k] + tangent[k] * (at - original));
        let Some((z, _)) = ground.ground(p[0], p[1], p[2]).filter(|(z, _)| (*z - p[2]).abs() < 3.) else {
            break;
        };
        p[2] = z;
        line.points.push(p);
        line.at.push(at);
        line.width.push(width);
    }
    if line.length() < finish + 20. {
        return Err("not enough grounded road beyond the finish for the result marker".into());
    }
    Ok(line.length() - original)
}

fn build(input: &Input, options: &Options, folder: &Path) -> Result<Report, String> {
    // A circuit is the lap from its line, wrapped; a stage keeps 50 m before the start and 40 m past the finish.
    let from = if input.circuit { input.start } else { (input.start - 50.).max(0.) };
    let to = if input.circuit { input.finish } else { (input.finish + 40.).min(input.line.length()) };
    let mut line = input.line.stretch(from, to - from, true, 2.5);
    // Preserve the source arc parameter: resampling a bend shortens its polyline slightly.
    // Timing markers and distance keys must stay attached to their source positions.
    line.at = (0..line.points.len()).map(|i| (i as f64 * 2.5).min(to - from)).collect();
    let start = input.start - from;
    let finish = input.finish - from;
    let origin = std::array::from_fn(|k| {
        ((line.points.iter().map(|p| p[k]).fold(f64::MAX, f64::min)
            + line.points.iter().map(|p| p[k]).fold(f64::MIN, f64::max))
            * 0.5)
            .round()
    });
    let draw = Corridor::new(&line, options.draw_corridor, 100.);
    let collide = Corridor::new(&line, options.collision_corridor, 25.);
    let far = Far::new(&line, options.draw_corridor);
    let mut far_triangles = 0;
    let surfaces = material::Surfaces::read(
        &source_path(&input.root, &format!("{}/data/surfaces.ini", input.layout))?,
        options.surface_overrides.clone(),
    )?;
    let mut collision = Vec::<Triangle>::new();
    let mut objects = Objects::new(&folder.join("Objects.zip"))?;
    let mut bank = material::TextureBank::new(&folder.join("Textures.zip"))?;
    bank.origin = origin;
    let mut materials = String::new();
    let mut names = Vec::new();
    let mut drawn_triangles = 0;
    let mut light_areas = BTreeMap::<String, f64>::new();
    let mut source_triangles = 0;
    let mut surface_counts = BTreeMap::<String, usize>::new();
    let mut repairs = Vec::new();
    let mut tile_textures = 0;
    // CSP draws its tree lists in place of the trees and hedges it hides from the models.
    let csp_trees = trees::read(&input.root);
    let hidden = if csp_trees.is_empty() { BTreeMap::new() } else { csp::replaced(&input.root) };
    let unrendered = csp::unrendered(&input.root);
    let grass_fx = csp::grass_fx(&input.root);
    for (model_index, (path, transform)) in input.models.iter().enumerate() {
        options.control.check()?;
        options.control.say(&format!("reading {}", path.file_name().unwrap().to_string_lossy()));
        let model = kn5::read(path, true, true, transform)?;
        if model.dropped_triangles + model.repaired_uvs + model.repaired_normals > 0 {
            options.control.say(&format!(
                "source repairs: {} dropped invalid triangles, {} UVs, {} normals",
                model.dropped_triangles, model.repaired_uvs, model.repaired_normals
            ));
            repairs.push(serde_json::json!({"file":path.file_name().unwrap().to_string_lossy(),"dropped_triangles":model.dropped_triangles,"repaired_uvs":model.repaired_uvs,"repaired_normals":model.repaired_normals}));
        }
        let mut builders = BTreeMap::<DrawKey, Builder>::new();
        let replaced = path.file_name().and_then(|n| hidden.get(&n.to_string_lossy().to_lowercase()));
        // Leaves, decals and glass whose texture was stripped would be opaque shards, and AC never draws some: leave
        // them out.
        let stripped: Vec<bool> = model
            .materials
            .iter()
            .map(|m| material::is_transparent(m) && material::is_placeholder(&model, m) || material::is_never_drawn(m))
            .collect();
        for mesh in &model.meshes {
            options.control.check()?;
            let (mut far_kept, mut far_rest) = (Vec::<Triangle>::new(), Vec::<Triangle>::new());
            source_triangles += mesh.triangle_count;
            if mesh.helper {
                continue;
            }
            // AC hides an inactive mesh but still collides with it: tracks keep their invisible walls that way.
            let physical = material::Surfaces::key(&mesh.name);
            let visible = mesh.active
                && mesh.visible
                && mesh.renderable
                && !stripped[mesh.material]
                && !replaced.is_some_and(|names| names.contains(&mesh.name.to_lowercase()))
                && !unrendered.iter().any(|pattern| csp::matches(pattern, &mesh.name.to_lowercase()));
            if physical.is_none() && !visible {
                continue;
            }
            // `ksPerPixelNM_UVMult` reads its diffuse at the UV times `1 + diffuseMult`.
            let uv_scale = material::uv_multipliers(&model, &model.materials[mesh.material]).map_or(1., |m| m[0]);
            for face in &mesh.triangles {
                // AC's frame is right handed and BTB's mesh frame left handed: the reflection reverses the winding.
                let vertices = [face[0], face[2], face[1]].map(|i| mesh.vertices[i as usize]);
                let t = Triangle {
                    pos: vertices.map(|v| ai::world(v.position)),
                    uv: vertices.map(|v| v.uv.map(|c| c * uv_scale)),
                    normal: vertices.map(|v| [v.normal[0], -v.normal[2], v.normal[1]]),
                    material: None,
                };
                if let Some(key) = physical.as_deref().filter(|_| collide.contains(&t.pos)) {
                    let steep = t.normal()[2].abs() < 0.3;
                    let id = surfaces.id(key, steep);
                    collision.push(Triangle { material: Some(id as u32), ..t });
                    *surface_counts.entry(format!("{key} -> {id}")).or_default() += 1;
                }
                if !visible {
                    continue;
                }
                let lod = (mesh.lod_in.to_bits(), mesh.lod_out.to_bits());
                if draw.contains(&t.pos) {
                    let key = (scenery_tile(&t, origin), mesh.material, lod.0, lod.1, false);
                    builders.entry(key).or_default().add(&t, origin, true);
                } else if far.keeps(&t.pos) {
                    far_kept.push(t);
                } else {
                    far_rest.push(t);
                }
            }
            let lod = (mesh.lod_in.to_bits(), mesh.lod_out.to_bits());
            let joined = far.connected(&far_kept, &far_rest);
            let rest = far_rest.into_iter().zip(joined).filter_map(|(t, joined)| joined.then_some(t));
            for t in far_kept.into_iter().chain(rest) {
                let key = (far.tile(&t, origin), mesh.material, lod.0, lod.1, true);
                builders.entry(key).or_default().add(&t, origin, true);
                far_triangles += 1;
            }
        }
        let builders: Vec<_> = builders.into_iter().collect();
        // World-space multilayer ground gets textures per tile, at a density falling off with distance from the road.
        let per_pixel: BTreeSet<usize> = builders
            .iter()
            .map(|((_, mat, ..), _)| *mat)
            .filter(|&mat| bank.layers(&model, &model.materials[mat]).is_some())
            .collect();
        let mut tiles = tile_bakes(&model, &builders, origin, &line, &per_pixel);
        let mut names_of = BTreeMap::new();
        let mut drawn = Vec::new();
        for (i, (key, mut b)) in builders.into_iter().enumerate() {
            let mat_index = key.1;
            if let Some((bytes, baked)) = tiles[i].take() {
                bank.baked.insert(model.materials[mat_index].shader.clone());
                let texture = bank.put(bytes, "dds")?;
                tile_textures += 1;
                b = baked;
                b.material = format!("ac\\{model_index}_{mat_index}_{i}");
                let extras = material::Extras {
                    grass: grass_fx.as_ref().is_some_and(|g| g.grows_on(&model.materials[mat_index].name)),
                    ..bank.tile_extras(&model, mat_index)
                };
                if !extras.bump_uv {
                    b.uvs2 = None;
                }
                material::section_with(&mut materials, &b.material, &texture, false, &extras);
                names.push(b.material.clone());
                drawn.push((key, b));
                continue;
            }
            b.material = match names_of.get(&mat_index) {
                Some(name) => String::clone(name),
                None => {
                    let mat = &model.materials[mat_index];
                    let (texture, opaque) = bank.diffuse(&model, mat_index)?;
                    let name = format!("ac\\{model_index}_{mat_index}");
                    let extras = material::Extras {
                        grass: grass_fx.as_ref().is_some_and(|g| g.grows_on(&mat.name)),
                        ..bank.extras(&model, mat_index)?
                    };
                    material::section_with(
                        &mut materials,
                        &name,
                        &texture,
                        !opaque && material::is_transparent(mat),
                        &extras,
                    );
                    names.push(name.clone());
                    names_of.insert(mat_index, name.clone());
                    name
                }
            };
            drawn.push((key, b));
        }
        let builders = drawn;
        let boxes: Vec<_> = builders
            .iter()
            .map(|((_, _, lod_in, lod_out, _), b)| (bounds(&b.positions), (*lod_in, *lod_out)))
            .collect();
        let mut batch = Vec::new();
        for (((_, mat, lod_in, lod_out, far), mut b), pad) in builders.into_iter().zip(lod_padding(&boxes)) {
            drawn_triangles += b.triangles.len();
            *light_areas.entry(b.material.clone()).or_default() += b.area();
            let lod_in = f32::from_bits(lod_in).max(0.);
            let source_lod = f64::from(f32::from_bits(lod_out));
            // A range the source gives is padded; the cap for a mesh without one is only a choice. Far scenery is drawn
            // as far as ReRBR sees, and an opaque surface in the corridor as far as its size holds ([`FAR_SHARE`]), so
            // the terrain and road of the valley below do not drop out in cells past `lod`; cut-outs keep `lod`.
            let cap = if far {
                FAR_REACH
            } else if material::is_transparent(&model.materials[mat]) {
                options.lod
            } else {
                let (low, high) = bounds(&b.positions);
                let extent = (0..3).map(|k| f64::from(high[k] - low[k])).fold(0., f64::max);
                (extent / FAR_SHARE).clamp(options.lod, FAR_REACH)
            };
            let lod_out = if source_lod > lod_in as f64 { source_lod.min(cap) + f64::from(pad) } else { cap };
            let lod_in = (lod_in - pad).max(0.);
            let entries = format!("LODIn={lod_in}\nLODOut={lod_out}\nVisible=1\nShadowCaster=1\nCollisionModel=-1\n");
            let material = std::mem::take(&mut b.material);
            batch.push((b, material, entries));
        }
        objects.meshes(batch)?;
        options.control.say(&format!(
            "{} source triangles, {drawn_triangles} drawn so far, {} collision so far",
            model.meshes.iter().map(|m| m.triangle_count).sum::<usize>(),
            collision.len()
        ));
    }
    let (tree_count, tree_triangles) = tree_cards(
        &csp_trees,
        &input.root,
        &draw,
        origin,
        options,
        (&mut objects, &mut bank, &mut materials, &mut names),
    )?;
    drawn_triangles += tree_triangles;
    if tree_count > 0 {
        options
            .control
            .say(&format!("{tree_count} of {} CSP trees drawn as cards ({tree_triangles} triangles)", csp_trees.len()));
    }
    if collision.is_empty() {
        return Err("no physical meshes near the course".into());
    }
    let turned_walls = crate::assetto::walls::face_road(&mut collision, &line);
    options.control.say(&format!("turned {turned_walls} wall faces toward the road"));
    let runoff_extension =
        if input.circuit { 0. } else { finish_runoff(&mut line, finish, &Grid::new(collision.iter().collect(), 4.))? };
    if runoff_extension > 0. {
        options.control.say(&format!("extended finish run-off {runoff_extension:.1} m over physical ground"));
    }
    options.control.check()?;
    if options.remesh {
        options.control.say("re-meshing road collision");
        let ground = Grid::new(collision.iter().collect(), 4.);
        let surface = crate::btb::remesh::build(&line, &ground);
        let mut adjusted = Vec::new();
        for mut t in collision.iter().copied() {
            if surface.replaces(&t) {
                continue;
            }
            if t.material == Some(37) || t.material == Some(65) {
                for p in &mut t.pos {
                    if let Some(z) = surface.settled(*p) {
                        p[2] = z;
                    }
                }
            }
            adjusted.push(t);
        }
        adjusted.extend(surface.triangles.iter().copied());
        collision = adjusted;
    }
    let ground = Grid::new(collision.iter().collect(), 4.);
    let mut missing = 0;
    let mut along = Vec::new();
    for (i, p) in line.points.iter_mut().enumerate() {
        match ground_near(&ground, *p) {
            Some((z, t)) => {
                p[2] = z;
                if let (Some(id), Some(next)) = (t.material, line.at.get(i + 1)) {
                    along.push((id as u8, next - line.at[i]));
                }
            }
            None => missing += 1,
        }
    }
    let physics = crate::btb::surfaces::physics(along);
    if missing > line.points.len() / 100 {
        return Err(format!("{missing}/{} driveline points have no collision within 3 m", line.points.len()));
    }
    // Keep the course's original arc distances: timing gates were projected onto that same polyline.
    options.control.say(&format!("collision heights set; {missing}/{} points without ground", line.points.len()));
    let spawn_position = if input.circuit {
        let (p, tangent) = line.sample(0.);
        let (z, _) = ground_near(&ground, p).ok_or("no road under the lap line")?;
        Spawn { position: [p[0], p[1], z], tangent, source: "lap_line", before_start: 0. }
    } else {
        start_position(input, &line, start, &ground)?
    };
    options.control.say(&format!(
        "spawn from {}, {:.1} m behind the timing gate",
        spawn_position.source, spawn_position.before_start
    ));
    drop(ground);
    let mut col_builders = BTreeMap::<((i64, i64), u8), Builder>::new();
    for t in &collision {
        col_builders.entry((tile(t), t.material.unwrap_or(37) as u8)).or_default().add(t, origin, false);
    }
    let collision_triangles = col_builders.values().map(|b| b.triangles.len()).sum();
    let ids = col_builders.keys().map(|(_, id)| *id).collect::<BTreeSet<_>>();
    let mut maps = String::new();
    for (index, id) in ids.iter().enumerate() {
        let texture = bank.put(crate::btb::textures::flat([*id as f32 / 255.; 4]), "dds")?;
        let name = format!("collision\\surface_{id}");
        material::section(&mut materials, &name, &texture, false);
        names.push(name);
        let _ = writeln!(maps, "[MAP{index}]\ntexture={texture}");
        let weather = if *id == 117 { [*id; 3] } else { crate::btb::surfaces::weathers(*id) };
        for (key, surface) in ["data", "damp", "wet"].into_iter().zip(weather) {
            for row in 0..16 {
                let _ = writeln!(maps, "{key}{row}={}", vec![surface.to_string(); 16].join(" "));
            }
        }
    }
    let batch = col_builders
        .into_iter()
        .map(|((_, id), b)| {
            let entries = "LODIn=0\nVisible=0\nShadowCaster=0\nCollisionModel=0\nCollisionOnly=1\n";
            (b, format!("collision\\surface_{id}"), entries.to_owned())
        })
        .collect();
    objects.meshes(batch)?;
    let texture_count = bank.count;
    let missing_textures = bank.missing.len();
    let simplified = bank.simplified.clone();
    let baked = bank.baked.clone();
    let placeholders = bank.placeholders.clone();
    let missing_names = bank.missing.clone();
    let (normal_mapped, highlighted, sheened, layered) =
        (bank.normal_mapped, bank.highlighted, bank.sheened, bank.layered);
    // CSP's grass atlas, where the track ships its own.
    let grass_atlas = match grass_fx.as_ref().and_then(|g| g.texture.as_ref()) {
        Some(path) => Some(bank.put(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?, "dds")?),
        None => None,
    };
    bank.finish()?;
    let write = |file: &str, text: String| std::fs::write(folder.join(file), text).map_err(|e| format!("{file}: {e}"));
    write("objectlist.ini", objects.finish()?)?;
    let mut list = format!("[MATERIALS]\ncount={}\n", names.len());
    for (i, n) in names.iter().enumerate() {
        let _ = writeln!(list, "MAT_{i}={n}");
    }
    write("materials.ini", format!("{list}\n{}", material::normalise_light(&materials, &light_areas)))?;
    write("mat.ini", format!("[MAPS]\ncount={}\n{maps}", ids.len()))?;
    let title = if input.layout == "." { input.title.clone() } else { format!("{} [{}]", input.title, input.layout) };
    write("track.ini",format!("[INFO]\nname={title}\nphysics={physics}\nauthor={}\nversion=AC local conversion\nlength={:.3}\ncomment=Converted locally by ac-stage\n{}",input.author,(input.finish-input.start)/1000.,if input.circuit{"circuit=1\n"}else{""}))?;
    let mut settings = String::from(SETTINGS_INI);
    if let Some(fx) = &grass_fx {
        settings.push('\n');
        settings.push_str(&fx.section(grass_atlas.as_deref()));
    }
    write("settings.ini", settings)?;
    let mut dl = format!("[DRIVELINE]\ncount={}\n", line.points.len());
    for (i, p) in line.points.iter().enumerate() {
        // A ring's last point repeats its first: neighbours wrap around the seam.
        let n = line.points.len();
        let (a, b) = if input.circuit {
            (line.points[(i + n - 2) % (n - 1)], line.points[(i + 1) % (n - 1)])
        } else {
            (line.points[i.saturating_sub(1)], line.points[(i + 1).min(n - 1)])
        };
        let length = ai::distance(a, b).max(1e-9);
        let t = std::array::from_fn::<_, 3, _>(|k| (b[k] - a[k]) / length);
        let _ = writeln!(
            dl,
            "K{i}={}, {}, {}, {}, {}, {}, {}, 0",
            p[0] - origin[0],
            p[1] - origin[1],
            p[2],
            t[0],
            t[1],
            t[2],
            line.at[i]
        );
    }
    write("driveline.ini", dl)?;
    // The result marker must be within the car's braking run after the finish.
    // A circuit's start, finish and end keys lie far past the lap so the 1.02 timekeeper never fires them.
    let mut notes = if input.circuit {
        vec![(0x15, 0, FAR), (0x16, 0, FAR + 10.), (0x18, 0, FAR + 20.)]
    } else {
        vec![(0x15, 0, start), (0x16, 0, finish), (0x18, 0, (finish + 15.).min(line.length()))]
    };
    notes.extend(input.splits.iter().map(|s| (0x17, 0, *s - from)));
    let called = pacenotes::notes(&line.points, &line.at, start, finish);
    notes.extend(called.iter().map(|n| (n.tag, n.flags, n.distance)));
    notes.sort_by(|a, b| a.2.total_cmp(&b.2));
    let mut pn = format!("[PACENOTES]\ncount={}\n", notes.len());
    for (i, (tag, flag, s)) in notes.iter().enumerate() {
        let _ = writeln!(pn, "\n[P{i}]\ntype={tag}\nflag={flag}\ndistance={s:.4}");
    }
    write("pacenotes.ini", pn)?;
    let Spawn { position: p, tangent, source: spawn_source, before_start: spawn_before_start_m } = spawn_position;
    let mut spawn = String::from("[SPAWN]\n");
    for i in 0..16 {
        let v = match i {
            0 | 5 | 10 => 1.,
            12 => p[0] - origin[0],
            13 => p[2],
            14 => p[1] - origin[1],
            _ => 0.,
        };
        let _ = writeln!(spawn, "m{i:02}={v}");
    }
    let _ = writeln!(spawn, "angleX=0\nangleY=0\nangleZ={}", tangent[0].atan2(tangent[1]));
    write("spawn.ini", spawn)?;
    let mut provenance = serde_json::json!({"source":input.root,"layout":input.layout,"origin":origin,
        "source_triangles":source_triangles,"drawn_triangles":drawn_triangles,"far_triangles":far_triangles,"csp_trees":tree_count,"csp_tree_triangles":tree_triangles,"collision_triangles":collision_triangles,
        "ai_source":input.ai_source,"reversed_ai":input.reversed_ai,"circuit":input.circuit,"ai_extensions_m":input.ai_extensions,
        "ai_trimmed_points":input.ai_trimmed_points,
        "runoff_extension_m":runoff_extension,"runoff_m":line.length()-finish,
        "spawn_source":spawn_source,"spawn_before_start_m":spawn_before_start_m,
        "source_repairs":repairs,
        "physics":physics,"surface_triangles":surface_counts,"declared_surface_keys":surfaces.declared,
        "length_m":input.finish-input.start,"ai_start_m":input.start,"ai_finish_m":input.finish,
        "textures":texture_count,"missing_textures":missing_names,"base_diffuse_only":simplified,"multilayer_baked":baked,"placeholder_textures":placeholders,"normal_mapped_materials":normal_mapped,"highlight_materials":highlighted,
        "multilayer_tile_textures":tile_textures,"remeshed":options.remesh,"turned_wall_faces":turned_walls,"pacenotes":called.len(),"missing_ground_points":missing,"end_after_finish_m":15,
        "limitations":["RBR surface classes approximate AC physics","multilayer shaders are baked when their textures fit 16384 px and useDetail materials with their detail; reflection maps are not drawn; normal maps, the sun highlight and the roads' dry sheen are written for ReRBR's renderer","CSP scripts and runtime model replacements are not executed","CSP trees are drawn as crossed cards of their rendered picture"]});
    provenance["sheen_materials"] = sheened.into();
    provenance["multilayer_per_pixel_materials"] = layered.into();
    write(CONVERSION_FILE, serde_json::to_string_pretty(&provenance).map_err(|e| e.to_string())?)?;
    options.control.say("validating BTB collision cells");
    let stats = crate::btb::stage::check_cells(folder)?;
    let max_cell_vertices = stats.max_vertices;
    options.control.say(&format!(
        "{} cells, largest {max_cell_vertices}/65536 vertices; {texture_count} textures, {missing_textures} missing",
        stats.cells
    ));
    Ok(Report {
        folder: folder.to_owned(),
        length: input.finish - input.start,
        drawn_triangles,
        collision_triangles,
        textures: texture_count,
        missing_textures,
        max_cell_vertices,
    })
}

/// A converted stage's `settings.ini`: the sun, and a mild warm-grey haze (`[ReRbrAir]`, read by `rbr_btb::track::Air`;
/// aerosol 0.2 is the sky model's hazy end, clear AC days stay clear under it) without our dawn and dusk valley mist,
/// which AC does not have.
const SETTINGS_INI: &str =
    "[SETTINGS]\nsunDirection=-0.843359,-0.503774,0.186968\n\n[ReRbrAir]\nfog_color=0.74, 0.68, 0.62\naerosol=0.2\nmist=0\n";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_settings_carry_a_mild_warm_grey_haze() {
        let air = rbr_btb::track::Air::from_ini(SETTINGS_INI).unwrap();
        assert_eq!((air.fog_color, air.aerosol, air.mist), (Some([0.74, 0.68, 0.62]), Some(0.2), Some(0.0)));
    }
    #[test]
    fn a_driveline_point_in_a_small_gap_takes_the_ground_beside_it() {
        let slab = |x0: f64, x1: f64| Triangle {
            pos: [[x0, -5., 2.], [x1, -5., 2.], [x0, 5., 2.]],
            uv: [[0.; 2]; 3],
            normal: [[0., 0., 1.]; 3],
            material: Some(37),
        };
        // Two triangles whose plan views leave a gap of 0.2 m around x = 0, and no ground at all beyond them.
        let (left, right) = (slab(-5., -0.1), slab(0.1, 5.));
        let grid = Grid::new(vec![&left, &right], 4.);
        assert!(grid.ground(0., 0., 2.).is_none());
        assert_eq!(ground_near(&grid, [0., 0., 2.]).map(|g| g.0), Some(2.));
        assert!(ground_near(&grid, [0., 0., 9.]).is_none());
        assert!(ground_near(&grid, [20., 0., 2.]).is_none());
    }
    #[test]
    fn finish_runoff_preserves_timing_and_requires_physical_ground() {
        let road = Triangle {
            pos: [[-10., -10., 0.], [10., -10., 0.], [0., 150., 0.]],
            uv: [[0.; 2]; 3],
            normal: [[0.; 3]; 3],
            material: Some(37),
        };
        let ground = Grid::new(vec![&road], 4.);
        let original = Line::new(vec![[0., 0., 0.], [0., 100., 0.]], vec![6.; 2]);
        let mut line = original.clone();
        assert_eq!(finish_runoff(&mut line, 100., &ground).unwrap(), 30.);
        assert_eq!(line.sample(100.).0, [0., 100., 0.]);
        assert_eq!(&line.at[..2], &original.at);
        assert_eq!(line.points.last().unwrap(), &[0., 130., 0.]);
        assert_eq!(line.points.len(), line.at.len());
        assert_eq!(line.points.len(), line.width.len());
        assert_eq!(finish_runoff(&mut line, 100., &ground).unwrap(), 0.);
        let mut line = original;
        assert!(finish_runoff(&mut line, 100., &Grid::new(vec![], 4.)).is_err());
        assert_eq!(line.length(), 100.);
    }
    #[test]
    fn range_padding_covers_the_box_of_the_meshes_merged_with_it() {
        let lod = (0, 200f32.to_bits());
        let a: Bounds = ([0., 0., 0.], [60., 0., 0.]);
        let b: Bounds = ([60., 0., 0.], [100., 0., 0.]);
        // Same cell and range: the union is 100 m wide, so each is padded by its half (50 m).
        let padding = lod_padding(&[(a, lod), (b, lod)]);
        assert_eq!(padding, vec![50., 50.]);
        // Another range is merged with neither, and a box over 150 m of half extent is drawn alone, unpadded.
        let other = (0, 400f32.to_bits());
        let solo: Bounds = ([-200., 0., 0.], [200., 0., 0.]);
        assert_eq!(lod_padding(&[(a, lod), (b, other), (solo, lod)]), vec![30., 20., 0.]);
        // Height counts: the distance is in three dimensions.
        assert_eq!(lod_padding(&[(([0., 0., 0.], [0., 24., 0.]), lod)]), vec![12.]);
    }
    #[test]
    fn scenery_tiles_follow_rerbrs_cells_from_the_stage_origin() {
        let at = |x: f64| Triangle { pos: [[x, 5., 0.]; 3], uv: [[0.; 2]; 3], normal: [[0.; 3]; 3], material: None };
        assert_eq!(scenery_tile(&at(1000.), [1000., 0.]), (0, 0));
        assert_eq!(scenery_tile(&at(1127.), [1000., 0.]), (0, 0));
        assert_eq!(scenery_tile(&at(1128.), [1000., 0.]), (1, 0));
        assert_eq!(scenery_tile(&at(999.), [1000., 0.]), (-1, 0));
    }
    #[test]
    fn far_scenery_keeps_triangles_large_for_their_distance() {
        let far = Far::new(&Line::new(vec![[0., 0., 0.], [0., 100., 0.]], vec![6.; 2]), 300.);
        let at = |x: f64, side: f64| [[x, 50., 0.], [x + side, 50., 0.], [x, 50. + side, 0.]];
        assert!(far.keeps(&at(1000., 30.)), "a mountain's 30 m triangle 1 km out");
        assert!(!far.keeps(&at(1000., 10.)), "a tree card 1 km out");
        assert!(far.keeps(&at(400., 10.)), "a tree card 400 m out");
        assert!(!far.keeps(&at(20_000., 2000.)), "past ReRBR's far plane");
    }
    #[test]
    fn far_scenery_stays_connected_across_small_triangles() {
        let far = Far::new(&Line::new(vec![[0., 0., 0.], [0., 100., 0.]], vec![6.; 2]), 300.);
        let tri = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| Triangle {
            pos: [a, b, c].map(|p| [1000. + p[0], p[1], 0.]),
            uv: [[0.; 2]; 3],
            normal: [[0.; 3]; 3],
            material: None,
        };
        // A large triangle with a strip of small ones leading away from it, however long, and a small one elsewhere.
        let kept = [tri([0., 0.], [100., 0.], [0., 100.])];
        let strip = (0..20).map(|i| {
            let x = 100. + 5. * f64::from(i);
            tri([x, 0.], [x + 5., 0.], [x, 5.])
        });
        let rest: Vec<Triangle> = strip.chain([tri([500., 500.], [505., 500.], [500., 505.])]).collect();
        assert!(kept.iter().all(|t| far.keeps(&t.pos)) && rest.iter().all(|t| !far.keeps(&t.pos)));
        let mut whole = vec![true; 20];
        whole.push(false);
        assert_eq!(far.connected(&kept, &rest), whole);
        assert_eq!(far.connected(&[], &rest), vec![false; 21]);
    }
    #[test]
    fn corridor_keeps_long_triangles_crossing_the_course() {
        let line = Line::new(vec![[0., 0., 0.], [0., 100., 0.]], vec![6.; 2]);
        let c = Corridor::new(&line, 25., 25.);
        assert!(c.contains(&[[-1000., 40., 0.], [1000., 40., 0.], [1000., 45., 0.]]));
        assert!(!c.contains(&[[1000., 0., 0.], [1001., 0., 0.], [1000., 1., 0.]]));
    }
    #[test]
    fn source_and_destination_names_cannot_escape_their_roots() {
        for value in ["../outside", "/outside", "C:\\outside", "a/../../outside"] {
            assert!(relative(value).is_err());
        }
        assert_eq!(relative("nested\\mesh.kn5").unwrap(), PathBuf::from("nested/mesh.kn5"));
    }
}
