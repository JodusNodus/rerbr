//! KN5 v5/v6 reader. Format references, not copied code:
//! RFloEng/AC_to_SVJ/kn5_reader.py and gro-ove/actools/AcTools/Kn5File/Kn5Basic.cs.
//! Keeps the original AC/Direct3D frame, UVs and winding. Parent transforms are baked into vertices;
//! markers retain their composed transforms. No car-specific axis change or mesh filtering.

use std::collections::BTreeMap;
use std::path::Path;

pub type Matrix = [f32; 16];
pub const IDENTITY: Matrix = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];

#[derive(Debug, Default)]
pub struct Material {
    pub name: String,
    pub shader: String,
    pub blend: u8,
    pub alpha_test: bool,
    pub properties: BTreeMap<String, f32>,
    /// The two-component value that follows a property's scalar; per-axis detail multipliers live here.
    pub pairs: BTreeMap<String, [f32; 2]>,
    pub textures: BTreeMap<String, String>,
}

impl Material {
    pub fn property(&self, key: &str, default: f32) -> f32 {
        self.properties.get(key).copied().unwrap_or(default)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[derive(Debug)]
pub struct Mesh {
    pub name: String,
    pub helper: bool,
    pub active: bool,
    pub visible: bool,
    pub renderable: bool,
    pub material: usize,
    pub lod_in: f32,
    pub lod_out: f32,
    pub triangle_count: usize,
    pub vertices: Vec<Vertex>,
    pub triangles: Vec<[u32; 3]>,
}

#[derive(Debug)]
pub struct Model {
    pub version: u32,
    /// Windows texture names are case insensitive. Empty entries can refer to a different KN5.
    pub textures: BTreeMap<String, Vec<u8>>,
    pub materials: Vec<Material>,
    pub meshes: Vec<Mesh>,
    pub markers: BTreeMap<String, Matrix>,
    pub dropped_triangles: usize,
    pub repaired_uvs: usize,
    pub repaired_normals: usize,
}

pub fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    std::array::from_fn(|i| (0..4).map(|k| a[i / 4 * 4 + k] * b[k * 4 + i % 4]).sum())
}

pub fn point(m: &Matrix, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| p[0] * m[i] + p[1] * m[4 + i] + p[2] * m[8 + i] + m[12 + i])
}

fn determinant(m: &Matrix) -> f32 {
    m[0] * (m[5] * m[10] - m[6] * m[9]) - m[1] * (m[4] * m[10] - m[6] * m[8]) + m[2] * (m[4] * m[9] - m[5] * m[8])
}

fn normal(m: &Matrix, n: [f32; 3]) -> [f32; 3] {
    // Cofactor matrix / determinant is the inverse transpose, for a row-vector normal.
    let cof = [
        m[5] * m[10] - m[6] * m[9],
        m[6] * m[8] - m[4] * m[10],
        m[4] * m[9] - m[5] * m[8],
        m[2] * m[9] - m[1] * m[10],
        m[0] * m[10] - m[2] * m[8],
        m[1] * m[8] - m[0] * m[9],
        m[1] * m[6] - m[2] * m[5],
        m[2] * m[4] - m[0] * m[6],
        m[0] * m[5] - m[1] * m[4],
    ];
    let sign = determinant(m).signum();
    let q: [f32; 3] = std::array::from_fn(|i| sign * (n[0] * cof[i] + n[1] * cof[3 + i] + n[2] * cof[6 + i]));
    let length = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    if length > 1e-10 {
        q.map(|v| v / length)
    } else {
        n
    }
}

/// Models.ini rotations in degrees, X then Y then Z, followed by translation.
pub fn placement(position: [f32; 3], rotation: [f32; 3]) -> Matrix {
    let [x, y, z] = rotation.map(f32::to_radians);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    let rx = [1., 0., 0., 0., 0., cx, sx, 0., 0., -sx, cx, 0., 0., 0., 0., 1.];
    let ry = [cy, 0., -sy, 0., 0., 1., 0., 0., sy, 0., cy, 0., 0., 0., 0., 1.];
    let rz = [cz, sz, 0., 0., -sz, cz, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
    let mut out = multiply(&multiply(&rx, &ry), &rz);
    out[12..15].copy_from_slice(&position);
    out
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).ok_or("KN5 size overflow")?;
        let data = self
            .data
            .get(self.at..end)
            .ok_or_else(|| format!("KN5 truncated at byte {} reading {n} bytes", self.at))?;
        self.at = end;
        Ok(data)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn float(&mut self) -> Result<f32, String> {
        let v = f32::from_le_bytes(self.take(4)?.try_into().unwrap());
        if !v.is_finite() {
            return Err(format!("non-finite KN5 float at {}", self.at - 4));
        }
        Ok(v)
    }
    fn vector<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut v = [0.; N];
        for item in &mut v {
            *item = self.float()?;
        }
        Ok(v)
    }
    fn raw_vector<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut out = [0.; N];
        for v in &mut out {
            *v = f32::from_le_bytes(self.take(4)?.try_into().unwrap());
        }
        Ok(out)
    }
    fn count(&mut self, stride: usize) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n > (self.data.len() - self.at) / stride.max(1) {
            return Err(format!("invalid KN5 count {n} at {}", self.at - 4));
        }
        Ok(n)
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.count(1)?;
        if n > 65_536 {
            return Err("KN5 string too long".into());
        }
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn node(
        &mut self,
        model: &mut Model,
        parent: &Matrix,
        active: bool,
        geometry: bool,
        depth: usize,
        helper: bool,
    ) -> Result<(), String> {
        if depth > 128 {
            return Err("KN5 node hierarchy too deep".into());
        }
        let kind = self.u32()?;
        let name = self.string()?;
        let helper = helper || name.starts_with("AC_");
        let children = self.count(13)?;
        let active = self.byte()? != 0 && active;
        let mut transform = *parent;
        match kind {
            1 => {
                transform = multiply(&self.vector::<16>()?, parent);
                // Timing/spawn helpers commonly have Active=0 to hide their debug meshes.
                // Their transforms still define the course.
                if name.starts_with("AC_") {
                    model.markers.insert(name, transform);
                }
            }
            2 => {
                self.byte()?; // casts shadows
                let visible = self.byte()? != 0;
                self.byte()?; // transparent
                let vertex_count = self.count(44)?;
                let mut vertices = Vec::new();
                let mut bad_normals = Vec::new();
                if geometry {
                    vertices.reserve(vertex_count);
                    for _ in 0..vertex_count {
                        let p = self.raw_vector::<3>()?;
                        let n = self.raw_vector::<3>()?;
                        let mut uv = self.raw_vector::<2>()?;
                        if !uv.iter().all(|v| v.is_finite()) {
                            model.repaired_uvs += 1;
                            for v in &mut uv {
                                if !v.is_finite() {
                                    *v = 0.;
                                }
                            }
                        }
                        let normal = if n.iter().all(|v| v.is_finite()) && n.iter().map(|v| v * v).sum::<f32>() > 1e-12
                        {
                            normal(&transform, n)
                        } else {
                            bad_normals.push(vertices.len());
                            model.repaired_normals += 1;
                            [0.; 3]
                        };
                        self.take(12)?;
                        vertices.push(Vertex { position: point(&transform, p), normal, uv });
                    }
                } else {
                    self.take(vertex_count * 44)?;
                }
                let index_count = self.count(2)?;
                if index_count % 3 != 0 {
                    return Err(format!("mesh {name}: incomplete triangle"));
                }
                let mut triangles = Vec::new();
                if geometry {
                    triangles.reserve(index_count / 3);
                    for _ in 0..index_count / 3 {
                        let mut t = [0; 3];
                        for index in &mut t {
                            *index = u16::from_le_bytes(self.take(2)?.try_into().unwrap()) as u32;
                            if *index as usize >= vertex_count {
                                return Err(format!("mesh {name}: index out of bounds"));
                            }
                        }
                        if determinant(&transform) < 0. {
                            t.swap(1, 2);
                        }
                        if t.iter().any(|&i| !vertices[i as usize].position.iter().all(|v| v.is_finite())) {
                            model.dropped_triangles += 1;
                        } else {
                            triangles.push(t);
                        }
                    }
                } else {
                    self.take(index_count * 2)?;
                }
                let material = self.u32()? as usize;
                if material >= model.materials.len() {
                    return Err(format!("mesh {name}: invalid material {material}"));
                }
                self.u32()?; // layer
                let lod_in = self.float()?;
                let lod_out = self.float()?;
                self.take(16)?; // bounding sphere
                let renderable = self.byte()? != 0;
                if !bad_normals.is_empty() {
                    let mut sum = vec![[0.; 3]; vertices.len()];
                    for t in &triangles {
                        let [a, b, c] = t.map(|i| vertices[i as usize].position);
                        let u = std::array::from_fn::<_, 3, _>(|k| b[k] - a[k]);
                        let v = std::array::from_fn::<_, 3, _>(|k| c[k] - a[k]);
                        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                        for &i in t {
                            for (s, n) in sum[i as usize].iter_mut().zip(n) {
                                *s += n;
                            }
                        }
                    }
                    for i in bad_normals {
                        let length = sum[i].iter().map(|v| v * v).sum::<f32>().sqrt();
                        vertices[i].normal = if length > 1e-10 { sum[i].map(|v| v / length) } else { [0., 1., 0.] };
                    }
                }
                model.meshes.push(Mesh {
                    name,
                    helper,
                    active,
                    visible,
                    renderable,
                    material,
                    lod_in,
                    lod_out,
                    triangle_count: index_count / 3,
                    vertices,
                    triangles,
                });
            }
            3 => return Err(format!("skinned track mesh {name} is not supported")),
            _ => {
                return Err(format!(
                    "unsupported KN5 node type {kind} ({name}); protected/new-format files are not supported"
                ))
            }
        }
        for _ in 0..children {
            self.node(model, &transform, active, geometry, depth + 1, helper)?;
        }
        Ok(())
    }
}

pub fn read(path: &Path, geometry: bool, textures: bool, transform: &Matrix) -> Result<Model, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&data, geometry, textures, transform).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn parse(data: &[u8], geometry: bool, textures: bool, transform: &Matrix) -> Result<Model, String> {
    let mut r = Reader { data, at: 0 };
    if r.take(6)? != b"sc6969" {
        return Err("not an ordinary KN5 file; protected files are not supported".into());
    }
    let version = r.u32()?;
    if !matches!(version, 5 | 6) {
        return Err(format!("unsupported KN5 version {version}, expected 5 or 6"));
    }
    if version == 6 {
        r.u32()?;
    }
    let mut model = Model {
        version,
        textures: BTreeMap::new(),
        materials: Vec::new(),
        meshes: Vec::new(),
        markers: BTreeMap::new(),
        dropped_triangles: 0,
        repaired_uvs: 0,
        repaired_normals: 0,
    };
    let count = r.count(12)?;
    for _ in 0..count {
        r.u32()?; // active flag; a length follows even when inactive (AcTools reader/writer).
        let name = r.string()?.to_lowercase();
        let n = r.count(1)?;
        let data = r.take(n)?;
        if textures && !data.is_empty() {
            model.textures.insert(name, data.to_vec());
        }
    }
    let count = r.count(18)?;
    for _ in 0..count {
        let name = r.string()?;
        let shader = r.string()?;
        let blend = r.byte()?;
        let alpha_test = r.byte()? != 0;
        r.u32()?; // depth mode
        let mut material = Material { name, shader, blend, alpha_test, ..Material::default() };
        let count = r.count(44)?;
        for _ in 0..count {
            let name = r.string()?;
            let value = r.float()?;
            let raw = r.take(36)?;
            let pair = std::array::from_fn(|k| f32::from_le_bytes(raw[k * 4..k * 4 + 4].try_into().unwrap()));
            material.pairs.insert(name.clone(), pair);
            material.properties.insert(name, value);
        }
        let count = r.count(12)?;
        for _ in 0..count {
            let sampler = r.string()?;
            r.u32()?;
            material.textures.insert(sampler, r.string()?.to_lowercase());
        }
        model.materials.push(material);
    }
    r.node(&mut model, transform, true, geometry, 0, false)?;
    // Custom Shaders Patch tools append per-mesh data (e.g. vertex AO) after the node tree. It is not geometry.
    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transforms_compose_child_before_parent_and_normals_use_inverse_transpose() {
        let a = placement([2., 0., 0.], [0.; 3]);
        let b = placement([0., 0., 3.], [0., 90., 0.]);
        let q = point(&multiply(&a, &b), [0.; 3]);
        assert!(q.iter().zip([0., 0., 1.]).all(|(a, b)| (a - b).abs() < 1e-5));
        let mut scale = IDENTITY;
        scale[0] = 2.;
        let n = normal(&scale, [1., 1., 0.]);
        assert!((n[1] / n[0] - 2.).abs() < 1e-5);
    }
    #[test]
    fn data_after_the_node_tree_is_ignored() {
        let mut data = b"sc6969\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0".to_vec(); // no textures or materials
        data.extend([1, 0, 0, 0, 1, 0, 0, 0, b'a', 0, 0, 0, 0, 1]); // one transform node
        data.extend(IDENTITY.iter().flat_map(|v| v.to_le_bytes()));
        assert!(parse(&data, true, true, &IDENTITY).unwrap().meshes.is_empty());
        data.extend(b"ver.mesh");
        assert!(parse(&data, true, true, &IDENTITY).unwrap().meshes.is_empty());
    }
    #[test]
    fn truncated_and_protected_files_return_errors() {
        for data in [b"sc6969".as_slice(), b"encrypted", b"sc6969\x06\0\0\0"] {
            assert!(parse(data, true, true, &IDENTITY).is_err());
        }
    }
}
