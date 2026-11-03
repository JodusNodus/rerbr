//! The BTB meshes of an RSF install (`RBR_RSF`): every `.x` and `.xform` in
//! every `RX_CONTENT/Tracks/*/Objects.zip` parses, and every `objectlist.ini`
//! `file=`/`xform=` name resolves to a zip entry. Skipped when `RBR_RSF` is
//! unset. Also see `notes/btb.md` "Meshes".

use rbr_btb::{xfile, xform};
use rbr_formats::rbz;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn rsf() -> Option<PathBuf> {
    std::env::var_os("RBR_RSF").map(PathBuf::from)
}

fn tracks(root: &std::path::Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root.join("RX_CONTENT/Tracks"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Latin-1 bytes to `String` (the game's `GetPrivateProfileStringA` reads
/// bytes; RSF installs carry non-UTF-8 ones).
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

#[test]
fn every_btb_mesh_and_placement_parses_and_objectlists_resolve() {
    let Some(root) = rsf() else {
        eprintln!("skipping: RBR_RSF not set");
        return;
    };
    let tracks = tracks(&root);
    assert!(!tracks.is_empty(), "{}: no RX_CONTENT/Tracks", root.display());
    let mut n_meshes = 0;
    let mut n_xforms = 0;
    let mut n_placements = 0;
    let mut n_objects = 0;
    let mut templates: BTreeMap<String, usize> = BTreeMap::new();
    let mut poly_sizes: BTreeMap<usize, usize> = BTreeMap::new();
    let mut uv_sets: BTreeMap<usize, usize> = BTreeMap::new();
    let mut with_colors = 0;
    let mut with_materials = 0;
    let mut nonzero_face_colors = 0;
    let mut face_material_cover: BTreeMap<usize, usize> = BTreeMap::new();
    let mut textures: BTreeSet<String> = BTreeSet::new();
    let mut decl_elements: BTreeMap<Vec<xfile::VertexElement>, usize> = BTreeMap::new();
    let mut decl_stride_bad = 0;
    let mut fvf_codes: BTreeMap<u32, usize> = BTreeMap::new();
    let mut fvf_len_bad = 0;
    let mut uv_count_bad = 0;
    let mut normal_count_bad = 0;
    let mut matrix_w_bad = 0;
    let mut unmatched_textures: Vec<String> = Vec::new();
    for folder in &tracks {
        let track = folder.file_name().unwrap().to_string_lossy().into_owned();
        let zip_path = folder.join("Objects.zip");
        let mut zip = rbz::open(&zip_path).unwrap_or_else(|e| panic!("{}: {e}", zip_path.display()));
        let mut entries = BTreeSet::new();
        let mut x_names = Vec::new();
        let mut xform_names = Vec::new();
        for i in 0..zip.len() {
            let file = zip.by_index(i).unwrap();
            let name = file.name().map(|name| name.into_owned()).unwrap();
            entries.insert(name.to_lowercase());
            if name.to_lowercase().ends_with(".x") {
                x_names.push(name);
            } else if name.to_lowercase().ends_with(".xform") {
                xform_names.push(name);
            }
        }
        // materials.ini section names for the texture cross-check below.
        let mat_text = latin1(&std::fs::read(folder.join("materials.ini")).unwrap());
        let mut sections = BTreeSet::new();
        for line in mat_text.lines() {
            let line = line.trim();
            if line.starts_with('[') && line.ends_with(']') {
                sections.insert(line[1..line.len() - 1].to_lowercase());
            }
        }
        for name in &x_names {
            let mut bytes = Vec::new();
            zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
            let file = xfile::parse(&bytes).unwrap_or_else(|e| panic!("{track}/{name}: {e}"));
            for template in &file.templates {
                *templates.entry(template.name.clone()).or_default() += 1;
            }
            let meshes = file.objects_of_type("Mesh");
            assert_eq!(meshes.len(), 1, "{track}/{name}: {} Mesh objects", meshes.len());
            let mesh = xfile::Mesh::decode(meshes[0], &file).unwrap_or_else(|e| panic!("{track}/{name}: {e}"));
            n_meshes += 1;
            for face in &mesh.faces {
                *poly_sizes.entry(face.0.len()).or_default() += 1;
            }
            *uv_sets.entry(mesh.uv_sets.len()).or_default() += 1;
            for uvs in &mesh.uv_sets {
                if uvs.len() != mesh.positions.len() {
                    uv_count_bad += 1;
                }
            }
            if mesh.colors.is_some() {
                with_colors += 1;
            }
            if let Some(normals) = &mesh.normals {
                if normals.normals.len() != mesh.positions.len() || normals.faces.len() != mesh.faces.len() {
                    normal_count_bad += 1;
                }
            } else {
                normal_count_bad += 1;
            }
            if let Some(materials) = &mesh.materials {
                with_materials += 1;
                *face_material_cover.entry(materials.materials.len()).or_default() += 1;
                assert_eq!(materials.face_materials.len(), mesh.faces.len(), "{track}/{name}: material index per face");
                for entry in &materials.materials {
                    let xfile::MaterialEntry::Material(material) = entry else {
                        panic!("{track}/{name}: material reference");
                    };
                    if material.face_color != [0.0; 4]
                        || material.power != 0.0
                        || material.specular != [0.0; 3]
                        || material.emissive != [0.0; 3]
                    {
                        nonzero_face_colors += 1;
                    }
                    match &material.texture {
                        Some(xfile::Texture::File(name)) => {
                            textures.insert(name.clone());
                            if !sections.contains(&name.to_lowercase()) && unmatched_textures.len() < 20 {
                                unmatched_textures.push(format!("{track}: {name}"));
                            }
                        }
                        other => panic!("{track}/{name}: unexpected texture {other:?}"),
                    }
                }
            }
            for decl in &mesh.decl {
                *decl_elements.entry(decl.elements.clone()).or_default() += 1;
                if decl.stride_dwords().is_none_or(|s| s * mesh.positions.len() != decl.data.len()) {
                    decl_stride_bad += 1;
                }
            }
            for fvf in &mesh.fvf {
                *fvf_codes.entry(fvf.code).or_default() += 1;
                if fvf.data.len() != fvf.uv_set_count() * 2 * mesh.positions.len() {
                    fvf_len_bad += 1;
                }
            }
            // The fan triangulation covers every polygon vertex.
            let tris = mesh.triangles();
            let expected: usize = mesh.faces.iter().map(|f| f.0.len().saturating_sub(2)).sum();
            assert_eq!(tris.len(), expected, "{track}/{name}: triangulation");
        }
        for name in &xform_names {
            let mut bytes = Vec::new();
            zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
            let matrices = xform::parse(&bytes).unwrap_or_else(|e| panic!("{track}/{name}: {e}"));
            n_xforms += 1;
            n_placements += matrices.len();
            for m in &matrices {
                if m[15] != 1.0 {
                    matrix_w_bad += 1;
                }
            }
        }
        // Every objectlist.ini mesh and placement name resolves (case-insensitive).
        let ini_text = latin1(&std::fs::read(folder.join("objectlist.ini")).unwrap());
        let mut section = String::new();
        let mut file = String::new();
        let mut xform = String::new();
        let mut flush = |section: &str, file: &str, xform: &str| {
            if section.to_lowercase().starts_with("object_") {
                assert!(!file.is_empty(), "{track}/{section}: no file=");
                assert!(
                    entries.contains(&format!("objects/{file}.x").to_lowercase()),
                    "{track}/{section}: {file}.x is not in Objects.zip"
                );
                assert!(
                    entries.contains(&format!("objects/{xform}.xform").to_lowercase()),
                    "{track}/{section}: {xform}.xform is not in Objects.zip"
                );
                n_objects += 1;
            }
        };
        for line in ini_text.lines() {
            let line = line.trim();
            if line.starts_with('[') && line.ends_with(']') {
                flush(&section, &file, &xform);
                section = line[1..line.len() - 1].to_string();
                file.clear();
                xform.clear();
            } else if let Some((key, value)) = line.split_once('=') {
                match key.trim().to_lowercase().as_str() {
                    "file" => file = value.trim().to_string(),
                    "xform" => xform = value.trim().to_string(),
                    _ => {}
                }
            }
        }
        flush(&section, &file, &xform);
    }
    eprintln!(
        "{} tracks, {n_meshes} meshes, {n_xforms} xforms ({n_placements} placements), {n_objects} objectlist entries",
        tracks.len()
    );
    eprintln!("templates: {templates:?}");
    eprintln!("polygon sizes: {poly_sizes:?}");
    eprintln!("uv sets per mesh: {uv_sets:?} (meshes with vertex colours: {with_colors})");
    eprintln!("meshes with materials: {with_materials} (nonzero face colours: {nonzero_face_colors})");
    eprintln!("material counts per mesh: {face_material_cover:?}");
    eprintln!("{} distinct textures", textures.len());
    eprintln!("DeclData element patterns:");
    for (elements, count) in &decl_elements {
        eprintln!("  {count}: {elements:?}");
    }
    eprintln!("FVFData codes: {fvf_codes:?}");
    assert_eq!(uv_count_bad, 0, "meshes whose UV count differs from the vertex count");
    assert_eq!(normal_count_bad, 0, "meshes without full normals");
    assert_eq!(decl_stride_bad, 0, "DeclData payloads not stride * vertices");
    assert_eq!(fvf_len_bad, 0, "FVFData payloads not sets * 2 * vertices");
    assert_eq!(matrix_w_bad, 0, "xform matrices not ending in 1.0");
    if !unmatched_textures.is_empty() {
        eprintln!("textures without a materials.ini section: {unmatched_textures:?}");
    }
}

use std::io::Read;

// --- Small synthetic inputs (no game data). ---

fn binary_file(tokens: &[u8]) -> Vec<u8> {
    let mut out = b"xof 0303bin 0032".to_vec();
    out.extend_from_slice(tokens);
    out
}

fn name_token(out: &mut Vec<u8>, token: u16, text: &[u8]) {
    out.extend_from_slice(&token.to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text);
}

fn ints_token(out: &mut Vec<u8>, values: &[i32]) {
    out.extend_from_slice(&6u16.to_le_bytes());
    out.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

fn floats_token(out: &mut Vec<u8>, values: &[f32]) {
    out.extend_from_slice(&7u16.to_le_bytes());
    out.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

/// A minimal binary file: one template with a fixed array and one data
/// object whose integer and float values share merged lists.
#[test]
fn synthetic_binary_template_and_data() {
    // template Seg { <uuid> DWORD n; array FLOAT v[n]; [...] }
    let mut t = Vec::new();
    t.extend_from_slice(&31u16.to_le_bytes());
    name_token(&mut t, 1, b"Seg");
    t.extend_from_slice(&10u16.to_le_bytes());
    t.extend_from_slice(&5u16.to_le_bytes());
    t.extend_from_slice(&[1u8; 16]);
    t.extend_from_slice(&41u16.to_le_bytes());
    name_token(&mut t, 1, b"n");
    t.extend_from_slice(&20u16.to_le_bytes());
    t.extend_from_slice(&52u16.to_le_bytes());
    t.extend_from_slice(&42u16.to_le_bytes());
    name_token(&mut t, 1, b"v");
    t.extend_from_slice(&14u16.to_le_bytes());
    name_token(&mut t, 1, b"n");
    t.extend_from_slice(&15u16.to_le_bytes());
    t.extend_from_slice(&20u16.to_le_bytes());
    t.extend_from_slice(&14u16.to_le_bytes());
    for _ in 0..3 {
        t.extend_from_slice(&18u16.to_le_bytes());
    }
    t.extend_from_slice(&15u16.to_le_bytes());
    t.extend_from_slice(&11u16.to_le_bytes());
    // Seg data { 2; 1.0, 2.0; }
    name_token(&mut t, 1, b"Seg");
    name_token(&mut t, 1, b"data");
    t.extend_from_slice(&10u16.to_le_bytes());
    ints_token(&mut t, &[2]);
    floats_token(&mut t, &[1.0, 2.0]);
    t.extend_from_slice(&11u16.to_le_bytes());
    let file = xfile::parse(&binary_file(&t)).unwrap();
    assert_eq!(file.templates.len(), 1);
    assert_eq!(file.templates[0].name, "Seg");
    assert_eq!(file.templates[0].uuid, Some([1u8; 16]));
    assert_eq!(file.templates[0].restrictions, xfile::Restrictions::Any);
    let object = match &file.roots[0] {
        xfile::Node::Object(object) => object,
        other => panic!("{other:?}"),
    };
    assert_eq!(object.ty, "Seg");
    assert_eq!(object.name.as_deref(), Some("data"));
    let decoded = xfile::decode_object(object, &file).unwrap();
    assert_eq!(decoded.members[0].name, "n");
    assert_eq!(decoded.members[1].value, xfile::MemberValue::Floats(vec![1.0, 2.0]));
}

/// The same shape as text, with a `{ name }` reference child.
#[test]
fn synthetic_text_template_data_and_reference() {
    let text = r#"xof 0303txt 0032
template Seg {
 <12345678-1234-1234-1234-123456789abc>
 DWORD n;
 array FLOAT v[n];
}
Seg data {
 2;
 1.0, 2.0;
 { other }
}
Seg other {
 1;
 3.0;
}
"#;
    let file = xfile::parse(text.as_bytes()).unwrap();
    assert!(file.templates[0].uuid.is_some());
    let object = match &file.roots[0] {
        xfile::Node::Object(object) => object,
        other => panic!("{other:?}"),
    };
    assert_eq!(object.children.len(), 1);
    assert_eq!(object.children[0], xfile::Node::Ref("other".to_string()));
    let decoded = xfile::decode_object(object, &file).unwrap();
    assert_eq!(decoded.members[1].value, xfile::MemberValue::Floats(vec![1.0, 2.0]));
    assert_eq!(file.find_named("other").unwrap().ty, "Seg");
}

/// Frames nest, matrices compose child-before-parent, meshes are found.
#[test]
fn synthetic_text_frames() {
    let text = r#"xof 0303txt 0032
template Vector {
 FLOAT x;
 FLOAT y;
 FLOAT z;
}
template MeshFace {
 DWORD nFaceVertexIndices;
 array DWORD faceVertexIndices[nFaceVertexIndices];
}
template Mesh {
 DWORD nVertices;
 array Vector vertices[nVertices];
 DWORD nFaces;
 array MeshFace faces[nFaces];
 [...]
}
template FrameTransformMatrix {
 FLOAT frameMatrix[16];
}
template Frame {
 [...]
}
Frame root {
 FrameTransformMatrix {
  2.0,0.0,0.0,0.0, 0.0,2.0,0.0,0.0, 0.0,0.0,2.0,0.0, 4.0,0.0,0.0,1.0;;
 }
 Frame child {
  Mesh tri {
   3; 0.0;0.0;0.0;; 1.0;0.0;0.0;; 0.0;1.0;0.0;;; 1; 3,0,1,2;;
  }
 }
}
"#;
    let file = xfile::parse(text.as_bytes()).unwrap();
    let root = match &file.roots[0] {
        xfile::Node::Object(object) => xfile::Frame::decode(object, &file).unwrap(),
        other => panic!("{other:?}"),
    };
    assert_eq!(root.name.as_deref(), Some("root"));
    assert_eq!(root.frames.len(), 1);
    assert_eq!(root.frames[0].name.as_deref(), Some("child"));
    let found = root.meshes_with_matrices();
    assert_eq!(found.len(), 1);
    // Row-vector order: the mesh takes the parent matrix as is (its
    // translation is not scaled by its own scale).
    assert_eq!(found[0].0, [2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 4.0, 0.0, 0.0, 1.0]);
    let mesh = xfile::Mesh::decode(found[0].1, &file).unwrap();
    assert_eq!(mesh.positions.len(), 3);
    assert_eq!(mesh.triangles(), vec![[0, 1, 2]]);
}

/// Malformed inputs fail with a named error, not a panic.
#[test]
fn synthetic_errors() {
    assert!(matches!(xfile::parse(b"short"), Err(xfile::Error::BadHeader(_))));
    assert!(matches!(xfile::parse(b"xof 0303zip 0032"), Err(xfile::Error::BadHeader(_))));
    assert!(matches!(xfile::parse(b"xof 0303bzip0032"), Err(xfile::Error::Compressed(_))));
    let mut truncated = binary_file(&[]);
    truncated.truncate(20);
    // A header plus half a token truncates.
    truncated.extend_from_slice(&1u16.to_le_bytes());
    assert!(matches!(xfile::parse(&truncated), Err(xfile::Error::Truncated { .. })));
    // Data of an undeclared template.
    let mut t = Vec::new();
    name_token(&mut t, 1, b"Nope");
    t.extend_from_slice(&10u16.to_le_bytes());
    t.extend_from_slice(&11u16.to_le_bytes());
    let file = xfile::parse(&binary_file(&t)).unwrap();
    let object = match &file.roots[0] {
        xfile::Node::Object(object) => object.clone(),
        other => panic!("{other:?}"),
    };
    assert!(matches!(xfile::decode_object(&object, &file), Err(xfile::Error::UnknownTemplate(_))));
}

/// `.xform` is raw 4x4 matrices; a partial matrix misaligns.
#[test]
fn synthetic_xform() {
    let mut bytes = vec![0u8; 128];
    for (i, slot) in bytes.chunks_exact_mut(4).enumerate() {
        slot.copy_from_slice(&(i as f32).to_le_bytes());
    }
    let matrices = xform::parse(&bytes).unwrap();
    assert_eq!(matrices.len(), 2);
    assert_eq!(matrices[0][0], 0.0);
    assert_eq!(matrices[1][15], 31.0);
    assert!(matches!(xform::parse(&bytes[..100]), Err(xform::Error::Misaligned { len: 100 })));
    assert!(matches!(xform::parse(&[]), Ok(v) if v.is_empty()));
}
