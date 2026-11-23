//! Materials: the `SHADERINSTANCE`s of a PSSG with the textures and constants they set by name.
//!
//! A `SHADERINSTANCE` names its `SHADERGROUP` (`shaderGroup="#terrain_road.fx"`); each `SHADERINPUT` it holds gives a
//! `parameterID`, the index into the group's `SHADERINPUTDEFINITION` list, and a texture reference or the big-endian
//! floats of a constant.
use crate::pssg::{floats, Pssg};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Material {
    /// The `SHADERINSTANCE` id (it can hold spaces and `!`).
    pub id: String,
    /// The shader group's file name, `terrain_road.fx`.
    pub shader: String,
    /// Texture inputs by name, `TDiffuseSpecMap1` -> `tracksplit.pssg#x.tga` as written (`#x.tga` is in the same file).
    pub textures: BTreeMap<String, String>,
    pub constants: BTreeMap<String, Vec<f32>>,
}

/// Every material of the file by id.
pub fn read(pssg: &Pssg) -> BTreeMap<String, Material> {
    let mut definitions: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for group in pssg.find("SHADERGROUP") {
        let names = group.children().filter(|c| c.name() == "SHADERINPUTDEFINITION").map(|c| c.str("name").unwrap_or("")).collect();
        definitions.insert(group.id().unwrap_or(""), names);
    }
    let mut out = BTreeMap::new();
    for instance in pssg.find("SHADERINSTANCE") {
        let shader = instance.str("shaderGroup").unwrap_or("").trim_start_matches('#');
        let names = definitions.get(shader);
        let mut material = Material {
            id: instance.id().unwrap_or("").to_owned(),
            shader: shader.to_owned(),
            ..Material::default()
        };
        for input in instance.children().filter(|c| c.name() == "SHADERINPUT") {
            let Some(name) = names.and_then(|n| n.get(input.u32("parameterID")? as usize)) else { continue };
            match input.str("texture") {
                Some(t) => material.textures.insert((*name).to_owned(), t.to_owned()).map(drop),
                None => material.constants.insert((*name).to_owned(), floats(input.payload())).map(drop),
            };
        }
        out.insert(material.id.clone(), material);
    }
    out
}

/// The inputs a material is drawn with, first found wins: a terrain's first layer, a prop's or tree's diffuse map,
/// then the layers an infield material starts at and the vista's large map (surveyed on both sample areas).
const DIFFUSE_INPUTS: [&str; 6] =
    ["TDiffuseSpecMap1", "TDiffuseAlphaMap", "TDiffuseSpecMap2", "TDiffuseSpecMap3", "TlargeColourMap", "TTopDownDiffuseAlphaMap"];

/// The input name and texture reference a material is drawn with; `None` for materials without one (`batched_track`).
pub fn diffuse_input(m: &Material) -> Option<(&str, &str)> {
    DIFFUSE_INPUTS.iter().find_map(|name| m.textures.get_key_value(*name).map(|(k, v)| (k.as_str(), v.as_str())))
}

/// The texture reference a material is drawn with.
pub fn diffuse(m: &Material) -> Option<&str> {
    diffuse_input(m).map(|(_, texture)| texture)
}

/// `[scale u, scale v, offset u, offset v]` of the diffuse input's layer (`Map<N>UVScaleAndOffset` for
/// `TDiffuseSpecMap<N>`); identity for inputs without one. `uv * scale + offset` is the coordinate the layer reads.
pub fn uv_transform(m: &Material) -> [f32; 4] {
    const IDENTITY: [f32; 4] = [1., 1., 0., 0.];
    let Some((input, _)) = diffuse_input(m) else { return IDENTITY };
    let layer = input.strip_prefix("TDiffuseSpecMap").filter(|n| n.len() == 1);
    let constant = layer.and_then(|n| m.constants.get(&format!("Map{n}UVScaleAndOffset")));
    match constant.map(Vec::as_slice) {
        Some(&[su, sv, ou, ov]) if su != 0. || sv != 0. => [su, sv, ou, ov],
        _ => IDENTITY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pssg::tests::sample_areas;

    fn material(textures: &[&str], constants: &[(&str, &[f32])]) -> Material {
        Material {
            id: "m".into(),
            shader: "s.fx".into(),
            textures: textures.iter().map(|t| (t.to_string(), format!("f.pssg#{t}.tga"))).collect(),
            constants: constants.iter().map(|(k, v)| (k.to_string(), v.to_vec())).collect(),
        }
    }

    #[test]
    fn diffuse_follows_the_shader_family() {
        let road = material(&["TNormalMap1", "TDiffuseSpecMap2", "TDiffuseSpecMap1"], &[("Map1UVScaleAndOffset", &[2., 3., 0.5, 0.25])]);
        assert_eq!(diffuse(&road), Some("f.pssg#TDiffuseSpecMap1.tga"));
        assert_eq!(uv_transform(&road), [2., 3., 0.5, 0.25]);
        let infield = material(&["TDiffuseSpecMap3", "TDiffuseSpecMap2"], &[("Map2UVScaleAndOffset", &[0., 0., 0., 0.])]);
        assert_eq!(diffuse_input(&infield).map(|d| d.0), Some("TDiffuseSpecMap2"));
        assert_eq!(uv_transform(&infield), [1., 1., 0., 0.]);
        let prop = material(&["TSpecularMap", "TDiffuseAlphaMap"], &[]);
        assert_eq!(diffuse_input(&prop).map(|d| d.0), Some("TDiffuseAlphaMap"));
        assert_eq!(diffuse(&material(&[], &[])), None);
    }

    #[test]
    fn every_material_of_the_sample_areas_reads() {
        for area in sample_areas() {
            for file in ["tracksplit.pssg", "route_0/route_objects.pssg", "objects.pssg", "trees.pssg"] {
                let p = Pssg::read(&area.join(file)).unwrap();
                let materials = read(&p);
                assert!(!materials.is_empty(), "{file}");
                let without: Vec<_> = materials.values().filter(|m| diffuse(m).is_none()).map(|m| m.shader.as_str()).collect();
                eprintln!("{} {file}: {} materials, without a diffuse: {without:?}", area.display(), materials.len());
                assert!(without.iter().all(|s| ["batched_track.fx", "volumetrics.fx", "foliage_billboard_double_shadow.fx"].contains(s)), "{without:?}");
            }
        }
    }
}
