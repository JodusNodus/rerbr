//! The collision under the wheel tracks of a prepared stage (`btb-converter prepare --track FOLDER --out DIR --maps DIR`),
//! as NGP's wheel contact meets it: along the driveline, 0.75 m either side, every 5 cm, the top surface near the last
//! height. Creases: the change of the face normal where a track crosses from one face to the next (median, 99th
//! percentile, those over 1° per km), and the lag-1 correlation of the slope changes there (-0.5 or below: a
//! sawtooth, each crease undone by the next; near 0 or above: the road's own shape). Steps: the two faces' heights
//! apart at the crossing. Stacked: another surface 0.5 to 50 mm under the top. Also the driveline's turns over 37°
//! between points (a line doubling back) and the fullest collision cell (over 65536 corners its 16-bit index wraps).

use rbr_formats::{col, trk};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Step along a track, metres.
const STEP: f64 = 0.05;
/// The tracks' offsets from the driveline, metres.
const SIDES: [f64; 2] = [-0.75, 0.75];
/// Plan bins of the faces, metres.
const BIN: f64 = 4.0;
/// Corners of a cell before their 16-bit index wraps.
const CELL_CORNERS: usize = 0x1_0000;

/// A sample of a track: the top surface's height and face (none where no surface is near), its driveline segment
/// and its plan position.
type Sample = (Option<(f64, usize)>, usize, [f64; 2]);

/// A face that is not a wall, its unit normal facing up (the stage files are z up).
struct Face {
    p: [[f64; 3]; 3],
    n: [f64; 3],
}

impl Face {
    /// Barycentric weights of plan point `(x, y)` in the face's plane.
    fn weights(&self, x: f64, y: f64) -> Option<[f64; 3]> {
        let [a, b, c] = self.p;
        let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if d.abs() < 1e-12 {
            return None;
        }
        let l0 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d;
        let l1 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d;
        Some([l0, l1, 1.0 - l0 - l1])
    }

    fn plane(&self, x: f64, y: f64) -> f64 {
        self.weights(x, y).map_or(f64::NAN, |l| l[0] * self.p[0][2] + l[1] * self.p[1][2] + l[2] * self.p[2][2])
    }

    fn height(&self, x: f64, y: f64) -> Option<f64> {
        self.weights(x, y).filter(|l| l.iter().all(|&v| v >= -1e-7)).map(|_| self.plane(x, y))
    }
}

fn find(dir: &Path, ext: &str) -> Result<PathBuf, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    entries
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == ext))
        .ok_or_else(|| format!("no .{ext} in {}", dir.display()))
}

pub fn run(dir: &Path) -> Result<(), String> {
    let read = |p: PathBuf| std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()));
    let col = col::parse(&read(find(dir, "col")?)?).map_err(|e| format!("col: {e}"))?;
    let trk = trk::parse(&read(find(dir, "trk")?)?).map_err(|e| format!("trk: {e}"))?;
    let line: Vec<[f64; 3]> = trk
        .chunks
        .iter()
        .find_map(|c| match &c.payload {
            trk::ChunkPayload::DriveLine(d) => Some(d),
            _ => None,
        })
        .ok_or("no driveline")?
        .records
        .iter()
        .map(|r| [r.vectors[0].x, r.vectors[0].y, r.vectors[0].z].map(f64::from))
        .collect();

    let fullest = col.cells.iter().max_by_key(|c| c.vertices.len());
    let (corners, faces) = fullest.map_or((0, 0), |c| (c.vertices.len(), c.tree.triangles.len()));
    println!("collision cells: {}, the fullest {corners} corners of {CELL_CORNERS} ({faces} faces)", col.cells.len());
    let mut at = 0.0;
    for w in line.windows(3) {
        let (u, v) = ([w[1][0] - w[0][0], w[1][1] - w[0][1]], [w[2][0] - w[1][0], w[2][1] - w[1][1]]);
        at += u[0].hypot(u[1]);
        let cos = (u[0] * v[0] + u[1] * v[1]) / (u[0].hypot(u[1]) * v[0].hypot(v[1])).max(1e-9);
        if cos < 0.8 {
            println!("driveline turns {:.0}° at {at:.1} m", cos.clamp(-1.0, 1.0).acos().to_degrees());
        }
    }

    let mut faces = Vec::new();
    for cell in &col.cells {
        for t in &cell.tree.triangles {
            let p = t.vertices.map(|i| cell.vertices[i as usize].map(f64::from));
            let (u, v) = ([0, 1, 2].map(|k| p[1][k] - p[0][k]), [0, 1, 2].map(|k| p[2][k] - p[0][k]));
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if l > 1e-12 && (n[2] / l).abs() >= 0.5 {
                faces.push(Face { p, n: n.map(|x| x / l * n[2].signum()) });
            }
        }
    }
    let mut bins: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
    for (i, f) in faces.iter().enumerate() {
        let lo = |k: usize| f.p.iter().map(|p| p[k]).fold(f64::MAX, f64::min);
        let hi = |k: usize| f.p.iter().map(|p| p[k]).fold(f64::MIN, f64::max);
        let c = |v: f64| (v / BIN).floor() as i64;
        for x in c(lo(0))..=c(hi(0)) {
            for y in c(lo(1))..=c(hi(1)) {
                bins.entry((x, y)).or_default().push(i as u32);
            }
        }
    }

    for side in SIDES {
        let mut track: Vec<Sample> = Vec::new();
        let (mut stacked, mut last) = (0usize, None::<f64>);
        for (segment, w) in line.windows(2).enumerate() {
            let (a, b) = (w[0], w[1]);
            let len = (b[0] - a[0]).hypot(b[1] - a[1]);
            if len < 1e-6 {
                continue;
            }
            let normal = [-(b[1] - a[1]) / len, (b[0] - a[0]) / len];
            for k in 0..(len / STEP).round() as usize {
                let f = k as f64 * STEP / len;
                let (x, y) = (a[0] + (b[0] - a[0]) * f + normal[0] * side, a[1] + (b[1] - a[1]) * f + normal[1] * side);
                let near = last.unwrap_or(a[2] + (b[2] - a[2]) * f);
                let key = ((x / BIN).floor() as i64, (y / BIN).floor() as i64);
                let mut hits: Vec<(f64, usize)> = bins
                    .get(&key)
                    .into_iter()
                    .flatten()
                    .filter_map(|&i| faces[i as usize].height(x, y).map(|h| (h, i as usize)))
                    .filter(|h| (h.0 - near).abs() < 1.5)
                    .collect();
                hits.sort_by(|p, q| q.0.total_cmp(&p.0));
                let top = hits.first().copied();
                if top.is_some_and(|(h, _)| hits[1..].iter().any(|o| h - o.0 > 0.0005 && h - o.0 < 0.05)) {
                    stacked += 1;
                }
                last = top.map(|t| t.0);
                track.push((top, segment, [x, y]));
            }
        }
        let (mut creases, mut steps, mut slope_changes) = (Vec::new(), Vec::new(), Vec::new());
        for i in 2..track.len().saturating_sub(3) {
            let (Some((_, f)), Some((_, g))) = (track[i].0, track[i + 1].0) else { continue };
            if f == g {
                continue;
            }
            let (p, q) = (faces[f].n, faces[g].n);
            creases.push((p[0] * q[0] + p[1] * q[1] + p[2] * q[2]).clamp(-1.0, 1.0).acos().to_degrees());
            let mid = [(track[i].2[0] + track[i + 1].2[0]) / 2.0, (track[i].2[1] + track[i + 1].2[1]) / 2.0];
            steps.push((faces[f].plane(mid[0], mid[1]) - faces[g].plane(mid[0], mid[1])).abs());
            let h = |j: usize| track[j].0.map(|t| t.0);
            if let (Some(a), Some(b), Some(c), Some(d)) = (h(i - 2), h(i), h(i + 1), h(i + 3)) {
                if track[i - 2].1 == track[i + 3].1 {
                    slope_changes.push((d - c) / (2.0 * STEP) - (b - a) / (2.0 * STEP));
                }
            }
        }
        let km = track.len() as f64 * STEP / 1000.0;
        let missing = track.iter().filter(|t| t.0.is_none()).count();
        let per_km = |v: &[f64], over: f64| v.iter().filter(|&&x| x > over).count() as f64 / km;
        let pct = |v: &mut Vec<f64>, q: f64| {
            v.sort_by(f64::total_cmp);
            v.get(((v.len().max(1) - 1) as f64 * q) as usize).copied().unwrap_or(f64::NAN)
        };
        let mean = slope_changes.iter().sum::<f64>() / slope_changes.len().max(1) as f64;
        let spread = slope_changes.iter().map(|v| (v - mean).powi(2)).sum::<f64>();
        let lag = slope_changes.windows(2).map(|w| (w[0] - mean) * (w[1] - mean)).sum::<f64>();
        println!(
            "{side:+} m: {km:.1} km, no surface {:.1} %, stacked {:.2} %; creases median {:.2}°, p99 {:.2}°, over 1° \
             {:.0}/km, lag-1 correlation {:.2}; steps p99 {:.1} mm, over 2 mm {:.0}/km",
            missing as f64 * 100.0 / track.len().max(1) as f64,
            stacked as f64 * 100.0 / track.len().max(1) as f64,
            pct(&mut creases, 0.5),
            pct(&mut creases, 0.99),
            per_km(&creases, 1.0),
            lag / spread.max(1e-30),
            pct(&mut steps, 0.99) * 1000.0,
            per_km(&steps, 0.002),
        );
    }
    Ok(())
}
