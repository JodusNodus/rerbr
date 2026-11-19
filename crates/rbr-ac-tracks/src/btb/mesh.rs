//! Writes a BTB mesh: a binary DirectX `.x` file (`xof 0303bin 0032`) holding the standard templates it uses and one
//! top-level `Mesh` with normals, one UV set and a one-material list, as `rbr_btb::xfile` reads BTB's own.

const NAME: u16 = 1;
const STRING: u16 = 2;
const INTEGER_LIST: u16 = 6;
const FLOAT_LIST: u16 = 7;
const OBRACE: u16 = 10;
const CBRACE: u16 = 11;
const OBRACKET: u16 = 14;
const CBRACKET: u16 = 15;
const DOT: u16 = 18;
const SEMICOLON: u16 = 20;
const TEMPLATE: u16 = 31;
const DWORD: u16 = 41;
const FLOAT: u16 = 42;
const LPSTR: u16 = 49;
const ARRAY: u16 = 52;

/// A mesh to write: triangles over per-vertex positions, normals and UVs, one texture.
pub struct XMesh<'a> {
    pub positions: &'a [[f32; 3]],
    pub normals: &'a [[f32; 3]],
    pub uvs: &'a [[f32; 2]],
    pub triangles: &'a [[u32; 3]],
    /// The material name (`materials.ini` section).
    pub material: &'a str,
}

struct Out(Vec<u8>);

impl Out {
    fn token(&mut self, t: u16) {
        self.0.extend_from_slice(&t.to_le_bytes());
    }

    fn name(&mut self, n: &str) {
        self.token(NAME);
        self.0.extend_from_slice(&(n.len() as u32).to_le_bytes());
        self.0.extend_from_slice(n.as_bytes());
    }

    fn ints(&mut self, v: &[u32]) {
        self.token(INTEGER_LIST);
        self.0.extend_from_slice(&(v.len() as u32).to_le_bytes());
        for x in v {
            self.0.extend_from_slice(&x.to_le_bytes());
        }
    }

    fn floats(&mut self, v: impl ExactSizeIterator<Item = f32>) {
        self.token(FLOAT_LIST);
        self.0.extend_from_slice(&(v.len() as u32).to_le_bytes());
        for x in v {
            self.0.extend_from_slice(&x.to_le_bytes());
        }
    }

    /// `template name { members [...] }`; a member is `(type token or struct name, name, array bound)`.
    fn template(&mut self, name: &str, members: &[(&str, &str, Option<&str>)], open: Option<&str>) {
        self.token(TEMPLATE);
        self.name(name);
        self.token(OBRACE);
        for &(ty, member, bound) in members {
            if bound.is_some() {
                self.token(ARRAY);
            }
            match ty {
                "DWORD" => self.token(DWORD),
                "FLOAT" => self.token(FLOAT),
                "STRING" => self.token(LPSTR),
                other => self.name(other),
            }
            self.name(member);
            if let Some(b) = bound {
                self.token(OBRACKET);
                self.name(b);
                self.token(CBRACKET);
            }
            self.token(SEMICOLON);
        }
        match open {
            Some("...") => {
                self.token(OBRACKET);
                for _ in 0..3 {
                    self.token(DOT);
                }
                self.token(CBRACKET);
            }
            Some(child) => {
                self.token(OBRACKET);
                self.name(child);
                self.token(CBRACKET);
            }
            None => {}
        }
        self.token(CBRACE);
    }
}

/// The `.x` bytes of `mesh`.
pub fn write(mesh: &XMesh) -> Vec<u8> {
    let mut o = Out(b"xof 0303bin 0032".to_vec());
    o.template("Vector", &[("FLOAT", "x", None), ("FLOAT", "y", None), ("FLOAT", "z", None)], None);
    o.template("Coords2d", &[("FLOAT", "u", None), ("FLOAT", "v", None)], None);
    o.template(
        "MeshFace",
        &[("DWORD", "nFaceVertexIndices", None), ("DWORD", "faceVertexIndices", Some("nFaceVertexIndices"))],
        None,
    );
    o.template(
        "Mesh",
        &[
            ("DWORD", "nVertices", None),
            ("Vector", "vertices", Some("nVertices")),
            ("DWORD", "nFaces", None),
            ("MeshFace", "faces", Some("nFaces")),
        ],
        Some("..."),
    );
    o.template(
        "MeshNormals",
        &[
            ("DWORD", "nNormals", None),
            ("Vector", "normals", Some("nNormals")),
            ("DWORD", "nFaceNormals", None),
            ("MeshFace", "faceNormals", Some("nFaceNormals")),
        ],
        None,
    );
    o.template(
        "MeshTextureCoords",
        &[("DWORD", "nTextureCoords", None), ("Coords2d", "textureCoords", Some("nTextureCoords"))],
        None,
    );
    o.template(
        "ColorRGBA",
        &[("FLOAT", "red", None), ("FLOAT", "green", None), ("FLOAT", "blue", None), ("FLOAT", "alpha", None)],
        None,
    );
    o.template("ColorRGB", &[("FLOAT", "red", None), ("FLOAT", "green", None), ("FLOAT", "blue", None)], None);
    o.template(
        "Material",
        &[
            ("ColorRGBA", "faceColor", None),
            ("FLOAT", "power", None),
            ("ColorRGB", "specularColor", None),
            ("ColorRGB", "emissiveColor", None),
        ],
        Some("..."),
    );
    o.template("TextureFilename", &[("STRING", "filename", None)], None);
    o.template(
        "MeshMaterialList",
        &[
            ("DWORD", "nMaterials", None),
            ("DWORD", "nFaceIndexes", None),
            ("DWORD", "faceIndexes", Some("nFaceIndexes")),
        ],
        Some("Material"),
    );

    let faces = |o: &mut Out| {
        let mut v = Vec::with_capacity(1 + mesh.triangles.len() * 4);
        v.push(mesh.triangles.len() as u32);
        for t in mesh.triangles {
            v.extend_from_slice(&[3, t[0], t[1], t[2]]);
        }
        o.ints(&v);
    };
    o.name("Mesh");
    o.token(OBRACE);
    o.ints(&[mesh.positions.len() as u32]);
    o.floats(mesh.positions.iter().flatten().copied().collect::<Vec<_>>().into_iter());
    faces(&mut o);

    o.name("MeshNormals");
    o.token(OBRACE);
    o.ints(&[mesh.normals.len() as u32]);
    o.floats(mesh.normals.iter().flatten().copied().collect::<Vec<_>>().into_iter());
    faces(&mut o);
    o.token(CBRACE);

    o.name("MeshTextureCoords");
    o.token(OBRACE);
    o.ints(&[mesh.uvs.len() as u32]);
    o.floats(mesh.uvs.iter().flatten().copied().collect::<Vec<_>>().into_iter());
    o.token(CBRACE);

    o.name("MeshMaterialList");
    o.token(OBRACE);
    let mut list = vec![1, mesh.triangles.len() as u32];
    list.extend(std::iter::repeat_n(0, mesh.triangles.len()));
    o.ints(&list);
    o.name("Material");
    o.token(OBRACE);
    o.floats([1.0f32, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0].into_iter());
    o.name("TextureFilename");
    o.token(OBRACE);
    o.token(STRING);
    o.0.extend_from_slice(&(mesh.material.len() as u32).to_le_bytes());
    o.0.extend_from_slice(mesh.material.as_bytes());
    o.token(SEMICOLON);
    o.token(CBRACE);
    o.token(CBRACE);
    o.token(CBRACE);

    o.token(CBRACE);
    o.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbr_btb::xfile;

    #[test]
    fn rbr_btb_reads_the_mesh_back() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.5, 1.0]];
        let normals = [[0.0, 1.0, 0.0]; 4];
        let uvs = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
        let triangles = [[0, 2, 1], [1, 2, 3]];
        let bytes = write(&XMesh {
            positions: &positions,
            normals: &normals,
            uvs: &uvs,
            triangles: &triangles,
            material: "surf\\asphalt",
        });
        let file = xfile::parse(&bytes).unwrap();
        let mesh = xfile::Mesh::decode(file.objects_of_type("Mesh")[0], &file).unwrap();
        assert_eq!(mesh.positions, positions.to_vec());
        assert_eq!(mesh.triangles(), vec![[0, 2, 1], [1, 2, 3]]);
        assert_eq!(mesh.normals.unwrap().normals, normals.to_vec());
        assert_eq!(mesh.uv_sets, vec![uvs.to_vec()]);
        let materials = mesh.materials.unwrap();
        assert_eq!(materials.face_materials, vec![0, 0]);
        assert_eq!(materials.materials[0].texture.as_deref(), Some("surf\\asphalt"));
    }
}
