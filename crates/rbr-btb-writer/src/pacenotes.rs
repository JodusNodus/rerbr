//! Pace notes from a driveline's shape (the converter's own, checked by driving), in the note types 1.02's co-driver voices
//! (`rbr_gameplay::markers::slot_word_id`): corners cut from the curvature and graded by tightest radius and turn, `long`
//! and `tightens` flags, crests and jumps from the height profile at the speed a car carries there, and a distance key
//! after a note with room before the next one. The constants of [`Rules`] are fitted against Luppis V3's notes on the
//! RSF stages (`notes/btb.md`, "Generated pacenotes"): where it finds a human's corner, grade and flags, not a physical
//! law.

/// A note: its `.dls` tag, flag word and distance along the driveline, metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    pub tag: u32,
    pub flags: u32,
    pub distance: f64,
}

/// The corner grades, hairpin to flat: the left and the right tag.
pub const GRADES: [(u32, u32); 7] = [
    (0, 11),  // hairpin
    (1, 10),  // 90
    (2, 9),   // K
    (3, 8),   // medium
    (4, 7),   // fast
    (5, 6),   // easy
    (26, 25), // flat
];
/// Crests (over crest) lift the car carried there (`v^2 / R`, in g: a hairpin's exit taken at 10 m/s does not); jumps
/// lift it off the ground (a radius `R` above `sqrt(g R)`). Convex runs closer than [`CREST_MERGE`] are one crest.
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

/// The rules the corners are called by, fitted against Luppis V3's notes on the RSF stages (`notes/btb.md`).
#[derive(Debug, Clone)]
pub struct Rules {
    /// A corner is a run of one-sided curvature over `1 / flat_radius` that turns `min_turn` degrees at least and is
    /// `len_k * radius ^ len_p` metres long at least (its tightest radius).
    pub flat_radius: f64,
    pub min_turn: f64,
    pub len_k: f64,
    pub len_p: f64,
    /// A run is cut at a valley under `valley` of its peak where it rises again (0: no cutting).
    pub valley: f64,
    /// A corner is called where its curvature first reaches this fraction of its peak.
    pub call_at: f64,
    /// A corner's grade: `radius / turn ^ grade_turn` (metres, degrees) up to each of these, hairpin to easy; flat above.
    pub grade_turn: f64,
    pub grades: [f64; 6],
    /// `long`: a corner past the 90s and short of the flats cut from a run of curvature this long (metres) or longer.
    pub long_run: f64,
    /// `tightens`: a corner with a radius within `tighten_radius` metres that turns `tighten_turn` degrees or more.
    pub tighten_radius: (f64, f64),
    pub tighten_turn: f64,
    /// ... whose radius over its first third is `tighten_ratio` times its tightest at least, which lies after
    /// `tighten_peak` of its length.
    pub tighten_ratio: f64,
    pub tighten_peak: f64,
    /// Crests: convex runs tighter than `crest_radius` metres that lift the car by `crest_lift` g or more; jumps lift it
    /// `jump_lift`.
    pub crest_radius: f64,
    pub crest_lift: f64,
    pub jump_lift: f64,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            flat_radius: 900.0,
            min_turn: 3.5,
            len_k: 2.6,
            len_p: 0.57,
            valley: 0.25,
            call_at: 0.4,
            grade_turn: 0.5,
            grades: [1.3, 3.3, 7.7, 17.5, 31.0, 150.0],
            long_run: 90.0,
            tighten_radius: (20.0, 130.0),
            tighten_turn: 45.0,
            tighten_ratio: 1.2,
            tighten_peak: 0.4,
            crest_radius: 370.0,
            crest_lift: 0.5,
            jump_lift: 0.9,
        }
    }
}

impl Rules {
    /// The grade (index into [`GRADES`]) of a corner.
    pub fn grade(&self, c: &Corner) -> usize {
        let score = c.radius / c.turn.powf(self.grade_turn);
        self.grades.iter().position(|&g| score <= g).unwrap_or(GRADES.len() - 1)
    }
}

/// A corner of the line: a run of one-sided curvature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corner {
    pub left: bool,
    /// Arc lengths of its first and last point and where it is called (its curvature first reaches `call_at` of the peak).
    pub start: f64,
    pub end: f64,
    pub called: f64,
    /// The turn (degrees) and the tightest radius (metres).
    pub turn: f64,
    pub radius: f64,
    /// Where it is tightest, as a fraction of its length; the radius over its first third.
    pub peak: f64,
    pub entry: f64,
    /// The length (metres) of the run of one-sided curvature the corner was cut from.
    pub run_length: f64,
}

/// A line's plan heading (unwrapped, radians, positive turning left: z up, right handed) and its rate over `2 ARM`
/// points (curvature, 1/m) at each point.
pub struct Plan {
    pub heading: Vec<f64>,
    pub curvature: Vec<f64>,
}

impl Plan {
    pub fn new(points: &[[f64; 3]], at: &[f64]) -> Plan {
        let n = points.len();
        let mut heading: Vec<f64> = Vec::with_capacity(n);
        for i in 0..n {
            let (a, b) = (points[i.saturating_sub(1)], points[(i + 1).min(n - 1)]);
            let h = (b[1] - a[1]).atan2(b[0] - a[0]);
            heading.push(heading.last().map_or(h, |&last| last + wrap(h - last)));
        }
        let curvature = (0..n)
            .map(|i| {
                let (a, b) = (i.saturating_sub(ARM), (i + ARM).min(n - 1));
                (heading[b] - heading[a]) / (at[b] - at[a]).max(1e-6)
            })
            .collect();
        Plan { heading, curvature }
    }
}

/// The corners of a line by `rules`.
pub fn corners(rules: &Rules, plan: &Plan, at: &[f64]) -> Vec<Corner> {
    let (heading, curvature, n) = (&plan.heading, &plan.curvature, at.len());
    let mut out = Vec::new();
    let flat = 1.0 / rules.flat_radius;
    let mut cut = |first: usize, last: usize, run: (usize, usize), sign: f64| {
        let turn = (heading[last] - heading[first]).abs().to_degrees();
        if turn < rules.min_turn {
            return;
        }
        let tightest = (first..=last).max_by(|&a, &b| curvature[a].abs().total_cmp(&curvature[b].abs())).unwrap();
        if at[last] - at[first] < rules.len_k * (1.0 / curvature[tightest].abs()).powf(rules.len_p) {
            return;
        }
        let called = (first..=tightest)
            .find(|&k| curvature[k].abs() >= rules.call_at * curvature[tightest].abs())
            .unwrap_or(first);
        let third = (last - first) / 3;
        let entry = (first..=first + third).map(|k| curvature[k].abs()).fold(0.0, f64::max);
        out.push(Corner {
            left: sign > 0.0,
            start: at[first],
            end: at[last],
            called: at[called],
            turn,
            radius: 1.0 / curvature[tightest].abs(),
            peak: (at[tightest] - at[first]) / (at[last] - at[first]).max(1e-6),
            entry: 1.0 / entry.max(1e-9),
            run_length: at[run.1] - at[run.0],
        });
    };
    let mut i = 0;
    while i < n {
        if curvature[i].abs() < flat {
            i += 1;
            continue;
        }
        let sign = curvature[i].signum();
        let run = (i, (i..n).take_while(|&k| curvature[k] * sign >= flat).last().unwrap());
        // A run of one side is cut at its valleys, where the bend falls under `valley` of its peak and rises again.
        let mut first = run.0;
        let (mut peak, mut valley) = (0.0_f64, (0.0_f64, first));
        for k in run.0..=run.1 {
            let c = curvature[k].abs();
            if c > peak {
                peak = c;
                valley = (c, k);
            } else if c < valley.0 {
                valley = (c, k);
            } else if valley.0 < peak * rules.valley && c > valley.0 * 1.5 {
                cut(first, valley.1, run, sign);
                (first, peak, valley) = (valley.1, c, (c, k));
            }
        }
        cut(first, run.1, run, sign);
        i = run.1 + 1;
    }
    out
}

/// The speed a car carries along the line (m/s): the corners' limit, from the standing start at `from` up and into
/// each corner down.
pub fn speeds(plan: &Plan, at: &[f64], from: f64) -> Vec<f64> {
    let n = at.len();
    let mut speed: Vec<f64> = plan.curvature.iter().map(|c| (LATERAL / c.abs().max(1e-9)).sqrt().min(TOP)).collect();
    if let Some(start) = at.iter().position(|&a| a >= from) {
        speed[start] = 0.0;
    }
    for i in 1..n {
        speed[i] = speed[i].min((speed[i - 1].powi(2) + 2.0 * ACCEL * (at[i] - at[i - 1])).sqrt());
    }
    for i in (0..n.saturating_sub(1)).rev() {
        speed[i] = speed[i].min((speed[i + 1].powi(2) + 2.0 * BRAKE * (at[i + 1] - at[i])).sqrt());
    }
    speed
}

/// A convex run of the height profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crest {
    pub distance: f64,
    /// The lift (`v^2 / R` in g) at the speed a car carries there, the vertical radius (metres) and that speed (m/s).
    pub lift: f64,
    pub radius: f64,
    pub speed: f64,
    /// The height (metres) the road falls over the 30 m after the crest and rises over the 30 m before it.
    pub fall: f64,
    pub rise: f64,
}

/// The crests of a line: runs of convex profile tighter than `crest_radius` that lift the car at `speed` by `crest_lift`
/// or more, at their most lift; runs closer than [`CREST_MERGE`] are one.
pub fn crests(rules: &Rules, points: &[[f64; 3]], at: &[f64], speed: &[f64]) -> Vec<Crest> {
    let n = points.len();
    let slope = |a: usize, b: usize| (points[b][2] - points[a][2]) / (at[b] - at[a]).max(1e-6);
    let bend: Vec<f64> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(VERTICAL_ARM), (i + VERTICAL_ARM).min(n - 1));
            (slope(i, b) - slope(a, i)) / ((at[b] - at[a]) * 0.5).max(1e-6)
        })
        .collect();
    let lift: Vec<f64> = bend.iter().zip(speed).map(|(b, v)| -b * v * v / G).collect();
    let crest = |i: usize| -bend[i] >= 1.0 / rules.crest_radius && lift[i] >= rules.crest_lift;
    let height_at = |d: f64| points[at.iter().position(|&a| a >= d).unwrap_or(n - 1)][2];
    let mut out = Vec::new();
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
        out.push(Crest {
            distance: at[top],
            lift: lift[top],
            radius: -1.0 / bend[top],
            speed: speed[top],
            fall: points[top][2] - height_at(at[top] + 30.0),
            rise: points[top][2] - height_at(at[top] - 30.0),
        });
    }
    out
}

/// The notes of a driveline: `points` (x, y, z with z up, metres) at distances `at`, between `from` and `to`.
pub fn notes(points: &[[f64; 3]], at: &[f64], from: f64, to: f64) -> Vec<Note> {
    notes_with(&Rules::default(), points, at, from, to)
}

/// [`notes`] by `rules`.
pub fn notes_with(rules: &Rules, points: &[[f64; 3]], at: &[f64], from: f64, to: f64) -> Vec<Note> {
    let n = points.len();
    if n < 2 * ARM + 2 {
        return Vec::new();
    }
    let plan = Plan::new(points, at);
    let mut out = Vec::new();
    for c in corners(rules, &plan, at) {
        if c.called < from || c.called > to {
            continue;
        }
        let grade = rules.grade(&c);
        let (left, right) = GRADES[grade];
        let mut flags = 0;
        if (2..GRADES.len() - 1).contains(&grade) && c.run_length >= rules.long_run {
            flags |= FLAG_LONG;
        }
        if (rules.tighten_radius.0..=rules.tighten_radius.1).contains(&c.radius) && c.turn >= rules.tighten_turn
            && c.entry >= rules.tighten_ratio * c.radius
            && c.peak >= rules.tighten_peak
        {
            flags |= FLAG_TIGHTENS;
        }
        out.push(Note { tag: if c.left { left } else { right }, flags, distance: c.called });
    }
    let speed = speeds(&plan, at, from);
    for c in crests(rules, points, at, &speed) {
        if c.distance >= from && c.distance <= to {
            out.push(Note { tag: if c.lift >= rules.jump_lift { TAG_JUMP } else { TAG_CREST }, flags: 0, distance: c.distance });
        }
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
            (70.0, 60.0, 3, 0),
            (100.0, -120.0, 8, FLAG_LONG),
            (300.0, 30.0, 5, FLAG_LONG),
            (800.0, 15.0, 26, 0),
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
    fn a_bend_that_tightens_is_flagged() {
        // Heading rate from 1/160 to 1/45 over 160 m, against the same turn in an even arc.
        let mut points = vec![[0.0, 0.0, 0.0]];
        let mut heading: f64 = 0.0;
        for k in 1..200 {
            let s = k as f64 * 2.5;
            if s > 200.0 {
                heading += 2.5 * (1.0 / 160.0 + (1.0 / 45.0 - 1.0 / 160.0) * ((s - 200.0) / 160.0).min(1.0));
            }
            let p = *points.last().unwrap();
            points.push([p[0] + 2.5 * heading.cos(), p[1] + 2.5 * heading.sin(), 0.0]);
        }
        let at: Vec<f64> = (0..points.len()).map(|i| i as f64 * 2.5).collect();
        let found = notes(&points, &at, 0.0, 1e9);
        assert_eq!(found[0].flags & FLAG_TIGHTENS, FLAG_TIGHTENS, "{found:?}");
        let (points, at) = track(60.0, 100.0);
        assert_eq!(notes(&points, &at, 0.0, 1e9)[0].flags & FLAG_TIGHTENS, 0);
    }

    #[test]
    fn a_sharp_crest_is_a_jump() {
        // Up at 10 %, over a crest of radius 50 m at 200 m, down, a sag, then a crest of radius 450 m at 600 m, too gentle to call.
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
        assert_eq!(found.iter().map(|n| n.tag).collect::<Vec<_>>(), [TAG_JUMP], "{found:?}");
        assert!((found[0].distance - 200.0).abs() < 5.0);
    }

    #[test]
    fn a_crest_out_of_a_hairpin_is_no_jump() {
        // A hairpin of radius 8 m from 200 m, then straight on and down at 12 % over a crest of `radius` starting `gap`
        // metres after its exit: none where the road falls away in the hairpin (Irohazaka's), a crest where the car is
        // still slow, a jump only with room to speed up.
        let (plan, at) = path(8.0, 180.0, 800.0);
        let exit = 200.0 + 8.0 * std::f64::consts::PI;
        for (gap, radius, called) in
            [(-20.0, 150.0, vec![]), (60.0, 80.0, vec![TAG_CREST]), (400.0, 80.0, vec![TAG_JUMP])]
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
