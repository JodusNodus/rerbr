//! The terrain drawn as the game blends it, by ReRBR's per-pixel layers (`ReRbrLayer*`).
//!
//! DiRT Rally's terrain shaders sum up to four diffuse layers by the vertex `Color` (R the second layer's weight, G the
//! third's, the first takes the rest), then multiply a macro colour map (`TColourMap`, times `ColourMapScale`) and a
//! baked lightmap (`TAmbientOcclusion`). Both maps lie on the second `ST` stream: one unique unwrap (an atlas) of the
//! area's ground, shared by every material of a section. The renderer sums `diffuse * (mask.r*detail0 + mask.g*detail1
//! + ..) * magic` per pixel with the diffuse and mask at the mesh's uv and each detail at `uv * scale`, so here:
//!
//! - the diffuse is the macro colour times the lightmap, the mask the vertex weights, both rasterised in the atlas (one
//!   texture each per section, shared by the materials whose triangles lie in it);
//! - the mesh's uv becomes the atlas, transposed (the unwrap lays a road's strip along u, the layers' ruts run along v);
//! - each detail is a layer's own texture at the repeat the game tiles it at, per axis, from the triangles' metres per
//!   atlas unit and per layer unit. The details tile the atlas's islands, not the surface, so a pattern jumps at an
//!   island's border and a slope's detail is not stretched as the game's is.
//!
//! The blend maps (`TBlendMap2`, `TBlendMap3`, which the game reads at the layer's uv to roughen a weight's border) are
//! not used: they live in a finer space than the mask holds.
use crate::material::{self, Material};
use crate::mesh::Mesh;
use crate::texture::Library;
use rbr_ac_tracks::btb::textures;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Side of a section's mask, texels (about 12 cm of ground on Greece's sections).
const MASK: usize = 2048;
/// Texels the rasterised images are grown past their triangles: the filter reads across an island's border.
const GROW: usize = 3;
/// The share of an island's triangles whose layer runs along (or across) the atlas for the atlas to carry its layers.
const ALIGNED: f32 = 0.8;
/// The most an island's atlas may differ in rate per metre between its axes to carry its layers.
const STRETCH: f32 = 1.6;
/// The share of the colour map's hue the diffuse keeps.
const TINT: f32 = 0.5;
/// The layer inputs in the order the shader sums them.
const LAYERS: [&str; 4] = ["TDiffuseSpecMap1", "TDiffuseSpecMap2", "TDiffuseSpecMap3", "TDiffuseSpecMap4"];
/// The RGBA channel of the mask each layer's weight goes in: the colour block holds two independent weights well, so the
/// third layer takes the alpha.
const CHANNEL: [usize; 4] = [0, 1, 3, 2];

/// What the renderer needs to draw a material per pixel: DDS files and numbers, shared by `Arc`.
#[derive(Clone)]
pub struct Layers {
    /// The details are read at the world's (x, z), not at the uv.
    pub world: bool,
    pub diffuse: Arc<Vec<u8>>,
    pub mask: Arc<Vec<u8>>,
    /// By mask channel; a channel without a layer repeats the first one (its weight is 0).
    pub details: [Arc<Vec<u8>>; 4],
    /// Each detail's `(u, v)` repeats per atlas unit.
    pub scale: [[f32; 2]; 4],
    pub magic: f32,
}

/// The shading of a file's terrain: the layers' textures shared across its materials.
#[derive(Default)]
pub struct Shader {
    details: HashMap<String, Option<Arc<Vec<u8>>>>,
}

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn length(a: V3) -> f64 {
    dot(a, a).sqrt()
}

/// The surface's derivatives `(dP/du, dP/dv)` of a triangle for the uv `uv`; `None` where the uv collapse.
fn derivatives(p: [V3; 3], uv: [[f32; 2]; 3]) -> Option<(V3, V3)> {
    let (e1, e2) = (sub(p[1], p[0]), sub(p[2], p[0]));
    let (a, b) = (f64::from(uv[1][0] - uv[0][0]), f64::from(uv[1][1] - uv[0][1]));
    let (c, d) = (f64::from(uv[2][0] - uv[0][0]), f64::from(uv[2][1] - uv[0][1]));
    let det = a * d - b * c;
    if det.abs() < 1e-12 {
        return None;
    }
    let mix = |x: f64, y: f64| std::array::from_fn(|k| (e1[k] * x + e2[k] * y) / det);
    Some((mix(d, -b), mix(-c, a)))
}

fn weighted_median(mut values: Vec<(f64, f64)>) -> Option<f64> {
    values.sort_by(|a, b| a.0.total_cmp(&b.0));
    let half = values.iter().map(|v| v.1).sum::<f64>() / 2.;
    let mut seen = 0.;
    values
        .into_iter()
        .find(|v| {
            seen += v.1;
            seen >= half
        })
        .map(|v| v.0)
}

/// Whether each triangle of `m` joins distant parts of the atlas (its corners belong to different islands): some edge
/// runs through the atlas at over `BRIDGE` times the mesh's usual rate per metre. Such a triangle would draw its
/// neighbours' texels in a wedge, so it is left to the unshaded draw.
fn bridges(m: &Mesh) -> Vec<bool> {
    const BRIDGE: f64 = 8.;
    let uv2 = m.uv2.as_ref().expect("usable");
    let rates: Vec<f64> = m
        .triangles
        .iter()
        .map(|t| {
            let i = t.map(|k| k as usize);
            [(0, 1), (1, 2), (0, 2)]
                .iter()
                .map(|&(a, b)| {
                    let atlas = f64::from(uv2[i[a]][0] - uv2[i[b]][0]).hypot(f64::from(uv2[i[a]][1] - uv2[i[b]][1]));
                    atlas / length(sub(m.positions[i[a]].map(f64::from), m.positions[i[b]].map(f64::from))).max(1e-3)
                })
                .fold(0., f64::max)
        })
        .collect();
    let mut sorted = rates.clone();
    sorted.sort_by(f64::total_cmp);
    let usual = sorted.get(sorted.len() / 2).copied().unwrap_or(0.);
    rates.iter().map(|&r| r > BRIDGE * usual).collect()
}

/// The triangles of `m` in connected pieces: those sharing a vertex are one island of the atlas.
fn islands(m: &Mesh, triangles: &[usize]) -> Vec<Vec<usize>> {
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut parent: Vec<usize> = (0..m.positions.len()).collect();
    for t in triangles.iter().map(|&t| m.triangles[t]) {
        let (a, b, c) =
            (find(&mut parent, t[0] as usize), find(&mut parent, t[1] as usize), find(&mut parent, t[2] as usize));
        parent[b] = a;
        parent[c] = a;
    }
    let mut pieces = BTreeMap::<usize, Vec<usize>>::new();
    for &i in triangles {
        let root = find(&mut parent, m.triangles[i][0] as usize);
        pieces.entry(root).or_default().push(i);
    }
    pieces.into_values().collect()
}

/// Repeats per atlas unit of a layer's `(u, v)` over some triangles of `m`: the medians of the metres per atlas unit and
/// per layer unit, and whether the layer's v runs along the atlas's u (a road's strips) or across it.
fn repeats(m: &Mesh, triangles: &[usize]) -> Repeats {
    let (mut atlas_u, mut atlas_v, mut u, mut v, mut along, mut across) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), 0., 0.);
    let uv2 = m.uv2.as_ref().expect("usable");
    for t in triangles.iter().map(|&t| m.triangles[t]) {
        let i = t.map(|k| k as usize);
        let p = i.map(|k| m.positions[k].map(f64::from));
        let area = length(cross(sub(p[1], p[0]), sub(p[2], p[0]))) / 2.;
        let (Some(layer), Some(lightmap)) = (derivatives(p, i.map(|k| m.uv[k])), derivatives(p, i.map(|k| uv2[k])))
        else {
            continue;
        };
        let (lu, lv) = (length(lightmap.0), length(lightmap.1));
        if area < 1e-6 || lu < 1e-9 || lv < 1e-9 {
            continue;
        }
        atlas_u.push((lu.ln(), area));
        atlas_v.push((lv.ln(), area));
        u.push((length(layer.0).ln(), area));
        v.push((length(layer.1).ln(), area));
        let cos = |a: V3, b: V3| dot(a, b).abs() / (length(a) * length(b)).max(1e-12);
        if cos(layer.1, lightmap.0) > 0.7 {
            along += area;
        } else if cos(layer.1, lightmap.1) > 0.7 {
            across += area;
        }
    }
    let median = |values: Vec<(f64, f64)>| weighted_median(values).map_or(1., f64::exp);
    let (au, av, u, v) = (median(atlas_u), median(atlas_v), median(u), median(v));
    // The atlas is transposed: its u is the mesh's y, its v the x. A layer whose v runs along the atlas's u keeps its
    // axes; one whose v runs across it turns a quarter.
    let [x, y] = if along >= across { [av / u, au / v] } else { [av / v, au / u] };
    Repeats {
        atlas: [x.max(1e-3) as f32, y.max(1e-3) as f32],
        world: [(1. / u) as f32, (1. / v) as f32],
        aligned: (along / (along + across).max(1e-9)) as f32,
        stretch: (au / av).max(av / au) as f32,
    }
}

/// The repeats of a layer over some triangles.
struct Repeats {
    /// Per atlas unit, on the transposed atlas's axes.
    atlas: [f32; 2],
    /// Per metre, on the layer's own axes.
    world: [f32; 2],
    /// The share (of the triangles whose layer runs along or across the atlas) where it runs along.
    aligned: f32,
    /// How much longer the atlas runs per metre along one axis than the other (1 where it keeps shapes).
    stretch: f32,
}

impl Repeats {
    /// Whether the layers are better read at the world's (x, z): the island's layer axes lie at no one angle to the
    /// atlas's, or the atlas shears it, so a pattern's direction and size would change across the island.
    fn world(&self) -> bool {
        !(self.aligned >= ALIGNED || self.aligned <= 1. - ALIGNED) || self.stretch > STRETCH
    }
}

/// A repeat's class: islands within about 20% of each other draw with one material.
fn class(repeat: [f32; 2]) -> [i32; 2] {
    repeat.map(|r| (r.max(1e-3).ln() / 0.4).round() as i32)
}

/// The repeat a class stands for.
fn of_class(class: [i32; 2]) -> [f32; 2] {
    class.map(|c| (c as f32 * 0.4).exp())
}

/// The layers of a terrain material, `None` unless every input it needs is real.
struct Plan {
    layers: Vec<String>,
    lightmap: String,
    colour: Option<String>,
    magic: f32,
}

fn plan(m: &Material, node: &str) -> Option<Plan> {
    if !m.shader.starts_with("terrain_") || material::is_vista(m) || node.starts_with("DECAL") {
        return None;
    }
    let layers: Vec<String> = LAYERS.iter().filter_map(|l| m.textures.get(*l).cloned()).collect();
    if layers.is_empty() {
        return None;
    }
    let colour = m.textures.get("TColourMap").cloned();
    // The diffuse holds the colour map as a multiplier of the layers' sum, halved (see `diffuse`).
    let magic = if colour.is_some() { 2. } else { 1. };
    Some(Plan { layers, lightmap: m.textures.get("TAmbientOcclusion")?.clone(), colour, magic })
}

/// The image of a library texture as RGBA8, `None` for a placeholder or a missing one.
fn image(library: &mut Library, reference: &str) -> Option<(usize, usize, Vec<u8>)> {
    let t = library.get(reference).filter(|t| !t.is_placeholder())?;
    Some((t.width as usize, t.height as usize, t.rgba()))
}

/// The macro colour times the lightmap in the atlas's transposed axes, at the larger of their sizes (and whether there is a colour map). A lightmap's colour
/// is its own sky tint and sun; the renderer lights the ground, so it is divided by its bright texels' mean (the
/// lightmap's white) and only its darkening is kept.
fn diffuse(
    lightmap: Option<(usize, usize, Vec<u8>)>,
    colour: Option<(usize, usize, Vec<u8>)>,
) -> (usize, Vec<f32>, bool) {
    let size = [&lightmap, &colour].iter().filter_map(|i| i.as_ref().map(|i| i.0.max(i.1))).max().unwrap_or(1);
    let white = lightmap.as_ref().map_or([1.; 3], |(_, _, px)| {
        let (mut sum, mut n) = ([0f64; 3], 0.);
        for p in px.chunks_exact(4) {
            if p[..3].iter().map(|&c| u32::from(c)).sum::<u32>() > 3 * 150 {
                (0..3).for_each(|k| sum[k] += f64::from(p[k]));
                n += 1.;
            }
        }
        if n > 0. {
            std::array::from_fn(|k| (sum[k] / n / 255.).max(0.05) as f32)
        } else {
            [1.; 3]
        }
    });
    let mut out = vec![1f32; size * size * 3];
    for (image, tint) in [(&lightmap, true), (&colour, false)] {
        let Some((w, h, px)) = image else { continue };
        for y in 0..size {
            for x in 0..size {
                // The output's (x, y) is the source's (y, x).
                let (sx, sy) = (y * w / size, x * h / size);
                let p = &px[(sy * w + sx) * 4..];
                for k in 0..3 {
                    let c = f32::from(p[k]) / 255.;
                    out[(y * size + x) * 3 + k] *= if tint { 0.5 + 0.5 * (c / white[k]).min(1.) } else { c };
                }
            }
        }
    }
    (size, out, colour.is_some())
}

/// A section's rasterised weights.
struct Atlas {
    /// RGBA8, `MASK` square.
    mask: Vec<u8>,
    covered: Vec<bool>,
}

impl Atlas {
    fn new() -> Atlas {
        Atlas { mask: vec![0; MASK * MASK * 4], covered: vec![false; MASK * MASK] }
    }

    /// The triangle's texels, `weights` the weight of each corner's layers as mask channels.
    fn triangle(&mut self, uv: [[f32; 2]; 3], weights: [[u8; 4]; 3]) {
        // The atlas is transposed: x is v, y is u.
        let p = uv.map(|c| [f64::from(c[1]) * MASK as f64, f64::from(c[0]) * MASK as f64]);
        let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
        if area.abs() < 1e-9 {
            return;
        }
        let range = |k: usize| {
            let lo = p.iter().map(|c| c[k]).fold(f64::MAX, f64::min).floor().max(0.) as usize;
            let hi = p.iter().map(|c| c[k]).fold(f64::MIN, f64::max).ceil().min(MASK as f64 - 1.) as usize;
            lo..=hi
        };
        for y in range(1) {
            for x in range(0) {
                let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
                let edge =
                    |a: [f64; 2], b: [f64; 2]| ((b[0] - a[0]) * (cy - a[1]) - (b[1] - a[1]) * (cx - a[0])) / area;
                let w = [edge(p[1], p[2]), edge(p[2], p[0]), edge(p[0], p[1])];
                // Slightly past the edge: neighbours' texels meet without a gap.
                if w.iter().any(|&w| w < -0.02) {
                    continue;
                }
                let at = y * MASK + x;
                for (c, texel) in self.mask[at * 4..at * 4 + 4].iter_mut().enumerate() {
                    let v: f64 = (0..3).map(|k| w[k] * f64::from(weights[k][c])).sum();
                    *texel = v.round().clamp(0., 255.) as u8;
                }
                self.covered[at] = true;
            }
        }
    }

    /// Fills `GROW` texels around the covered ones with their neighbours' values.
    fn grow(&mut self) {
        for _ in 0..GROW {
            let (mask, covered) = (self.mask.clone(), self.covered.clone());
            for y in 0..MASK {
                for x in 0..MASK {
                    let at = y * MASK + x;
                    if covered[at] {
                        continue;
                    }
                    let near = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)];
                    if let Some(&(nx, ny)) =
                        near.iter().find(|&&(nx, ny)| nx < MASK && ny < MASK && covered[ny * MASK + nx])
                    {
                        let from = (ny * MASK + nx) * 4;
                        self.mask[at * 4..at * 4 + 4].copy_from_slice(&mask[from..from + 4]);
                        self.covered[at] = true;
                    }
                }
            }
        }
    }
}

/// The diffuse texture of an atlas: the lightmap's and macro colour's image, grown past the atlas's coverage by `GROW`
/// of its own texels, empty ground the mean of what is covered.
fn finish_diffuse((size, mut image, tinted): (usize, Vec<f32>, bool), atlas: &Atlas) -> Vec<u8> {
    let step = MASK / size;
    let mut covered: Vec<bool> = (0..size * size)
        .map(|at| {
            let (x, y) = (at % size, at / size);
            (0..step).any(|j| (0..step).any(|i| atlas.covered[(y * step + j) * MASK + x * step + i]))
        })
        .collect();
    let (mut sum, mut n) = ([0f64; 3], 0.);
    for (at, _) in covered.iter().enumerate().filter(|(_, c)| **c) {
        (0..3).for_each(|k| sum[k] += f64::from(image[at * 3 + k]));
        n += 1.;
    }
    let mean: [f32; 3] = std::array::from_fn(|k| if n > 0. { (sum[k] / n) as f32 } else { 1. });
    if tinted {
        // The macro colour is a tint about the ground's own colour, as a whole a cast (lavender on Greece's sections):
        // the covered texels' mean is the renderer's neutral, and the stored value half the multiplier (`magic` doubles it).
        for p in image.chunks_exact_mut(3) {
            (0..3).for_each(|k| p[k] = p[k] / mean[k].max(0.02) * 0.5);
            // Its hue is half taken: a map's tints (pink on one section's roads, blue on another's rock) go beyond
            // the ground's own colour, its light and dark are kept.
            let grey = 0.3 * p[0] + 0.59 * p[1] + 0.11 * p[2];
            (0..3).for_each(|k| p[k] = (grey + (p[k] - grey) * TINT).clamp(0., 1.));
        }
    }
    let mean: [f32; 3] = if tinted { [0.5; 3] } else { mean };
    for _ in 0..GROW {
        let (before, was) = (image.clone(), covered.clone());
        for y in 0..size {
            for x in 0..size {
                let at = y * size + x;
                if was[at] {
                    continue;
                }
                let near = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)];
                if let Some(&(nx, ny)) = near.iter().find(|&&(nx, ny)| nx < size && ny < size && was[ny * size + nx]) {
                    for k in 0..3 {
                        image[at * 3 + k] = before[(ny * size + nx) * 3 + k];
                    }
                    covered[at] = true;
                }
            }
        }
    }
    let mut rgba = Vec::with_capacity(size * size * 4);
    for at in 0..size * size {
        for k in 0..3 {
            let c = if covered[at] { image[at * 3 + k] } else { mean[k] };
            rgba.push((c * 255.).round().clamp(0., 255.) as u8);
        }
        rgba.push(255);
    }
    textures::encode(false, size as u32, size as u32, rgba)
}

impl Shader {
    fn detail(&mut self, library: &mut Library, reference: &str) -> Option<Arc<Vec<u8>>> {
        self.details
            .entry(reference.to_lowercase())
            .or_insert_with(|| library.get(reference).filter(|t| !t.is_placeholder()).map(|t| Arc::new(t.dds())))
            .clone()
    }

    /// Shades the terrain meshes whose material the renderer can sum per pixel: their uv becomes the atlas, and each
    /// piece of the atlas whose ground the layers tile at another repeat becomes a mesh of its own material (the
    /// source's id and the repeat's class). The others are left as they are.
    pub fn shade(&mut self, meshes: Vec<Mesh>, sources: &BTreeMap<String, Material>, library: &mut Library) -> Shaded {
        // Sections: the meshes of one lightmap and macro colour, with their plans.
        let mut sections = BTreeMap::<(String, String), Vec<usize>>::new();
        let mut plans = BTreeMap::<String, Option<Plan>>::new();
        for (i, m) in meshes.iter().enumerate() {
            let Some(source) = sources.get(&m.material) else { continue };
            let plan = plans.entry(m.material.clone()).or_insert_with(|| plan(source, &m.node));
            let usable = m.uv2.is_some() && !m.uv.is_empty() && m.uv.len() == m.positions.len();
            if let (Some(plan), true) = (plan, usable) {
                let key = (plan.lightmap.to_lowercase(), plan.colour.clone().unwrap_or_default().to_lowercase());
                sections.entry(key).or_default().push(i);
            }
        }
        let mut shaded = Shaded { meshes: Vec::new(), layers: BTreeMap::new(), source: BTreeMap::new() };
        let mut done = vec![false; meshes.len()];
        for members in sections.values() {
            let first = plans[&meshes[members[0]].material].as_ref().expect("planned");
            let (lightmap, colour) =
                (image(library, &first.lightmap), first.colour.as_ref().and_then(|c| image(library, c)));
            // The materials of the section whose layer textures are all real.
            let mut details = BTreeMap::<String, [Arc<Vec<u8>>; 4]>::new();
            for &i in members {
                let id = &meshes[i].material;
                if details.contains_key(id) {
                    continue;
                }
                let plan = plans[id].as_ref().expect("planned");
                let files: Option<Vec<_>> = plan.layers.iter().map(|l| self.detail(library, l)).collect();
                if let Some(files) = files {
                    let mut by_channel: [Arc<Vec<u8>>; 4] = std::array::from_fn(|_| files[0].clone());
                    for (layer, file) in files.iter().enumerate() {
                        by_channel[CHANNEL[layer]] = file.clone();
                    }
                    details.insert(id.clone(), by_channel);
                }
            }
            let mut atlas = Atlas::new();
            for &i in members.iter().filter(|&&i| details.contains_key(&meshes[i].material)) {
                let m = &meshes[i];
                let n = plans[&m.material].as_ref().map_or(1, |p| p.layers.len());
                let uv2 = m.uv2.as_ref().expect("usable");
                let weight = |k: usize| {
                    // The first layer takes what the others leave; R, G, B are the second, third and fourth.
                    let c = m.colour.as_ref().map_or([0; 4], |c| c[k]);
                    let mut w = [0u8; 4];
                    let mut rest = 255i32;
                    for layer in 1..n {
                        w[CHANNEL[layer]] = c[layer - 1].min(rest as u8);
                        rest -= i32::from(w[CHANNEL[layer]]);
                    }
                    w[CHANNEL[0]] = rest.max(0) as u8;
                    w
                };
                for (t, _) in m.triangles.iter().zip(bridges(m)).filter(|(_, bridge)| !bridge) {
                    let i = t.map(|k| k as usize);
                    atlas.triangle(i.map(|k| uv2[k]), i.map(weight));
                }
                done[i] = true;
            }
            atlas.grow();
            let diffuse = Arc::new(finish_diffuse(diffuse(lightmap, colour), &atlas));
            let mask = Arc::new(textures::encode(true, MASK as u32, MASK as u32, std::mem::take(&mut atlas.mask)));
            // Each mesh split by the class of its islands' repeat.
            for &i in members.iter().filter(|&&i| details.contains_key(&meshes[i].material)) {
                let m = &meshes[i];
                let plan = plans[&m.material].as_ref().expect("planned");
                let mut by_class = BTreeMap::<(bool, [i32; 2]), Vec<[u32; 3]>>::new();
                let bridging = bridges(m);
                let (rest, good): (Vec<usize>, Vec<usize>) = (0..m.triangles.len()).partition(|&t| bridging[t]);
                if !rest.is_empty() {
                    shaded.meshes.push(Mesh { triangles: rest.iter().map(|&t| m.triangles[t]).collect(), ..m.clone() });
                }
                for piece in islands(m, &good) {
                    let r = repeats(m, &piece);
                    let world = r.world();
                    let class = (world, class(if world { r.world } else { r.atlas }));
                    by_class.entry(class).or_default().extend(piece.iter().map(|&t| m.triangles[t]));
                }
                for ((world, class), triangles) in by_class {
                    let id = format!("{}\u{1f}{}{},{}", m.material, if world { "w" } else { "" }, class[0], class[1]);
                    if !shaded.layers.contains_key(&id) {
                        let repeat = of_class(class);
                        let mut scale = [repeat; 4];
                        for layer in 0..4 {
                            scale[CHANNEL[layer.min(plan.layers.len() - 1)]] = repeat;
                        }
                        shaded.layers.insert(
                            id.clone(),
                            Layers {
                                world,
                                diffuse: diffuse.clone(),
                                mask: mask.clone(),
                                details: details[&m.material].clone(),
                                scale,
                                magic: plan.magic,
                            },
                        );
                        shaded.source.insert(id.clone(), m.material.clone());
                    }
                    // The atlas is the uv now, transposed as the images are.
                    let uv2 = m.uv2.as_ref().expect("usable");
                    shaded.meshes.push(Mesh {
                        material: id,
                        uv: uv2.iter().map(|c| [c[1], c[0]]).collect(),
                        triangles,
                        ..m.clone()
                    });
                }
            }
        }
        shaded.meshes.extend(meshes.into_iter().zip(done).filter(|(_, d)| !d).map(|(m, _)| m));
        shaded
    }
}

/// The terrain after [`Shader::shade`].
pub struct Shaded {
    pub meshes: Vec<Mesh>,
    /// The layers of each shaded material, by its id here.
    pub layers: BTreeMap<String, Layers>,
    /// The source material's id of each shaded one.
    pub source: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivatives_of_a_stretched_triangle() {
        // 3 m per u, 2 m per v.
        let p = [[0., 0., 0.], [3., 0., 0.], [0., 2., 0.]];
        let (du, dv) = derivatives(p, [[0., 0.], [1., 0.], [0., 1.]]).unwrap();
        assert_eq!((du, dv), ([3., 0., 0.], [0., 2., 0.]));
        assert!(derivatives(p, [[0., 0.], [1., 1.], [2., 2.]]).is_none());
    }

    #[test]
    fn a_triangle_rasterises_its_weights_transposed() {
        let mut atlas = Atlas::new();
        // A triangle over the atlas's lower u half, all weight on the first layer.
        atlas.triangle([[0., 0.], [0.5, 0.], [0., 0.5]], [[255, 0, 0, 0]; 3]);
        let at = |x: usize, y: usize| (y * MASK + x, atlas.covered[y * MASK + x]);
        // u 0.1, v 0.1 is x 204, y 204: inside; u 0.9 is y 1843: outside.
        assert!(at(204, 204).1 && !at(204, 1843).1);
        assert_eq!(atlas.mask[at(204, 204).0 * 4], 255);
        atlas.grow();
        assert!(atlas.covered[(MASK / 2 + 1) * MASK + 1]);
    }

    /// Two 10 m quads side by side, the second's atlas far from the first's, and a triangle joining them.
    fn islands_mesh() -> Mesh {
        Mesh {
            positions: vec![
                [0., 0., 0.],
                [10., 0., 0.],
                [0., 0., 10.],
                [10., 0., 10.],
                [20., 0., 0.],
                [30., 0., 0.],
                [20., 0., 10.],
                [30., 0., 10.],
            ],
            uv: vec![[0.; 2]; 8],
            uv2: Some(vec![
                [0., 0.],
                [0.01, 0.],
                [0., 0.01],
                [0.01, 0.01],
                [0.6, 0.6],
                [0.61, 0.6],
                [0.6, 0.61],
                [0.61, 0.61],
            ]),
            triangles: vec![[0, 1, 2], [1, 3, 2], [4, 5, 6], [5, 7, 6], [1, 4, 3], [4, 7, 3], [3, 4, 7]],
            ..Mesh::default()
        }
    }

    #[test]
    fn a_triangle_between_islands_bridges_them() {
        let m = islands_mesh();
        let bridging = bridges(&m);
        assert_eq!(bridging, [false, false, false, false, true, true, true]);
        let good: Vec<usize> = (0..4).collect();
        let pieces = islands(&m, &good);
        assert_eq!(pieces, vec![vec![0, 1], vec![2, 3]]);
    }

    #[test]
    fn repeats_classes_are_stable_within_a_fifth() {
        assert_eq!(class([30., 40.]), class([31., 41.]));
        assert!((of_class(class([30., 8.]))[0] / 30. - 1.).abs() < 0.25);
    }

    #[test]
    fn repeats_follow_the_layer_and_the_atlas() {
        // A 24 m square, the atlas 240 m per unit, the layer 8 m per unit with its v along the atlas's u.
        let m = Mesh {
            positions: vec![[0., 0., 0.], [24., 0., 0.], [0., 0., 24.], [24., 0., 24.]],
            uv: vec![[0., 0.], [0., 3.], [3., 0.], [3., 3.]],
            uv2: Some(vec![[0., 0.], [0.1, 0.], [0., 0.1], [0.1, 0.1]]),
            triangles: vec![[0, 1, 2], [1, 3, 2]],
            ..Mesh::default()
        };
        let r = repeats(&m, &[0, 1]).atlas;
        assert!((r[0] - 30.).abs() < 0.01 && (r[1] - 30.).abs() < 0.01, "{r:?}");
    }
}
