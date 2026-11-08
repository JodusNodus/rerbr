//! Collada shapes (`.dae`) as a [`Shape`], for levels that ship no `.cdae` (BeamNG builds those on first load). What
//! Blender exports: `<triangles>` and `<polylist>` over `<float_array>` sources, nodes with a `<matrix>`. Each
//! instanced geometry becomes one object, its node's matrix baked into the vertices; under a `start01` node a name's
//! trailing number is its detail size (`a400`; `Colmesh-1` collision), else it joins the most detailed level. Corners
//! are unshared (a triangle soup), wound clockwise and with `v` flipped as Torque imports them.

use crate::cdae::{Detail, Mesh, Node, Object, Primitive, Shape};
use quick_xml::events::Event;
use std::collections::HashMap;

#[derive(Debug, Default)]
struct El {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    kids: Vec<El>,
}

impl El {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    fn kid(&self, n: &str) -> Option<&El> {
        self.kids.iter().find(|k| k.name == n)
    }
    fn kids<'a>(&'a self, n: &'a str) -> impl Iterator<Item = &'a El> + 'a {
        self.kids.iter().filter(move |k| k.name == n)
    }
    fn walk<'a>(&'a self, out: &mut Vec<&'a El>) {
        out.push(self);
        self.kids.iter().for_each(|k| k.walk(out));
    }
    fn numbers<T: std::str::FromStr>(&self) -> Vec<T> {
        self.text.split_ascii_whitespace().filter_map(|s| s.parse().ok()).collect()
    }
}

fn tree(xml: &str) -> Result<El, String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut stack = vec![El::default()];
    let open = |e: &quick_xml::events::BytesStart| El {
        name: String::from_utf8_lossy(e.local_name().as_ref()).into_owned(),
        attrs: e
            .attributes()
            .flatten()
            .map(|a| {
                (String::from_utf8_lossy(a.key.as_ref()).into_owned(), String::from_utf8_lossy(&a.value).into_owned())
            })
            .collect(),
        ..El::default()
    };
    loop {
        match reader.read_event().map_err(|e| format!("dae: {e}"))? {
            Event::Start(e) => stack.push(open(&e)),
            Event::Empty(e) => {
                let el = open(&e);
                stack.last_mut().unwrap().kids.push(el);
            }
            Event::Text(t) => stack.last_mut().unwrap().text.push_str(&String::from_utf8_lossy(&t)),
            Event::End(_) => {
                let el = stack.pop().unwrap();
                stack.last_mut().ok_or("dae: unbalanced")?.kids.push(el);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    stack.pop().ok_or_else(|| "dae: empty".into())
}

type Mat = [[f64; 4]; 4];
/// An input of a primitive: semantic, offset, source data and stride.
type Input<'a> = (&'a str, usize, (Vec<f32>, usize));

fn mul(a: &Mat, b: &Mat) -> Mat {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..4).map(|k| a[i][k] * b[k][j]).sum()))
}

const ID: Mat = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// Parses a Collada document into a shape of one detail level.
pub fn parse(xml: &str) -> Result<Shape, String> {
    let root = tree(xml)?;
    let doc = root.kid("COLLADA").ok_or("dae: no COLLADA")?;
    let mut all = Vec::new();
    doc.walk(&mut all);
    let by_id: HashMap<&str, &El> = all.iter().filter_map(|e| e.attr("id").map(|i| (i, *e))).collect();
    let get = |r: &str| by_id.get(r.trim_start_matches('#')).copied();
    let y_up = all.iter().any(|e| e.name == "up_axis" && e.text.trim() == "Y_UP");
    let base: Mat = if y_up { [[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [0.0, 1.0, 0.0, 0.0], ID[3]] } else { ID };

    let mut materials = Vec::new();
    let mut material_index: HashMap<String, u32> = HashMap::new();
    // Every instanced geometry: its node's name, detail size and mesh.
    let mut found: Vec<(String, f32, Mesh)> = Vec::new();

    // Nodes of the scene with their world matrices and whether they hang under Torque's `start01` (whose children
    // name their detail size).
    let scene = doc.kid("scene").and_then(|s| s.kid("instance_visual_scene")).and_then(|i| i.attr("url")).and_then(get);
    let mut stack: Vec<(&El, Mat, bool)> =
        scene.map(|s| s.kids("node").map(|n| (n, base, false)).collect()).unwrap_or_default();
    while let Some((node, parent, under_start)) = stack.pop() {
        let mut m = parent;
        if let Some(mx) = node.kid("matrix") {
            let v: Vec<f64> = mx.numbers();
            if v.len() == 16 {
                m = mul(&m, &std::array::from_fn(|i| std::array::from_fn(|j| v[i * 4 + j])));
            }
        }
        let name = node.attr("name").unwrap_or("object");
        let start = name.to_lowercase().starts_with("start");
        stack.extend(node.kids("node").map(|n| (n, m, start)));
        let size = if under_start { detail_size(name) } else { None };
        for inst in node.kids("instance_geometry") {
            let Some(geom) = inst.attr("url").and_then(get) else { continue };
            let Some(mesh) = geom.kid("mesh") else { continue };
            // Material symbol to material name.
            let mut symbols: HashMap<&str, String> = HashMap::new();
            let mut binds = Vec::new();
            inst.walk(&mut binds);
            for b in binds.into_iter().filter(|b| b.name == "instance_material") {
                let target = b.attr("target").unwrap_or_default();
                let name = get(target).and_then(|t| t.attr("name")).unwrap_or(target.trim_start_matches('#'));
                symbols.insert(b.attr("symbol").unwrap_or_default(), name.to_owned());
            }
            let out = geometry(mesh, &m, &get, &symbols, &mut materials, &mut material_index);
            found.push((name.to_owned(), size.unwrap_or(f32::NAN), out));
        }
    }
    // Geometry without a size of its own joins the most detailed level.
    let top = found.iter().map(|f| f.1).filter(|s| *s >= 0.0).fold(1.0f32, f32::max);
    let mut sizes: Vec<f32> = found.iter().map(|f| if f.1.is_nan() { top } else { f.1 }).collect();
    let mut levels = sizes.clone();
    levels.sort_by(|a, b| b.total_cmp(a));
    levels.dedup();
    let mut shape = Shape {
        nodes: vec![Node { name: 0, parent: -1 }],
        rotations: vec![[0.0, 0.0, 0.0, 1.0]],
        translations: vec![[0.0; 3]],
        names: vec!["root".into()],
        sub_shape_first_object: vec![0],
        sub_shape_object_count: vec![found.len() as i32],
        materials,
        ..Shape::default()
    };
    for (d, &size) in levels.iter().enumerate() {
        shape.details.push(Detail { name: shape.names.len() as i32, sub_shape: 0, object_detail: d as i32, size });
        shape.names.push(format!("detail{size}"));
    }
    // Each object holds a mesh in its own level's slot, null meshes in the others.
    for ((name, _, mesh), size) in found.into_iter().zip(sizes.drain(..)) {
        let slot = levels.iter().position(|&l| l == size).unwrap_or(0);
        let start = shape.meshes.len() as i32;
        let mut mesh = Some(mesh);
        for d in 0..levels.len() {
            let null = || Mesh { kind: 4, ..Mesh::default() };
            shape.meshes.push(if d == slot { mesh.take().unwrap_or_else(null) } else { null() });
        }
        shape.objects.push(Object {
            name: shape.names.len() as i32,
            mesh_count: levels.len() as i32,
            start_mesh: start,
            node: 0,
        });
        shape.names.push(name);
    }
    Ok(shape)
}

/// Torque's detail size from a node name under `start01`: a trailing number (`tree_a400` 400), negative after a
/// minus (`Colmesh-1`, collision).
fn detail_size(name: &str) -> Option<f32> {
    let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    let (head, number) = name.split_at(name.len() - digits);
    let n: f32 = number.parse().ok()?;
    Some(if head.ends_with('-') { -n } else { n })
}

fn geometry<'a>(
    mesh: &El,
    m: &Mat,
    get: &dyn Fn(&str) -> Option<&'a El>,
    symbols: &HashMap<&str, String>,
    materials: &mut Vec<String>,
    index: &mut HashMap<String, u32>,
) -> Mesh {
    let source = |r: &str| -> (Vec<f32>, usize) {
        let Some(s) = get(r) else { return (Vec::new(), 1) };
        // A `<vertices>` element points on to its POSITION source.
        if s.name == "vertices" {
            if let Some(src) =
                s.kids("input").find(|i| i.attr("semantic") == Some("POSITION")).and_then(|i| i.attr("source"))
            {
                if let Some(p) = get(src) {
                    return read_source(p);
                }
            }
            return (Vec::new(), 1);
        }
        read_source(s)
    };
    let normal_m = |n: [f64; 3]| -> [f32; 3] {
        let v: [f64; 3] = std::array::from_fn(|i| m[i][0] * n[0] + m[i][1] * n[1] + m[i][2] * n[2]);
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
        v.map(|x| (x / l) as f32)
    };
    let mut out = Mesh::default();
    for prim in mesh.kids.iter().filter(|k| k.name == "triangles" || k.name == "polylist") {
        let inputs: Vec<Input> = prim
            .kids("input")
            .filter(|i| i.attr("set").is_none_or(|s| s == "0"))
            .map(|i| {
                let off = i.attr("offset").and_then(|o| o.parse().ok()).unwrap_or(0);
                (i.attr("semantic").unwrap_or_default(), off, source(i.attr("source").unwrap_or_default()))
            })
            .collect();
        let stride = inputs.iter().map(|i| i.1).max().unwrap_or(0) + 1;
        let p: Vec<usize> = prim.kid("p").map(|p| p.numbers()).unwrap_or_default();
        let counts: Vec<usize> = match prim.kid("vcount") {
            Some(v) => v.numbers(),
            None => vec![3; p.len() / stride / 3],
        };
        let symbol = prim.attr("material").unwrap_or_default();
        let name = symbols.get(symbol).cloned().unwrap_or_else(|| symbol.to_owned());
        let mat = *index.entry(name.clone()).or_insert_with(|| {
            materials.push(name);
            materials.len() as u32 - 1
        });
        let start = out.indices.len() as u32;
        let corner = |c: usize, out: &mut Mesh| {
            let mut pos = [0.0f32; 3];
            let mut nrm = [0.0f32, 0.0, 1.0];
            let mut uv = [0.0f32; 2];
            for (sem, off, (data, w)) in &inputs {
                let Some(&i) = p.get(c * stride + off) else { continue };
                let v = |k: usize| data.get(i * w + k).copied().unwrap_or(0.0) as f64;
                match *sem {
                    "VERTEX" => {
                        let r: [f64; 3] =
                            std::array::from_fn(|r| m[r][0] * v(0) + m[r][1] * v(1) + m[r][2] * v(2) + m[r][3]);
                        pos = r.map(|x| x as f32);
                    }
                    "NORMAL" => nrm = normal_m([v(0), v(1), v(2)]),
                    "TEXCOORD" => uv = [v(0) as f32, 1.0 - v(1) as f32],
                    _ => {}
                }
            }
            out.indices.push(out.verts.len() as u32);
            out.verts.push(pos);
            out.norms.push(nrm);
            out.tverts.push(uv);
        };
        let mut c = 0;
        for n in counts {
            for k in 1..n.saturating_sub(1) {
                // Collada winds counter-clockwise; Torque's meshes clockwise.
                for j in [0, k + 1, k] {
                    corner(c + j, &mut out);
                }
            }
            c += n;
        }
        out.primitives.push(Primitive { start, count: out.indices.len() as u32 - start, material: mat });
    }
    out
}

fn read_source(s: &El) -> (Vec<f32>, usize) {
    let data = s.kid("float_array").map(|f| f.numbers()).unwrap_or_default();
    let stride = s
        .kid("technique_common")
        .and_then(|t| t.kid("accessor"))
        .and_then(|a| a.attr("stride"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    (data, stride)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_quad_as_two_clockwise_triangles() {
        let xml = r##"<?xml version="1.0"?><COLLADA><asset><up_axis>Z_UP</up_axis></asset>
<library_materials><material id="m-material" name="road"/></library_materials>
<library_geometries><geometry id="g"><mesh>
<source id="p"><float_array>0 0 0 1 0 0 1 1 0 0 1 0</float_array><technique_common><accessor stride="3"/></technique_common></source>
<vertices id="v"><input semantic="POSITION" source="#p"/></vertices>
<polylist material="m-material" count="1"><input semantic="VERTEX" source="#v" offset="0"/><vcount>4</vcount><p>0 1 2 3</p></polylist>
</mesh></geometry></library_geometries>
<library_visual_scenes><visual_scene id="s"><node name="n"><matrix>1 0 0 10 0 1 0 0 0 0 1 0 0 0 0 1</matrix>
<instance_geometry url="#g"><bind_material><technique_common><instance_material symbol="m-material" target="#m-material"/></technique_common></bind_material></instance_geometry></node></visual_scene></library_visual_scenes>
<scene><instance_visual_scene url="#s"/></scene></COLLADA>"##;
        let s = super::parse(xml).unwrap();
        assert_eq!(s.materials, ["road"]);
        let m = &s.meshes[0];
        assert_eq!(m.triangles().len(), 2);
        assert_eq!(m.verts[0], [10.0, 0.0, 0.0]);
        assert_eq!(m.verts[1], [11.0, 1.0, 0.0]);
    }

    #[test]
    fn detail_sizes_from_names() {
        assert_eq!(super::detail_size("tro_tree_2_mediuma400"), Some(400.0));
        assert_eq!(super::detail_size("Colmesh_tro_bush_small-1"), Some(-1.0));
        assert_eq!(super::detail_size("road"), None);
    }
}
