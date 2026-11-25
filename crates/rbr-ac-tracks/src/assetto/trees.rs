//! CSP's tree lists: `extension/*.ini` `[TREES]` names lists (`LIST_<n>`, in `extension/trees`) of `tree:` lines, each
//! a `.bin` zip holding a `tree.ini`, LOD meshes and their textures. A tree is far too heavy to draw: 17,000 stand
//! along Mulholland, 3,000 oaks of 10,000 triangles within 300 m of Roure. Each is drawn as [`CARDS`] crossed vertical
//! cards, each card the tree's most detailed LOD rendered from one side ([`bake`]).
//!
//! The mesh format (`.mesh`, little endian) is undocumented, read from the files: a version word (1), a flag byte and
//! the texture names (`u32` count, each a `u32` hash, a `u32` length and the name), then parts of an albedo and a normal
//! texture index, `u32` index count and `u16` indices, `u32` vertex count and 44-byte vertices (position, normal, UV,
//! 12 bytes unread), and a 12 byte trailer. Positions are the tree scaled into `x, z` in -1..1 and `y` in 0..1;
//! `tree.ini` `WIDTH` is its full width and `HEIGHT` its height, in metres at a `size` of 1.
use rbr_btb_writer::textures::{self, Image};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

/// Vertical cards drawn for each tree, at equal angles about its axis.
pub const CARDS: usize = 4;
/// Side of a card's picture, texels; the cards sit side by side in one texture.
const VIEW: u32 = 256;

/// The distance from which a tree's card, `VIEW` texels across its width, is no coarser than the screen: 1080 pixels over
/// a 60 degree view are 935 pixels a metre at 1 m, and a card has `VIEW / span` texels a metre.
pub fn card_range(span: f64) -> f64 {
    935. * span / f64::from(VIEW)
}
/// The picture is rendered this many times as large across and averaged down.
const SUPERSAMPLE: u32 = 2;

/// One placed tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Tree {
    /// The `.bin` file name.
    pub bin: String,
    /// AC's `x, y, z` (y up).
    pub position: [f64; 3],
    /// Multiplies the tree's metres.
    pub size: f64,
    /// Multiplies its width only.
    pub width: f64,
    /// Turn about the vertical, radians.
    pub angle: f64,
}

/// A random number in `0..1` from `seed` (SplitMix64).
fn unit(seed: u64) -> f64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
}

/// The numbers after `=` on a `configure:` line, e.g. `size variance = 1.5, 0.8`.
fn pair(value: &str) -> Option<[f64; 2]> {
    let mut numbers = value.split(',').map(|v| v.trim().parse::<f64>().ok());
    let (a, b) = (numbers.next()??, numbers.next()??);
    Some([a.min(b), a.max(b)])
}

/// The trees of one list. `configure:` lines set the random `size` and `angle` ranges of the trees that follow; a tree
/// without a `size` takes one of the range, at random but the same on every run.
pub fn parse_list(text: &str) -> Vec<Tree> {
    let (mut size, mut angle) = ([1.; 2], [0., 360.]);
    let mut trees = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("configure:") {
            let Some((key, value)) = rest.split(';').next().unwrap_or("").split_once('=') else { continue };
            match key.trim() {
                "size variance" => size = pair(value).unwrap_or(size),
                "angle variance" => angle = pair(value).unwrap_or(angle),
                _ => {}
            }
            continue;
        }
        let Some(rest) = line.strip_prefix("tree:") else { continue };
        let mut fields = rest.split(';').map(str::trim);
        let Some(bin) = fields.next().filter(|b| !b.is_empty()) else { continue };
        let (mut position, mut explicit, mut width, mut turn) = (None, None, 1., None);
        for field in fields.filter(|f| !f.is_empty()) {
            let (key, value) = field.split_once('=').map_or(("pos", field), |(k, v)| (k.trim(), v.trim()));
            let number = || value.parse::<f64>().ok();
            match key {
                "pos" => {
                    let p: Vec<f64> = value.split(',').filter_map(|v| v.trim().parse().ok()).collect();
                    position = p.try_into().ok().filter(|p: &[f64; 3]| p.iter().all(|v| v.is_finite()));
                }
                "size" => explicit = number(),
                "width" => width = number().unwrap_or(1.),
                "angle" => turn = number().map(f64::to_radians),
                _ => {}
            }
        }
        let Some(position) = position else { continue };
        let seed = position.iter().fold(trees.len() as u64, |s, v| s.rotate_left(21) ^ v.to_bits());
        let (random, spin) = (unit(seed), unit(seed ^ 0x5851_f42d_4c95_7f2d));
        trees.push(Tree {
            bin: bin.to_owned(),
            position,
            size: explicit.unwrap_or(size[0] + (size[1] - size[0]) * random),
            width,
            angle: turn.unwrap_or((angle[0] + (angle[1] - angle[0]) * spin).to_radians()),
        });
    }
    trees
}

/// The trees of every `[TREES]` list of the track's `extension` folder.
pub fn read(root: &Path) -> Vec<Tree> {
    let mut trees = Vec::new();
    for text in super::csp::inis(root) {
        let mut in_trees = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                in_trees = line.to_uppercase().starts_with("[TREES]");
            } else if let Some((key, value)) = line.split_once('=').filter(|_| in_trees) {
                if !key.trim().to_uppercase().starts_with("LIST_") {
                    continue;
                }
                let file = value.split(';').next().unwrap_or("").trim();
                let path = ["extension/trees", "extension"]
                    .iter()
                    .find_map(|dir| super::stage::source_path(root, &format!("{dir}/{file}")).ok());
                if let Some(text) = path.and_then(|p| std::fs::read(p).ok()) {
                    trees.extend(parse_list(&String::from_utf8_lossy(&text)));
                }
            }
        }
    }
    trees
}

/// One draw of a tree mesh: its textures and its geometry.
struct Part {
    albedo: usize,
    normal: usize,
    indices: Vec<u16>,
    vertices: Vec<([f32; 3], [f32; 2])>,
    normals: Vec<[f32; 3]>,
}

struct TreeMesh {
    textures: Vec<String>,
    parts: Vec<Part>,
}

fn parse_mesh(data: &[u8]) -> Option<TreeMesh> {
    let mut at = 0;
    let word = |at: &mut usize| {
        let v = u32::from_le_bytes(data.get(*at..*at + 4)?.try_into().ok()?);
        *at += 4;
        Some(v)
    };
    (word(&mut at)? == 1).then_some(())?;
    at += 1;
    let mut names = Vec::new();
    for _ in 0..word(&mut at)? {
        word(&mut at)?;
        let length = word(&mut at)? as usize;
        names.push(String::from_utf8_lossy(data.get(at..at + length)?).into_owned());
        at += length;
    }
    let mut parts = Vec::new();
    // The trailer is 12 bytes: no part is that short.
    while data.len().saturating_sub(at) > 12 {
        let (albedo, normal) = (word(&mut at)? as usize, word(&mut at)? as usize);
        let count = word(&mut at)? as usize;
        let indices: Vec<u16> =
            data.get(at..at + count * 2)?.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        at += count * 2;
        let count = word(&mut at)? as usize;
        let floats = |at: usize, n: usize| -> Option<Vec<f32>> {
            data.get(at..at + n * 4)
                .map(|b| b.chunks_exact(4).map(|f| f32::from_le_bytes([f[0], f[1], f[2], f[3]])).collect())
        };
        let mut vertices = Vec::with_capacity(count);
        let mut normals = Vec::with_capacity(count);
        for i in 0..count {
            let v = floats(at + i * 44, 11)?;
            vertices.push(([v[0], v[1], v[2]], [v[6], v[7]]));
            normals.push([v[3], v[4], v[5]]);
        }
        at += count * 44;
        if albedo >= names.len() || normal >= names.len() || !indices.len().is_multiple_of(3) {
            return None;
        }
        if indices.iter().any(|&i| i as usize >= vertices.len()) {
            return None;
        }
        parts.push(Part { albedo, normal, indices, vertices, normals });
    }
    Some(TreeMesh { textures: names, parts })
}

/// A tree's cards: the picture, the box of its geometry and its size.
#[derive(Debug)]
pub struct Prototype {
    /// [`CARDS`] views side by side, each [`VIEW`] square.
    pub atlas: Image,
    /// Half width of the card in the unit mesh, and its lowest and highest points.
    pub radius: f32,
    pub low: f32,
    pub high: f32,
    /// Metres of a unit of the mesh across and up, at a `size` of 1.
    pub width: f64,
    pub height: f64,
    /// The tree's width in metres at a `size` of 1 (`tree.ini` `WIDTH`).
    pub span: f64,
    /// The LOD 0 mesh itself, one model for each of its parts.
    pub parts: Vec<ModelPart>,
}

/// One part of a tree's mesh, in the unit mesh's space (`x, z` in -1..1, `y` up), with its own texture.
#[derive(Debug)]
pub struct ModelPart {
    /// The albedo with the lighting's `BRIGHTNESS` on it and the cut-out's alpha ([`Sampler::image`]).
    pub texture: Image,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u16>,
}

/// A part's texture lookup: its colour, with the alpha of the albedo or, where that has none, of the normal map (the
/// cut-out of Roure's pines is kept there).
struct Sampler {
    albedo: Image,
    cut: Option<Image>,
}
impl Sampler {
    fn new(albedo: Option<Image>, normal: Option<Image>) -> Self {
        let albedo = albedo.unwrap_or((1, 1, vec![90, 110, 60, 255]));
        let opaque = albedo.2.chunks_exact(4).all(|t| t[3] == 255);
        let cut = normal.filter(|n| opaque && n.2.chunks_exact(4).any(|t| t[3] < 128));
        Self { albedo, cut }
    }
    fn texel(image: &Image, uv: [f32; 2]) -> [u8; 4] {
        let wrap = |v: f32| v - v.floor();
        let x = ((wrap(uv[0]) * image.0 as f32) as usize).min(image.0 as usize - 1);
        let y = ((wrap(uv[1]) * image.1 as f32) as usize).min(image.1 as usize - 1);
        let at = (y * image.0 as usize + x) * 4;
        [image.2[at], image.2[at + 1], image.2[at + 2], image.2[at + 3]]
    }
    /// The albedo as [`render`] draws it: `brightness` on the colour, the cut-out in the alpha.
    fn image(&self, brightness: f32) -> Image {
        let (width, height, texels) = &self.albedo;
        let mut rgba = texels.clone();
        for (at, t) in rgba.chunks_exact_mut(4).enumerate() {
            if let Some(cut) = &self.cut {
                let (x, y) = ((at % *width as usize) as f32, (at / *width as usize) as f32);
                t[3] = Self::texel(cut, [(x + 0.5) / *width as f32, (y + 0.5) / *height as f32])[3];
            }
            (0..3).for_each(|k| t[k] = (f32::from(t[k]) * brightness).min(255.) as u8);
        }
        (*width, *height, rgba)
    }
    fn sample(&self, uv: [f32; 2]) -> [u8; 4] {
        let mut t = Self::texel(&self.albedo, uv);
        if let Some(cut) = &self.cut {
            t[3] = Self::texel(cut, uv)[3];
        }
        t
    }
}

/// Renders `parts` from the side `azimuth` radians about the vertical: orthographic, over `-radius..radius` across and
/// `low..high` up, the nearest texel at or over half alpha. `size` square, RGBA with coverage in alpha.
fn render(
    parts: &[(&Part, Sampler)],
    azimuth: f32,
    (radius, low, high): (f32, f32, f32),
    brightness: f32,
    size: u32,
) -> Vec<[u8; 4]> {
    let n = size as usize;
    let (c, s) = (azimuth.cos(), azimuth.sin());
    let mut depth = vec![f32::NEG_INFINITY; n * n];
    let mut colour = vec![[0u8; 4]; n * n];
    for (part, sampler) in parts {
        // Pixel position, depth towards the viewer and UV of each vertex.
        let project: Vec<_> = part
            .vertices
            .iter()
            .map(|(p, uv)| {
                let x = (p[0] * c + p[2] * s + radius) / (2. * radius) * n as f32;
                let y = (high - p[1]) / (high - low) * n as f32;
                (x, y, -p[0] * s + p[2] * c, *uv)
            })
            .collect();
        for tri in part.indices.chunks_exact(3) {
            let [a, b, d] = [tri[0], tri[1], tri[2]].map(|i| project[i as usize]);
            let area = (b.0 - a.0) * (d.1 - a.1) - (b.1 - a.1) * (d.0 - a.0);
            if area.abs() < 1e-9 {
                continue;
            }
            let lo = |f: fn(f32, f32) -> f32, k: usize| {
                let v = |p: &(f32, f32, f32, [f32; 2])| if k == 0 { p.0 } else { p.1 };
                f(f(v(&a), v(&b)), v(&d))
            };
            let (x0, x1) =
                ((lo(f32::min, 0).floor().max(0.)) as usize, (lo(f32::max, 0).ceil().max(0.) as usize).min(n));
            let (y0, y1) =
                ((lo(f32::min, 1).floor().max(0.)) as usize, (lo(f32::max, 1).ceil().max(0.) as usize).min(n));
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((b.0 - px) * (d.1 - py) - (b.1 - py) * (d.0 - px)) / area;
                    let w1 = ((d.0 - px) * (a.1 - py) - (d.1 - py) * (a.0 - px)) / area;
                    let w2 = 1. - w0 - w1;
                    if w0 < 0. || w1 < 0. || w2 < 0. {
                        continue;
                    }
                    let z = w0 * a.2 + w1 * b.2 + w2 * d.2;
                    if z <= depth[y * n + x] {
                        continue;
                    }
                    let uv = [0, 1].map(|k| w0 * a.3[k] + w1 * b.3[k] + w2 * d.3[k]);
                    let texel = sampler.sample(uv);
                    if texel[3] >= 128 {
                        depth[y * n + x] = z;
                        let [r, g, b] = [0, 1, 2].map(|k| (f32::from(texel[k]) * brightness).min(255.) as u8);
                        colour[y * n + x] = [r, g, b, 255];
                    }
                }
            }
        }
    }
    colour
}

/// The atlas of [`CARDS`] views of `mesh`, [`VIEW`] square each, with the colour of empty texels the tree's mean so a
/// mipmapped edge does not darken.
fn bake(mesh: &TreeMesh, files: &BTreeMap<String, Vec<u8>>, extent: (f32, f32, f32), brightness: f32) -> Image {
    let image = |name: &String| files.get(&name.to_lowercase()).and_then(|b| textures::decode(b));
    let samplers: Vec<_> = mesh
        .parts
        .iter()
        .map(|p| (p, Sampler::new(image(&mesh.textures[p.albedo]), image(&mesh.textures[p.normal]))))
        .collect();
    let (width, size) = (VIEW * CARDS as u32, VIEW * SUPERSAMPLE);
    let mut rgba = vec![0u8; (width * VIEW * 4) as usize];
    let (mut sum, mut covered) = ([0u64; 3], 0u64);
    for view in 0..CARDS {
        let azimuth = std::f32::consts::PI * view as f32 / CARDS as f32;
        let big = render(&samplers, azimuth, extent, brightness, size);
        for y in 0..VIEW {
            for x in 0..VIEW {
                let (mut rgb, mut count) = ([0u32; 3], 0u32);
                for (dy, dx) in (0..SUPERSAMPLE).flat_map(|dy| (0..SUPERSAMPLE).map(move |dx| (dy, dx))) {
                    let t = big[((y * SUPERSAMPLE + dy) * size + x * SUPERSAMPLE + dx) as usize];
                    if t[3] != 0 {
                        count += 1;
                        (0..3).for_each(|k| rgb[k] += u32::from(t[k]));
                    }
                }
                if count == 0 {
                    continue;
                }
                let at = ((y * width + view as u32 * VIEW + x) * 4) as usize;
                (0..3).for_each(|k| {
                    rgba[at + k] = (rgb[k] / count) as u8;
                    sum[k] += u64::from(rgba[at + k]);
                });
                rgba[at + 3] = (count * 255 / (SUPERSAMPLE * SUPERSAMPLE)) as u8;
                covered += 1;
            }
        }
    }
    let mean = sum.map(|s| (s / covered.max(1)) as u8);
    for t in rgba.chunks_exact_mut(4).filter(|t| t[3] == 0) {
        t[..3].copy_from_slice(&mean);
    }
    (width, VIEW, rgba)
}

/// The cards of the tree in the `.bin` file at `path`: its first (most detailed) LOD baked ([`bake`]).
pub fn prototype(path: &Path) -> Result<Prototype, String> {
    let mut zip =
        zip::ZipArchive::new(std::fs::File::open(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let mut files = BTreeMap::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        files.insert(file.name().map_err(|e| e.to_string())?.to_lowercase(), bytes);
    }
    let ini = String::from_utf8_lossy(files.get("tree.ini").ok_or("no tree.ini")?).into_owned();
    let sections = rbr_formats::ini::parse(&ini).map_err(|e| e.to_string())?;
    let value = |section: &str, key: &str| {
        sections.iter().find(|s| s.name.eq_ignore_ascii_case(section)).and_then(|s| s.get(key)).map(str::trim)
    };
    let number = |key: &str| value("BASIC", key).and_then(|v| v.parse::<f64>().ok()).filter(|v| *v > 0.);
    let (width, height) = number("WIDTH").zip(number("HEIGHT")).ok_or("tree.ini has no WIDTH and HEIGHT")?;
    let model = value("LOD_0", "MODEL").ok_or("tree.ini has no LOD_0 MODEL")?;
    let mesh =
        files.get(&model.to_lowercase()).and_then(|d| parse_mesh(d)).ok_or_else(|| format!("unreadable {model}"))?;
    let points = mesh.parts.iter().flat_map(|p| p.vertices.iter().map(|v| v.0));
    let (mut radius, mut low, mut high, mut x, mut z) =
        (0f32, f32::MAX, f32::MIN, [f32::MAX, f32::MIN], [f32::MAX, f32::MIN]);
    for p in points {
        radius = radius.max(p[0].abs()).max(p[2].abs());
        (low, high) = (low.min(p[1]), high.max(p[1]));
        (x, z) = ([x[0].min(p[0]), x[1].max(p[0])], [z[0].min(p[2]), z[1].max(p[2])]);
    }
    if !(radius > 0. && high > low) {
        return Err(format!("{model} has no extent"));
    }
    // `BRIGHTNESS` is the lighting's gain on the albedo as `ksDiffuse` is a material's (0.2 to 0.3 for most; Roure's oak
    // has 0.95 for a leaf albedo of 0.07). The pictures are lit by ReRBR as an ordinary texture is: relative to 0.3.
    let brightness =
        (value("SHADING", "BRIGHTNESS").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.3) / 0.3).clamp(0.5, 4.);
    let atlas = bake(&mesh, &files, (radius, low, high), brightness);
    let image = |name: &String| files.get(&name.to_lowercase()).and_then(|b| textures::decode(b));
    let parts = mesh
        .parts
        .iter()
        .map(|p| ModelPart {
            texture: Sampler::new(image(&mesh.textures[p.albedo]), image(&mesh.textures[p.normal])).image(brightness),
            positions: p.vertices.iter().map(|v| v.0).collect(),
            normals: p.normals.clone(),
            uvs: p.vertices.iter().map(|v| v.1).collect(),
            indices: p.indices.clone(),
        })
        .collect();
    // `WIDTH` and `HEIGHT` are the tree's box: the mesh's is the box of its points.
    let across = f64::from((x[1] - x[0]).max(z[1] - z[0]));
    Ok(Prototype {
        atlas,
        radius,
        low,
        high,
        width: width / across,
        height: height / f64::from(high - low),
        span: width,
        parts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_takes_configured_ranges_and_explicit_sizes() {
        let text = "configure: size variance = 1.5, 0.8\t;\nconfigure: angle variance = 0, 90\n\n\
                    tree: a.bin; pos=1, 2, 3; \ntree:b.bin;4,5,6;size=3.5;width=0.5\ntree: c.bin; no position\n";
        let trees = parse_list(text);
        assert_eq!(trees.len(), 2);
        assert_eq!((trees[0].bin.as_str(), trees[0].position), ("a.bin", [1., 2., 3.]));
        assert!((0.8..=1.5).contains(&trees[0].size));
        assert!((0. ..=std::f64::consts::FRAC_PI_2).contains(&trees[0].angle));
        assert_eq!((trees[1].size, trees[1].width, trees[1].position), (3.5, 0.5, [4., 5., 6.]));
        assert_eq!(parse_list(text), trees.to_vec(), "the same on every run");
    }

    fn mesh_bytes(vertices: &[([f32; 3], [f32; 2])], indices: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(1u32.to_le_bytes());
        out.push(1);
        out.extend(2u32.to_le_bytes());
        for name in ["a.png", "n.png"] {
            out.extend(0u32.to_le_bytes());
            out.extend((name.len() as u32).to_le_bytes());
            out.extend(name.as_bytes());
        }
        out.extend([0u32, 1, indices.len() as u32].iter().flat_map(|w| w.to_le_bytes()));
        out.extend(indices.iter().flat_map(|i| i.to_le_bytes()));
        out.extend((vertices.len() as u32).to_le_bytes());
        for (p, uv) in vertices {
            let v = [p[0], p[1], p[2], 0., 1., 0., uv[0], uv[1], 0., 0., 0.];
            out.extend(v.iter().flat_map(|f| f.to_le_bytes()));
        }
        out.extend([0u8; 12]);
        out
    }

    #[test]
    fn a_mesh_reads_and_its_card_is_drawn_where_the_geometry_is() {
        // A quad in the x-y plane over the left half of the box, opaque red.
        let quad =
            [([-1., 0., 0.], [0., 1.]), ([0., 0., 0.], [1., 1.]), ([0., 1., 0.], [1., 0.]), ([-1., 1., 0.], [0., 0.])];
        let bytes = mesh_bytes(&quad, &[0, 1, 2, 0, 2, 3]);
        let mesh = parse_mesh(&bytes).unwrap();
        assert_eq!((mesh.textures.len(), mesh.parts.len(), mesh.parts[0].vertices.len()), (2, 1, 4));
        assert!(parse_mesh(&bytes[..bytes.len() - 20]).is_none());
        let files = BTreeMap::from([("a.png".to_owned(), rbr_formats::dds::rgba8_file(1, 1, &[200, 10, 10, 255]))]);
        let (width, height, rgba) = bake(&mesh, &files, (1., 0., 1.), 1.);
        assert_eq!((width, height), (VIEW * CARDS as u32, VIEW));
        let at = |x: u32, y: u32| &rgba[((y * width + x) * 4) as usize..][..4];
        // The front view (azimuth 0) sees the quad on the left; the right is empty.
        assert_eq!(at(VIEW / 4, VIEW / 2), [200, 10, 10, 255]);
        assert_eq!(at(VIEW * 3 / 4, VIEW / 2)[3], 0);
    }
}
