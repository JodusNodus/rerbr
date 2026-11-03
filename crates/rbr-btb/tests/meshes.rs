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
    let mut with_materials = 0;
    let mut textures: BTreeSet<String> = BTreeSet::new();
    let mut decl_elements: BTreeMap<Vec<xfile::VertexElement>, usize> = BTreeMap::new();
    let mut fvf_codes: BTreeMap<u32, usize> = BTreeMap::new();
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
            for mesh in file.objects_of_type("Mesh") {
                let mesh = xfile::Mesh::decode(mesh, &file).unwrap_or_else(|e| panic!("{track}/{name}: {e}"));
                n_meshes += 1;
                for face in &mesh.faces {
                    *poly_sizes.entry(face.0.len()).or_default() += 1;
                }
                *uv_sets.entry(mesh.uv_sets.len()).or_default() += 1;
                if let Some(materials) = &mesh.materials {
                    with_materials += 1;
                    for material in &materials.materials {
                        if let Some(name) = &material.texture {
                            textures.insert(name.clone());
                            if !sections.contains(&name.to_lowercase()) && unmatched_textures.len() < 20 {
                                unmatched_textures.push(format!("{track}: {name}"));
                            }
                        }
                    }
                }
                for decl in &mesh.decl {
                    *decl_elements.entry(decl.elements.clone()).or_default() += 1;
                }
                for fvf in &mesh.fvf {
                    *fvf_codes.entry(fvf.code).or_default() += 1;
                }
            }
        }
        for name in &xform_names {
            let mut bytes = Vec::new();
            zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
            let matrices = xform::parse(&bytes).unwrap_or_else(|e| panic!("{track}/{name}: {e}"));
            n_xforms += 1;
            n_placements += matrices.len();
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
    eprintln!("uv sets per mesh: {uv_sets:?}");
    eprintln!("meshes with materials: {with_materials}");
    eprintln!("{} distinct textures", textures.len());
    eprintln!("DeclData element patterns:");
    for (elements, count) in &decl_elements {
        eprintln!("  {count}: {elements:?}");
    }
    eprintln!("FVFData codes: {fvf_codes:?}");
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

fn str_token(out: &mut Vec<u8>, text: &[u8]) {
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text);
}

fn obrace(out: &mut Vec<u8>) {
    out.extend_from_slice(&10u16.to_le_bytes());
}

fn cbrace(out: &mut Vec<u8>) {
    out.extend_from_slice(&11u16.to_le_bytes());
}

/// A minimal binary file: one template (with uuid, a counted array and a
/// string member) and one data object whose values share merged lists.
#[test]
fn synthetic_binary_template_and_data() {
    // template Seg { <uuid> DWORD n; array FLOAT v[n]; LPSTR s; [...] }
    let mut t = Vec::new();
    t.extend_from_slice(&31u16.to_le_bytes());
    name_token(&mut t, 1, b"Seg");
    obrace(&mut t);
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
    t.extend_from_slice(&49u16.to_le_bytes());
    name_token(&mut t, 1, b"s");
    t.extend_from_slice(&20u16.to_le_bytes());
    t.extend_from_slice(&14u16.to_le_bytes());
    for _ in 0..3 {
        t.extend_from_slice(&18u16.to_le_bytes());
    }
    t.extend_from_slice(&15u16.to_le_bytes());
    cbrace(&mut t);
    // Seg data { 2; 1.0, 2.0; "hi"; }
    name_token(&mut t, 1, b"Seg");
    name_token(&mut t, 1, b"data");
    obrace(&mut t);
    ints_token(&mut t, &[2]);
    floats_token(&mut t, &[1.0, 2.0]);
    str_token(&mut t, b"hi");
    cbrace(&mut t);
    let file = xfile::parse(&binary_file(&t)).unwrap();
    assert_eq!(file.templates.len(), 1);
    assert_eq!(file.templates[0].name, "Seg");
    assert_eq!(file.templates[0].uuid, Some([1u8; 16]));
    let object = &file.roots[0];
    assert_eq!(object.ty, "Seg");
    assert_eq!(object.name.as_deref(), Some("data"));
    let decoded = xfile::decode_object(object, &file).unwrap();
    assert_eq!(decoded.members[0].name, "n");
    assert_eq!(decoded.members[1].value, xfile::MemberValue::Floats(vec![1.0, 2.0]));
    assert_eq!(decoded.members[2].value, xfile::MemberValue::Strings(vec!["hi".to_string()]));
}

/// A named child object parses; a `{ name }` reference fails.
#[test]
fn synthetic_binary_child_and_reference() {
    // template Seg { DWORD n; }
    let mut decl = Vec::new();
    decl.extend_from_slice(&31u16.to_le_bytes());
    name_token(&mut decl, 1, b"Seg");
    obrace(&mut decl);
    decl.extend_from_slice(&41u16.to_le_bytes());
    name_token(&mut decl, 1, b"n");
    decl.extend_from_slice(&20u16.to_le_bytes());
    cbrace(&mut decl);
    // Seg { 1; Seg kid { 2; } }
    let mut t = decl.clone();
    name_token(&mut t, 1, b"Seg");
    obrace(&mut t);
    ints_token(&mut t, &[1]);
    name_token(&mut t, 1, b"Seg");
    name_token(&mut t, 1, b"kid");
    obrace(&mut t);
    ints_token(&mut t, &[2]);
    cbrace(&mut t);
    cbrace(&mut t);
    let file = xfile::parse(&binary_file(&t)).unwrap();
    assert_eq!(file.roots[0].children.len(), 1);
    assert_eq!(file.roots[0].children[0].name.as_deref(), Some("kid"));
    xfile::decode_object(&file.roots[0], &file).unwrap();
    // Seg { 1; { other } }
    let mut r = decl;
    name_token(&mut r, 1, b"Seg");
    obrace(&mut r);
    ints_token(&mut r, &[1]);
    obrace(&mut r);
    name_token(&mut r, 1, b"other");
    cbrace(&mut r);
    cbrace(&mut r);
    assert!(matches!(xfile::parse(&binary_file(&r)), Err(xfile::Error::Unexpected(_))));
}

/// A one-triangle mesh decodes to positions, one face and one triangle.
#[test]
fn synthetic_binary_mesh() {
    let mut t = Vec::new();
    let template = |t: &mut Vec<u8>, name: &[u8], members: &[u8]| {
        t.extend_from_slice(&31u16.to_le_bytes());
        name_token(t, 1, name);
        obrace(t);
        t.extend_from_slice(members);
        cbrace(t);
    };
    // template Vector { FLOAT x; FLOAT y; FLOAT z; }
    let mut m = Vec::new();
    for axis in [b"x", b"y", b"z"] {
        m.extend_from_slice(&42u16.to_le_bytes());
        name_token(&mut m, 1, axis);
        m.extend_from_slice(&20u16.to_le_bytes());
    }
    template(&mut t, b"Vector", &m);
    // template MeshFace { DWORD nFaceVertexIndices; array DWORD faceVertexIndices[nFaceVertexIndices]; }
    let mut m = Vec::new();
    m.extend_from_slice(&41u16.to_le_bytes());
    name_token(&mut m, 1, b"nFaceVertexIndices");
    m.extend_from_slice(&20u16.to_le_bytes());
    m.extend_from_slice(&52u16.to_le_bytes());
    m.extend_from_slice(&41u16.to_le_bytes());
    name_token(&mut m, 1, b"faceVertexIndices");
    m.extend_from_slice(&14u16.to_le_bytes());
    name_token(&mut m, 1, b"nFaceVertexIndices");
    m.extend_from_slice(&15u16.to_le_bytes());
    m.extend_from_slice(&20u16.to_le_bytes());
    template(&mut t, b"MeshFace", &m);
    // template Mesh { DWORD nVertices; array Vector vertices[nVertices];
    //   DWORD nFaces; array MeshFace faces[nFaces]; [...] }
    let mut m = Vec::new();
    m.extend_from_slice(&41u16.to_le_bytes());
    name_token(&mut m, 1, b"nVertices");
    m.extend_from_slice(&20u16.to_le_bytes());
    m.extend_from_slice(&52u16.to_le_bytes());
    name_token(&mut m, 1, b"Vector");
    name_token(&mut m, 1, b"vertices");
    m.extend_from_slice(&14u16.to_le_bytes());
    name_token(&mut m, 1, b"nVertices");
    m.extend_from_slice(&15u16.to_le_bytes());
    m.extend_from_slice(&20u16.to_le_bytes());
    m.extend_from_slice(&41u16.to_le_bytes());
    name_token(&mut m, 1, b"nFaces");
    m.extend_from_slice(&20u16.to_le_bytes());
    m.extend_from_slice(&52u16.to_le_bytes());
    name_token(&mut m, 1, b"MeshFace");
    name_token(&mut m, 1, b"faces");
    m.extend_from_slice(&14u16.to_le_bytes());
    name_token(&mut m, 1, b"nFaces");
    m.extend_from_slice(&15u16.to_le_bytes());
    m.extend_from_slice(&20u16.to_le_bytes());
    m.extend_from_slice(&14u16.to_le_bytes());
    for _ in 0..3 {
        m.extend_from_slice(&18u16.to_le_bytes());
    }
    m.extend_from_slice(&15u16.to_le_bytes());
    template(&mut t, b"Mesh", &m);
    // Mesh { 3; 0,0,0; 1,0,0; 0,1,0; 1; 3,0,1,2; }
    name_token(&mut t, 1, b"Mesh");
    obrace(&mut t);
    ints_token(&mut t, &[3]);
    floats_token(&mut t, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    ints_token(&mut t, &[1, 3, 0, 1, 2]);
    cbrace(&mut t);
    let file = xfile::parse(&binary_file(&t)).unwrap();
    let mesh = xfile::Mesh::decode(&file.roots[0], &file).unwrap();
    assert_eq!(mesh.positions, vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    assert_eq!(mesh.faces, vec![xfile::Face(vec![0, 1, 2])]);
    assert_eq!(mesh.triangles(), vec![[0, 1, 2]]);
    assert_eq!(xfile::triangulate(&[0, 1, 2, 3]), vec![[0, 1, 2], [0, 2, 3]]);
}

/// Malformed inputs fail with a named error, not a panic.
#[test]
fn synthetic_errors() {
    assert!(matches!(xfile::parse(b"short"), Err(xfile::Error::BadHeader(_))));
    assert!(matches!(xfile::parse(b"xof 0303zip 0032"), Err(xfile::Error::BadHeader(_))));
    assert!(matches!(xfile::parse(b"xof 0303txt 0032"), Err(xfile::Error::Unsupported(_))));
    assert!(matches!(xfile::parse(b"xof 0303bin 0064"), Err(xfile::Error::Unsupported(_))));
    assert!(matches!(xfile::parse(b"xof 0303bzip0032"), Err(xfile::Error::Compressed(_))));
    let mut truncated = binary_file(&[]);
    truncated.truncate(20);
    // A header plus half a token truncates.
    truncated.extend_from_slice(&1u16.to_le_bytes());
    assert!(matches!(xfile::parse(&truncated), Err(xfile::Error::Truncated { .. })));
    // Data of an undeclared template.
    let mut t = Vec::new();
    name_token(&mut t, 1, b"Nope");
    obrace(&mut t);
    cbrace(&mut t);
    let file = xfile::parse(&binary_file(&t)).unwrap();
    let object = file.roots[0].clone();
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
