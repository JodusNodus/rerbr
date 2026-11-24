//! PSSG, the PhyreEngine scene files the game ships gzip'd: the schema, the node tree and its payloads.
//!
//! Layout (big endian): `PSSG`, file size, attribute-info count, node-info count; the schema (per node kind its id,
//! name and attributes' ids and names); then the nodes, each `id, size, attribute bytes, attributes (id, length,
//! value)` followed by either child nodes or a raw payload. A string value is `u32 length` + characters, an integer
//! or float is 4 bytes. Format as in EgoEngineModding/Ego-Engine-Modding (`EgoEngineLibrary/Graphics/Pssg`, MIT),
//! written afresh here and checked against the shipped files.
use std::collections::HashMap;
use std::io::Read;
use std::ops::Range;
use std::path::Path;

/// Nodes that only hold bytes, never children (a payload could pass the child chain check by accident).
const PAYLOAD_NODES: &[&str] = &[
    "DATABLOCKDATA",
    "INDEXSOURCEDATA",
    "TEXTUREIMAGEBLOCKDATA",
    "TRANSFORM",
    "BOUNDINGBOX",
    "SHADERINPUT",
    "INVERSEBINDMATRIX",
    "DATA",
];

struct NodeData {
    name: u32,
    attrs: Vec<(u32, Range<usize>)>,
    children: Vec<u32>,
    payload: Range<usize>,
}

/// A parsed file: the decompressed bytes and the node tree over them.
pub struct Pssg {
    data: Vec<u8>,
    node_names: Vec<String>,
    attr_names: Vec<String>,
    nodes: Vec<NodeData>,
    roots: Vec<u32>,
    by_id: HashMap<String, u32>,
}

/// One node of a [`Pssg`].
#[derive(Clone, Copy)]
pub struct Node<'a> {
    pssg: &'a Pssg,
    index: u32,
}

fn be32(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).map(|v| u32::from_be_bytes([v[0], v[1], v[2], v[3]])).ok_or_else(|| "truncated".to_string())
}

/// The big-endian floats of a payload (a TRANSFORM, a constant shader input).
pub fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|c| f32::from_bits(u32::from_be_bytes([c[0], c[1], c[2], c[3]]))).collect()
}

impl Pssg {
    /// Reads a file, gzip'd or raw.
    pub fn read(path: &Path) -> Result<Pssg, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Pssg::parse(bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Parses a file's bytes, gzip'd or raw.
    pub fn parse(bytes: Vec<u8>) -> Result<Pssg, String> {
        let data = if bytes.starts_with(&[0x1f, 0x8b]) {
            let hint = bytes.len().checked_sub(4).map_or(0, |at| be32(&bytes, at).map_or(0, u32::swap_bytes));
            let mut out = Vec::with_capacity(hint as usize);
            flate2::read::GzDecoder::new(&bytes[..]).read_to_end(&mut out).map_err(|e| format!("gzip: {e}"))?;
            out
        } else {
            bytes
        };
        if !data.starts_with(b"PSSG") {
            return Err("not a PSSG file".into());
        }
        let (attr_count, node_count) = (be32(&data, 8)?, be32(&data, 12)?);
        let mut at = 16;
        let mut node_names: Vec<String> = Vec::new();
        let mut attr_names: Vec<String> = Vec::new();
        let string = |at: &mut usize| -> Result<String, String> {
            let len = be32(&data, *at + 4)? as usize;
            let s = data.get(*at + 8..*at + 8 + len).ok_or("truncated schema")?;
            *at += 8 + len;
            Ok(String::from_utf8_lossy(s).into_owned())
        };
        for _ in 0..node_count {
            let id = be32(&data, at)? as usize;
            let name = string(&mut at)?;
            if node_names.len() <= id {
                node_names.resize(id + 1, String::new());
            }
            node_names[id] = name;
            let count = be32(&data, at)?;
            at += 4;
            for _ in 0..count {
                let id = be32(&data, at)? as usize;
                let name = string(&mut at)?;
                if attr_names.len() <= id {
                    attr_names.resize(id + 1, String::new());
                }
                attr_names[id] = name;
            }
        }
        let _ = attr_count;
        let mut pssg =
            Pssg { data, node_names, attr_names, nodes: Vec::new(), roots: Vec::new(), by_id: HashMap::new() };
        let end = pssg.data.len();
        if !pssg.chain_ok(at, end) {
            return Err("the node chain after the schema is damaged".into());
        }
        pssg.roots = pssg.read_chain(at, end)?;
        // Attribute ids are per node kind: several of them are called `id`.
        for (i, node) in pssg.nodes.iter().enumerate() {
            let Some((_, r)) = node.attrs.iter().find(|(a, _)| pssg.attr_names[*a as usize] == "id") else { continue };
            if let Some(id) = decode_str(&pssg.data[r.clone()]) {
                pssg.by_id.entry(id.to_owned()).or_insert(i as u32);
            }
        }
        Ok(pssg)
    }

    /// Whether `o..end` is exactly a run of whole nodes of known kinds.
    fn chain_ok(&self, mut o: usize, end: usize) -> bool {
        while o < end {
            let (Ok(id), Ok(size), Ok(attr_size)) =
                (be32(&self.data, o), be32(&self.data, o + 4), be32(&self.data, o + 8))
            else {
                return false;
            };
            let known = self.node_names.get(id as usize).is_some_and(|n| !n.is_empty());
            if !known || attr_size > size || o + 8 + size as usize > end {
                return false;
            }
            o += 8 + size as usize;
        }
        o == end
    }

    /// The nodes of a chain already checked by `chain_ok`, each with its children or payload.
    fn read_chain(&mut self, mut o: usize, end: usize) -> Result<Vec<u32>, String> {
        let mut out = Vec::new();
        while o < end {
            let (name, size, attr_size) =
                (be32(&self.data, o)?, be32(&self.data, o + 4)? as usize, be32(&self.data, o + 8)? as usize);
            let (mut at, attrs_end, node_end) = (o + 12, o + 12 + attr_size, o + 8 + size);
            let mut attrs = Vec::new();
            while at < attrs_end {
                let (id, len) = (be32(&self.data, at)?, be32(&self.data, at + 4)? as usize);
                attrs.push((id, at + 8..at + 8 + len));
                at += 8 + len;
            }
            if at != attrs_end {
                return Err("attributes overrun their node".into());
            }
            let index = self.nodes.len() as u32;
            self.nodes.push(NodeData { name, attrs, children: Vec::new(), payload: attrs_end..node_end });
            let is_payload = PAYLOAD_NODES.contains(&self.node_names[name as usize].as_str());
            if !is_payload && attrs_end < node_end && self.chain_ok(attrs_end, node_end) {
                let children = self.read_chain(attrs_end, node_end)?;
                let node = &mut self.nodes[index as usize];
                node.children = children;
                node.payload = node_end..node_end;
            }
            out.push(index);
            o = node_end;
        }
        Ok(out)
    }

    /// The top-level nodes (one `PSSGDATABASE`).
    pub fn roots(&self) -> impl Iterator<Item = Node<'_>> {
        self.roots.iter().map(|&index| Node { pssg: self, index })
    }

    /// The node whose `id` attribute is `id`.
    pub fn by_id(&self, id: &str) -> Option<Node<'_>> {
        self.by_id.get(id).map(|&index| Node { pssg: self, index })
    }

    /// Every node of the file in file order.
    pub fn all(&self) -> impl Iterator<Item = Node<'_>> {
        (0..self.nodes.len() as u32).map(|index| Node { pssg: self, index })
    }

    /// The nodes called `name` anywhere in the file, in file order.
    pub fn find<'a>(&'a self, name: &'a str) -> impl Iterator<Item = Node<'a>> {
        self.all().filter(move |n| n.name() == name)
    }

    /// The file's decompressed size.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

pub(crate) fn decode_str(v: &[u8]) -> Option<&str> {
    let len = u32::from_be_bytes(v.get(..4)?.try_into().ok()?) as usize;
    (v.len() == 4 + len).then(|| std::str::from_utf8(&v[4..]).ok()).flatten()
}

impl<'a> Node<'a> {
    /// The node kind, `RENDERDATASOURCE` and so on.
    pub fn name(&self) -> &'a str {
        &self.pssg.node_names[self.data().name as usize]
    }

    fn data(&self) -> &'a NodeData {
        &self.pssg.nodes[self.index as usize]
    }

    /// The raw value of an attribute.
    pub fn attr(&self, name: &str) -> Option<&'a [u8]> {
        let (_, r) = self.data().attrs.iter().find(|(a, _)| self.pssg.attr_names[*a as usize] == name)?;
        Some(&self.pssg.data[r.clone()])
    }

    /// Every attribute with its raw value, in file order.
    pub fn attrs(&self) -> impl Iterator<Item = (&'a str, &'a [u8])> + 'a {
        let pssg = self.pssg;
        self.data().attrs.iter().map(move |(a, r)| (pssg.attr_names[*a as usize].as_str(), &pssg.data[r.clone()]))
    }

    /// A string attribute.
    pub fn str(&self, name: &str) -> Option<&'a str> {
        decode_str(self.attr(name)?)
    }

    /// An integer attribute.
    pub fn u32(&self, name: &str) -> Option<u32> {
        let v = self.attr(name)?;
        (v.len() == 4).then(|| u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
    }

    /// A float attribute.
    pub fn f32(&self, name: &str) -> Option<f32> {
        self.u32(name).map(f32::from_bits)
    }

    /// The `id` attribute.
    pub fn id(&self) -> Option<&'a str> {
        self.str("id")
    }

    pub fn children(&self) -> impl Iterator<Item = Node<'a>> + 'a {
        let pssg = self.pssg;
        self.data().children.iter().map(move |&index| Node { pssg, index })
    }

    /// The first child called `name`.
    pub fn child(&self, name: &str) -> Option<Node<'a>> {
        self.children().find(|c| c.name() == name)
    }

    /// The bytes of a leaf node (empty on a node with children).
    pub fn payload(&self) -> &'a [u8] {
        &self.pssg.data[self.data().payload.clone()]
    }

    /// The payload of the child called `name`.
    pub fn child_payload(&self, name: &str) -> &'a [u8] {
        self.child(name).map_or(&[], |c| c.payload())
    }

    /// A `TRANSFORM` child (row major, row vectors, translation in the last row), identity when absent.
    pub fn transform(&self) -> [[f32; 4]; 4] {
        let f = floats(self.child_payload("TRANSFORM"));
        let mut m = [[0.0; 4]; 4];
        for (i, row) in m.iter_mut().enumerate() {
            *row = if f.len() == 16 { [f[4 * i], f[4 * i + 1], f[4 * i + 2], f[4 * i + 3]] } else { [0.; 4] };
            if f.len() != 16 {
                row[i] = 1.;
            }
        }
        m
    }

    /// The node an attribute such as `indices="#id"` or `shader="#id"` points at in this file.
    pub fn reference(&self, attr: &str) -> Option<Node<'a>> {
        self.pssg.by_id(self.str(attr)?.strip_prefix('#')?)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn s(v: &str) -> Vec<u8> {
        let mut o = (v.len() as u32).to_be_bytes().to_vec();
        o.extend(v.as_bytes());
        o
    }

    fn node(id: u32, attrs: &[(u32, Vec<u8>)], body: &[u8]) -> Vec<u8> {
        let mut a = Vec::new();
        for (aid, v) in attrs {
            a.extend(aid.to_be_bytes());
            a.extend((v.len() as u32).to_be_bytes());
            a.extend(v);
        }
        let mut o = id.to_be_bytes().to_vec();
        o.extend(((a.len() + body.len() + 4) as u32).to_be_bytes());
        o.extend((a.len() as u32).to_be_bytes());
        o.extend(a);
        o.extend(body);
        o
    }

    /// Kinds 1 `ROOT`, 2 `ITEM`, 3 `TRANSFORM`; attributes 1 `id`, 2 `n`, 3 `f`.
    pub(crate) fn tiny() -> Vec<u8> {
        let mut schema = Vec::new();
        for (id, name, attrs) in [
            (1u32, "ROOT", vec![(1u32, "id")]),
            (2, "ITEM", vec![(1, "id"), (2, "n"), (3, "f")]),
            (3, "TRANSFORM", vec![]),
        ] {
            schema.extend(id.to_be_bytes());
            schema.extend(s(name));
            schema.extend((attrs.len() as u32).to_be_bytes());
            for (aid, an) in attrs {
                schema.extend(aid.to_be_bytes());
                schema.extend(s(an));
            }
        }
        let mut t = Vec::new();
        for i in 0..16 {
            t.extend((i as f32).to_be_bytes());
        }
        let item = node(
            2,
            &[(1, s("a#b")), (2, 7u32.to_be_bytes().to_vec()), (3, 1.5f32.to_be_bytes().to_vec())],
            &node(3, &[], &t),
        );
        let leaf = node(2, &[(1, s("empty"))], &[]);
        let mut kids = item;
        kids.extend(leaf);
        let root = node(1, &[(1, s("root"))], &kids);
        let mut f = b"PSSG".to_vec();
        f.extend(0u32.to_be_bytes());
        f.extend(4u32.to_be_bytes());
        f.extend(3u32.to_be_bytes());
        f.extend(schema);
        f.extend(root);
        f
    }

    #[test]
    fn tree_attributes_payloads_and_ids() {
        let p = Pssg::parse(tiny()).unwrap();
        let root = p.roots().next().unwrap();
        assert_eq!((root.name(), root.id()), ("ROOT", Some("root")));
        let item = p.by_id("a#b").unwrap();
        assert_eq!((item.u32("n"), item.f32("f"), item.str("n")), (Some(7), Some(1.5), None));
        assert_eq!(item.transform()[3], [12., 13., 14., 15.]);
        assert_eq!(item.child("TRANSFORM").unwrap().payload().len(), 64);
        let empty = p.by_id("empty").unwrap();
        assert_eq!(empty.children().count(), 0);
        assert_eq!(empty.transform()[2], [0., 0., 1., 0.]);
        assert_eq!(p.find("ITEM").count(), 2);
    }

    #[test]
    fn gzip_and_damage() {
        use std::io::Write;
        let raw = tiny();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&raw).unwrap();
        assert!(Pssg::parse(gz.finish().unwrap()).unwrap().by_id("root").is_some());
        assert!(Pssg::parse(raw[..raw.len() - 3].to_vec()).is_err());
        assert!(Pssg::parse(b"nope".to_vec()).is_err());
    }

    /// Real files: `RBR_DIRT_RALLY` is the game folder; tests skip when it is unset.
    pub(crate) fn sample_areas() -> Vec<std::path::PathBuf> {
        let Some(game) = std::env::var_os("RBR_DIRT_RALLY") else { return Vec::new() };
        let install = crate::install::Install::at(Path::new(&game)).unwrap();
        ["greece/ghymno_rally_01", "finland/ouninpohja_rally_01"]
            .iter()
            .map(|a| install.data.join("tracks/locations").join(a))
            .collect()
    }

    #[test]
    fn every_pssg_of_the_sample_areas_parses() {
        for area in sample_areas() {
            let mut files: Vec<_> = std::fs::read_dir(&area).unwrap().map(|e| e.unwrap().path()).collect();
            files.extend(std::fs::read_dir(area.join("route_0")).unwrap().map(|e| e.unwrap().path()));
            for f in files.into_iter().filter(|f| f.extension().is_some_and(|e| e == "pssg")) {
                let started = std::time::Instant::now();
                match Pssg::read(&f) {
                    Ok(p) => {
                        assert!(p.roots().next().is_some_and(|r| r.name() == "PSSGDATABASE"), "{}", f.display());
                        eprintln!("{:>5} ms {:>10} B {}", started.elapsed().as_millis(), p.len(), f.display());
                    }
                    // route_objecttypes.pssg is XML text, not PSSG.
                    Err(e) => assert!(e.contains("not a PSSG") && f.to_string_lossy().contains("objecttypes"), "{e}"),
                }
            }
        }
    }
}
