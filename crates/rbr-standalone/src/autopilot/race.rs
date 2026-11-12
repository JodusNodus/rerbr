//! Race driving (`--race LAT,BRAKE,SLIP`, ours, test tooling): the autopilot at the car's measured limits on a line that
//! uses the road's width, so stage times can be read against real ones (`notes/tyre-model.md`, "Race autopilot").
//!
//! At the start the mode builds the line once: stations every `STEP` m of driveline, the road's edges at each from the
//! stage's collision mesh (`Ground`: the ground's material is road, gravel, tarmac, snow or dirt, and its height stays
//! within `HEIGHT` m plus `CAMBER` per m of the driveline's), the line that bends least inside the edges less
//! `MARGIN` (minimum curvature: the sum of the squared second differences, by projected accelerated gradient), and
//! the heavy props by the driveline (`Placed`) shut the side of the road they stand on. On that line the speed plan
//! takes the given lateral and braking acceleration (m/s², the car's measured limits on the stage's surface: the flat
//! circle's and the threshold stop's, `harness/limit_pace.py --surface`) with the friction ellipse in braking. The
//! controller steers to the line (on tarmac by the default's law, on a `Line::loose` surface by the `Tracker`), holds
//! the plan `ANTICIPATE` s ahead and brakes under the `--plan` threshold controller at the stop's slip; gears,
//! traction, recovery and the start are the default's.

use rbr_formats::data::Data;
use rbr_formats::lsp_keys::{Class, KEYS};
use rbr_formats::{col, mat, stage_files};
use rbr_physics::ngp::material::{cell, SIDE};

use super::{distance, point, Placed, SpeedZone, CLEAR, PASSED};
use rbr_formats::trk::DriveLineRecord;
use rbr_recording::pose::CarPose;

/// The line's station spacing along the driveline (m).
const STEP: f32 = 2.0;
/// The road scan across each station: its step and its reach either side (m).
const SCAN: f32 = 0.25;
const REACH: f32 = 10.0;
/// A ground point is road while its height is within `HEIGHT` m plus `CAMBER` per m across of the driveline's.
const HEIGHT: f32 = 0.3;
const CAMBER: f32 = 0.06;
/// The driveline's sampling for its length: per record.
const DENSE: f32 = 64.0;
/// The edges' smoothing: stations either side.
const EDGE_SPAN: usize = 2;
/// The car's centre keeps this far inside the road's edge (m: half the car, 0.9, and a margin). 1.0-1.4 drive RSF 562
/// alike (the car is power-limited there), 2.2 cost 30 s.
const MARGIN: f32 = 1.4;
/// The line's solver: iterations of the projected gradient.
const ITERATIONS: usize = 12000;
/// The top speed without `--top-speed` (m/s): above the cars' own, so the plan and the car set the speed.
pub const TOP: f32 = 60.0;
/// The speed plan holds the speed this far ahead (s): the pedals and the car answer late (RSF 562: 0.25 s 594 s, 0.15
/// 589, 0.1 592, none 611 with recoveries).
pub const ANTICIPATE: f32 = 0.15;
/// The tracking guard's distance from the line (m; the default's `TRACK_ERR` is 1.2): slowing at 1.2 m cost 4 s on RSF
/// 562.
pub const TRACK_ERR: f32 = 2.0;
/// `Line::loose`'s slip: tarmac's best stops held 0.2-0.3, gravel's and snow's 0.5.
const LOOSE_SLIP: f32 = 0.4;

/// `--race LAT,BRAKE,SLIP`: the car's lateral and braking acceleration on the stage's surface (m/s², both over 0) and
/// the slip ratio its best threshold stop held (`harness/limit_pace.py`'s `brake_run`: 0.5 on gravel and snow, 0.2-0.3
/// on tarmac; under it the race pedal holds, as `--plan`'s does at `PLAN_SLIP`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Limits {
    pub lateral: f32,
    pub braking: f32,
    pub slip: f32,
}

impl std::str::FromStr for Limits {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let v: Vec<f32> =
            s.split(',').map(|x| x.parse().map_err(|_| format!("--race {s}"))).collect::<Result<_, _>>()?;
        match v[..] {
            [lateral, braking, slip] if lateral > 0.0 && braking > 0.0 && slip > 0.0 && slip < 1.0 => {
                Ok(Limits { lateral, braking, slip })
            }
            _ => Err(format!("--race {s}: want LAT,BRAKE,SLIP: m/s² over 0 and a slip ratio in 0-1")),
        }
    }
}

/// The ground of a stage: its collision triangles in a grid of `GRID` m cells, each with the material it reads as road.
#[derive(Clone, Default)]
pub struct Ground {
    faces: Vec<Face>,
    cells: std::collections::HashMap<(i32, i32), Vec<u32>>,
    /// The `.mat` entry's sheets as road flags, 16 by 16 per run.
    sheets: Vec<Vec<bool>>,
}

impl std::fmt::Debug for Ground {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Ground({} faces)", self.faces.len())
    }
}

/// `--race`: the limits and the stage's ground.
#[derive(Clone, Debug)]
pub struct Race {
    pub limits: Limits,
    pub ground: Ground,
}

#[derive(Clone)]
struct Face {
    corners: [[f32; 3]; 3],
    /// Per material, per corner: the sheet's low and high nibbles over 15.
    grid: [[[f32; 2]; 3]; 2],
    /// Per material: its sheet in the `.mat` entry.
    runs: [u8; 2],
    weight: [f32; 3],
}

const GRID: f32 = 4.0;

fn barycentric(c: [[f32; 3]; 3], x: f32, y: f32) -> Option<[f32; 3]> {
    let d = (c[1][1] - c[2][1]) * (c[0][0] - c[2][0]) + (c[2][0] - c[1][0]) * (c[0][1] - c[2][1]);
    if d.abs() < 1e-9 {
        return None;
    }
    let a = ((c[1][1] - c[2][1]) * (x - c[2][0]) + (c[2][0] - c[1][0]) * (y - c[2][1])) / d;
    let b = ((c[2][1] - c[0][1]) * (x - c[2][0]) + (c[0][0] - c[2][0]) * (y - c[2][1])) / d;
    let e = 1.0 - a - b;
    (a >= -1e-4 && b >= -1e-4 && e >= -1e-4).then_some([a, b, e])
}

/// Whether surface id `id` is a road surface (NGP's material names: gravel, tarmac, snow on gravel or ice, dirt).
fn road_ids() -> [bool; 256] {
    let mut road = [false; 256];
    for k in KEYS.iter().filter(|k| k.class == Class::MaterialManager) {
        let name = k.key.trim_start_matches("US2_").trim_start_matches("BR2_");
        if ["Gravel_", "Tarmac_", "SnowOn", "Dirt_"].iter().any(|p| name.starts_with(p)) {
            if let Some(r) = road.get_mut(k.offset as usize) {
                *r = true;
            }
        }
    }
    road
}

impl Ground {
    /// The ground of `stage` (`stage_files`): its `.col` triangles and `.mat` sheets (the new dry entry, else the first).
    pub fn load(data: &Data, stage: &str) -> Result<Ground, String> {
        let col = col::parse(&stage_files::read(data, stage, "col")?).map_err(|e| format!("{stage}.col: {e:?}"))?;
        let mat = mat::parse(&stage_files::read(data, stage, "mat")?).map_err(|e| format!("{stage}.mat: {e:?}"))?;
        let entry = mat::select(&mat.entries, "new", "dry").unwrap_or(0);
        let runs = &mat.entries.get(entry).ok_or(format!("{stage}.mat: no entry"))?.runs;
        let road = road_ids();
        let mut faces = Vec::new();
        let mut cells: std::collections::HashMap<(i32, i32), Vec<u32>> = std::collections::HashMap::new();
        for c in &col.cells {
            for t in &c.tree.triangles {
                let [Some(a), Some(b), Some(d)] = t.vertices.map(|v| c.vertices.get(v as usize).copied()) else {
                    continue;
                };
                let corners = [a, b, d];
                let nib = |byte: u8| [f32::from(byte & 0xf) / 15.0, f32::from(byte >> 4) / 15.0];
                let face = Face {
                    corners,
                    grid: [std::array::from_fn(|i| nib(t.grid[i])), std::array::from_fn(|i| nib(t.grid[3 + i]))],
                    runs: t.materials,
                    weight: t.attr_a().0.map(|w| f32::from(w) / 31.0),
                };
                let lo = |k: usize| corners.iter().map(|p| p[k]).fold(f32::MAX, f32::min);
                let hi = |k: usize| corners.iter().map(|p| p[k]).fold(f32::MIN, f32::max);
                let (x0, x1) = ((lo(0) / GRID).floor() as i32, (hi(0) / GRID).floor() as i32);
                let (y0, y1) = ((lo(1) / GRID).floor() as i32, (hi(1) / GRID).floor() as i32);
                if x1 - x0 > 16 || y1 - y0 > 16 {
                    continue;
                }
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        cells.entry((x, y)).or_default().push(faces.len() as u32);
                    }
                }
                faces.push(face);
            }
        }
        let sheets = runs.iter().map(|r| r.cells.iter().map(|&id| road[usize::from(id)]).collect()).collect();
        Ok(Ground { faces, cells, sheets })
    }

    /// The highest ground under `(x, y)` up to `z` + 3.5 m: its height and whether it is road.
    fn below(&self, x: f32, y: f32, z: f32) -> Option<(f32, bool)> {
        let mut best: Option<(f32, &Face, [f32; 3])> = None;
        for &i in self.cells.get(&((x / GRID).floor() as i32, (y / GRID).floor() as i32))? {
            let f = &self.faces[i as usize];
            let Some(w) = barycentric(f.corners, x, y) else { continue };
            let h = (0..3).map(|k| w[k] * f.corners[k][2]).sum::<f32>();
            if h <= z + 3.5 && best.is_none_or(|(bz, _, _)| h > bz) {
                best = Some((h, f, w));
            }
        }
        let (h, f, w) = best?;
        let weight: f32 = (0..3).map(|i| w[i] * f.weight[i]).sum();
        let k = usize::from(weight >= 0.5);
        let (a, b) = (0..3).fold((0.0, 0.0), |(a, b), i| (a + w[i] * f.grid[k][i][0], b + w[i] * f.grid[k][i][1]));
        let sheet = self.sheets.get(usize::from(f.runs[k]));
        let road = sheet.is_some_and(|sheet| sheet[(cell(b) * SIDE + cell(a)) as usize]);
        Some((h, road))
    }

    /// The road's extent across `centre` along the left normal `left` (m to the left, m to the right): the scan stops at
    /// the first point off the road or off its height.
    fn edges(&self, centre: [f32; 3], left: [f32; 2]) -> [f32; 2] {
        let Some((z0, true)) = self.below(centre[0], centre[1], centre[2]) else { return [0.0; 2] };
        let side = |sign: f32| {
            let mut reach = 0.0;
            let mut o = SCAN;
            while o <= REACH {
                let (x, y) = (centre[0] + sign * o * left[0], centre[1] + sign * o * left[1]);
                match self.below(x, y, z0) {
                    Some((h, true)) if (h - z0).abs() < HEIGHT + CAMBER * o => reach = o,
                    _ => break,
                }
                o += SCAN;
            }
            reach
        };
        [side(1.0), side(-1.0)]
    }
}

/// The race line: stations every `STEP` m of the driveline's length, each with its driveline distance, the line's point,
/// the plan's speed and the line's curvature; and the steering's state.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Each station's driveline distance (m, `values`' scale).
    at: Vec<f32>,
    points: Vec<[f32; 3]>,
    speed: Vec<f32>,
    /// Signed (left positive, 1/m), smoothed as the plan reads it.
    curvature: Vec<f32>,
    /// The offset from the driveline (m, left positive) and the road's edges either side (m).
    offset: Vec<f32>,
    edges: Vec<[f32; 2]>,
    /// The antilock's slip ratio (`Limits::slip`).
    pub slip: f32,
    tracker: Tracker,
}

impl Line {
    /// The line on `track` within `ground`'s road, kept clear of `props`, and its speed plan at `limits` up to `top`,
    /// holding each of `zones`' speed in its window (`--speed-zone`).
    pub(super) fn build(
        track: &[DriveLineRecord],
        values: &[f32],
        ground: &Ground,
        props: &[Placed],
        limits: Limits,
        top: f32,
        zones: &[SpeedZone],
    ) -> Line {
        // Stations every `STEP` m of the driveline's own length: its distance (`values`) runs unevenly along the
        // Hermite segments (2 m of it spans 0.5-4.7 m of road on RSF 562), and uneven stations bend the line.
        let last = (track.len() - 1) as f32;
        let dense: Vec<f32> = (0..=(last * DENSE) as usize).map(|k| k as f32 / DENSE).collect();
        let mut length = vec![0.0f32; dense.len()];
        for k in 1..dense.len() {
            let (a, b) = (point(track, dense[k - 1]), point(track, dense[k]));
            length[k] = length[k - 1] + (b[0] - a[0]).hypot(b[1] - a[1]);
        }
        let n = (length.last().copied().unwrap_or(0.0) / STEP).floor() as usize + 1;
        let us: Vec<f32> = (0..n)
            .map(|i| {
                let l = i as f32 * STEP;
                let k = length.partition_point(|&x| x <= l).clamp(1, length.len() - 1);
                let t = ((l - length[k - 1]) / (length[k] - length[k - 1]).max(1e-6)).clamp(0.0, 1.0);
                dense[k - 1] + t * (dense[k] - dense[k - 1])
            })
            .collect();
        let at: Vec<f32> = us.iter().map(|&u| distance(values, u)).collect();
        let centre: Vec<[f32; 3]> = us.iter().map(|&u| point(track, u)).collect();
        let left: Vec<[f32; 2]> = (0..n)
            .map(|i| {
                let (a, b) = (centre[i.saturating_sub(1)], centre[(i + 1).min(n - 1)]);
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let l = dx.hypot(dy).max(1e-6);
                [-dy / l, dx / l]
            })
            .collect();
        let scanned: Vec<[f32; 2]> = (0..n).map(|i| ground.edges(centre[i], left[i])).collect();
        // The scan's 0.25 m steps and the mesh's jags would kink a line laid along an edge: each edge is the least
        // over `EDGE_SPAN` stations either side, then their mean.
        let window = |i: usize| i.saturating_sub(EDGE_SPAN)..(i + EDGE_SPAN + 1).min(n);
        let least: Vec<[f32; 2]> = (0..n)
            .map(|i| std::array::from_fn(|k| scanned[window(i)].iter().map(|e| e[k]).fold(f32::MAX, f32::min)))
            .collect();
        let edges: Vec<[f32; 2]> = (0..n)
            .map(|i| {
                std::array::from_fn(|k| least[window(i)].iter().map(|e| e[k]).sum::<f32>() / window(i).len() as f32)
            })
            .collect();
        let mut lo: Vec<f32> = edges.iter().map(|e| (MARGIN - e[1]).min(0.0)).collect();
        let mut hi: Vec<f32> = edges.iter().map(|e| (e[0] - MARGIN).max(0.0)).collect();
        // A prop shuts its side: the line passes it `CLEAR` m clear, on the side nearer the driveline if it straddles it.
        for p in props {
            let [a, b] = [p.left[0] - CLEAR, p.left[1] + CLEAR];
            for i in (0..n).filter(|&i| at[i] >= p.from - PASSED && at[i] <= p.to + PASSED) {
                if a >= 0.0 || (b > 0.0 && a > -b) {
                    hi[i] = hi[i].min(a.max(lo[i]));
                } else {
                    lo[i] = lo[i].max(b.min(hi[i]));
                }
            }
        }
        let offset = least_bend(&centre, &left, &lo, &hi);
        let points: Vec<[f32; 3]> = (0..n)
            .map(|i| [centre[i][0] + offset[i] * left[i][0], centre[i][1] + offset[i] * left[i][1], centre[i][2]])
            .collect();
        let (speed, curvature) = plan(&points, &at, limits, top, zones);
        Line { at, points, speed, curvature, offset, edges, slip: limits.slip, tracker: Tracker::default() }
    }

    /// The line's point at driveline distance `s` (interpolated between stations).
    pub fn point(&self, s: f32) -> [f32; 3] {
        let (i, t) = self.index(s);
        let (a, b) = (self.points[i], self.points[(i + 1).min(self.points.len() - 1)]);
        std::array::from_fn(|k| a[k] + t * (b[k] - a[k]))
    }

    /// The plan's speed at driveline distance `s`.
    pub fn speed(&self, s: f32) -> f32 {
        lerp(&self.speed, self.index(s))
    }

    /// The line's signed curvature at driveline distance `s`.
    fn curvature_at(&self, s: f32) -> f32 {
        lerp(&self.curvature, self.index(s))
    }

    /// The line's heading (rad) at driveline distance `s`.
    fn heading_at(&self, s: f32) -> f32 {
        let (p, q) = (self.point(s - 1.0), self.point(s + 1.0));
        (q[1] - p[1]).atan2(q[0] - p[0])
    }

    /// How far left of the line the car at `at` is (m, negative right) at driveline distance `s`.
    fn left_of(&self, s: f32, at: [f32; 3]) -> f32 {
        let (h, c) = (self.heading_at(s), self.point(s));
        h.cos() * (at[1] - c[1]) - h.sin() * (at[0] - c[0])
    }

    /// A loose surface (gravel, snow): its best braking slip is over `LOOSE_SLIP`. A tyre whose force peaks at a large
    /// slip also drifts at the limit, so the race steers with the `Tracker` and eases the throttle in a drift; on tarmac
    /// it steers with the default's law on the line, which countersteers all sideslip (on RSF 562 the `Tracker` cost
    /// 11 s under `ngp` and spun under the brush, the throttle's ease 3 s).
    pub fn loose(&self) -> bool {
        self.slip > LOOSE_SLIP
    }

    /// The steering for the car at driveline distance `s` after `dt` s (`Tracker`).
    pub fn steer(&mut self, s: f32, pose: &CarPose, dt: f32) -> f32 {
        let mut tracker = self.tracker;
        let steer = tracker.steer(self, s, pose, dt);
        self.tracker = tracker;
        steer
    }

    /// A line on the line: its length against the driveline's, the road's width and the line's offset, the plan's time.
    pub fn summary(&self, limits: Limits) -> String {
        let pct = |v: &mut Vec<f32>, q: f32| {
            v.sort_by(f32::total_cmp);
            v.get(((v.len() as f32 - 1.0) * q) as usize).copied().unwrap_or(0.0)
        };
        let mut width: Vec<f32> = self.edges.iter().map(|e| e[0] + e[1]).collect();
        let mut offset: Vec<f32> = self.offset.iter().map(|o| o.abs()).collect();
        let (mut length, mut time) = (0.0, 0.0);
        for (i, w) in self.points.windows(2).enumerate() {
            let d = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
            length += d;
            time += d / (0.5 * (self.speed[i] + self.speed[i + 1])).max(1.0);
        }
        format!(
            "race line {length:.0} m over {:.0} m of driveline, road width p10/p50 {:.1}/{:.1} m, offset p50/p90 \
             {:.1}/{:.1} m; plan at {:.2}/{:.2} m/s² (lateral/braking), {time:.1} s without acceleration limits",
            STEP * (self.points.len() - 1) as f32,
            pct(&mut width, 0.1),
            pct(&mut width, 0.5),
            pct(&mut offset, 0.5),
            pct(&mut offset, 0.9),
            limits.lateral,
            limits.braking,
        )
    }

    fn index(&self, s: f32) -> (usize, f32) {
        let i = self.at.partition_point(|&a| a <= s).clamp(1, self.at.len().max(2) - 1) - 1;
        let span = self.at.get(i + 1).map_or(1.0, |&b| b - self.at[i]).max(1e-6);
        (i, ((s - self.at[i]) / span).clamp(0.0, 1.0))
    }
}

fn lerp(v: &[f32], (i, t): (usize, f32)) -> f32 {
    v[i] + t * (v[(i + 1).min(v.len() - 1)] - v[i])
}

/// The offsets along `left` from `centre` within `lo..=hi` that minimise the line's summed squared second differences
/// (FISTA, step 1/32: the operator's norm is at most 16, the gradient twice it).
fn least_bend(centre: &[[f32; 3]], left: &[[f32; 2]], lo: &[f32], hi: &[f32]) -> Vec<f32> {
    let n = centre.len();
    let (mut x, mut y) = (vec![0.0f32; n], vec![0.0f32; n]);
    let mut t = 1.0f32;
    let (mut a, mut g) = (vec![[0.0f32; 2]; n], vec![0.0f32; n]);
    for _ in 0..ITERATIONS {
        let p = |i: usize| [centre[i][0] + y[i] * left[i][0], centre[i][1] + y[i] * left[i][1]];
        for (i, ai) in a.iter_mut().enumerate().take(n.saturating_sub(1)).skip(1) {
            let (q, r, s) = (p(i - 1), p(i), p(i + 1));
            *ai = [q[0] - 2.0 * r[0] + s[0], q[1] - 2.0 * r[1] + s[1]];
        }
        for i in 0..n {
            let at = |j: usize| if j < n { a[j] } else { [0.0; 2] };
            let prev = if i > 0 { at(i - 1) } else { [0.0; 2] };
            let (m, next) = (at(i), at(i + 1));
            let d = [prev[0] - 2.0 * m[0] + next[0], prev[1] - 2.0 * m[1] + next[1]];
            g[i] = 2.0 * (d[0] * left[i][0] + d[1] * left[i][1]);
        }
        let t2 = (1.0 + (1.0 + 4.0 * t * t).sqrt()) / 2.0;
        for i in 0..n {
            let new = (y[i] - g[i] / 32.0).clamp(lo[i], hi[i]);
            y[i] = (new + (t - 1.0) / t2 * (new - x[i])).clamp(lo[i], hi[i]);
            x[i] = new;
        }
        t = t2;
    }
    x
}

/// The speed plan on `points`: each station's curve limit √(lateral / κ), at most `top`, and from the end back the
/// braking reachable within the friction ellipse (the braking share left by the next station's cornering). A station
/// whose driveline distance `at` lies in one of `zones` takes the zone's speed in place of its curve limit, and
/// leaves the braking whole: its speed is the test's, not the plan's share of the grip.
fn plan(points: &[[f32; 3]], at: &[f32], limits: Limits, top: f32, zones: &[SpeedZone]) -> (Vec<f32>, Vec<f32>) {
    let n = points.len();
    let length = |a: [f32; 3], b: [f32; 3]| (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-3);
    let heading = |a: [f32; 3], b: [f32; 3]| (b[1] - a[1]).atan2(b[0] - a[0]);
    let mut curvature = vec![0.0f32; n];
    for i in 1..n.saturating_sub(1) {
        let (a, b, c) = (points[i - 1], points[i], points[i + 1]);
        let mut turn = heading(b, c) - heading(a, b);
        turn = (turn + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        curvature[i] = turn / (0.5 * (length(a, b) + length(b, c)));
    }
    // A binomial mean over nine stations (σ 2.8 m) takes out the stations' rounding.
    const WEIGHTS: [f32; 9] = [1.0, 8.0, 28.0, 56.0, 70.0, 56.0, 28.0, 8.0, 1.0];
    let smooth: Vec<f32> = (0..n)
        .map(|i| {
            let at = |k: usize| curvature[(i + k).saturating_sub(4).min(n - 1)];
            (0..9).map(|k| WEIGHTS[k] * at(k)).sum::<f32>() / 256.0
        })
        .collect();
    let zone = |i: usize| zones.iter().find(|z| z.from <= at[i] && at[i] < z.to);
    let mut speed: Vec<f32> = (0..n)
        .map(|i| zone(i).map_or((limits.lateral / smooth[i].abs().max(1e-5)).sqrt(), |z| z.speed).min(top))
        .collect();
    for i in (0..n.saturating_sub(1)).rev() {
        let v = speed[i + 1];
        let used = if zone(i + 1).is_some() { 0.0 } else { (v * v * smooth[i + 1].abs() / limits.lateral).min(1.0) };
        let left = (1.0 - used * used).sqrt();
        speed[i] = speed[i].min((v * v + 2.0 * limits.braking * left * length(points[i], points[i + 1])).sqrt());
    }
    (speed, smooth)
}

/// How far the car is from the line (m), horizontally, at driveline distance `s`.
pub fn off_line(line: &Line, s: f32, at: [f32; 3]) -> f32 {
    let p = line.point(s);
    (at[0] - p[0]).hypot(at[1] - p[1])
}

/// The race pedals (throttle, brake) for `gap`, the plan's speed less the car's (m/s), at sideslip `beta` (rad): full
/// throttle `FULL` m/s under the plan, easing to `HOLD` on it, none within `COAST` over it, then the brake, full
/// `BRAKE_FULL` m/s further (the threshold controller caps it). On a `loose` surface the throttle eases past
/// `DRIFT_THROTTLE` rad of sideslip, to `THROTTLE_MIN` `DRIFT_SPAN` further (full throttle out of gravel's bends
/// pushed the drifting car off RSF 524); the brake eases past `SLIDE` rad, to none `SLIDE_OFF` further (braked into a
/// left-right on RSF 562 at 7490 m the brush's tail stepped out, sideslip 0.1 to 0.7 rad in 1 s, and the car spun).
pub fn pedals(gap: f32, beta: f32, loose: bool) -> (f32, f32) {
    if gap >= 0.0 {
        let drift =
            if loose { (1.0 - (beta.abs() - DRIFT_THROTTLE) / DRIFT_SPAN).clamp(THROTTLE_MIN, 1.0) } else { 1.0 };
        ((HOLD + (1.0 - HOLD) * gap / FULL).min(1.0) * drift, 0.0)
    } else if gap > -COAST {
        (0.0, 0.0)
    } else {
        let settle = (1.0 - (beta.abs() - SLIDE) / SLIDE_OFF).clamp(0.0, 1.0);
        (0.0, ((-gap - COAST) / BRAKE_FULL).min(1.0) * settle)
    }
}

const FULL: f32 = 1.0;
const HOLD: f32 = 0.3;
const COAST: f32 = 0.3;
const BRAKE_FULL: f32 = 1.5;
const DRIFT_THROTTLE: f32 = 0.06;
const DRIFT_SPAN: f32 = 0.08;
const THROTTLE_MIN: f32 = 0.2;
const SLIDE: f32 = 0.08;
const SLIDE_OFF: f32 = 0.1;

/// The race steering on a loose surface (ours): the path curvature wanted is the line's `LEAD` s ahead less a correction on the car's
/// distance from the line and its course's angle to it over a preview of `PREVIEW` s (at least `PREVIEW_MIN` m:
/// 1 / preview² per m, 2 / preview per rad, critically damped on the path), and the steering is that curvature over
/// the car's gain, the path curvature per unit of steering, learnt on the run per `BIN` m/s of speed over `LEARN` s
/// (from `GAIN0` / (1 + (v / `GAIN_SPEED`)²), RSF 562's tarmac; RSF 524's gravel is about 0.7 of it). Past `DRIFT`
/// rad of sideslip it countersteers at the default's `STEER_GAIN` per rad. The default's law steers on the nose's
/// bearing to its target, which countersteers every degree of sideslip: in gravel's steady drift at the limit
/// (sideslip 0.1-0.18 rad on RSF 524) it held the steering at 0.2-0.4 while the car ran wide off the road.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Tracker {
    gains: [f32; 13],
    course: Option<f32>,
    /// The course's curvature (1/m), filtered over `TURN_TAU` s, and the last steering.
    turn: f32,
    last: f32,
}

const LEAD: f32 = 0.1;
const PREVIEW: f32 = 0.5;
const PREVIEW_MIN: f32 = 8.0;
const BIN: f32 = 5.0;
const GAIN0: f32 = 0.13;
const GAIN_SPEED: f32 = 35.0;
const LEARN: f32 = 1.0;
const TURN_TAU: f32 = 0.05;
const DRIFT: f32 = 0.15;

impl Tracker {
    fn steer(&mut self, line: &Line, s: f32, pose: &CarPose, dt: f32) -> f32 {
        let [vx, vy, _] = pose.velocity;
        let v = vx.hypot(vy).max(1.0);
        let course = vy.atan2(vx);
        let wrap = |a: f32| (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        if let Some(last) = self.course {
            self.turn += (wrap(course - last) / (v * dt) - self.turn) * (dt / TURN_TAU).min(1.0);
        }
        self.course = Some(course);
        let bin = ((v / BIN) as usize).min(self.gains.len() - 1);
        if self.gains[bin] == 0.0 {
            self.gains[bin] = GAIN0 / (1.0 + ((bin as f32 + 0.5) * BIN / GAIN_SPEED).powi(2));
        }
        let preview = (PREVIEW * v).max(PREVIEW_MIN);
        let (e, psi) = (line.left_of(s, pose.position), wrap(course - line.heading_at(s)));
        let want = line.curvature_at(s + LEAD * v) - e / (preview * preview) - 2.0 * psi / preview;
        let beta = sideslip(pose);
        let past = beta.signum() * (beta.abs() - DRIFT).max(0.0);
        // Left (counter-clockwise) curvature is negative steering.
        let steer = (-want / self.gains[bin] - super::STEER_GAIN * past).clamp(-1.0, 1.0);
        // The gain from the steering held in a bend: the last step's steering against the course's curvature.
        if self.last.abs() > 0.05 && self.last.abs() < 0.9 && self.turn.abs() > 0.004 && beta.abs() < 0.25 {
            let gain = (-self.turn / self.last).clamp(0.02, 0.3);
            self.gains[bin] += (gain - self.gains[bin]) * (dt / LEARN).min(1.0);
        }
        self.last = steer;
        steer
    }
}

/// The body sideslip (rad, the ground velocity's angle from the forward axis, counter-clockwise positive; 0 under
/// 0.5 m/s).
pub fn sideslip(pose: &CarPose) -> f32 {
    let f = pose.forward();
    let [vx, vy, _] = pose.velocity;
    if vx.hypot(vy) < 0.5 {
        return 0.0;
    }
    (f[0] * vy - f[1] * vx).atan2(f[0] * vx + f[1] * vy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_takes_the_curve_limit_and_brakes_into_it() {
        // 400 m straight, then a 50 m radius left turn of 200 m.
        let mut points: Vec<[f32; 3]> = (0..200).map(|i| [i as f32 * STEP, 0.0, 0.0]).collect();
        points.extend((0..100).map(|i| {
            let a = i as f32 * STEP / 50.0;
            [400.0 + 50.0 * a.sin(), 50.0 - 50.0 * a.cos(), 0.0]
        }));
        let limits = Limits { lateral: 8.0, braking: 6.0, slip: 0.5 };
        let at: Vec<f32> = (0..points.len()).map(|i| i as f32 * STEP).collect();
        let (speed, curvature) = plan(&points, &at, limits, TOP, &[]);
        assert!((curvature[250] - 0.02).abs() < 1e-4, "{}", curvature[250]);
        assert!((speed[250] - 20.0).abs() < 0.05, "{}", speed[250]);
        // Braking at 6 m/s² on the straight: 200 m before the bend √(20² + 2·6·200) (the smoothed curvature eases in
        // over 8 m), the top from 267 m.
        assert_eq!(speed[10], TOP);
        assert!((speed[100] - 2800f32.sqrt()).abs() < 0.5, "{}", speed[100]);
        // `--speed-zone 380,520,30`: the bend's first 120 m at 30 m/s, over its limit of 20. The plan brakes for the
        // zone from the straight, and inside its end (the last station at 518 m) for the bend's limit after it, both
        // at the whole 6 m/s².
        let zone = SpeedZone { from: 380.0, to: 520.0, speed: 30.0 };
        let (zoned, _) = plan(&points, &at, limits, TOP, &[zone]);
        assert_eq!(zoned[190], 30.0);
        assert_eq!(zoned[230], 30.0);
        assert!((zoned[100] - (900.0f32 + 12.0 * 180.0).sqrt()).abs() < 0.05, "{}", zoned[100]);
        assert!((zoned[255] - (400.0f32 + 12.0 * 8.0).sqrt()).abs() < 0.05, "{}", zoned[255]);
        assert!((zoned[270] - 20.0).abs() < 0.05, "{}", zoned[270]);
    }

    #[test]
    fn the_pedals_ease_the_throttle_in_a_drift_only_on_loose_surfaces() {
        assert_eq!(pedals(2.0, 0.0, true), (1.0, 0.0));
        assert_eq!(pedals(0.0, 0.0, false), (HOLD, 0.0));
        assert_eq!(pedals(-0.2, 0.0, false), (0.0, 0.0));
        assert_eq!(pedals(-1.8, 0.0, false), (0.0, 1.0));
        assert_eq!(pedals(2.0, 0.3, false), (1.0, 0.0));
        assert_eq!(pedals(2.0, 0.3, true), (THROTTLE_MIN, 0.0));
        // The brake lets the car settle past `SLIDE` rad of sideslip, either surface.
        assert_eq!(pedals(-1.8, SLIDE + SLIDE_OFF, false), (0.0, 0.0));
    }
}
