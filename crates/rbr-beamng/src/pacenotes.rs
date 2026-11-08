//! Pace notes from a driveline's shape (`notes/beamng.md` "Pace notes"), in the note types 1.02's co-driver voices
//! (`rbr_gameplay::markers::slot_word_id`): corners graded by their tightest radius, `long` and `tightens` flags,
//! crests and jumps from the height profile, and a distance key after a note with room before the next one.

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
/// Crests (over crest) and jumps by the tightest radius of the convex height profile, metres (at most): a car leaves
/// the ground over a radius `R` above `sqrt(g R)`, 63 m/s (225 km/h) for the jump's 400 m. Convex runs closer than
/// [`CREST_MERGE`] are one crest.
const CREST: f64 = 500.0;
const JUMP: f64 = 400.0;
const CREST_MERGE: f64 = 30.0;
/// The arm of the slope differences, in points either side.
const VERTICAL_ARM: usize = 6;
/// The arm of the heading and slope differences, in points either side.
const ARM: usize = 4;
/// A distance key follows a note this far after it when the next note is at least [`ROOM`] metres on.
const DISTANCE_AFTER: f64 = 0.1;
const ROOM: f64 = 30.0;

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
    // Crests and jumps: runs of convex profile tighter than CREST, at their tightest point.
    let slope = |a: usize, b: usize| (points[b][2] - points[a][2]) / (at[b] - at[a]).max(1e-6);
    let bend: Vec<f64> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(VERTICAL_ARM), (i + VERTICAL_ARM).min(n - 1));
            (slope(i, b) - slope(a, i)) / ((at[b] - at[a]) * 0.5).max(1e-6)
        })
        .collect();
    let mut i = 0;
    while i < n {
        if -bend[i] < 1.0 / CREST {
            i += 1;
            continue;
        }
        let start = i;
        let mut last = i;
        while i < n && at[i] - at[last] <= CREST_MERGE {
            if -bend[i] >= 1.0 / CREST {
                last = i;
            }
            i += 1;
        }
        i = last + 1;
        let top = (start..=last).max_by(|&a, &b| (-bend[a]).total_cmp(&-bend[b])).unwrap();
        if at[top] < from || at[top] > to {
            continue;
        }
        let tag = if -bend[top] >= 1.0 / JUMP { TAG_JUMP } else { TAG_CREST };
        out.push(Note { tag, flags: 0, distance: at[top] });
    }
    out.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    let mut with_distances = Vec::with_capacity(out.len() * 2);
    for (k, note) in out.iter().enumerate() {
        with_distances.push(*note);
        let next = out.get(k + 1).map_or(to, |m| m.distance);
        if next - note.distance >= ROOM {
            with_distances.push(Note { tag: TAG_DISTANCE, flags: 0, distance: note.distance + DISTANCE_AFTER });
        }
    }
    with_distances
}

/// An angle into -pi..pi.
fn wrap(a: f64) -> f64 {
    (a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line every 2.5 m: straight, an arc of `radius` turning `turn` degrees (left when positive), straight.
    fn track(radius: f64, turn: f64) -> (Vec<[f64; 3]>, Vec<f64>) {
        let mut points = vec![[0.0, 0.0, 0.0]];
        let mut heading: f64 = 0.0;
        let arc = radius * turn.abs().to_radians();
        let total = 200.0 + arc + 200.0;
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
}
