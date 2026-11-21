//! What CSP's `extension/*.ini` files do to a track's meshes: hide them. Only unconditional changes are applied.
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use rbr_btb::scene::Grass;

/// An ini value without its comment: CSP takes `;` and `//` (Embrun–Prunières' `PIECE_0 = 1,1 // daisy`).
fn uncommented(value: &str) -> &str {
    let end = [value.find(';'), value.find("//")].into_iter().flatten().min().unwrap_or(value.len());
    value[..end].trim()
}

/// The text of each `extension/*.ini` file, in order.
pub(super) fn inis(root: &Path) -> Vec<String> {
    let Ok(dir) = super::stage::source_path(root, "extension") else { return Vec::new() };
    let mut inis: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("ini")))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .collect();
    inis.sort();
    inis
}

/// The meshes CSP hides from each model (`[MODEL_REPLACEMENT_<n>]` `FILE` and `HIDE`), keyed by the lowercase file name:
/// a track with a tree list replaces its drawn trees and hedges with them.
pub fn replaced(root: &Path) -> BTreeMap<String, HashSet<String>> {
    let mut hidden = BTreeMap::<String, HashSet<String>>::new();
    for text in inis(root) {
        let (mut file, mut hide, mut active) = (String::new(), Vec::new(), false);
        let mut flush = |file: &mut String, hide: &mut Vec<String>, active: bool| {
            if active && !file.is_empty() {
                hidden.entry(std::mem::take(file).to_lowercase()).or_default().extend(hide.drain(..));
            }
            file.clear();
            hide.clear();
        };
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                flush(&mut file, &mut hide, active);
                active = line.to_uppercase().starts_with("[MODEL_REPLACEMENT");
            } else if let Some((key, value)) = line.split_once('=').filter(|_| active) {
                let value = uncommented(value);
                match key.trim().to_uppercase().as_str() {
                    "FILE" => file = value.to_owned(),
                    "HIDE" => hide.extend(value.split(',').map(|n| n.trim().to_lowercase()).filter(|n| !n.is_empty())),
                    _ => {}
                }
            }
        }
        flush(&mut file, &mut hide, active);
    }
    hidden
}

/// The names (lowercase, `?` and `*` wildcards) of the meshes `[MESH_ADJUSTMENT_<n>]` makes unrenderable (`IS_RENDERABLE = 0`,
/// `ACTIVE` and `CONDITION` unset or always on): Thizy's night window glows.
pub fn unrendered(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for text in inis(root) {
        let (mut meshes, mut hide, mut active, mut always) = (Vec::new(), false, true, true);
        let mut flush = |meshes: &mut Vec<String>, hide: bool, active: bool, always: bool| {
            if hide && active && always {
                names.append(meshes);
            }
            meshes.clear();
        };
        let mut section = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                flush(&mut meshes, hide, active, always);
                let upper = line.to_uppercase();
                section = upper.starts_with("[MESH_ADJUSTMENT") || upper.starts_with("[MESH_ADJUSTEMENT");
                (hide, active, always) = (false, true, true);
            } else if let Some((key, value)) = line.split_once('=').filter(|_| section) {
                let value = uncommented(value);
                match key.trim().to_uppercase().as_str() {
                    "MESHES" => meshes.extend(
                        value.split(',').map(|n| n.trim().to_lowercase()).filter(|n| !n.is_empty() && !n.contains(':')),
                    ),
                    "IS_RENDERABLE" => hide = value == "0",
                    "ACTIVE" => active = value != "0",
                    "CONDITION" => always = value.eq_ignore_ascii_case("ALWAYS_ON"),
                    _ => {}
                }
            }
        }
        flush(&mut meshes, hide, active, always);
    }
    names
}

/// CSP's `MASK_MAIN_THRESHOLD` and `MASK_RED_THRESHOLD` where a track gives none (CSP's GrassFX documentation).
const MASK_DEFAULT: [f32; 2] = [0.5, 0.05];

/// CSP's GrassFX (`[GRASS_FX]` and `[GRASS_FX_TEXTURE_GROUP_<n>]`) as far as ReRBR's grass uses it: the materials it grows
/// on and those it hides under, its atlas and the pieces drawn from it, and the luminance range of the ground it grows on;
/// with its configurations (`[GRASS_FX_CONFIGURATION_A]` to `_D`) and the adjustments that blend them in on some
/// materials and meshes (`[GRASS_FX_ADJUSTMENT_...]`), as profiles ([`GrassFx::profile`]).
#[derive(Debug, Default)]
pub struct GrassFx {
    /// `GRASS_MATERIALS`, lowercase, with `?` and `*` wildcards.
    pub materials: Vec<String>,
    /// `OCCLUDING_MATERIALS` and `OCCLUDING_MATERIALS_ALPHA` alike: the grass grows nowhere under them (the second:
    /// under their opaque texels).
    pub occluding: [Vec<String>; 2],
    /// `TEXTURE` where it names a file of the track's own `extension` folder; `None` for CSP's own atlases and no key.
    pub texture: Option<std::path::PathBuf>,
    keys: BTreeMap<String, String>,
    groups: BTreeMap<u32, BTreeMap<String, String>>,
    /// `[GRASS_FX_CONFIGURATION_A]` to `_D`'s keys.
    configurations: [BTreeMap<String, String>; 4],
    /// Each `[GRASS_FX_ADJUSTMENT_...]` in order.
    adjustments: Vec<Adjustment>,
    /// The adjustments' distinct maps, profile 1 onwards ([`GrassFx::profile`]).
    maps: Vec<[f32; 4]>,
}

/// A `[GRASS_FX_ADJUSTMENT_...]`: its materials and meshes (lowercase, wildcards; a mesh takes it when it matches
/// either: Joux Verte's first names its talus meshes along with a material none of them has), its rectangle, and its
/// `MAP`, the intensity of configurations A to D (`MAP = A`, `MAP = A, 0.5` or `MAP = 0.8, 0.5, 0.2, 0.0`; `EXTRA_<n>`
/// likewise). One with a local area (`@ = GrassFX_ExtraArea...`) or `ACTIVE = 0` is left out.
#[derive(Debug, Default)]
struct Adjustment {
    materials: Vec<String>,
    meshes: Vec<String>,
    area: Area,
    map: [f32; 4],
}

/// An adjustment's rectangle on the ground, in AC's world metres: `CENTER` (x, z, or x, y, z) and `SIZE` (the same),
/// turned `ANGLE` degrees about the vertical; ours keeps its whole size and leaves out its `FADE`. Akina's covers the
/// track. No `SIZE`: everywhere.
#[derive(Debug, Default)]
struct Area {
    centre: [f32; 2],
    size: Option<[f32; 2]>,
    angle: f32,
}

impl Area {
    /// A key's x and z: its first and last of two or three numbers.
    fn xz(value: &str) -> Option<[f32; 2]> {
        let values: Vec<f32> = value.split(',').map(|v| v.trim().parse().ok()).collect::<Option<_>>()?;
        matches!(values.len(), 2 | 3).then(|| [values[0], values[values.len() - 1]])
    }

    fn contains(&self, [x, z]: [f32; 2]) -> bool {
        let Some([width, depth]) = self.size else { return true };
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let (dx, dz) = (x - self.centre[0], z - self.centre[1]);
        let (u, v) = (dx * cos + dz * sin, -dx * sin + dz * cos);
        u.abs() <= width * 0.5 && v.abs() <= depth * 0.5
    }
}

/// A grass profile's numbers ([`GrassFx::shape`]).
struct Shape {
    base: f32,
    size: Option<f32>,
    luminance: Option<[f32; 2]>,
    mask: [f32; 2],
    groups: Vec<f32>,
}

impl Shape {
    /// Its `[ReRbrGrass]` keys but the groups' pieces: `base`, `size`, `luminance`, `mask` and `group<n>`.
    fn keys(&self) -> String {
        let mut out = format!("base={}\n", self.base);
        if let Some(size) = self.size {
            out += &format!("size={size}\n");
        }
        if let Some([low, high]) = self.luminance {
            out += &format!("luminance={low},{high}\n");
        }
        out += &format!("mask={},{}\n", self.mask[0], self.mask[1]);
        for (n, chance) in self.groups.iter().enumerate() {
            out += &format!("group{n}={chance}\n");
        }
        out
    }
}

/// A key's number; a pair (CSP's start and end of the trim period, `SHAPE_SIZE = 5, 4.0`) by its mean. `None` for
/// anything else (a track condition).
fn number(keys: &BTreeMap<String, String>, key: &str) -> Option<f32> {
    let values: Vec<f32> = keys.get(key)?.split(',').map(|v| v.trim().parse().ok()).collect::<Option<_>>()?;
    (!values.is_empty() && values.len() <= 2).then(|| values.iter().sum::<f32>() / values.len() as f32)
}

/// A `MAP` or `EXTRA_<n>` value's intensities of configurations A to D.
fn map_of(value: &str) -> [f32; 4] {
    let parts: Vec<&str> = value.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    let mut map = [0.0; 4];
    match parts.first().map(|p| p.to_ascii_uppercase()) {
        Some(name) if matches!(name.as_str(), "A" | "B" | "C" | "D") => {
            let weight = parts.get(1).and_then(|w| w.parse().ok()).unwrap_or(1.0);
            map[usize::from(name.as_bytes()[0] - b'A')] = weight;
        }
        _ => {
            for (slot, part) in map.iter_mut().zip(&parts) {
                *slot = part.parse().unwrap_or(0.0);
            }
        }
    }
    map
}

impl GrassFx {
    /// The grass profile on `mesh` (any case) of `material` by where it stands (AC's world x and z): 0 the stage's
    /// (`[GRASS_FX]`), else the one of the last adjustment naming either, or naming neither but a rectangle, whose
    /// rectangle holds the point ([`GrassFx::section`] writes it as `[ReRbrGrass<n>]`).
    pub fn profile(&self, material: &str, mesh: &str) -> impl Fn([f32; 2]) -> u8 + '_ {
        let (material, mesh) = (material.to_lowercase(), mesh.to_lowercase());
        let any = |patterns: &[String], name: &str| patterns.iter().any(|p| matches(p, name));
        let named: Vec<(&Area, u8)> = (self.adjustments.iter().rev())
            .filter(|a| match a.materials.is_empty() && a.meshes.is_empty() {
                true => a.area.size.is_some(),
                false => any(&a.materials, &material) || any(&a.meshes, &mesh),
            })
            .map(|a| (&a.area, self.maps.iter().position(|m| *m == a.map).map_or(0, |i| (i + 1).min(255) as u8)))
            .collect();
        move |at| named.iter().find(|(area, _)| area.contains(at)).map_or(0, |&(_, profile)| profile)
    }

    /// The profiles' sections, `[ReRbrGrass<n>]` for profile `n` from 1: the numbers each blends ([`GrassFx::shape`]),
    /// its pieces the stage's.
    fn profiles(&self) -> String {
        (1..).zip(&self.maps).map(|(n, map)| format!("[ReRbrGrass{n}]\n{}", self.shape(*map).keys())).collect()
    }

    /// A profile's numbers: the stage's, blended toward each configuration in turn by its intensity in `map` (CSP: "80%
    /// of configuration A, then 50% of B"); a key a configuration leaves out keeps the value so far.
    fn shape(&self, map: [f32; 4]) -> Shape {
        let keys = &self.keys;
        let luminance = match (number(keys, "MASK_MIN_LUMINANCE"), number(keys, "MASK_MAX_LUMINANCE")) {
            (Some(low), Some(high)) => Some([low, high]),
            (None, Some(high)) => Some([0.0, high]),
            _ => None,
        };
        let groups = self.groups.keys().next_back().map_or(0, |last| last + 1);
        let mut shape = Shape {
            base: number(keys, "TEXTURE_BASE_CHANCE").unwrap_or(1.0),
            size: number(keys, "SHAPE_SIZE"),
            luminance,
            // CSP's defaults where the track sets none: grass on green ground only.
            mask: [
                number(keys, "MASK_MAIN_THRESHOLD").unwrap_or(MASK_DEFAULT[0]),
                number(keys, "MASK_RED_THRESHOLD").unwrap_or(MASK_DEFAULT[1]),
            ],
            groups: (0..groups).map(|n| number(keys, &format!("TEXTURE_GROUP_{n}_CHANCE")).unwrap_or(0.0)).collect(),
        };
        for (config, &w) in self.configurations.iter().zip(&map) {
            if w <= 0.0 || config.is_empty() {
                continue;
            }
            let blend = |value: f32, key: &str| number(config, key).map_or(value, |c| value * (1.0 - w) + c * w);
            shape.base = blend(shape.base, "TEXTURE_BASE_CHANCE");
            // No size of the stage's to blend from: the configuration's.
            shape.size = shape.size.map(|s| blend(s, "SHAPE_SIZE")).or(number(config, "SHAPE_SIZE"));
            if number(config, "MASK_MIN_LUMINANCE").is_some() || number(config, "MASK_MAX_LUMINANCE").is_some() {
                let [low, high] = shape.luminance.unwrap_or([0.0, 1.0]);
                shape.luminance = Some([blend(low, "MASK_MIN_LUMINANCE"), blend(high, "MASK_MAX_LUMINANCE")]);
            }
            shape.mask = [blend(shape.mask[0], "MASK_MAIN_THRESHOLD"), blend(shape.mask[1], "MASK_RED_THRESHOLD")];
            for (n, chance) in shape.groups.iter_mut().enumerate() {
                *chance = blend(*chance, &format!("TEXTURE_GROUP_{n}_CHANCE"));
            }
        }
        shape
    }

    /// What the grass does on `material` (any case): grows on it (at the stage's profile, [`GrassFx::profile`] gives a
    /// mesh's), hides under it (an occluder first) or neither.
    pub fn role(&self, material: &str) -> Grass {
        let material = material.to_lowercase();
        let any = |patterns: &[String]| patterns.iter().any(|pattern| matches(pattern, &material));
        if any(&self.occluding[1]) {
            Grass::HidesAlpha
        } else if any(&self.occluding[0]) {
            Grass::Hides
        } else if any(&self.materials) {
            Grass::Grows(0)
        } else {
            Grass::None
        }
    }

    /// The stage's `[ReRbrGrass]` section (`settings.ini`), `texture` being the atlas's name in `Textures.zip`, and its
    /// profiles' (`[ReRbrGrass<n>]`).
    pub fn section(&self, texture: Option<&str>) -> String {
        use std::fmt::Write;
        let mut out = String::from("[ReRbrGrass]\n");
        if let Some(texture) = texture {
            let _ = writeln!(out, "texture={texture}");
            if let Some(grid) = self.keys.get("TEXTURE_GRID") {
                let _ = writeln!(out, "grid={grid}");
            }
        }
        let shape = self.shape([0.0; 4]);
        out += &shape.keys();
        for n in 0..shape.groups.len() as u32 {
            let group = self.groups.get(&n).cloned().unwrap_or_default();
            for (m, piece) in (0..).map_while(|m| group.get(&format!("PIECE_{m}")).map(|p| (m, p))) {
                let cells: Vec<&str> = piece.split(',').map(str::trim).collect();
                let cells = match cells[..] {
                    [x, y] => [x, y, "1", "1"],
                    [x, y, w, h] => [x, y, w, h],
                    _ => continue,
                };
                let chance = number(&group, &format!("PIECE_{m}_CHANCE")).unwrap_or(1.0);
                let mult = group.get(&format!("PIECE_{m}_SIZE_MULT")).map(|v| {
                    let v: Vec<&str> = v.split(',').map(str::trim).collect();
                    format!(",{},{}", v[0], v.get(1).unwrap_or(&v[0]))
                });
                let _ = writeln!(out, "group{n}_piece{m}={},{chance}{}", cells.join(","), mult.unwrap_or_default());
            }
        }
        out + &self.profiles()
    }
}

/// The track's GrassFX, if it lists materials.
pub fn grass_fx(root: &Path) -> Option<GrassFx> {
    let mut fx = GrassFx::default();
    for text in inis(root) {
        let mut section = String::new();
        // The adjustment being read, and whether it is kept (active, no local area).
        let mut adjustment: Option<(Adjustment, bool)> = None;
        let mut flush = |adjustment: &mut Option<(Adjustment, bool)>| {
            if let Some((a, true)) = adjustment.take() {
                fx.adjustments.push(a);
            }
        };
        for line in text.lines().map(str::trim) {
            if let Some(name) = line.strip_prefix('[') {
                flush(&mut adjustment);
                section = name.split(']').next().unwrap_or("").trim().to_uppercase();
                if section.starts_with("GRASS_FX_ADJUSTMENT") {
                    adjustment = Some((Adjustment::default(), true));
                }
            } else if let Some((key, value)) = line.split_once('=') {
                let (key, value) = (key.trim().to_uppercase(), uncommented(value).to_owned());
                let list = || value.split(',').map(|m| m.trim().to_lowercase()).filter(|m| !m.is_empty()).collect();
                let configuration = section.strip_prefix("GRASS_FX_CONFIGURATION_").and_then(|c| match c {
                    "A" | "B" | "C" | "D" => Some(usize::from(c.as_bytes()[0] - b'A')),
                    _ => None,
                });
                if let Some((a, whole)) = adjustment.as_mut() {
                    match key.as_str() {
                        "MATERIALS" => a.materials = list(),
                        "MESHES" => a.meshes = list(),
                        "MAP" => a.map = map_of(&value),
                        "CENTER" => a.area.centre = Area::xz(&value).unwrap_or_default(),
                        "SIZE" => a.area.size = Area::xz(&value),
                        "ANGLE" => a.area.angle = value.trim().parse().unwrap_or(0.0),
                        "ACTIVE" => *whole &= value != "0",
                        "@" => *whole = false,
                        k if k.starts_with("EXTRA_") => {
                            for (slot, w) in a.map.iter_mut().zip(map_of(&value)) {
                                if w > 0.0 {
                                    *slot = w;
                                }
                            }
                        }
                        _ => {}
                    }
                } else if section == "GRASS_FX" {
                    fx.keys.insert(key, value);
                } else if let Some(c) = configuration {
                    fx.configurations[c].insert(key, value);
                } else if let Some(n) = section.strip_prefix("GRASS_FX_TEXTURE_GROUP_").and_then(|n| n.parse().ok()) {
                    fx.groups.entry(n).or_default().insert(key, value);
                }
            }
        }
        flush(&mut adjustment);
    }
    for adjustment in &fx.adjustments {
        if adjustment.map != [0.0; 4] && !fx.maps.contains(&adjustment.map) {
            fx.maps.push(adjustment.map);
        }
    }
    let list = |key: &str| -> Vec<String> {
        let value = fx.keys.get(key).map_or("", String::as_str);
        value.split(',').map(|m| m.trim().to_lowercase()).filter(|m| !m.is_empty()).collect()
    };
    fx.materials = list("GRASS_MATERIALS");
    fx.occluding = [list("OCCLUDING_MATERIALS"), list("OCCLUDING_MATERIALS_ALPHA")];
    if fx.materials.is_empty() {
        return None;
    }
    fx.texture = fx.keys.get("TEXTURE").and_then(|file| {
        let extension = super::stage::source_path(root, "extension").ok()?;
        let path = super::stage::source_path(&extension, &file.replace('\\', "/")).ok()?;
        (path.is_file() && path.extension().is_some_and(|x| x.eq_ignore_ascii_case("dds"))).then_some(path)
    });
    Some(fx)
}

/// Whether `name` (lowercase) matches `pattern`, where `?` and `*` are any run of characters: CSP's filters take `?`
/// as "any number of symbols" (its own `extension/config` notes), so Joux Verte's `?talus?` is every embankment.
pub fn matches(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let mut row = vec![false; n.len() + 1];
    row[0] = true;
    for c in p {
        let any = c == '*' || c == '?';
        let mut next = vec![any && row[0]; n.len() + 1];
        for j in 0..n.len() {
            next[j + 1] = match c {
                '*' | '?' => next[j] || row[j + 1],
                c => row[j] && n[j] == c,
            };
        }
        row = next;
    }
    row[n.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grass_fx_names_its_materials_atlas_pieces_and_luminance() {
        let dir = std::env::temp_dir().join(format!("csp-grass-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("extension")).unwrap();
        std::fs::write(dir.join("extension/Mine.DDS"), b"x").unwrap();
        std::fs::write(
            dir.join("extension/ext_config.ini"),
            "[GRASS_FX]\nGRASS_MATERIALS = Ter?, rock_cluster\nOCCLUDING_MATERIALS = shale, ter9\nOCCLUDING_MATERIALS_ALPHA = roadEdge_*\nTEXTURE = mine.dds ; mine\nTEXTURE_GRID = 8, 3\n\
             TEXTURE_BASE_CHANCE = 4.0\nTEXTURE_GROUP_1_CHANCE = 2\nMASK_MAX_LUMINANCE = 0.25\nSHAPE_SIZE = 6.5\nMASK_RED_THRESHOLD = 0.01\n\
             [GRASS_FX_TEXTURE_GROUP_1]\nPIECE_0 = 1, 1\nPIECE_1 = 4, 1, 2, 2\nPIECE_1_CHANCE = 0.5\n\
             PIECE_1_SIZE_MULT = 0.6, 1.5 // tall\n",
        )
        .unwrap();
        let fx = grass_fx(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            ["TER0", "rock_cluster", "road"].map(|m| fx.role(m)),
            [Grass::Grows(0), Grass::Grows(0), Grass::None]
        );
        assert_eq!(
            ["Shale", "ter9", "roadEdge_tar"].map(|m| fx.role(m)),
            [Grass::Hides, Grass::Hides, Grass::HidesAlpha]
        );
        assert!(fx.texture.as_ref().is_some_and(|t| t.ends_with("Mine.DDS")));
        assert_eq!(
            fx.section(Some("ac_00001.dds")),
            "[ReRbrGrass]\ntexture=ac_00001.dds\ngrid=8, 3\nbase=4\nsize=6.5\nluminance=0,0.25\nmask=0.5,0.01\ngroup0=0\ngroup1=2\n\
             group1_piece0=1,1,1,1,1\ngroup1_piece1=4,1,2,2,0.5,0.6,1.5\n"
        );
    }

    #[test]
    fn grass_fx_adjustments_blend_its_configurations_in_on_their_meshes() {
        let dir = std::env::temp_dir().join(format!("csp-grass-adjust-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("extension")).unwrap();
        // Joux Verte's, shortened.
        std::fs::write(
            dir.join("extension/ext_config.ini"),
            "[GRASS_FX]\nGRASS_MATERIALS = grass?\nSHAPE_SIZE = 4\n[GRASS_FX_TEXTURE_GROUP_0]\nPIECE_0 = 1, 1\n\
             [GRASS_FX_CONFIGURATION_A]\nTEXTURE_GROUP_0_CHANCE = 0.5\nMASK_MAIN_THRESHOLD = -0.05\nMASK_MIN_LUMINANCE = 0.01\n\
             SHAPE_SIZE = 5, 4.0\n[GRASS_FX_CONFIGURATION_B]\nMASK_MAIN_THRESHOLD = -0.5\n\
             [GRASS_FX_ADJUSTMENT_...]\nMATERIALS = Grass0050_1_L\nMESHES = 1GRASS208, WALL7\nMAP = A\n\
             [GRASS_FX_ADJUSTMENT_...]\nMATERIALS =\nMESHES = 1GRASS364\nMAP = B, 0.5\n\
             [GRASS_FX_ADJUSTMENT_...]\nMESHES = 1GRASS1\nMAP = A\n@ = GrassFX_ExtraArea, 0, 0, 10\n\
             [GRASS_FX_ADJUSTMENT_...]\nCENTER = 100, 5, 0 ; x, y, z\nSIZE = 20, 4\nANGLE = 90\nMAP = A\n",
        )
        .unwrap();
        let fx = grass_fx(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let profiles =
            [("grass0050_1_l", "other"), ("grass1", "1GRASS208"), ("grass1", "1grass364"), ("grass1", "1grass1")];
        assert_eq!(profiles.map(|(material, mesh)| fx.profile(material, mesh)([0.0; 2])), [1, 1, 2, 0]);
        // A rectangle turned upright: 4 m across x, 20 m along z.
        let at = fx.profile("grass1", "1grass1");
        assert_eq!([[100.0, 9.0], [101.5, 0.0], [103.0, 0.0], [100.0, 11.0]].map(at), [1, 1, 0, 0]);
        let section = fx.section(None);
        assert!(section.starts_with("[ReRbrGrass]\nbase=1\nsize=4\nmask=0.5,0.05\ngroup0=0\ngroup0_piece0=1,1,1,1,1\n"));
        assert!(section.ends_with(
            "[ReRbrGrass1]\nbase=1\nsize=4.5\nluminance=0.01,1\nmask=-0.05,0.05\ngroup0=0.5\n\
             [ReRbrGrass2]\nbase=1\nsize=4\nmask=0,0.05\ngroup0=0\n"
        ));
    }

    #[test]
    fn grass_fx_without_a_file_of_the_track_has_no_atlas() {
        let dir = std::env::temp_dir().join(format!("csp-grass-none-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("extension")).unwrap();
        std::fs::write(
            dir.join("extension/a.ini"),
            "[GRASS_FX]\nGRASS_MATERIALS = a\nTEXTURE = grass_fx/highlands.dds\n",
        )
        .unwrap();
        let fx = grass_fx(&dir).unwrap();
        std::fs::write(dir.join("extension/a.ini"), "[GRASS_FX]\nTEXTURE = x.dds\n").unwrap();
        let none = grass_fx(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(fx.texture.is_none() && none.is_none());
        assert!(fx.section(None).starts_with("[ReRbrGrass]\nbase=1\n"));
    }

    #[test]
    fn model_replacements_name_the_meshes_to_hide() {
        let dir = std::env::temp_dir().join(format!("trees-hidden-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("extension")).unwrap();
        std::fs::write(
            dir.join("extension/a.ini"),
            "[MODEL_REPLACEMENT_0]\nFILE= Hedges.kn5\nHIDE= hedgesB, HedgesC\n\n[OTHER]\nHIDE=x\n",
        )
        .unwrap();
        let hidden = replaced(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(hidden["hedges.kn5"], HashSet::from(["hedgesb".to_owned(), "hedgesc".to_owned()]));
    }

    #[test]
    fn unconditional_mesh_adjustments_hide_meshes_and_names_take_wildcards() {
        let dir = std::env::temp_dir().join(format!("csp-unrendered-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("extension")).unwrap();
        std::fs::write(
            dir.join("extension/a.ini"),
            "[MESH_ADJUSTEMENT_1]\nMESHES = OBJ1, shader:ksTree\nIS_RENDERABLE = 0\nCONDITION = ALWAYS_ON\n\n\
             [MESH_ADJUSTMENT_2]\nMESHES = OBJ2\nIS_RENDERABLE = 0\nCONDITION = NIGHT_SMOOTH\n\n\
             [MESH_ADJUSTMENT_3]\nMESHES = OBJ3\nIS_RENDERABLE = 1\n\n[MESH_ADJUSTMENT_4]\nMESHES = OBJ4_?\nIS_RENDERABLE=0\n",
        )
        .unwrap();
        let names = unrendered(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(names, ["obj1", "obj4_?"]);
        assert!(matches("obj4_?", "obj4_a") && matches("obj4_?", "obj4_ab") && matches("?_lod550m", "x_lod550m"));
        assert!(matches("?talus?", "talus_d1apep3_01") && matches("obj4_?", "obj4_") && !matches("obj4_?", "obj5_a"));
    }
}
