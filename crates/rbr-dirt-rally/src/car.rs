//! A DiRT Rally car's `<car>_highlod.pssg`: its parts and what each draws.
//!
//! The scene is `LOD0_` (the bundle node of the nearest level) over one `MATRIXPALETTEJOINTNODE` per part (`x0_door_fl`),
//! each holding `MATRIXPALETTEJOINTRENDERINSTANCE`s: a range of a shared `RENDERDATASOURCE`'s vertices
//! (`streamOffset`, `elementCountFromOffset`) and indices (`indexOffset`, `indicesCountFromOffset`), the
//! `SHADERINSTANCE` (`shader`) that draws it and the joint (`jointID`) of the matrix palette it is skinned to.
use crate::mesh::{self, Geometry};
use crate::pssg::Pssg;
use std::collections::HashMap;
use std::rc::Rc;

/// One range of a source drawn with one material.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    /// The `SHADERINSTANCE` id (`Car Body Paint`, `Tarmac Wheel`).
    pub material: String,
    /// The matrix palette joint the range is skinned to.
    pub joint: Option<u32>,
    /// The `RENDERDATASOURCE` the range is cut from.
    pub source: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    /// Counter-clockwise, right handed, into this instance's vertices.
    pub triangles: Vec<[u32; 3]>,
}

/// A part of the car and its instances.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    /// The node's nickname without its `x0_`: `door_fl`.
    pub name: String,
    /// Part space to the car's (row vectors, translation in the last row): the node's transform under `LOD0_`'s.
    pub transform: mesh::Mat,
    pub instances: Vec<Instance>,
}

/// `count` elements of `v` from `first`.
fn slice<T: Clone>(v: &[T], first: usize, count: usize) -> Option<Vec<T>> {
    v.get(first..first + count).map(<[T]>::to_vec)
}

/// The parts of the nearest level of detail.
pub fn parts(pssg: &Pssg) -> Result<Vec<Part>, String> {
    let lod = pssg.by_id("LOD0_").ok_or("no LOD0_ node")?;
    let mut sources: HashMap<String, Rc<Geometry>> = HashMap::new();
    let mut out = Vec::new();
    for node in lod.children() {
        let Some(name) = node.id().and_then(|id| id.strip_prefix("x0_")) else { continue };
        let mut instances = Vec::new();
        for item in node.children().filter(|c| c.name() == "MATRIXPALETTEJOINTRENDERINSTANCE") {
            let source = item.reference("indices").ok_or("an instance without its source")?;
            let id = source.id().unwrap_or("").to_owned();
            if !sources.contains_key(&id) {
                sources.insert(id.clone(), Rc::new(mesh::geometry(source)?));
            }
            let geometry = &sources[&id];
            let at = |attr: &str| item.u32(attr).unwrap_or(0) as usize;
            let (first, count) = (at("streamOffset"), at("elementCountFromOffset"));
            let (start, length) = (at("indexOffset"), at("indicesCountFromOffset"));
            let indices =
                geometry.indices.get(start..start + length).ok_or_else(|| format!("{id}: indices past the end"))?;
            // The indices count from the range's first vertex (checked on the shipped cars: `info`).
            let (lo, hi) = indices.iter().fold((u32::MAX, 0), |(lo, hi), &i| (lo.min(i), hi.max(i)));
            let base =
                if !indices.is_empty() && lo as usize >= first && hi as usize >= count { first as u32 } else { 0 };
            let triangles =
                indices.chunks_exact(3).map(|t| [t[0] - base, t[1] - base, t[2] - base]).collect::<Vec<_>>();
            let positions =
                slice(&geometry.positions, first, count).ok_or_else(|| format!("{id}: vertices past the end"))?;
            if triangles.iter().flatten().any(|&i| i as usize >= positions.len()) {
                return Err(format!("{id}: an index past its range (base {base}, {lo}..{hi}, {count} vertices)"));
            }
            instances.push(Instance {
                material: item.str("shader").unwrap_or("").trim_start_matches('#').to_owned(),
                joint: item.u32("jointID"),
                source: id,
                normals: slice(&geometry.normals, first, count).unwrap_or_default(),
                uv: slice(&geometry.uv, first, count).unwrap_or_default(),
                positions,
                triangles,
            });
        }
        out.push(Part { name: name.to_owned(), transform: mesh::mul(&node.transform(), &lod.transform()), instances });
    }
    Ok(out)
}

/// The parts of a car's interior scene (`int_<car>.pssg`): one per node, already in the car's frame (identity transform).
pub fn interior(pssg: &Pssg) -> Result<Vec<Part>, String> {
    let mut out: Vec<Part> = Vec::new();
    for mesh in mesh::scene(pssg)? {
        let instance = Instance {
            material: mesh.material,
            joint: None,
            source: mesh.node.clone(),
            positions: mesh.positions,
            normals: mesh.normals,
            uv: mesh.uv,
            triangles: mesh.triangles,
        };
        match out.iter_mut().find(|p| p.name == mesh.node) {
            Some(part) => part.instances.push(instance),
            None => out.push(Part {
                name: mesh.node,
                transform: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]],
                instances: vec![instance],
            }),
        }
    }
    Ok(out)
}

/// One line per instance: the part, material, joint, sizes and the box of its positions.
pub fn info(parts: &[Part], verbose: bool) -> String {
    let mut out = String::new();
    for part in parts {
        let t = part.transform;
        out += &format!(
            "{} at [{:.3} {:.3} {:.3}] x-axis [{:.2} {:.2} {:.2}] z-axis [{:.2} {:.2} {:.2}]\n",
            part.name, t[3][0], t[3][1], t[3][2], t[0][0], t[0][1], t[0][2], t[2][0], t[2][1], t[2][2]
        );
        if !verbose {
            continue;
        }
        for i in &part.instances {
            let (lo, hi) = i.positions.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
                (std::array::from_fn(|k| lo[k].min(p[k])), std::array::from_fn(|k| hi[k].max(p[k])))
            });
            let (uv_lo, uv_hi) = i.uv.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), p| {
                (std::array::from_fn(|k| lo[k].min(p[k])), std::array::from_fn(|k| hi[k].max(p[k])))
            });
            out += &format!(
                "{:<24} {:<18} j{:<3} {:<10} v{:<6} n{:<6} t{:<6} uv{:<6} [{:6.2} {:6.2} {:6.2}] .. [{:6.2} {:6.2} {:6.2}] uv [{:.2} {:.2}] .. [{:.2} {:.2}]\n",
                part.name,
                i.material,
                i.joint.map_or("-".to_owned(), |j| j.to_string()),
                i.source,
                i.positions.len(),
                i.normals.len(),
                i.triangles.len(),
                i.uv.len(),
                lo[0],
                lo[1],
                lo[2],
                hi[0],
                hi[1],
                hi[2],
                uv_lo[0],
                uv_lo[1],
                uv_hi[0],
                uv_hi[1],
            );
        }
    }
    out
}

/// One line per material of the file: its shader group, textures and constants.
pub fn materials(pssg: &Pssg) -> String {
    let mut out = String::new();
    for m in crate::material::read(pssg).values() {
        out += &format!("{:<20} {:<22} {:?}\n", m.id, m.shader, m.textures);
        for (name, value) in &m.constants {
            out += &format!("    {name} = {value:?}\n");
        }
    }
    out
}
