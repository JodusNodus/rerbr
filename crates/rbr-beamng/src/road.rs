//! Centre lines: a `DecalRoad`'s nodes as a spline sampled at a fixed step (BeamNG draws AI roads through their nodes;
//! a centripetal Catmull-Rom curve through them), and the arc length of points projected onto it.

/// A polyline with its cumulative arc length.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub points: Vec<[f64; 3]>,
    /// Arc length at each point.
    pub at: Vec<f64>,
    /// Road width at each point.
    pub width: Vec<f64>,
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

impl Line {
    pub fn new(points: Vec<[f64; 3]>, width: Vec<f64>) -> Line {
        let mut at = Vec::with_capacity(points.len());
        let mut s = 0.0;
        for (i, p) in points.iter().enumerate() {
            if i > 0 {
                s += dist(points[i - 1], *p);
            }
            at.push(s);
        }
        Line { points, at, width }
    }

    pub fn length(&self) -> f64 {
        self.at.last().copied().unwrap_or(0.0)
    }

    /// The spline through `nodes` (`x, y, z, width`), sampled about every `step` metres; a looped road closes back
    /// on its first node.
    pub fn spline(nodes: &[[f64; 4]], looped: bool, step: f64) -> Line {
        let n = nodes.len();
        let mut points = Vec::new();
        let mut width = Vec::new();
        if n < 2 {
            return Line::new(nodes.iter().map(|p| [p[0], p[1], p[2]]).collect(), nodes.iter().map(|p| p[3]).collect());
        }
        let get = |i: isize| -> [f64; 4] {
            if looped {
                nodes[i.rem_euclid(n as isize) as usize]
            } else {
                nodes[i.clamp(0, n as isize - 1) as usize]
            }
        };
        let spans = if looped { n } else { n - 1 };
        for i in 0..spans as isize {
            let (p0, p1, p2, p3) = (get(i - 1), get(i), get(i + 1), get(i + 2));
            let xyz = |p: [f64; 4]| [p[0], p[1], p[2]];
            let len = dist(xyz(p1), xyz(p2));
            let count = (len / step).ceil().max(1.0) as usize;
            for k in 0..count {
                let t = k as f64 / count as f64;
                points.push(catmull_rom(xyz(p0), xyz(p1), xyz(p2), xyz(p3), t));
                width.push(p1[3] + (p2[3] - p1[3]) * t);
            }
        }
        let last = if looped { nodes[0] } else { nodes[n - 1] };
        points.push([last[0], last[1], last[2]]);
        width.push(last[3]);
        Line::new(points, width)
    }

    /// The nearest point of the line to `p`: (arc length, distance).
    pub fn project(&self, p: [f64; 3]) -> (f64, f64) {
        let mut best = (0.0, f64::MAX);
        for i in 1..self.points.len() {
            let (a, b) = (self.points[i - 1], self.points[i]);
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let len2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
            let t = if len2 > 0.0 {
                (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1] + (p[2] - a[2]) * ab[2]) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let q = [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t];
            let d = dist(p, q);
            if d < best.1 {
                best = (self.at[i - 1] + (self.at[i] - self.at[i - 1]) * t, d);
            }
        }
        best
    }

    /// The point and unit tangent at arc length `s` (clamped).
    pub fn sample(&self, s: f64) -> ([f64; 3], [f64; 3]) {
        let i = self.at.partition_point(|&a| a <= s).clamp(1, self.points.len().max(2) - 1);
        let (a, b) = (self.points[i - 1], self.points[i]);
        let span = (self.at[i] - self.at[i - 1]).max(1e-9);
        let t = ((s - self.at[i - 1]) / span).clamp(0.0, 1.0);
        let d = dist(a, b).max(1e-9);
        (
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t],
            [(b[0] - a[0]) / d, (b[1] - a[1]) / d, (b[2] - a[2]) / d],
        )
    }

    /// The width at arc length `s`.
    pub fn width_at(&self, s: f64) -> f64 {
        let i = self.at.partition_point(|&a| a <= s).min(self.width.len().saturating_sub(1));
        self.width.get(i).copied().unwrap_or(0.0)
    }

    /// The line from arc length `from` running `length` metres on, wrapping past the end of a closed line,
    /// reversed when `forward` is false, resampled every `step` metres.
    pub fn stretch(&self, from: f64, length: f64, forward: bool, step: f64) -> Line {
        let total = self.length();
        let count = (length / step).ceil() as usize;
        let mut points = Vec::with_capacity(count + 1);
        let mut width = Vec::with_capacity(count + 1);
        for k in 0..=count {
            let d = (k as f64 * step).min(length);
            let s = if forward { from + d } else { from - d }.rem_euclid(total);
            points.push(self.sample(s).0);
            width.push(self.width_at(s));
        }
        Line::new(points, width)
    }
}

/// Centripetal Catmull-Rom between `p1` and `p2`.
fn catmull_rom(p0: [f64; 3], p1: [f64; 3], p2: [f64; 3], p3: [f64; 3], t: f64) -> [f64; 3] {
    let knot = |a: [f64; 3], b: [f64; 3]| dist(a, b).sqrt().max(1e-6);
    let t0 = 0.0;
    let t1 = t0 + knot(p0, p1);
    let t2 = t1 + knot(p1, p2);
    let t3 = t2 + knot(p2, p3);
    let u = t1 + (t2 - t1) * t;
    let lerp = |a: [f64; 3], b: [f64; 3], ta: f64, tb: f64| -> [f64; 3] {
        let w = (u - ta) / (tb - ta);
        std::array::from_fn(|i| a[i] + (b[i] - a[i]) * w)
    };
    let a1 = lerp(p0, p1, t0, t1);
    let a2 = lerp(p1, p2, t1, t2);
    let a3 = lerp(p2, p3, t2, t3);
    let b1 = lerp(a1, a2, t0, t2);
    let b2 = lerp(a2, a3, t1, t3);
    lerp(b1, b2, t1, t2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spline_passes_its_nodes_and_measures_its_length() {
        let nodes = [[0.0, 0.0, 0.0, 8.0], [10.0, 0.0, 0.0, 8.0], [20.0, 0.0, 0.0, 6.0]];
        let line = Line::spline(&nodes, false, 1.0);
        assert!((line.length() - 20.0).abs() < 1e-6);
        assert!(line.points.contains(&[10.0, 0.0, 0.0]));
        let (s, d) = line.project([5.0, 1.0, 0.0]);
        assert!((s - 5.0).abs() < 1e-6 && (d - 1.0).abs() < 1e-6);
        let back = line.stretch(15.0, 10.0, false, 2.5);
        assert_eq!(back.points.len(), 5);
        assert!((back.points[4][0] - 5.0).abs() < 1e-6);
    }

    #[test]
    fn a_looped_road_wraps() {
        let nodes = [[0.0, 0.0, 0.0, 1.0], [10.0, 0.0, 0.0, 1.0], [10.0, 10.0, 0.0, 1.0], [0.0, 10.0, 0.0, 1.0]];
        let line = Line::spline(&nodes, true, 1.0);
        assert_eq!(line.points.first(), line.points.last());
        let around = line.stretch(line.length() - 1.0, 2.0, true, 1.0);
        assert!(around.points[2][0] < 2.0 && around.points[2][1] < 1.0);
    }
}
