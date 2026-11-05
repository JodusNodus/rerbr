//! The roadside the engine's reflections follow (ours, `rbr_sound::reflections::Roadside`): how far the nearest
//! reflecting surface is on each side of the car, how much of the side it fills and how hard it is, and a ceiling
//! over the car. Read from the stage's geometry after each step, never written (physics frame, z up).
//!
//! Two sources, each cheap:
//! - the scenery's collision meshes (the `.trk`'s kind `0x16`: trees, rocks, trunks, posts, signs, vehicles), each
//!   placement taken as an upright cylinder about its box. Every one within [`RANGE`] in a lobe about the side's axis
//!   ([`LOBE_COS`]) adds its echo: its cross-section at ear height times its hardness ([`hardness`], by the mesh's
//!   surface id; bushes, bendable trees and spectators are left out), the lobe's weight, and `1 / distance`;
//! - the stage's collision mesh (`.col`, [`StageProbe`]: banks, rock faces, walls, bridges) along [`RAYS`] rays a
//!   side, level and a little up, ahead and behind, from [`RAY_HEIGHT`] above the car. A hit adds its echo by how
//!   squarely its face looks back along the ray (the square of the cosine, so the road and gentle slopes count for
//!   nothing) at the distance of its plane (the specular point); a downward segment over the car finds a ceiling.
//!
//! A side's distance is its echoes' mean distance weighted by their squares (the strongest surface leads), its
//! hardness their weighted mean, and its size what makes `rbr_sound::reflections::side_gain` give their sum. The
//! constants are ours (game feel), tuned on the retail stages (`notes/sound.md`).

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Quat, Vec2, Vec3};
use rbr_formats::data::Data;
use rbr_formats::trk::{self, ChunkPayload, CollisionMesh};
use rbr_sound::reflections::{Roadside as Heard, Side, NEAR};

use crate::stage_probe::StageProbe;

/// The probe's reach, m.
pub const RANGE: f32 = 35.0;
/// The grid's square for the scenery, m.
const CELL: f32 = 24.0;
/// The band about the car's centre, m below and above, whose cross-section reflects to the cabin.
const EAR: [f32; 2] = [-0.5, 2.5];
/// The cross-section, m², that fills a side.
const FULL_AREA: f32 = 2.0;
/// The lobe about a side's axis: the cosine of its half angle (60°).
const LOBE_COS: f32 = 0.5;
/// The terrain's hardness (earth, grass, rock alike: the `.col` probe carries no surface).
const TERRAIN_HARDNESS: f32 = 0.7;
/// The rays' start above the car's centre, m, and their directions in the side's frame: yaw toward the front (rad)
/// and pitch up (rad).
const RAY_HEIGHT: f32 = 0.8;
pub const RAYS: [(f32, f32); 4] = [(0.0, 0.0), (0.5, 0.0), (-0.5, 0.0), (0.0, 0.25)];
/// The ceiling's search above the car, m: a face looking up between these heights (a deck, an overhang).
const CEILING: [f32; 2] = [1.5, 15.0];
const CEILING_HARDNESS: f32 = 0.9;

/// A scenery placement as the probe takes it: an upright cylinder. Vegetation the sound lets through (`growth`) is
/// kept for the body's contacts with it (`crate::verge`) and left out of the echoes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Post {
    pub at: Vec2,
    pub radius: f32,
    pub bottom: f32,
    pub top: f32,
    pub hardness: f32,
    pub growth: Option<Growth>,
}

/// Vegetation the car drives into: a bush of a density (0 to 1) or a bendable tree's branches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Growth {
    Bush(f32),
    Branches,
}

/// The vegetation of surface id `material` (`.trk` kind `0x16`), or `None`: the Spret and Lov bushes by size, the four
/// bush densities and `Bendable_Tree` (ids as [`hardness`]'s).
pub fn growth(material: u8) -> Option<Growth> {
    Some(match material {
        0x18 | 0x1b => Growth::Bush(0.5),
        0x19 | 0x1c => Growth::Bush(0.75),
        0x1a | 0x1d => Growth::Bush(1.0),
        0xa5 => Growth::Bush(0.3),
        0xa6 => Growth::Bush(0.5),
        0xa7 => Growth::Bush(0.8),
        0xa8 => Growth::Bush(1.0),
        0x79 => Growth::Branches,
        _ => return None,
    })
}

/// The stage's reflecting geometry.
#[derive(Debug, Clone, Default)]
pub struct Roadside {
    posts: Vec<Post>,
    cells: HashMap<(i32, i32), Vec<u32>>,
    widest: f32,
    ground: Option<Arc<StageProbe>>,
}

/// How hard a scenery mesh of surface id `material` and object kind `kind` (`.trk` kind `0x16`) reflects, or `None`
/// for what lets the sound through (bushes, bendable trees, spectators, cones). Ids: `physics.lsp`'s
/// `MaterialManager` (`rbr_formats::lsp_keys`).
pub fn hardness(material: u8, kind: u32) -> Option<f32> {
    Some(match material {
        // Spret and Lov bushes, Bendable_Tree, the four bush densities, spectators.
        0x18..=0x1d | 0x79 | 0xa5..=0xa8 | 0xc8 | 0xff => return None,
        // Tree_Small to Tree_Large: trunks and branches.
        0x1e..=0x20 => 0.5,
        0x38..=0x3a => 0.9,
        // Trunk_Small to Trunk_Large (logs, wooden poles and piles), Tree_Stump.
        0x3b..=0x3d | 0x78 => 0.6,
        0x3e => 0.4,
        0x3f | 0x40 => 1.0,
        0 => match kind {
            1 => return None,
            // Vehicles, the start fence, tall static meshes.
            0 | 5 => 0.8,
            2 | 3 => 0.7,
            6 => 0.4,
            _ => 0.5,
        },
        _ => 0.6,
    })
}

/// The posts of the collision mesh `mesh`'s placements: what reflects ([`hardness`]) and the vegetation ([`growth`]).
fn posts_of(mesh: &CollisionMesh) -> impl Iterator<Item = Post> + '_ {
    let growth = growth(mesh.unknown_byte);
    let kept = hardness(mesh.unknown_byte, mesh.kind).or(growth.map(|_| 0.0));
    let hard = if mesh.vertices.is_empty() { None } else { kept };
    let v = |p: &trk::Vec3| Vec3::new(p.x, p.y, p.z);
    let lo = mesh.vertices.iter().map(v).fold(Vec3::INFINITY, Vec3::min);
    let hi = mesh.vertices.iter().map(v).fold(Vec3::NEG_INFINITY, Vec3::max);
    hard.into_iter().flat_map(move |hardness| {
        mesh.objects.iter().map(move |o| {
            let [x, y, z, w] = o.values;
            let rotation = Quat::from_xyzw(x, y, z, w).normalize();
            let (at, scale) = (v(&o.position), v(&o.extent));
            let corners = (0..8).map(|i| {
                let c = Vec3::new(
                    if i & 1 == 0 { lo.x } else { hi.x },
                    if i & 2 == 0 { lo.y } else { hi.y },
                    if i & 4 == 0 { lo.z } else { hi.z },
                );
                at + rotation * (c * scale)
            });
            let (a, b) = corners.fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(a, b), c| (a.min(c), b.max(c)));
            let half = (b - a) * 0.5;
            let (at, radius) = (((a + b) * 0.5).truncate(), half.x.max(half.y));
            Post { at, radius, bottom: a.z, top: b.z, hardness, growth }
        })
    })
}

fn key(p: Vec2) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}

/// One echo's contribution to a side: its weight (relative amplitude), distance and hardness.
#[derive(Debug, Default, Clone, Copy)]
struct Sum {
    weight: f32,
    squares: f32,
    distance: f32,
    hardness: f32,
}

impl Sum {
    fn add(&mut self, weight: f32, distance: f32, hardness: f32) {
        self.weight += weight;
        self.squares += weight * weight;
        self.distance += weight * weight * distance;
        self.hardness += weight * hardness;
    }

    /// The side its echoes make: the size that gives `side_gain` their summed weight at the weighted distance.
    fn side(&self) -> Side {
        if self.weight <= 0.0 {
            return Side::default();
        }
        let distance = self.distance / self.squares;
        let hardness = self.hardness / self.weight;
        let size = (self.weight * distance.max(NEAR) / hardness).min(1.0);
        Side { distance, size, hardness }
    }
}

impl Roadside {
    /// The roadside of scenery `posts` and the stage's collision mesh `ground`.
    pub fn new(posts: Vec<Post>, ground: Option<Arc<StageProbe>>) -> Roadside {
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, post) in posts.iter().enumerate() {
            cells.entry(key(post.at)).or_default().push(i as u32);
        }
        let widest = posts.iter().map(|p| p.radius).fold(0.0, f32::max);
        Roadside { posts, cells, widest, ground }
    }

    /// The roadside of stage `stage` (a name or folder): its `.trk`'s scenery (none when it is missing or unreadable)
    /// and `ground`, the stage's collision probe the cameras use.
    pub fn load(data: &Data, stage: &str, ground: Option<Arc<StageProbe>>) -> Roadside {
        let trk = rbr_formats::stage_files::read(data, stage, "trk").ok().and_then(|b| trk::parse(&b).ok());
        let posts = trk
            .iter()
            .flat_map(|t| &t.chunks)
            .filter_map(|c| match &c.payload {
                ChunkPayload::CollisionMeshes(m) => Some(&m.meshes),
                _ => None,
            })
            .flatten()
            .flat_map(posts_of)
            .collect();
        Roadside::new(posts, ground)
    }

    /// The scenery placements it holds.
    pub fn posts(&self) -> &[Post] {
        &self.posts
    }

    /// The placements (index, post) whose cells lie within `reach` m of `p`, plus the widest post's radius.
    pub fn near(&self, p: Vec2, reach: f32) -> impl Iterator<Item = (u32, &Post)> + '_ {
        let reach = reach + self.widest;
        let (lo, hi) = (key(p - Vec2::splat(reach)), key(p + Vec2::splat(reach)));
        (lo.0..=hi.0)
            .flat_map(move |x| (lo.1..=hi.1).map(move |y| (x, y)))
            .flat_map(|cell| self.cells.get(&cell).into_iter().flatten())
            .map(|&i| (i, &self.posts[i as usize]))
    }

    /// The roadside about the car at `position` facing `forward` (physics frame): its left and right as the car
    /// has them, and the ceiling over it.
    pub fn probe(&self, position: Vec3, forward: Vec3) -> Heard {
        let ahead = forward.truncate().normalize_or_zero();
        if ahead == Vec2::ZERO {
            return Heard::default();
        }
        // Right-handed, z up: right is forward x up.
        let right = Vec2::new(ahead.y, -ahead.x);
        let mut sums = [Sum::default(); 2];
        self.scenery(position, right, &mut sums);
        let mut overhead = Side::default();
        if let Some(ground) = &self.ground {
            terrain(ground, position, ahead, right, &mut sums);
            overhead = ceiling(ground, position);
        }
        Heard { sides: sums.map(|s| s.side()), overhead }
    }

    fn scenery(&self, position: Vec3, right: Vec2, sums: &mut [Sum; 2]) {
        let p = position.truncate();
        let reach = RANGE + self.widest;
        let (lo, hi) = (key(p - Vec2::splat(reach)), key(p + Vec2::splat(reach)));
        let band = [position.z + EAR[0], position.z + EAR[1]];
        for x in lo.0..=hi.0 {
            for y in lo.1..=hi.1 {
                for &i in self.cells.get(&(x, y)).into_iter().flatten() {
                    let post = &self.posts[i as usize];
                    if post.growth.is_some() {
                        continue;
                    }
                    let to = post.at - p;
                    let centre = to.length();
                    let distance = centre - post.radius;
                    let height = post.top.min(band[1]) - post.bottom.max(band[0]);
                    if distance <= 0.0 || distance > RANGE || height <= 0.0 {
                        continue;
                    }
                    let across = to.dot(right) / centre;
                    let lobe = ((across.abs() - LOBE_COS) / (1.0 - LOBE_COS)).max(0.0);
                    if lobe == 0.0 {
                        continue;
                    }
                    let size = (2.0 * post.radius * height / FULL_AREA).min(1.0);
                    let weight = size * post.hardness * lobe * lobe / distance.max(NEAR);
                    sums[usize::from(across > 0.0)].add(weight, distance, post.hardness);
                }
            }
        }
    }
}

/// The terrain's echoes along each side's rays.
fn terrain(ground: &StageProbe, position: Vec3, ahead: Vec2, right: Vec2, sums: &mut [Sum; 2]) {
    let from = position + Vec3::Z * RAY_HEIGHT;
    for (side, sum) in sums.iter_mut().enumerate() {
        let out = if side == 0 { -right } else { right };
        for &(yaw, pitch) in &RAYS {
            let level = out * yaw.cos() + ahead * yaw.sin();
            let direction = (level * pitch.cos()).extend(pitch.sin());
            let Some(hit) = ground.segment(from, from + direction * RANGE) else { continue };
            let facing = -hit.normal.dot(direction);
            if facing <= 0.0 {
                continue;
            }
            let distance = hit.distance(from);
            let weight = facing * facing * TERRAIN_HARDNESS / distance.max(NEAR) / RAYS.len() as f32;
            sum.add(weight, distance, TERRAIN_HARDNESS);
        }
    }
}

/// A face looking down at the car from between [`CEILING`]'s heights: a bridge's deck, an overhang.
fn ceiling(ground: &StageProbe, position: Vec3) -> Side {
    let top = position + Vec3::Z * CEILING[1];
    match ground.segment(top, position + Vec3::Z * CEILING[0]) {
        Some(hit) if hit.normal.z > 0.5 => {
            Side { distance: hit.point.z - position.z, size: 1.0, hardness: CEILING_HARDNESS }
        }
        _ => Side::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wall `d` m to the car's right (+x, the car facing +y), 60 m long and 6 m tall, facing the car.
    fn wall(d: f32) -> StageProbe {
        let (a, b, c, e) =
            (Vec3::new(d, -30.0, -1.0), Vec3::new(d, 30.0, -1.0), Vec3::new(d, 30.0, 5.0), Vec3::new(d, -30.0, 5.0));
        // Wound so that `e2 x e1` points back toward -x.
        let probe = StageProbe::from_triangles(&[[a, b, c], [a, c, e]]);
        let n = probe.segment(Vec3::ZERO, Vec3::new(2.0 * d, 0.0, 0.0)).map(|h| h.normal);
        assert_eq!(n, Some(Vec3::NEG_X), "the wall faces the car");
        probe
    }

    /// A floor at z = -0.5 under the car, facing up.
    fn floor() -> [[Vec3; 3]; 2] {
        let (a, b, c, d) = (
            Vec3::new(-60.0, -60.0, -0.5),
            Vec3::new(60.0, -60.0, -0.5),
            Vec3::new(60.0, 60.0, -0.5),
            Vec3::new(-60.0, 60.0, -0.5),
        );
        [[a, c, b], [a, d, c]]
    }

    #[test]
    fn a_wall_at_a_known_distance_gives_that_distance_on_its_side() {
        for d in [3.0, 12.0, 30.0] {
            let roadside = Roadside::new(Vec::new(), Some(Arc::new(wall(d))));
            let heard = roadside.probe(Vec3::ZERO, Vec3::Y);
            let [left, right] = heard.sides;
            assert_eq!(left, Side::default(), "{d}");
            assert!((right.distance - d).abs() < 1e-3, "{d}: {right:?}");
            assert!((right.hardness - TERRAIN_HARDNESS).abs() < 1e-5);
            assert!(right.size > 0.5, "{d}: {right:?}");
            // Turned round, it is on the left.
            let back = roadside.probe(Vec3::ZERO, Vec3::NEG_Y);
            assert!((back.sides[0].distance - d).abs() < 1e-3 && back.sides[1] == Side::default());
        }
        // Beyond the reach, nothing.
        assert_eq!(Roadside::new(Vec::new(), Some(Arc::new(wall(40.0)))).probe(Vec3::ZERO, Vec3::Y), Heard::default());
    }

    #[test]
    fn open_ground_gives_nothing() {
        let ground = StageProbe::from_triangles(&floor());
        let heard = Roadside::new(Vec::new(), Some(Arc::new(ground))).probe(Vec3::ZERO, Vec3::Y);
        assert_eq!(heard, Heard::default());
        assert_eq!(Roadside::default().probe(Vec3::ZERO, Vec3::Y), Heard::default());
    }

    #[test]
    fn a_tree_beside_the_car_counts_and_one_ahead_does_not() {
        let tree = |x: f32, y: f32| Post {
            at: Vec2::new(x, y),
            radius: 0.3,
            bottom: -1.0,
            top: 10.0,
            hardness: 0.5,
            growth: None,
        };
        let roadside = Roadside::new(vec![tree(-5.3, 0.0), tree(0.0, 8.0)], None);
        let [left, right] = roadside.probe(Vec3::ZERO, Vec3::Y).sides;
        assert!((left.distance - 5.0).abs() < 1e-4 && left.hardness == 0.5 && left.size > 0.0, "{left:?}");
        assert_eq!(right, Side::default());
        // A post under the band (a kerb stone) or over it (a branch) does not.
        let low = Post { top: -0.6, ..tree(4.0, 0.0) };
        let high = Post { bottom: 3.0, ..tree(4.0, 0.0) };
        assert_eq!(Roadside::new(vec![low, high], None).probe(Vec3::ZERO, Vec3::Y), Heard::default());
        // A bush or a bendable tree beside it neither.
        let bush = Post { growth: Some(Growth::Bush(1.0)), hardness: 0.0, ..tree(-5.3, 0.0) };
        let branches = Post { growth: Some(Growth::Branches), hardness: 0.0, ..tree(5.3, 0.0) };
        assert_eq!(Roadside::new(vec![bush, branches], None).probe(Vec3::ZERO, Vec3::Y), Heard::default());
    }

    #[test]
    fn a_deck_over_the_car_is_a_ceiling() {
        let (a, b, c, d) = (
            Vec3::new(-5.0, -5.0, 6.0),
            Vec3::new(5.0, -5.0, 6.0),
            Vec3::new(5.0, 5.0, 6.0),
            Vec3::new(-5.0, 5.0, 6.0),
        );
        let deck = StageProbe::from_triangles(&[[a, c, b], [a, d, c]]);
        let heard = Roadside::new(Vec::new(), Some(Arc::new(deck))).probe(Vec3::ZERO, Vec3::Y);
        assert!((heard.overhead.distance - 6.0).abs() < 1e-4 && heard.overhead.size == 1.0, "{heard:?}");
    }

    #[test]
    fn bushes_and_spectators_let_the_sound_through() {
        assert_eq!(hardness(0x79, 0), None);
        assert_eq!(hardness(0x1c, 0), None);
        assert_eq!(hardness(0xc8, 0), None);
        assert_eq!(hardness(0x3a, 0), Some(0.9));
        assert_eq!(hardness(0x1f, 0), Some(0.5));
        assert_eq!(
            (growth(0x79), growth(0x1d), growth(0xa5)),
            (Some(Growth::Branches), Some(Growth::Bush(1.0)), Some(Growth::Bush(0.3)))
        );
        assert_eq!((growth(0xc8), growth(0xff), growth(0x1f)), (None, None, None));
    }

    /// Every retail stage's scenery loads, and every stage but map 69 has some.
    #[test]
    fn retail_scenery_loads() {
        let Some(data) = Data::from_env() else {
            eprintln!("skipping: RBR_DATA is not set");
            return;
        };
        let maps = data.install().join("Maps");
        let (mut stages, mut bare) = (0, Vec::new());
        for entry in std::fs::read_dir(&maps).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(stage) = name.strip_suffix(".trk").filter(|s| s.starts_with("track-")) else { continue };
            let roadside = Roadside::load(&data, stage, None);
            if roadside.posts().is_empty() {
                bare.push(stage.to_owned());
            }
            assert!(roadside.posts().iter().all(|p| p.radius.is_finite() && p.top >= p.bottom), "{stage}");
            stages += 1;
        }
        // Map 69 (five weathers) is the one stage without scenery.
        assert!(stages > 40 && bare.iter().all(|s| s.starts_with("track-69_")), "{stages}, without scenery: {bare:?}");
    }
}
