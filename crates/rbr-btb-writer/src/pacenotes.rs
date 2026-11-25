//! Pace notes from a driveline's shape (the converter's own, checked by driving), in the note types 1.02's co-driver voices
//! (`rbr_gameplay::markers::slot_word_id`): corners graded by their tightest radius, `long` and `tightens` flags,
//! crests and jumps from the height profile at the speed a car carries there, and a distance key after a note with
//! room before the next one.

/// A note: its `.dls` tag, flag word and distance along the driveline, metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    pub tag: u32,
    pub flags: u32,
    pub distance: f64,
}

/// Corner grades by their tightest radius (metres, at most): the left and the right tag.
pub const GRADES: [(f64, u32, u32); 7] = [
    (15.0, 0, 11),   // hairpin
    (25.0, 1, 10),   // 90
    (40.0, 2, 9),    // K
    (70.0, 3, 8),    // medium
    (120.0, 4, 7),   // fast
    (200.0, 5, 6),   // easy
    (400.0, 26, 25), // flat
];
/// A corner turns at least this much, degrees.
const MIN_TURN: f64 = 15.0;
/// `long`: a corner past the 90s turning at least this much (degrees), or this long (metres).
const LONG_TURN: f64 = 100.0;
const LONG_LENGTH: f64 = 200.0;
/// `tightens`: the radius over the corner's first third at least this many times its tightest, which lies in the
/// last third.
const TIGHTENS: f64 = 1.6;
/// Crests (over crest): convex runs of the height profile tighter than `CREST` (metres) that lift the car carried there
/// by `CREST_LIFT` or more (`v^2 / R`, in g: a hairpin's exit taken at 10 m/s does not); jumps those lifting it `JUMP`
/// or more, off the ground (a radius `R` above `sqrt(g R)`). Convex runs closer than [`CREST_MERGE`] are one crest.
const CREST: f64 = 500.0;
const CREST_LIFT: f64 = 0.1;
const JUMP: f64 = 1.0;
const CREST_MERGE: f64 = 30.0;
/// The speed a car carries, roughly a rally car's on tarmac: at most `TOP`, cornering at `LATERAL` on the plan
/// curvature, accelerating at `ACCEL` from the standing start and out of corners, braking at `BRAKE` into them (m/s,
/// m/s^2). A crest out of a hairpin is no jump.
const TOP: f64 = 63.0;
const LATERAL: f64 = 12.0;
const ACCEL: f64 = 5.0;
const BRAKE: f64 = 10.0;
const G: f64 = 9.81;
/// The arm of the slope differences, in points either side.
const VERTICAL_ARM: usize = 6;
/// The arm of the heading differences, in points either side.
pub const ARM: usize = 4;
/// A distance key follows a note this far after it when the next note is at least [`ROOM`] metres on.
pub const DISTANCE_AFTER: f64 = 0.1;
pub const ROOM: f64 = 30.0;

pub const TAG_CREST: u32 = 16;
pub const TAG_JUMP: u32 = 20;
pub const TAG_DISTANCE: u32 = 13;
pub const FLAG_TIGHTENS: u32 = 4;
pub const FLAG_LONG: u32 = 0x400;

/// The notes of a driveline: `points` (x, y, z with z up, metres) at distances `at`, between `from` and `to`.
pub fn notes(points: &[[f64; 3]], at: &[f64], from: f64, to: f64) -> Vec<Note> {
    let n = points.len();
    if n < 2 * ARM + 2 {
        return Vec::new();
    }
    // Plan heading, unwrapped, and its rate over 2 ARM points (positive turns left: z up, right handed).
    let mut heading = Vec::with_capacity(n);
    for i in 0..n {
        let (a, b) = (points[i.saturating_sub(1)], points[(i + 1).min(n - 1)]);
        let h = (b[1] - a[1]).atan2(b[0] - a[0]);
        let h = match heading.last() {
            Some(&last) => last + wrap(h - last),
            None => h,
        };
        heading.push(h);
    }
    let rate = |i: usize| {
        let (a, b) = (i.saturating_sub(ARM), (i + ARM).min(n - 1));
        (heading[b] - heading[a]) / (at[b] - at[a]).max(1e-6)
    };
    let curvature: Vec<f64> = (0..n).map(rate).collect();
    let mut out = Vec::new();
    let flat = 1.0 / GRADES[GRADES.len() - 1].0;
    let mut i = 0;
    while i < n {
        if curvature[i].abs() < flat {
            i += 1;
            continue;
        }
        let sign = curvature[i].signum();
        let start = i;
        while i < n && curvature[i] * sign >= flat {
            i += 1;
        }
        let end = i - 1;
        let turn = (heading[end] - heading[start]).abs().to_degrees();
        if turn < MIN_TURN || at[start] < from || at[start] > to {
            continue;
        }
        let tightest = (start..=end).max_by(|&a, &b| curvature[a].abs().total_cmp(&curvature[b].abs())).unwrap();
        let radius = 1.0 / curvature[tightest].abs();
        let Some(&(_, left, right)) = GRADES.iter().find(|g| radius <= g.0) else { continue };
        let length = at[end] - at[start];
        let mut flags = 0;
        if radius > GRADES[1].0 && (turn >= LONG_TURN || length >= LONG_LENGTH) {
            flags |= FLAG_LONG;
        }
        let third = (end - start) / 3;
        if third > 0 && tightest >= end - third {
            let entry = (start..start + third).map(|k| curvature[k].abs()).fold(0.0, f64::max);
            if entry > 0.0 && 1.0 / entry >= TIGHTENS * radius {
                flags |= FLAG_TIGHTENS;
            }
        }
        out.push(Note { tag: if sign > 0.0 { left } else { right }, flags, distance: at[start] });
    }
    // The speed a car carries: the corners' limit, from the standing start up and into each corner down.
    let mut speed: Vec<f64> = curvature.iter().map(|c| (LATERAL / c.abs().max(1e-9)).sqrt().min(TOP)).collect();
    if let Some(start) = at.iter().position(|&a| a >= from) {
        speed[start] = 0.0;
    }
    for i in 1..n {
        speed[i] = speed[i].min((speed[i - 1].powi(2) + 2.0 * ACCEL * (at[i] - at[i - 1])).sqrt());
    }
    for i in (0..n - 1).rev() {
        speed[i] = speed[i].min((speed[i + 1].powi(2) + 2.0 * BRAKE * (at[i + 1] - at[i])).sqrt());
    }
    // Crests and jumps: runs of convex profile tighter than CREST taken at speed, at their most lift.
    let slope = |a: usize, b: usize| (points[b][2] - points[a][2]) / (at[b] - at[a]).max(1e-6);
    let bend: Vec<f64> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(VERTICAL_ARM), (i + VERTICAL_ARM).min(n - 1));
            (slope(i, b) - slope(a, i)) / ((at[b] - at[a]) * 0.5).max(1e-6)
        })
        .collect();
    let lift: Vec<f64> = bend.iter().zip(&speed).map(|(b, v)| -b * v * v / G).collect();
    let crest = |i: usize| -bend[i] >= 1.0 / CREST && lift[i] >= CREST_LIFT;
    let mut i = 0;
    while i < n {
        if !crest(i) {
            i += 1;
            continue;
        }
        let start = i;
        let mut last = i;
        while i < n && at[i] - at[last] <= CREST_MERGE {
            if crest(i) {
                last = i;
            }
            i += 1;
        }
        i = last + 1;
        let top = (start..=last).max_by(|&a, &b| lift[a].total_cmp(&lift[b])).unwrap();
        if at[top] < from || at[top] > to {
            continue;
        }
        let tag = if lift[top] >= JUMP { TAG_JUMP } else { TAG_CREST };
        out.push(Note { tag, flags: 0, distance: at[top] });
    }
    out.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    with_distance_keys(&out, to)
}

/// `notes` (sorted) each followed by a distance key where the next note, or `to`, is at least [`ROOM`] on.
pub fn with_distance_keys(notes: &[Note], to: f64) -> Vec<Note> {
    let mut out = Vec::with_capacity(notes.len() * 2);
    for (k, note) in notes.iter().enumerate() {
        out.push(*note);
        let next = notes.get(k + 1).map_or(to, |m| m.distance);
        if next - note.distance >= ROOM {
            out.push(Note { tag: TAG_DISTANCE, flags: 0, distance: note.distance + DISTANCE_AFTER });
        }
    }
    out
}

/// An angle into -pi..pi.
pub fn wrap(a: f64) -> f64 {
    (a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line every 2.5 m: straight, an arc of `radius` turning `turn` degrees (left when positive), straight.
    fn track(radius: f64, turn: f64) -> (Vec<[f64; 3]>, Vec<f64>) {
        path(radius, turn, 200.0)
    }

    /// [`track`] with `after` metres after the arc.
    fn path(radius: f64, turn: f64, after: f64) -> (Vec<[f64; 3]>, Vec<f64>) {
        let mut points = vec![[0.0, 0.0, 0.0]];
        let mut heading: f64 = 0.0;
        let arc = radius * turn.abs().to_radians();
        let total = 200.0 + arc + after;
        let mut s = 0.0;
        while s < total {
            s += 2.5;
            if s > 200.0 && s <= 200.0 + arc {
                heading += 2.5 / radius * turn.signum();
            }
            let p = *points.last().unwrap();
            points.push([p[0] + 2.5 * heading.cos(), p[1] + 2.5 * heading.sin(), 0.0]);
        }
        let at = (0..points.len()).map(|i| i as f64 * 2.5).collect();
        (points, at)
    }

    #[test]
    fn corners_are_graded_by_radius_and_side() {
        for (radius, turn, tag, flags) in [
            (12.0, 160.0, 0, 0),
            (20.0, -90.0, 10, 0),
            (55.0, 60.0, 3, 0),
            (100.0, -120.0, 7, FLAG_LONG),
            (300.0, 20.0, 26, 0),
        ] {
            let (points, at) = track(radius, turn);
            let found: Vec<Note> =
                notes(&points, &at, 0.0, 1e9).into_iter().filter(|n| n.tag != TAG_DISTANCE).collect();
            assert_eq!(found.len(), 1, "{radius} m {turn}: {found:?}");
            assert_eq!((found[0].tag, found[0].flags), (tag, flags), "{radius} m {turn}");
            assert!((found[0].distance - 200.0).abs() < 15.0, "{found:?}");
        }
        let (points, at) = track(1000.0, 10.0);
        assert!(notes(&points, &at, 0.0, 1e9).is_empty());
    }

    #[test]
    fn a_sharp_crest_is_a_jump() {
        // Up at 10 %, over a crest of radius 50 m at 200 m, down, a sag, then a crest of radius 450 m at 600 m.
        let slope = |s: f64| {
            let c = |v: f64| v.clamp(-0.1, 0.1);
            if s < 400.0 {
                c(0.1 - (s - 195.0) / 50.0)
            } else if s < 500.0 {
                c(-0.1 + (s - 400.0) / 50.0)
            } else {
                c(0.1 - (s - 555.0) / 450.0)
            }
        };
        let at: Vec<f64> = (0..400).map(|i| i as f64 * 2.5).collect();
        let mut z = 0.0;
        let points: Vec<[f64; 3]> = at
            .iter()
            .map(|&s| {
                z += slope(s) * 2.5;
                [s, 0.0, z]
            })
            .collect();
        let found: Vec<Note> = notes(&points, &at, 0.0, 1e9).into_iter().filter(|n| n.tag != TAG_DISTANCE).collect();
        assert_eq!(found.iter().map(|n| n.tag).collect::<Vec<_>>(), [TAG_JUMP, TAG_CREST], "{found:?}");
        assert!((found[0].distance - 200.0).abs() < 5.0);
    }

    #[test]
    fn a_crest_out_of_a_hairpin_is_no_jump() {
        // A hairpin of radius 8 m from 200 m, then straight on and down at 12 % over a crest of `radius` starting `gap`
        // metres after its exit: none where the road falls away in the hairpin (Irohazaka's), a crest a little after
        // it, a jump only with room to speed up.
        let (plan, at) = path(8.0, 180.0, 800.0);
        let exit = 200.0 + 8.0 * std::f64::consts::PI;
        for (gap, radius, called) in
            [(-20.0, 150.0, vec![]), (15.0, 80.0, vec![TAG_CREST]), (400.0, 80.0, vec![TAG_JUMP])]
        {
            let slope = |s: f64| (-(s - exit - gap) / radius).clamp(-0.12, 0.0);
            let mut z = 0.0;
            let points: Vec<[f64; 3]> = plan
                .iter()
                .zip(&at)
                .map(|(p, &s)| {
                    z += slope(s) * 2.5;
                    [p[0], p[1], z]
                })
                .collect();
            let found: Vec<u32> = notes(&points, &at, 0.0, 1e9)
                .into_iter()
                .map(|n| n.tag)
                .filter(|&t| t == TAG_CREST || t == TAG_JUMP)
                .collect();
            assert_eq!(found, called, "{gap} m after the hairpin");
        }
    }
}
