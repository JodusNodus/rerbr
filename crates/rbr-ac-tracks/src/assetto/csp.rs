//! What CSP's `extension/*.ini` files do to a track's meshes: hide them. Only unconditional changes are applied.
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

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
                let value = value.split(';').next().unwrap_or("").trim();
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
                let value = value.split(';').next().unwrap_or("").trim();
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
