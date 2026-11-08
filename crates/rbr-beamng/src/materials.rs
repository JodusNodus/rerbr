//! A level's materials (`*.materials.json`: objects of `Material` entries keyed by name, `mapTo` the name meshes
//! use) and its own ground models (`groundmodels/*.json`: name to parameters, `collisiontype` the built-in model it
//! behaves as, `aliases`).

use crate::json::{self, Json};
use crate::zipfs::Archive;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Material {
    pub name: String,
    /// The mesh material name it stands for (`mapTo`, else its name).
    pub map_to: String,
    /// `groundType`: the ground model of faces with this material.
    pub ground: Option<String>,
    /// The first stage's colour texture (`baseColorMap`, `colorMap` or `diffuseMap`), a level path, maybe without its
    /// extension.
    pub color_map: Option<String>,
    /// The first stage's `opacityMap`: the alpha kept apart from the colour.
    pub opacity_map: Option<String>,
    /// The colour factor (`baseColorFactor` or `diffuseColor`), RGBA.
    pub color: Option<[f32; 4]>,
    pub translucent: bool,
    /// `alphaTest` with `alphaRef`: cut out by the texture's alpha.
    pub alpha_test: bool,
}

/// Every material of the level, by `mapTo` name (lower case); the first file in name order wins a duplicate.
pub fn read(archive: &mut Archive, level: &str) -> Result<HashMap<String, Material>, String> {
    let prefix = format!("levels/{level}/").to_lowercase();
    let mut files: Vec<String> = archive
        .names()
        .into_iter()
        .filter(|n| {
            let l = n.to_lowercase();
            l.starts_with(&prefix) && l.ends_with(".materials.json")
        })
        .collect();
    files.sort();
    let mut out = HashMap::new();
    for file in files {
        let doc = json::parse(&archive.read_text(&file)?).map_err(|e| format!("{file}: {e}"))?;
        for (key, m) in doc.members() {
            if m.get("class").and_then(Json::str) != Some("Material") {
                continue;
            }
            let material = parse(key, m);
            out.entry(material.map_to.to_lowercase()).or_insert(material);
        }
    }
    Ok(out)
}

fn parse(key: &str, m: &Json) -> Material {
    let name = m.get("name").and_then(Json::str).unwrap_or(key).to_owned();
    let stage = m.get("Stages").map(Json::arr).and_then(|s| s.first());
    let stage_str = |k: &str| stage.and_then(|s| s.get(k)).and_then(Json::str).map(str::to_owned);
    let color = stage
        .and_then(|s| s.get("baseColorFactor").or_else(|| s.get("diffuseColor")))
        .map(Json::nums)
        .filter(|n| n.len() >= 3)
        .map(|n| [n[0] as f32, n[1] as f32, n[2] as f32, n.get(3).copied().unwrap_or(1.0) as f32]);
    Material {
        map_to: m.get("mapTo").and_then(Json::str).unwrap_or(&name).to_owned(),
        name,
        ground: m.get("groundType").and_then(Json::str).map(str::to_owned),
        color_map: stage_str("baseColorMap").or_else(|| stage_str("colorMap")).or_else(|| stage_str("diffuseMap")),
        opacity_map: stage_str("opacityMap"),
        color,
        translucent: m.get("translucent").and_then(Json::flag).unwrap_or(false),
        alpha_test: m.get("alphaTest").and_then(Json::flag).unwrap_or(false),
    }
}

/// The level's own ground models: name (and each alias, upper case) to the built-in model it collides as
/// (`collisiontype`).
pub fn ground_models(archive: &mut Archive, level: &str) -> Result<HashMap<String, String>, String> {
    let prefix = format!("levels/{level}/groundmodels/").to_lowercase();
    let files: Vec<String> = archive
        .names()
        .into_iter()
        .filter(|n| {
            let l = n.to_lowercase();
            l.starts_with(&prefix) && l.ends_with(".json")
        })
        .collect();
    let mut out = HashMap::new();
    for file in files {
        let doc = json::parse(&archive.read_text(&file)?).map_err(|e| format!("{file}: {e}"))?;
        for (name, model) in doc.members() {
            let Some(kind) = model.get("collisiontype").and_then(Json::str) else { continue };
            out.insert(name.to_uppercase(), kind.to_uppercase());
            for alias in model.get("aliases").map(Json::arr).unwrap_or_default() {
                if let Some(a) = alias.str() {
                    out.insert(a.to_uppercase(), kind.to_uppercase());
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_material_entry() {
        let doc = json::parse(
            r#"{"road": {"name": "road", "mapTo": "asph", "class": "Material", "groundType": "ASPHALT",
                "Stages": [{"baseColorMap": "/levels/l/art/road.dds", "baseColorFactor": [0.5, 0.5, 0.5, 1]}],
                "translucent": true}}"#,
        )
        .unwrap();
        let (key, m) = &doc.members()[0];
        let m = parse(key, m);
        assert_eq!((m.map_to.as_str(), m.ground.as_deref()), ("asph", Some("ASPHALT")));
        assert_eq!(m.color_map.as_deref(), Some("/levels/l/art/road.dds"));
        assert_eq!(m.color, Some([0.5, 0.5, 0.5, 1.0]));
        assert!(m.translucent && !m.alpha_test);
    }
}
