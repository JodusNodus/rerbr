//! The classic collision file (`CACHE/col.col`) of a BTB stage, built from its `CollisionModel=0` objects the way
//! `rbr_rx.dll` does (`ColSplit` 0x10022c10, `.\col.cpp`). Rules and evidence: `notes/btb.md` "Collision".

use crate::track::Track;
use crate::{scene, xfile, xform};
use rbr_formats::rbz;
use std::collections::HashMap;

/// Triangles below which a node is a leaf (`Split` 0x10025e80).
const LEAF_BELOW: usize = 9;
/// A node with fewer triangles is a region (`10025b60`).
const REGION_BELOW: u32 = 600;
/// A node with both half extents (x, y) below this is a region.
const REGION_HALF: f32 = 100.0;
/// The constant attribute words of every triangle record.
const ATTRS: u16 = 0x422f;
/// Size of a node (6 bound floats, flags, pointer).
const NODE: u32 = 0x20;

/// One collision triangle as `GetMeshFaces` (0x10021fa0) leaves it: the vertices in record order `(v0, v2, v1)`
/// with y and z swapped, their texture coordinates, and the `mat.ini` map index (-1: none).
#[derive(Clone, Copy)]
struct Face {
    pos: [[f32; 3]; 3],
    uv: [[f32; 2]; 3],
    map: i32,
    dbg: u32,
}

/// D3DX hands out every non-zero position component one unit in the last place smaller (the bits minus one).
pub fn shift_input(v: f32) -> f32 {
    let bits = v.to_bits();
    if bits & 0x7fff_ffff == 0 {
        v
    } else {
        f32::from_bits(bits.wrapping_sub(1))
    }
}

/// Whether `m` (file order) is exactly the identity, which skips the transform (`10023340`).
pub fn is_identity(m: &[f32; 16]) -> bool {
    (0..16).all(|i| m[i] == if i % 5 == 0 { 1.0 } else { 0.0 })
}

/// `D3DXVec3Transform` with the file matrix: per component `(x*A + y*B) + (z*C + D)` in `f32`, no fused
/// multiply-add.
pub fn transform(m: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    let at = |j: usize| (p[0] * m[j] + p[1] * m[4 + j]) + (p[2] * m[8 + j] + m[12 + j]);
    [at(0), at(1), at(2)]
}

/// The texture coordinate to a nibble byte: `(int)(u * 15)` truncated and cut to a byte (not to a nibble, so a large
/// `u` sets bits of the high nibble), or-ed with the low nibble of `(int)(v * 15)` shifted up.
pub fn nibbles(uv: [f32; 2]) -> u8 {
    (uv[0] * 15.0) as i32 as u8 | ((uv[1] * 15.0) as i32 as u8 & 0xf) << 4
}

/// The base name after the last backslash.
fn basename(path: &str) -> &str {
    path.rsplit('\\').next().unwrap_or(path)
}

/// A kd-tree node: bounds (centre, half extent) and, until split, its triangles.
struct Node {
    centre: [f32; 3],
    half: [f32; 3],
    faces: Vec<u32>,
    leaf: bool,
    /// Triangle count of a split node (`+0x24`).
    count: u32,
    children: Option<Box<[Node; 2]>>,
}

fn centroid(f: &Face) -> [f32; 3] {
    let c = |i: usize| ((f.pos[0][i] + f.pos[1][i]) + f.pos[2][i]) / 3.0;
    [c(0), c(1), c(2)]
}

/// Min and max of points, as the plugin's loops update them.
fn extent(points: impl Iterator<Item = [f32; 3]>, first: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let (mut lo, mut hi) = (first, first);
    for p in points {
        for i in 0..3 {
            if p[i] < lo[i] {
                lo[i] = p[i];
            } else if hi[i] < p[i] {
                hi[i] = p[i];
            }
        }
    }
    (lo, hi)
}

impl Node {
    fn new(faces: Vec<u32>) -> Node {
        Node { centre: [0.0; 3], half: [0.0; 3], faces, leaf: false, count: 0, children: None }
    }

    /// `Split` (0x10025e80): bounds over the vertices, a leaf below nine triangles, else the sorted halves.
    fn split(&mut self, all: &[Face], cents: &[[f32; 3]]) -> Result<(), String> {
        if self.faces.is_empty() {
            return Err("an empty collision node".into());
        }
        let first = cents[self.faces[0] as usize];
        let (lo, hi) = extent(self.faces.iter().flat_map(|&f| all[f as usize].pos), first);
        for i in 0..3 {
            self.centre[i] = (lo[i] + hi[i]) * 0.5;
            self.half[i] = (hi[i] - lo[i]) * 0.5;
        }
        if self.faces.len() < LEAF_BELOW {
            self.leaf = true;
            return Ok(());
        }
        let (a, b) = sort_halves(&self.faces, cents);
        self.count = self.faces.len() as u32;
        self.faces = Vec::new();
        let mut kids = Box::new([Node::new(a), Node::new(b)]);
        kids[0].split(all, cents)?;
        kids[1].split(all, cents)?;
        self.children = Some(kids);
        Ok(())
    }

    /// `10025b60`; only asked of split nodes.
    fn region(&self) -> bool {
        if self.half[0] < REGION_HALF && self.half[1] < REGION_HALF {
            return true;
        }
        self.count < REGION_BELOW || self.children.as_ref().is_some_and(|k| k[0].leaf || k[1].leaf)
    }
}

/// `SortFacesRightLeft` (0x10025650): the longest centroid axis (ties lowest), centroids below the centre left,
/// the rest right; a side left empty retries axes 0, 1, 2 with the same centre.
fn sort_halves(faces: &[u32], cents: &[[f32; 3]]) -> (Vec<u32>, Vec<u32>) {
    let c0 = cents[faces[0] as usize];
    let (lo, hi) = extent(faces.iter().map(|&f| cents[f as usize]), c0);
    let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    let centre = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
    let mut axis = 0;
    for i in 1..3 {
        if size[axis] < size[i] {
            axis = i;
        }
    }
    let run = |axis: usize| {
        let (mut left, mut right) = (Vec::new(), Vec::new());
        for &f in faces {
            if centre[axis] > cents[f as usize][axis] {
                right.push(f);
            } else {
                left.push(f);
            }
        }
        (left, right)
    };
    let mut halves = run(axis);
    if halves.0.is_empty() || halves.1.is_empty() {
        for axis in 0..3 {
            halves = run(axis);
            if halves.0.len() != faces.len() && halves.1.len() != faces.len() {
                break;
            }
        }
    }
    halves
}

fn put(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_bounds(out: &mut Vec<u8>, n: &Node) {
    for v in n.centre.iter().chain(&n.half) {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

/// The leaf faces of a subtree in preorder (`10026e80`).
fn collect(n: &Node, out: &mut Vec<u32>) {
    match &n.children {
        Some(k) => {
            collect(&k[0], out);
            collect(&k[1], out);
        }
        None => out.extend_from_slice(&n.faces),
    }
}

/// Bytes a node's content takes in a cell tree: an internal node's child pair and subtrees, a leaf's triangles.
fn tree_size(n: &Node) -> u32 {
    match &n.children {
        Some(k) => 2 * NODE + tree_size(&k[0]) + tree_size(&k[1]),
        None => n.faces.len() as u32 * 18 + (n.faces.len() as u32 & 1) * 2,
    }
}

/// A cell's triangle indices and faces, in the cell's face order.
struct Table<'a> {
    all: &'a [Face],
    order: Vec<u32>,
    index: Vec<u16>,
    next: usize,
}

/// `DumpNode` (0x10026470): `n`'s child pair, then each child's subtree; a leaf writes its triangles. `at` is where
/// `n`'s content starts, relative to the tree root.
fn write_tree(n: &Node, at: u32, t: &mut Table, out: &mut Vec<u8>) {
    let Some(k) = &n.children else {
        for _ in 0..n.faces.len() {
            let f = &t.all[t.order[t.next] as usize];
            let i = &t.index[t.next * 3..t.next * 3 + 3];
            t.next += 1;
            // Record slots (1, 0, 2): the swap of the first two indices.
            let slots = [1, 0, 2];
            for s in slots {
                out.extend_from_slice(&i[s].to_le_bytes());
            }
            out.extend_from_slice(&ATTRS.to_le_bytes());
            out.extend_from_slice(&ATTRS.to_le_bytes());
            let map = f.map.max(0) as u8;
            out.extend_from_slice(&[map, map]);
            // The nibbles come from the table entry the (16-bit) index names.
            let nib: Vec<u8> = slots
                .iter()
                .map(|&s| {
                    let entry = i[s] as usize;
                    nibbles(t.all[t.order[entry / 3] as usize].uv[entry % 3])
                })
                .collect();
            out.extend_from_slice(&nib);
            out.extend_from_slice(&nib);
        }
        if n.faces.len() & 1 == 1 {
            out.extend_from_slice(&[0, 0]);
        }
        return;
    };
    let first = at + 2 * NODE;
    let second = first + tree_size(&k[0]);
    for (kid, ptr) in k.iter().zip([first, second]) {
        put_bounds(out, kid);
        put(out, kid.faces.len() as u32);
        put(out, ptr);
    }
    write_tree(&k[0], first, t, out);
    write_tree(&k[1], second, t, out);
}

/// One cell (`DumpBranch` 0x10026860): the vertex table of its faces and its subtree, the root first.
fn cell(root: &Node, path: u32, all: &[Face]) -> Result<Vec<u8>, String> {
    let mut order = Vec::new();
    collect(root, &mut order);
    let count = order.len() as u32 * 3;
    let mut out = Vec::new();
    for w in [0x104, path, count, 0x14, count * 12 + 0x14] {
        put(&mut out, w);
    }
    // The table holds (position, uv) per face corner in record order; a corner indexes its first equal entry.
    let key = |p: [f32; 3], uv: [f32; 2]| {
        let b = |v: f32| if v == 0.0 { 0 } else { v.to_bits() };
        [b(p[0]), b(p[1]), b(p[2]), b(uv[0]), b(uv[1])]
    };
    let mut first: HashMap<[u32; 5], u32> = HashMap::with_capacity(count as usize);
    let mut index = Vec::with_capacity(count as usize);
    for &f in &order {
        let f = &all[f as usize];
        for k in 0..3 {
            for v in f.pos[k] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            let slot = index.len() as u32;
            // The plugin stores the table index in 16 bits: a cell over 65536 vertices wraps.
            index.push(*first.entry(key(f.pos[k], f.uv[k])).or_insert(slot) as u16);
        }
    }
    put_bounds(&mut out, root);
    put(&mut out, 0x20_0000);
    put(&mut out, NODE);
    write_tree(root, NODE, &mut Table { all, order, index, next: 0 }, &mut out);
    Ok(out)
}

/// The regions, in file order: the children of each region node in preorder, with their graft path.
fn regions<'a>(n: &'a Node, depth: u32, path: u32, out: &mut Vec<(&'a Node, u32)>) {
    let Some(k) = &n.children else { return };
    if n.region() {
        for (i, kid) in k.iter().enumerate() {
            out.push((kid, depth + 1 << 24 | (path | (i as u32) << depth)));
        }
    } else {
        for (i, kid) in k.iter().enumerate() {
            regions(kid, depth + 1, path | (i as u32) << depth, out);
        }
    }
}

/// `maintree.col` (`100241c0`): the tree down to the region nodes, whose children are graft placeholders.
fn main_tree(n: &Node, at: u32, out: &mut Vec<u8>) {
    let Some(k) = &n.children else { return };
    let region = n.region();
    let first = at + 2 * NODE;
    let second = first + if region { 0 } else { main_size(&k[0]) };
    for (kid, ptr) in k.iter().zip([first, second]) {
        put_bounds(out, kid);
        if region {
            put(out, 0x20_0000);
            put(out, 0);
        } else {
            put(out, 0);
            put(out, ptr);
        }
    }
    if !region {
        main_tree(&k[0], first, out);
        main_tree(&k[1], second, out);
    }
}

fn main_size(n: &Node) -> u32 {
    match &n.children {
        Some(k) if !n.region() => 2 * NODE + main_size(&k[0]) + main_size(&k[1]),
        Some(_) => 2 * NODE,
        None => 0,
    }
}

/// The faces of one mesh before placement: positions (input shift applied), triangles, UVs and map index.
struct Mesh {
    positions: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
    uvs: Vec<[f32; 2]>,
    map: i32,
}

/// The mesh's material texture, through `materials.ini`, to the first `mat.ini` map with that base name.
struct Maps {
    /// Lower-case material section name to its base-named diffuse texture.
    diffuse: HashMap<String, String>,
    textures: Vec<Option<String>>,
}

impl Maps {
    fn index(&self, material: Option<&str>) -> i32 {
        let Some(texture) = material.and_then(|m| self.diffuse.get(&m.to_lowercase())) else { return -1 };
        self.textures
            .iter()
            .position(|m| m.as_deref().is_some_and(|m| basename(m).eq_ignore_ascii_case(texture)))
            .map_or(-1, |i| i as i32)
    }
}

fn load_mesh(name: &str, bytes: &[u8], maps: &Maps) -> Result<Mesh, String> {
    let fail = |e: xfile::Error| format!("{name}.x: {e}");
    let file = xfile::parse(bytes).map_err(fail)?;
    let root = *file.objects_of_type("Mesh").first().ok_or_else(|| format!("{name}.x: no Mesh"))?;
    let mesh = xfile::Mesh::decode(root, &file).map_err(fail)?;
    let texture = mesh.materials.as_ref().and_then(|m| m.materials.first()).and_then(|m| m.texture.as_deref());
    let map = maps.index(texture);
    Ok(Mesh {
        positions: mesh.positions.iter().map(|p| p.map(shift_input)).collect(),
        triangles: mesh
            .triangles()
            .into_iter()
            .filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
            .collect(),
        uvs: pick_uvs(&mesh),
        map,
    })
}

fn pick_uvs(mesh: &xfile::Mesh) -> Vec<[f32; 2]> {
    let want: i32 = std::env::var("COLUV").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let base = mesh.uv_sets.first().cloned().unwrap_or_default();
    if want == 0 {
        return base;
    }
    let n = mesh.positions.len();
    for d in &mesh.decl {
        let Some(stride) = d.stride_dwords() else { continue };
        let mut off = 0;
        for e in &d.elements {
            if e.usage == 5 && e.usage_index as i32 == want {
                return (0..n)
                    .map(|v| [f32::from_bits(d.data[v * stride + off]), f32::from_bits(d.data[v * stride + off + 1])])
                    .collect();
            }
            off += e.width_dwords().unwrap_or(0);
        }
    }
    for f in &mesh.fvf {
        if want == 1 {
            let stride = f.data.len() / n.max(1);
            return (0..n).map(|v| [f32::from_bits(f.data[v * stride]), f32::from_bits(f.data[v * stride + 1])]).collect();
        }
    }
    base
}

/// The faces of every `CollisionModel=0` object, clone by clone, in the order the plugin gathers them.
fn gather(track: &Track) -> Result<Vec<Face>, String> {
    let objects = track.objects()?;
    // A material whose diffuse texture `Textures.zip` lacks loads without one: its meshes get no map.
    let archive = rbr_formats::stage_files::in_dir(&track.folder, "Textures.zip")
        .ok_or_else(|| format!("{}: no Textures.zip", track.folder.display()))?;
    let zip = rbz::open(&archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    let held: std::collections::HashSet<String> =
        zip.file_names().flatten().map(|n| n.rsplit(['/', '\\']).next().unwrap_or(&n).to_lowercase()).collect();
    let mut diffuse = HashMap::new();
    for m in track.materials()?.sections {
        let texture = m.texture("diffuseTexture").map(basename);
        if let Some(t) = texture.filter(|t| held.contains(&t.to_lowercase()) && !t.to_lowercase().ends_with(".png")) {
            diffuse.entry(m.name.to_lowercase()).or_insert_with(|| t.to_owned());
        }
    }
    let maps = Maps { diffuse, textures: track.surface_maps()?.textures };
    let path = rbr_formats::stage_files::in_dir(&track.folder, "Objects.zip")
        .ok_or_else(|| format!("{}: no Objects.zip", track.folder.display()))?;
    let mut zip = rbz::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut meshes: HashMap<String, Mesh> = HashMap::new();
    let mut faces = Vec::new();
    for object in objects.iter().filter(|o| o.collision_model == Some(0)) {
        let key = object.file.to_lowercase();
        if !meshes.contains_key(&key) {
            let bytes = scene::read_entry(&mut zip, &format!("Objects/{}.x", object.file))?;
            meshes.insert(key.clone(), load_mesh(&object.file, &bytes, &maps)?);
        }
        let mesh = &meshes[&key];
        let bytes = scene::read_entry(&mut zip, &format!("Objects/{}.xform", object.xform))?;
        for m in xform::parse(&bytes).map_err(|e| format!("{}.xform: {e}", object.xform))? {
            let identity = is_identity(&m);
            let place = |i: u32| {
                let p = mesh.positions[i as usize];
                let p = if identity { p } else { transform(&m, p) };
                [p[0], p[2], p[1]]
            };
            let uv = |i: u32| mesh.uvs.get(i as usize).copied().unwrap_or([0.0; 2]);
            for &[a, b, c] in &mesh.triangles {
                faces.push(Face { pos: [place(a), place(c), place(b)], uv: [uv(a), uv(c), uv(b)], map: mesh.map, dbg: object.index as u32 });
            }
        }
    }
    Ok(faces)
}

/// The stage's `col.col`.
pub fn build(track: &Track) -> Result<Vec<u8>, String> {
    let faces = gather(track)?;
    if std::env::var_os("COLDBG").is_some() {
        let mut c = std::collections::BTreeMap::new();
        for f in &faces {
            *c.entry((f.map, f.dbg)).or_insert(0) += 1;
        }
        for (k, v) in c { eprintln!("MAPCOUNT {:?} {}", k, v); }
    }
    let cents: Vec<[f32; 3]> = faces.iter().map(centroid).collect();
    let mut root = Node::new((0..faces.len() as u32).collect());
    root.split(&faces, &cents)?;
    if root.children.is_none() {
        return Err("fewer than nine collision triangles".into());
    }
    let mut list = Vec::new();
    regions(&root, 0, 0, &mut list);
    let cells = list
        .iter()
        .filter(|(n, _)| !n.leaf)
        .map(|&(n, path)| cell(n, path, &faces))
        .collect::<Result<Vec<_>, _>>()?;

    let mut tree = Vec::new();
    put_bounds(&mut tree, &root);
    put(&mut tree, 0);
    put(&mut tree, NODE);
    main_tree(&root, NODE, &mut tree);

    let table = 0x40 + tree.len() as u32;
    let mut out = Vec::with_capacity(table as usize + cells.iter().map(Vec::len).sum::<usize>() + cells.len() * 4);
    for w in [0x5237_434f, 0x10, cells.len() as u32, table] {
        put(&mut out, w);
    }
    // The root block between the header and the tree, as the plugin's template holds it.
    for w in [0x304, 0, 0, 0x30, 0x30, 0, 0, 0x30, 0, 0x30, 0, 0] {
        put(&mut out, w);
    }
    out.extend_from_slice(&tree);
    let mut at = table + cells.len() as u32 * 4;
    for c in &cells {
        put(&mut out, at);
        at += c.len() as u32;
    }
    for c in &cells {
        out.extend_from_slice(c);
    }
    Ok(out)
}
