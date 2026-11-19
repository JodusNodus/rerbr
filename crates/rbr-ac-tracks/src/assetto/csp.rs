//! What CSP's `extension/*.ini` files do to a track's meshes: hide them. Only unconditional changes are applied.
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

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
/// on, its atlas and the pieces drawn from it, and the luminance range of the ground it grows on.
#[derive(Debug, Default)]
pub struct GrassFx {
    /// `GRASS_MATERIALS`, lowercase, with `?` and `*` wildcards.
    pub materials: Vec<String>,
    /// `TEXTURE` where it names a file of the track's own `extension` folder; `None` for CSP's own atlases and no key.
    pub texture: Option<std::path::PathBuf>,
    keys: BTreeMap<String, String>,
    groups: BTreeMap<u32, BTreeMap<String, String>>,
}

impl GrassFx {
    /// Whether `material` (any case) is one the grass grows on.
    pub fn grows_on(&self, material: &str) -> bool {
        let material = material.to_lowercase();
        self.materials.iter().any(|pattern| matches(pattern, &material))
    }

    /// The stage's `[ReRbrGrass]` section (`settings.ini`), `texture` being the atlas's name in `Textures.zip`.
    pub fn section(&self, texture: Option<&str>) -> String {
        use std::fmt::Write;
        let number = |keys: &BTreeMap<String, String>, key: &str| keys.get(key).and_then(|v| v.parse::<f32>().ok());
        let mut out = String::from("[ReRbrGrass]\n");
        if let Some(texture) = texture {
            let _ = writeln!(out, "texture={texture}");
            if let Some(grid) = self.keys.get("TEXTURE_GRID") {
                let _ = writeln!(out, "grid={grid}");
            }
        }
        let _ = writeln!(out, "base={}", number(&self.keys, "TEXTURE_BASE_CHANCE").unwrap_or(1.0));
        if let Some(size) = number(&self.keys, "SHAPE_SIZE") {
            let _ = writeln!(out, "size={size}");
        }
        if let (Some(low), Some(high)) =
            (number(&self.keys, "MASK_MIN_LUMINANCE"), number(&self.keys, "MASK_MAX_LUMINANCE"))
        {
            let _ = writeln!(out, "luminance={low},{high}");
        } else if let Some(high) = number(&self.keys, "MASK_MAX_LUMINANCE") {
            let _ = writeln!(out, "luminance=0,{high}");
        }
        // CSP's defaults where the track sets none: grass on green ground only.
        let main = number(&self.keys, "MASK_MAIN_THRESHOLD").unwrap_or(MASK_DEFAULT[0]);
        let red = number(&self.keys, "MASK_RED_THRESHOLD").unwrap_or(MASK_DEFAULT[1]);
        let _ = writeln!(out, "mask={main},{red}");
        for n in 0..self.groups.keys().next_back().map_or(0, |last| last + 1) {
            let group = self.groups.get(&n).cloned().unwrap_or_default();
            let chance = number(&self.keys, &format!("TEXTURE_GROUP_{n}_CHANCE")).unwrap_or(0.0);
            let _ = writeln!(out, "group{n}={chance}");
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
        out
    }
}

/// The track's GrassFX, if it lists materials.
pub fn grass_fx(root: &Path) -> Option<GrassFx> {
    let mut fx = GrassFx::default();
    for text in inis(root) {
        let mut section = String::new();
        for line in text.lines().map(str::trim) {
            if let Some(name) = line.strip_prefix('[') {
                section = name.split(']').next().unwrap_or("").trim().to_uppercase();
            } else if let Some((key, value)) = line.split_once('=') {
                let (key, value) = (key.trim().to_uppercase(), uncommented(value).to_owned());
                if section == "GRASS_FX" {
                    fx.keys.insert(key, value);
                } else if let Some(n) = section.strip_prefix("GRASS_FX_TEXTURE_GROUP_").and_then(|n| n.parse().ok()) {
                    fx.groups.entry(n).or_default().insert(key, value);
                }
            }
        }
    }
    fx.materials =
        fx.keys.get("GRASS_MATERIALS")?.split(',').map(|m| m.trim().to_lowercase()).filter(|m| !m.is_empty()).collect();
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

/// Whether `name` (lowercase) matches `pattern`, where `?` is any character and `*` any run of them.
pub fn matches(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let mut row = vec![false; n.len() + 1];
    row[0] = true;
    for c in p {
        let mut next = vec![c == '*' && row[0]; n.len() + 1];
        for j in 0..n.len() {
            next[j + 1] = match c {
                '*' => next[j] || row[j + 1],
                '?' => row[j],
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
            "[GRASS_FX]\nGRASS_MATERIALS = Ter?, rock_cluster\nTEXTURE = mine.dds ; mine\nTEXTURE_GRID = 8, 3\n\
             TEXTURE_BASE_CHANCE = 4.0\nTEXTURE_GROUP_1_CHANCE = 2\nMASK_MAX_LUMINANCE = 0.25\nSHAPE_SIZE = 6.5\nMASK_RED_THRESHOLD = 0.01\n\
             [GRASS_FX_TEXTURE_GROUP_1]\nPIECE_0 = 1, 1\nPIECE_1 = 4, 1, 2, 2\nPIECE_1_CHANCE = 0.5\n\
             PIECE_1_SIZE_MULT = 0.6, 1.5 // tall\n",
        )
        .unwrap();
        let fx = grass_fx(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(fx.grows_on("TER0") && fx.grows_on("rock_cluster") && !fx.grows_on("road"));
        assert!(fx.texture.as_ref().is_some_and(|t| t.ends_with("Mine.DDS")));
        assert_eq!(
            fx.section(Some("ac_00001.dds")),
            "[ReRbrGrass]\ntexture=ac_00001.dds\ngrid=8, 3\nbase=4\nsize=6.5\nluminance=0,0.25\nmask=0.5,0.01\ngroup0=0\ngroup1=2\n\
             group1_piece0=1,1,1,1,1\ngroup1_piece1=4,1,2,2,0.5,0.6,1.5\n"
        );
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
        assert!(matches("obj4_?", "obj4_a") && !matches("obj4_?", "obj4_ab") && matches("*_lod550m", "x_lod550m"));
    }
}
