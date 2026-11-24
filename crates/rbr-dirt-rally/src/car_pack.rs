//! A DiRT Rally car as the model folder of an NGP car pack (`Cars/<name>/`): `.sgc`, `.ini`, `_shaders.ini` and
//! textures, so the renderer draws it as any community car.
//!
//! DiRT's meshes replace the template car's: the template (a car folder of the player's, such as an RSF car of the same
//! model) keeps its collision hulls, helper nodes, points, cameras, lamp settings and shader settings, and gives the
//! frame the physics pack was fitted to (the rear axle at the origin, the front at -z, +x the left). The DiRT car's wheel
//! centres are laid on the template's, the body between them follows (the wheelbases differ by centimetres).
use crate::binxml::{self, Element};
use crate::car::{self, Instance, Part};
use crate::mesh::Mat;
use crate::pssg::Pssg;
use crate::texture::{self, Texture};
use rbr_formats::sgc;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// What `convert` is asked for.
pub struct Options {
    /// The car's folder under `cars/models` (`cme`).
    pub car: String,
    /// The generic texture set of its class under `cars/generics` (`historic`, `rally`, `groupb`, `rallyx`).
    pub class: String,
    /// The livery number (`livery_00`).
    pub livery: u32,
    /// The template car's folder (`.../Cars/Escort_mk_2`).
    pub template: PathBuf,
    /// The pack's model folder name (`Cars/<name>/<name>.sgc`).
    pub name: String,
}

/// A mesh of the output before it is written: vertices in the part's own space and the node's place.
struct Piece {
    name: String,
    /// The node's translation in the template's frame.
    at: [f32; 3],
    /// The node's axes in the template's frame (its columns), identity for a part modelled in the car's.
    axes: [[f32; 3]; 3],
    faces: Vec<Face>,
}

/// The car's own axes: a part modelled in the car's frame.
const UPRIGHT: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// The triangles of one shader class and material.
struct Face {
    shader: &'static str,
    material: String,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    triangles: Vec<[u32; 3]>,
}

/// The tyre classes by the suffix of the livery's wheel file and the generic tread file.
const CLASSES: [(&str, &str, &str); 3] =
    [("tarmac", "tm", "Tarmac"), ("gravel", "gr", "Gravel"), ("snow", "sn", "Snow")];
/// DiRT's wheel suffixes and the RBR's, in the wheel order of the renderer.
const SIDES: [(&str, &str); 4] = [("fl", "lf"), ("fr", "rf"), ("bl", "lb"), ("br", "rb")];
/// A part's vertex limit: the triangle indices are 16 bits.
const MAX_VERTICES: usize = 60_000;

/// What a material draws as: the shader class, the material key and the texture it names (`file`, `source`).
struct Style {
    shader: &'static str,
    key: String,
    texture: Source,
    opacity: bool,
}

/// Where a texture comes from.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    /// A texture of the livery's main file, by its name (`cme_main.tga`).
    Livery(String),
    /// A texture of the livery's file for a tyre class (`tm`, `gr`, `sn`).
    LiveryClass(&'static str, String),
    /// A texture of the class's generic file (`car_gen_main_high`, `car_gen_tm_high`).
    Generic(&'static str, String),
}

fn style(material: &str, car: &str) -> Option<Style> {
    let livery = |name: &str| Source::Livery(name.to_owned());
    let flat = |shader, key: &str, texture| Some(Style { shader, key: key.to_owned(), texture, opacity: false });
    if let Some((_, short, _)) = CLASSES.iter().find(|(_, _, title)| material.starts_with(&format!("{title} "))) {
        let class = CLASSES.iter().find(|(_, s, _)| s == short).map(|(c, _, _)| *c)?;
        return if material.ends_with(" Wheel") {
            flat("CarRim", &format!("wheel_{class}"), Source::LiveryClass(short, format!("{car}_wheel.tga")))
        } else {
            flat("CarTire", &format!("tyre_{class}"), Source::Generic(short, "tread.tga".to_owned()))
        };
    }
    match material {
        "Car Body Paint" => flat("CarPaint", "paint", livery(&format!("{car}_main.tga"))),
        "Matt" | "Shiny Metal" => flat("CarTextureNoDirt", "paint", livery(&format!("{car}_main.tga"))),
        "Cabin" => flat("CarPlastic", "cabin", livery(&format!("{car}_cabin.tga"))),
        "Window Glass" => Some(Style {
            shader: "CarGlass",
            key: "glass".to_owned(),
            texture: livery(&format!("{car}_glass.tga")),
            opacity: true,
        }),
        // The lens carries the lamp's colours: its UVs lie on the lights atlas (red and amber at the tail, a clear
        // circle at the head); the atlas's alpha is the lens's own.
        "Lights Glass" => Some(Style {
            shader: "CarGlass",
            key: "lens".to_owned(),
            texture: livery(&format!("{car}_lights.tga")),
            opacity: true,
        }),
        "Lights Pod" | "Lights Satin" => flat("CarTextureNoDirt", "lights", livery(&format!("{car}_lights.tga"))),
        "Discs" => flat("CarTextureNoDirt", "disc", livery("disc.tga")),
        "Suspension" => flat("CarTextureNoDirt", "suspension", Source::Generic("main", "suspension.tga".to_owned())),
        "Grills" => flat("CarTextureNoDirt", "grills", Source::Generic("main", "grills.tga".to_owned())),
        _ => None,
    }
}

/// The file a texture is written as: `dr_<key>.dds`.
fn file(key: &str) -> String {
    format!("dr_{key}.dds")
}

/// Parts not drawn in the first pass.
fn dropped(part: &str) -> bool {
    part.contains("_pos_0")
}

/// The window parts: DiRT's suffixes (`window_fl`) and the RBR's part (`e_window_lf`).
fn window(part: &str) -> Option<String> {
    let side = part.strip_prefix("window_")?;
    let side = SIDES.iter().find(|(d, _)| *d == side).map_or(side, |(_, r)| r);
    (!side.contains('_')).then(|| format!("e_window_{side}"))
}

/// The lamps: DiRT's lens part and the renderer's part name before the side (`e_light_brake_lb`). The combined tail and brake
/// lamp is the brake lamp; the bumper's extra lamps stay in the body.
const LAMPS: [(&str, &str); 3] =
    [("light_glass", "e_light_"), ("light_combi_glass", "e_light_brake_"), ("light_rev_glass", "e_light_reverse_")];

/// A lamp part's switch box and whether it is the lit part, by the names the renderer gives lamps (`e_light_lf_lit`:
/// `light_lf`, lit; `e_light_brake_lb`: `brake_lb`, unlit).
fn lamp(part: &str) -> Option<(String, bool)> {
    let rest = part.strip_prefix("e_light_")?;
    let (rest, lit) = rest.strip_suffix("_lit").map_or((rest, false), |rest| (rest, true));
    let named = rest.starts_with("brake_") || rest.starts_with("reverse_");
    Some((if named { rest.to_owned() } else { format!("light_{rest}") }, lit))
}

/// Whether `part` is the lens of a lamp of [`LAMPS`].
fn lens(part: &str) -> bool {
    LAMPS.iter().any(|(lens, _)| part.strip_prefix(lens).is_some_and(|side| side.starts_with('_') && side.len() == 3))
}

/// A part's matrix in the template's frame: the DiRT frame (x left, y up, z forward, right handed) turned into the
/// model's (z back, left handed) about the wheels' centre, then shifted to the template's wheels.
struct Frame {
    /// DiRT's wheel centres' mean, `z`, and the template's.
    z_dirt: f32,
    z_template: f32,
    y_shift: f32,
}

impl Frame {
    fn point(&self, p: [f32; 3]) -> [f32; 3] {
        [p[0], p[1] + self.y_shift, -(p[2] - self.z_dirt) + self.z_template]
    }

    fn direction(n: [f32; 3]) -> [f32; 3] {
        [n[0], n[1], -n[2]]
    }
}

/// The part's vertices as the template's frame with `at` taken out (for a wheel: its local frame).
fn place(instance: &Instance, matrix: &Mat, frame: &Frame, at: [f32; 3]) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let positions = instance
        .positions
        .iter()
        .map(|&p| {
            let q = frame.point(crate::mesh::point(matrix, p));
            [q[0] - at[0], q[1] - at[1], q[2] - at[2]]
        })
        .collect();
    let normals = instance.normals.iter().map(|&n| Frame::direction(crate::mesh::direction(matrix, n))).collect();
    (positions, normals)
}

/// The instances of `parts` as faces of one piece, grouped by shader class and material key.
fn faces<'a>(
    instances: impl Iterator<Item = (&'a Instance, Mat)>,
    frame: &Frame,
    at: [f32; 3],
    car: &str,
    wanted: impl Fn(&str) -> bool,
) -> Vec<Face> {
    let mut groups: BTreeMap<(&'static str, String), Face> = BTreeMap::new();
    for (instance, matrix) in instances {
        if !wanted(&instance.material) {
            continue;
        }
        let Some(style) = style(&instance.material, car) else { continue };
        let (positions, normals) = place(instance, &matrix, frame, at);
        let face = groups.entry((style.shader, style.key.clone())).or_insert_with(|| Face {
            shader: style.shader,
            material: style.key.clone(),
            positions: Vec::new(),
            normals: Vec::new(),
            uv: Vec::new(),
            triangles: Vec::new(),
        });
        let base = face.positions.len() as u32;
        face.positions.extend(positions);
        face.normals.extend(normals);
        let missing = instance.positions.len() - instance.uv.len().min(instance.positions.len());
        face.uv.extend(instance.uv.iter().copied().chain(std::iter::repeat_n([0.0; 2], missing)));
        // The z mirror turns right-handed counter-clockwise fronts into the renderer's, which wants them the other way.
        face.triangles.extend(instance.triangles.iter().map(|t| [t[0] + base, t[2] + base, t[1] + base]));
    }
    groups.into_values().filter(|f| !f.triangles.is_empty()).collect()
}

/// No transform.
const IDENTITY: Mat = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// Rotation and scale only: a part's matrix without its translation.
fn without_translation(m: &Mat) -> Mat {
    let mut r = *m;
    r[3] = [0.0, 0.0, 0.0, 1.0];
    r
}

/// A part's instances with the part's matrix.
fn all(p: &Part) -> Vec<(&Instance, Mat)> {
    p.instances.iter().map(|i| (i, p.transform)).collect()
}

/// A part's instances with its rotation only, in the part's own frame.
fn local(p: &Part) -> Vec<(&Instance, Mat)> {
    p.instances.iter().map(|i| (i, without_translation(&p.transform))).collect()
}

fn pieces(parts: &[Part], frame: &Frame, car: &str) -> Vec<Piece> {
    let part = |name: &str| parts.iter().find(|p| p.name == name);
    let mut out = Vec::new();

    // Wheels: each class's rim, tyre and the disc in the wheel's own frame, the node at the hub.
    for (dirt, rbr) in SIDES {
        let (Some(wheel), Some(tyre), Some(disc)) =
            (part(&format!("wheel_{dirt}")), part(&format!("tyre_{dirt}")), part(&format!("disc_{dirt}")))
        else {
            continue;
        };
        let hub = frame.point([wheel.transform[3][0], wheel.transform[3][1], wheel.transform[3][2]]);
        for (class, _, title) in CLASSES {
            let instances = [wheel, tyre, disc].into_iter().flat_map(local).collect::<Vec<_>>();
            let wanted = |m: &str| m.starts_with(title) || m == "Discs";
            let faces = faces(
                instances.into_iter(),
                &Frame { z_dirt: 0.0, z_template: 0.0, y_shift: 0.0 },
                [0.0; 3],
                car,
                wanted,
            );
            out.push(Piece { name: format!("e_wheel_{class}_{rbr}"), at: hub, axes: UPRIGHT, faces });
        }
    }

    // Windows.
    for p in parts {
        if let Some(name) = window(&p.name) {
            out.push(Piece {
                name,
                at: [0.0; 3],
                axes: UPRIGHT,
                faces: faces(all(p).into_iter(), frame, [0.0; 3], car, |_| true),
            });
        }
    }

    // The lamps: each lens as an unlit and a lit part, which the lamp's switch box swaps (`ini_file`).
    for (dirt, rbr) in LAMPS {
        for (side, rbr_side) in SIDES {
            let Some(lens) = part(&format!("{dirt}_{side}")) else { continue };
            for suffix in ["", "_lit"] {
                let faces = faces(all(lens).into_iter(), frame, [0.0; 3], car, |_| true);
                out.push(Piece { name: format!("{rbr}{rbr_side}{suffix}"), at: [0.0; 3], axes: UPRIGHT, faces });
            }
        }
    }

    // The interior: the cabin material of every part, seen from outside and from the cockpit.
    let cabin = |p: &Part| p.name.starts_with("main_interior") || p.instances.iter().any(|i| i.material == "Cabin");
    let interior: Vec<_> = parts.iter().filter(|p| cabin(p) && p.name != "steering_wheel").flat_map(all).collect();
    for name in ["e_internal", "i_internal"] {
        let faces = faces(interior.clone().into_iter(), frame, [0.0; 3], car, |m| m == "Cabin");
        out.push(Piece { name: name.to_owned(), at: [0.0; 3], axes: UPRIGHT, faces });
    }

    // The steering wheel, in its own frame: the node's z is the column, toward the driver (the template's convention,
    // which the crew's hands and the wheel's turn use). The vertices are DiRT's with z negated (`faces` through the
    // zero frame), the axes the part's turned the same way: the mirror once, so the triangles are flipped once.
    if let Some(p) = part("steering_wheel") {
        let row = |k: usize| [p.transform[k][0], p.transform[k][1], p.transform[k][2]];
        let (x, y, z) = (Frame::direction(row(0)), Frame::direction(row(1)), Frame::direction(row(2)));
        let axes = [x, y, z.map(|v| -v)];
        let hub = frame.point([p.transform[3][0], p.transform[3][1], p.transform[3][2]]);
        let local = Frame { z_dirt: 0.0, z_template: 0.0, y_shift: 0.0 };
        let instances: Vec<_> = p.instances.iter().map(|i| (i, IDENTITY)).collect();
        for name in ["i_steeringwheel", "e_steeringwheel"] {
            let faces = faces(instances.clone().into_iter(), &local, [0.0; 3], car, |_| true);
            out.push(Piece { name: name.to_owned(), at: hub, axes, faces });
        }
    }

    // Everything else outside: the body.
    let is_wheel = |n: &str| n.starts_with("wheel_") || n.starts_with("tyre_") || n.starts_with("disc_");
    let body: Vec<_> = parts
        .iter()
        .filter(|p| {
            !is_wheel(&p.name)
                && window(&p.name).is_none()
                && !lens(&p.name)
                && !dropped(&p.name)
                && p.name != "steering_wheel"
        })
        .flat_map(all)
        .collect();
    let mut faces = faces(body.into_iter(), frame, [0.0; 3], car, |m| m != "Cabin" && m != "Window Glass");
    // Split the body when it holds more vertices than a part can index.
    let mut n = 0;
    while !faces.is_empty() {
        let mut take = Vec::new();
        let mut vertices = 0;
        while let Some(face) = faces.first() {
            if !take.is_empty() && vertices + face.positions.len() > MAX_VERTICES {
                break;
            }
            vertices += face.positions.len();
            take.push(faces.remove(0));
        }
        n += 1;
        let name = if n == 1 { "e_body".to_owned() } else { format!("e_body{n}") };
        out.push(Piece { name, at: [0.0; 3], axes: UPRIGHT, faces: take });
    }
    out
}

fn le_f32(out: &mut Vec<u8>, values: &[f32]) {
    out.extend(values.iter().flat_map(|v| v.to_le_bytes()));
}

fn cstr(out: &mut Vec<u8>, s: &str) {
    out.extend(s.as_bytes());
    out.push(0);
}

/// A kind-0 subtype-1 record body: node header, bounds, streams, triangles and faces, no morph lists.
fn mesh_body(piece: &Piece, id: i32, parent: i32) -> Result<Vec<u8>, String> {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uv: Vec<[f32; 2]> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut faces: Vec<(&Face, u32, u32)> = Vec::new();
    for face in &piece.faces {
        let base = positions.len() as u32;
        let first = triangles.len() as u32;
        positions.extend(&face.positions);
        normals.extend(&face.normals);
        uv.extend(&face.uv);
        triangles.extend(face.triangles.iter().map(|t| t.map(|i| i + base)));
        faces.push((face, base, first));
    }
    if positions.len() > 65_535 {
        return Err(format!("{}: {} vertices", piece.name, positions.len()));
    }
    let (lo, hi) = positions.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
        (std::array::from_fn(|k| lo[k].min(p[k])), std::array::from_fn(|k| hi[k].max(p[k])))
    });
    let (centre, half): ([f32; 3], [f32; 3]) =
        (std::array::from_fn(|k| (lo[k] + hi[k]) / 2.0), std::array::from_fn(|k| (hi[k] - lo[k]) / 2.0));

    let mut b = Vec::new();
    cstr(&mut b, &piece.name);
    b.extend(id.to_le_bytes());
    b.extend(parent.to_le_bytes());
    le_f32(&mut b, &[0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0]);
    let (a, t) = (piece.axes, piece.at);
    le_f32(
        &mut b,
        &[a[0][0], a[1][0], a[2][0], t[0], a[0][1], a[1][1], a[2][1], t[1], a[0][2], a[1][2], a[2][2], t[2]],
    );
    le_f32(&mut b, &centre);
    le_f32(&mut b, &half);
    b.extend((positions.len() as i32).to_le_bytes());
    b.extend(7u32.to_le_bytes());
    for (p, n) in positions.iter().zip(&normals) {
        le_f32(&mut b, p);
        le_f32(&mut b, n);
    }
    for t in &uv {
        le_f32(&mut b, t);
    }
    b.extend((triangles.len() as i32).to_le_bytes());
    for t in &triangles {
        b.extend(t.iter().flat_map(|&i| (i as u16).to_le_bytes()));
    }
    b.extend((faces.len() as i32).to_le_bytes());
    for (face, base, first) in faces {
        cstr(&mut b, &material_name(&face.material));
        let used = face.positions.len() as u32;
        for word in [face.triangles.len() as u32, base, used, first * 3] {
            b.extend(word.to_le_bytes());
        }
    }
    b.extend(0u32.to_le_bytes());
    Ok(b)
}

/// The material's name as the model and the ini write it.
fn material_name(key: &str) -> String {
    format!("dr::{key}")
}

fn record(out: &mut Vec<u8>, kind: u32, subtype: u32, body: &[u8]) {
    for word in [12, kind, subtype, 1, body.len() as u32] {
        out.extend(word.to_le_bytes());
    }
    out.extend(body);
}

/// The nodes the crew sits and reaches by: the seats, the shifter and the handbrake (`rbr-render`'s `characters/incar`).
const CREW: [&str; 6] = [
    "D_driver_use",
    "D_codriver_use",
    "D_hand R_gear",
    "D_hand R_gear p",
    "D_hand R_handbrake",
    "D_hand R_handbrake p",
];

/// A helper node's `body` with its place mirrored across the car's middle: the translation's x and the world matrix's.
fn mirrored(body: &[u8]) -> Option<Vec<u8>> {
    let name = body.iter().position(|&b| b == 0)?;
    // The name, the id and the parent, then 14 words of transform and the 3x4 world matrix, row by row.
    let transform = name + 1 + 8;
    let mut out = body.to_vec();
    for at in [transform, transform + 14 * 4 + 3 * 4] {
        let x = f32::from_le_bytes(out.get(at..at + 4)?.try_into().ok()?);
        out[at..at + 4].copy_from_slice(&(-x).to_le_bytes());
    }
    Some(out)
}

/// The template's records but its meshes, then ours. The crew's nodes follow the steering wheel to its side of the car.
fn sgc_file(template: &[u8], pieces: &[Piece]) -> Result<Vec<u8>, String> {
    let mut out = sgc::BANNER.to_vec();
    let elements = sgc::parse(template).map_err(|e| format!("template model: {e}"))?;
    let root = elements
        .iter()
        .find_map(|e| match e {
            sgc::Element::Node(n) if n.parent == -1 => Some(n.id),
            _ => None,
        })
        .ok_or("the template model has no root node")?;
    let seat = elements.iter().find_map(|e| match e {
        sgc::Element::Node(n) if n.name == "D_driver_use" => Some(n.world[0][3]),
        _ => None,
    });
    let hub = pieces.iter().find(|p| p.name == "i_steeringwheel").map(|p| p.at[0]);
    let flip = seat.zip(hub).is_some_and(|(seat, hub)| seat * hub < 0.0);
    for rec in sgc::records(template).map_err(|e| format!("template model: {e}"))? {
        if rec.kind == sgc::KIND_GEOMETRY {
            continue;
        }
        let named = |name: &str| rec.body.strip_prefix(name.as_bytes()).is_some_and(|rest| rest.first() == Some(&0));
        let crew = flip && rec.kind == sgc::KIND_HELPER && CREW.iter().any(|n| named(n));
        let body = if crew { mirrored(rec.body) } else { None };
        record(&mut out, rec.kind, rec.subtype, body.as_deref().unwrap_or(rec.body));
    }
    for (i, piece) in pieces.iter().enumerate() {
        record(&mut out, sgc::KIND_GEOMETRY, sgc::SUBTYPE_STATIC, &mesh_body(piece, 1000 + i as i32, root)?);
    }
    Ok(out)
}

/// The template's camera sections that DiRT's views replace: the section, DiRT's view and the parameter holding its place.
const CAMERAS: [(&str, &str, &str); 3] =
    [("Cam_internal", "head-cam", "position"), ("Cam_bonnet", "bumper", "offset"), ("Cam_bonnet2", "bonnet", "offset")];

/// The view `ident` under `e`.
fn view<'a>(e: &'a Element, ident: &str) -> Option<&'a Element> {
    if e.name == "View" && e.attr("ident") == Some(ident) {
        return Some(e);
    }
    e.children.iter().find_map(|c| view(c, ident))
}

/// Where DiRT's views put the eye, in the template's frame, by the template's camera section.
fn eyes(cameras: &Element, frame: &Frame) -> BTreeMap<&'static str, [f32; 3]> {
    let mut out = BTreeMap::new();
    for (section, ident, parameter) in CAMERAS {
        let place = view(cameras, ident)
            .and_then(|v| v.children_named("Parameter").find(|p| p.attr("name") == Some(parameter)))
            .and_then(|p| Some([p.attr_num("x")? as f32, p.attr_num("y")? as f32, p.attr_num("z")? as f32]));
        if let Some(place) = place {
            out.insert(section, frame.point(place));
        }
    }
    out
}

/// The part sections of the output ini and, before them, the template's cameras, points and planes, the cameras
/// moved to `eyes` (the look keeps its direction and its length).
fn ini_file(template: &str, pieces: &[Piece], eyes: &BTreeMap<&str, [f32; 3]>) -> String {
    let mut out = String::new();
    let keep = |name: &str| ["cam_", "point_", "plane_"].iter().any(|p| name.to_ascii_lowercase().starts_with(p));
    let value = |section: &str, key: &str| {
        let mut inside = false;
        template.lines().find_map(|line| {
            let line = line.trim();
            match line.strip_prefix('[').and_then(|l| l.split(']').next()) {
                Some(name) => inside = name.eq_ignore_ascii_case(section),
                None if inside => {
                    let (k, v) = line.split_once('=')?;
                    return k.trim().eq_ignore_ascii_case(key).then(|| binxml::numbers::<3>(v)).flatten();
                }
                None => {}
            }
            None
        })
    };
    let (mut kept, mut section) = (false, String::new());
    for line in template.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(name) = line.trim().strip_prefix('[').and_then(|l| l.split(']').next()) {
            kept = keep(name);
            section = name.to_owned();
        }
        if !kept {
            continue;
        }
        let key = line.split_once('=').map(|(k, _)| k.trim().to_ascii_lowercase());
        let eye = eyes.iter().find(|(s, _)| s.eq_ignore_ascii_case(&section)).map(|(_, e)| *e);
        let moved = match (eye, key.as_deref()) {
            (Some(eye), Some("pos")) => Some(eye),
            (Some(eye), Some("target")) => value(&section, "Pos")
                .zip(value(&section, "Target"))
                .map(|(pos, target)| std::array::from_fn(|k| eye[k] + target[k] - pos[k])),
            _ => None,
        };
        match (moved, line.split_once('=')) {
            (Some(v), Some((k, _))) => out += &format!("{}= {:.6} {:.6} {:.6}\n", k, v[0], v[1], v[2]),
            _ => {
                out += line;
                out.push('\n');
            }
        }
    }
    for piece in pieces {
        out += &match lamp(&piece.name) {
            Some((switch, lit)) => {
                format!(
                    "\n[{}]\nSwitch = true\nSwitchBox = {switch}\nPredicate = {}\n",
                    piece.name,
                    ["off", "on"][lit as usize]
                )
            }
            None => format!("\n[{}]\nSwitch = false\nSwitchBox = light\nPredicate = NotUsed\n", piece.name),
        };
        for (i, face) in piece.faces.iter().enumerate() {
            out += &format!(
                "FaceBlock{i}_Shader = {}\nFaceBlock{i}_Material = {}\n",
                face.shader,
                material_name(&face.material)
            );
        }
    }
    out
}

/// `<name>_shaders.ini`: every material of the pieces with its texture.
fn shaders_file(name: &str, materials: &BTreeMap<String, (bool, String)>) -> String {
    let mut out = String::from(
        "[Default Material]\nAmbient=0.500000 0.500000 0.500000\nDiffuse=0.500000 0.500000 0.500000\n\
         Specular=0.500000 0.500000 0.500000\nShininess=0.000000\nShineStrenth=0.000000\nTransparency=0.000000\n\
         Requirements=0\nDiffuseChannel=false\nOpacityChannel=false\nReflectionChannel=false\n\
         BumpChannel=false\nSelfIllumunationChannel=false\nSpecularColorChannel=false\n\n[TSDefault]\n",
    );
    let files: std::collections::BTreeSet<_> = materials.values().map(|(_, f)| f.clone()).collect();
    for f in &files {
        out += &format!("{f} = cars\\{name}\\textures\\{f}\n");
    }
    out += "\n[TextureSets]\nTextureSet0 = TSDefault\n\n[Textures]\n";
    for (i, f) in files.iter().enumerate() {
        out += &format!("Texture{i} = {f}\n");
    }
    for (key, (opacity, f)) in materials {
        let m = material_name(key);
        let bitmap = |kind: &str, mono: bool| {
            format!(
                "\n[{m}_{kind}_bitmap]\nFilename={f}\nAmount=1.000000\nAlphaAsMono={mono}\nAlphaAsRGB=false\n\
                 TileU=1.000000\nTileV=1.000000\nOffsetU=0.000000\nOffsetV=0.000000\nWrapU=3\nWrapV=3\n"
            )
        };
        out += &bitmap("diffuse", false);
        if *opacity {
            out += &bitmap("opacity", true);
        }
        out += &format!(
            "\n[{m}]\nAmbient=0.500000 0.500000 0.500000\nDiffuse=0.500000 0.500000 0.500000\n\
             Specular=0.900000 0.900000 0.900000\nShininess=0.100000\nShineStrength=0.300000\nTransparency=0.000000\n\
             Requirements=33808\nDiffuseChannel=true\nDiffuseType=Bitmap\nOpacityChannel={opacity}\n{}\
             ReflectionChannel=false\nSelfIlluminationChannel=false\nBumpChannel=false\nSpecularColorChannel=false\n",
            if *opacity { "OpacityType=Bitmap\n" } else { "" },
        );
    }
    out
}

/// The texture `source` of the DiRT install as a `.dds`, from the files of `car_dir`.
struct Library {
    car_dir: PathBuf,
    generics: PathBuf,
    class: String,
    livery: u32,
}

impl Library {
    fn find(&self, source: &Source) -> Result<Vec<u8>, String> {
        let (path, name) = match source {
            Source::Livery(name) => (self.livery_file(""), name),
            Source::LiveryClass(short, name) => (self.livery_file(&format!("_{short}")), name),
            Source::Generic(short, name) => {
                (self.generics.join(&self.class).join(format!("car_gen_{short}_high.pssg")), name)
            }
        };
        let pssg = Pssg::read(&path)?;
        let textures = texture::read_car(&pssg);
        let found: Option<&Texture> = textures.iter().find(|t| t.name.eq_ignore_ascii_case(name));
        found.map(Texture::dds).ok_or_else(|| format!("{}: no texture {name}", path.display()))
    }

    /// `livery_NN/textures_high/<car>_tex<variant>_high_NN.pssg`.
    fn livery_file(&self, variant: &str) -> PathBuf {
        let car = self.car_dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let n = self.livery;
        self.car_dir.join(format!("livery_{n:02}/textures_high/{car}_tex{variant}_high_{n:02}.pssg"))
    }
}

/// Converts the car and writes `<out>/Cars/<name>/`; returns the model's `.sgc` path.
pub fn convert(data: &Path, options: &Options, out: &Path) -> Result<PathBuf, String> {
    let car_dir = data.join("cars/models").join(&options.car);
    let pssg = Pssg::read(&car_dir.join(format!("{}_highlod.pssg", options.car)))?;
    let parts = car::parts(&pssg)?;

    let template_name = options.template.file_name().and_then(|n| n.to_str()).ok_or("template folder name")?;
    let template_sgc = fs::read(options.template.join(format!("{template_name}.sgc"))).map_err(|e| e.to_string())?;
    let model = sgc::parse(&template_sgc).map_err(|e| format!("template model: {e}"))?;
    let worlds = sgc::chained_worlds(&model);
    let hub = |side: &str| -> Result<[f32; 3], String> {
        model
            .iter()
            .find_map(|e| match e {
                sgc::Element::Mesh(m) if m.node.name == format!("e_wheel_tarmac_{side}") => {
                    let w = worlds[&m.node.id];
                    Some([w[0][3], w[1][3], w[2][3]])
                }
                _ => None,
            })
            .ok_or_else(|| format!("the template has no e_wheel_tarmac_{side}"))
    };
    let template_hubs = SIDES.map(|(_, rbr)| hub(rbr)).into_iter().collect::<Result<Vec<_>, _>>()?;
    let dirt_hubs: Vec<[f32; 3]> = SIDES
        .iter()
        .map(|(dirt, _)| {
            parts
                .iter()
                .find(|p| p.name == format!("wheel_{dirt}"))
                .map(|p| [p.transform[3][0], p.transform[3][1], p.transform[3][2]])
                .ok_or_else(|| format!("no wheel_{dirt}"))
        })
        .collect::<Result<_, _>>()?;
    let mean = |hubs: &[[f32; 3]], axis: usize| hubs.iter().map(|h| h[axis]).sum::<f32>() / hubs.len() as f32;
    let frame = Frame {
        z_dirt: mean(&dirt_hubs, 2),
        z_template: mean(&template_hubs, 2),
        y_shift: mean(&template_hubs, 1) - mean(&dirt_hubs, 1),
    };

    let pieces = pieces(&parts, &frame, &options.car);

    let folder = out.join("Cars").join(&options.name);
    let textures = folder.join("textures");
    fs::create_dir_all(&textures).map_err(|e| e.to_string())?;
    let mut materials: BTreeMap<String, (bool, String)> = BTreeMap::new();
    let mut sources: BTreeMap<String, Source> = BTreeMap::new();
    for part in &parts {
        for instance in &part.instances {
            if let Some(s) = style(&instance.material, &options.car) {
                materials.insert(s.key.clone(), (s.opacity, file(&s.key)));
                sources.insert(s.key, s.texture);
            }
        }
    }
    let used: std::collections::BTreeSet<&str> =
        pieces.iter().flat_map(|p| p.faces.iter().map(|f| f.material.as_str())).collect();
    materials.retain(|k, _| used.contains(k.as_str()));
    let library =
        Library { car_dir, generics: data.join("cars/generics"), class: options.class.clone(), livery: options.livery };
    for key in materials.keys() {
        let dds = library.find(&sources[key])?;
        fs::write(textures.join(file(key)), dds).map_err(|e| e.to_string())?;
    }

    let name = &options.name;
    let model_path = folder.join(format!("{name}.sgc"));
    fs::write(&model_path, sgc_file(&template_sgc, &pieces)?).map_err(|e| e.to_string())?;
    let template_ini = fs::read_to_string(options.template.join(format!("{template_name}.ini")))
        .or_else(|_| {
            fs::read(options.template.join(format!("{template_name}.ini")))
                .map(|b| String::from_utf8_lossy(&b).into_owned())
        })
        .map_err(|e| e.to_string())?;
    let cameras = binxml::parse(&fs::read(library.car_dir.join("cameras.xml")).map_err(|e| e.to_string())?)?;
    fs::write(folder.join(format!("{name}.ini")), ini_file(&template_ini, &pieces, &eyes(&cameras, &frame)))
        .map_err(|e| e.to_string())?;
    fs::write(folder.join(format!("{name}_shaders.ini")), shaders_file(name, &materials)).map_err(|e| e.to_string())?;
    for suffix in ["_settings.ini", "_shader_settings_clear.ini", "_shader_settings_overcast.ini"] {
        let from = options.template.join(format!("{template_name}{suffix}"));
        fs::copy(&from, folder.join(format!("{name}{suffix}"))).map_err(|e| format!("{}: {e}", from.display()))?;
    }
    Ok(model_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle(material: &str) -> Face {
        Face {
            shader: "CarPaint",
            material: material.to_owned(),
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            uv: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            triangles: vec![[0, 1, 2]],
        }
    }

    #[test]
    fn a_written_mesh_reads_back_with_its_faces() {
        let piece = Piece {
            name: "e_body".into(),
            at: [0.5, 0.25, -1.0],
            axes: UPRIGHT,
            faces: vec![triangle("paint"), triangle("glass")],
        };
        let mut file = sgc::BANNER.to_vec();
        record(&mut file, sgc::KIND_GEOMETRY, sgc::SUBTYPE_STATIC, &mesh_body(&piece, 7, -1).unwrap());
        let elements = sgc::parse(&file).unwrap();
        let [sgc::Element::Mesh(mesh)] = elements.as_slice() else { panic!("one mesh") };
        assert_eq!(
            (mesh.node.name.as_str(), mesh.node.id, mesh.vertices.len(), mesh.triangles.len()),
            ("e_body", 7, 6, 2)
        );
        assert_eq!(mesh.node.world[1][3], 0.25);
        let faces: Vec<_> =
            mesh.faces.iter().map(|f| (f.name.as_str(), f.prim_count, f.min_vertex, f.start_index)).collect();
        assert_eq!(faces, [("dr::paint", 1, 0, 0), ("dr::glass", 1, 3, 3)]);
        assert_eq!(mesh.triangles[1], [3, 4, 5]);
    }

    #[test]
    fn a_helper_node_mirrors_across_the_middle() {
        let mut body = b"D_driver_use\0".to_vec();
        let words = [0.3, 0.1, -1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0]
            .into_iter()
            .chain([1.0, 0.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.1, 0.0, 0.0, 1.0, -1.0]);
        body.extend([98i32, 1].iter().flat_map(|v| v.to_le_bytes()));
        body.extend(words.flat_map(f32::to_le_bytes));
        let mut file = sgc::BANNER.to_vec();
        record(&mut file, sgc::KIND_HELPER, 13, &mirrored(&body).unwrap());
        let elements = sgc::parse(&file).unwrap();
        let [sgc::Element::Node(node)] = elements.as_slice() else { panic!("one node") };
        assert_eq!(
            (node.transform.words()[..3].to_vec(), node.world[0][3], node.world[1][3]),
            (vec![-0.3, 0.1, -1.0], -0.3, 0.1)
        );
    }

    #[test]
    fn materials_map_to_the_renderers_classes() {
        let class = |m: &str| style(m, "cme").map(|s| (s.shader, s.key));
        assert_eq!(class("Car Body Paint"), Some(("CarPaint", "paint".to_owned())));
        assert_eq!(class("Gravel Wheel"), Some(("CarRim", "wheel_gravel".to_owned())));
        assert_eq!(class("Snow Tyre Tread"), Some(("CarTire", "tyre_snow".to_owned())));
        assert_eq!(class("Nothing"), None);
        assert_eq!(window("window_fl").as_deref(), Some("e_window_lf"));
        assert_eq!(window("window_f_trim"), None);
    }

    #[test]
    fn lamp_parts_name_their_switch_boxes() {
        assert_eq!(lamp("e_light_lf"), Some(("light_lf".to_owned(), false)));
        assert_eq!(lamp("e_light_brake_lb_lit"), Some(("brake_lb".to_owned(), true)));
        assert_eq!(lamp("e_light_reverse_rb"), Some(("reverse_rb".to_owned(), false)));
        assert_eq!(lamp("e_body"), None);
        assert!(["light_glass_fl", "light_combi_glass_br", "light_rev_glass_bl"].iter().all(|p| lens(p)));
        assert!(!["light_rig_glass_fl", "light_combi_pod_bl", "light_glass_fl_x"].iter().any(|p| lens(p)));
    }
}
