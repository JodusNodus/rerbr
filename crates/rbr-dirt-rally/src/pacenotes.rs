//! DiRT Rally's codriver calls as 1.02 pace notes. A call's `distanceAlongTrack` is where the codriver speaks, 40 to 180 m
//! before the corner, from the start line (`start` here); a note's distance is where the corner begins (the game's own
//! callout goes ahead of it). The calls' corner subcalls are aligned with the corners of the road line as a whole
//! ([`align`]): in order, each corner once, at a lead (corner start minus the call's place) that is 3 seconds of the
//! speed the line allows plus a route offset learned from its confident matches, with the turn near the angle and the
//! side the subcall's (1 left, 2 right). A subcall joined to the next of its side (distance link 12) is the entry of a
//! corner that tightens: one note, graded by the second angle. What no corner fits goes at the call plus the lead. The
//! grade is the `angle`, the corner's turn in degrees (the corners the calls name turn 17, 30, 46, 60, 75, 102 and 166
//! degrees on average for angles 15, 30, 40, 50, 65, 100 and 140). Subcalls of other types are not decoded and leave no
//! note (`notes/dirt-rally.md`).
use crate::route::{Call, Subcall};
use rbr_btb_writer::pacenotes::{self as graded, wrap, Note, ARM, FLAG_TIGHTENS, TAG_CREST, TAG_JUMP};
#[cfg(test)]
use rbr_btb_writer::pacenotes::{DISTANCE_AFTER, TAG_DISTANCE};

/// The tag of a corner by its angle, up to the bound (degrees): easy, fast, medium, K, 90 and hairpin, as the left and
/// the right tag (`rbr_btb_writer::pacenotes::GRADES`). The bounds lie between DiRT's own grades, 15, 30, 40 and 50,
/// 65, 80 and 100, 140 and 180.
pub const TURNS: [(f64, u32, u32); 6] =
    [(22.5, 5, 6), (35.0, 4, 7), (57.5, 3, 8), (72.5, 2, 9), (120.0, 1, 10), (f64::INFINITY, 0, 11)];

/// A bend is a run of the line curving to one side with a radius of 1000 m or less (heading rate over 20 m), turning at
/// least 5 degrees; the gentler ones are runs over 60 m, radius 1500 m, 5 degrees (fast flat corners, Ouninpohja's).
const MIN_TURN: f64 = 5.0;
const FLAT: f64 = 1.0 / 1000.0;
const GENTLE: f64 = 1.0 / 1500.0;
const GENTLE_TURN: f64 = 5.0;

/// A bend of the road line: arc lengths of its first and last point, the turn (degrees), the tightest radius (metres).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corner {
    pub start: f64,
    pub end: f64,
    pub left: bool,
    pub turn: f64,
    pub radius: f64,
    /// Where the bend is tightest, as a fraction of its length, and the radius of its first third.
    pub peak: f64,
    pub entry: f64,
}

/// The corners of a line (x, y, z with z up) at arc lengths `at`.
pub fn corners(points: &[[f64; 3]], at: &[f64]) -> Vec<Corner> {
    bends(points, at, ARM, FLAT, MIN_TURN)
}

/// The bends of a line whose heading rate over `2 * arm` points is `flat` (1/m) or more and that turn `min_turn` degrees.
pub fn bends(points: &[[f64; 3]], at: &[f64], arm: usize, flat: f64, min_turn: f64) -> Vec<Corner> {
    let n = points.len();
    if n < 2 * arm + 2 {
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
            let (a, b) = (i.saturating_sub(arm), (i + arm).min(n - 1));
            (heading[b] - heading[a]) / (at[b] - at[a]).max(1e-6)
        })
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if curvature[i].abs() < flat {
            i += 1;
            continue;
        }
        let sign = curvature[i].signum();
        let mut first = i;
        // A run of one side is cut at its valleys, where the bend falls under half its peak and rises again.
        let (mut peak, mut valley) = (0.0_f64, (0.0_f64, i));
        while i < n && curvature[i] * sign >= flat {
            let c = curvature[i].abs();
            if c > peak {
                peak = c;
                valley = (c, i);
            } else if c < valley.0 {
                valley = (c, i);
            } else if valley.0 < peak * 0.5 && c > valley.0 * 1.5 {
                push(&mut out, &heading, &curvature, at, first, valley.1, sign, min_turn);
                (first, peak, valley) = (valley.1, c, (c, i));
            }
            i += 1;
        }
        push(&mut out, &heading, &curvature, at, first, i - 1, sign, min_turn);
    }
    out
}

fn push(
    out: &mut Vec<Corner>,
    heading: &[f64],
    curvature: &[f64],
    at: &[f64],
    first: usize,
    last: usize,
    sign: f64,
    min_turn: f64,
) {
    let turn = (heading[last] - heading[first]).abs().to_degrees();
    if turn >= min_turn {
        let top = (first..=last).max_by(|&a, &b| curvature[a].abs().total_cmp(&curvature[b].abs())).unwrap_or(first);
        let third = (last - first) / 3;
        let entry = (first..=first + third).fold(0.0_f64, |m, k| m.max(curvature[k].abs()));
        out.push(Corner {
            start: at[first],
            end: at[last],
            left: sign > 0.0,
            turn,
            radius: 1.0 / curvature[top].abs(),
            peak: (at[top] - at[first]) / (at[last] - at[first]).max(1e-6),
            entry: 1.0 / entry,
        });
    }
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
    /// The call's side (a left corner call).
    pub tag_left: bool,
    pub note: Note,
    pub corner: Option<Corner>,
}

/// A corner call: where it is spoken and what it names.
#[derive(Debug, Clone, Copy)]
struct Sub {
    call: usize,
    /// Its place among the call's corner subcalls.
    nth: usize,
    left: bool,
    angle: f64,
    spoken: f64,
    /// The speed the line allows there (m/s).
    speed: f64,
    /// The corner tightens into this angle.
    tightens: bool,
}

/// Whether corner subcall `i` is the entry of the corner the next one names (same side, distance link 12): the pairs
/// that match the line's corners tighten (tightest after 0.6 of the bend in 47 % against 28 % of single calls, and
/// its first third 1.9 times the tightest radius against 1.3).
fn joined(named: &[&Subcall], i: usize) -> bool {
    named[i].distance_link == 12 && named.get(i + 1).is_some_and(|n| n.kind == named[i].kind)
}

/// What a lead (corner start minus the place of the call) is expected to be: the route's offset and spread (metres) on top
/// of [`SECONDS`] of the speed the line allows at the call (the matched leads grow with it: 0.5 correlation, and 3
/// seconds takes the spread of a route's leads from 30 to 26 m).
#[derive(Debug, Clone, Copy)]
struct Lead {
    offset: f64,
    spread: f64,
}

const SECONDS: f64 = 3.0;

impl Lead {
    fn mean(self, sub: &Sub) -> f64 {
        self.offset + SECONDS * sub.speed
    }
}

/// Costs (in half squared spreads, as a log likelihood) of not naming a corner and of naming one of the other side.
const SKIP: f64 = 5.0;
const OTHER_SIDE: f64 = 6.0;
const CAP: f64 = 8.0;
/// A turn is taken to be this much off its angle (log of the ratio) at one spread.
const TURN_SPREAD: f64 = 0.6;

/// What it costs for `sub` to name `corner`: the lead against the route's, the turn against the angle, the side.
fn cost(sub: &Sub, corner: &Corner, lead: Lead) -> f64 {
    let gap = corner.start - sub.spoken;
    if corner.end < sub.spoken || gap > 3.0 * lead.mean(sub) + 6.0 * lead.spread {
        return f64::INFINITY;
    }
    // Later subcalls of a call come later: their lead is looser and longer.
    let (mean, spread) = (lead.mean(sub) + 40.0 * sub.nth as f64, lead.spread * (1.0 + sub.nth as f64));
    let z = (gap - mean) / spread;
    let t = (corner.turn / sub.angle).ln() / TURN_SPREAD;
    let side = if corner.left == sub.left { 0.0 } else { OTHER_SIDE };
    (0.5 * z * z).min(CAP) + (0.5 * t * t).min(CAP) + side
}

/// What it costs for a call's next subcall to name `next` right after `prev` (none: the call's first).
fn pair(prev: Option<&Corner>, next: &Corner) -> f64 {
    let gap = prev.map_or(0.0, |p| next.start - p.end);
    (0.5 * (gap.max(0.0) / 80.0).powi(2) + if gap < -3.0 { 2.0 } else { 0.0 }).min(3.0)
}

/// The order-preserving assignment of `subs` to `corners` (none where a call names no corner) of least total cost: each
/// corner is named once, later subcalls name later corners and a call's subcalls are consecutive corners.
fn align(subs: &[Sub], corners: &[Corner], lead: Lead) -> Vec<Option<usize>> {
    let (n, m) = (subs.len(), corners.len());
    if n == 0 {
        return Vec::new();
    }
    // a[i][k]: subcall i names corner k; s[i][k]: it names none and the last one named was k (k = m: none yet).
    let mut a = vec![vec![f64::INFINITY; m + 1]; n];
    let mut s = vec![vec![f64::INFINITY; m + 1]; n];
    // For a: the corner named before (m: none) and whether that was the previous subcall.
    let mut back = vec![vec![(m, false); m + 1]; n];
    for i in 0..n {
        for k in 0..=m {
            let before = match i {
                0 if k == m => 0.0,
                0 => f64::INFINITY,
                _ => a[i - 1][k].min(s[i - 1][k]),
            };
            s[i][k] = before + SKIP;
        }
        let same = i > 0 && subs[i - 1].call == subs[i].call;
        for k in 0..m {
            let u = cost(&subs[i], &corners[k], lead);
            if !u.is_finite() {
                continue;
            }
            let mut from = (f64::INFINITY, (m, false));
            for j in (0..k).chain([m]) {
                let options = match i {
                    0 if j == m => [(0.0, false), (f64::INFINITY, false)],
                    0 => [(f64::INFINITY, false); 2],
                    _ if same => {
                        [(a[i - 1][j] + pair((j < m).then(|| &corners[j]), &corners[k]), true), (s[i - 1][j], false)]
                    }
                    _ => [(a[i - 1][j], true), (s[i - 1][j], false)],
                };
                for (c, via_a) in options {
                    if c < from.0 {
                        from = (c, (j, via_a));
                    }
                }
            }
            a[i][k] = u + from.0;
            back[i][k] = from.1;
        }
    }
    let mut out = vec![None; n];
    let Some(mut state) = (0..=m)
        .filter(|&k| a[n - 1][k].min(s[n - 1][k]).is_finite())
        .min_by(|&x, &y| a[n - 1][x].min(s[n - 1][x]).total_cmp(&a[n - 1][y].min(s[n - 1][y])))
    else {
        return out;
    };
    // Whether the subcall's own state was reached by naming (a) or not (s).
    let mut named = state < m && a[n - 1][state] <= s[n - 1][state];
    for i in (0..n).rev() {
        if named {
            out[i] = Some(state);
            let (j, via_a) = back[i][state];
            (state, named) = (j, via_a);
        } else if i > 0 {
            named = state < m && a[i - 1][state] <= s[i - 1][state];
        }
    }
    out
}

/// The corner notes of `calls` (distances from the start line at `from` on the line): each corner subcall is aligned
/// with the corners of the line (see [`align`]) and put at the start of its corner, or, with none, at the route's
/// median lead past its call.
pub fn place(calls: &[Call], points: &[[f64; 3]], at: &[f64], from: f64) -> Vec<Placed> {
    let corners = candidates(points, at);
    let speeds = speeds(points, at);
    let speed_at = |d: f64| speeds[at.partition_point(|a| *a < d).min(at.len() - 1)];
    let mut subs = Vec::new();
    for (index, call) in calls.iter().enumerate() {
        let spoken = from + call.distance;
        let named = call.subcalls.iter().filter(|s| matches!(s.kind, 1 | 2) && s.angle > 0.0);
        // A subcall joined to the next of its side (link 12) is the entry of a corner that tightens into it: one corner.
        let named: Vec<_> = named.collect();
        let named: Vec<_> = (0..named.len())
            .filter(|&i| !joined(&named, i))
            .map(|i| (named[i], i > 0 && joined(&named, i - 1)))
            .collect();
        for (nth, (sub, tightens)) in named.into_iter().enumerate() {
            subs.push(Sub {
                call: index,
                nth,
                left: sub.kind == 1,
                angle: f64::from(sub.angle),
                spoken,
                speed: speed_at(spoken),
                tightens,
            });
        }
    }
    // The route's lead is learned from its confident matches, and the alignment run again with it.
    let mut lead = Lead { offset: 0.0, spread: 60.0 };
    let mut named = align(&subs, &corners, lead);
    for _ in 0..2 {
        let mut leads: Vec<f64> = subs
            .iter()
            .zip(&named)
            .filter(|(sub, k)| sub.nth == 0 && k.is_some_and(|k| confident(sub, &corners[k])))
            .map(|(sub, k)| corners[k.unwrap()].start - sub.spoken - SECONDS * sub.speed)
            .collect();
        if leads.len() < 5 {
            break;
        }
        leads.sort_by(f64::total_cmp);
        let mean = leads[leads.len() / 2];
        let mut spreads: Vec<f64> = leads.iter().map(|l| (l - mean).abs()).collect();
        spreads.sort_by(f64::total_cmp);
        lead = Lead { offset: mean, spread: (1.4826 * spreads[spreads.len() / 2]).max(20.0) };
        named = align(&subs, &corners, lead);
    }
    let mut out: Vec<Placed> = subs
        .iter()
        .zip(&named)
        .map(|(sub, k)| {
            let corner = k.map(|k| corners[k]);
            let distance = corner.map_or(sub.spoken + lead.mean(sub).max(0.0), |c| c.start.max(sub.spoken));
            Placed {
                call: sub.call,
                angle: sub.angle,
                tag_left: sub.left,
                note: Note {
                    tag: tag(sub.angle, sub.left),
                    flags: if sub.tightens { FLAG_TIGHTENS } else { 0 },
                    distance,
                },
                corner,
            }
        })
        .collect();
    let mut last = from;
    for placed in &mut out {
        placed.note.distance = placed.note.distance.max(last + 1.0).max(from + 1.0);
        last = placed.note.distance;
    }
    out
}

/// A match is confident (it teaches the route's lead) when its side agrees and its turn is within 1.6 times its angle.
fn confident(sub: &Sub, corner: &Corner) -> bool {
    corner.left == sub.left && (corner.turn / sub.angle).ln().abs() < 0.5
}

/// The speed (m/s) a rally car holds along the line: the corner speed of its curvature (lateral acceleration `LATERAL`),
/// limited by braking into and accelerating out of the corners.
pub fn speeds(points: &[[f64; 3]], at: &[f64]) -> Vec<f64> {
    const LATERAL: f64 = 7.0;
    const TOP: f64 = 55.0;
    const BRAKE: f64 = 6.0;
    const ACCEL: f64 = 4.0;
    let n = points.len();
    let mut v: Vec<f64> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(ARM), (i + ARM).min(n - 1));
            let (p, q, r) = (points[a], points[i], points[b]);
            let (h0, h1) = ((q[1] - p[1]).atan2(q[0] - p[0]), (r[1] - q[1]).atan2(r[0] - q[0]));
            let k = wrap(h1 - h0).abs() / (at[b] - at[a]).max(1e-6) * 2.0;
            (LATERAL / k.max(1e-6)).sqrt().min(TOP)
        })
        .collect();
    for i in (0..n.saturating_sub(1)).rev() {
        v[i] = v[i].min((v[i + 1].powi(2) + 2.0 * BRAKE * (at[i + 1] - at[i])).sqrt());
    }
    for i in 1..n {
        v[i] = v[i].min((v[i - 1].powi(2) + 2.0 * ACCEL * (at[i] - at[i - 1])).sqrt());
    }
    v
}

/// The corners a call can name: the line's bends, and the longer gentler ones that no sharper bend covers.
pub fn candidates(points: &[[f64; 3]], at: &[f64]) -> Vec<Corner> {
    let mut out = bends(points, at, ARM, FLAT, MIN_TURN);
    let sharp = out.clone();
    out.extend(
        bends(points, at, 3 * ARM, GENTLE, GENTLE_TURN)
            .into_iter()
            .filter(|c| !sharp.iter().any(|s| s.left == c.left && s.start < c.end && c.start < s.end)),
    );
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    out
}

/// A crest or jump of the line's own height profile is dropped this close to a corner note (metres).
const NEAR_CORNER: f64 = 20.0;

/// The route's pace notes between `from` and `to` on the line: [`place`]'s, the converter's crests and jumps (its corner
/// notes and flags are not used) that are not within [`NEAR_CORNER`] of one, each with a distance key after it where
/// the next note is far enough on.
pub fn notes(calls: &[Call], points: &[[f64; 3]], at: &[f64], from: f64, to: f64) -> Vec<Note> {
    let mut placed: Vec<Note> =
        place(calls, points, at, from).into_iter().map(|p| p.note).filter(|n| n.distance < to).collect();
    let corners = placed.clone();
    placed.extend(graded::notes(points, at, from, to).into_iter().filter(|n| {
        matches!(n.tag, TAG_CREST | TAG_JUMP) && corners.iter().all(|c| (c.distance - n.distance).abs() > NEAR_CORNER)
    }));
    placed.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    graded::with_distance_keys(&placed, to)
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
        assert!(found[0].note.distance > 120.0 + 60.0, "{found:?}");
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

    #[test]
    fn crests_stay_unless_a_corner_note_is_near() {
        // Up at 10 %, over a sharp crest at 200 m (the converter's own test line), down.
        let slope = |s: f64| (0.1 - (s - 195.0) / 50.0).clamp(-0.1, 0.1);
        let at: Vec<f64> = (0..200).map(|i| i as f64 * 2.5).collect();
        let mut z = 0.0;
        let points: Vec<[f64; 3]> = at
            .iter()
            .map(|&s| {
                z += slope(s) * 2.5;
                [s, 0.0, z]
            })
            .collect();
        let tags = |calls: &[Call]| notes(calls, &points, &at, 20.0, 480.0).iter().map(|n| n.tag).collect::<Vec<_>>();
        assert_eq!(tags(&[]), [graded_crest_or_jump(&points, &at), TAG_DISTANCE]);
        // A corner call that falls back to 195 m (call 30 plus 3 s at the line's top speed, 165 m): the crest at 200 is dropped.
        assert_eq!(tags(&[call(10.0, vec![sub(1, 40.0)])]), [3, TAG_DISTANCE]);
    }

    fn graded_crest_or_jump(points: &[[f64; 3]], at: &[f64]) -> u32 {
        graded::notes(points, at, 20.0, 480.0)
            .iter()
            .map(|n| n.tag)
            .find(|t| matches!(*t, TAG_CREST | TAG_JUMP))
            .unwrap()
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
            // Subcalls joined to the next of their side (a corner that tightens) make one note.
            let wanted = calls
                .iter()
                .map(|c| {
                    let named: Vec<_> =
                        c.subcalls.iter().filter(|s| matches!(s.kind, 1 | 2) && s.angle > 0.0).collect();
                    (0..named.len()).filter(|&i| !joined(&named, i)).count()
                })
                .sum::<usize>();
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
