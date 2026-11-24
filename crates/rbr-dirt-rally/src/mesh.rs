//! Drawn meshes of a PSSG: terrain tiles in world space and prop types in their own space.
//!
//! A `RENDERSTREAMINSTANCE` (`indices="#source" shader="#material"`) names a `RENDERDATASOURCE`: its
//! `RENDERINDEXSOURCE` (big-endian u16 or u32 triangle list) and `RENDERSTREAM`s (`dataBlock="#block"
//! subStream=k`), the k-th `DATABLOCKSTREAM` (render type, data type, offset, stride) of an interleaved `DATABLOCK`.
//! Everything is in DiRT Rally's frame (right handed, y up, metres, counter-clockwise front faces); `frame` maps it to
//! the stage's.
use crate::material::{self, Material};
use crate::pssg::{Node, Pssg};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    /// The id of the node the mesh hangs under (`SHADOWCASTING_9_9`, `lod!45`).
    pub node: String,
    /// The `SHADERINSTANCE` id, a key of `material::read`.
    pub material: String,
    pub positions: Vec<[f32; 3]>,
    /// Empty when the source has none (position-only copies).
    pub normals: Vec<[f32; 3]>,
    /// Empty without an `ST` stream. What the drawn texture reads (`material::diffuse`): its `ST` stream's `xy`
    /// (`material::uv_stream`) with the layer's `Map<N>UVScaleAndOffset` applied.
    pub uv: Vec<[f32; 2]>,
    /// The first `ST` stream's `zw` (terrain: the finer layer coordinates; meaning of the pair not fully verified).
    pub uv_alt: Option<Vec<[f32; 2]>>,
    /// The second `ST` stream's `xy`: the lightmap (terrain, buildings) or the vista's large map.
    pub uv2: Option<Vec<[f32; 2]>>,
    /// `Color` as RGBA. Stored ARGB; terrain keeps its layer 2 and 3 blend weights in R and G with A 255.
    pub colour: Option<Vec<[u8; 4]>>,
    /// Counter-clockwise, right handed.
    pub triangles: Vec<[u32; 3]>,
}

pub(crate) type Mat = [[f32; 4]; 4];

pub(crate) const IDENTITY: Mat = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];

pub(crate) fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut m = [[0.; 4]; 4];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

pub(crate) fn point(m: &Mat, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|j| p[0] * m[0][j] + p[1] * m[1][j] + p[2] * m[2][j] + m[3][j])
}

pub(crate) fn direction(m: &Mat, n: [f32; 3]) -> [f32; 3] {
    let d: [f32; 3] = std::array::from_fn(|j| n[0] * m[0][j] + n[1] * m[1][j] + n[2] * m[2][j]);
    let len = d.iter().map(|v| v * v).sum::<f32>().sqrt();
    if len > 0. {
        d.map(|v| v / len)
    } else {
        d
    }
}

pub(crate) fn determinant(m: &Mat) -> f32 {
    let r = |i: usize| m[i];
    let (a, b, c) = (r(0), r(1), r(2));
    a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])
}

fn half(bits: u16) -> f32 {
    let (sign, exp, frac) = (f32::from(bits >> 15) * -2. + 1., i32::from((bits >> 10) & 31), f32::from(bits & 1023));
    match exp {
        0 => sign * frac * 2f32.powi(-24),
        31 if frac == 0. => sign * f32::INFINITY,
        31 => f32::NAN,
        _ => sign * (1. + frac / 1024.) * 2f32.powi(exp - 15),
    }
}

/// One decoded stream: `width` components of each element.
struct Stream {
    render_type: String,
    width: usize,
    values: Vec<[f32; 4]>,
    bytes: Vec<[u8; 4]>,
}

fn decode_stream(block: Node, k: usize) -> Result<Stream, String> {
    let count = block.u32("elementCount").ok_or("DATABLOCK without elementCount")? as usize;
    let def = block.children().filter(|c| c.name() == "DATABLOCKSTREAM").nth(k).ok_or("missing DATABLOCKSTREAM")?;
    let (render_type, data_type) = (def.str("renderType").unwrap_or(""), def.str("dataType").unwrap_or(""));
    let (offset, stride) = (def.u32("offset").unwrap_or(0) as usize, def.u32("stride").unwrap_or(0) as usize);
    let data = block.child_payload("DATABLOCKDATA");
    let size = match data_type {
        "float2" => 8,
        "float3" => 12,
        "float4" => 16,
        "float" | "half2" | "uint_color_argb" | "uchar4" => 4,
        "half4" => 8,
        other => return Err(format!("{render_type}: unknown data type {other}")),
    };
    if offset + size > stride || stride * count > data.len() {
        return Err(format!("{render_type}: stream outside its block"));
    }
    let mut stream = Stream {
        render_type: render_type.to_owned(),
        width: size / if data_type.starts_with("half") { 2 } else { 4 },
        values: Vec::new(),
        bytes: Vec::new(),
    };
    for i in 0..count {
        let e = &data[i * stride + offset..i * stride + offset + size];
        match data_type {
            "uint_color_argb" | "uchar4" => stream.bytes.push([e[0], e[1], e[2], e[3]]),
            _ => {
                let mut v = [0.; 4];
                for (j, c) in v.iter_mut().take(stream.width).enumerate() {
                    *c = if data_type.starts_with("half") {
                        half(u16::from_be_bytes([e[2 * j], e[2 * j + 1]]))
                    } else {
                        f32::from_bits(u32::from_be_bytes([e[4 * j], e[4 * j + 1], e[4 * j + 2], e[4 * j + 3]]))
                    };
                }
                stream.values.push(v);
            }
        }
    }
    Ok(stream)
}

/// The triangle indices and streams of one `RENDERDATASOURCE`. The indices are big-endian `ushort` or `uint` (a car's
/// largest sources: the i20's body, 24484 vertices).
fn read_source(source: Node) -> Result<(Vec<u32>, Vec<Stream>), String> {
    let index = source.child("RENDERINDEXSOURCE").ok_or("RENDERDATASOURCE without indices")?;
    if source.str("primitive").or(index.str("primitive")) != Some("triangles") {
        return Err("only triangle lists are read".into());
    }
    let data = index.child_payload("INDEXSOURCEDATA");
    let indices = match index.str("format") {
        Some("ushort") => data.chunks_exact(2).map(|c| u32::from(u16::from_be_bytes([c[0], c[1]]))).collect(),
        Some("uint") => data.chunks_exact(4).map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]])).collect(),
        other => return Err(format!("indices of format {other:?}")),
    };
    let mut streams = Vec::new();
    for stream in source.children().filter(|c| c.name() == "RENDERSTREAM") {
        let block = stream.reference("dataBlock").ok_or("RENDERSTREAM without its DATABLOCK")?;
        streams.push(decode_stream(block, stream.u32("subStream").unwrap_or(0) as usize)?);
    }
    Ok((indices, streams))
}

/// A `RENDERDATASOURCE` read whole, for a reader that slices it itself (the cars': one source serves many parts, each
/// instance naming its range of the vertices and indices).
pub(crate) struct Geometry {
    pub indices: Vec<u32>,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// The first `ST` stream's `xy`; empty without one.
    pub uv: Vec<[f32; 2]>,
}

/// The indices and the `Vertex`, `Normal` and first `ST` streams of `source`.
pub(crate) fn geometry(source: Node) -> Result<Geometry, String> {
    let (indices, streams) = read_source(source)?;
    let of = |t: &str| streams.iter().find(|s| s.render_type == t);
    let vec3 = |s: Option<&Stream>| s.map_or(Vec::new(), |s| s.values.iter().map(|v| [v[0], v[1], v[2]]).collect());
    Ok(Geometry {
        indices,
        positions: vec3(of("Vertex")),
        normals: vec3(of("Normal")),
        uv: of("ST").map_or(Vec::new(), |s| s.values.iter().map(xy).collect()),
    })
}

fn xy(v: &[f32; 4]) -> [f32; 2] {
    [v[0], v[1]]
}

/// The mesh of one `RENDERSTREAMINSTANCE` under `parent`, moved by `m`; `None` without positions or triangles.
fn build(
    instance: Node,
    parent: &str,
    m: &Mat,
    materials: &BTreeMap<String, Material>,
) -> Result<Option<Mesh>, String> {
    let Some(source) = instance.reference("indices") else { return Ok(None) };
    let (indices, streams) = read_source(source)?;
    let of = |t: &str| streams.iter().filter(|s| s.render_type == t).collect::<Vec<_>>();
    let Some(vertex) = of("Vertex").first().map(|s| &s.values) else { return Ok(None) };
    if indices.len() < 3 || streams.iter().any(|s| s.values.len().max(s.bytes.len()) != vertex.len()) {
        return if indices.len() < 3 {
            Ok(None)
        } else {
            Err(format!("{}: streams of different lengths", source.id().unwrap_or("")))
        };
    }
    if indices.iter().any(|&i| i as usize >= vertex.len()) {
        return Err(format!("{}: index out of range", source.id().unwrap_or("")));
    }
    let material = instance.str("shader").unwrap_or("").trim_start_matches('#').to_owned();
    let [su, sv, ou, ov] = materials.get(&material).map_or([1., 1., 0., 0.], material::uv_transform);
    let st = of("ST");
    let drawn = materials.get(&material).map_or(0, material::uv_stream);
    let flip = determinant(m) < 0.;
    Ok(Some(Mesh {
        node: parent.to_owned(),
        positions: vertex.iter().map(|v| point(m, [v[0], v[1], v[2]])).collect(),
        normals: of("Normal")
            .first()
            .map_or(Vec::new(), |s| s.values.iter().map(|v| direction(m, [v[0], v[1], v[2]])).collect()),
        uv: st.get(drawn).map_or(Vec::new(), |s| s.values.iter().map(|v| [v[0] * su + ou, v[1] * sv + ov]).collect()),
        uv_alt: st.first().filter(|s| s.width == 4).map(|s| s.values.iter().map(|v| [v[2], v[3]]).collect()),
        uv2: st.get(1).map(|s| s.values.iter().map(xy).collect()),
        colour: of("Color").first().map(|s| s.bytes.iter().map(|c| [c[1], c[2], c[3], c[0]]).collect()),
        triangles: indices
            .chunks_exact(3)
            .map(|t| if flip { [t[0], t[2], t[1]] } else { [t[0], t[1], t[2]] })
            .collect(),
        material,
    }))
}

/// The scene kinds visited: `LODRENDERINSTANCES` (the coarser LODs) and skinned or joint nodes are not.
const SCENE: [&str; 4] = ["ROOTNODE", "NODE", "RENDERNODE", "LODVISIBLERENDERNODE"];

/// Calls `f` with each `RENDERSTREAMINSTANCE` under `node`, its parent node and the parent's accumulated transform.
fn visit<'a>(
    node: Node<'a>,
    m: &Mat,
    f: &mut impl FnMut(Node<'a>, Node<'a>, &Mat) -> Result<(), String>,
) -> Result<(), String> {
    for child in node.children() {
        match child.name() {
            "RENDERSTREAMINSTANCE" => f(child, node, m)?,
            n if SCENE.contains(&n) => visit(child, &mul(&child.transform(), m), f)?,
            _ => {}
        }
    }
    Ok(())
}

/// Each scene node's accumulated transform under the file's roots, by id (render nodes and plain ones, such as the
/// interior's `int_needle_tach_max`).
pub fn node_worlds(pssg: &Pssg) -> BTreeMap<String, Mat> {
    fn walk(node: Node<'_>, m: &Mat, out: &mut BTreeMap<String, Mat>) {
        for child in node.children().filter(|c| SCENE.contains(&c.name())) {
            let world = mul(&child.transform(), m);
            if let Some(id) = child.id() {
                out.entry(id.to_owned()).or_insert(world);
            }
            walk(child, &world, out);
        }
    }
    let mut out = BTreeMap::new();
    for root in pssg.find("ROOTNODE") {
        walk(root, &root.transform(), &mut out);
    }
    out
}

/// The tile tier of a node id: `SHADOWCASTING_9_9` is `SHADOWCASTING`.
fn tier(id: &str) -> &str {
    id.split('_').next().unwrap_or(id)
}

/// Tiers drawn from a terrain file. `LOW` is a decimated copy of the same ground (about half its vertices are the
/// detailed tiers' own, it covers the area of `SHADOWCASTING` and `HIGH` entirely), and the `*BATCH` tiers are
/// position-only copies; both would draw the ground twice. `HIGH` and `SHADOWCASTING` hold different surfaces (no shared
/// vertex in Greece or Finland), `DECAL` overlays and `NONLOD` is the vista.
const TERRAIN_TIERS: [&str; 4] = ["HIGH", "SHADOWCASTING", "DECAL", "NONLOD"];

/// The drawn terrain of a `routesplit` or `tracksplit` in world space (their transforms are identity).
pub fn terrain(pssg: &Pssg) -> Result<Vec<Mesh>, String> {
    let materials = material::read(pssg);
    let mut meshes = Vec::new();
    for root in pssg.find("ROOTNODE") {
        visit(root, &root.transform(), &mut |instance, parent, m| {
            let id = parent.id().unwrap_or("");
            if TERRAIN_TIERS.contains(&tier(id)) {
                meshes.extend(build(instance, id, m, &materials)?);
            }
            Ok(())
        })?;
    }
    if meshes.is_empty() {
        return Err("no terrain tiles (HIGH, SHADOWCASTING, DECAL, NONLOD) in this file".into());
    }
    Ok(meshes)
}

/// A car's interior scene (`int_<car>.pssg`): the meshes of every node, each at its place in the scene's rest pose (the
/// car's frame), named by the node.
pub fn scene(pssg: &Pssg) -> Result<Vec<Mesh>, String> {
    let materials = material::read(pssg);
    let mut meshes = Vec::new();
    for root in pssg.find("ROOTNODE") {
        visit(root, &root.transform(), &mut |instance, parent, m| {
            meshes.extend(build(instance, parent.id().unwrap_or(""), m, &materials)?);
            Ok(())
        })?;
    }
    Ok(meshes)
}

/// The tuft models of a `ground_cover` file: each `RENDERNODE` named `t<type>_<model>` and its meshes.
pub fn tuft_meshes(pssg: &Pssg) -> Result<Vec<(String, Vec<Mesh>)>, String> {
    let materials = material::read(pssg);
    let mut out = Vec::new();
    for node in pssg.find("RENDERNODE") {
        let id = node.id().unwrap_or("").to_owned();
        let mut meshes = Vec::new();
        visit(node, &node.transform(), &mut |instance, parent, m| {
            meshes.extend(build(instance, parent.id().unwrap_or(""), m, &materials)?);
            Ok(())
        })?;
        out.push((id, meshes));
    }
    Ok(out)
}

/// The placeable object types of an `objects`, `route_objects` or `trees` file: each `ROOTNODE`'s visible level-0
/// meshes in its own space, and the ids that name it (its own and every node under it, `default!2`,
/// `gre_rock_rand_small_default_001`), which is what `route_objecttypes` references.
pub struct Types {
    pub meshes: Vec<Vec<Mesh>>,
    /// Each type's far-view billboard, where the root holds one.
    pub sprites: Vec<Option<Sprite>>,
    pub index: BTreeMap<String, usize>,
}

/// A tree's far level: a camera-facing quad (`foliage_billboard.fx`, the `_x2` node) cut from an atlas of `views`
/// pictures of the tree turned about its axis in equal steps. The vertices all sit at `centre`; each vertex's
/// `uv2` is its offset (right, up) from it, in metres of the type's own space.
#[derive(Debug, Clone, PartialEq)]
pub struct Sprite {
    /// The `SHADERINSTANCE` id.
    pub material: String,
    pub centre: [f32; 3],
    /// Half the width and the height.
    pub half: [f32; 2],
    /// The first view's `[u min, v min, u max, v max]` in the atlas.
    pub uv: [f32; 4],
    pub views: u32,
}

fn sprite_of(meshes: &[Mesh], materials: &BTreeMap<String, Material>) -> Option<Sprite> {
    let m = meshes.iter().find(|m| {
        materials.get(&m.material).is_some_and(|x| x.shader == "foliage_billboard.fx")
            && m.positions.len() == 4
            && m.uv.len() == 4
            && m.uv2.is_some()
    })?;
    let offsets = m.uv2.as_ref()?;
    let (mut uv, mut half) = ([f32::MAX, f32::MAX, f32::MIN, f32::MIN], [0f32; 2]);
    for (t, o) in m.uv.iter().zip(offsets) {
        uv = [uv[0].min(t[0]), uv[1].min(t[1]), uv[2].max(t[0]), uv[3].max(t[1])];
        half = [half[0].max(o[0].abs()), half[1].max(o[1].abs())];
    }
    let views = (1. / (uv[2] - uv[0])).round();
    (views >= 1. && half[0] > 0. && half[1] > 0.).then(|| Sprite {
        material: m.material.clone(),
        centre: m.positions[0],
        half,
        uv,
        views: views as u32,
    })
}

/// What [`types`] draws of a root: the level 0 under its `lod` nodes. The `default` and `damage` nodes beside them
/// repeat that geometry for the physics and the broken state, the `LODRENDERINSTANCES` are coarser levels, and the
/// camera-facing billboards (the trees' far levels), shadow quads and the light shafts of `volumetrics.fx` (additive cones
/// with no texture, dark as solids) are sprites no near view wants.
pub fn types(pssg: &Pssg) -> Result<Types, String> {
    let materials = material::read(pssg);
    let mut types = Types { meshes: Vec::new(), sprites: Vec::new(), index: BTreeMap::new() };
    for root in pssg.find("ROOTNODE") {
        let (mut meshes, mut seen, mut sprites) = (Vec::new(), BTreeSet::new(), Vec::new());
        let m = root.transform();
        for lod in root.children().filter(|c| SCENE.contains(&c.name()) && c.str("nickname") == Some("lod")) {
            visit(lod, &mul(&lod.transform(), &m), &mut |instance, parent, m| {
                let sprite = instance
                    .str("shader")
                    .and_then(|s| materials.get(s.trim_start_matches('#')))
                    .is_some_and(|x| x.shader.contains("billboard") || x.shader.contains("volumetric"));
                if seen.insert(instance.str("indices").unwrap_or("").to_owned()) {
                    let built = build(instance, parent.id().unwrap_or(""), m, &materials)?;
                    if sprite {
                        sprites.extend(built)
                    } else {
                        meshes.extend(built)
                    }
                }
                Ok(())
            })?;
        }
        let at = types.meshes.len();
        types.sprites.push(sprite_of(&sprites, &materials));
        types.meshes.push(meshes);
        types.index.extend(root.id().map(|id| (id.to_owned(), at)));
        index_nodes(root, at, &mut types.index);
    }
    Ok(types)
}

fn index_nodes(node: Node, at: usize, index: &mut BTreeMap<String, usize>) {
    for child in node.children().filter(|c| c.name() == "NODE") {
        index.extend(child.id().map(|id| (id.to_owned(), at)));
        index_nodes(child, at, index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pssg::tests::sample_areas;

    #[test]
    fn halves_decode() {
        assert_eq!([half(0x3c00), half(0xc000), half(0x3800), half(0)], [1., -2., 0.5, 0.]);
        assert_eq!(half(0x0001), 2f32.powi(-24));
        assert!(half(0x7e00).is_nan() && half(0x7c00).is_infinite());
    }

    #[test]
    fn transforms_move_points_and_flip_mirrors() {
        let mut m = IDENTITY;
        m[3] = [1., 2., 3., 1.];
        m[0][0] = 2.;
        assert_eq!(point(&m, [1., 1., 1.]), [3., 3., 4.]);
        assert_eq!(point(&mul(&m, &m), [0., 0., 0.]), [3., 4., 6.]);
        assert!(determinant(&m) > 0.);
        m[1][1] = -1.;
        assert!(determinant(&m) < 0.);
        assert_eq!(direction(&m, [0., 3., 0.]), [0., -1., 0.]);
        assert_eq!(tier("SHADOWCASTING_9_9"), "SHADOWCASTING");
    }

    fn check(meshes: &[Mesh]) -> (usize, [f32; 6]) {
        let mut tris = 0;
        let mut b = [f32::MAX, f32::MAX, f32::MAX, f32::MIN, f32::MIN, f32::MIN];
        for m in meshes {
            let n = m.positions.len();
            assert!(m.normals.is_empty() || m.normals.len() == n);
            assert!(m.uv.is_empty() || m.uv.len() == n, "{}", m.node);
            assert!(m.uv2.as_ref().is_none_or(|u| u.len() == n) && m.colour.as_ref().is_none_or(|c| c.len() == n));
            assert!(m.triangles.iter().flatten().all(|&i| (i as usize) < n));
            assert!(
                m.positions
                    .iter()
                    .flatten()
                    .chain(m.uv.iter().flatten())
                    .chain(m.normals.iter().flatten())
                    .all(|v| v.is_finite()),
                "{}",
                m.node
            );
            for p in &m.positions {
                for a in 0..3 {
                    b[a] = b[a].min(p[a]);
                    b[a + 3] = b[a + 3].max(p[a]);
                }
            }
            tris += m.triangles.len();
        }
        (tris, b)
    }

    #[test]
    fn terrain_of_the_sample_areas_decodes() {
        for area in sample_areas() {
            let files: &[&str] = if area.ends_with("ghymno_rally_01") {
                &["route_0/routesplit.pssg", "tracksplit.pssg"]
            } else {
                &["tracksplit.pssg"]
            };
            for file in files {
                let started = std::time::Instant::now();
                let meshes = terrain(&Pssg::read(&area.join(file)).unwrap()).unwrap();
                let mut tiers: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
                for m in &meshes {
                    let e = tiers.entry(tier(&m.node)).or_default();
                    e.0 += 1;
                    e.1 += m.triangles.len();
                }
                let (tris, bounds) = check(&meshes);
                eprintln!(
                    "{} {file}: {} meshes {tris} triangles {tiers:?} bounds {bounds:?} in {} ms",
                    area.display(),
                    meshes.len(),
                    started.elapsed().as_millis()
                );
                assert!(tris > 10_000 && !tiers.contains_key("LOW"));
                // Counter-clockwise faces look the way their vertex normals point.
                let (mut facing, mut total) = (0usize, 0usize);
                for m in meshes.iter().filter(|m| !m.normals.is_empty()).step_by(7) {
                    for t in &m.triangles {
                        let [a, b, c] = t.map(|i| m.positions[i as usize]);
                        let (u, v) = (
                            std::array::from_fn::<f32, 3, _>(|k| b[k] - a[k]),
                            std::array::from_fn::<f32, 3, _>(|k| c[k] - a[k]),
                        );
                        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                        let vn = m.normals[t[0] as usize];
                        let dot: f32 = (0..3).map(|k| n[k] * vn[k]).sum();
                        total += 1;
                        facing += usize::from(dot >= 0.);
                    }
                }
                assert!(facing as f64 > 0.99 * total as f64, "{facing}/{total} faces follow their normals");
            }
        }
    }

    #[test]
    fn objects_resolve_every_reference_of_route_objecttypes() {
        for area in sample_areas() {
            let route = area.join("route_0");
            let xml = std::fs::read_to_string(route.join("route_objecttypes.pssg")).unwrap_or_default();
            for (file, dir) in [("objects.pssg", &area), ("route_objects.pssg", &route), ("trees.pssg", &area)] {
                let started = std::time::Instant::now();
                let types = types(&Pssg::read(&dir.join(file)).unwrap()).unwrap();
                let all: Vec<Mesh> = types.meshes.iter().flatten().cloned().collect();
                let (tris, _) = check(&all);
                let wanted: BTreeSet<&str> = xml
                    .split(&format!("{file}#"))
                    .skip(1)
                    .filter_map(|r| r.split('"').next())
                    .filter(|k| k.starts_with("default"))
                    .collect();
                let missing: Vec<_> = wanted.iter().filter(|k| !types.index.contains_key(**k)).collect();
                let empty = types.meshes.iter().filter(|m| m.is_empty()).count();
                eprintln!(
                    "{file}: {} types ({empty} empty), {tris} triangles, {} referenced, missing {missing:?}, {} ms",
                    types.meshes.len(),
                    wanted.len(),
                    started.elapsed().as_millis()
                );
                assert!(missing.is_empty() && !types.meshes.is_empty());
            }
        }
    }
}
