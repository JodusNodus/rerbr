//! AC `ksMultilayer*` shaders. The diffuse map is a macro tint, often near white; the colour comes from four detail
//! textures weighted by the channels of a mask: `diffuse * (R*mask.r + G*mask.g + B*mask.b + A*mask.a) * magicMult`
//! (the compiled `ksMultilayer*_ps` shaders). A BTB material has one texture, so the sum is baked at the larger of
//! the diffuse and mask sizes.
//!
//! Detail texels are sampled at `uv * mult` for the `_objsp` shaders and at the world `(x, z) * mult` for the others.
//! The latter is baked per drawn tile where the stage is built (`bake_tile`), else by `bake` with the mesh's average
//! metres per UV unit, so the detail has the right scale but not the exact position. Detail finer than the baked
//! resolution becomes its average colour.
use crate::assetto::kn5::{Material, Model};
use crate::btb::textures::{self, Image};

/// Bakes above this size on either side are not made; the base diffuse is kept. The renderer takes what the GPU does
/// (ReRBR's largest stage texture is 16384 px) and the bake needs `4 * width * height` bytes for each map it holds.
const LIMIT: u32 = 16384;

const LAYERS: [&str; 4] = ["txDetailR", "txDetailG", "txDetailB", "txDetailA"];
const MULTIPLIERS: [&str; 4] = ["multR", "multG", "multB", "multA"];

/// Average horizontal metres per unit of `u` and of `v` over the faces using material `index`, weighted by UV area.
pub fn metres_per_uv(model: &Model, index: usize) -> [f64; 2] {
    let (mut sum, mut weight) = ([0.; 2], 0.);
    for mesh in model.meshes.iter().filter(|m| m.material == index) {
        for t in &mesh.triangles {
            let [a, b, c] = t.map(|i| mesh.vertices[i as usize]);
            let plan = |p: [f32; 3], q: [f32; 3]| [f64::from(q[0] - p[0]), f64::from(q[2] - p[2])];
            let (e1, e2) = (plan(a.position, b.position), plan(a.position, c.position));
            let (d1, d2) = (
                [f64::from(b.uv[0] - a.uv[0]), f64::from(b.uv[1] - a.uv[1])],
                [f64::from(c.uv[0] - a.uv[0]), f64::from(c.uv[1] - a.uv[1])],
            );
            let det = d1[0] * d2[1] - d1[1] * d2[0];
            if det.abs() < 1e-12 {
                continue;
            }
            let du = [(e1[0] * d2[1] - e2[0] * d1[1]) / det, (e1[1] * d2[1] - e2[1] * d1[1]) / det];
            let dv = [(e2[0] * d1[0] - e1[0] * d2[0]) / det, (e2[1] * d1[0] - e1[1] * d2[0]) / det];
            sum[0] += du[0].hypot(du[1]) * det.abs();
            sum[1] += dv[0].hypot(dv[1]) * det.abs();
            weight += det.abs();
        }
    }
    if weight > 0. {
        sum.map(|s| s / weight)
    } else {
        [1.; 2]
    }
}

/// Detail repeats per UV unit for layers R, G, B and A.
fn tiles(mat: &Material, metres: [f64; 2]) -> [[f64; 2]; 4] {
    std::array::from_fn(|i| {
        if mat.shader.contains("objsp") {
            mat.pairs.get(MULTIPLIERS[i]).map_or([0.; 2], |p| p.map(f64::from))
        } else {
            metres.map(|m| m * f64::from(mat.property(MULTIPLIERS[i], 0.)))
        }
    })
}

pub(super) fn texture(model: &Model, mat: &Material, slot: &str) -> Option<Image> {
    let data = model.textures.get(mat.textures.get(slot)?).filter(|d| !d.is_empty())?;
    textures::decode(data)
}

/// Bilinear RGBA sample at `(u, v)` in 0..1, repeating or clamped to the edge.
pub(super) fn sample(image: &Image, u: f64, v: f64, repeat: bool) -> [f32; 4] {
    let (w, h) = (image.0 as usize, image.1 as usize);
    let (x, y) = (u * w as f64 - 0.5, v * h as f64 - 0.5);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = ((x - x0) as f32, (y - y0) as f32);
    let wrap = |i: f64, n: usize| {
        if repeat {
            i.rem_euclid(n as f64) as usize
        } else {
            i.clamp(0., n as f64 - 1.) as usize
        }
    };
    let texel = |xi: f64, yi: f64| {
        let at = (wrap(yi, h) * w + wrap(xi, w)) * 4;
        std::array::from_fn::<f32, 4, _>(|k| f32::from(image.2[at + k]))
    };
    let (a, b, c, d) = (texel(x0, y0), texel(x0 + 1., y0), texel(x0, y0 + 1.), texel(x0 + 1., y0 + 1.));
    std::array::from_fn(|k| (a[k] * (1. - fx) + b[k] * fx) * (1. - fy) + (c[k] * (1. - fx) + d[k] * fx) * fy)
}

/// The detail halved while it stays at least `target` texels wide and high: finer would only alias.
pub(super) fn fit(mut image: Image, target: [f64; 2]) -> Image {
    while image.0 > 1 && image.1 > 1 && f64::from(image.0 / 2) >= target[0] && f64::from(image.1 / 2) >= target[1] {
        image = textures::half(image.0, image.1, &image.2);
    }
    image
}

/// The baked diffuse of a multilayer material, or `None` when a texture is missing or too large.
pub fn bake(model: &Model, index: usize, metres: [f64; 2]) -> Option<Image> {
    let mat = &model.materials[index];
    let (diffuse, mask) = (texture(model, mat, "txDiffuse")?, texture(model, mat, "txMask")?);
    let (width, height) = (diffuse.0.max(mask.0), diffuse.1.max(mask.1));
    if width > LIMIT || height > LIMIT {
        return None;
    }
    let tiles = tiles(mat, metres);
    let magic = mat.property("magicMult", 1.);
    let mut details = Vec::new();
    for (slot, t) in LAYERS.iter().zip(tiles) {
        let target = [f64::from(width) / t[0].max(1e-9), f64::from(height) / t[1].max(1e-9)];
        details.push(fit(texture(model, mat, slot)?, target));
    }
    let mut rgba = vec![255u8; width as usize * height as usize * 4];
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let rows = (height as usize).div_ceil(threads);
    std::thread::scope(|scope| {
        for (n, chunk) in rgba.chunks_mut(rows * width as usize * 4).enumerate() {
            let (diffuse, mask, details, tiles) = (&diffuse, &mask, &details, &tiles);
            scope.spawn(move || {
                for (r, row) in chunk.chunks_mut(width as usize * 4).enumerate() {
                    let v = ((n * rows + r) as f64 + 0.5) / f64::from(height);
                    for (x, out) in row.chunks_mut(4).enumerate() {
                        let u = (x as f64 + 0.5) / f64::from(width);
                        let (base, weights) = (sample(diffuse, u, v, false), sample(mask, u, v, false));
                        let mut color = [0f32; 3];
                        for ((detail, t), weight) in details.iter().zip(tiles).zip(weights) {
                            // A zero multiplier samples the first texel, as the shader's `uv * 0` does.
                            let s = if *t == [0.; 2] {
                                std::array::from_fn(|k| f32::from(detail.2[k]))
                            } else {
                                sample(detail, u * t[0], v * t[1], true)
                            };
                            for k in 0..3 {
                                color[k] += s[k] * weight / 255.;
                            }
                        }
                        for k in 0..3 {
                            out[k] = (base[k] * color[k] / 255. * magic).clamp(0., 255.).round() as u8;
                        }
                    }
                }
            });
        }
    });
    Some((width, height, rgba))
}

/// A tile bake has at most this many texels (2 MB as DXT1) and this long a side (a road tile 128 m long at 1.6 cm).
const TILE_TEXELS: f64 = 4_194_304.;
const TILE_SIDE: u32 = 8192;

/// A world-space multilayer material's textures, decoded once for its tile bakes; each detail with its halvings.
pub struct Layers {
    /// Metres along the coarser side of a texel of the material's single bake (`bake`); infinite when it is not made.
    pub texel: f64,
    diffuse: Image,
    mask: Image,
    details: Vec<Vec<Image>>,
    mult: [f64; 4],
    magic: f32,
}

impl Layers {
    /// `None` for an `_objsp` material (its details follow the UVs; the material bake keeps them), another shader or a
    /// missing texture.
    pub fn new(model: &Model, index: usize) -> Option<Self> {
        let mat = &model.materials[index];
        if !mat.shader.contains("Multilayer") || mat.shader.contains("objsp") {
            return None;
        }
        let mut details = Vec::new();
        for slot in LAYERS {
            let mut chain = vec![texture(model, mat, slot)?];
            while let Some(last) = chain.last().filter(|i| i.0 > 1 && i.1 > 1) {
                chain.push(textures::half(last.0, last.1, &last.2));
            }
            details.push(chain);
        }
        let (diffuse, mask) = (texture(model, mat, "txDiffuse")?, texture(model, mat, "txMask")?);
        let size = [diffuse.0.max(mask.0), diffuse.1.max(mask.1)];
        let metres = metres_per_uv(model, index);
        let texel = if size.iter().any(|&s| s > LIMIT) {
            f64::INFINITY
        } else {
            (0..2).map(|k| metres[k] / f64::from(size[k])).fold(0., f64::max)
        };
        Some(Self {
            texel,
            diffuse,
            mask,
            details,
            mult: std::array::from_fn(|i| f64::from(mat.property(MULTIPLIERS[i], 0.))),
            magic: mat.property("magicMult", 1.),
        })
    }
}

/// A corner of a triangle to bake: its position in AC's frame and its source UV.
#[derive(Clone, Copy, Debug)]
pub struct Corner {
    pub position: [f64; 3],
    pub uv: [f64; 2],
}

/// A tile's bake: its texture and the mesh drawn with it, whose vertices are copies of source vertices: a vertex on
/// the seam between two charts has one copy in each.
pub struct TileBake {
    pub image: Image,
    /// The source vertex of each vertex.
    pub vertices: Vec<u32>,
    pub uvs: Vec<[f32; 2]>,
    pub triangles: Vec<[u32; 3]>,
}

/// Empty texels around each chart in the atlas, so filtering and the first mipmaps stay within it.
const PAD: usize = 2;

/// A chart splits while its triangles cover less than this share of its box...
const FILL: f64 = 0.4;
/// ...and it is wider than this many texels at the target density.
const CHART_TEXELS: f64 = 64.;

/// Triangles laid out together: rotated by `turn` (cosine, sine) from the island's metres, with that box.
struct Chart {
    triangles: Vec<usize>,
    turn: [f64; 2],
    low: [f64; 2],
    size: [f64; 2],
}

/// Bakes one tile of a world-space multilayer material (`vertices`, `triangles` as indices) at about
/// `metres_per_texel`. AC's UVs are often sheared and scaled differently per island, so each UV island (triangles
/// connected through shared vertices) is mapped to metres by the affine map that makes its average texel square, cut
/// into charts that fill their boxes and packed into one texture. Each texel takes the source UV and world position
/// of the triangle it lies in, so the mask and details sit where AC's shader puts them; texels outside every
/// triangle take their neighbours'. `None` when the charts do not fit one texture.
pub fn bake_tile(
    layers: &Layers,
    vertices: &[Corner],
    triangles: &[[u32; 3]],
    metres_per_texel: f64,
) -> Option<TileBake> {
    let mut metres = vec![[0.; 2]; vertices.len()];
    let mut charts = Vec::new();
    for members in islands(vertices.len(), triangles) {
        let corners: Vec<[Corner; 3]> = members.iter().map(|&t| triangles[t].map(|v| vertices[v as usize])).collect();
        let used: std::collections::BTreeSet<u32> = members.iter().flat_map(|&t| triangles[t]).collect();
        let place = |by: &dyn Fn(&Corner) -> [f64; 2], metres: &mut [[f64; 2]]| {
            for &v in &used {
                metres[v as usize] = by(&vertices[v as usize]);
            }
        };
        // UVs that nearly collapse lay the island out at the wrong size: then the plane of its faces.
        let laid = |metres: &[[f64; 2]]| {
            members.iter().map(|&t| flat_area(triangles[t].map(|v| metres[v as usize]))).sum::<f64>()
        };
        let world: f64 = corners.iter().map(|t| norm(normal(t.map(|c| c.position))) / 2.).sum();
        if let Some([a, b, d]) = square(&corners) {
            place(&|c| [a * c.uv[0] + b * c.uv[1], d * c.uv[1]], &mut metres);
        }
        let ratio = laid(&metres) / world.max(1e-12);
        if !(0.5..=2.).contains(&ratio) {
            let n = corners
                .iter()
                .map(|t| normal(t.map(|c| c.position)))
                .fold([0.; 3], |s, n| [0, 1, 2].map(|k| s[k] + n[k]));
            let n = n.map(|x| x / norm(n).max(1e-12));
            // Any unit vector in the plane, then the one across it.
            let x = if n[1].abs() < 0.9 { cross(n, [0., 1., 0.]) } else { cross(n, [1., 0., 0.]) };
            let x = x.map(|v| v / norm(x).max(1e-12));
            let y = cross(n, x);
            place(&|c| [dot(c.position, x), dot(c.position, y)], &mut metres);
        }
        split(&metres, triangles, members, (CHART_TEXELS * metres_per_texel).powi(2), &mut charts);
    }
    // Square texels at the target, within the texel budget and side limit.
    let area: f64 = charts.iter().map(|c| c.size[0] * c.size[1]).sum();
    let mut texel = metres_per_texel.max((area / TILE_TEXELS).sqrt()).max(1e-6);
    let layout = |texel: f64| {
        pack(&charts.iter().map(|c| c.size.map(|s| (s / texel).ceil() as usize + 2 * PAD)).collect::<Vec<_>>())
    };
    let fits = |(w, h, _): &(usize, usize, Vec<[usize; 2]>)| {
        *w as f64 * *h as f64 <= TILE_TEXELS && *w.max(h) <= TILE_SIDE as usize
    };
    // Coarser until it fits; charts too many for the budget even at a texel each fail.
    let mut packed = layout(texel);
    for _ in 0..64 {
        if fits(&packed) {
            break;
        }
        texel *= 1.25;
        packed = layout(texel);
    }
    if !fits(&packed) {
        return None;
    }
    // The texture's power-of-two sides leave room: spend it on finer texels.
    for _ in 0..8 {
        let finer = layout(texel / 1.1);
        if finer.0 * finer.1 > packed.0 * packed.1 || !fits(&finer) {
            break;
        }
        packed = finer;
        texel /= 1.1;
    }
    let (width, height, at) = packed;
    // Each chart's vertices at their texel: metres turned, from the chart's corner, past the padding, at its place.
    let mut out = TileBake { image: (0, 0, Vec::new()), vertices: Vec::new(), uvs: Vec::new(), triangles: Vec::new() };
    let mut texels = Vec::new();
    for (chart, at) in charts.iter().zip(&at) {
        let mut copies = std::collections::HashMap::new();
        for &t in &chart.triangles {
            let triangle = triangles[t].map(|v| {
                *copies.entry(v).or_insert_with(|| {
                    let m = turn(chart.turn, metres[v as usize]);
                    texels.push([0, 1].map(|k| (m[k] - chart.low[k]) / texel + (at[k] + PAD) as f64));
                    out.vertices.push(v);
                    out.vertices.len() as u32 - 1
                })
            });
            out.triangles.push(triangle);
        }
    }
    let corners: Vec<Corner> = out.vertices.iter().map(|&v| vertices[v as usize]).collect();
    out.image = raster(layers, &corners, &out.triangles, &texels, texel, width, height);
    out.uvs = texels.iter().map(|t| [(t[0] / width as f64) as f32, (t[1] / height as f64) as f32]).collect();
    Some(out)
}

fn dot(p: [f64; 3], q: [f64; 3]) -> f64 {
    p[0] * q[0] + p[1] * q[1] + p[2] * q[2]
}

fn cross(p: [f64; 3], q: [f64; 3]) -> [f64; 3] {
    [p[1] * q[2] - p[2] * q[1], p[2] * q[0] - p[0] * q[2], p[0] * q[1] - p[1] * q[0]]
}

fn norm(p: [f64; 3]) -> f64 {
    dot(p, p).sqrt()
}

/// A face's normal, as long as twice its area.
fn normal(p: [[f64; 3]; 3]) -> [f64; 3] {
    cross([0, 1, 2].map(|k| p[1][k] - p[0][k]), [0, 1, 2].map(|k| p[2][k] - p[0][k]))
}

fn flat_area(p: [[f64; 2]; 3]) -> f64 {
    ((p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1])).abs() / 2.
}

fn turn(by: [f64; 2], p: [f64; 2]) -> [f64; 2] {
    [by[0] * p[0] + by[1] * p[1], by[0] * p[1] - by[1] * p[0]]
}

/// Lays `members` out along the principal axis of their faces and halves them across it while they fill less than
/// `FILL` of a box larger than `smallest` square metres.
fn split(metres: &[[f64; 2]], triangles: &[[u32; 3]], members: Vec<usize>, smallest: f64, out: &mut Vec<Chart>) {
    let corners = |t: usize| triangles[t].map(|v| metres[v as usize]);
    let centre = |p: [[f64; 2]; 3]| [0, 1].map(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.);
    let weights: Vec<f64> = members.iter().map(|&t| flat_area(corners(t)) + 1e-12).collect();
    let total: f64 = weights.iter().sum();
    let mean =
        [0, 1].map(|k| members.iter().zip(&weights).map(|(&t, w)| centre(corners(t))[k] * w).sum::<f64>() / total);
    let mut moment = [0.; 3];
    for (&t, w) in members.iter().zip(&weights) {
        let c = centre(corners(t));
        let d = [c[0] - mean[0], c[1] - mean[1]];
        for (sum, value) in moment.iter_mut().zip([d[0] * d[0], d[0] * d[1], d[1] * d[1]]) {
            *sum += value * w;
        }
    }
    let angle = 0.5 * (2. * moment[1]).atan2(moment[0] - moment[2]);
    let by = [angle.cos(), angle.sin()];
    let turned = |t: usize| corners(t).map(|p| turn(by, p));
    let low = [0, 1].map(|k| members.iter().flat_map(|&t| turned(t)).map(|p| p[k]).fold(f64::MAX, f64::min));
    let high = [0, 1].map(|k| members.iter().flat_map(|&t| turned(t)).map(|p| p[k]).fold(f64::MIN, f64::max));
    let size = [0, 1].map(|k| (high[k] - low[k]).max(0.));
    let boxed = size[0] * size[1];
    if members.len() < 2 || boxed <= smallest || total >= FILL * boxed {
        out.push(Chart { triangles: members, turn: by, low, size });
        return;
    }
    // Halve by area along the long axis, which the principal axis is.
    let mut order: Vec<(f64, usize, f64)> =
        members.iter().zip(&weights).map(|(&t, &w)| (centre(turned(t))[0], t, w)).collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut sum = 0.;
    let cut = order.iter().position(|o| {
        sum += o.2;
        sum >= total / 2.
    });
    let cut = cut.unwrap_or(0).clamp(1, order.len() - 1);
    let second = order.split_off(cut);
    split(metres, triangles, order.into_iter().map(|o| o.1).collect(), smallest, out);
    split(metres, triangles, second.into_iter().map(|o| o.1).collect(), smallest, out);
}

/// The triangles connected through shared vertices, each group in triangle order.
fn islands(vertex_count: usize, triangles: &[[u32; 3]]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..vertex_count).collect();
    fn root(parent: &mut [usize], mut v: usize) -> usize {
        while parent[v] != v {
            parent[v] = parent[parent[v]];
            v = parent[v];
        }
        v
    }
    for t in triangles {
        let a = root(&mut parent, t[0] as usize);
        for &v in &t[1..] {
            let b = root(&mut parent, v as usize);
            parent[b] = a;
        }
    }
    let mut groups = std::collections::BTreeMap::<usize, Vec<usize>>::new();
    for (i, t) in triangles.iter().enumerate() {
        groups.entry(root(&mut parent, t[0] as usize)).or_default().push(i);
    }
    groups.into_values().collect()
}

/// The affine map `(u, v) -> (a u + b v, d v)` to metres under which `triangles`' texels are square on average
/// (the Cholesky factor of their UV-area-weighted metric), or `None` when their UVs have no area. Faces with far
/// more metres per UV area than the median, whose UVs nearly collapse, are left out.
fn square(triangles: &[[Corner; 3]]) -> Option<[f64; 3]> {
    let faces: Vec<([f64; 3], [f64; 3], f64)> = triangles
        .iter()
        .filter_map(|[a, b, c]| {
            let (e1, e2): ([f64; 3], [f64; 3]) = (
                std::array::from_fn(|k| b.position[k] - a.position[k]),
                std::array::from_fn(|k| c.position[k] - a.position[k]),
            );
            let (d1, d2) = ([b.uv[0] - a.uv[0], b.uv[1] - a.uv[1]], [c.uv[0] - a.uv[0], c.uv[1] - a.uv[1]]);
            let det = d1[0] * d2[1] - d1[1] * d2[0];
            // World metres per unit of u and of v on this face.
            let du: [f64; 3] = std::array::from_fn(|k| (e1[k] * d2[1] - e2[k] * d1[1]) / det);
            let dv: [f64; 3] = std::array::from_fn(|k| (e2[k] * d1[0] - e1[k] * d2[0]) / det);
            (det.abs() > 1e-12).then_some((du, dv, det.abs()))
        })
        .collect();
    let scale = |(du, dv, _): &([f64; 3], [f64; 3], f64)| norm(cross(*du, *dv));
    let mut scales: Vec<f64> = faces.iter().map(scale).collect();
    scales.sort_by(f64::total_cmp);
    let median = *scales.get(scales.len() / 2)?;
    let (mut g, mut weight) = ([0.; 3], 0.);
    for face @ (du, dv, det) in &faces {
        if scale(face) <= 16. * median {
            for (sum, value) in g.iter_mut().zip([dot(*du, *du), dot(*du, *dv), dot(*dv, *dv)]) {
                *sum += value * det;
            }
            weight += det;
        }
    }
    let [uu, uv, vv] = g.map(|s| s / weight);
    let a = uu.sqrt();
    let b = uv / a;
    let d = (vv - b * b).sqrt();
    (weight > 0. && a > 1e-12 && d > 1e-12).then_some([a, b, d])
}

/// Shelf packing of `sizes` (tallest first) into the smallest power-of-two texture: its width, height and each
/// rectangle's corner.
fn pack(sizes: &[[usize; 2]]) -> (usize, usize, Vec<[usize; 2]>) {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes[i][1]));
    let widest = sizes.iter().map(|s| s[0]).max().unwrap_or(1).max(4).next_power_of_two();
    (0..8).map(|k| shelves(sizes, &order, widest << k)).min_by_key(|(w, h, _)| (w * h, w.max(h) - w.min(h))).unwrap()
}

fn shelves(sizes: &[[usize; 2]], order: &[usize], width: usize) -> (usize, usize, Vec<[usize; 2]>) {
    let (mut x, mut y, mut row) = (0, 0, 0);
    let mut at = vec![[0; 2]; sizes.len()];
    for &i in order {
        if x + sizes[i][0] > width {
            (x, y, row) = (0, y + row, 0);
        }
        at[i] = [x, y];
        x += sizes[i][0];
        row = row.max(sizes[i][1]);
    }
    (width, (y + row).max(4).next_power_of_two(), at)
}

/// Rasterizes `triangles` at the texel positions `texels` (by vertex) into a `width` by `height` bake whose texels
/// are `texel` metres.
fn raster(
    layers: &Layers,
    vertices: &[Corner],
    triangles: &[[u32; 3]],
    texels: &[[f64; 2]],
    texel: f64,
    width: usize,
    height: usize,
) -> Image {
    // Each detail at the halving that keeps at least one of its texels per baked texel.
    let details: Vec<&Image> = layers
        .details
        .iter()
        .zip(layers.mult)
        .map(|(chain, mult)| {
            let target = if mult > 0. { 1. / (mult * texel) } else { 0. };
            let mut level = &chain[0];
            for next in &chain[1..] {
                if f64::from(level.0 / 2) < target || f64::from(level.1 / 2) < target {
                    break;
                }
                level = next;
            }
            level
        })
        .collect();
    let (w, h) = (width, height);
    let mut rgba = vec![0u8; w * h * 4];
    let mut covered = vec![false; w * h];
    for triangle in triangles {
        let (p, t) = (triangle.map(|v| texels[v as usize]), triangle.map(|v| vertices[v as usize]));
        let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
        if area.abs() < 1e-12 {
            continue;
        }
        let range = |k: usize, n: usize| {
            let lo = p.iter().map(|q| q[k]).fold(f64::MAX, f64::min).floor().max(0.) as usize;
            let hi = (p.iter().map(|q| q[k]).fold(f64::MIN, f64::max).ceil().max(0.) as usize).min(n);
            lo..hi
        };
        for y in range(1, h) {
            for x in range(0, w) {
                let c = [x as f64 + 0.5, y as f64 + 0.5];
                let edge =
                    |a: [f64; 2], b: [f64; 2]| ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])) / area;
                let weights = [edge(p[1], p[2]), edge(p[2], p[0]), edge(p[0], p[1])];
                if weights.iter().any(|w| *w < -1e-6) || covered[y * w + x] {
                    continue;
                }
                let at = |f: &dyn Fn(&Corner) -> f64| weights.iter().zip(t).map(|(w, c)| w * f(&c)).sum::<f64>();
                let (u, v) = (at(&|c| c.uv[0]), at(&|c| c.uv[1]));
                let (wx, wz) = (at(&|c| c.position[0]), at(&|c| c.position[2]));
                let (base, mask) = (sample(&layers.diffuse, u, v, true), sample(&layers.mask, u, v, true));
                let mut color = [0f32; 3];
                for ((detail, mult), weight) in details.iter().zip(layers.mult).zip(mask) {
                    // A zero multiplier samples the first texel, as the shader's `uv * 0` does.
                    let s = if mult == 0. {
                        std::array::from_fn(|k| f32::from(detail.2[k]))
                    } else {
                        sample(detail, wx * mult, wz * mult, true)
                    };
                    for k in 0..3 {
                        color[k] += s[k] * weight / 255.;
                    }
                }
                let out = &mut rgba[(y * w + x) * 4..][..4];
                for k in 0..3 {
                    out[k] = (base[k] * color[k] / 255. * layers.magic).clamp(0., 255.).round() as u8;
                }
                out[3] = 255;
                covered[y * w + x] = true;
            }
        }
    }
    dilate(&mut rgba, &mut covered, w, h);
    (width as u32, height as u32, rgba)
}

/// Fills texels outside every triangle from their filled neighbours, so filtering and mipmaps at a mesh's edge do not
/// pull in black; whatever stays empty takes the mean colour.
fn dilate(rgba: &mut [u8], covered: &mut [bool], w: usize, h: usize) {
    for _ in 0..8 {
        let mut next = Vec::new();
        for y in 0..h {
            for x in 0..w {
                if covered[y * w + x] {
                    continue;
                }
                let (mut sum, mut n) = ([0u32; 4], 0);
                for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx >= 0
                        && ny >= 0
                        && (nx as usize) < w
                        && (ny as usize) < h
                        && covered[ny as usize * w + nx as usize]
                    {
                        let at = (ny as usize * w + nx as usize) * 4;
                        for k in 0..4 {
                            sum[k] += u32::from(rgba[at + k]);
                        }
                        n += 1;
                    }
                }
                if n > 0 {
                    next.push((y * w + x, sum.map(|s| (s / n) as u8)));
                }
            }
        }
        if next.is_empty() {
            break;
        }
        for (i, color) in next {
            rgba[i * 4..][..4].copy_from_slice(&color);
            covered[i] = true;
        }
    }
    let filled = covered.iter().filter(|c| **c).count().max(1) as u64;
    let mean: [u8; 4] = std::array::from_fn(|k| {
        (covered.iter().enumerate().filter(|(_, c)| **c).map(|(i, _)| u64::from(rgba[i * 4 + k])).sum::<u64>() / filled)
            as u8
    });
    for (i, _) in covered.iter().enumerate().filter(|(_, c)| !**c) {
        rgba[i * 4..][..4].copy_from_slice(&mean);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assetto::kn5::{Mesh, Vertex, IDENTITY};
    use std::collections::BTreeMap;

    fn dds(rgba: [u8; 4]) -> Vec<u8> {
        rbr_formats::dds::rgba8_file(2, 2, &rgba.repeat(4))
    }

    fn model(shader: &str, mask: [u8; 4]) -> Model {
        let slots = [("txDiffuse", "d"), ("txMask", "m"), ("txDetailR", "r"), ("txDetailG", "g")]
            .into_iter()
            .chain([("txDetailB", "b"), ("txDetailA", "a")]);
        let mut material = Material { shader: shader.into(), ..Material::default() };
        material.textures = slots.map(|(slot, name)| (slot.to_string(), name.to_string())).collect();
        material.properties.insert("magicMult".into(), 0.5);
        let textures = [
            ("d", [200, 100, 50, 255]),
            ("m", mask),
            ("r", [100, 0, 0, 255]),
            ("g", [0, 100, 0, 255]),
            ("b", [0, 0, 100, 255]),
            ("a", [100; 4]),
        ];
        Model {
            version: 6,
            textures: textures.into_iter().map(|(n, c)| (n.to_string(), dds(c))).collect(),
            materials: vec![material],
            meshes: Vec::new(),
            markers: BTreeMap::from([("unused".to_string(), IDENTITY)]),
            dropped_triangles: 0,
            repaired_uvs: 0,
            repaired_normals: 0,
        }
    }

    #[test]
    fn the_baked_colour_is_diffuse_times_the_mask_weighted_details_times_magic() {
        // Half red detail, half green detail: (50, 50, 0), times the diffuse over 255, times 0.5.
        let (w, h, rgba) = bake(&model("ksMultilayer", [128, 128, 0, 0]), 0, [1.; 2]).unwrap();
        assert_eq!((w, h), (2, 2));
        let expected = [200. * 50.2 / 255. * 0.5, 100. * 50.2 / 255. * 0.5, 0.];
        for (got, want) in rgba[..3].iter().zip(expected) {
            assert!((f32::from(*got) - want).abs() <= 1., "{rgba:?}");
        }
        assert_eq!(rgba[3], 255);
    }

    #[test]
    fn a_missing_detail_keeps_the_base_diffuse() {
        let mut m = model("ksMultilayer", [255, 0, 0, 0]);
        m.textures.remove("g");
        assert!(bake(&m, 0, [1.; 2]).is_none());
    }

    #[test]
    fn metres_per_uv_follows_the_plan_view_scale_of_the_faces() {
        let corner = |x: f32, z: f32, u: f32, v: f32| Vertex { position: [x, 5., z], normal: [0., 1., 0.], uv: [u, v] };
        let mut m = model("ksMultilayer", [255, 0, 0, 0]);
        // 40 m along x for one u, 10 m along z for one v, on a slope that does not count.
        m.meshes.push(Mesh {
            name: "m".into(),
            helper: false,
            active: true,
            visible: true,
            renderable: true,
            cast_shadows: false,
            transparent: false,
            material: 0,
            lod_in: 0.,
            lod_out: 0.,
            vertex_count: 3,
            triangle_count: 1,
            vertices: vec![corner(0., 0., 0., 0.), corner(40., 0., 1., 0.), corner(0., 10., 0., 1.)],
            triangles: vec![[0, 1, 2]],
        });
        let [u, v] = metres_per_uv(&m, 0);
        assert!((u - 40.).abs() < 1e-9 && (v - 10.).abs() < 1e-9);
    }

    fn flat(rgba: [u8; 4]) -> Image {
        (4, 4, rgba.repeat(16))
    }

    fn layers() -> Layers {
        Layers {
            texel: f64::INFINITY,
            diffuse: flat([200, 100, 50, 255]),
            mask: flat([255, 0, 0, 0]),
            details: vec![vec![flat([128, 255, 64, 255])], vec![flat([0; 4])], vec![flat([0; 4])], vec![flat([0; 4])]],
            mult: [0.5, 0., 0., 0.],
            magic: 1.,
        }
    }

    /// A flat quad `length` by `width` metres along x and z, its UVs from `uv` at the corners.
    fn quad(length: f64, width: f64, uv: impl Fn(f64, f64) -> [f64; 2]) -> Vec<Corner> {
        [(0., 0.), (length, 0.), (length, width), (0., width)]
            .map(|(x, z)| Corner { position: [x, 0., z], uv: uv(x, z) })
            .to_vec()
    }

    /// Texels per metre along x and along z of a baked quad's first triangle.
    fn density(bake: &TileBake, corners: &[Corner]) -> [f64; 2] {
        let (w, h) = (f64::from(bake.image.0), f64::from(bake.image.1));
        let at = |i: usize| {
            let uv = bake.uvs[i];
            [f64::from(uv[0]) * w, f64::from(uv[1]) * h]
        };
        let source = |i: usize| corners[bake.vertices[i] as usize].position;
        let t = bake.triangles[0].map(|v| v as usize);
        let along = |k: usize| {
            let (a, b) = t.iter().flat_map(|&a| t.iter().map(move |&b| (a, b))).find(|&(a, b)| {
                let (p, q) = (source(a), source(b));
                (q[k] - p[k]).abs() > 0.1 && (q[2 - k] - p[2 - k]).abs() < 1e-9
            })?;
            let (p, q) = (at(a), at(b));
            Some(((q[0] - p[0]).hypot(q[1] - p[1])) / (source(b)[k] - source(a)[k]).abs())
        };
        [along(0).unwrap(), along(2).unwrap()]
    }

    #[test]
    fn sheared_uvs_bake_to_square_texels_at_the_target() {
        // A 12 m by 3 m road whose UVs are sheared and stretched along it, as AC road meshes often are.
        let corners = quad(12., 3., |x, z| [0.1 * x + 0.3 * z, 0.02 * x]);
        let bake = bake_tile(&layers(), &corners, &[[0, 1, 2], [0, 2, 3]], 0.05).unwrap();
        let [along, across] = density(&bake, &corners);
        assert!((along / across - 1.).abs() < 0.01, "{along} {across}");
        // The target or finer, as the texture's spare room allows.
        assert!((19.9..30.).contains(&along), "{along}");
        // Diffuse times the red layer's detail.
        assert_eq!(&bake.image.2[..4], &[100, 100, 13, 255]);
    }

    #[test]
    fn islands_are_charted_separately_and_a_bent_one_is_cut() {
        // Two quads that share no vertex, then an L of two long arms that fills a quarter of its box.
        let mut corners = quad(10., 2., |x, z| [x / 10., z / 10.]);
        corners.extend(quad(10., 2., |x, z| [x / 10. + 0.5, z / 10.]));
        let two = bake_tile(&layers(), &corners, &[[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7]], 0.05).unwrap();
        assert_eq!(two.vertices.len(), 8);
        let l = [[0., 0.], [40., 0.], [40., 2.], [2., 2.], [2., 40.], [0., 40.]]
            .map(|[x, z]| Corner { position: [x, 0., z], uv: [x / 40., z / 40.] });
        let cut = bake_tile(&layers(), &l, &[[0, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 5]], 0.05).unwrap();
        assert!(cut.vertices.len() > l.len(), "{:?}", cut.vertices);
        // Every triangle keeps its source corners.
        let mut sources: Vec<[u32; 3]> = cut.triangles.iter().map(|t| t.map(|v| cut.vertices[v as usize])).collect();
        sources.sort();
        assert_eq!(sources, [[0, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 5]]);
    }

    #[test]
    fn a_soup_of_more_charts_than_the_budget_holds_is_not_baked() {
        // Every triangle its own island: 180,000 charts of at least 5 by 5 texels with padding.
        let corners: Vec<Corner> = (0..180_000 * 3)
            .map(|i| {
                let (t, k) = ((i / 3) as f64, i % 3);
                let p = [t + f64::from(u8::from(k == 1)), 0., f64::from(u8::from(k == 2))];
                Corner { position: p, uv: [p[0] / 10., p[2] / 10.] }
            })
            .collect();
        let triangles: Vec<[u32; 3]> = (0..180_000u32).map(|t| [3 * t, 3 * t + 1, 3 * t + 2]).collect();
        assert!(bake_tile(&layers(), &corners, &triangles, 0.05).is_none());
    }

    #[test]
    fn the_texel_budget_coarsens_a_large_tile() {
        let corners = quad(1000., 1000., |x, z| [x / 100., z / 100.]);
        let bake = bake_tile(&layers(), &corners, &[[0, 1, 2], [0, 2, 3]], 0.04).unwrap();
        let (w, h) = (bake.image.0, bake.image.1);
        assert!(f64::from(w) * f64::from(h) <= TILE_TEXELS && w.max(h) <= TILE_SIDE, "{w}x{h}");
    }
}
