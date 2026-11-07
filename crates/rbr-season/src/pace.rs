//! A stage's reference time for a car class (ours): a speed plan along the driveline, each point's speed limited by
//! the curvature ahead and the class's grip on the surface, with the class's acceleration and braking between them.
//!
//! The acceleration and the braking share one grip budget with the cornering (the friction circle): what the corner
//! takes, the exit and the entry cannot use. The grip grows with the square of the speed (the downforce's share,
//! [`ClassPace::aero`): the fast amateurs of the hot laps hold the fast corners well past the standing-start grip.
//!
//! The driveline is sampled about every 1.5 m; the curvature at each sample is the heading change over that step in
//! the ground plane (x and y: z is up in the game's tracks), smoothed over a corner's length so one record cannot set
//! the pace. The speed limit is
//! `sqrt(grip_eff / curvature)`, capped at the class's top speed. A forward pass applies the acceleration (falling
//! linearly to 0 at top speed) and a backward pass the braking; the time is the trapezoid integral.
//!
//! `grip_eff` scales the class's dry-tarmac grip by the surface (tarmac 1, gravel 0.86, snow 0.85), on the loose by
//! the class's [`ClassPace::loose`], and by the wetness (up to -25 % soaked). The class table ([`CLASSES`]) is
//! anchored to the 1.02 AI's winning times on the retail stages (its level-2 pars) in the `WRC 2.0` row, 2003's cars
//! as 1.02's are; each other row takes the shape of its installed cars' pace in the NGP physics over the anchor's,
//! scaled to the class's time over it on the RallySimFans hot laps (`examples/class_fit.rs`). A row is the speed
//! plan's pace, not a car's figures. The calibration is in `notes/season.md`.

use crate::config::Surface;
use rbr_formats::trk::DriveLineData;

/// A car class's performance for the speed plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassPace {
    /// Lateral grip on dry tarmac (m/s²); the surface and wetness scale it.
    pub grip: f32,
    /// Acceleration from low speed (m/s²), falling to 0 at `top_speed`.
    pub accel: f32,
    /// Braking (m/s²), scaled with the grip.
    pub brake: f32,
    /// Top speed (m/s).
    pub top_speed: f32,
    /// The high-speed grip per (m/s)²: the grip at `v` is `grip · (1 + aero · v²)`. The anchor's fitted value
    /// (the downforce of the winged cars and the fast amateurs' commitment in the fast corners, `notes/season.md`
    /// "Calibration"); 0 on the other rows, a car model's own (`car_pace`).
    pub aero: f32,
    /// The share of the surface's grip the class keeps on the loose (gravel and snow): two driven wheels and old
    /// chassis lose more there than on tarmac.
    pub loose: f32,
}

/// The sample step along the driveline (m).
pub(crate) const STEP: f32 = 1.5;
/// The curvature smoothing radius (samples each side): 12 samples at 1.5 m is a ~37 m window, about a corner's
/// length, so the line's records do not set a corner's speed on their own; the RallySimFans hot laps' stage times
/// are closest with it (`notes/season.md` "Calibration").
const SMOOTH: usize = 12;
/// The slowest corner speed (m/s): below this the plan would crawl through hairpins no car crawls through.
const MIN_CORNER: f32 = 4.0;

/// A class's row in [`CLASSES`]: its name, the spellings it matches and its pace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassRow {
    pub name: &'static str,
    /// Matched against the lower-cased alphanumeric spelling, the first row with one contained wins.
    pub keys: &'static [&'static str],
    pub pace: ClassPace,
}

#[allow(clippy::too_many_arguments)]
const fn row(
    name: &'static str,
    keys: &'static [&'static str],
    grip: f32,
    accel: f32,
    brake: f32,
    top_speed: f32,
    loose: f32,
    aero: f32,
) -> ClassRow {
    ClassRow { name, keys, pace: ClassPace { grip, accel, brake, top_speed, aero, loose } }
}

/// The class table, in match order (`super1600` holds an `r1` and `super2000` an `r2`, so they come first). The
/// plain `WRC` row (2003's cars, `WRC 2.0`) is the anchor, the 1.02 AI's winning times on the retail stages; the
/// others follow the RallySimFans hot laps' class effects over it (`notes/season.md` "Calibration").
pub const CLASSES: [ClassRow; 23] = [
    row("WRC 1.6", &["wrc16", "rally1", "rallye1"], 10.19, 10.57, 12.10, 60.3, 1.023, 0.0),
    row("WRC 2.0", &["wrc"], 8.88, 8.51, 10.64, 55.8, 1.000, 0.0002),
    row("Group B", &["groupb", "grpb"], 8.39, 7.86, 9.91, 55.1, 0.991, 0.0),
    row("Group RGT", &["rgt"], 8.37, 7.38, 9.40, 59.9, 0.887, 0.0),
    row("Group A7", &["groupa7", "grpa7", "kitcar"], 8.99, 5.72, 9.73, 61.3, 0.949, 0.0),
    row("Group A6", &["groupa6", "grpa6"], 8.54, 4.75, 9.65, 52.2, 1.035, 0.0),
    row("Group A5", &["groupa5", "grpa5"], 7.70, 4.32, 9.83, 39.8, 1.050, 0.0),
    row("Group A8", &["groupa", "grpa"], 8.83, 7.73, 10.25, 55.9, 1.010, 0.0),
    row("Super 2000", &["s2000", "2000"], 9.29, 8.90, 10.99, 57.2, 1.024, 0.0),
    row("Super 1600", &["s1600", "1600"], 9.01, 5.52, 9.89, 63.4, 0.963, 0.0),
    row("Group N4", &["n4", "groupn", "grpn"], 8.75, 6.42, 9.82, 56.6, 1.009, 0.0),
    row("Group R4", &["r4"], 9.29, 8.30, 11.34, 56.8, 1.021, 0.0),
    row("Rally 2", &["rally2"], 10.09, 9.16, 11.89, 59.2, 1.029, 0.0),
    row("Group R5", &["r5"], 9.58, 8.42, 11.14, 57.3, 1.023, 0.0),
    row("Rally 3", &["rally3"], 9.28, 7.31, 10.82, 56.1, 1.023, 0.0),
    row("Group R3", &["r3"], 8.96, 5.41, 10.04, 61.3, 0.969, 0.0),
    row("Rally 4", &["rally4"], 9.10, 5.37, 9.82, 59.4, 0.990, 0.0),
    row("Group R2", &["r2"], 8.72, 5.14, 9.95, 55.7, 0.995, 0.0),
    row("Rally 5", &["rally5"], 9.03, 5.16, 9.71, 55.9, 0.968, 0.0),
    row("Group R1", &["r1"], 7.91, 4.00, 9.19, 47.1, 0.994, 0.0),
    row("Group 4", &["group4", "grp4", "historic"], 8.25, 7.06, 8.95, 53.9, 0.998, 0.0),
    row("Group 2", &["group2", "grp2"], 7.32, 4.29, 9.33, 39.0, 1.013, 0.0),
    // Unknown groups: the middle class, Rally 3's pace.
    row("", &[""], 9.28, 7.31, 10.82, 56.1, 1.023, 0.0),
];

/// The pace of a catalog car group (`Group R5`, `WRC 2.0`, `Group B`, ...); unknown groups get a middle class.
///
/// The match is on the lower-cased alphanumeric spelling, so the sidebar class names (`Rally 2`), the RSF base
/// groups (`Group R5`) and the per-car groups (`Ford Fiesta R5`) all land in the same table.
pub fn class_pace(class: &str) -> ClassPace {
    let n: String = class.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    CLASSES.iter().find(|r| r.keys.iter().any(|k| n.contains(k))).map_or(CLASSES[CLASSES.len() - 1].pace, |r| r.pace)
}

/// The surface's share of the dry-tarmac grip.
fn surface_grip(surface: Surface) -> f32 {
    match surface {
        Surface::Tarmac => 1.0,
        Surface::Gravel => 0.86,
        Surface::Snow => 0.85,
    }
}

/// A stage's timed road: the driveline's spline points ([`driveline_points`]) from the start marker to the finish,
/// and the split markers (m from the start). The pace calibration and the launcher both time this.
#[derive(Debug, Clone, PartialEq)]
pub struct TimedRoad {
    pub points: Vec<[f32; 3]>,
    pub splits: Vec<f32>,
}

/// The timed road of `trk` between `markers`' start and finish (its `.dls`); the whole line without them, or when
/// they do not fit the line.
pub fn timed_road(trk: &DriveLineData, markers: Option<&rbr_formats::markers::StageMarkers>) -> TimedRoad {
    let points = driveline_points(trk);
    let length = path_length(&points);
    let Some(m) = markers else { return TimedRoad { points, splits: Vec::new() } };
    let (start, finish) = (m.start().unwrap_or(0.0), m.finish().unwrap_or(length));
    let (start, finish) = if 0.0 <= start && start < finish { (start, finish.min(length)) } else { (0.0, length) };
    let splits = m.splits().filter(|&d| d > start && d < finish).map(|d| d - start).collect();
    TimedRoad { points: timed_slice(&points, start, finish), splits }
}

/// The driveline's points in order (m): the `.trk` line through its Hermite spline, the line the game drives (the
/// autopilot's `point`: `vectors[0]` is the position, `vectors[1]` the tangent, over-long tangents fitted back to
/// the chord). Sampled finely enough that [`reference_time`] sees the corners, not the records.
pub fn driveline_points(trk: &DriveLineData) -> Vec<[f32; 3]> {
    const SUBDIV: usize = 4;
    let at = |v: rbr_formats::trk::Vec3| [v.x, v.y, v.z];
    let records = &trk.records;
    if records.len() < 2 {
        return records.iter().map(|r| at(r.vectors[0])).collect();
    }
    let mut out = Vec::with_capacity(records.len() * SUBDIV + 1);
    for w in records.windows(2) {
        let (p0, p1) = (at(w[0].vectors[0]), at(w[1].vectors[0]));
        let (m0, m1) = (fit_tangent(&w[0], &w[1]), fit_tangent(&w[1], &w[0]));
        for k in 0..SUBDIV {
            out.push(hermite(p0, m0, p1, m1, k as f32 / SUBDIV as f32));
        }
    }
    out.push(at(records.last().unwrap().vectors[0]));
    out
}

/// The points of `points` between `start` and `finish` (m along the line), with interpolated ends.
fn timed_slice(points: &[[f32; 3]], start: f32, finish: f32) -> Vec<[f32; 3]> {
    if points.len() < 2 || finish <= start {
        return points.to_vec();
    }
    let mut cum = vec![0.0f32; points.len()];
    for i in 1..points.len() {
        cum[i] = cum[i - 1] + dist(points[i - 1], points[i]);
    }
    let total = cum[points.len() - 1];
    let (start, finish) = (start.clamp(0.0, total), finish.clamp(0.0, total));
    if finish <= start {
        return points.to_vec();
    }
    let lerp = |d: f32| {
        let i = cum.partition_point(|&c| c < d).min(points.len() - 1);
        if i == 0 {
            return points[0];
        }
        let (a, b) = (points[i - 1], points[i]);
        let span = (cum[i] - cum[i - 1]).max(1e-6);
        let t = ((d - cum[i - 1]) / span).clamp(0.0, 1.0);
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
    };
    let mut out = vec![lerp(start)];
    for (i, &c) in cum.iter().enumerate() {
        if c > start && c < finish {
            out.push(points[i]);
        }
    }
    out.push(lerp(finish));
    out
}

/// A Hermite sample (the autopilot's `point` in `rbr-standalone/src/autopilot`).
fn hermite(p0: [f32; 3], m0: [f32; 3], p1: [f32; 3], m1: [f32; 3], t: f32) -> [f32; 3] {
    let (t2, t3) = (t * t, t * t * t);
    let h = [2.0 * t3 - 3.0 * t2 + 1.0, t3 - 2.0 * t2 + t, -2.0 * t3 + 3.0 * t2, t3 - t2];
    std::array::from_fn(|k| h[0] * p0[k] + h[1] * m0[k] + h[2] * p1[k] + h[3] * m1[k])
}

/// One record's tangent fitted back to its chord with the next, as the autopilot fits them.
fn fit_tangent(record: &rbr_formats::trk::DriveLineRecord, next: &rbr_formats::trk::DriveLineRecord) -> [f32; 3] {
    const LOOP: f32 = 8.0;
    const BACK: f32 = 1.5;
    let at = |v: rbr_formats::trk::Vec3| [v.x, v.y, v.z];
    let (p0, p1) = (at(record.vectors[0]), at(next.vectors[0]));
    let m = at(record.vectors[1]);
    let chord = dist(p0, p1);
    if chord == 0.0 {
        return m;
    }
    let length = dot(m, m).sqrt();
    if length <= chord {
        return m;
    }
    // How far the segment runs back along its chord (the autopilot's `runs_back`, sampled).
    let mut ahead = 0.0f32;
    let mut back = 0.0f32;
    for k in 1..=16 {
        let along = dot(sub(hermite(p0, m, p1, m, k as f32 / 16.0), p0), sub(p1, p0)) / chord;
        ahead = ahead.max(along);
        back = back.max(ahead - along);
    }
    if back > BACK || length > LOOP * chord {
        m.map(|x| x * chord / length)
    } else {
        m
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|k| a[k] - b[k])
}

/// The polyline's length (m).
pub fn path_length(points: &[[f32; 3]]) -> f32 {
    points.windows(2).map(|w| dist(w[0], w[1])).sum()
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt()
}

/// The driveline resampled to `STEP` metres: walks the polyline, interpolating between points.
fn resample(points: &[[f32; 3]]) -> Vec<[f32; 3]> {
    let mut out = Vec::new();
    if points.len() < 2 {
        out.extend_from_slice(points);
        return out;
    }
    out.push(points[0]);
    let mut carry = 0.0f32;
    for w in points.windows(2) {
        let (mut a, b) = (w[0], w[1]);
        let mut seg = dist(a, b);
        if seg == 0.0 {
            continue;
        }
        while carry + seg >= STEP {
            let t = (STEP - carry) / seg;
            a = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
            out.push(a);
            seg = dist(a, b);
            carry = 0.0;
        }
        carry += seg;
    }
    if out.len() < 2 {
        out.push(*points.last().unwrap());
    }
    out
}

/// A speed plan along a driveline: the samples (every `STEP` metres), the speed limit at each and the planned speed.
pub(crate) struct Plan {
    pub samples: Vec<[f32; 3]>,
    pub limit: Vec<f32>,
    pub speed: Vec<f32>,
    pub top_speed: f32,
}

impl Plan {
    /// The time (s) from sample `from` to sample `to`.
    pub fn time(&self, from: usize, to: usize) -> f32 {
        (from..to.min(self.samples.len() - 1)).map(|i| 2.0 * STEP / (self.speed[i] + self.speed[i + 1]).max(0.5)).sum()
    }

    /// The sample at `distance` metres along the plan.
    pub fn index(&self, distance: f32) -> usize {
        ((distance / STEP).round().max(0.0) as usize).min(self.samples.len() - 1)
    }
}

/// The speed plan of a stage over `driveline` on `surface`, `wetness` 0 (dry) to 1; none for a line too short to sample.
pub(crate) fn plan(driveline: &[[f32; 3]], surface: Surface, wetness: f32, class: &ClassPace) -> Option<Plan> {
    let wet = wetness.clamp(0.0, 1.0);
    let samples = resample(driveline);
    if samples.len() < 3 {
        return None;
    }
    let loose = if surface == Surface::Tarmac { 1.0 } else { class.loose };
    let grip_eff = class.grip * surface_grip(surface) * loose * (1.0 - 0.25 * wet);
    let brake_eff = class.brake * grip_eff / class.grip;
    // Curvature from the signed heading change per step in the ground plane (x, y). The signs are smoothed first: the
    // records' lateral jitter weaves left and right and cancels out, while a real corner keeps its sign.
    let mut turn = vec![0.0f32; samples.len()];
    for i in 1..samples.len() - 1 {
        let (ax, ay) = (samples[i][0] - samples[i - 1][0], samples[i][1] - samples[i - 1][1]);
        let (bx, by) = (samples[i + 1][0] - samples[i][0], samples[i + 1][1] - samples[i][1]);
        let (la, lb) = ((ax * ax + ay * ay).sqrt(), (bx * bx + by * by).sqrt());
        if la > 0.0 && lb > 0.0 {
            let cross = (ax * by - ay * bx) / (la * lb);
            turn[i] = cross.clamp(-1.0, 1.0).asin() / STEP;
        }
    }
    let mut smooth = vec![0.0f32; samples.len()];
    for (i, s) in smooth.iter_mut().enumerate() {
        let (lo, hi) = (i.saturating_sub(SMOOTH), (i + SMOOTH + 1).min(samples.len()));
        *s = turn[lo..hi].iter().sum::<f32>() / (hi - lo) as f32;
    }
    let mut curve = vec![0.0f32; samples.len()];
    let mut limit = vec![0.0f32; samples.len()];
    for (i, &k) in smooth.iter().enumerate() {
        let k = k.abs();
        curve[i] = k;
        // The corner's speed where the grip, the downforce's included, holds the curvature: v² k = grip (1 + aero v²).
        let hold = k - grip_eff * class.aero;
        limit[i] =
            if hold <= 0.0 { class.top_speed } else { (grip_eff / hold).sqrt().min(class.top_speed).max(MIN_CORNER) };
    }
    // Forward pass: the acceleration falls linearly to 0 at top speed, within what the corner leaves of the grip.
    let mut speed = vec![0.0f32; samples.len()];
    speed[0] = limit[0].min(20.0);
    for i in 0..samples.len() - 1 {
        let grip = grip_eff * (1.0 + class.aero * speed[i] * speed[i]);
        let lateral = speed[i] * speed[i] * curve[i];
        let avail = (grip * grip - lateral * lateral).max(0.0).sqrt();
        let engine = (class.accel * (1.0 - speed[i] / class.top_speed)).max(0.0);
        let a = engine.min(avail);
        let v = (speed[i] * speed[i] + 2.0 * a * STEP).sqrt();
        speed[i + 1] = v.min(limit[i + 1]);
    }
    // Backward pass: the braking, scaled with the grip, within the same budget.
    for i in (0..samples.len() - 1).rev() {
        let v2 = speed[i + 1] * speed[i + 1];
        let grip = grip_eff * (1.0 + class.aero * v2);
        let lateral = v2 * curve[i + 1];
        let avail = (grip * grip - lateral * lateral).max(0.0).sqrt();
        let brakes = brake_eff * (1.0 + class.aero * v2);
        let b = brakes.min(avail);
        let v = (v2 + 2.0 * b * STEP).sqrt();
        speed[i] = speed[i].min(v);
    }
    Some(Plan { samples, limit, speed, top_speed: class.top_speed })
}

/// The winning time (s) of a stage over `driveline` (its points in order, m) on `surface`, `wetness` 0 (dry) to 1.
pub fn reference_time(driveline: &[[f32; 3]], surface: Surface, wetness: f32, class: &ClassPace) -> f32 {
    match plan(driveline, surface, wetness, class) {
        Some(plan) => plan.time(0, plan.samples.len() - 1),
        None => path_length(driveline) / (class.top_speed * 0.7).max(1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight(n: usize, step: f32) -> Vec<[f32; 3]> {
        (0..n).map(|i| [i as f32 * step, 0.0, 0.0]).collect()
    }

    fn circle(radius: f32, n: usize) -> Vec<[f32; 3]> {
        (0..n)
            .map(|i| {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                [radius * a.cos(), radius * a.sin(), 0.0]
            })
            .collect()
    }

    #[test]
    fn a_straight_runs_near_top_speed_and_a_hairpin_crawls() {
        let class = class_pace("WRC 2.0");
        let line = straight(200, 10.0);
        let time = reference_time(&line, Surface::Gravel, 0.0, &class);
        let length = path_length(&line);
        let avg = length / time;
        assert!(avg > 0.85 * class.top_speed, "straight avg {avg} of top {}", class.top_speed);
        let hairpin = circle(12.0, 200);
        let slow = reference_time(&hairpin, Surface::Gravel, 0.0, &class);
        let quick = reference_time(&hairpin, Surface::Tarmac, 0.0, &class);
        assert!(slow > quick, "gravel {slow} should exceed tarmac {quick}");
        // Same shape, same length class: the hairpin lap takes a multiple of the straight's pace.
        let lap = path_length(&hairpin);
        let laps = lap / length;
        assert!(slow > time * laps * 1.5, "hairpin {slow} should crawl vs straight {time} (lap share {laps})");
    }

    #[test]
    fn wet_and_snow_slow_the_reference() {
        let class = class_pace("Rally 2");
        let line = circle(60.0, 300);
        let dry = reference_time(&line, Surface::Gravel, 0.0, &class);
        let wet = reference_time(&line, Surface::Gravel, 1.0, &class);
        let snow = reference_time(&line, Surface::Snow, 0.0, &class);
        assert!(dry < wet && wet < snow + dry, "dry {dry} wet {wet} snow {snow}");
        // Snow on studs holds nearly as much as gravel (the hot laps: 0.85 of tarmac against gravel's 0.86).
        assert!(snow > dry && snow < dry * 1.05, "snow {snow} against dry gravel {dry}");
    }

    #[test]
    fn every_known_group_lands_in_its_table() {
        assert_eq!(class_pace("WRC 1.6"), class_pace("Rally1"));
        assert_eq!(class_pace("WRC"), class_pace("WRC 2.0"));
        assert_eq!(class_pace("Rally 2 / R5"), class_pace("Rally2"));
        assert_eq!(class_pace("Group B"), class_pace("GroupB"));
        // Spellings holding another's key land in their own row.
        assert_eq!(class_pace("Super 1600"), CLASSES.iter().find(|r| r.name == "Super 1600").unwrap().pace);
        assert_eq!(class_pace("Super 2000"), CLASSES.iter().find(|r| r.name == "Super 2000").unwrap().pace);
        assert_eq!(class_pace("Group A7"), class_pace("Kit Car"));
        // Every row's own name finds it, and unknown groups get the middle class.
        for row in CLASSES.iter().filter(|r| !r.name.is_empty()) {
            assert_eq!(class_pace(row.name), row.pace, "{}", row.name);
        }
        assert_eq!(class_pace("Flying Saucers"), class_pace("Rally3"));
    }

    #[test]
    fn short_or_empty_drivelines_still_give_a_time() {
        let class = class_pace("WRC");
        assert!(reference_time(&[], Surface::Gravel, 0.0, &class) == 0.0);
        let t = reference_time(&[[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]], Surface::Gravel, 0.0, &class);
        assert!(t > 0.0 && t < 60.0, "{t}");
    }
}
