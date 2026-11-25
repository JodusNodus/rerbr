//! DiRT Rally's codriver calls as 1.02 pace notes. A call's `distanceAlongTrack` is where the codriver speaks, 60 to 100 m
//! before the corner, from the start line (`start` here); a note's distance is where the corner begins (the game's own
//! callout goes ahead of it), so each corner call is put at the start of the first corner of its side on the road line
//! that it can mean, the call's own place if none is near. The side is the subcall's type (1 left, 2 right) and the
//! grade its `angle`, the corner's turn in degrees (the corners the calls name turn 17, 30, 46, 60, 75, 102 and 166
//! degrees on average for angles 15, 30, 40, 50, 65, 100 and 140). Subcalls of other types are not decoded and leave no
//! note (`notes/dirt-rally.md`).
use crate::route::Call;
use rbr_ac_tracks::btb::pacenotes::{Note, TAG_DISTANCE};

/// The tag of a corner by its angle, up to the bound (degrees): easy, fast, medium, K, 90 and hairpin, as the left and
/// the right tag (`rbr_ac_tracks::btb::pacenotes::GRADES`). The bounds lie between DiRT's own grades, 15, 30, 40 and 50,
/// 65, 80 and 100, 140 and 180.
pub const TURNS: [(f64, u32, u32); 6] =
    [(22.5, 5, 6), (35.0, 4, 7), (57.5, 3, 8), (72.5, 2, 9), (120.0, 1, 10), (f64::INFINITY, 0, 11)];

/// A corner is a run of the line bending to one side with a radius of 700 m or less, turning at least this many degrees.
const MIN_TURN: f64 = 8.0;
const FLAT: f64 = 1.0 / 700.0;
/// The arm of the heading difference, in points either side (the converter's own).
const ARM: usize = 4;
/// A call can mean a corner that begins this far past its place (and has not ended before it).
const REACH: f64 = 220.0;
/// A corner turns at least this fraction of the angle its call gives (the matched ones turn 0.9 to 1.1 times it on average).
const SMALLEST: f64 = 0.3;
/// Where a corner call goes when no corner is near: this far past the call, [`LEAD`] unless the route's matched calls
/// say otherwise (the lead follows the stage's speed: its median is 60 to 100 m on most, 150 m on Ouninpohja).
const LEAD: f64 = 70.0;
/// A distance key follows a note this far after it when the next note is at least [`ROOM`] metres on.
const DISTANCE_AFTER: f64 = 0.1;
const ROOM: f64 = 30.0;

/// A bend of the road line: arc lengths of its first and last point, the turn (degrees), the tightest radius (metres).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corner {
    pub start: f64,
    pub end: f64,
    pub left: bool,
    pub turn: f64,
    pub radius: f64,
}

/// The corners of a line (x, y, z with z up) at arc lengths `at`.
pub fn corners(points: &[[f64; 3]], at: &[f64]) -> Vec<Corner> {
    let n = points.len();
    if n < 2 * ARM + 2 {
        return Vec::new();
    }
    let mut heading: Vec<f64> = Vec::with_capacity(n);
    for i in 0..n {
        let (a, b) = (points[i.saturating_sub(1)], points[(i + 1).min(n - 1)]);
        let h = (b[1] - a[1]).atan2(b[0] - a[0]);
        heading.push(match heading.last() {
            Some(&last) => last + wrap(h - last),
            None => h,
        });
    }
    let curvature: Vec<f64> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(ARM), (i + ARM).min(n - 1));
            (heading[b] - heading[a]) / (at[b] - at[a]).max(1e-6)
        })
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if curvature[i].abs() < FLAT {
            i += 1;
            continue;
        }
        let sign = curvature[i].signum();
        let mut first = i;
        // A run of one side is cut at its valleys, where the bend falls under half its peak and rises again.
        let (mut peak, mut valley) = (0.0_f64, (0.0_f64, i));
        while i < n && curvature[i] * sign >= FLAT {
            let c = curvature[i].abs();
            if c > peak {
                peak = c;
                valley = (c, i);
            } else if c < valley.0 {
                valley = (c, i);
            } else if valley.0 < peak * 0.5 && c > valley.0 * 1.5 {
                push(&mut out, &heading, &curvature, at, first, valley.1, sign);
                (first, peak, valley) = (valley.1, c, (c, i));
            }
            i += 1;
        }
        push(&mut out, &heading, &curvature, at, first, i - 1, sign);
    }
    out
}

fn push(out: &mut Vec<Corner>, heading: &[f64], curvature: &[f64], at: &[f64], first: usize, last: usize, sign: f64) {
    let turn = (heading[last] - heading[first]).abs().to_degrees();
    if turn >= MIN_TURN {
        let tightest = curvature[first..=last].iter().fold(0.0_f64, |m, c| m.max(c.abs()));
        out.push(Corner { start: at[first], end: at[last], left: sign > 0.0, turn, radius: 1.0 / tightest });
    }
}

fn wrap(a: f64) -> f64 {
    (a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

/// The tag of a corner call of `angle` degrees on the given side.
pub fn tag(angle: f64, left: bool) -> u32 {
    let &(_, l, r) = TURNS.iter().find(|t| angle <= t.0).unwrap_or(&TURNS[TURNS.len() - 1]);
    if left {
        l
    } else {
        r
    }
}

/// A corner note with what it came from: the call's index and the angle, and the corner it was put at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub call: usize,
    pub angle: f64,
    pub note: Note,
    pub corner: Option<Corner>,
}

/// The corner notes of `calls` (distances from the start line at `from` on the line): in order, each at the start of
/// the next corner of its side that ends past the call and begins within [`REACH`] of it.
pub fn place(calls: &[Call], points: &[[f64; 3]], at: &[f64], from: f64) -> Vec<Placed> {
    let corners = corners(points, at);
    let corner_calls = |call: &Call| {
        call.subcalls.iter().filter(|s| matches!(s.kind, 1 | 2) && s.angle > 0.0).copied().collect::<Vec<_>>()
    };
    let (mut out, mut leads) = (Vec::new(), Vec::new());
    // Matched corners first, then the others at the route's median lead past their call.
    let mut next = 0;
    for (index, call) in calls.iter().enumerate() {
        let spoken = from + call.distance;
        for sub in corner_calls(call) {
            let left = sub.kind == 1;
            let found = (next..corners.len()).find(|&k| {
                let c = &corners[k];
                c.left == left
                    && c.end >= spoken
                    && c.start <= spoken + REACH
                    && c.turn >= SMALLEST * f64::from(sub.angle)
            });
            let corner = found.map(|k| corners[k]);
            if let Some(k) = found {
                next = k + 1;
                leads.push(corners[k].start - spoken);
            }
            let angle = f64::from(sub.angle);
            let distance = corner.map_or(spoken, |c| c.start.max(spoken));
            out.push(Placed { call: index, angle, note: Note { tag: tag(angle, left), flags: 0, distance }, corner });
        }
    }
    leads.sort_by(f64::total_cmp);
    let lead = leads.get(leads.len() / 2).copied().unwrap_or(LEAD).max(0.0);
    let mut last = from;
    for placed in &mut out {
        if placed.corner.is_none() {
            placed.note.distance += lead;
        }
        placed.note.distance = placed.note.distance.max(last + 1.0).max(from + 1.0);
        last = placed.note.distance;
    }
    out
}

/// The route's pace notes between `from` and `to` on the line: [`place`]'s, each with a distance key after it where
/// the next note is far enough on.
pub fn notes(calls: &[Call], points: &[[f64; 3]], at: &[f64], from: f64, to: f64) -> Vec<Note> {
    let placed: Vec<Note> =
        place(calls, points, at, from).into_iter().map(|p| p.note).filter(|n| n.distance < to).collect();
    let mut out = Vec::with_capacity(placed.len() * 2);
    for (k, note) in placed.iter().enumerate() {
        out.push(*note);
        let next = placed.get(k + 1).map_or(to, |m| m.distance);
        if next - note.distance >= ROOM {
            out.push(Note { tag: TAG_DISTANCE, flags: 0, distance: note.distance + DISTANCE_AFTER });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::Subcall;

    fn sub(kind: u32, angle: f32) -> Subcall {
        Subcall { kind, delay: 0, angle, modifier_a: 0, modifier_b: 0, distance_link: 0 }
    }

    fn call(distance: f64, subcalls: Vec<Subcall>) -> Call {
        Call { distance, name: String::new(), subcalls }
    }

    /// A line every 2.5 m: 200 m straight, an arc of `radius` turning `turn` degrees (left when positive), 300 m straight.
    fn track(radius: f64, turn: f64) -> (Vec<[f64; 3]>, Vec<f64>) {
        let mut points = vec![[0.0, 0.0, 0.0]];
        let mut heading: f64 = 0.0;
        let arc = radius * turn.abs().to_radians();
        let mut s = 0.0;
        while s < 500.0 + arc {
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
    fn angles_are_binned_between_dirts_grades() {
        let left: Vec<u32> =
            [15., 22., 30., 40., 50., 53., 65., 72., 80., 100., 140., 180.].map(|a| tag(a, true)).to_vec();
        assert_eq!(left, [5, 5, 4, 3, 3, 3, 2, 2, 1, 1, 0, 0]);
        assert_eq!((tag(15., false), tag(65., false), tag(140., false)), (6, 9, 11));
    }

    #[test]
    fn a_call_goes_to_the_corner_it_names() {
        let (points, at) = track(40.0, -75.0);
        // Start line at 20 m, the call 100 m on (line 120): a right 75 begins at 200 m.
        let calls = [call(100.0, vec![sub(2, 65.0), sub(0, 0.0)])];
        let found = place(&calls, &points, &at, 20.0);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].note.tag, found[0].note.flags), (9, 0));
        assert!((found[0].note.distance - 200.0).abs() < 15.0, "{found:?}");
        let corner = found[0].corner.unwrap();
        assert!(!corner.left && (corner.turn - 75.0).abs() < 10.0);
        // The other side finds no corner: the call's own place and the lead.
        let calls = [call(100.0, vec![sub(1, 75.0)])];
        let found = place(&calls, &points, &at, 20.0);
        assert_eq!((found[0].corner, found[0].note.tag), (None, 1));
        assert_eq!(found[0].note.distance, 120.0 + LEAD);
    }

    #[test]
    fn notes_carry_distance_keys_and_leave_other_types_out() {
        let (points, at) = track(40.0, 90.0);
        let calls = [call(100.0, vec![sub(1, 100.0), sub(7, 0.0)]), call(110.0, vec![sub(3, 0.0)])];
        let found = notes(&calls, &points, &at, 20.0, 480.0);
        assert_eq!(found.iter().map(|n| n.tag).collect::<Vec<_>>(), [1, TAG_DISTANCE]);
        assert!((found[1].distance - found[0].distance - DISTANCE_AFTER).abs() < 1e-9);
        assert!(notes(&calls, &points, &at, 20.0, 150.0).is_empty());
    }

    /// Every rally route of the install (`RBR_DIRT_RALLY`, the game folder; skipped when unset): each corner subcall
    /// makes a note, in order inside the stage, and most find the corner they name.
    #[test]
    fn every_rally_routes_calls_become_ordered_notes_on_its_line() {
        use crate::install::Install;
        let Some(path) = std::env::var_os("RBR_DIRT_RALLY") else { return };
        let install = Install::at(std::path::Path::new(&path)).unwrap();
        for route in install.routes().into_iter().filter(|r| r.is_rally()) {
            let course = crate::route::course(&route).unwrap();
            let (line, start, finish, _) = crate::import::centre_line(&route, &course).unwrap();
            let from = (start - 50.).max(0.);
            let line = line.stretch(from, (finish + 40.).min(line.length()) - from, true, 2.5);
            let (start, finish) = (start - from, finish - from);
            let calls = crate::route::calls(&route).unwrap();
            let wanted =
                calls.iter().flat_map(|c| &c.subcalls).filter(|s| matches!(s.kind, 1 | 2) && s.angle > 0.0).count();
            let placed = place(&calls, &line.points, &line.at, start);
            let id = route.id();
            assert_eq!(placed.len(), wanted, "{id}");
            assert!(placed.windows(2).all(|w| w[0].note.distance < w[1].note.distance), "{id}");
            assert!(placed.iter().all(|p| p.note.distance > start && p.note.distance < finish + 400.), "{id}");
            let matched = placed.iter().filter(|p| p.corner.is_some()).count();
            assert!(matched * 100 >= placed.len() * 40, "{id}: {matched} of {}", placed.len());
            let written = notes(&calls, &line.points, &line.at, start, finish);
            assert!(written.windows(2).all(|w| w[0].distance <= w[1].distance), "{id}");
        }
    }
}
