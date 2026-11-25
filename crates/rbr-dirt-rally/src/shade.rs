//! The terrain drawn as the game blends it, by ReRBR's per-pixel layers (`ReRbrLayer*` with `ReRbrLayerUv`).
//!
//! DiRT Rally's terrain shaders sum up to four diffuse layers by the vertex `Color` (R, G, B the second to fourth layers'
//! weights, the first the rest), then multiply a macro colour map (`TColourMap`, times `ColourMapScale`) and a baked
//! lightmap (`TAmbientOcclusion`). Both maps lie on the second `ST` stream: one unique unwrap (an atlas) of the area's
//! ground, shared by every material of a section; the layers lie on the first stream's `xy`, which tile. The renderer
//! reads the same two coordinates, so here:
//!
//! - the diffuse is the macro colour times the lightmap, one BC7 texture per section (DXT1's 5:6:5 endpoints turn a
//!   smooth, nearly grey diffuse into magenta and green blotches), read at the atlas (UV set 0);
//! - each layer's own texture is a detail at the layer coordinates (UV set 1) times the layer's `Map<N>UVScaleAndOffset`
//!   relative to the first layer's, which the coordinates already carry;
//! - the weights are the vertex colour, per pixel (UV sets 2 and 3), as the game reads them.
//!
//! A material is shaded as a whole: its meshes keep their coordinates, [`Layers`] only says how to draw them.
use crate::material::{self, Material};
use crate::mesh::Mesh;
use crate::texture::Library;
use rbr_btb_writer::textures;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Texels the rasterised coverage is grown past its triangles: the filter reads across an island's border.
const GROW: usize = 3;
/// The share of the colour map's hue the diffuse keeps.
const TINT: f32 = 0.3;
/// The layers in the order the shader sums them.
const LAYERS: [&str; 4] = ["TDiffuseSpecMap1", "TDiffuseSpecMap2", "TDiffuseSpecMap3", "TDiffuseSpecMap4"];

/// What the renderer needs to draw a material per pixel: DDS files and numbers, shared by `Arc`.
#[derive(Clone)]
pub struct Layers {
    pub diffuse: Arc<Vec<u8>>,
    /// The layers' textures in order; the missing ones repeat the first (their weight is 0).
    pub details: [Arc<Vec<u8>>; 4],
    /// How many layers the material has (the weights past them are 0).
    pub count: usize,
    /// Each detail's `(u, v)` repeats per layer unit (the first layer's own repeat is the coordinates').
    pub scale: [[f32; 2]; 4],
    /// The whole number of layer units a mesh's coordinates may be shifted by without moving any layer's tiling.
    pub period: f32,
    pub magic: f32,
    /// `DetailBlendScale` of a road whose second and third details carry their blend maps in the alpha (`ReRbrLayerBlend`).
    pub blend: Option<f32>,
    /// The normal map of the layer that weighs most over the material's first mesh, read at that layer's repeat
    /// (`None` where the layer has no real one).
    pub normal: Option<(Arc<Vec<u8>>, [f32; 2])>,
}

/// The shading of a file's terrain: the layers' textures shared across its materials.
#[derive(Default)]
pub struct Shader {
    details: HashMap<String, Option<Arc<Vec<u8>>>>,
    normals: HashMap<String, Option<Arc<Vec<u8>>>>,
    blended: HashMap<(String, String), Option<Arc<Vec<u8>>>>,
}

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: V3) -> f64 {
    dot(a, a).sqrt()
}

/// Whether each triangle of `m` joins distant parts of the atlas (its corners belong to different islands): some edge
/// runs through the atlas at over `BRIDGE` times the mesh's usual rate per metre. Such a triangle would draw its
/// neighbours' texels in a wedge, so it is left to the unshaded draw.
pub fn bridges(m: &Mesh) -> Vec<bool> {
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

/// The layer (of `n`) with the most vertex weight over `triangles`: R, G, B weigh the second to fourth, the first the rest.
fn dominant(m: &Mesh, triangles: &[[u32; 3]], n: usize) -> usize {
    let mut sum = [0u64; 4];
    for &k in triangles.iter().flatten() {
        let c = m.colour.as_ref().map_or([0; 4], |c| c[k as usize]);
        let others: u64 = (1..n).map(|l| u64::from(c[l - 1])).sum();
        sum[0] += 255u64.saturating_sub(others);
        (1..n).for_each(|l| sum[l] += u64::from(c[l - 1]));
    }
    (0..n).max_by_key(|&l| sum[l]).unwrap_or(0)
}

/// The vertex weights of a shaded material's corner, R, G, B as bytes: the layers it does not have weigh nothing.
pub fn weights(colour: [u8; 4], layers: usize) -> [u8; 3] {
    std::array::from_fn(|k| if k + 1 < layers { colour[k] } else { 0 })
}

/// The smallest whole number of layer units (up to 64) that is also a whole number of units of every layer, whose
/// repeats are `relative` times the first's: a mesh's coordinates shifted by it tile as before, and stay small for `f32`.
fn period(relative: &[[f32; 2]]) -> f32 {
    let whole = |k: f32| relative.iter().flatten().all(|r| (r * k - (r * k).round()).abs() < 1e-3);
    (1..=64).map(|k| k as f32).find(|&k| whole(k)).unwrap_or(0.)
}

/// The layers of a terrain material, `None` unless every input it needs is real.
struct Plan {
    layers: Vec<String>,
    /// Each layer's `TNormalMap<N>`.
    normals: Vec<Option<String>>,
    /// Each layer's `Map<N>UVScaleAndOffset` scale relative to the first layer's, which the mesh's uv already carries.
    relative: Vec<[f32; 2]>,
    lightmap: String,
    colour: Option<String>,
    magic: f32,
    /// The road blend: `DetailBlendScale` and the layer 2 and 3 blend maps (`TBlendMap2`, `TBlendMap3`).
    blend: Option<(f32, [String; 2])>,
}

fn plan(m: &Material, node: &str) -> Option<Plan> {
    if !m.shader.starts_with("terrain_") || material::is_vista(m) || node.starts_with("DECAL") {
        return None;
    }
    let present: Vec<usize> = (0..LAYERS.len()).filter(|&l| m.textures.contains_key(LAYERS[l])).collect();
    let layers: Vec<String> = present.iter().map(|&l| m.textures[LAYERS[l]].clone()).collect();
    if layers.is_empty() {
        return None;
    }
    let scale = |l: usize| match m.constants.get(&format!("Map{}UVScaleAndOffset", l + 1)).map(Vec::as_slice) {
        Some(&[u, v, ..]) if u != 0. && v != 0. => [u, v],
        _ => [1., 1.],
    };
    let normals = present.iter().map(|&l| m.textures.get(&format!("TNormalMap{}", l + 1)).cloned()).collect();
    let first = scale(present[0]);
    let relative = present.iter().map(|&l| scale(l)).map(|s| [s[0] / first[0], s[1] / first[1]]).collect();
    let colour = m.textures.get("TColourMap").cloned();
    // The diffuse holds the colour map as a multiplier of the layers' sum, halved (see `diffuse`).
    let magic = if colour.is_some() { 2. } else { 1. };
    // The road shader's blend (read from its DXBC): the height maps lie at their layer's coordinates, so they ride in the
    // layers' alpha unless a map is offset. Other terrain shaders keep the plain weights.
    let offset = |l: usize| {
        m.constants.get(&format!("Map{l}UVScaleAndOffset")).is_some_and(|c| c.get(2..4).is_some_and(|o| o != [0., 0.]))
    };
    let blend = (m.shader == "terrain_road.fx" && present.starts_with(&[0, 1, 2]) && !offset(2) && !offset(3))
        .then(|| {
            let scale = *m.constants.get("DetailBlendScale")?.first()?;
            Some((scale, [m.textures.get("TBlendMap2")?.clone(), m.textures.get("TBlendMap3")?.clone()]))
        })
        .flatten();
    Some(Plan {
        layers,
        normals,
        relative,
        lightmap: m.textures.get("TAmbientOcclusion")?.clone(),
        colour,
        magic,
        blend,
    })
}

/// The image of a library texture as RGBA8, `None` for a placeholder or a missing one.
fn image(library: &mut Library, reference: &str) -> Option<(usize, usize, Vec<u8>)> {
    let t = library.get(reference).filter(|t| !t.is_placeholder())?;
    Some((t.width as usize, t.height as usize, t.rgba()))
}

/// RGBA8 `pixels` of `from` texels at size `to`: bilinear, wrapped at the edges (the textures tile).
fn resized(pixels: &[u8], from: (usize, usize), to: (usize, usize)) -> Vec<u8> {
    if from == to {
        return pixels.to_vec();
    }
    let at = |x: usize, y: usize, k: usize| f32::from(pixels[((y % from.1) * from.0 + x % from.0) * 4 + k]);
    let mut out = Vec::with_capacity(to.0 * to.1 * 4);
    for y in 0..to.1 {
        let sy = ((y as f32 + 0.5) * from.1 as f32 / to.1 as f32 - 0.5).rem_euclid(from.1 as f32);
        let (y0, fy) = (sy as usize, sy.fract());
        for x in 0..to.0 {
            let sx = ((x as f32 + 0.5) * from.0 as f32 / to.0 as f32 - 0.5).rem_euclid(from.0 as f32);
            let (x0, fx) = (sx as usize, sx.fract());
            for k in 0..4 {
                let top = at(x0, y0, k) * (1. - fx) + at(x0 + 1, y0, k) * fx;
                let bottom = at(x0, y0 + 1, k) * (1. - fx) + at(x0 + 1, y0 + 1, k) * fx;
                out.push((top * (1. - fy) + bottom * fy).round() as u8);
            }
        }
    }
    out
}

/// The macro colour times the lightmap at the larger of their sizes (and whether there is a colour map). A lightmap's colour
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
                let (sx, sy) = (x * w / size, y * h / size);
                let p = &px[(sy * w + sx) * 4..];
                for k in 0..3 {
                    let c = f32::from(p[k]) / 255.;
                    out[(y * size + x) * 3 + k] *= if tint { 0.2 + 0.8 * (c / white[k]).min(1.) } else { c };
                }
            }
        }
    }
    (size, out, colour.is_some())
}

/// The share of a section's atlas its triangles cover, `size` texels square.
struct Coverage {
    size: usize,
    covered: Vec<bool>,
}

impl Coverage {
    fn new(size: usize) -> Coverage {
        Coverage { size, covered: vec![false; size * size] }
    }

    /// Marks the texels of a triangle of the atlas.
    fn triangle(&mut self, uv: [[f32; 2]; 3]) {
        let p = uv.map(|c| c.map(|c| f64::from(c) * self.size as f64));
        let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
        if area.abs() < 1e-9 {
            return;
        }
        let range = |k: usize| {
            let lo = p.iter().map(|c| c[k]).fold(f64::MAX, f64::min).floor().max(0.) as usize;
            let hi = p.iter().map(|c| c[k]).fold(f64::MIN, f64::max).ceil().min(self.size as f64 - 1.) as usize;
            lo..=hi
        };
        for y in range(1) {
            for x in range(0) {
                let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
                let edge =
                    |a: [f64; 2], b: [f64; 2]| ((b[0] - a[0]) * (cy - a[1]) - (b[1] - a[1]) * (cx - a[0])) / area;
                let w = [edge(p[1], p[2]), edge(p[2], p[0]), edge(p[0], p[1])];
                // Slightly past the edge: neighbours' texels meet without a gap.
                if w.iter().all(|&w| w >= -0.02) {
                    self.covered[y * self.size + x] = true;
                }
            }
        }
    }
}

/// The diffuse texture of an atlas: the lightmap's and macro colour's image, grown past the atlas's coverage by `GROW`
/// texels, empty ground the mean of what is covered.
fn finish_diffuse((size, mut image, tinted): (usize, Vec<f32>, bool), atlas: &Coverage) -> Vec<u8> {
    debug_assert_eq!(atlas.size, size);
    let mut covered = atlas.covered.clone();
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
    textures::encode_bc7(false, size as u32, size as u32, rgba)
}

impl Shader {
    fn detail(&mut self, library: &mut Library, reference: &str) -> Option<Arc<Vec<u8>>> {
        self.details
            .entry(reference.to_lowercase())
            .or_insert_with(|| library.get(reference).filter(|t| !t.is_placeholder()).map(|t| Arc::new(t.dds())))
            .clone()
    }

    /// A layer's texture as BC7 with its blend map's height (red) in the alpha, both read wrapped at the larger of their sizes.
    /// A blend map the library lacks reads 1.
    fn blended(&mut self, library: &mut Library, layer: &str, map: &str) -> Option<Arc<Vec<u8>>> {
        let key = (layer.to_lowercase(), map.to_lowercase());
        if let Some(hit) = self.blended.get(&key) {
            return hit.clone();
        }
        let made = image(library, layer).map(|(w, h, rgba)| {
            let height = library.get(map).map(|t| (t.width as usize, t.height as usize, t.rgba()));
            let size = height.as_ref().map_or((w, h), |(bw, bh, _)| (w.max(*bw), h.max(*bh)));
            let mut out = resized(&rgba, (w, h), size);
            if let Some((bw, bh, px)) = height {
                let height = resized(&px, (bw, bh), size);
                out.chunks_exact_mut(4).zip(height.chunks_exact(4)).for_each(|(o, p)| o[3] = p[0]);
            } else {
                out.chunks_exact_mut(4).for_each(|o| o[3] = 255);
            }
            Arc::new(textures::encode_bc7(true, size.0 as u32, size.1 as u32, out))
        });
        self.blended.insert(key, made.clone());
        made
    }

    fn normal(&mut self, library: &mut Library, reference: &str) -> Option<Arc<Vec<u8>>> {
        self.normals
            .entry(reference.to_lowercase())
            .or_insert_with(|| library.get(reference).filter(|t| !t.is_placeholder()).map(|t| Arc::new(t.normal_dds())))
            .clone()
    }

    /// Shades the terrain meshes whose material the renderer can sum per pixel: each such material (the source's id with
    /// `SHADED`) is drawn on its meshes' own coordinates, and the triangles that bridge the atlas's islands stay with the
    /// source material, drawn with its first layer. The others are left as they are.
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
                let files: Option<Vec<_>> = plan
                    .layers
                    .iter()
                    .enumerate()
                    .map(|(l, layer)| match &plan.blend {
                        Some((_, maps)) if l == 1 || l == 2 => self.blended(library, layer, &maps[l - 1]),
                        _ => self.detail(library, layer),
                    })
                    .collect();
                if let Some(files) = files {
                    details.insert(id.clone(), std::array::from_fn(|l| files.get(l).unwrap_or(&files[0]).clone()));
                }
            }
            let diffuse = diffuse(lightmap, colour);
            let mut atlas = Coverage::new(diffuse.0);
            for &i in members.iter().filter(|&&i| details.contains_key(&meshes[i].material)) {
                let m = &meshes[i];
                let uv2 = m.uv2.as_ref().expect("usable");
                for (t, _) in m.triangles.iter().zip(bridges(m)).filter(|(_, bridge)| !bridge) {
                    atlas.triangle(t.map(|k| uv2[k as usize]));
                }
                done[i] = true;
            }
            let diffuse = Arc::new(finish_diffuse(diffuse, &atlas));
            for &i in members.iter().filter(|&&i| details.contains_key(&meshes[i].material)) {
                let m = &meshes[i];
                let plan = plans[&m.material].as_ref().expect("planned");
                let (bridging, shaded_triangles): (Vec<_>, Vec<_>) =
                    m.triangles.iter().zip(bridges(m)).partition(|(_, bridge)| *bridge);
                let triangles = |of: Vec<(&[u32; 3], bool)>| of.into_iter().map(|(t, _)| *t).collect::<Vec<_>>();
                if !bridging.is_empty() {
                    shaded.meshes.push(Mesh { triangles: triangles(bridging), ..m.clone() });
                }
                let triangles = triangles(shaded_triangles);
                if triangles.is_empty() {
                    continue;
                }
                let id = format!("{}{SHADED}", m.material);
                if !shaded.layers.contains_key(&id) {
                    let n = plan.layers.len();
                    let scale = std::array::from_fn(|l| plan.relative.get(l).copied().unwrap_or([1., 1.]));
                    let normal = Some(dominant(m, &triangles, n))
                        .and_then(|layer| Some((self.normal(library, plan.normals[layer].as_ref()?)?, scale[layer])));
                    shaded.layers.insert(
                        id.clone(),
                        Layers {
                            normal,
                            diffuse: diffuse.clone(),
                            details: details[&m.material].clone(),
                            count: n,
                            scale,
                            period: period(&plan.relative),
                            magic: plan.magic,
                            blend: plan.blend.as_ref().map(|(scale, _)| *scale),
                        },
                    );
                    shaded.source.insert(id.clone(), m.material.clone());
                }
                shaded.meshes.push(Mesh { material: id, triangles, ..m.clone() });
            }
        }
        shaded.meshes.extend(meshes.into_iter().zip(done).filter(|(_, d)| !d).map(|(m, _)| m));
        shaded
    }
}

/// What a shaded material's id adds to its source's.
const SHADED: &str = "\u{1f}layers";

/// The terrain after [`Shader::shade`]: a shaded mesh keeps its coordinates (`uv` the layers', `uv2` the atlas) and its
/// colour (the weights), see [`Layers`].
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
    fn the_layer_with_most_weight_is_the_one_whose_normal_map_the_material_takes() {
        // Corner weights: R, G, B are the second to fourth layers; the first takes the rest.
        let m = Mesh { colour: Some(vec![[200, 0, 0, 0], [200, 0, 0, 0], [0, 90, 0, 0]]), ..Mesh::default() };
        assert_eq!(dominant(&m, &[[0, 1, 1]], 3), 1);
        assert_eq!(dominant(&m, &[[2, 2, 2]], 3), 0);
        assert_eq!(dominant(&Mesh::default(), &[[0, 0, 0]], 2), 0);
    }

    #[test]
    fn a_triangle_covers_its_texels() {
        let mut atlas = Coverage::new(2048);
        // A triangle over the atlas's lower half in u.
        atlas.triangle([[0., 0.], [0.5, 0.], [0., 0.5]]);
        let at = |x: usize, y: usize| atlas.covered[y * 2048 + x];
        // u 0.1, v 0.1 is x 204, y 204: inside; v 0.9 is y 1843: outside.
        assert!(at(204, 204) && !at(204, 1843));
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
        assert_eq!(bridges(&islands_mesh()), [false, false, false, false, true, true, true]);
    }

    #[test]
    fn weights_stop_at_the_materials_layers() {
        assert_eq!(weights([10, 20, 30, 255], 4), [10, 20, 30]);
        assert_eq!(weights([10, 20, 30, 255], 2), [10, 0, 0]);
        assert_eq!(weights([10, 20, 30, 255], 1), [0, 0, 0]);
    }

    #[test]
    fn a_period_keeps_every_layers_tiling() {
        assert_eq!(period(&[[1., 1.], [2., 2.]]), 1.);
        assert_eq!(period(&[[1., 1.], [0.5, 2.]]), 2.);
        assert_eq!(period(&[[1., 1.], [1.5, 1.]]), 2.);
        assert_eq!(period(&[[1., 1.], [0.3, 1.]]), 10.);
    }
}
