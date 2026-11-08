//! Retail trees in place of a level's (`--trees-from`, `notes/beamng.md` "Retail trees"): the trees of a stage of the
//! player's install, as the renderer's lighting finds them (`rbr_render::foliage::plants`), each kind placed where a
//! level's tree stood, nearest in height, and scattered along its tree-line cards at the retail stage's own spacing.

use std::collections::HashMap;
use std::path::Path;

/// A retail plant this tall (metres) or taller is a tree; lower ones are saplings, bushes and grass (on 44 the 3 to 5 m
/// kinds were a third of its plants, and as the lowest kind they took the level's bushes for trees).
const TREE: f32 = 5.0;
/// At most this many kinds, the most placed on the retail stage.
const KINDS: usize = 16;
/// A placed kind is scaled within this share of its height.
const SCALE: f32 = 0.2;
/// A level's shape object no wider than this (metres) holding cut-out triangles is one tree (trunk, limbs and leaf
/// cards that need not touch); wider objects are split into their connected pieces.
const OBJECT: f64 = 30.0;
/// A piece this long across (metres) or longer is a tree-line card: no tree is that wide.
const CARD: f64 = 40.0;
/// A card's corner is on its foot when another corner within this plan distance (metres) is higher by
/// [`CARD_HIGH`] or more.
const CARD_NEAR: f64 = 1.0;
const CARD_HIGH: f64 = 1.0;

/// One retail tree kind: its parts by texture, in its own frame (its foot at the origin, y up).
#[derive(Debug, Clone)]
pub struct Kind {
    pub height: f32,
    /// How many of the retail stage's trees are this kind.
    pub count: usize,
    pub parts: Vec<KindPart>,
}

#[derive(Debug, Clone)]
pub struct KindPart {
    pub texture: String,
    pub image: Vec<u8>,
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub triangles: Vec<[u32; 3]>,
}

impl Kind {
    pub fn triangles(&self) -> usize {
        self.parts.iter().map(|p| p.triangles.len()).sum()
    }
}

/// A retail stage's tree kinds, shortest first, and the spacing of its trees.
#[derive(Debug, Clone)]
pub struct Library {
    pub stage: String,
    pub kinds: Vec<Kind>,
    /// The median plan distance from a retail tree to its nearest neighbour, metres.
    pub spacing: f64,
    /// The retail stage's trees, of every kind.
    pub trees: usize,
}

/// The tree kinds of retail stage `stage` (`track-44_O`) in `maps`.
pub fn library(maps: &Path, stage: &str) -> Result<Library, String> {
    let mut loaded = rbr_render::stage::load(maps, stage, rbr_render::stage::Selection::default())
        .map_err(|e| format!("{stage}: {e:?}"))?;
    loaded.load_textures(maps).map_err(|e| format!("{stage} textures: {e:?}"))?;
    let library = of_plants(stage, rbr_render::foliage::plants(&loaded));
    if library.kinds.is_empty() {
        return Err(format!("{stage}: no trees (crowns {TREE} m or taller with stage textures)"));
    }
    Ok(library)
}

/// [`library`] from a stage's plants: its crowns [`TREE`] or taller whose textures are all stage `.dds`, grouped into
/// kinds by their textures, triangle count and height (to 0.25 m), the [`KINDS`] most placed.
pub fn of_plants(stage: &str, plants: rbr_render::foliage::Plants) -> Library {
    let images = &plants.images;
    let trees: Vec<_> = plants
        .plants
        .into_iter()
        .filter(|p| {
            p.crown && p.high[1] - p.low[1] >= TREE && p.parts.iter().all(|part| images.contains_key(&part.texture))
        })
        .collect();
    let feet: Vec<[f64; 2]> =
        trees.iter().map(|p| [f64::from(p.low[0] + p.high[0]) * 0.5, f64::from(p.low[2] + p.high[2]) * 0.5]).collect();
    let spacing = median_nearest(&feet).unwrap_or(8.0);
    let mut kinds: HashMap<(Vec<String>, usize, i32), Kind> = HashMap::new();
    for p in &trees {
        let height = p.high[1] - p.low[1];
        let mut textures: Vec<String> = p.parts.iter().map(|part| part.texture.to_lowercase()).collect();
        textures.sort();
        let triangles = p.parts.iter().map(|part| part.indices.len() / 3).sum();
        let key = (textures, triangles, (height * 4.0).round() as i32);
        kinds
            .entry(key)
            .or_insert_with(|| {
                let foot = [(p.low[0] + p.high[0]) * 0.5, p.low[1], (p.low[2] + p.high[2]) * 0.5];
                let parts = p
                    .parts
                    .iter()
                    .map(|part| KindPart {
                        texture: part.texture.clone(),
                        image: images[&part.texture].clone(),
                        positions: part.positions.iter().map(|v| [0, 1, 2].map(|k| v[k] - foot[k])).collect(),
                        uvs: part.uvs.clone(),
                        triangles: part.indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect(),
                    })
                    .collect();
                Kind { height, count: 0, parts }
            })
            .count += 1;
    }
    let mut kinds: Vec<Kind> = kinds.into_values().collect();
    kinds.sort_by(|a, b| b.count.cmp(&a.count).then(a.height.total_cmp(&b.height)));
    kinds.truncate(KINDS);
    kinds.sort_by(|a, b| a.height.total_cmp(&b.height));
    Library { stage: stage.to_owned(), kinds, spacing, trees: trees.len() }
}

/// The median distance from each point to its nearest other, `None` for fewer than two.
fn median_nearest(points: &[[f64; 2]]) -> Option<f64> {
    if points.len() < 2 {
        return None;
    }
    let cell = 10.0;
    let key = |p: [f64; 2]| ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64);
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, &p) in points.iter().enumerate() {
        grid.entry(key(p)).or_default().push(i);
    }
    let mut nearest: Vec<f64> = Vec::new();
    for (i, &p) in points.iter().enumerate() {
        let (cx, cy) = key(p);
        // Rings of cells out until one holds a neighbour, then one more ring (a nearer one can sit just past it).
        let mut best = f64::MAX;
        for r in 0..50i64 {
            for dx in -r..=r {
                for dy in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    for &j in grid.get(&(cx + dx, cy + dy)).into_iter().flatten().filter(|&&j| j != i) {
                        best = best.min(((points[j][0] - p[0]).powi(2) + (points[j][1] - p[1]).powi(2)).sqrt());
                    }
                }
            }
            if best <= r as f64 * cell {
                break;
            }
        }
        if best < f64::MAX {
            nearest.push(best);
        }
    }
    nearest.sort_by(f64::total_cmp);
    nearest.get(nearest.len() / 2).copied()
}

impl Library {
    /// The shortest kind's height scaled down by [`SCALE`]: lower pieces are not trees (bushes).
    pub fn lowest(&self) -> f64 {
        f64::from(self.kinds.first().map_or(f32::MAX, |k| k.height) * (1.0 - SCALE))
    }

    /// The kind and its placement (a row-major Direct3D matrix, `rbr_btb::scene::place_point`) for a tree `height`
    /// tall standing at `foot` (stage frame, y up): of the kinds within [`SCALE`] of the height one by `seed`, else
    /// the nearest, scaled to the height within [`SCALE`], turned by `seed`.
    pub fn place(&self, foot: [f32; 3], height: f64, seed: u64) -> (usize, [f32; 16]) {
        let height = height as f32;
        let fits: Vec<usize> =
            (0..self.kinds.len()).filter(|&k| (height / self.kinds[k].height - 1.0).abs() <= SCALE).collect();
        let kind = if fits.is_empty() {
            (0..self.kinds.len())
                .min_by(|&a, &b| {
                    (self.kinds[a].height - height).abs().total_cmp(&(self.kinds[b].height - height).abs())
                })
                .unwrap_or(0)
        } else {
            fits[(seed % fits.len() as u64) as usize]
        };
        let s = (height / self.kinds[kind].height).clamp(1.0 - SCALE, 1.0 + SCALE);
        let yaw = (seed >> 16) as f32 / (1u64 << 48) as f32 * std::f32::consts::TAU;
        let (sin, cos) = yaw.sin_cos();
        let matrix = [
            s * cos,
            0.0,
            -s * sin,
            0.0, //
            0.0,
            s,
            0.0,
            0.0, //
            s * sin,
            0.0,
            s * cos,
            0.0, //
            foot[0],
            foot[1],
            foot[2],
            1.0,
        ];
        (kind, matrix)
    }
}

/// A level's tree: its foot (BeamNG frame, z up) and height, metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub foot: [f64; 3],
    pub height: f64,
}

/// What [`split`] makes of a shape object's triangles.
#[derive(Debug, Default)]
pub struct Split {
    /// Where retail trees go.
    pub spots: Vec<Spot>,
    /// The triangles that stay as they are (by index).
    pub kept: Vec<usize>,
    /// Trees from tree-line cards among [`Split::spots`], and the cards.
    pub from_cards: usize,
    pub cards: usize,
}

/// A level's shape object's `triangles` (BeamNG frame, z up), `cut` the ones of see-through materials, as trees to
/// replace: an object no wider than [`OBJECT`] holding cut triangles is one tree, a wider one its connected pieces
/// that hold cut triangles. A piece [`CARD`] long or longer is a tree-line card, its foot scattered with trees
/// `spacing` apart ([`card_spots`]); a piece lower than `lowest` stays (bushes), as do pieces without cut triangles.
pub fn split(triangles: &[[[f64; 3]; 3]], cut: &[bool], spacing: f64, lowest: f64) -> Split {
    let mut out = Split::default();
    let pieces: Vec<Vec<usize>> = if across(triangles.iter().flatten()) <= OBJECT {
        vec![(0..triangles.len()).collect()]
    } else {
        connected(triangles)
    };
    for piece in pieces {
        if !piece.iter().any(|&t| cut[t]) {
            out.kept.extend(piece);
            continue;
        }
        let corners = || piece.iter().flat_map(|&t| triangles[t].iter());
        if across(corners()) >= CARD {
            let spots = card_spots(&piece.iter().map(|&t| triangles[t]).collect::<Vec<_>>(), spacing);
            out.from_cards += spots.len();
            out.cards += 1;
            out.spots.extend(spots);
            continue;
        }
        let (low, high) = bounds(corners());
        let height = high[2] - low[2];
        if height < lowest {
            out.kept.extend(piece);
            continue;
        }
        out.spots.push(Spot { foot: [(low[0] + high[0]) * 0.5, (low[1] + high[1]) * 0.5, low[2]], height });
    }
    out.kept.sort_unstable();
    out
}

fn bounds<'a>(points: impl Iterator<Item = &'a [f64; 3]>) -> ([f64; 3], [f64; 3]) {
    points.fold(([f64::MAX; 3], [f64::MIN; 3]), |(lo, hi), p| {
        ([0, 1, 2].map(|k| lo[k].min(p[k])), [0, 1, 2].map(|k| hi[k].max(p[k])))
    })
}

/// The longer side of the plan box of `points`.
fn across<'a>(points: impl Iterator<Item = &'a [f64; 3]>) -> f64 {
    let (lo, hi) = bounds(points);
    (hi[0] - lo[0]).max(hi[1] - lo[1]).max(0.0)
}

/// Corners within 1 cm are one.
fn weld(p: [f64; 3]) -> [i64; 3] {
    p.map(|v| (v * 100.0).round() as i64)
}

/// The triangles' connected pieces (sharing a corner), as lists of indices.
fn connected(triangles: &[[[f64; 3]; 3]]) -> Vec<Vec<usize>> {
    let mut id: HashMap<[i64; 3], usize> = HashMap::new();
    let mut parent: Vec<usize> = Vec::new();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut first = Vec::with_capacity(triangles.len());
    for t in triangles {
        let v = t.map(|p| {
            let n = id.len();
            *id.entry(weld(p)).or_insert_with(|| {
                parent.push(n);
                n
            })
        });
        let r = root(&mut parent, v[0]);
        for &x in &v[1..] {
            let o = root(&mut parent, x);
            parent[o] = r;
        }
        first.push(v[0]);
    }
    let mut pieces: HashMap<usize, Vec<usize>> = HashMap::new();
    for (t, &v) in first.iter().enumerate() {
        pieces.entry(root(&mut parent, v)).or_default().push(t);
    }
    let mut pieces: Vec<Vec<usize>> = pieces.into_values().collect();
    pieces.sort();
    pieces
}

/// Trees along a tree-line card's foot: its corners with a higher corner above them ([`CARD_NEAR`], [`CARD_HIGH`]),
/// each as tall as the highest; along each edge between two foot corners, one tree per `spacing` (rounded by a seed of
/// the edge), each as tall as the card there.
pub fn card_spots(triangles: &[[[f64; 3]; 3]], spacing: f64) -> Vec<Spot> {
    let cell = |p: [f64; 3]| ((p[0] / CARD_NEAR).floor() as i64, (p[1] / CARD_NEAR).floor() as i64);
    let mut grid: HashMap<(i64, i64), Vec<[f64; 3]>> = HashMap::new();
    for &p in triangles.iter().flatten() {
        grid.entry(cell(p)).or_default().push(p);
    }
    // A corner's card height: the highest corner near it above it, when it is a foot.
    let tall = |p: [f64; 3]| {
        let (cx, cy) = cell(p);
        let top = (-1..=1)
            .flat_map(|dx| (-1..=1).map(move |dy| (cx + dx, cy + dy)))
            .flat_map(|c| grid.get(&c).into_iter().flatten())
            .filter(|q| (q[0] - p[0]).hypot(q[1] - p[1]) <= CARD_NEAR)
            .map(|q| q[2])
            .fold(p[2], f64::max);
        (top - p[2] >= CARD_HIGH).then_some(top - p[2])
    };
    // Foot edges by their welded ends: their ends and the card's height at each.
    let mut edges: HashMap<([i64; 3], [i64; 3]), FootEdge> = HashMap::new();
    for t in triangles {
        let feet = t.map(tall);
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if let (Some(ha), Some(hb)) = (feet[k], feet[(k + 1) % 3]) {
                let (ka, kb) = (weld(a), weld(b));
                let key = if ka <= kb { (ka, kb) } else { (kb, ka) };
                edges.insert(key, (a, b, ha, hb));
            }
        }
    }
    let mut keys: Vec<_> = edges.keys().copied().collect();
    keys.sort();
    let mut out = Vec::new();
    for key in keys {
        let (a, b, ha, hb) = edges[&key];
        let length = (b[0] - a[0]).hypot(b[1] - a[1]);
        let u = (seed(key.0) % 1000) as f64 / 1000.0;
        let n = (length / spacing + u).floor() as usize;
        for i in 0..n {
            let t = (i as f64 + 0.5) / n as f64;
            let foot = [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t);
            out.push(Spot { foot, height: ha + (hb - ha) * t });
        }
    }
    out
}

/// A foot edge of a card ([`card_spots`]): its ends and the card's height over each.
type FootEdge = ([f64; 3], [f64; 3], f64, f64);

/// A seed from a place (FNV-1a over its welded corner).
pub fn seed(at: [i64; 3]) -> u64 {
    at.iter()
        .flat_map(|v| v.to_le_bytes())
        .fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
}

/// The seed of a level tree's foot ([`seed`]).
pub fn seed_of(foot: [f64; 3]) -> u64 {
    seed(weld(foot))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two triangles: a vertical card from `a` to `b` (plan), `height` tall, standing at z = `z`.
    fn card(a: [f64; 2], b: [f64; 2], z: f64, height: f64) -> Vec<[[f64; 3]; 3]> {
        let p = |q: [f64; 2], up: f64| [q[0], q[1], z + up];
        vec![[p(a, 0.0), p(b, 0.0), p(b, height)], [p(a, 0.0), p(b, height), p(a, height)]]
    }

    #[test]
    fn small_objects_are_one_tree_and_tree_lines_are_scattered() {
        // A tree: a trunk (not cut) and a crossed card above it, not touching.
        let mut tree = card([0.0, 0.0], [0.2, 0.0], 10.0, 4.0);
        tree.extend(card([-3.0, 0.0], [3.0, 0.0], 13.0, 9.0));
        let cut = [false, false, true, true];
        let one = split(&tree, &cut, 8.0, 4.0);
        assert_eq!(one.spots, [Spot { foot: [0.0, 0.0, 10.0], height: 12.0 }]);
        assert!(one.kept.is_empty());
        // A wide object: a 100 m tree-line card, a low bush and an opaque wall.
        let mut wide = card([0.0, 0.0], [100.0, 0.0], 0.0, 20.0);
        wide.extend(card([0.0, 30.0], [1.0, 30.0], 0.0, 1.0));
        wide.extend(card([0.0, 50.0], [5.0, 50.0], 0.0, 3.0));
        let cut = [true, true, true, true, false, false];
        let split = split(&wide, &cut, 10.0, 4.0);
        assert_eq!((split.cards, split.from_cards), (1, 10));
        assert!(split.spots.iter().all(|s| s.height == 20.0 && s.foot[1] == 0.0 && s.foot[2] == 0.0));
        assert_eq!(split.kept, [2, 3, 4, 5]);
    }

    #[test]
    fn nearest_neighbours_median() {
        let points: Vec<[f64; 2]> = (0..20).map(|i| [i as f64 * 7.0, 3.0]).collect();
        assert_eq!(median_nearest(&points), Some(7.0));
        assert_eq!(median_nearest(&points[..1]), None);
    }
}
