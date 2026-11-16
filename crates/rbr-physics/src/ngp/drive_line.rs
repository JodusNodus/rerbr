//! NGP's drive line: a cubic Hermite path through points with tangents, searched in doubles. The point and the
//! points' floats widen to f64 and only the result rounds back to f32. Against 1.02's search
//! (`engine_driveline::layout::project_point`) the refinement takes up to 40 bisection steps to a dot of 0.002 (1.02: 9
//! steps, 0.01), and a NaN dot raises the lower bound. [`layout::drive_line`](super::layout::drive_line) reads the
//! line from memory.

use crate::fpu::trunc_i32;

pub type V3 = [f64; 3];

/// A path of `count` points, each with a position and a tangent.
pub trait Line {
    fn count(&self) -> u32;
    /// Ours: the line is closed, its last entry the first's copy, so a search that runs off one end comes in at the
    /// other ([`project`]). Only NGP's search (and so a circuit) knows closed lines.
    fn closed(&self) -> bool {
        false
    }
    fn position(&self, k: u32) -> V3;
    fn tangent(&self, k: u32) -> V3;
}

fn scale(v: V3, s: f64) -> V3 {
    v.map(|x| s * x)
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// The Hermite point between the points `idx` and `idx + 1` at `t`; zero outside `0..count - 1`.
fn hermite_point(line: &impl Line, idx: u32, t: f64) -> V3 {
    let n = line.count();
    if n <= 1 || idx >= n - 1 {
        return [0.0; 3];
    }
    let (p0, v0) = (line.position(idx), line.tangent(idx));
    let (p1, v1) = (line.position(idx + 1), line.tangent(idx + 1));
    let t2 = t * t;
    let t3 = t2 * t;
    let (a, b) = (t2 * 3.0, t3 + t3);
    let out = scale(p0, (b - a) + 1.0);
    let out = add(scale(v0, (t3 - (t2 + t2)) + t), out);
    let out = add(out, scale(p1, a - b));
    add(out, scale(v1, t3 - t2))
}

/// The Hermite point at the integer part of `x` and its fraction (index 0 and fraction 0 below 0 or NaN).
fn point_at(line: &impl Line, x: f64) -> V3 {
    if x >= 0.0 {
        let i = trunc_i32(x);
        hermite_point(line, i as u32, x - f64::from(i))
    } else {
        hermite_point(line, 0, 0.0)
    }
}

/// `hermite_point` in doubles, rounded to f32 lane by lane.
pub fn hermite_point_f32(line: &impl Line, idx: i32, t: f32) -> [f32; 3] {
    hermite_point(line, idx as u32, f64::from(t)).map(|x| x as f32)
}

/// The tangents of `idx` and `idx + 1` lerped in doubles, `a + (b - a) * t`, rounded to f32 lane by lane; zero outside
/// `0..count - 1`, where 1.02 traps.
pub fn tangent_lerp_f32(line: &impl Line, idx: i32, t: f32) -> [f32; 3] {
    let (n, i) = (line.count(), idx as u32);
    if n <= 1 || i >= n - 1 {
        return [0.0; 3];
    }
    let (a, b) = (line.tangent(i), line.tangent(i + 1));
    let d = scale(sub(b, a), f64::from(t));
    add(a, d).map(|x| x as f32)
}

/// The squared distance, summed in the order the original takes.
fn squared_distance(p: V3, q: V3) -> f64 {
    let d = sub(p, q);
    (d[1] * d[1] + d[0] * d[0]) + d[2] * d[2]
}

/// The position along the line of the point `p`, as an index plus a fraction, searching from the index `x`.
pub fn project(line: &impl Line, p: [f32; 3], x: f32) -> f32 {
    project_f64(line, p.map(f64::from), f64::from(x)) as f32
}

fn project_f64(line: &impl Line, p: V3, x: f64) -> f64 {
    if line.closed() {
        return project_closed(line, p, x);
    }
    let n = line.count();
    if n < 2 || 0.0 > x {
        return 0.0;
    }
    let (start, start_point) = if x >= 0.0 {
        match trunc_i32(x) as u32 {
            i if i < n => (i, point_at(line, x)),
            _ => (n - 1, line.position(n - 1)),
        }
    } else {
        (0, [0.0; 3])
    };
    let d = sub(start_point, p);
    let dist = ((d[1] * d[1] + d[0] * d[0]) + d[2] * d[2]).sqrt();
    // The search window: half the distance beyond 6 (at least 1) points either side of the start.
    let reach = match trunc_i32((dist - 6.0) * 0.5) {
        k if k <= 0 => 1,
        k => k,
    };
    let lo = (start as i32).wrapping_sub(reach);
    let mut nearest = if lo < 0 { 0 } else { lo as u32 };
    let end = match (start as i32).wrapping_add(reach) as u32 {
        e if e < n => e,
        _ => n - 1,
    };

    // The nearest point in the window (the first wins a tie).
    let mut best = squared_distance(p, line.position(nearest));
    for u in nearest + 1..=end {
        let dd = squared_distance(p, line.position(u));
        if best > dd {
            best = dd;
            nearest = u;
        }
    }
    let mut segment = nearest;
    if segment != 0 {
        if segment == n - 1 {
            segment -= 1;
        } else if segment < n {
            // A tangent that points back (its dot is negative) steps the segment back one.
            let q = line.position(segment);
            let e = sub(p, q);
            let t = line.tangent(segment);
            if 0.0 > (e[1] * t[1] + e[0] * t[0]) + t[2] * e[2] {
                segment -= 1;
            }
        }
    }

    f64::from(segment) + bisect(line, p, segment)
}

/// Bisection on the dot of the offset with the lerped tangent over the segment `segment`, from [0, 1]: the fraction.
fn bisect(line: &impl Line, p: V3, segment: u32) -> f64 {
    let (t0, t1) = (line.tangent(segment), line.tangent(segment + 1));
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    let mut mid = 0.0;
    for _ in 0..40 {
        mid = (hi + lo) * 0.5;
        let b = hermite_point(line, segment, mid);
        let w = add(t0, scale(sub(t1, t0), mid));
        let dot = ((p[0] - b[0]) * w[0] + (p[1] - b[1]) * w[1]) + (p[2] - b[2]) * w[2];
        if 0.002 >= dot.abs() {
            break;
        }
        // a positive or NaN dot raises the lower bound
        if 0.0 >= dot {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    mid
}

/// [`project_f64`] on a closed line (ours, for circuits): its `count - 1` segments go round and the search window wraps
/// at the seam, so the index runs over `0..count - 1` and comes in at 0 again, where an open line's stops at its end.
fn project_closed(line: &impl Line, p: V3, x: f64) -> f64 {
    let n = line.count();
    if n < 3 {
        return 0.0;
    }
    let period = i64::from(n - 1);
    let wrap = |i: i64| i.rem_euclid(period) as u32;
    // A NaN or negative hint starts at 0, a hint past the period (a restart) comes round into it.
    let x = if x >= 0.0 { x % period as f64 } else { 0.0 };
    let start = i64::from(trunc_i32(x));
    let d = sub(point_at(line, x), p);
    let dist = ((d[1] * d[1] + d[0] * d[0]) + d[2] * d[2]).sqrt();
    // The window as an open line's, at most half the line either side (all of it).
    let reach = i64::from(trunc_i32((dist - 6.0) * 0.5).max(1)).min(period / 2);

    // The nearest entry in the window (the first wins a tie).
    let mut nearest = start - reach;
    let mut best = squared_distance(p, line.position(wrap(nearest)));
    for u in start - reach + 1..=start + reach {
        let dd = squared_distance(p, line.position(wrap(u)));
        if best > dd {
            best = dd;
            nearest = u;
        }
    }
    // A tangent that points back (its dot is negative) steps the segment back one, over the seam if need be.
    let (q, t) = (line.position(wrap(nearest)), line.tangent(wrap(nearest)));
    let e = sub(p, q);
    let segment = if 0.0 > (e[1] * t[1] + e[0] * t[0]) + t[2] * e[2] { nearest - 1 } else { nearest };
    let segment = wrap(segment);
    f64::from(segment) + bisect(line, p, segment)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A circle of `SEGMENTS` Hermite segments round the origin, radius 100: entry `SEGMENTS` repeats entry 0.
    struct Ring {
        closed: bool,
    }

    const SEGMENTS: u32 = 200;
    const STEP: f64 = std::f64::consts::TAU / SEGMENTS as f64;

    impl Line for Ring {
        fn count(&self) -> u32 {
            SEGMENTS + 1
        }
        fn closed(&self) -> bool {
            self.closed
        }
        fn position(&self, k: u32) -> V3 {
            let a = f64::from(k) * STEP;
            [100.0 * a.cos(), 100.0 * a.sin(), 0.0]
        }
        fn tangent(&self, k: u32) -> V3 {
            let a = f64::from(k) * STEP;
            [-100.0 * STEP * a.sin(), 100.0 * STEP * a.cos(), 0.0]
        }
    }

    /// The car's position at `index` (a fractional entry index) on the circle, 3 m outside the line.
    fn car(index: f64) -> [f32; 3] {
        let a = index * STEP;
        [(103.0 * a.cos()) as f32, (103.0 * a.sin()) as f32, 0.0]
    }

    /// Drives round the circle 1.5 laps from entry 190 in steps of a third of an entry, the hint the last answer, as
    /// the physics does.
    fn lap(line: &Ring) -> Vec<f32> {
        let mut x = 190.0f32;
        let mut out = Vec::new();
        for k in 0..(SEGMENTS * 3 / 2 * 3) {
            x = project(line, car(f64::from(k) / 3.0 + 190.0), x);
            out.push(x);
        }
        out
    }

    #[test]
    fn a_closed_line_comes_in_at_the_seam_and_follows_the_car_round() {
        let line = Ring { closed: true };
        let drove = lap(&line);
        let mut wraps = 0;
        for (k, pair) in drove.windows(2).enumerate() {
            if pair[1] < pair[0] - SEGMENTS as f32 / 2.0 {
                wraps += 1;
            } else {
                assert!(pair[1] >= pair[0] - 0.05, "step {k} went back: {pair:?}");
            }
        }
        assert_eq!(wraps, 1 + 1, "the seam once a lap: the car starts 190 entries in, 1.5 laps go over it twice");
        for (k, &x) in drove.iter().enumerate() {
            let expected = (f64::from(k as u32) / 3.0 + 190.0) % f64::from(SEGMENTS);
            let off = (f64::from(x) - expected).abs();
            assert!(off.min(f64::from(SEGMENTS) - off) < 0.02, "step {k}: {x} for {expected}");
        }
    }

    #[test]
    fn an_open_line_stops_at_its_end() {
        let drove = lap(&Ring { closed: false });
        // Past the end (entry 200, 30 steps on) the car is round the start, and the answer stays at the end.
        assert!(drove[40..60].iter().all(|&x| x >= (SEGMENTS - 1) as f32), "{:?}", &drove[..60]);
    }

    #[test]
    fn a_closed_line_takes_a_hint_past_its_period_or_below_zero() {
        let line = Ring { closed: true };
        let at = |x| project(&line, car(50.0), x);
        assert!((at(50.0 + SEGMENTS as f32) - 50.0).abs() < 0.02);
        assert!((at(-3.0) - 50.0).abs() < 0.02);
        assert!(at(f32::NAN) >= 0.0);
    }
}
