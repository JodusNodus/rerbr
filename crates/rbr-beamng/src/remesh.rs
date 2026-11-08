//! Road re-mesh for a converted stage's collision. Some levels build their roads from coarse strips that crease where
//! NGP's wheel contact feels it: 2K-Irohazaka's road is cross-sections about 2 m apart with a 0.64° crease at the
//! median triangle crossing (2.5 times retail's), each crease mostly undone by the next (lag-1 correlation -0.67:
//! noise and twist, not the road's shape). BeamNG's soft tyres ride over it. Moving the level's vertices onto a smooth
//! surface leaves most of it (0.58°: the creases sit between them), so the band along the driveline is sampled from
//! the level's collision every [`FINE`] along and on columns across, out to [`HALF`] or the first break across (a
//! barrier, a ditch), each column is smoothed along the road (a local linear fit under Gaussian weights), and the band
//! is written back as strips whose sections lie no farther apart than the heights and the plan allow. The level's
//! faces under the band go; those its edge crosses stay, their corners within it moved onto it. Camber, crown and
//! kerbs (across) stay, as do crests and dips (along) longer than a few metres.

use crate::road::Line;
use crate::world::{Grid, Triangle};
use std::collections::HashMap;
use std::f64::consts::PI;

/// Sampling step along the driveline, metres.
const FINE: f64 = 0.25;
/// Columns each side of the driveline; [`COLUMNS`] across with the centre.
const SIDE: usize = 7;
const COLUMNS: usize = 2 * SIDE + 1;
/// The band's half width, metres: the whole road wherever the driveline runs on it (the levels' AI road widths, 2.5 to
/// 5 m, fall short of it).
const HALF: f64 = 7.0;
/// Level faces within this plan distance of the driveline can be under the band, metres.
pub const REACH: f64 = HALF + 2.0;
/// A section's direction: the chord from this far behind to this far ahead, metres.
const TANGENT: f64 = 2.5;
/// The inside of a curve keeps within this share of its radius (further, sections would cross).
const INSIDE: f64 = 0.75;
/// Smoothing along a column: the Gaussian sigma of the local linear fit, metres.
const SIGMA: f64 = 2.0;
/// The centre sample's surface: within `NEAR` (and `GROW` more per metre since the last one found, at most `FIRST`)
/// of the height the last one kept to the driveline, within `FIRST` until one is found, metres.
const NEAR: f64 = 0.5;
const GROW: f64 = 0.1;
const FIRST: f64 = 2.0;
/// Outward, each sample's surface rises or falls at most this slope from its neighbour's, plus `STEP_UP` (a kerb
/// stays, a barrier's top or a ditch ends the band there).
const ACROSS_SLOPE: f64 = 0.15;
const STEP_UP: f64 = 0.05;
/// The band's edge each side: its narrowest within this distance along, metres.
const EVEN: f64 = 2.0;
/// Written sections: no point between two of them further than this from their chord (heights, plan), metres.
const TOL_HEIGHT: f64 = 0.002;
const TOL_PLAN: f64 = 0.05;
/// Written sections lie at most this far apart, metres.
const MAX_STEP: f64 = 3.0;
/// A level face is replaced when every test point on it lies over a written cell, within this of its height, metres.
const DROP: f64 = 0.25;
/// Test points on a level face, at most this far apart, metres.
const PROBE: f64 = 0.5;
/// A level face is replaced only this far inside the band's side edges, metres: a written face's edge follows the
/// chord of the fine points, up to TOL_PLAN inside them.
const EDGE: f64 = 0.15;
/// Plan bins of the band's fine strips, metres.
const BIN: f64 = 4.0;

/// A sample of the level's surface: its height, material, and whether its face winds clockwise (normal down).
type Sample = Option<(f64, Option<u32>, bool)>;

struct Section {
    s: f64,
    centre: [f64; 3],
    /// Plan unit normal, to the left of travel.
    normal: [f64; 2],
    /// Column offsets along the normal, ascending.
    offsets: [f64; COLUMNS],
}

impl Section {
    fn point(&self, j: usize) -> [f64; 2] {
        let t = self.offsets[j];
        [self.centre[0] + self.normal[0] * t, self.centre[1] + self.normal[1] * t]
    }
}

/// The re-meshed road band.
pub struct Surface {
    sections: Vec<Section>,
    /// Smoothed heights per fine section and column; `None` where the level has no surface for the band.
    heights: Vec<[Option<f64>; COLUMNS]>,
    /// Whether the cell between fine sections `i`, `i + 1` and columns `j`, `j + 1` lies under a written face.
    covered: Vec<[bool; COLUMNS - 1]>,
    /// Whether it replaces the level: covered, as are the cells before and after it (beside the band's edges, see
    /// [`EDGE`]).
    inner: Vec<[bool; COLUMNS - 1]>,
    bins: HashMap<(i64, i64), Vec<u32>>,
    /// The band's faces, wound like the level's faces they replace, each with the material of the level's face under
    /// its centroid.
    pub triangles: Vec<Triangle>,
    /// Written sections.
    pub written: usize,
    /// Samples taken and those with no surface.
    pub samples: usize,
    pub missing: usize,
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

/// Barycentric weights of `p` in the plan triangle `abc`, if inside.
fn inside(p: [f64; 2], a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<[f64; 3]> {
    let d = cross(sub(b, a), sub(c, a));
    if d.abs() < 1e-12 {
        return None;
    }
    let l1 = cross(sub(p, a), sub(c, a)) / d;
    let l2 = cross(sub(b, a), sub(p, a)) / d;
    let l0 = 1.0 - l1 - l2;
    let eps = -1e-9;
    (l0 >= eps && l1 >= eps && l2 >= eps).then_some([l0, l1, l2])
}

/// The re-meshed band along `line` over the level's collision `ground`.
pub fn build(line: &Line, ground: &Grid) -> Surface {
    let length = line.length();
    let n = (length / FINE).floor() as usize + 1;
    let mut sections = Vec::with_capacity(n);
    let mut heading = Vec::with_capacity(n);
    for i in 0..n {
        let s = (i as f64 * FINE).min(length);
        let (a, b) = (line.sample((s - TANGENT).max(0.0)).0, line.sample((s + TANGENT).min(length)).0);
        let d = [b[0] - a[0], b[1] - a[1]];
        let l = d[0].hypot(d[1]).max(1e-9);
        heading.push(d[1].atan2(d[0]));
        sections.push(Section { s, centre: line.sample(s).0, normal: [-d[1] / l, d[0] / l], offsets: [0.0; COLUMNS] });
    }
    // Curvature, positive turning left (the centre on the normal's side), and each side's reach.
    let curvature: Vec<f64> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(1), (i + 1).min(n - 1));
            let turn = (heading[b] - heading[a] + PI).rem_euclid(2.0 * PI) - PI;
            if b > a {
                turn / (sections[b].s - sections[a].s).max(1e-9)
            } else {
                0.0
            }
        })
        .collect();
    let window = (HALF / FINE).ceil() as usize;
    for i in 0..n {
        let (mut left, mut right) = (HALF, HALF);
        for &c in &curvature[i.saturating_sub(window)..=(i + window).min(n - 1)] {
            if c > 0.0 {
                left = left.min(INSIDE / c);
            } else if c < 0.0 {
                right = right.min(INSIDE / -c);
            }
        }
        for (j, o) in sections[i].offsets.iter_mut().enumerate() {
            *o = if j < SIDE { -right * (SIDE - j) as f64 } else { left * (j - SIDE) as f64 } / SIDE as f64;
        }
    }

    // Samples: the centre near the driveline's height (less the offset last found), then outward while the surface
    // stays continuous.
    let mut raw: Vec<[Sample; COLUMNS]> = vec![[None; COLUMNS]; n];
    let (mut offset, mut found, mut since) = (0.0, false, 0.0);
    for (i, sec) in sections.iter().enumerate() {
        let sample = |j: usize, z: f64, reach: f64| {
            let p = sec.point(j);
            ground
                .ground(p[0], p[1], z)
                .filter(|(h, _)| (h - z).abs() <= reach)
                .map(|(h, t)| (h, t.material, t.normal()[2] < 0.0))
        };
        let reach = if found { (NEAR + GROW * since).min(FIRST) } else { FIRST };
        let centre = sample(SIDE, sec.centre[2] + offset, reach);
        match centre {
            Some((h, ..)) => {
                offset = h - sec.centre[2];
                found = true;
                since = 0.0;
            }
            None => since += FINE,
        }
        raw[i][SIDE] = centre;
        for outward in [1i64, -1] {
            let (mut z, mut j) = (centre.map(|c| c.0), SIDE as i64 + outward);
            while let (Some(last), true) = (z, (0..COLUMNS as i64).contains(&j)) {
                let width = (sec.offsets[j as usize] - sec.offsets[(j - outward) as usize]).abs();
                let hit = sample(j as usize, last, ACROSS_SLOPE * width + STEP_UP);
                raw[i][j as usize] = hit;
                z = hit.map(|h| h.0);
                j += outward;
            }
        }
    }
    // Each side's edge held to its narrowest within EVEN along (a ragged edge would write a section at every change).
    let (outward_left, outward_right): (Vec<usize>, Vec<usize>) =
        ((SIDE..COLUMNS).collect(), (0..=SIDE).rev().collect());
    let found = |side: &[usize]| -> Vec<usize> {
        raw.iter().map(|r| side.iter().take_while(|&&j| r[j].is_some()).count()).collect()
    };
    let (left, right) = (found(&outward_left), found(&outward_right));
    let even = (EVEN / FINE).round() as usize;
    let narrowest =
        |found: &[usize], i: usize| found[i.saturating_sub(even)..=(i + even).min(n - 1)].iter().copied().min();
    for (i, r) in raw.iter_mut().enumerate() {
        let (l, rt) = (narrowest(&left, i).unwrap_or(0), narrowest(&right, i).unwrap_or(0));
        for (j, sample) in r.iter_mut().enumerate() {
            if if j >= SIDE { j - SIDE >= l } else { SIDE - j >= rt } {
                *sample = None;
            }
        }
    }
    let samples = n * COLUMNS;
    let missing = raw.iter().flatten().filter(|r| r.is_none()).count();

    // Each column smoothed along the road.
    let span = (3.0 * SIGMA / FINE).ceil() as usize;
    let mut heights = vec![[None; COLUMNS]; n];
    for i in 0..n {
        for j in 0..COLUMNS {
            if raw[i][j].is_none() {
                continue;
            }
            let (mut sw, mut sx, mut sxx, mut sz, mut sxz) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for m in i.saturating_sub(span)..=(i + span).min(n - 1) {
                let Some((z, ..)) = raw[m][j] else { continue };
                let x = sections[m].s - sections[i].s;
                let w = (-0.5 * (x / SIGMA).powi(2)).exp();
                sw += w;
                sx += w * x;
                sxx += w * x * x;
                sz += w * z;
                sxz += w * x * z;
            }
            let det = sw * sxx - sx * sx;
            heights[i][j] = Some(if det.abs() > 1e-12 * sw * sw { (sz * sxx - sx * sxz) / det } else { sz / sw });
        }
    }

    // Written sections: as far apart as the chords allow, and wherever the columns found change.
    let mask = |i: usize| raw[i].map(|r| r.is_some());
    let fits = |a: usize, c: usize| {
        let (pa, pc) = (&sections[a], &sections[c]);
        (a + 1..c).all(|i| {
            let f = (sections[i].s - pa.s) / (pc.s - pa.s).max(1e-9);
            (0..COLUMNS).filter(|&j| raw[a][j].is_some()).all(|j| {
                let (q, qa, qc) = (sections[i].point(j), pa.point(j), pc.point(j));
                let plan = [qa[0] + (qc[0] - qa[0]) * f - q[0], qa[1] + (qc[1] - qa[1]) * f - q[1]];
                let height = match (heights[a][j], heights[i][j], heights[c][j]) {
                    (Some(za), Some(z), Some(zc)) => (za + (zc - za) * f - z).abs(),
                    _ => 0.0,
                };
                plan[0].hypot(plan[1]) <= TOL_PLAN && height <= TOL_HEIGHT
            })
        })
    };
    let most = (MAX_STEP / FINE).round() as usize;
    let mut written = vec![0usize];
    let mut a = 0;
    while a + 1 < n {
        let mut b = a + 1;
        while b + 1 < n && b + 1 - a <= most && mask(b) == mask(a) && mask(b + 1) == mask(a) && fits(a, b + 1) {
            b += 1;
        }
        written.push(b);
        a = b;
    }

    // The faces, wound like most of the level's faces sampled.
    let down = raw.iter().flatten().flatten().filter(|r| r.2).count() * 2 > n * COLUMNS - missing;
    let mut covered = vec![[false; COLUMNS - 1]; n.saturating_sub(1)];
    let mut triangles = Vec::new();
    for w in written.windows(2) {
        let (a, b) = (w[0], w[1]);
        for j in 0..COLUMNS - 1 {
            let corner = |i: usize, k: usize| {
                let p = sections[i].point(k);
                heights[i][k].map(|z| [p[0], p[1], z])
            };
            let (Some(a0), Some(a1), Some(b0), Some(b1)) =
                (corner(a, j), corner(a, j + 1), corner(b, j), corner(b, j + 1))
            else {
                continue;
            };
            if b > a + 1 && (a + 1..b).any(|i| heights[i][j].is_none() || heights[i][j + 1].is_none()) {
                continue;
            }
            // Counter-clockwise in plan (along, then across); the diagonal with the flatter fold.
            let plan = |p: [f64; 3]| [p[0], p[1]];
            let area = |t: [[f64; 3]; 3]| cross(sub(plan(t[1]), plan(t[0])), sub(plan(t[2]), plan(t[0])));
            let unit = |t: [[f64; 3]; 3]| {
                Triangle { pos: t, uv: [[0.0; 2]; 3], normal: [[0.0; 3]; 3], material: None }.normal()
            };
            let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
            let one = [[a0, b0, b1], [a0, b1, a1]];
            let other = [[a0, b0, a1], [a1, b0, b1]];
            let pair = if dot(unit(one[0]), unit(one[1])) >= dot(unit(other[0]), unit(other[1])) { one } else { other };
            if pair.iter().any(|&t| area(t) <= 1e-6) {
                continue;
            }
            for t in pair {
                let c = [0, 1, 2].map(|k| (t[0][k] + t[1][k] + t[2][k]) / 3.0);
                let material = ground.ground(c[0], c[1], c[2]).and_then(|(_, t)| t.material);
                let pos = if down { [t[0], t[2], t[1]] } else { t };
                let mut t = Triangle { pos, uv: [[0.0; 2]; 3], normal: [[0.0; 3]; 3], material };
                t.normal = [t.normal().map(|v| v as f32); 3];
                triangles.push(t);
            }
            for c in &mut covered[a..b] {
                c[j] = true;
            }
        }
    }

    // Plan bins of the fine strips.
    let mut bins: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
    for i in 0..n.saturating_sub(1) {
        let points: Vec<[f64; 2]> =
            (0..COLUMNS).flat_map(|j| [sections[i].point(j), sections[i + 1].point(j)]).collect();
        let lo = |k: usize| points.iter().map(|p| p[k]).fold(f64::MAX, f64::min);
        let hi = |k: usize| points.iter().map(|p| p[k]).fold(f64::MIN, f64::max);
        let c = |v: f64| (v / BIN).floor() as i64;
        for x in c(lo(0))..=c(hi(0)) {
            for y in c(lo(1))..=c(hi(1)) {
                bins.entry((x, y)).or_default().push(i as u32);
            }
        }
    }
    let at = |i: usize, j: usize| covered.get(i).is_some_and(|c: &[bool; COLUMNS - 1]| c[j]);
    let inner: Vec<[bool; COLUMNS - 1]> = (0..covered.len())
        .map(|i| std::array::from_fn(|j| i > 0 && [(i, j), (i - 1, j), (i + 1, j)].iter().all(|&(i, j)| at(i, j))))
        .collect();
    Surface { sections, heights, inner, covered, bins, triangles, written: written.len(), samples, missing }
}

impl Surface {
    /// The band's height under a level vertex `p` (within [`DROP`] of it) where a written face lies.
    pub fn settled(&self, p: [f64; 3]) -> Option<f64> {
        self.height_near(p, p[2], false)
    }

    /// The band's height over plan point `p` near height `z` (within [`DROP`]), in a covered cell, or an inner one
    /// at least [`EDGE`] inside the band's sides.
    fn height_near(&self, p: [f64; 3], z: f64, inner: bool) -> Option<f64> {
        let q = [p[0], p[1]];
        let key = ((p[0] / BIN).floor() as i64, (p[1] / BIN).floor() as i64);
        for &i in self.bins.get(&key).into_iter().flatten() {
            let i = i as usize;
            let (s0, s1) = (&self.sections[i], &self.sections[i + 1]);
            let t = (q[0] - s0.centre[0]) * s0.normal[0] + (q[1] - s0.centre[1]) * s0.normal[1];
            let j = s0.offsets.partition_point(|&o| o <= t).clamp(1, COLUMNS - 1) - 1;
            for j in j.saturating_sub(1)..=(j + 1).min(COLUMNS - 2) {
                let side = |k: usize| k < COLUMNS - 1 && self.covered[i][k];
                let clear = (j > 0 && side(j - 1) || t >= s0.offsets[j] + EDGE)
                    && (side(j + 1) || t <= s0.offsets[j + 1] - EDGE);
                if !(if inner { self.inner[i][j] && clear } else { self.covered[i][j] }) {
                    continue;
                }
                let (a0, a1, b0, b1) = (s0.point(j), s0.point(j + 1), s1.point(j), s1.point(j + 1));
                let h = |i: usize, k: usize| self.heights[i][k].unwrap_or(f64::NAN);
                let (za0, za1, zb0, zb1) = (h(i, j), h(i, j + 1), h(i + 1, j), h(i + 1, j + 1));
                let height = inside(q, a0, b0, b1)
                    .map(|l| l[0] * za0 + l[1] * zb0 + l[2] * zb1)
                    .or_else(|| inside(q, a0, b1, a1).map(|l| l[0] * za0 + l[1] * zb1 + l[2] * za1));
                if let Some(h) = height.filter(|h| (h - z).abs() <= DROP) {
                    return Some(h);
                }
            }
        }
        None
    }

    /// Whether the band replaces the level's face `t`: every test point on it (a barycentric grid at most
    /// [`PROBE`] apart) over an inner cell, near the band's height there.
    pub fn replaces(&self, t: &Triangle) -> bool {
        let [a, b, c] = t.pos;
        let longest =
            [(a, b), (b, c), (c, a)].iter().map(|(p, q)| (q[0] - p[0]).hypot(q[1] - p[1])).fold(0.0, f64::max);
        let n = (longest / PROBE).ceil().max(1.0) as usize;
        (0..=n).all(|i| {
            (0..=n - i).all(|j| {
                let (u, v) = (i as f64 / n as f64, j as f64 / n as f64);
                let p = [0, 1, 2].map(|k| a[k] * (1.0 - u - v) + b[k] * u + c[k] * v);
                self.height_near(p, p[2], true).is_some()
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight road 8 m wide along x, its strips 2 m long and twisted: each section's camber flips (heights
    /// plus or minus 1 cm at the edges), the centre on z = 0.05 x.
    fn twisted() -> Vec<Triangle> {
        let height = |x: f64, y: f64| 0.05 * x + if (x / 2.0).round() as i64 % 2 == 0 { 0.0025 } else { -0.0025 } * y;
        let mut out = Vec::new();
        for k in 0..60 {
            for m in 0..4 {
                let (x0, x1, y0, y1) =
                    (k as f64 * 2.0 - 10.0, k as f64 * 2.0 - 8.0, m as f64 * 2.0 - 4.0, m as f64 * 2.0 - 2.0);
                let p = |x: f64, y: f64| [x, y, height(x, y)];
                for pos in [[p(x0, y0), p(x1, y1), p(x1, y0)], [p(x0, y0), p(x0, y1), p(x1, y1)]] {
                    out.push(Triangle { pos, uv: [[0.0; 2]; 3], normal: [[0.0; 3]; 3], material: Some(3) });
                }
            }
        }
        out
    }

    fn line() -> Line {
        let points: Vec<[f64; 3]> = (0..=40).map(|k| [k as f64 * 2.5, 0.0, 0.125 * k as f64]).collect();
        Line::new(points, vec![6.0; 41])
    }

    #[test]
    fn the_band_replaces_a_twisted_road_with_a_smooth_one() {
        let level = twisted();
        let grid = Grid::new(level.iter().collect(), 4.0);
        let surface = build(&line(), &grid);
        // The columns past the level's edge (4 m) find nothing.
        let beyond = (0..COLUMNS).filter(|&j| (j as f64 - SIDE as f64).abs() * HALF / SIDE as f64 > 4.0 + 1e-9).count();
        assert_eq!(surface.missing, beyond * surface.samples / COLUMNS);
        assert!(!surface.triangles.is_empty());
        // Wound like the level's faces (normal down), the level's material.
        let down = level[0].normal()[2] < 0.0;
        assert!(surface.triangles.iter().all(|t| (t.normal()[2] < 0.0) == down && t.material == Some(3)));
        // The slope survives, the twist (half a centimetre at 0.75 m) goes.
        let band = Grid::new(surface.triangles.iter().collect(), 4.0);
        for x in [20.3, 41.1, 63.7] {
            let z = band.ground(x, 0.75, 0.05 * x).unwrap().0;
            assert!((z - 0.05 * x).abs() < 0.0015, "{x}: {z}");
        }
        // Faces inside the band go, those at its edge (the level's, 4 m) stay, their corners within it moved onto it.
        let along = |t: &&Triangle| t.pos.iter().all(|p| p[0] > 5.0 && p[0] < 95.0);
        let inner = level.iter().filter(along).filter(|t| t.pos.iter().all(|p| p[1].abs() <= 2.0));
        assert!(inner.clone().count() > 0 && inner.clone().all(|t| surface.replaces(t)));
        for t in level.iter().filter(along).filter(|t| t.pos.iter().any(|p| p[1].abs() >= 4.0)) {
            assert!(!surface.replaces(t));
            for p in t.pos.iter().filter(|p| p[1].abs() <= 3.0) {
                assert!((surface.settled(*p).unwrap() - 0.05 * p[0]).abs() < 0.0015);
            }
        }
    }

    #[test]
    fn a_raised_verge_ends_the_band() {
        // The road 4 m wide, a verge 30 cm higher from 2 m to 4 m either side: the band stops at the road's edge.
        let mut level: Vec<Triangle> =
            twisted().into_iter().filter(|t| t.pos.iter().all(|p| p[1].abs() <= 2.0)).collect();
        let p = |x: f64, y: f64| [x, y, 0.05 * x + 0.3];
        for k in 0..60 {
            let (x0, x1) = (k as f64 * 2.0 - 10.0, k as f64 * 2.0 - 8.0);
            for (y0, y1) in [(-4.0, -2.0), (2.0, 4.0)] {
                for pos in [[p(x0, y0), p(x1, y1), p(x1, y0)], [p(x0, y0), p(x0, y1), p(x1, y1)]] {
                    level.push(Triangle { pos, uv: [[0.0; 2]; 3], normal: [[0.0; 3]; 3], material: Some(4) });
                }
            }
        }
        let grid = Grid::new(level.iter().collect(), 4.0);
        let surface = build(&line(), &grid);
        assert!(surface.missing > 0);
        assert!(surface.triangles.iter().all(|t| {
            t.material == Some(3) && t.pos.iter().all(|p| p[1].abs() <= 2.0 && (p[2] - 0.05 * p[0]).abs() < 0.01)
        }));
        assert!(level.iter().filter(|t| t.material == Some(4)).all(|t| !surface.replaces(t)));
    }
}
