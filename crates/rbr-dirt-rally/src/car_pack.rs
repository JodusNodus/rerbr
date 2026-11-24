//! A DiRT Rally car as the model folder of an NGP car pack (`Cars/<name>/`): `.sgc`, `.ini`, `_shaders.ini` and
//! textures, so the renderer draws it as any community car.
//!
//! DiRT's meshes replace the template car's: the template (a car folder of the player's, such as an RSF car of the same
//! model) keeps its collision hulls, helper nodes, points, cameras, lamp settings and shader settings, and gives the
//! frame the physics pack was fitted to (the rear axle at the origin, the front at -z, +x the left). The DiRT car's wheel
//! centres are laid on the template's, the body between them follows (the wheelbases differ by centimetres).
//!
//! Every car's files have one layout (`notes/dirt-rally.md`, "Cars"): the exterior's materials are named alike on every
//! car and bind their textures by name ([`exterior_style`]), the interior's bind theirs ([`interior_style`]).
use crate::binxml::{self, Element};
use crate::car::{self, Instance, Part};
use crate::car_maps::{self, Plane};
use crate::gauges;
use crate::material::{self, Material};
use crate::mesh::{self, Mat};
use crate::needles;
use crate::pssg::Pssg;
use crate::texture::{self, Texture};
use rbr_formats::car_gauges::{self, Needle};
use rbr_formats::{ini, rsf, sgc};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// What `convert` is asked for.
pub struct Options {
    /// The car's folder under `cars/models` (`cme`).
    pub car: String,
    /// The generic texture set of its class under `cars/generics` (`historic`, `rally`, `groupb`, `rallyx`,
    /// `trailblazer`): the game's database says which, the car's files do not.
    pub class: String,
    /// The livery number (`livery_00`); `None` for the car's first.
    pub livery: Option<u32>,
    /// The template car's model (`.../Cars/Escort_mk_2/Escort_mk_2.sgc`); its folder holds its `.ini` and settings
    /// under the same name.
    pub template: PathBuf,
    /// The pack's model folder name (`Cars/<name>/<name>.sgc`).
    pub name: String,
}

/// What a material draws as: the shader class, the material key (the faces of a key share its texture) and the texture
/// with its other maps.
#[derive(Clone, Debug, PartialEq)]
struct Style {
    shader: &'static str,
    key: String,
    texture: Source,
    /// The texture's alpha is the faces' opacity (the glass, the lenses), not the specular.
    opacity: bool,
    /// The UVs' repeats (the carbon weave's `CarbonUVRepeat`).
    repeat: f32,
    maps: Maps,
}

impl Style {
    fn new(shader: &'static str, key: impl Into<String>, texture: Source) -> Style {
        Style { shader, key: key.into(), texture, opacity: false, repeat: 1.0, maps: Maps::default() }
    }

    fn with_normal(self, normal: Source) -> Style {
        Style { maps: Maps { normal: Some(normal), ..self.maps }, ..self }
    }
}

/// A material's other maps, on its texture's own UVs (`car_maps`).
#[derive(Clone, Debug, Default, PartialEq)]
struct Maps {
    normal: Option<Source>,
    /// The normal map's panels carry a damage crumple, which the car does not take undamaged (`car_maps::normal`).
    crumpled: bool,
    /// The map and channel of the baked occlusion, and how much of it the colour takes.
    occlusion: Option<(Source, usize, f32)>,
    /// The map and channel of the specular, and the specular of the finish that reflects in full.
    specular: Option<(Source, usize, f32)>,
    /// The backlight by night (the interior's `TEmissiveMap`: the dials' and lamps' `_on` maps), written as the texture's
    /// glow map (`<file>_glow.dds`, `rbr-render`'s car textures).
    emissive: Option<Source>,
}

/// Where a texture comes from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
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

impl Source {
    /// The file a map is written as, once whatever reads it: `dr_<name>.dds`, with the file's class where the name is a
    /// class file's (`dr_cme_wheel_nm_tm.dds`, `dr_tread_nm_gr.dds`).
    fn file(&self) -> String {
        let (name, class) = match self {
            Source::Livery(name) | Source::Interior(name) => (name.as_str(), "main"),
            Source::LiveryClass(class, name) | Source::Generic(class, name) => (name.as_str(), *class),
            Source::Gear => ("gear", "main"),
        };
        let stem = stem(name);
        if class == "main" {
            format!("dr_{stem}.dds")
        } else {
            format!("dr_{stem}_{class}.dds")
        }
    }
}

/// The tyre classes: the renderer's name, the suffix of the livery's wheel file and of the generic tread file, and the
/// materials' prefix (`Tarmac Wheel`).
const CLASSES: [(&str, &str, &str); 3] =
    [("tarmac", "tm", "Tarmac"), ("gravel", "gr", "Gravel"), ("snow", "sn", "Snow")];

/// The specular that reflects in full (chrome and bare metal reach 0.8 to 1 in DiRT's maps, the Escort's red paint is
/// 0.43, the dull plastics and rubber under 0.2): the paint's clear coat takes about half its strength, the metal all.
const FULL_SPECULAR: f32 = 0.9;

/// The first value of a material's constant `name`.
fn constant(m: &Material, name: &str) -> Option<f32> {
    m.constants.get(name).and_then(|v| v.first()).copied()
}

/// What a material of the car's file draws as. The exterior's materials hold constants only and are named alike on every
/// car (surveyed on the 46); their textures bind by name: the livery's atlases (`<car>_main`, `_cabin`, `_glass`,
/// `_lights`, `disc`, `caliper`), its wheel file per tyre class and the class's generics (`tread`, `suspension`,
/// `grills`). Not drawn: the materials of the damage and the weather (none on the parts) and the spare wheels but the
/// tarmac class's (the spare is in the body, all three classes in one place).
fn exterior_style(car: &str, m: &Material) -> Option<Style> {
    let own = |name: &str| Source::Livery(format!("{car}_{name}.tga"));
    let livery = |name: &str| Source::Livery(format!("{name}.tga"));
    let generic = |class: &'static str, name: &str| Source::Generic(class, format!("{name}.tga"));
    let name = match m.id.strip_prefix("Spare ") {
        Some(rest) if rest.starts_with("Tarmac ") => rest,
        Some(_) => return None,
        None => m.id.as_str(),
    };
    if let Some(&(class, short, title)) = CLASSES.iter().find(|(_, _, title)| name.starts_with(title)) {
        // The wheel's atlas holds the rim and the tyre's sidewall (the wall's UVs span all of it); the tread has its own
        // tiled texture.
        let atlas = |shader| {
            Style::new(shader, format!("wheel_{class}"), Source::LiveryClass(short, format!("{car}_wheel.tga")))
                .with_normal(Source::LiveryClass(short, format!("{car}_wheel_nm.tga")))
        };
        return match &name[title.len()..] {
            " Wheel" => Some(atlas("CarRim")),
            " Tyre Wall" => Some(atlas("CarTire")),
            " Tyre Tread" => Some(
                Style::new("CarTire", format!("tyre_{class}"), generic(short, "tread"))
                    .with_normal(generic(short, "tread_nm")),
            ),
            _ => None,
        };
    }
    // The body's atlas, which most materials read: its normal map (with the damage crumple), and `<car>_specocc`
    // (specular in the colour, baked occlusion in the alpha) on its UVs, the occlusion in the strength the material gives
    // it (`OcclusionMapScale`, per car and material: the Escort's paint 0.4, `Matt` 0.8, `Shiny Metal` 0.5). One texture
    // per strength.
    let body = |shader| {
        let occlusion = constant(m, "OcclusionMapScale").unwrap_or(1.0);
        let maps = Maps {
            normal: Some(own("main_nm")),
            crumpled: true,
            occlusion: Some((own("specocc"), 3, occlusion)),
            specular: Some((own("specocc"), 1, FULL_SPECULAR)),
            emissive: None,
        };
        Style { maps, ..Style::new(shader, format!("main{:.0}", occlusion * 100.0), own("main")) }
    };
    let matt = |key: &str, texture| Style::new("CarTextureNoDirt", key, texture);
    // A texture and its normal map beside it (`disc`, `disc_nm`).
    let mapped =
        |key: &str, source: &dyn Fn(&str) -> Source| matt(key, source(key)).with_normal(source(&format!("{key}_nm")));
    match name {
        // Bare metal reflects as the paint does (DiRT gives both `carpaint_metallic.fx`).
        "Car Body Paint" | "Shiny Metal" => Some(body("CarPaint")),
        "Matt" | "Mud Flaps" | "Aerials" | "Carbon Fiber" => Some(body("CarTextureNoDirt")),
        "Cabin" => Some(matt("cabin", own("cabin"))),
        "Window Glass" => Some(Style { opacity: true, ..Style::new("CarGlass", "glass", own("glass")) }),
        // The lens carries the lamp's colours: its UVs lie on the lights atlas (red and amber at the tail, a clear
        // circle at the head); the atlas's alpha is the lens's own.
        "Lights Glass" => {
            Some(Style { opacity: true, ..Style::new("CarGlass", "lens", own("lights")) }.with_normal(own("lights_nm")))
        }
        // The housings' specular is in `<car>_lights_spec_occ` (their occlusion is left out: the lit lamps' colours are
        // the texture's).
        "Lights Pod" | "Lights Satin" => {
            let maps = Maps { specular: Some((own("lights_spec_occ"), 1, FULL_SPECULAR)), ..Maps::default() };
            Some(Style { maps, ..matt("lights", own("lights")) }.with_normal(own("lights_nm")))
        }
        "Discs" => Some(mapped("disc", &livery)),
        "Caliper" => Some(mapped("caliper", &livery)),
        "Suspension" => Some(mapped("suspension", &|name| generic("main", name))),
        "Grills" => Some(mapped("grills", &|name| generic("main", name))),
        _ => None,
    }
}

/// The occlusion the interior's own maps put in its colours. The game's `OcclusionMapScale` is 1 there, on the ambient
/// light alone (which the dash takes from the sky it sees, `rbr-render`'s cabin light), so the bake takes a part of it.
const INTERIOR_OCCLUSION: f32 = 0.5;

/// What a material of the interior scene draws as, by its shader and the textures it binds (surveyed on the 46 cars):
/// its diffuse (`TDiffuseAlphaMap`), with the normal, occlusion and specular maps that are the car's own (`int_<car>_nm`,
/// `_occ`, `_sp`, on the atlas's UVs; the shared `int_gen_sp` is a 32 texel constant) and an occlusion its
/// `OcclusionMapScale` does not turn off. The alpha and netting shaders' alpha is their opacity. The carbon fibre's UVs
/// lie on the atlas's lightmap layout, not on a picture of carbon: it draws the weave (`TCarbonDiffuseMap`) repeated
/// `CarbonUVRepeat` times over them, as DiRT's shader does. The gear display (`lcd_text`, a texture of numerals the
/// game scrolls) is a black texture of its own which the gauges draw the digit on, for a car whose gauges are measured
/// ([`gauges::layout`]); the other LCDs and the glass are not drawn. The mirrors' glass (which the game draws as a
/// texture of what is behind) is the renderer's glass class on a flat texel: it shows the surroundings by the part's
/// name (`i_mirrorglass_*`, `rbr-render`'s `mirrors`).
fn interior_style(car: &str, m: &Material, gauged: bool) -> Option<Style> {
    let bound = |input: &str| m.textures.get(input).map(|t| t.trim_start_matches('#').to_owned());
    let own = |input: &str| bound(input).filter(|t| !t.starts_with("int_gen_")).map(Source::Interior);
    let style = |key: String, texture| Style::new("CarTextureNoDirt", key, texture);
    match m.shader.as_str() {
        "interior_windscreen.fx" => None,
        "lcd_text.fx" => (gauged && m.id == "gear indicator").then(|| style("int_gear".into(), Source::Gear)),
        "mirror.fx" => {
            Some(Style { shader: "CarGlass", ..style("int_mirror".into(), Source::Interior("int_gen_sp.tga".into())) })
        }
        "interior_carbon.fx" => {
            let weave = bound("TCarbonDiffuseMap")?;
            let repeat = constant(m, "CarbonUVRepeat").unwrap_or(1.0);
            Some(Style { repeat, ..style(interior_key(car, &weave), Source::Interior(weave)) })
        }
        shader => {
            let texture = bound("TDiffuseAlphaMap")?;
            let opacity = matches!(shader, "interior_alpha.fx" | "interior_netting.fx");
            let occlusion = own("TOcclusionMap").filter(|_| constant(m, "OcclusionMapScale").unwrap_or(1.0) > 0.0);
            let maps = Maps {
                normal: own("TNormalMap"),
                crumpled: false,
                occlusion: occlusion.map(|map| (map, 0, INTERIOR_OCCLUSION)),
                specular: own("TSpecularMap").filter(|_| !opacity).map(|map| (map, 0, FULL_SPECULAR)),
                emissive: own("TEmissiveMap"),
            };
            let key = interior_key(car, &texture) + if opacity { "_alpha" } else { "" };
            Some(Style { opacity, maps, ..style(key, Source::Interior(texture)) })
        }
    }
}

/// A texture's name without its extension, in lower case (`int_405_main.TGA`: `int_405_main`).
fn stem(texture: &str) -> String {
    texture.rsplit_once('.').map_or(texture, |(stem, _)| stem).to_ascii_lowercase()
}

/// An interior texture's key: its name without the car's (`int_cme_main.tga`: `int_main`).
fn interior_key(car: &str, texture: &str) -> String {
    let stem = stem(texture);
    stem.strip_prefix(&format!("int_{car}_")).map_or_else(|| stem.clone(), |rest| format!("int_{rest}"))
}

/// The interior's materials' styles: one per drawn texture, with every map any of its materials gives it (the dash,
/// doors, seats, cage, levers and wheel read one atlas, `int_<car>_main`, whatever their shader, and the Escort's chrome
/// turns its occlusion off).
fn interior_styles(car: &str, materials: &BTreeMap<String, Material>, gauged: bool) -> BTreeMap<String, Style> {
    let styles: Vec<(&String, Style)> =
        materials.values().filter_map(|m| Some((&m.id, interior_style(car, m, gauged)?))).collect();
    let mut merged: BTreeMap<&str, Style> = BTreeMap::new();
    for (_, style) in &styles {
        let first = &mut merged.entry(style.key.as_str()).or_insert_with(|| style.clone()).maps;
        first.normal = first.normal.take().or_else(|| style.maps.normal.clone());
        first.occlusion = first.occlusion.take().or_else(|| style.maps.occlusion.clone());
        first.specular = first.specular.take().or_else(|| style.maps.specular.clone());
        first.emissive = first.emissive.take().or_else(|| style.maps.emissive.clone());
    }
    styles.iter().map(|(id, style)| ((*id).clone(), merged[style.key.as_str()].clone())).collect()
}

/// The file a material's texture is written as: `dr_<key>.dds`.
pub(crate) fn file(key: &str) -> String {
    format!("dr_{key}.dds")
}

/// The file of a material's texture's glow map: `dr_<key>_glow.dds`, beside it.
fn glow_file(key: &str) -> String {
    file(&format!("{key}_glow"))
}

/// A mesh of the output before it is written: vertices in the part's own space and the node's place.
struct Piece {
    name: String,
    /// The node's translation in the template's frame.
    at: [f32; 3],
    /// The node's axes in the template's frame (its columns), [`UPRIGHT`] for a part modelled in the car's.
    axes: [[f32; 3]; 3],
    faces: Vec<Face>,
}

impl Piece {
    /// A piece modelled in the car's frame.
    fn upright(name: impl Into<String>, faces: Vec<Face>) -> Piece {
        Piece { name: name.into(), at: [0.0; 3], axes: UPRIGHT, faces }
    }
}

/// The car's own axes.
const UPRIGHT: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// The triangles of one shader class and material key.
struct Face {
    shader: &'static str,
    material: String,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    triangles: Vec<[u32; 3]>,
}

/// DiRT's wheel suffixes and the RBR's, in the wheel order of the renderer.
const SIDES: [(&str, &str); 4] = [("fl", "lf"), ("fr", "rf"), ("bl", "lb"), ("br", "rb")];
/// A part's vertex limit: the triangle indices are 16 bits.
const MAX_VERTICES: usize = 60_000;

/// Parts not drawn: the wipers' animation keys (`wiper_fl_pos_01`).
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
/// (`""` unlit, `_lit` lit), the first of a name kept. The combined tail and brake lamp is both: the tail's part always on,
/// so at night the lamp glows before the pedal is touched, and the brake's lit part over it (the same faces) while it
/// brakes, with no unlit one of its own to cover the tail's lit one, where the car has no brake lens of its own. The
/// second sets (`light_2_*`: inner headlamps, second tail lamps) and the pods on the bonnet (`light_rig_*`) are lamps of
/// their own on the side's headlamp or tail lamp switch ([`lamp`]), lit with the headlights.
const LAMPS: [(&str, &str, &[&str]); 7] = [
    ("light_glass", "e_light_", &["", "_lit"]),
    ("light_2_glass", "e_light_2_", &["", "_lit"]),
    ("light_rig_glass", "e_light_rig_", &["", "_lit"]),
    ("light_brake_glass", "e_light_brake_", &["", "_lit"]),
    ("light_combi_glass", "e_light_", &["", "_lit"]),
    ("light_combi_glass", "e_light_brake_", &["_lit"]),
    ("light_rev_glass", "e_light_reverse_", &["", "_lit"]),
];

/// A lamp part's switch box and whether it is the lit part, by the names the renderer gives lamps (`e_light_lf_lit`:
/// `light_lf`, lit; `e_light_brake_lb`: `brake_lb`, unlit; a second set's or a pod's the side's: `e_light_rig_lf_lit`:
/// `light_lf`).
fn lamp(part: &str) -> Option<(String, bool)> {
    let rest = part.strip_prefix("e_light_")?;
    let (rest, lit) = rest.strip_suffix("_lit").map_or((rest, false), |rest| (rest, true));
    let rest = ["2_", "rig_"].iter().find_map(|set| rest.strip_prefix(set)).unwrap_or(rest);
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
    /// The mirror alone, for a part drawn in its own frame.
    const LOCAL: Frame = Frame { z_dirt: 0.0, z_template: 0.0, y_shift: 0.0 };

    /// The frame that lays the DiRT car's wheel centres (their mean) on the template's `hubs`.
    fn fitted(parts: &[Part], hubs: &[[f32; 3]; 4]) -> Result<Frame, String> {
        let dirt = SIDES
            .iter()
            .map(|(side, _)| {
                let wheel = parts.iter().find(|p| p.name == format!("wheel_{side}"));
                wheel.map(|p| translation(&p.transform)).ok_or_else(|| format!("the car has no wheel_{side}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mean = |hubs: &[[f32; 3]], axis: usize| hubs.iter().map(|h| h[axis]).sum::<f32>() / hubs.len() as f32;
        Ok(Frame { z_dirt: mean(&dirt, 2), z_template: mean(hubs, 2), y_shift: mean(hubs, 1) - mean(&dirt, 1) })
    }

    fn point(&self, p: [f32; 3]) -> [f32; 3] {
        [p[0], p[1] + self.y_shift, -(p[2] - self.z_dirt) + self.z_template]
    }

    fn direction(n: [f32; 3]) -> [f32; 3] {
        [n[0], n[1], -n[2]]
    }
}

fn translation(m: &Mat) -> [f32; 3] {
    [m[3][0], m[3][1], m[3][2]]
}

/// The instances, with the matrices that place them in DiRT's frame, as faces in `frame`, grouped by shader class and
/// material key: the materials `styles` draws and `wanted` takes.
fn faces<'a>(
    instances: impl IntoIterator<Item = (&'a Instance, Mat)>,
    frame: &Frame,
    styles: &BTreeMap<String, Style>,
    wanted: impl Fn(&str) -> bool,
) -> Vec<Face> {
    let mut groups: BTreeMap<(&'static str, &str), Face> = BTreeMap::new();
    for (instance, matrix) in instances {
        let Some(style) = styles.get(&instance.material).filter(|_| wanted(&instance.material)) else { continue };
        let face = groups.entry((style.shader, &style.key)).or_insert_with(|| Face {
            shader: style.shader,
            material: style.key.clone(),
            positions: Vec::new(),
            normals: Vec::new(),
            uv: Vec::new(),
            triangles: Vec::new(),
        });
        let (base, count) = (face.positions.len() as u32, instance.positions.len());
        face.positions.extend(instance.positions.iter().map(|&p| frame.point(mesh::point(&matrix, p))));
        let normals = instance.normals.iter().map(|&n| Frame::direction(mesh::direction(&matrix, n)));
        face.normals.extend(normals.chain(std::iter::repeat([0.0, 1.0, 0.0])).take(count));
        let uv = instance.uv.iter().map(|t| t.map(|v| v * style.repeat));
        face.uv.extend(uv.chain(std::iter::repeat([0.0; 2])).take(count));
        // The z mirror turns right-handed counter-clockwise fronts into the renderer's, which wants them the other way.
        face.triangles.extend(instance.triangles.iter().map(|t| [t[0] + base, t[2] + base, t[1] + base]));
    }
    groups.into_values().filter(|f| !f.triangles.is_empty()).collect()
}

/// A part's instances with the part's matrix.
fn all(p: &Part) -> impl Iterator<Item = (&Instance, Mat)> {
    p.instances.iter().map(|i| (i, p.transform))
}

/// A part's instances with its rotation alone, for a part drawn in its own frame.
fn local(p: &Part) -> impl Iterator<Item = (&Instance, Mat)> {
    let mut m = p.transform;
    m[3] = [0.0, 0.0, 0.0, 1.0];
    p.instances.iter().map(move |i| (i, m))
}

/// The interior scene's mirror glass (the centre mirror and the driver's wing mirror) and the glass parts they become.
const MIRRORS: [(&str, &str); 2] = [("int_reflection_m", "i_mirrorglass_m"), ("int_reflection_r", "i_mirrorglass_r")];

/// The model's node of DiRT's needle `node`: `int_needle_tach` is `i_needle_tach` (an interior part).
fn needle_node(node: &str) -> String {
    format!("i_{}", node.strip_prefix("int_").unwrap_or(node))
}

/// DiRT's needle `needle` in the template's frame, on its model node: the mirror turns it the other way.
fn needle_in(needle: &Needle, frame: &Frame) -> Needle {
    Needle {
        node: needle_node(&needle.node),
        pivot: frame.point(needle.pivot),
        axis: Frame::direction(needle.axis),
        sweep: -needle.sweep,
        ..needle.clone()
    }
}

/// The cockpit's source: the interior scene (`int_cme.pssg`) and the exterior nodes its config keeps active.
struct Cockpit<'a> {
    scene: &'a [Part],
    /// The exterior parts the cockpit draws as well (`bonnet`, `wing_fl`), by name without `x0_`.
    active: Vec<String>,
    /// The scene's needles (`needles::read`, DiRT's frame): each is a piece of its own, which the renderer turns.
    needles: &'a [Needle],
    /// Where the scene is cut for the outside's copy.
    clip: Option<Clip>,
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

/// The cabin's top and half width in DiRT's frame: what of the interior scene lies above or beyond them is in or past
/// the body's paint (the cage's roof tubes, the lining at the roof and the sides).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Clip {
    roof: f32,
    side: f32,
}

/// How far past the glass the cabin's [`Clip`] lies.
const GLASS_MARGIN: f32 = 0.01;

/// The cabin's [`Clip`] from the car's glass, [`GLASS_MARGIN`] on: the windscreen's top (`window_f`; the rear window
/// can sit higher, the Escort's at 1.41, under the roof's slope) and the side windows' outer edge (the Escort: 1.40 and
/// 0.68, the values found by eye before they were measured); `None` for a car without them.
fn clip(parts: &[Part]) -> Option<Clip> {
    let glass = |wanted: &dyn Fn(&str) -> bool| -> Vec<[f32; 3]> {
        let panes = parts.iter().filter(|p| wanted(&p.name));
        let instances =
            panes.flat_map(|p| p.instances.iter().filter(|i| i.material == "Window Glass").map(|i| (i, p.transform)));
        instances.flat_map(|(i, m)| i.positions.iter().map(move |&q| mesh::point(&m, q))).collect()
    };
    let (screen, sides) =
        (glass(&|n| n == "window_f"), glass(&|n| SIDES.iter().any(|(s, _)| n == format!("window_{s}"))));
    let ((_, top), (lo, hi)) = (car::bounds(&screen), car::bounds(&sides));
    (!screen.is_empty() && !sides.is_empty())
        .then(|| Clip { roof: top[1] + GLASS_MARGIN, side: hi[0].max(-lo[0]) + GLASS_MARGIN })
}

/// `instance` without the triangles whose centre lies above the clip's roof or beyond its sides.
fn within_body(instance: &Instance, clip: Clip) -> Instance {
    let inside = |t: &&[u32; 3]| {
        let centre = |k: usize| t.iter().map(|&i| instance.positions[i as usize][k]).sum::<f32>() / 3.0;
        centre(1) <= clip.roof && centre(0).abs() <= clip.side
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
        out.push(Piece::upright(name, take));
    }
    out
}

/// What the pieces are cut from: the car's parts and its interior scene, placed by `frame`, drawn as their styles say.
struct Model<'a> {
    parts: &'a [Part],
    frame: Frame,
    exterior: &'a BTreeMap<String, Style>,
    interior: &'a BTreeMap<String, Style>,
    cockpit: Option<Cockpit<'a>>,
}

impl Model<'_> {
    fn part(&self, name: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.name == name)
    }

    fn pieces(&self) -> Vec<Piece> {
        [self.wheels(), self.windows(), self.lamps(), self.cabin(), self.steering_wheel(), self.body()]
            .into_iter()
            .flatten()
            .collect()
    }

    /// Each class's rim, tyre and the disc in the wheel's own frame, the node at the hub.
    fn wheels(&self) -> Vec<Piece> {
        let mut out = Vec::new();
        for (dirt, rbr) in SIDES {
            let parts: Vec<&Part> =
                ["wheel", "tyre", "disc"].iter().filter_map(|kind| self.part(&format!("{kind}_{dirt}"))).collect();
            if parts.len() < 3 {
                continue;
            }
            let hub = self.frame.point(translation(&parts[0].transform));
            for (class, _, title) in CLASSES {
                let instances = parts.iter().flat_map(|p| local(p));
                let faces = faces(instances, &Frame::LOCAL, self.exterior, |m| m.starts_with(title) || m == "Discs");
                out.push(Piece { name: format!("e_wheel_{class}_{rbr}"), at: hub, axes: UPRIGHT, faces });
            }
        }
        out
    }

    fn windows(&self) -> Vec<Piece> {
        let piece =
            |p: &Part| Some(Piece::upright(window(&p.name)?, faces(all(p), &self.frame, self.exterior, |_| true)));
        self.parts.iter().filter_map(piece).collect()
    }

    /// Each lens as an unlit and a lit part, which the lamp's switch box swaps (`ini_file`).
    fn lamps(&self) -> Vec<Piece> {
        let mut out: Vec<Piece> = Vec::new();
        for (dirt, rbr, states) in LAMPS {
            for (side, rbr_side) in SIDES {
                let Some(lens) = self.part(&format!("{dirt}_{side}")) else { continue };
                for suffix in states {
                    let name = format!("{rbr}{rbr_side}{suffix}");
                    if !out.iter().any(|p| p.name == name) {
                        out.push(Piece::upright(name, faces(all(lens), &self.frame, self.exterior, |_| true)));
                    }
                }
            }
        }
        out
    }

    /// The cabin, drawn in the cockpit (`i_internal`) and seen through the windows from outside (`e_internal`): the
    /// interior scene (dash, door cards, cage, seats, pedals, levers, gauges; not the wipers, whose rest pose the body's
    /// own show, nor the steering wheel) for both, and the exterior nodes the interior config keeps active (the bonnet and
    /// wings seen over the dash) in the cockpit. Only the interior scene is used where the car has one: the exterior
    /// file's cabin material is its low-detail copy. Without an interior scene it is the fallback for both.
    fn cabin(&self) -> Vec<Piece> {
        let Some(c) = &self.cockpit else {
            let has_cabin =
                |p: &&Part| p.name.starts_with("main_interior") || p.instances.iter().any(|i| i.material == "Cabin");
            let shell = || self.parts.iter().filter(has_cabin).filter(|p| p.name != "steering_wheel").flat_map(all);
            let cabin = |name| Piece::upright(name, faces(shell(), &self.frame, self.exterior, |m| m == "Cabin"));
            return vec![cabin("e_internal"), cabin("i_internal")];
        };
        let drawn: Vec<_> = c
            .scene
            .iter()
            .filter(|p| !p.name.starts_with("int_wiper") && p.name != "int_steering_wheel")
            .filter(|p| !c.needles.iter().any(|n| n.node == p.name))
            .filter(|p| !MIRRORS.iter().any(|(node, _)| *node == p.name))
            .flat_map(all)
            .collect();
        let mut out = split("i_internal", faces(drawn.iter().copied(), &self.frame, self.interior, |_| true));
        // Seen from outside, the cage's roof tubes and the lining at the roof and the sides lie in the body's paint
        // (DiRT never draws this scene outside) and show through it as grey bands along the roof edge.
        let clipped: Vec<_> =
            drawn.iter().map(|(i, m)| (c.clip.map_or_else(|| (*i).clone(), |clip| within_body(i, clip)), *m)).collect();
        out.extend(split(
            "e_internal",
            faces(clipped.iter().map(|(i, m)| (i, *m)), &self.frame, self.interior, |_| true),
        ));
        for (node, name) in MIRRORS {
            for p in c.scene.iter().filter(|p| p.name == node) {
                out.push(Piece::upright(name, faces(all(p), &self.frame, self.interior, |_| true)));
            }
        }
        for needle in c.needles {
            for p in c.scene.iter().filter(|p| p.name == needle.node) {
                out.push(Piece::upright(
                    needle_node(&needle.node),
                    faces(all(p), &self.frame, self.interior, |_| true),
                ));
            }
        }
        let near = self.parts.iter().filter(|p| c.active.contains(&p.name)).flat_map(all);
        out.extend(split("i_body", faces(near, &self.frame, self.exterior, |m| m != "Cabin" && m != "Window Glass")));
        out
    }

    /// The steering wheel, in its own frame: the node's z is the column, toward the driver (the template's convention,
    /// which the crew's hands and the wheel's turn use). The vertices are DiRT's with z negated (`faces` through
    /// [`Frame::LOCAL`]), the axes the part's turned the same way: the mirror once, so the triangles are flipped once. The
    /// wheel is the interior scene's own, taken into the exterior wheel's node, in both sets; the exterior file's
    /// where there is no interior scene.
    fn steering_wheel(&self) -> Vec<Piece> {
        let Some(p) = self.part("steering_wheel") else { return Vec::new() };
        let row = |k: usize| Frame::direction([p.transform[k][0], p.transform[k][1], p.transform[k][2]]);
        let axes = [row(0), row(1), row(2).map(|v| -v)];
        let hub = self.frame.point(translation(&p.transform));
        let own: Vec<Instance> = self
            .cockpit
            .iter()
            .flat_map(|c| c.scene.iter())
            .filter(|q| q.name == "int_steering_wheel")
            .flat_map(|q| q.instances.iter().map(|i| in_part_space(i, &p.transform)))
            .collect();
        let wheel = || {
            let (instances, styles) =
                if own.is_empty() { (&p.instances, self.exterior) } else { (&own, self.interior) };
            faces(instances.iter().map(|i| (i, mesh::IDENTITY)), &Frame::LOCAL, styles, |_| true)
        };
        ["i_steeringwheel", "e_steeringwheel"]
            .map(|name| Piece { name: name.to_owned(), at: hub, axes, faces: wheel() })
            .into()
    }

    /// Everything else outside.
    fn body(&self) -> Vec<Piece> {
        let is_wheel = |n: &str| n.starts_with("wheel_") || n.starts_with("tyre_") || n.starts_with("disc_");
        let body = self
            .parts
            .iter()
            .filter(|p| {
                !is_wheel(&p.name)
                    && window(&p.name).is_none()
                    && !lens(&p.name)
                    && !dropped(&p.name)
                    && p.name != "steering_wheel"
            })
            .flat_map(all);
        split("e_body", faces(body, &self.frame, self.exterior, |m| m != "Cabin" && m != "Window Glass"))
    }
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
    let (lo, hi) = car::bounds(&positions);
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

/// `<name>_shaders.ini`: every material by its key with its texture, and its normal map as the
/// `[<material>_bump_bitmap]` section the renderer's cars read (`rbr_formats::car_parts::bump_path`).
fn shaders_file(name: &str, styles: &BTreeMap<&str, &Style>) -> String {
    let mut out = String::from(
        "[Default Material]\nAmbient=0.500000 0.500000 0.500000\nDiffuse=0.500000 0.500000 0.500000\n\
         Specular=0.500000 0.500000 0.500000\nShininess=0.000000\nShineStrenth=0.000000\nTransparency=0.000000\n\
         Requirements=0\nDiffuseChannel=false\nOpacityChannel=false\nReflectionChannel=false\n\
         BumpChannel=false\nSelfIllumunationChannel=false\nSpecularColorChannel=false\n\n[TSDefault]\n",
    );
    let normal = |style: &Style| style.maps.normal.as_ref().map(Source::file);
    let files: BTreeSet<String> =
        styles.iter().flat_map(|(key, style)| [Some(file(key)), normal(style)]).flatten().collect();
    for f in &files {
        out += &format!("{f} = cars\\{name}\\textures\\{f}\n");
    }
    out += "\n[TextureSets]\nTextureSet0 = TSDefault\n\n[Textures]\n";
    for (i, f) in files.iter().enumerate() {
        out += &format!("Texture{i} = {f}\n");
    }
    for (key, style) in styles {
        let m = material_name(key);
        let bitmap = |kind: &str, f: &str, mono: bool| {
            format!(
                "\n[{m}_{kind}_bitmap]\nFilename={f}\nAmount=1.000000\nAlphaAsMono={mono}\nAlphaAsRGB=false\n\
                 TileU=1.000000\nTileV=1.000000\nOffsetU=0.000000\nOffsetV=0.000000\nWrapU=3\nWrapV=3\n"
            )
        };
        out += &bitmap("diffuse", &file(key), false);
        if style.opacity {
            out += &bitmap("opacity", &file(key), true);
        }
        if let Some(normal) = normal(style) {
            out += &bitmap("bump", &normal, false);
        }
        out += &format!(
            "\n[{m}]\nAmbient=0.500000 0.500000 0.500000\nDiffuse=0.500000 0.500000 0.500000\n\
             Specular=0.900000 0.900000 0.900000\nShininess=0.100000\nShineStrength=0.300000\nTransparency=0.000000\n\
             Requirements=33808\nDiffuseChannel=true\nDiffuseType=Bitmap\nOpacityChannel={}\n{}\
             ReflectionChannel=false\nSelfIlluminationChannel=false\nBumpChannel={}\nSpecularColorChannel=false\n",
            style.opacity,
            if style.opacity { "OpacityType=Bitmap\n" } else { "" },
            style.maps.normal.is_some(),
        );
    }
    out
}

/// The car's files in the DiRT install, and the textures read from them (each file read once).
struct Library {
    data: PathBuf,
    car: String,
    /// The class's folder under `cars/generics`.
    generics: PathBuf,
    livery: u32,
    files: RefCell<BTreeMap<PathBuf, Rc<Vec<Texture>>>>,
}

impl Library {
    fn new(data: &Path, options: &Options) -> Result<Library, String> {
        let generics = data.join("cars/generics").join(&options.class);
        if !generics.is_dir() {
            return Err(format!("{}: no such class", generics.display()));
        }
        let folder = data.join("cars/models").join(&options.car);
        let livery = match options.livery {
            Some(livery) => livery,
            None => {
                let entries = fs::read_dir(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
                let numbers =
                    entries.filter_map(|e| e.ok()?.file_name().to_str()?.strip_prefix("livery_")?.parse().ok());
                numbers.min().ok_or_else(|| format!("{}: no livery", folder.display()))?
            }
        };
        Ok(Library { data: data.to_owned(), car: options.car.clone(), generics, livery, files: RefCell::default() })
    }

    /// `cars/models/<car>/<name>`.
    fn car_file(&self, name: &str) -> PathBuf {
        self.data.join("cars/models").join(&self.car).join(name)
    }

    /// `cars/interiors/models/<car>/int_<car>.pssg`.
    fn interior_file(&self) -> PathBuf {
        self.data.join(format!("cars/interiors/models/{0}/int_{0}.pssg", self.car))
    }

    fn find(&self, source: &Source) -> Result<Texture, String> {
        let (car, n) = (&self.car, self.livery);
        let livery =
            |variant: &str| self.car_file(&format!("livery_{n:02}/textures_high/{car}_tex{variant}_high_{n:02}.pssg"));
        let (path, name) = match source {
            Source::Livery(name) => (livery(""), name),
            Source::LiveryClass(short, name) => (livery(&format!("_{short}")), name),
            Source::Generic(short, name) => (self.generics.join(format!("car_gen_{short}_high.pssg")), name),
            Source::Interior(name) => (self.interior_file(), name),
            Source::Gear => return Err("the gear display's base is made, not read".into()),
        };
        let cached = self.files.borrow().get(&path).cloned();
        let textures = match cached {
            Some(textures) => textures,
            None => {
                let textures = Rc::new(texture::read_car(&Pssg::read(&path)?));
                self.files.borrow_mut().insert(path.clone(), textures.clone());
                textures
            }
        };
        let found = textures.iter().find(|t| t.name.eq_ignore_ascii_case(name));
        found.cloned().ok_or_else(|| format!("{}: no texture {name}", path.display()))
    }

    /// A style's texture as the `.dds` the car reads: with the occlusion baked in and the specular as its alpha where
    /// its maps have them, else its blocks as they are.
    fn diffuse(&self, style: &Style) -> Result<Vec<u8>, String> {
        if style.texture == Source::Gear {
            return Ok(gauges::gear_base());
        }
        let texture = self.find(&style.texture)?;
        let plane = |map: &Option<(Source, usize, f32)>| {
            map.as_ref()
                .map(|(source, channel, value)| Ok::<_, String>((Plane::of(&self.find(source)?, *channel), *value)))
        };
        match (plane(&style.maps.occlusion).transpose()?, plane(&style.maps.specular).transpose()?) {
            (None, None) => Ok(texture.dds()),
            (occlusion, specular) => Ok(car_maps::diffuse(&texture, occlusion.as_ref(), specular.as_ref())),
        }
    }
}

/// A template's wheel nodes closer than this, m, are at one point: a class the model left stacked.
const STACKED: f32 = 0.2;

/// The template car's model and ini, and its wheels' hubs at rest.
struct Template {
    folder: PathBuf,
    name: String,
    sgc: Vec<u8>,
    ini: String,
    hubs: [[f32; 3]; 4],
}

impl Template {
    fn read(model: &Path) -> Result<Template, String> {
        let name = model.file_stem().and_then(|n| n.to_str()).ok_or("the template's model has no name")?.to_owned();
        let folder = model.parent().ok_or("the template's model has no folder")?;
        let read = |suffix: &str| {
            let path = folder.join(format!("{name}{suffix}"));
            fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
        };
        let sgc = read(".sgc")?;
        let model = sgc::parse(&sgc).map_err(|e| format!("template model: {e}"))?;
        let worlds = sgc::chained_worlds(&model);
        let hub = |class: &str, side: &str| {
            let name = format!("e_wheel_{class}_{side}");
            model.iter().find_map(|e| match e {
                sgc::Element::Mesh(m) if m.node.name == name => {
                    worlds.get(&m.node.id).map(|w| [w[0][3], w[1][3], w[2][3]])
                }
                _ => None,
            })
        };
        // Where the renderer draws the wheels at rest (`rbr-render`'s `car::wheels`): the first class whose four are
        // apart; RSF's Polo WRC 2016, i20s and others stack the tarmac ones at one point.
        let apart = |hubs: &[[f32; 3]; 4]| {
            (0..4).all(|i| {
                (i + 1..4).all(|j| (0..3).map(|a| (hubs[i][a] - hubs[j][a]).powi(2)).sum::<f32>() > STACKED.powi(2))
            })
        };
        let hubs = ["tarmac", "gravel", "snow"]
            .into_iter()
            .filter_map(|class| {
                let [lf, rf, lb, rb] = SIDES.map(|(_, side)| hub(class, side));
                Some([lf?, rf?, lb?, rb?])
            })
            .find(apart)
            .ok_or("the template has no four wheels apart (e_wheel_tarmac_lf ...)")?;
        let ini = String::from_utf8_lossy(&read(".ini")?).into_owned();
        Ok(Template { folder: folder.to_owned(), name, sgc, ini, hubs })
    }
}

/// Converts the car and writes `<out>/Cars/<name>/` (and the gauges' pack, for a car whose gauges are measured);
/// returns the model's `.sgc` path.
pub fn convert(data: &Path, options: &Options, out: &Path) -> Result<PathBuf, String> {
    let library = Library::new(data, options)?;
    let car = options.car.as_str();
    let pssg = Pssg::read(&library.car_file(&format!("{car}_highlod.pssg")))?;
    let parts = car::parts(&pssg)?;
    let template = Template::read(&options.template)?;

    // The interior scene and its config, where the car has them (its cockpit is then DiRT's own).
    let interior = match library.interior_file() {
        path if path.exists() => Some(Pssg::read(&path)?),
        _ => None,
    };
    let scene = interior.as_ref().map(car::interior).transpose()?;
    let config = fs::read_to_string(library.interior_file().with_extension("xml")).unwrap_or_default();
    let gauges = gauges::layout(car).filter(|_| scene.is_some());
    let exterior: BTreeMap<String, Style> =
        material::read(&pssg).values().filter_map(|m| Some((m.id.clone(), exterior_style(car, m)?))).collect();
    let inside =
        interior.as_ref().map_or_else(BTreeMap::new, |p| interior_styles(car, &material::read(p), gauges.is_some()));
    let needles = interior.as_ref().map(|p| needles::read(p, &config)).unwrap_or_default();
    let model = Model {
        parts: &parts,
        frame: Frame::fitted(&parts, &template.hubs)?,
        exterior: &exterior,
        interior: &inside,
        cockpit: scene.as_deref().map(|scene| Cockpit {
            scene,
            active: active_nodes(&config),
            needles: &needles,
            clip: clip(&parts),
        }),
    };
    let pieces = model.pieces();

    let folder = out.join("Cars").join(&options.name);
    let textures = folder.join("textures");
    fs::create_dir_all(&textures).map_err(|e| format!("{}: {e}", textures.display()))?;
    let write = |path: PathBuf, bytes: &[u8]| fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()));
    let used: BTreeSet<&str> = pieces.iter().flat_map(|p| p.faces.iter().map(|f| f.material.as_str())).collect();
    let styles: BTreeMap<&str, &Style> = exterior
        .values()
        .chain(inside.values())
        .filter(|s| used.contains(s.key.as_str()))
        .map(|s| (s.key.as_str(), s))
        .collect();
    for (key, style) in &styles {
        write(textures.join(file(key)), &library.diffuse(style)?)?;
        if let Some(emissive) = &style.maps.emissive {
            write(textures.join(glow_file(key)), &library.find(emissive)?.dds())?;
        }
    }
    let normals: BTreeMap<&Source, bool> =
        styles.values().filter_map(|s| Some((s.maps.normal.as_ref()?, s.maps.crumpled))).collect();
    for (source, crumpled) in normals {
        write(textures.join(source.file()), &car_maps::normal(&library.find(source)?, crumpled))?;
    }
    if let Some(layout) = gauges {
        let find = |name: &str| library.find(&Source::Interior(name.to_owned()));
        let sources = gauges::Sources {
            dials: &find(&format!("int_{car}_dials.tga"))?,
            dials_on: &find(&format!("int_{car}_dials_on.tga"))?,
            indicator: &find("int_indicator.tga")?,
            indicator_on: &find("int_indicator_on.tga")?,
            digits: &find("int_digi_speed.tga")?,
            gear_font: &gauges::gear_font(data, &config)?,
            config: &config,
        };
        gauges::write(out, &options.name, layout, &sources)?;
    }

    let name = &options.name;
    let model_path = folder.join(format!("{name}.sgc"));
    write(model_path.clone(), &sgc_file(&template.sgc, &pieces)?)?;
    let cameras = library.car_file("cameras.xml");
    let cameras = binxml::parse(&fs::read(&cameras).map_err(|e| format!("{}: {e}", cameras.display()))?)?;
    write(
        folder.join(format!("{name}.ini")),
        ini_file(&template.ini, &pieces, &eyes(&cameras, &model.frame)).as_bytes(),
    )?;
    write(folder.join(format!("{name}_shaders.ini")), shaders_file(name, &styles).as_bytes())?;
    let turned: Vec<Needle> = needles.iter().map(|n| needle_in(n, &model.frame)).collect();
    if !turned.is_empty() {
        write(folder.join(car_gauges::file_name(name)), car_gauges::write(&turned).as_bytes())?;
    }
    for suffix in ["_settings.ini", "_shader_settings_clear.ini", "_shader_settings_overcast.ini"] {
        let from = template.folder.join(format!("{}{suffix}", template.name));
        let text = fs::read(&from).map_err(|e| format!("{}: {e}", from.display()))?;
        write(folder.join(format!("{name}{suffix}")), &text)?;
    }
    night_lights(&template, &folder, name)?;
    Ok(model_path)
}

/// The option folder of the night's lights beside the model (`light_on/`, a bare option the renderer applies at night,
/// `rbr_formats::rsf::night_options`).
const NIGHT: &str = "light_on";

/// Writes the template's night lights as the model's [`NIGHT`] option: the template's own night options (headlights on,
/// pods and extra lights on) merged over its files, of which the lamp points and the light records. Without them a
/// template whose `.ini` lacks the lamp points (the gvrc Mini WRC's are only in its options) lit nothing at night. The
/// options' other sections name the template's own meshes and cameras, which the model's replace, and are left out.
fn night_lights(template: &Template, folder: &Path, name: &str) -> Result<(), String> {
    let pick = rsf::night_options(&template.folder);
    if pick.options().is_empty() {
        return Ok(());
    }
    let parse =
        |text: &str, file: &str| ini::parse(text).map_err(|e| format!("{file}: line {}: {}", e.line, e.message));
    let options = |dirs: &[PathBuf]| dirs.iter().map(|d| rsf::read_option_files(d)).collect::<Result<Vec<_>, _>>();
    let settings_file = template.folder.join(format!("{}_settings.ini", template.name));
    let bytes = fs::read(&settings_file).map_err(|e| format!("{}: {e}", settings_file.display()))?;
    let mut parts = parse(&template.ini, &template.name)?;
    let mut settings = parse(&String::from_utf8_lossy(&bytes), &settings_file.display().to_string())?;
    // Each option over the model's files as RSF's picker copies them, the last value of a key kept (the gvrc Escort's
    // `light_ramp_on` gives `[e_lightglass_rf]` twice; the renderer's checked merge leaves such an option out).
    for option in options(&pick.options())? {
        parts = rsf::overlay_sections(&parts, &option.parts);
        settings = rsf::overlay_sections(&settings, &option.settings);
    }
    let text = |sections: &mut dyn Iterator<Item = &ini::Section>| {
        sections
            .map(|s| s.entries.iter().fold(format!("[{}]\r\n", s.name), |t, (k, v)| t + &format!("{k} = {v}\r\n")))
            .collect::<Vec<_>>()
            .join("\r\n")
    };
    let out = folder.join(NIGHT);
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let points = &mut parts.iter().filter(|s| s.name.to_ascii_lowercase().starts_with("point_"));
    for (file, body) in
        [(format!("{name}.ini"), text(points)), (format!("{name}_settings.ini"), text(&mut settings.iter()))]
    {
        let path = out.join(file);
        fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
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

    /// A material of shader `shader` with `textures` bound (input, file) and `constants`.
    fn material(id: &str, shader: &str, textures: &[(&str, &str)], constants: &[(&str, f32)]) -> Material {
        Material {
            id: id.to_owned(),
            shader: shader.to_owned(),
            textures: textures.iter().map(|(k, v)| (k.to_string(), format!("#{v}"))).collect(),
            constants: constants.iter().map(|(k, v)| (k.to_string(), vec![*v])).collect(),
        }
    }

    fn exterior(id: &str) -> Option<Style> {
        exterior_style("cme", &material(id, "car.fx", &[], &[("OcclusionMapScale", 0.4)]))
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
        let class = |m: &str| exterior(m).map(|s| (s.shader, s.key));
        assert_eq!(class("Car Body Paint"), Some(("CarPaint", "main40".to_owned())));
        assert_eq!(class("Matt"), Some(("CarTextureNoDirt", "main40".to_owned())));
        // Bare metal reflects as the paint does (DiRT gives both `carpaint_metallic.fx`).
        assert_eq!(class("Shiny Metal").map(|c| c.0), Some("CarPaint"));
        assert_eq!(class("Gravel Wheel"), Some(("CarRim", "wheel_gravel".to_owned())));
        assert_eq!(class("Snow Tyre Tread"), Some(("CarTire", "tyre_snow".to_owned())));
        // The sidewall is on the wheel's atlas (one texture with the rim's), the tread has its own texture.
        assert_eq!(class("Tarmac Tyre Wall"), Some(("CarTire", "wheel_tarmac".to_owned())));
        let texture = |m: &str| exterior(m).map(|s| s.texture);
        assert_eq!(texture("Tarmac Tyre Wall"), Some(Source::LiveryClass("tm", "cme_wheel.tga".to_owned())));
        assert_eq!(texture("Tarmac Tyre Tread"), Some(Source::Generic("tm", "tread.tga".to_owned())));
        assert_eq!(texture("Caliper"), Some(Source::Livery("caliper.tga".to_owned())));
        // The spare is drawn once, as the tarmac wheel.
        assert_eq!(class("Spare Tarmac Wheel"), class("Tarmac Wheel"));
        assert_eq!(class("Spare Gravel Tyre Wall"), None);
        assert_eq!(class("Nothing"), None);
        assert_eq!(window("window_fl").as_deref(), Some("e_window_lf"));
        assert_eq!(window("window_f_trim"), None);
    }

    #[test]
    fn the_bodys_maps_take_the_materials_own_occlusion() {
        let body = |id: &str, scale: f32| {
            exterior_style("cme", &material(id, "car.fx", &[], &[("OcclusionMapScale", scale)])).unwrap()
        };
        for (id, scale) in [("Car Body Paint", 0.4), ("Matt", 0.8), ("Shiny Metal", 0.5)] {
            let style = body(id, scale);
            assert_eq!(style.texture, Source::Livery("cme_main.tga".to_owned()), "{id}");
            assert_eq!(style.maps.normal, Some(Source::Livery("cme_main_nm.tga".to_owned())), "{id}");
            assert!(style.maps.crumpled, "{id}");
            assert_eq!(style.maps.occlusion, Some((Source::Livery("cme_specocc.tga".to_owned()), 3, scale)), "{id}");
            assert_eq!(style.maps.specular, Some((Source::Livery("cme_specocc.tga".to_owned()), 1, FULL_SPECULAR)));
        }
        // One texture per strength, whatever the class.
        assert_eq!(body("Matt", 0.8).key, "main80");
        assert_eq!(body("Mud Flaps", 0.8).key, body("Matt", 0.8).key);
        assert_ne!(body("Car Body Paint", 0.4).key, body("Shiny Metal", 0.5).key);
        // The lens's alpha is its opacity, so it takes no specular, and the lit lamps' colours no occlusion.
        let lens = exterior("Lights Glass").unwrap();
        assert!(lens.opacity && lens.maps.normal.is_some() && lens.maps.specular.is_none());
        let pod = exterior("Lights Pod").unwrap().maps;
        assert!(pod.normal.is_some() && pod.specular.is_some() && pod.occlusion.is_none());
        assert!(exterior("Window Glass").unwrap().maps.normal.is_none());
        assert!(exterior("Cabin").is_some_and(|s| s.maps.normal.is_none()));
    }

    #[test]
    fn a_maps_file_names_its_source_once() {
        assert_eq!(Source::Livery("cme_main_nm.tga".into()).file(), "dr_cme_main_nm.dds");
        assert_eq!(Source::LiveryClass("tm", "cme_wheel_nm.tga".into()).file(), "dr_cme_wheel_nm_tm.dds");
        assert_eq!(Source::Generic("gr", "tread_nm.tga".into()).file(), "dr_tread_nm_gr.dds");
        assert_eq!(Source::Generic("main", "grills_nm.tga".into()).file(), "dr_grills_nm.dds");
        assert_eq!(Source::Interior("int_405_nm.TGA".into()).file(), "dr_int_405_nm.dds");
    }

    #[test]
    fn the_shaders_file_names_each_normal_map_as_a_bump_bitmap() {
        let paint = exterior("Car Body Paint").unwrap();
        let disc = Style::new("CarTextureNoDirt", "disc", Source::Livery("disc.tga".into()));
        let styles = BTreeMap::from([("paint", &paint), ("disc", &disc)]);
        let text = shaders_file("Car", &styles);
        let sections = rbr_formats::ini::parse(&text).unwrap();
        let bump = rbr_formats::car_parts::bump_path;
        assert_eq!(bump(&sections, &material_name("paint")), Some("cars\\Car\\textures\\dr_cme_main_nm.dds"));
        assert_eq!(bump(&sections, &material_name("disc")), None);
        assert!(text.contains("BumpChannel=true") && text.contains("BumpChannel=false"));
    }

    #[test]
    fn lamp_parts_name_their_switch_boxes() {
        assert_eq!(lamp("e_light_lf"), Some(("light_lf".to_owned(), false)));
        assert_eq!(lamp("e_light_brake_lb_lit"), Some(("brake_lb".to_owned(), true)));
        assert_eq!(lamp("e_light_reverse_rb"), Some(("reverse_rb".to_owned(), false)));
        assert_eq!(lamp("e_light_rig_rf_lit"), Some(("light_rf".to_owned(), true)));
        assert_eq!(lamp("e_light_2_lb"), Some(("light_lb".to_owned(), false)));
        assert_eq!(lamp("e_body"), None);
        assert!([
            "light_glass_fl",
            "light_combi_glass_br",
            "light_rev_glass_bl",
            "light_2_glass_bl",
            "light_rig_glass_fl"
        ]
        .iter()
        .all(|p| lens(p)));
        assert!(!["light_rig_pod_fl", "light_combi_pod_bl", "light_glass_fl_x", "light_2_brake_pod_bl"]
            .iter()
            .any(|p| lens(p)));
    }

    fn instance(material: &str, positions: Vec<[f32; 3]>, triangles: Vec<[u32; 3]>) -> Instance {
        Instance {
            material: material.into(),
            joint: None,
            source: "src".into(),
            normals: vec![[0.0, 0.0, 1.0]; positions.len()],
            uv: vec![[0.0, 0.0]; positions.len()],
            positions,
            triangles,
        }
    }

    fn part(name: &str, instances: Vec<Instance>) -> Part {
        Part { name: name.to_owned(), transform: mesh::IDENTITY, instances }
    }

    #[test]
    fn a_separate_brake_lens_takes_the_brake_lamp_from_the_combined_one() {
        let lens = |name: &str| part(name, vec![instance("Lights Glass", vec![[0.0; 3]; 3], vec![[0, 1, 2]])]);
        let parts = [lens("light_combi_glass_bl"), lens("light_brake_glass_bl")];
        let exterior = BTreeMap::from([("Lights Glass".to_owned(), exterior("Lights Glass").unwrap())]);
        let model = Model {
            parts: &parts,
            frame: Frame::LOCAL,
            exterior: &exterior,
            interior: &BTreeMap::new(),
            cockpit: None,
        };
        let names: Vec<String> = model.lamps().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["e_light_brake_lb", "e_light_brake_lb_lit", "e_light_lb", "e_light_lb_lit"]);
    }

    #[test]
    fn the_interior_draws_what_its_materials_bind() {
        let atlas = [
            ("TDiffuseAlphaMap", "int_cme_main.tga"),
            ("TNormalMap", "int_cme_nm.tga"),
            ("TOcclusionMap", "int_cme_occ.tga"),
            ("TSpecularMap", "int_cme_sp.tga"),
        ];
        let style = |m: &Material| interior_style("cme", m, true);
        let leather = style(&material("Leather", "interior.fx", &atlas, &[("OcclusionMapScale", 1.0)])).unwrap();
        assert_eq!((leather.shader, leather.key.as_str(), leather.opacity), ("CarTextureNoDirt", "int_main", false));
        assert_eq!(leather.maps.normal, Some(Source::Interior("int_cme_nm.tga".into())));
        assert_eq!(leather.maps.occlusion, Some((Source::Interior("int_cme_occ.tga".into()), 0, INTERIOR_OCCLUSION)));
        // An `OcclusionMapScale` of 0 turns the occlusion off; the shared 32 texel specular is not the atlas's.
        let chrome = style(&material("Chrome", "interior.fx", &atlas, &[("OcclusionMapScale", 0.0)])).unwrap();
        assert!(chrome.key == "int_main" && chrome.maps.occlusion.is_none() && chrome.maps.normal.is_some());
        let dials = [("TDiffuseAlphaMap", "int_cme_dials.TGA"), ("TSpecularMap", "int_gen_sp.tga")];
        let dial = style(&material("dials matt", "interior.fx", &dials, &[])).unwrap();
        assert!(dial.key == "int_dials" && dial.maps == Maps::default());
        // The alpha shader's alpha is the opacity: a key of its own, no specular.
        let alpha = style(&material("dials matt", "interior_alpha.fx", &dials, &[])).unwrap();
        assert!(alpha.key == "int_dials_alpha" && alpha.opacity && alpha.maps.specular.is_none());
        let carbon = [("TCarbonDiffuseMap", "int_carbon_diff.tga"), ("TOcclusionMap", "int_cme_occ.tga")];
        let weave =
            style(&material("carbon fibre", "interior_carbon.fx", &carbon, &[("CarbonUVRepeat", 300.0)])).unwrap();
        assert!(weave.key == "int_carbon_diff" && weave.repeat == 300.0 && weave.maps.occlusion.is_none());
        assert_eq!(style(&material("mirror", "mirror.fx", &[], &[])).map(|s| s.shader), Some("CarGlass"));
        assert!(style(&material("interior glass windows", "interior_windscreen.fx", &[], &[])).is_none());
        assert!(style(&material("digital speedometer", "lcd_text.fx", &[], &[])).is_none());
        // The gear display is drawn only for a car whose gauges are measured.
        let gear = material("gear indicator", "lcd_text.fx", &[], &[]);
        assert_eq!(style(&gear).map(|s| (s.key, s.texture)), Some(("int_gear".to_owned(), Source::Gear)));
        assert!(interior_style("cme", &gear, false).is_none());
    }

    #[test]
    fn an_atlas_takes_every_map_its_materials_give_it() {
        let atlas = [("TDiffuseAlphaMap", "int_cme_main.tga"), ("TOcclusionMap", "int_cme_occ.tga")];
        let materials = BTreeMap::from([
            ("Chrome".to_owned(), material("Chrome", "interior.fx", &atlas, &[("OcclusionMapScale", 0.0)])),
            ("Matt".to_owned(), material("Matt", "interior.fx", &atlas, &[("OcclusionMapScale", 1.0)])),
        ]);
        let styles = interior_styles("cme", &materials, false);
        assert_eq!(styles["Chrome"], styles["Matt"]);
        assert!(styles["Chrome"].maps.occlusion.is_some());
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
        let local = in_part_space(&instance("Matt", vec![[1.0, 2.0, 3.0], [1.0, 2.0, 2.0]], vec![[0, 1, 0]]), &m);
        assert_eq!(local.positions, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        // Back through the part's own matrix.
        assert_eq!(mesh::point(&m, local.positions[1]), [1.0, 2.0, 2.0]);
        assert_eq!(local.normals[0], [-1.0, 0.0, 0.0], "the car's z is the part's -x");
    }

    #[test]
    fn the_roof_and_sides_of_the_glass_clip_the_outside_cabin() {
        // A windscreen up to y 1.39 (its part's matrix moves it up), side glass out to x 0.67; the rear window and the
        // paint above them do not count.
        let pane = |name: &str, material: &str, positions| part(name, vec![instance(material, positions, vec![])]);
        let mut screen = pane("window_f", "Window Glass", vec![[-0.6, 1.0, 0.4], [0.6, 1.29, 0.4]]);
        screen.transform[3][1] = 0.1;
        let side = pane("window_bl", "Window Glass", vec![[0.67, 1.0, -0.5]]);
        let rear = pane("window_b", "Window Glass", vec![[0.0, 1.41, -1.0], [0.8, 1.2, -1.0]]);
        let paint = pane("roof", "Car Body Paint", vec![[0.0, 1.5, 0.0]]);
        let clip = clip(&[screen.clone(), side, rear, paint]).unwrap();
        assert!((clip.roof - 1.40).abs() < 1e-5 && (clip.side - 0.68).abs() < 1e-5, "{clip:?}");
        assert_eq!(super::clip(&[screen]), None, "no side glass");

        let tri = |dx: f32, y: f32| [[dx, y, 0.0], [dx + 0.1, y, 0.0], [dx, y + 0.1, 0.0]];
        let triangles = [tri(0.0, 1.0), tri(0.0, 1.45), tri(0.7, 1.0), tri(-0.4, 0.5)];
        let positions: Vec<[f32; 3]> = triangles.iter().flatten().copied().collect();
        let all: Vec<[u32; 3]> = (0..4).map(|k| [3 * k, 3 * k + 1, 3 * k + 2]).collect();
        let kept = within_body(&instance("Matt", positions, all), clip);
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

    #[test]
    fn the_templates_night_option_is_written_with_its_points_and_light_records() {
        let root = std::env::temp_dir().join(format!("rbr-dirt-rally-night-{}", std::process::id()));
        let (template, option, out) = (root.join("mini"), root.join("mini/lights on"), root.join("out"));
        for dir in [&option, &out] {
            fs::create_dir_all(dir).unwrap();
        }
        let write = |path: PathBuf, text: &str| fs::write(path, text).unwrap();
        write(template.join("mini_settings.ini"), "[light_1]\r\nRange = 10\r\n");
        write(option.join("mini.ini"), "[point_light_lf]\r\nPos = 1 2 3\r\n[e_body]\r\nSwitch = true\r\n");
        write(option.join("mini_settings.ini"), "[light_1]\r\nRange = 80\r\n[light_2]\r\nRange = 40\r\n");
        let template = Template {
            folder: template,
            name: "mini".into(),
            sgc: Vec::new(),
            ini: "[point_tail]\r\nPos = 0 0 0\r\n".into(),
            hubs: [[0.0; 3]; 4],
        };
        night_lights(&template, &out, "DiRT_mcr").unwrap();
        let read = |file: &str| fs::read_to_string(out.join(NIGHT).join(file)).unwrap();
        // The template's points and the option's, not its mesh sections; the option's light records over the template's.
        assert_eq!(read("DiRT_mcr.ini"), "[point_tail]\r\nPos = 0 0 0\r\n\r\n[point_light_lf]\r\nPos = 1 2 3\r\n");
        assert_eq!(read("DiRT_mcr_settings.ini"), "[light_1]\r\nRange = 80\r\n\r\n[light_2]\r\nRange = 40\r\n");
        fs::remove_dir_all(root).unwrap();
    }
}
