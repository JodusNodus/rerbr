//! A DiRT Rally car as the model folder of an NGP car pack (`Cars/<name>/`): `.sgc`, `.ini`, `_shaders.ini` and
//! textures, so the renderer draws it as any community car.
//!
//! DiRT's meshes replace the template car's: the template (a car folder of the player's, such as an RSF car of the same
//! model) keeps its collision hulls, helper nodes, points, cameras, lamp settings and shader settings, and gives the
//! frame the physics pack was fitted to (the rear axle at the origin, the front at -z, +x the left). The DiRT car's wheel
//! centres are laid on the template's, the body between them follows (the wheelbases differ by centimetres).
use crate::binxml::{self, Element};
use crate::car::{self, Instance, Part};
use crate::car_maps::{self, Plane};
use crate::gauges;
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

/// What a material draws as: the shader class, the material key and the texture it names (`file`, `source`), and its
/// other maps.
#[derive(Clone)]
struct Style {
    shader: &'static str,
    key: String,
    texture: Source,
    opacity: bool,
    maps: Maps,
}

/// A material's other maps, on its texture's own UVs (`car_maps`).
#[derive(Clone, Default)]
struct Maps {
    normal: Option<Source>,
    /// The normal map's panels carry a damage crumple, which the car does not take undamaged (`car_maps::normal`).
    crumpled: bool,
    /// The map and channel of the baked occlusion, and the material's `OcclusionMapScale`.
    occlusion: Option<(Source, usize, f32)>,
    /// The map and channel of the specular, and the specular of the finish that reflects in full.
    specular: Option<(Source, usize, f32)>,
}

/// The specular of the Escort's red paint in `cme_specocc` (the median of its red texels, 0.43): the paint reflects in
/// full, the dull plastics and rubber less.
const PAINT_SPECULAR: f32 = 0.43;

/// The occlusion the interior's main atlas takes. The game's `OcclusionMapScale` is 1 there, on the ambient light alone
/// (which the dash takes from the sky it sees, `rbr-render`'s cabin light), so the bake takes a part of it.
const INTERIOR_OCCLUSION: f32 = 0.5;

/// Where a texture comes from.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    /// A texture of the livery's main file, by its name (`cme_main.tga`).
    Livery(String),
    /// A texture of the livery's file for a tyre class (`tm`, `gr`, `sn`).
    LiveryClass(&'static str, String),
    /// A texture of the class's generic file (`car_gen_main_high`, `car_gen_tm_high`).
    Generic(&'static str, String),
    /// A texture of the car's interior file (`cars/interiors/models/cme/int_cme.pssg`).
    Interior(String),
    /// The gear display's base, black: the gauges draw the digit on it (`gauges::gear_base`).
    Gear,
}

fn style(material: &str, car: &str) -> Option<Style> {
    let livery = |name: &str| Source::Livery(name.to_owned());
    let flat = |shader, key: &str, texture| {
        Some(Style { shader, key: key.to_owned(), texture, opacity: false, maps: Maps::default() })
    };
    let with = |style: Option<Style>, maps: Maps| style.map(|style| Style { maps, ..style });
    // The body's maps share the livery's UVs: the normal map, and `cme_specocc` (specular in the colour, occlusion in
    // the alpha), the occlusion in the strength the material gives it.
    let body = |occlusion: f32| Maps {
        normal: Some(livery(&format!("{car}_main_nm.tga"))),
        crumpled: true,
        occlusion: Some((livery(&format!("{car}_specocc.tga")), 3, occlusion)),
        specular: Some((livery(&format!("{car}_specocc.tga")), 1, PAINT_SPECULAR)),
    };
    let normal = |source: Source| Maps { normal: Some(source), ..Maps::default() };
    if let Some((_, short, _)) = CLASSES.iter().find(|(_, _, title)| material.starts_with(&format!("{title} "))) {
        let class = CLASSES.iter().find(|(_, s, _)| s == short).map(|(c, _, _)| *c)?;
        // The wheel's atlas holds the rim and the tyre's sidewall (the wall's UVs span all of it), the tread has its own
        // tiled texture.
        let atlas = |key: &str, shader| {
            let flat = flat(shader, &format!("{key}_{class}"), Source::LiveryClass(short, format!("{car}_wheel.tga")));
            with(flat, normal(Source::LiveryClass(short, format!("{car}_wheel_nm.tga"))))
        };
        return if material.ends_with(" Wheel") {
            atlas("wheel", "CarRim")
        } else if material.ends_with(" Tyre Wall") {
            atlas("wall", "CarTire")
        } else {
            let tyre = flat("CarTire", &format!("tyre_{class}"), Source::Generic(short, "tread.tga".to_owned()));
            with(tyre, normal(Source::Generic(short, "tread_nm.tga".to_owned())))
        };
    }
    match material {
        // The three finishes share the livery's atlas and take its occlusion in their own strength (`OcclusionMapScale`).
        "Car Body Paint" => with(flat("CarPaint", "paint", livery(&format!("{car}_main.tga"))), body(0.4)),
        "Matt" => with(flat("CarTextureNoDirt", "matt", livery(&format!("{car}_main.tga"))), body(0.8)),
        "Shiny Metal" => with(flat("CarTextureNoDirt", "metal", livery(&format!("{car}_main.tga"))), body(0.5)),
        "Cabin" => flat("CarTextureNoDirt", "cabin", livery(&format!("{car}_cabin.tga"))),
        "Window Glass" => Some(Style {
            shader: "CarGlass",
            key: "glass".to_owned(),
            texture: livery(&format!("{car}_glass.tga")),
            opacity: true,
            maps: Maps::default(),
        }),
        // The lens carries the lamp's colours: its UVs lie on the lights atlas (red and amber at the tail, a clear
        // circle at the head); the atlas's alpha is the lens's own.
        "Lights Glass" => Some(Style {
            shader: "CarGlass",
            key: "lens".to_owned(),
            texture: livery(&format!("{car}_lights.tga")),
            opacity: true,
            maps: normal(livery(&format!("{car}_lights_nm.tga"))),
        }),
        // The housings' specular is in `<car>_lights_spec_occ` (their occlusion is left out: the lit lamps' colours are the texture's).
        "Lights Pod" | "Lights Satin" => with(
            flat("CarTextureNoDirt", "lights", livery(&format!("{car}_lights.tga"))),
            Maps {
                specular: Some((livery(&format!("{car}_lights_spec_occ.tga")), 1, PAINT_SPECULAR)),
                ..normal(livery(&format!("{car}_lights_nm.tga")))
            },
        ),
        "Discs" => with(flat("CarTextureNoDirt", "disc", livery("disc.tga")), normal(livery("disc_nm.tga"))),
        "Suspension" => with(
            flat("CarTextureNoDirt", "suspension", Source::Generic("main", "suspension.tga".to_owned())),
            normal(Source::Generic("main", "suspension_nm.tga".to_owned())),
        ),
        "Grills" => with(
            flat("CarTextureNoDirt", "grills", Source::Generic("main", "grills.tga".to_owned())),
            normal(Source::Generic("main", "grills_nm.tga".to_owned())),
        ),
        _ => None,
    }
}

/// What a material of the interior scene draws as. The dash, doors, seats, cage, levers and wheel all read one atlas
/// (`int_cme_main`), whatever their shader; the gauge faces and the indicator lights have theirs. The carbon fibre's
/// UVs lie on the atlas's lightmap layout, not on a picture of carbon: it draws the weave (`int_carbon_diff`) repeated
/// [`CARBON_REPEAT`] times over them, as DiRT's shader does. The gear display (`lcd_text`, a texture of numerals the
/// game scrolls) is a black texture of its own which the gauges draw the digit on, for a car whose gauges are measured
/// ([`gauges::layout`]); the speed display is drawn on the dials atlas. Not drawn: the glass. The mirrors' glass (`mirror`, which the game draws as a texture of what
/// is behind) is the renderer's glass class on a flat texel: it shows the surroundings from the part's name
/// (`i_mirrorglass_*`, `rbr-render`'s `mirrors`).
fn interior_style(material: &str, car: &str) -> Option<Style> {
    let style = |key: &str, texture: String| {
        Some(Style {
            shader: "CarTextureNoDirt",
            key: key.to_owned(),
            texture: Source::Interior(texture),
            opacity: false,
            maps: Maps::default(),
        })
    };
    // The main atlas's maps (`int_<car>_nm`, `_occ`, `_sp`) share its UVs.
    let atlas = |name: &str| Source::Interior(format!("int_{car}_{name}.tga"));
    match material {
        "Matt" | "Leather" | "Shiny Painted Metal" | "bare metal" | "Chrome" | "shiny plastic" | "suede" => {
            style("int_main", format!("int_{car}_main.tga")).map(|style| Style {
                maps: Maps {
                    normal: Some(atlas("nm")),
                    crumpled: false,
                    occlusion: Some((atlas("occ"), 0, INTERIOR_OCCLUSION)),
                    specular: Some((atlas("sp"), 0, PAINT_SPECULAR)),
                },
                ..style
            })
        }
        "carbon fibre" => style("int_carbon", "int_carbon_diff.tga".to_owned()),
        "dials matt" | "dials shiny" => style("int_dials", format!("int_{car}_dials.tga")),
        "shift light_01" | "limit lights" => style("int_indicator", "int_indicator.tga".to_owned()),
        "gear indicator" if gauges::layout(car).is_some() => Some(Style {
            shader: "CarTextureNoDirt",
            key: "int_gear".to_owned(),
            texture: Source::Gear,
            opacity: false,
            maps: Maps::default(),
        }),
        "mirror" => Some(Style {
            shader: "CarGlass",
            key: "int_mirror".to_owned(),
            texture: Source::Interior("int_gen_sp.tga".to_owned()),
            opacity: false,
            maps: Maps::default(),
        }),
        _ => None,
    }
}

/// The weave's repeats over the UV range (DiRT's `CarbonUVRepeat`).
const CARBON_REPEAT: f32 = 500.0;

/// The file a texture is written as: `dr_<key>.dds`.
fn file(key: &str) -> String {
    format!("dr_{key}.dds")
}

/// The file a normal map is written as: `dr_<key>_nm.dds`.
fn normal_file(key: &str) -> String {
    format!("dr_{key}_nm.dds")
}

/// A material of the car's `_shaders.ini`: its texture file, whether it has an opacity channel, its normal map's file.
struct Entry {
    opacity: bool,
    file: String,
    normal: Option<String>,
}

/// `texture` toned by `tone`, as the `.dds` the car reads: with the occlusion baked in and the specular as its alpha
/// where `maps` has them, else the blocks as they are.
fn bake(library: &Library, maps: &Maps, texture: &Texture, tone: fn(f32) -> f32) -> Result<Vec<u8>, String> {
    if maps.occlusion.is_none() && maps.specular.is_none() {
        return Ok(texture.toned(tone).dds());
    }
    let plane = |(source, channel, value): &(Source, usize, f32)| {
        Ok::<_, String>((Plane::of(&library.find(source)?, *channel), *value))
    };
    let occlusion = maps.occlusion.as_ref().map(plane).transpose()?;
    let specular = maps.specular.as_ref().map(plane).transpose()?;
    Ok(car_maps::diffuse(
        texture,
        tone,
        occlusion.as_ref().map(|(plane, scale)| (plane, *scale)),
        specular.as_ref().map(|(plane, reference)| (plane, *reference)),
    ))
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

/// The lamps: DiRT's lens part, the renderer's part name before the side (`e_light_brake_lb`) and the states it draws
/// (`""` unlit, `_lit` lit). The combined tail and brake lamp is both: the tail's part always on, so at night the lamp
/// glows before the pedal is touched, and the brake's lit part over it (the same faces) while it brakes, with no unlit
/// one of its own to cover the tail's lit one. The bumper's extra lamps stay in the body.
const LAMPS: [(&str, &str, &[&str]); 4] = [
    ("light_glass", "e_light_", &["", "_lit"]),
    ("light_combi_glass", "e_light_", &["", "_lit"]),
    ("light_combi_glass", "e_light_brake_", &["_lit"]),
    ("light_rev_glass", "e_light_reverse_", &["", "_lit"]),
];

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
    LAMPS.iter().any(|(lens, ..)| part.strip_prefix(lens).is_some_and(|side| side.starts_with('_') && side.len() == 3))
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
    style_of: &dyn Fn(&str) -> Option<Style>,
    wanted: impl Fn(&str) -> bool,
) -> Vec<Face> {
    let mut groups: BTreeMap<(&'static str, String), Face> = BTreeMap::new();
    for (instance, matrix) in instances {
        if !wanted(&instance.material) {
            continue;
        }
        let Some(style) = style_of(&instance.material) else { continue };
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
        let repeat = if style.key == "int_carbon" { CARBON_REPEAT } else { 1.0 };
        let uv = instance.uv.iter().map(|t| [t[0] * repeat, t[1] * repeat]);
        face.uv.extend(uv.chain(std::iter::repeat_n([0.0; 2], missing)));
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

/// The interior scene's mirror glass (the centre mirror and the driver's wing mirror) and the glass parts they become.
const MIRRORS: [(&str, &str); 2] = [("int_reflection_m", "i_mirrorglass_m"), ("int_reflection_r", "i_mirrorglass_r")];

/// The cockpit's source: the interior scene (`int_cme.pssg`) and the exterior nodes its config keeps active.
struct Cockpit<'a> {
    scene: &'a [Part],
    /// The exterior parts the cockpit draws as well (`bonnet`, `wing_fl`), by name without `x0_`.
    active: &'a [String],
    /// The tach's needle is drawn by the gauges (into the dials atlas), not as the mesh at rest.
    gauged: bool,
}

/// The exterior nodes an interior config keeps active in the cockpit (`<node name="x0_bonnet"/>`), without their `x0_`.
fn active_nodes(config: &str) -> Vec<String> {
    let names = config.split("<node name=\"").skip(1).filter_map(|rest| rest.split('"').next());
    names.filter_map(|name| name.strip_prefix("x0_")).map(str::to_owned).collect()
}

/// `p` of the car's frame as the part whose place is `m` sees it (`m` a rotation and a translation).
fn in_part_space(instance: &Instance, m: &Mat) -> Instance {
    let turn = |v: [f32; 3]| -> [f32; 3] { std::array::from_fn(|j| (0..3).map(|k| v[k] * m[j][k]).sum()) };
    Instance {
        positions: instance.positions.iter().map(|p| turn([p[0] - m[3][0], p[1] - m[3][1], p[2] - m[3][2]])).collect(),
        normals: instance.normals.iter().map(|&n| turn(n)).collect(),
        ..instance.clone()
    }
}

/// The interior scene's roof and sides, DiRT's frame: what lies above or beyond them is in or past the body's paint.
const ROOF: f32 = 1.40;
const SIDE: f32 = 0.68;

/// `instance` without the triangles whose centre lies above [`ROOF`] or beyond [`SIDE`].
fn within_body(instance: &Instance) -> Instance {
    let inside = |t: &&[u32; 3]| {
        let centre = |k: usize| t.iter().map(|&i| instance.positions[i as usize][k]).sum::<f32>() / 3.0;
        centre(1) <= ROOF && centre(0).abs() <= SIDE
    };
    Instance { triangles: instance.triangles.iter().filter(inside).copied().collect(), ..instance.clone() }
}

/// `faces` as pieces of at most `MAX_VERTICES` vertices each, named `name`, `name2`, ...
fn split(name: &str, mut faces: Vec<Face>) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
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
        let name = if out.is_empty() { name.to_owned() } else { format!("{name}{}", out.len() + 1) };
        out.push(Piece { name, at: [0.0; 3], axes: UPRIGHT, faces: take });
    }
    out
}

fn pieces(parts: &[Part], cockpit: Option<&Cockpit>, frame: &Frame, car: &str) -> Vec<Piece> {
    let part = |name: &str| parts.iter().find(|p| p.name == name);
    let exterior = |m: &str| style(m, car);
    let inside = |m: &str| interior_style(m, car);
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
                &exterior,
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
                faces: faces(all(p).into_iter(), frame, [0.0; 3], &exterior, |_| true),
            });
        }
    }

    // The lamps: each lens as an unlit and a lit part, which the lamp's switch box swaps (`ini_file`).
    for (dirt, rbr, states) in LAMPS {
        for (side, rbr_side) in SIDES {
            let Some(lens) = part(&format!("{dirt}_{side}")) else { continue };
            for suffix in states {
                let faces = faces(all(lens).into_iter(), frame, [0.0; 3], &exterior, |_| true);
                out.push(Piece { name: format!("{rbr}{rbr_side}{suffix}"), at: [0.0; 3], axes: UPRIGHT, faces });
            }
        }
    }

    // The cabin, drawn in the cockpit (`i_internal`) and seen through the windows from outside (`e_internal`): the
    // interior scene (dash, door cards, cage, seats, pedals, levers, gauges; not the wipers, whose rest pose the body's
    // own show, nor the steering wheel) for both, and the exterior nodes the interior config keeps active (the bonnet and
    // wings seen over the dash) in the cockpit. Only the interior scene is used where the car has one: the exterior
    // file's cabin material is its low-detail copy. Without an interior scene it is the fallback for both.
    match cockpit {
        Some(c) => {
            let drawn: Vec<_> = c
                .scene
                .iter()
                .filter(|p| !p.name.starts_with("int_wiper") && p.name != "int_steering_wheel")
                .filter(|p| !(c.gauged && p.name == "int_needle_tach"))
                .filter(|p| !MIRRORS.iter().any(|(node, _)| *node == p.name))
                .flat_map(all)
                .collect();
            out.extend(split("i_internal", faces(drawn.clone().into_iter(), frame, [0.0; 3], &inside, |_| true)));
            // Seen from outside, the cage's roof tubes and the lining at the roof and the sides lie in the body's paint
            // (DiRT never draws this scene outside) and show through it as grey bands along the roof edge.
            let clipped: Vec<_> = drawn.iter().map(|(i, m)| (within_body(i), *m)).collect();
            let kept = clipped.iter().map(|(i, m)| (i, *m));
            out.extend(split("e_internal", faces(kept, frame, [0.0; 3], &inside, |_| true)));
            for (node, name) in MIRRORS {
                for p in c.scene.iter().filter(|p| p.name == node) {
                    let faces = faces(all(p).into_iter(), frame, [0.0; 3], &inside, |_| true);
                    out.push(Piece { name: name.to_owned(), at: [0.0; 3], axes: UPRIGHT, faces });
                }
            }
            let near: Vec<_> = parts.iter().filter(|p| c.active.contains(&p.name)).flat_map(all).collect();
            let near = faces(near.into_iter(), frame, [0.0; 3], &exterior, |m| m != "Cabin" && m != "Window Glass");
            out.extend(split("i_body", near));
        }
        None => {
            let has_cabin =
                |p: &Part| p.name.starts_with("main_interior") || p.instances.iter().any(|i| i.material == "Cabin");
            let shell: Vec<_> =
                parts.iter().filter(|p| has_cabin(p) && p.name != "steering_wheel").flat_map(all).collect();
            for name in ["e_internal", "i_internal"] {
                let faces = faces(shell.clone().into_iter(), frame, [0.0; 3], &exterior, |m| m == "Cabin");
                out.push(Piece { name: name.to_owned(), at: [0.0; 3], axes: UPRIGHT, faces });
            }
        }
    }

    // The steering wheel, in its own frame: the node's z is the column, toward the driver (the template's convention,
    // which the crew's hands and the wheel's turn use). The vertices are DiRT's with z negated (`faces` through the
    // zero frame), the axes the part's turned the same way: the mirror once, so the triangles are flipped once. The
    // wheel is the interior scene's own, taken into the exterior wheel's node, in both sets; the exterior file's
    // where there is no interior scene.
    if let Some(p) = part("steering_wheel") {
        let row = |k: usize| [p.transform[k][0], p.transform[k][1], p.transform[k][2]];
        let (x, y, z) = (Frame::direction(row(0)), Frame::direction(row(1)), Frame::direction(row(2)));
        let axes = [x, y, z.map(|v| -v)];
        let hub = frame.point([p.transform[3][0], p.transform[3][1], p.transform[3][2]]);
        let local = Frame { z_dirt: 0.0, z_template: 0.0, y_shift: 0.0 };
        let own: Vec<Instance> = cockpit
            .iter()
            .flat_map(|c| c.scene.iter())
            .filter(|q| q.name == "int_steering_wheel")
            .flat_map(|q| q.instances.iter().map(|i| in_part_space(i, &p.transform)))
            .collect();
        let wheel = || {
            if own.is_empty() {
                faces(p.instances.iter().map(|i| (i, IDENTITY)), &local, [0.0; 3], &exterior, |_| true)
            } else {
                faces(own.iter().map(|i| (i, IDENTITY)), &local, [0.0; 3], &inside, |_| true)
            }
        };
        for name in ["i_steeringwheel", "e_steeringwheel"] {
            out.push(Piece { name: name.to_owned(), at: hub, axes, faces: wheel() });
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
    let faces = faces(body.into_iter(), frame, [0.0; 3], &exterior, |m| m != "Cabin" && m != "Window Glass");
    out.extend(split("e_body", faces));
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

/// How far the renderer moves the cockpit eye back from `Cam_internal`'s `Pos` (`rbr-render`'s `views::COCKPIT_BACK`), m.
/// DiRT's head camera sits at the driver's head, among the seat's headrest: moved back, the eye sat behind it and saw its
/// top edge as a bar across the windscreen. `Pos` is put this far ahead of the head so that the eye lands on it.
const COCKPIT_BACK: f32 = 0.5;

/// The template's camera sections that DiRT's views replace: the section, DiRT's view, the parameter holding its place
/// and how far ahead of that place the section's `Pos` goes.
const CAMERAS: [(&str, &str, &str, f32); 3] = [
    ("Cam_internal", "head-cam", "position", COCKPIT_BACK),
    ("Cam_bonnet", "bumper", "offset", 0.0),
    ("Cam_bonnet2", "bonnet", "offset", 0.0),
];

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
    for (section, ident, parameter, ahead) in CAMERAS {
        let place = view(cameras, ident)
            .and_then(|v| v.children_named("Parameter").find(|p| p.attr("name") == Some(parameter)))
            .and_then(|p| Some([p.attr_num("x")? as f32, p.attr_num("y")? as f32, p.attr_num("z")? as f32]));
        if let Some(place) = place {
            let [x, y, z] = frame.point(place);
            out.insert(section, [x, y, z - ahead]);
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

/// `<name>_shaders.ini`: every material of the pieces with its texture, and its normal map as the `[<material>_bump_bitmap]`
/// section the renderer's cars read (`rbr_formats::car_parts::bump_path`).
fn shaders_file(name: &str, materials: &BTreeMap<String, Entry>) -> String {
    let mut out = String::from(
        "[Default Material]\nAmbient=0.500000 0.500000 0.500000\nDiffuse=0.500000 0.500000 0.500000\n\
         Specular=0.500000 0.500000 0.500000\nShininess=0.000000\nShineStrenth=0.000000\nTransparency=0.000000\n\
         Requirements=0\nDiffuseChannel=false\nOpacityChannel=false\nReflectionChannel=false\n\
         BumpChannel=false\nSelfIllumunationChannel=false\nSpecularColorChannel=false\n\n[TSDefault]\n",
    );
    let files: std::collections::BTreeSet<_> =
        materials.values().flat_map(|m| [Some(&m.file), m.normal.as_ref()]).flatten().collect();
    for f in &files {
        out += &format!("{f} = cars\\{name}\\textures\\{f}\n");
    }
    out += "\n[TextureSets]\nTextureSet0 = TSDefault\n\n[Textures]\n";
    for (i, f) in files.iter().enumerate() {
        out += &format!("Texture{i} = {f}\n");
    }
    for (key, Entry { opacity, file, normal }) in materials {
        let m = material_name(key);
        let bitmap = |kind: &str, f: &str, mono: bool| {
            format!(
                "\n[{m}_{kind}_bitmap]\nFilename={f}\nAmount=1.000000\nAlphaAsMono={mono}\nAlphaAsRGB=false\n\
                 TileU=1.000000\nTileV=1.000000\nOffsetU=0.000000\nOffsetV=0.000000\nWrapU=3\nWrapV=3\n"
            )
        };
        out += &bitmap("diffuse", file, false);
        if *opacity {
            out += &bitmap("opacity", file, true);
        }
        if let Some(normal) = normal {
            out += &bitmap("bump", normal, false);
        }
        out += &format!(
            "\n[{m}]\nAmbient=0.500000 0.500000 0.500000\nDiffuse=0.500000 0.500000 0.500000\n\
             Specular=0.900000 0.900000 0.900000\nShininess=0.100000\nShineStrength=0.300000\nTransparency=0.000000\n\
             Requirements=33808\nDiffuseChannel=true\nDiffuseType=Bitmap\nOpacityChannel={opacity}\n{}\
             ReflectionChannel=false\nSelfIlluminationChannel=false\nBumpChannel={}\nSpecularColorChannel=false\n",
            if *opacity { "OpacityType=Bitmap\n" } else { "" },
            normal.is_some(),
        );
    }
    out
}

/// `settings` with `key` of `section` set to `value`, the rest as it was (line endings too).
///
/// The paint's `Kf` goes to 0: the renderer's clear coat (a sky reflection and a broad sheen over the paint, on where
/// `Kf` and `FresnelMax` are both above 0) washed the livery's saturated red (168, 7, 10) to pink, where DiRT's paint
/// reflects little (its `FresnelMin` 0.1, `EnvironmentColour` 0.54). Without it the red holds and the sun's highlight stays.
fn set_key(settings: &str, section: &str, key: &str, value: &str) -> String {
    let mut inside = false;
    settings
        .split_inclusive('\n')
        .map(|line| {
            if let Some(name) = line.trim().strip_prefix('[').and_then(|l| l.split(']').next()) {
                inside = name == section;
            }
            match line.split_once('=') {
                Some((k, _)) if inside && k.trim() == key => {
                    format!("{}= {value}{}", k, &line[line.trim_end_matches(['\r', '\n']).len()..])
                }
                _ => line.to_owned(),
            }
        })
        .collect()
}

/// The cabin atlas's gain. DiRT lights its dark interior (a black dash, carbon panels, dark gauge faces) far more than the
/// renderer's cabin light does: unchanged the dash, the wheel and the gauges come out black in the cockpit. The atlas's
/// colours go through `x * g / (1 + (g - 1) * x)`: black and white stay, the darks rise (a dash of 0.05 reads 0.17).
const CABIN_GAIN: f32 = 4.0;

fn cabin_lift(x: f32) -> f32 {
    x * CABIN_GAIN / (1.0 + (CABIN_GAIN - 1.0) * x)
}

/// The texture `source` of the DiRT install as a `.dds`, from the files of `car_dir`.
struct Library {
    car_dir: PathBuf,
    generics: PathBuf,
    class: String,
    livery: u32,
}

impl Library {
    fn find(&self, source: &Source) -> Result<Texture, String> {
        let (path, name) = match source {
            Source::Livery(name) => (self.livery_file(""), name),
            Source::LiveryClass(short, name) => (self.livery_file(&format!("_{short}")), name),
            Source::Generic(short, name) => {
                (self.generics.join(&self.class).join(format!("car_gen_{short}_high.pssg")), name)
            }
            Source::Interior(name) => (self.interior_file(), name),
            Source::Gear => return Err("the gear display's base is made, not read".into()),
        };
        let pssg = Pssg::read(&path)?;
        let textures = texture::read_car(&pssg);
        let found: Option<&Texture> = textures.iter().find(|t| t.name.eq_ignore_ascii_case(name));
        found.cloned().ok_or_else(|| format!("{}: no texture {name}", path.display()))
    }

    /// `cars/interiors/models/<car>/int_<car>.pssg`, beside the `cars/models` folder of the car.
    fn interior_file(&self) -> PathBuf {
        let car = self.car_dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let cars = self.car_dir.parent().and_then(Path::parent).unwrap_or(Path::new(""));
        cars.join(format!("interiors/models/{car}/int_{car}.pssg"))
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

    let library =
        Library { car_dir, generics: data.join("cars/generics"), class: options.class.clone(), livery: options.livery };
    // The interior scene and its config, where the car has them (its cockpit is then DiRT's own).
    let interior = match library.interior_file() {
        path if path.exists() => Some(car::interior(&Pssg::read(&path)?)?),
        _ => None,
    };
    let config = fs::read_to_string(library.interior_file().with_extension("xml")).unwrap_or_default();
    let active = active_nodes(&config);
    let gauges = gauges::layout(&options.car).filter(|_| interior.is_some());
    let cockpit = interior.as_deref().map(|scene| Cockpit { scene, active: &active, gauged: gauges.is_some() });
    let pieces = pieces(&parts, cockpit.as_ref(), &frame, &options.car);

    let folder = out.join("Cars").join(&options.name);
    let textures = folder.join("textures");
    fs::create_dir_all(&textures).map_err(|e| e.to_string())?;
    let mut materials: BTreeMap<String, Entry> = BTreeMap::new();
    let mut styles: BTreeMap<String, Style> = BTreeMap::new();
    let mut normals: BTreeMap<Source, String> = BTreeMap::new();
    let scene = cockpit.iter().flat_map(|c| c.scene);
    for (part, inside) in parts.iter().map(|p| (p, false)).chain(scene.map(|p| (p, true))) {
        for instance in &part.instances {
            let style = if inside {
                interior_style(&instance.material, &options.car)
            } else {
                style(&instance.material, &options.car)
            };
            if let Some(s) = style {
                // The livery's three finishes read one normal map: it is written once, by the first.
                let normal = s
                    .maps
                    .normal
                    .clone()
                    .map(|source| normals.entry(source).or_insert_with(|| normal_file(&s.key)).clone());
                materials.insert(s.key.clone(), Entry { opacity: s.opacity, file: file(&s.key), normal });
                styles.insert(s.key.clone(), s);
            }
        }
    }
    let used: std::collections::BTreeSet<&str> =
        pieces.iter().flat_map(|p| p.faces.iter().map(|f| f.material.as_str())).collect();
    materials.retain(|k, _| used.contains(k.as_str()));
    let mut written = std::collections::BTreeSet::new();
    for key in materials.keys() {
        let style = &styles[key];
        let bytes = match &style.texture {
            Source::Gear => gauges::gear_base(),
            source => {
                let texture = library.find(source)?;
                let dark = key == "cabin" || key.starts_with("int_");
                let tone: fn(f32) -> f32 = if dark { cabin_lift } else { |x| x };
                bake(&library, &style.maps, &texture, tone)?
            }
        };
        fs::write(textures.join(file(key)), bytes).map_err(|e| e.to_string())?;
        if let (Some(source), Some(file)) = (&style.maps.normal, &materials[key].normal) {
            if written.insert(file.clone()) {
                fs::write(textures.join(file), car_maps::normal(&library.find(source)?, style.maps.crumpled))
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    if let Some(layout) = gauges {
        let find = |name: &str| library.find(&Source::Interior(name.to_owned()));
        let car = &options.car;
        let gear_font = gauges::gear_font(data, &config)?;
        let sources = gauges::Sources {
            dials: &find(&format!("int_{car}_dials.tga"))?,
            dials_on: &find(&format!("int_{car}_dials_on.tga"))?,
            indicator: &find("int_indicator.tga")?,
            indicator_on: &find("int_indicator_on.tga")?,
            digits: &find("int_digi_speed.tga")?,
            gear_font: &gear_font,
            config: &config,
            tone: cabin_lift,
        };
        gauges::write(out, &options.name, layout, &sources)?;
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
        let mut text = fs::read(&from).map_err(|e| format!("{}: {e}", from.display()))?;
        if suffix.contains("shader_settings") {
            text = set_key(&String::from_utf8_lossy(&text), "CarPaint", "Kf", "0.000000").into_bytes();
        }
        fs::write(folder.join(format!("{name}{suffix}")), text).map_err(|e| e.to_string())?;
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
        assert_eq!(class("Matt"), Some(("CarTextureNoDirt", "matt".to_owned())));
        assert_eq!(class("Shiny Metal"), Some(("CarTextureNoDirt", "metal".to_owned())));
        assert_eq!(class("Gravel Wheel"), Some(("CarRim", "wheel_gravel".to_owned())));
        assert_eq!(class("Snow Tyre Tread"), Some(("CarTire", "tyre_snow".to_owned())));
        // The sidewall is on the wheel's atlas, the tread has its own texture.
        assert_eq!(class("Tarmac Tyre Wall"), Some(("CarTire", "wall_tarmac".to_owned())));
        let texture = |m: &str| style(m, "cme").map(|s| s.texture);
        assert!(texture("Tarmac Tyre Wall") == Some(Source::LiveryClass("tm", "cme_wheel.tga".to_owned())));
        assert!(texture("Tarmac Tyre Tread") == Some(Source::Generic("tm", "tread.tga".to_owned())));
        assert_eq!(class("Nothing"), None);
        assert_eq!(window("window_fl").as_deref(), Some("e_window_lf"));
        assert_eq!(window("window_f_trim"), None);
    }

    #[test]
    fn the_bodys_maps_are_the_liveries_in_the_materials_own_occlusion() {
        let maps = |m: &str| style(m, "cme").map(|s| s.maps).unwrap();
        for (material, scale) in [("Car Body Paint", 0.4), ("Matt", 0.8), ("Shiny Metal", 0.5)] {
            let maps = maps(material);
            assert!(maps.normal == Some(Source::Livery("cme_main_nm.tga".to_owned())), "{material}");
            assert!(maps.crumpled, "{material}");
            assert!(maps.occlusion == Some((Source::Livery("cme_specocc.tga".to_owned()), 3, scale)), "{material}");
            assert!(maps.specular.is_some_and(
                |(source, channel, _)| channel == 1 && source == Source::Livery("cme_specocc.tga".to_owned())
            ));
        }
        // The lens's alpha is its opacity, so it takes no specular, and the lit lamps' colours no occlusion.
        let lens = maps("Lights Glass");
        assert!(lens.normal.is_some() && lens.specular.is_none() && lens.occlusion.is_none());
        let pod = maps("Lights Pod");
        assert!(pod.normal.is_some() && pod.specular.is_some() && pod.occlusion.is_none());
        assert!(maps("Window Glass").normal.is_none() && maps("Cabin").normal.is_none());
        let interior = interior_style("Leather", "cme").unwrap().maps;
        assert!(interior.normal == Some(Source::Interior("int_cme_nm.tga".to_owned())) && !interior.crumpled);
        assert!(interior.occlusion.is_some_and(
            |(source, channel, _)| channel == 0 && source == Source::Interior("int_cme_occ.tga".to_owned())
        ));
    }

    #[test]
    fn the_shaders_file_names_each_normal_map_as_a_bump_bitmap() {
        let entry = |normal: Option<&str>| Entry {
            opacity: false,
            file: "dr_paint.dds".to_owned(),
            normal: normal.map(str::to_owned),
        };
        let materials =
            BTreeMap::from([("paint".to_owned(), entry(Some("dr_paint_nm.dds"))), ("disc".to_owned(), entry(None))]);
        let text = shaders_file("Car", &materials);
        let sections = rbr_formats::ini::parse(&text).unwrap();
        let bump = rbr_formats::car_parts::bump_path;
        assert_eq!(bump(&sections, &material_name("paint")), Some("cars\\Car\\textures\\dr_paint_nm.dds"));
        assert_eq!(bump(&sections, &material_name("disc")), None);
        assert!(text.contains("BumpChannel=true") && text.contains("BumpChannel=false"));
    }

    #[test]
    fn a_settings_key_is_set_in_its_section_only() {
        let text = "[CarPaint]\r\nKd = 1.6\r\nKf = 0.210000\r\n[CarGlass]\r\nKf = 1.0\r\n";
        assert_eq!(
            set_key(text, "CarPaint", "Kf", "0.000000"),
            "[CarPaint]\r\nKd = 1.6\r\nKf = 0.000000\r\n[CarGlass]\r\nKf = 1.0\r\n"
        );
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

    fn instance(positions: Vec<[f32; 3]>, triangles: Vec<[u32; 3]>) -> Instance {
        Instance {
            material: "Matt".into(),
            joint: None,
            source: "src".into(),
            normals: vec![[0.0, 0.0, 1.0]; positions.len()],
            uv: vec![[0.0, 0.0]; positions.len()],
            positions,
            triangles,
        }
    }

    #[test]
    fn the_interior_scene_has_its_own_materials() {
        let class = |m: &str| interior_style(m, "cme").map(|s| (s.shader, s.key, s.opacity));
        for m in ["Matt", "Leather", "Shiny Painted Metal", "bare metal", "Chrome", "shiny plastic", "suede"] {
            assert_eq!(class(m), Some(("CarTextureNoDirt", "int_main".to_owned(), false)), "{m}");
        }
        assert_eq!(class("carbon fibre").map(|c| c.1), Some("int_carbon".to_owned()));
        assert_eq!(class("dials shiny").map(|c| c.1), Some("int_dials".to_owned()));
        assert_eq!(class("limit lights").map(|c| c.1), Some("int_indicator".to_owned()));
        assert_eq!(class("mirror").map(|c| c.0), Some("CarGlass"));
        for m in ["interior glass windows", "digital speedometer", "Car Body Paint"] {
            assert_eq!(class(m), None, "{m}");
        }
        // The gear display is drawn only for a car whose gauges are measured.
        assert_eq!(class("gear indicator").map(|c| c.1), Some("int_gear".to_owned()));
        assert!(interior_style("gear indicator", "cme").is_some_and(|s| s.texture == Source::Gear));
        assert!(interior_style("gear indicator", "another").is_none());
        let texture = |m: &str| interior_style(m, "cme").map(|s| s.texture);
        assert!(texture("Matt") == Some(Source::Interior("int_cme_main.tga".to_owned())));
    }

    #[test]
    fn the_interior_configs_active_nodes_lose_their_prefix() {
        let config = "<active_nodes>\n<node name=\"x0_bonnet\"/>\n<node name=\"x0_wing_fl\"/>\n</active_nodes>\n\
                      <displays><display node=\"int_needle_tach\"/></displays>";
        assert_eq!(active_nodes(config), ["bonnet", "wing_fl"]);
        assert!(active_nodes("").is_empty());
    }

    #[test]
    fn a_part_is_taken_into_its_own_space() {
        // A part turned a quarter about y (its x along the car's -z) and put at (1, 2, 3).
        let m: Mat = [[0.0, 0.0, -1.0, 0.0], [0.0, 1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [1.0, 2.0, 3.0, 1.0]];
        let local = in_part_space(&instance(vec![[1.0, 2.0, 3.0], [1.0, 2.0, 2.0]], vec![[0, 1, 0]]), &m);
        assert_eq!(local.positions, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        // Back through the part's own matrix.
        assert_eq!(crate::mesh::point(&m, local.positions[1]), [1.0, 2.0, 2.0]);
        assert_eq!(local.normals[0], [-1.0, 0.0, 0.0], "the car's z is the part's -x");
    }

    #[test]
    fn the_roof_and_sides_are_clipped_from_the_outside_cabin() {
        let tri = |dx: f32, y: f32| [[dx, y, 0.0], [dx + 0.1, y, 0.0], [dx, y + 0.1, 0.0]];
        let triangles = [tri(0.0, 1.0), tri(0.0, 1.45), tri(0.7, 1.0), tri(-0.4, 0.5)];
        let positions: Vec<[f32; 3]> = triangles.iter().flatten().copied().collect();
        let all: Vec<[u32; 3]> = (0..4).map(|k| [3 * k, 3 * k + 1, 3 * k + 2]).collect();
        let kept = within_body(&instance(positions, all));
        assert_eq!(kept.triangles, [[0, 1, 2], [9, 10, 11]]);
        assert_eq!(kept.positions.len(), 12, "the vertices are left as they were");
    }

    #[test]
    fn faces_over_the_vertex_limit_make_more_pieces() {
        let face = |n: usize| Face { positions: vec![[0.0; 3]; n], ..triangle("paint") };
        let pieces = split("i_internal", vec![face(40_000), face(40_000), face(100)]);
        let named: Vec<_> = pieces.iter().map(|p| (p.name.as_str(), p.faces.len())).collect();
        assert_eq!(named, [("i_internal", 1), ("i_internal2", 2)]);
        assert!(split("e_body", Vec::new()).is_empty());
    }

    #[test]
    fn the_cockpit_eye_goes_ahead_of_the_head_by_the_renderers_back_step() {
        let parameter = |name: &str, [x, y, z]: [&str; 3]| Element {
            name: "Parameter".into(),
            attrs: [("name", name), ("x", x), ("y", y), ("z", z)].map(|(k, v)| (k.into(), v.into())).to_vec(),
            ..Default::default()
        };
        let view = |ident: &str, parameter| Element {
            name: "View".into(),
            attrs: vec![("ident".into(), ident.into())],
            children: vec![parameter],
            ..Default::default()
        };
        let cameras = Element {
            name: "Cameras".into(),
            children: vec![
                view("head-cam", parameter("position", ["-0.343", "1.2", "-0.3"])),
                view("bumper", parameter("offset", ["0.0", "0.5", "2.0"])),
            ],
            ..Default::default()
        };
        let frame = Frame { z_dirt: 0.0, z_template: -1.0, y_shift: 0.0 };
        let eyes = eyes(&cameras, &frame);
        let head = frame.point([-0.343, 1.2, -0.3]);
        assert_eq!(eyes["Cam_internal"], [head[0], head[1], head[2] - COCKPIT_BACK]);
        assert_eq!(eyes["Cam_bonnet"], frame.point([0.0, 0.5, 2.0]));
        assert!(!eyes.contains_key("Cam_bonnet2"));
    }
}
