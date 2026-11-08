//! BeamNG's cached Collada shapes (`.cdae`, version 31): a little-endian `u32` version (low byte 31, the exporter
//! version above bit 16), a `u32` header size, a MessagePack header map (`compression`, `bodysize`, ...), then the
//! body, Zstandard-compressed when `compression` is set. The body is a MessagePack stream holding Torque3D's `TSShape`
//! in the order of BeamNG's documentation (<https://documentation.beamng.com/modding/file_formats/cdae/>): shape
//! info, the shape vectors (each `{count, element size, bytes}`), names, meshes, sequences, materials. Element
//! layouts are Torque3D's (`ts/tsShape.h`, `ts/tsMesh.h`). Facts: `notes/beamng.md`.

use crate::msgpack::{Reader, Value};

/// A shape's node: its name and parent (`-1` for a root).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Node {
    pub name: i32,
    pub parent: i32,
}

/// A shape's object: one mesh per detail level from `start_mesh`, hung on `node`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Object {
    pub name: i32,
    pub mesh_count: i32,
    pub start_mesh: i32,
    pub node: i32,
}

/// A detail level: its name, subshape, the object detail number (the mesh slot of each object) and its size, negative
/// for the levels never drawn (collision).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Detail {
    pub name: i32,
    pub sub_shape: i32,
    pub object_detail: i32,
    pub size: f32,
}

/// A draw primitive: `count` indices from `start` with a material, as triangles, a strip or a fan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Primitive {
    pub start: u32,
    pub count: u32,
    /// The `TSDrawPrimitive` word: type in the top two bits (0 triangles, 1 strip, 2 fan), `NoMaterial` 0x10000000,
    /// the material index in the low 28 bits.
    pub material: u32,
}

impl Primitive {
    pub const NO_MATERIAL: u32 = 0x1000_0000;

    /// The material index, `None` for `NoMaterial`.
    pub fn material_index(&self) -> Option<usize> {
        (self.material & Self::NO_MATERIAL == 0).then_some((self.material & 0x0fff_ffff) as usize)
    }
}

/// A mesh's geometry. A mesh with a parent shares the parent's vertex data ([`Shape::mesh_vertices`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// 0 standard, 1 skin, 3 sorted, 4 null (no data).
    pub kind: u32,
    pub parent: i32,
    pub verts: Vec<[f32; 3]>,
    pub tverts: Vec<[f32; 2]>,
    pub norms: Vec<[f32; 3]>,
    pub primitives: Vec<Primitive>,
    pub indices: Vec<u32>,
}

impl Mesh {
    /// The triangles of its primitives, with each primitive's material word.
    pub fn triangles(&self) -> Vec<([u32; 3], u32)> {
        let mut out = Vec::new();
        for p in &self.primitives {
            let end = (p.start + p.count) as usize;
            let Some(idx) = self.indices.get(p.start as usize..end) else { continue };
            match p.material >> 30 {
                0 => out.extend(idx.chunks_exact(3).map(|t| ([t[0], t[1], t[2]], p.material))),
                1 => {
                    for (i, w) in idx.windows(3).enumerate() {
                        let t = if i % 2 == 0 { [w[0], w[1], w[2]] } else { [w[1], w[0], w[2]] };
                        out.push((t, p.material));
                    }
                }
                _ => out.extend(idx[1..].windows(2).map(|w| ([idx[0], w[0], w[1]], p.material))),
            }
        }
        out.retain(|(t, _)| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
        out
    }
}

/// A read shape: what the converter needs of it.
#[derive(Debug, Clone, Default)]
pub struct Shape {
    pub exporter: u32,
    pub nodes: Vec<Node>,
    pub objects: Vec<Object>,
    pub sub_shape_first_object: Vec<i32>,
    pub sub_shape_object_count: Vec<i32>,
    /// Per node, the default rotation `(x, y, z, w)` and translation.
    pub rotations: Vec<[f32; 4]>,
    pub translations: Vec<[f32; 3]>,
    pub details: Vec<Detail>,
    pub names: Vec<String>,
    pub meshes: Vec<Mesh>,
    /// The material list's names; primitives index it.
    pub materials: Vec<String>,
}

/// The body bytes of a `.cdae` file, decompressed.
pub fn body(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let word = |at: usize| -> Result<u32, String> {
        let b = bytes.get(at..at + 4).ok_or("cdae: shorter than its header")?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let version = word(0)?;
    if version & 0xff != 31 {
        return Err(format!("cdae: version {} (only 31 is read)", version & 0xff));
    }
    let header_size = word(4)? as usize;
    let header = bytes.get(8..8 + header_size).ok_or("cdae: header past the end")?;
    let header = Reader::new(header).value()?;
    let rest = &bytes[8 + header_size..];
    match header.get("compression") {
        Some(Value::Bool(true)) => {
            let mut out = Vec::new();
            let mut decoder = ruzstd::decoding::StreamingDecoder::new(rest).map_err(|e| format!("cdae: zstd: {e}"))?;
            std::io::Read::read_to_end(&mut decoder, &mut out).map_err(|e| format!("cdae: zstd: {e}"))?;
            Ok(out)
        }
        _ => Ok(rest.to_vec()),
    }
}

/// A `pack_vector`: count, element size, bytes.
struct Packed {
    count: usize,
    size: usize,
    bytes: Vec<u8>,
}

impl Packed {
    fn read(r: &mut Reader, what: &str) -> Result<Packed, String> {
        let count = r.int(what)?;
        let size = r.int(what)?;
        let bytes = r.bin(what)?;
        let (count, size) = (usize::try_from(count).unwrap_or(0), usize::try_from(size).unwrap_or(0));
        if bytes.len() != count * size {
            return Err(format!("{what}: {} bytes for {count} x {size}", bytes.len()));
        }
        Ok(Packed { count, size, bytes })
    }

    fn records(&self, what: &str, size: usize) -> Result<impl Iterator<Item = &[u8]>, String> {
        if self.count > 0 && self.size != size {
            return Err(format!("{what}: element size {} (expected {size})", self.size));
        }
        Ok(self.bytes.chunks_exact(size.max(1)))
    }

    fn i32s(&self, what: &str) -> Result<Vec<i32>, String> {
        Ok(self.records(what, 4)?.map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
    }

    fn floats<const N: usize>(&self, what: &str) -> Result<Vec<[f32; N]>, String> {
        Ok(self.records(what, 4 * N)?.map(|c| std::array::from_fn(|i| f32_at(c, 4 * i))).collect())
    }
}

/// Reads `n` numbers, written one by one or as arrays (`Point3F`, `Box3F`) or as raw `f32` bytes.
fn numbers(r: &mut Reader, n: usize, what: &str) -> Result<(), String> {
    let mut seen = 0;
    while seen < n {
        seen += match r.value()? {
            Value::Int(_) | Value::Float(_) => 1,
            Value::Array(items) => items.iter().map(|v| if let Value::Array(a) = v { a.len() } else { 1 }).sum(),
            Value::Bin(b) => b.len() / 4,
            v => return Err(format!("{what}: expected numbers, found {v:?}")),
        };
    }
    if seen != n {
        return Err(format!("{what}: {seen} numbers, expected {n}"));
    }
    Ok(())
}

fn i32_at(b: &[u8], at: usize) -> i32 {
    i32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Reads a whole `.cdae` file.
pub fn parse(bytes: &[u8]) -> Result<Shape, String> {
    let exporter = bytes.get(0..4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) >> 16);
    let body = body(bytes)?;
    let mut shape = parse_body(&body)?;
    shape.exporter = exporter;
    Ok(shape)
}

/// Reads a decompressed body.
pub fn parse_body(body: &[u8]) -> Result<Shape, String> {
    let r = &mut Reader::new(body);
    r.float("smallestVisibleSize")?;
    r.int("smallestVisibleDL")?;
    r.float("radius")?;
    r.float("tubeRadius")?;
    numbers(r, 9, "center/bounds")?;
    let nodes = Packed::read(r, "nodes")?;
    let nodes = nodes.records("nodes", 20)?.map(|c| Node { name: i32_at(c, 0), parent: i32_at(c, 4) }).collect();
    let objects = Packed::read(r, "objects")?;
    let objects = objects
        .records("objects", 24)?
        .map(|c| Object { name: i32_at(c, 0), mesh_count: i32_at(c, 4), start_mesh: i32_at(c, 8), node: i32_at(c, 12) })
        .collect();
    let _first_node = Packed::read(r, "subShapeFirstNode")?;
    let sub_shape_first_object = Packed::read(r, "subShapeFirstObject")?.i32s("subShapeFirstObject")?;
    let _node_count = Packed::read(r, "subShapeNumNodes")?;
    let sub_shape_object_count = Packed::read(r, "subShapeNumObjects")?.i32s("subShapeNumObjects")?;
    let rotations = Packed::read(r, "defaultRotations")?;
    let rotations = rotations
        .records("defaultRotations", 8)?
        .map(|c| std::array::from_fn(|i| f32::from(i16::from_le_bytes([c[2 * i], c[2 * i + 1]])) / 32767.0))
        .collect();
    let translations = Packed::read(r, "defaultTranslations")?.floats::<3>("defaultTranslations")?;
    for what in [
        "nodeRotations",
        "nodeTranslations",
        "nodeUniformScales",
        "nodeAlignedScales",
        "nodeArbitraryScaleFactors",
        "nodeArbitraryScaleRots",
        "groundTranslations",
        "groundRotations",
        "objectStates",
        "triggers",
    ] {
        Packed::read(r, what)?;
    }
    let details = Packed::read(r, "details")?;
    let details = details
        .records("details", 52)?
        .map(|c| Detail {
            name: i32_at(c, 0),
            sub_shape: i32_at(c, 4),
            object_detail: i32_at(c, 8),
            size: f32_at(c, 12),
        })
        .collect();
    let names = (0..r.int("names")?).map(|_| r.string_value("name")).collect::<Result<Vec<_>, _>>()?;
    let mesh_count = r.int("meshes")?;
    let mut meshes = Vec::with_capacity(usize::try_from(mesh_count).unwrap_or(0));
    for i in 0..mesh_count {
        meshes.push(mesh(r).map_err(|e| format!("mesh {i}: {e}"))?);
    }
    // Sequences: static levels carry none; skip what there is.
    let sequences = r.int("sequences")?;
    for _ in 0..sequences {
        for _ in 0..15 {
            r.value()?;
        }
        for _ in 0..6 {
            r.value()?;
        }
    }
    let mut materials = Vec::new();
    if !r.at_end() {
        for _ in 0..r.int("materials")? {
            materials.push(r.string_value("material name")?);
            for _ in 0..6 {
                r.value()?;
            }
        }
    }
    Ok(Shape {
        exporter: 0,
        nodes,
        objects,
        sub_shape_first_object,
        sub_shape_object_count,
        rotations,
        translations,
        details,
        names,
        meshes,
        materials,
    })
}

fn mesh(r: &mut Reader) -> Result<Mesh, String> {
    let kind = r.int("mesh type")? as u32;
    if kind == 4 {
        return Ok(Mesh { kind, parent: -1, ..Mesh::default() });
    }
    r.int("numFrames")?;
    r.int("numMatFrames")?;
    let parent = r.int("parentMesh")? as i32;
    numbers(r, 10, "bounds/center/radius")?;
    let verts = Packed::read(r, "verts")?.floats::<3>("verts")?;
    let tverts = Packed::read(r, "tverts")?.floats::<2>("tverts")?;
    Packed::read(r, "tverts2")?;
    Packed::read(r, "colors")?;
    let norms = Packed::read(r, "norms")?.floats::<3>("norms")?;
    Packed::read(r, "encodedNorms")?;
    let primitives = Packed::read(r, "primitives")?;
    let primitives = primitives
        .records("primitives", 12)?
        .map(|c| Primitive { start: i32_at(c, 0) as u32, count: i32_at(c, 4) as u32, material: i32_at(c, 8) as u32 })
        .collect();
    let indices = Packed::read(r, "indices")?;
    let indices = match indices.size {
        2 => indices.bytes.chunks_exact(2).map(|c| u32::from(u16::from_le_bytes([c[0], c[1]]))).collect(),
        _ => indices.i32s("indices")?.into_iter().map(|i| i as u32).collect(),
    };
    Packed::read(r, "tangents")?;
    r.int("vertsPerFrame")?;
    r.int("flags")?;
    if kind == 1 {
        if parent < 0 {
            Packed::read(r, "initialVerts")?;
            Packed::read(r, "initialNorms")?;
        }
        Packed::read(r, "initialTransforms")?;
        if parent < 0 {
            for what in ["vertexIndex", "boneIndex", "weight", "nodeIndex"] {
                Packed::read(r, what)?;
            }
        }
    }
    Ok(Mesh { kind, parent, verts, tverts, norms, primitives, indices })
}

impl Shape {
    pub fn name(&self, index: i32) -> &str {
        usize::try_from(index).ok().and_then(|i| self.names.get(i)).map_or("", String::as_str)
    }

    /// The mesh holding `mesh`'s vertex data (its parent when it has one and no data of its own).
    pub fn mesh_vertices(&self, mesh: usize) -> &Mesh {
        let m = &self.meshes[mesh];
        match usize::try_from(m.parent).ok().and_then(|p| self.meshes.get(p)) {
            Some(parent) if m.verts.is_empty() => parent,
            _ => m,
        }
    }

    /// The subshape an object belongs to.
    pub fn sub_shape_of(&self, object: usize) -> Option<usize> {
        let o = object as i32;
        (0..self.sub_shape_first_object.len()).find(|&s| {
            let first = self.sub_shape_first_object[s];
            let count = self.sub_shape_object_count.get(s).copied().unwrap_or(0);
            o >= first && o < first + count
        })
    }

    /// The object's meshes paired with the detail levels they draw at: the subshape's details in order, the n-th
    /// detail taking mesh `start + n` (`TSShape::getObjectDetails`). Null meshes are left out.
    pub fn object_meshes(&self, object: usize) -> Vec<(usize, &Detail)> {
        let o = &self.objects[object];
        let sub = self.sub_shape_of(object).map(|s| s as i32);
        let details = self.details.iter().filter(|d| Some(d.sub_shape) == sub);
        let mut out = Vec::new();
        for d in details {
            if d.object_detail < 0 || d.object_detail >= o.mesh_count {
                continue;
            }
            let mesh = (o.start_mesh + d.object_detail) as usize;
            if self.meshes.get(mesh).is_some_and(|m| m.kind != 4) {
                out.push((mesh, d));
            }
        }
        out
    }

    /// The node's transform to the shape frame, row-major with the translation in the last column: the default
    /// rotations and translations composed from the root (Torque's `QuatF::setMatrix`).
    pub fn node_transform(&self, node: usize) -> [[f64; 4]; 3] {
        let mut m = IDENTITY;
        let mut n = Some(node);
        let mut guard = 0;
        while let Some(i) = n {
            let local = local_transform(self.rotations.get(i), self.translations.get(i));
            m = compose(&local, &m);
            n = usize::try_from(self.nodes.get(i).map_or(-1, |n| n.parent)).ok();
            guard += 1;
            if guard > self.nodes.len() {
                break;
            }
        }
        m
    }
}

pub const IDENTITY: [[f64; 4]; 3] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];

/// Torque's `QuatF::setMatrix`: Torque quaternions rotate the other way from the textbook ones, so the matrix is the
/// transpose of the usual one.
fn local_transform(q: Option<&[f32; 4]>, t: Option<&[f32; 3]>) -> [[f64; 4]; 3] {
    let [x, y, z, w] = q.map_or([0.0, 0.0, 0.0, 1.0], |q| q.map(f64::from));
    let t = t.map_or([0.0; 3], |t| t.map(f64::from));
    if x * x + y * y + z * z < 1e-20 {
        return [[1.0, 0.0, 0.0, t[0]], [0.0, 1.0, 0.0, t[1]], [0.0, 0.0, 1.0, t[2]]];
    }
    let (xs, ys, zs) = (x * 2.0, y * 2.0, z * 2.0);
    let (wx, wy, wz) = (w * xs, w * ys, w * zs);
    let (xx, xy, xz) = (x * xs, x * ys, x * zs);
    let (yy, yz, zz) = (y * ys, y * zs, z * zs);
    [
        [1.0 - (yy + zz), xy + wz, xz - wy, t[0]],
        [xy - wz, 1.0 - (xx + zz), yz + wx, t[1]],
        [xz + wy, yz - wx, 1.0 - (xx + yy), t[2]],
    ]
}

/// `a` after `b`: `a * b` for column vectors.
pub fn compose(a: &[[f64; 4]; 3], b: &[[f64; 4]; 3]) -> [[f64; 4]; 3] {
    let mut out = [[0.0; 4]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum::<f64>() + if j == 3 { a[i][3] } else { 0.0 };
        }
    }
    out
}

pub fn apply(m: &[[f64; 4]; 3], p: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_and_fans_become_triangles() {
        let mesh = Mesh {
            indices: vec![0, 1, 2, 3, 4, 5, 6, 7],
            primitives: vec![
                Primitive { start: 0, count: 4, material: 1 << 30 },
                Primitive { start: 4, count: 4, material: 2 << 30 },
            ],
            ..Mesh::default()
        };
        let t: Vec<[u32; 3]> = mesh.triangles().into_iter().map(|(t, _)| t).collect();
        assert_eq!(t, vec![[0, 1, 2], [2, 1, 3], [4, 5, 6], [4, 6, 7]]);
    }

    #[test]
    fn a_node_chain_composes_from_the_root() {
        let shape = Shape {
            nodes: vec![Node { name: 0, parent: -1 }, Node { name: 1, parent: 0 }],
            rotations: vec![[0.0, 0.0, 0.0, 1.0]; 2],
            translations: vec![[10.0, 0.0, 0.0], [0.0, 2.0, 0.0]],
            ..Shape::default()
        };
        assert_eq!(apply(&shape.node_transform(1), [1.0, 1.0, 1.0]), [11.0, 3.0, 1.0]);
    }
}
