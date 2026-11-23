//! How much of a stage's road the sun reaches from a direction (ours), for where golden hour's sun stands
//! (`relight::Clock::golden`): the share of points along the driveline, [`HEIGHT`] over it, whose ray toward the sun
//! meets none of what casts the sun's shadows (`renderer::shadows`). That is the world's draws that write depth and do
//! not blend, less the far level of detail and our grass (`Lod::Close`), their cut-out texels letting the ray through
//! (`foliage::Cut`); and the backdrop but a sky dome, [`crate::sun::backdrop_drop`] lower as in the sun's maps. The
//! stage's own grass tufts, which cast no sun shadow either, block the rays here: telling them from the other plants
//! (`foliage::with_plants`) took 50 s on 580 Helette, the probe about a second. On the CPU at load, before the renderer
//! is built: the sun stays where it is for the drive.

use crate::foliage::{self, Cut};
use crate::parallel::in_parallel;
use crate::renderer::cutout;
use crate::stage::{Anchor, Draw, Layer, Lod, Stage, TextureRef};
use glam::{Vec2, Vec3, Vec3Swizzles};
use std::collections::HashMap;

/// The points' spacing along the driveline, metres.
const SPACING: f32 = 10.0;
/// The points' height over the driveline, metres: about a car's middle.
const HEIGHT: f32 = 1.0;
/// The grid's cell, metres, at the least: wider where the stage is more than [`CELLS`] of them across.
const CELL: f32 = 8.0;
const CELLS: f32 = 2048.0;

/// A triangle that may stand in the sun's way: its mesh, its first index, and its cut-out texture.
#[derive(Clone, Copy)]
struct Blocker {
    mesh: u32,
    first: u32,
    cut: Option<TextureRef>,
}

/// A stage's road points and what stands between them and the sun: the stage's blockers and the backdrop's.
pub struct Sunlit<'a> {
    stage: &'a Stage,
    cuts: HashMap<TextureRef, Cut>,
    points: Vec<Vec3>,
    world: Grid,
    backdrop: Grid,
}

/// Blockers by the cells of a grid over x and z that their boxes cover, and each cell's highest point.
struct Grid {
    blockers: Vec<Blocker>,
    low: Vec2,
    cell: f32,
    size: [usize; 2],
    /// Cell `i`'s blockers are `items[starts[i]..starts[i + 1]]`.
    starts: Vec<u32>,
    items: Vec<u32>,
    top: Vec<f32>,
    highest: f32,
}

impl<'a> Sunlit<'a> {
    /// `stage`'s (its textures loaded, `Stage::load_textures`).
    pub fn of(stage: &'a Stage) -> Sunlit<'a> {
        let cuts = foliage::cut_out(stage, cutout);
        let over = crate::sky_dome::Over::of(&stage.meshes, &stage.driveline);
        let (mut blockers, mut backdrop) = (Vec::new(), Vec::new());
        for (m, mesh) in stage.meshes.iter().enumerate().filter(|(_, mesh)| mesh.anchor == Anchor::World) {
            let back = match mesh.layer {
                Layer::Solid => false,
                Layer::Sky if !over.as_ref().is_some_and(|o| o.roofed_by(mesh)) => true,
                _ => continue,
            };
            let casts = |draw: &&Draw| {
                (draw.state.depth_write || back) && !draw.state.blend && !matches!(draw.lod, Lod::Close | Lod::Far)
            };
            for draw in mesh.draws.iter().filter(casts) {
                let cut = draw.state.textures[0].filter(|texture| cutout(&draw.state) && cuts.contains_key(texture));
                let into = if back { &mut backdrop } else { &mut blockers };
                into.extend(draw.indices.clone().step_by(3).map(|first| Blocker {
                    mesh: m as u32,
                    first: first as u32,
                    cut,
                }));
            }
        }
        let (world, backdrop) = (Grid::of(blockers, stage), Grid::of(backdrop, stage));
        Sunlit { stage, cuts, points: points(&stage.driveline), world, backdrop }
    }

    /// The share of the road points the sun `toward` reaches; all of them on a stage without a driveline.
    pub fn share(&self, toward: Vec3) -> f32 {
        if self.points.is_empty() {
            return 1.0;
        }
        let toward = toward.normalize();
        let drop = crate::sun::backdrop_drop(-toward);
        let stops = |b: &Blocker, from: Vec3| self.stops(b, from, toward);
        let lit = in_parallel(&self.points, |&p| {
            !self.world.blocks(p, toward, stops) && !self.backdrop.blocks(p - drop, toward, stops)
        });
        lit.iter().filter(|&&lit| lit).count() as f32 / self.points.len() as f32
    }

    /// Whether `b` stops the ray from `from` along `dir`: it meets the triangle at an opaque texel.
    fn stops(&self, b: &Blocker, from: Vec3, dir: Vec3) -> bool {
        let Some((u, v)) = meet(corners(self.stage, b), from, dir) else { return false };
        let Some(cut) = b.cut.and_then(|texture| self.cuts.get(&texture)) else { return true };
        let mesh = &self.stage.meshes[b.mesh as usize];
        let uv = |k: usize| Vec2::from(mesh.vertices[mesh.indices[b.first as usize + k] as usize].uv0);
        !cut.clear_at((uv(0) * (1.0 - u - v) + uv(1) * u + uv(2) * v).to_array())
    }
}

impl Grid {
    /// Whether a blocker `stops` the ray from `from` along `dir` (up): the cells it crosses, in order, until
    /// it is over the stage's highest point or off the grid (Amanatides and Woo's walk).
    fn blocks(&self, from: Vec3, dir: Vec3, stops: impl Fn(&Blocker, Vec3) -> bool) -> bool {
        let g = self;
        let at = (from.xz() - g.low) / g.cell;
        let (mut x, mut z) = (at.x.floor() as i64, at.y.floor() as i64);
        let end = (g.highest - from.y) / dir.y.max(1e-4);
        let step = |d: f32| if d > 0.0 { 1 } else { -1 };
        let (sx, sz) = (step(dir.x), step(dir.z));
        let cross = |c: i64, s: i64, o: f32, d: f32| {
            let edge = (c + i64::from(s > 0)) as f32;
            if d == 0.0 {
                f32::INFINITY
            } else {
                (edge - o) * g.cell / d
            }
        };
        let (mut tx, mut tz) = (cross(x, sx, at.x, dir.x), cross(z, sz, at.y, dir.z));
        let (dx, dz) = (g.cell / dir.x.abs(), g.cell / dir.z.abs());
        let mut enter = 0.0f32;
        while enter <= end {
            if x < 0 || z < 0 || x as usize >= g.size[0] || z as usize >= g.size[1] {
                return false;
            }
            let cell = z as usize * g.size[0] + x as usize;
            if g.top[cell] >= from.y + dir.y * enter {
                let items = &g.items[g.starts[cell] as usize..g.starts[cell + 1] as usize];
                if items.iter().any(|&i| stops(&self.blockers[i as usize], from)) {
                    return true;
                }
            }
            if tx < tz {
                (x, enter, tx) = (x + sx, tx, tx + dx);
            } else {
                (z, enter, tz) = (z + sz, tz, tz + dz);
            }
        }
        false
    }

    fn empty() -> Grid {
        Grid {
            blockers: Vec::new(),
            low: Vec2::ZERO,
            cell: CELL,
            size: [0, 0],
            starts: vec![0],
            items: Vec::new(),
            top: Vec::new(),
            highest: 0.0,
        }
    }

    /// `blockers`, of `stage`.
    fn of(blockers: Vec<Blocker>, stage: &Stage) -> Grid {
        let boxes: Vec<(Vec3, Vec3)> = blockers
            .iter()
            .map(|b| {
                let [a, b, c] = corners(stage, b);
                (a.min(b).min(c), a.max(b).max(c))
            })
            .collect();
        let Some((low, high)) = boxes.iter().copied().reduce(|(l, h), (bl, bh)| (l.min(bl), h.max(bh))) else {
            return Grid::empty();
        };
        let span = (high - low).xz();
        let cell = CELL.max(span.max_element() / CELLS);
        let size = [(span.x / cell) as usize + 1, (span.y / cell) as usize + 1];
        let cells = |(l, h): (Vec3, Vec3)| {
            let at = |p: Vec3| (((p.x - low.x) / cell) as usize, ((p.z - low.z) / cell) as usize);
            let ((x0, z0), (x1, z1)) = (at(l), at(h));
            (z0..=z1.min(size[1] - 1)).flat_map(move |z| (x0..=x1.min(size[0] - 1)).map(move |x| z * size[0] + x))
        };
        let mut counts = vec![0u32; size[0] * size[1] + 1];
        let mut top = vec![f32::MIN; size[0] * size[1]];
        for &b in &boxes {
            for i in cells(b) {
                counts[i + 1] += 1;
                top[i] = top[i].max(b.1.y);
            }
        }
        for i in 1..counts.len() {
            counts[i] += counts[i - 1];
        }
        let mut items = vec![0u32; counts[counts.len() - 1] as usize];
        let mut fill = counts.clone();
        for (k, &b) in boxes.iter().enumerate() {
            for i in cells(b) {
                items[fill[i] as usize] = k as u32;
                fill[i] += 1;
            }
        }
        Grid { blockers, low: low.xz(), cell, size, starts: counts, items, top, highest: high.y }
    }
}

/// Blocker `b`'s corners.
fn corners(stage: &Stage, b: &Blocker) -> [Vec3; 3] {
    let mesh = &stage.meshes[b.mesh as usize];
    [0, 1, 2].map(|k| Vec3::from(mesh.vertices[mesh.indices[b.first as usize + k] as usize].position))
}

/// Points [`SPACING`] apart along `driveline`, [`HEIGHT`] over it.
fn points(driveline: &[Vec3]) -> Vec<Vec3> {
    let mut out = Vec::new();
    let mut along = 0.0;
    for pair in driveline.windows(2) {
        let length = pair[0].distance(pair[1]);
        while along < length {
            out.push(pair[0].lerp(pair[1], along / length) + Vec3::Y * HEIGHT);
            along += SPACING;
        }
        along -= length;
    }
    out
}

/// Where the ray from `from` along `dir` meets the triangle `[a, b, c]` ahead of it, as the weights of `b` and `c`
/// (Möller and Trumbore), from either side.
fn meet([a, b, c]: [Vec3; 3], from: Vec3, dir: Vec3) -> Option<(f32, f32)> {
    let (e1, e2) = (b - a, c - a);
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None;
    }
    let s = (from - a) / det;
    let u = s.dot(p);
    let q = s.cross(e1);
    let v = dir.dot(q);
    let t = e2.dot(q);
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t > 1e-3).then_some((u, v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ray_meets_a_triangle_ahead_from_either_side() {
        let t = [Vec3::new(-1.0, 5.0, -1.0), Vec3::new(1.0, 5.0, -1.0), Vec3::new(0.0, 5.0, 1.0)];
        assert!(meet(t, Vec3::ZERO, Vec3::Y).is_some());
        assert!(meet(t, Vec3::new(0.0, 10.0, 0.0), -Vec3::Y).is_some());
        assert!(meet(t, Vec3::ZERO, -Vec3::Y).is_none());
        assert!(meet(t, Vec3::new(3.0, 0.0, 0.0), Vec3::Y).is_none());
    }

    #[test]
    fn the_points_lie_evenly_along_the_driveline() {
        let line = [Vec3::ZERO, Vec3::new(15.0, 0.0, 0.0), Vec3::new(15.0, 0.0, 25.0)];
        let p = points(&line);
        assert_eq!(p.len(), 4);
        assert!((p[2] - Vec3::new(15.0, HEIGHT, 5.0)).length() < 1e-4);
    }
}
