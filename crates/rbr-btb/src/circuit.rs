//! Finding a circuit among the BTB stages that do not say they are one, and making it one (ours, `notes/session.md`).
//!
//! A circuit's own files (`track.ini` `circuit = 1`) are made ready by their converter: a closed ring starting at the
//! spawn, the start, finish and end keys far past the lap. Many BTB stages are laps drawn as a stage: a line that ends
//! where it begins. [`is_candidate`] finds those from the drive line alone and [`close`] makes the ring and the keys
//! out of the stage's lines, so the launcher can offer them as circuits (`Track::auto_circuit`) without touching the
//! player's files.

use rbr_formats::markers::{TAG_END, TAG_FINISH, TAG_START};

use crate::track::{DrivelinePoint, PaceNote};

/// Where the start, finish and end keys go: past any lap, so the 1.02 timekeeper and marker ticker never fire them.
pub const FAR: f64 = 40000.0;
/// The drive line's point spacing a bridge over the seam keeps, metres (NGP's search windows are made for it).
const SPACING: f64 = 2.5;
/// A line shorter than this is no lap, metres.
const MIN_LENGTH: f64 = 500.0;
/// The widest gap between the line's end and start, of its length: the Nordschleife's is 0.05%.
const MAX_GAP: f64 = 0.002;
/// The gap is allowed this much whatever the length, metres.
const MIN_GAP: f64 = 3.0;
/// How alike the headings at the seam must be (the cosine of the angle, 20° or less).
const HEADING: f64 = 0.94;
/// A second pass of the start this near and alike means the line is several laps, metres.
const REPASS: f64 = 5.0;

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(a, b)| (a - b) * (a - b)).sum::<f64>().sqrt()
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

/// Whether the line is one lap that ends where it starts: the end within a few metres of the start and heading the
/// same way, and no other pass of the start between (a line of several laps would time a partial lap).
pub fn is_candidate(points: &[DrivelinePoint]) -> bool {
    let (Some(first), Some(last)) = (points.first(), points.last()) else { return false };
    let length = last.distance;
    if points.len() < 50 || length < MIN_LENGTH {
        return false;
    }
    let gap = distance(first.position, last.position);
    if gap > (length * MAX_GAP).max(MIN_GAP) || dot(first.tangent, last.tangent) < HEADING {
        return false;
    }
    let (from, to) = (points.len() / 10, points.len() - points.len() / 10);
    !points[from..to]
        .iter()
        .any(|p| distance(p.position, first.position) < REPASS && dot(p.tangent, first.tangent) > HEADING)
}

/// A point `t` of the way from `a` to `b`, its tangent the blend of theirs.
fn between(a: &DrivelinePoint, b: &DrivelinePoint, t: f64) -> DrivelinePoint {
    let mix = |a: [f64; 3], b: [f64; 3]| [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t);
    let tangent = mix(a.tangent, b.tangent);
    let norm = dot(tangent, tangent).sqrt().max(1e-9);
    DrivelinePoint { position: mix(a.position, b.position), tangent: tangent.map(|v| v / norm), distance: 0.0 }
}

/// The circuit a candidate line makes: a closed ring that starts at `spawn` (its last point repeats the first), and the
/// notes moved with it. The start, finish and end keys go to [`FAR`]; every other note keeps its place on the lap.
pub fn close(points: &[DrivelinePoint], notes: &[PaceNote], spawn: [f64; 3]) -> (Vec<DrivelinePoint>, Vec<PaceNote>) {
    let mut ring = points.to_vec();
    // The last point repeats the first in some lines: the ring has it once.
    if ring.len() > 1 && distance(ring[0].position, ring[ring.len() - 1].position) < 0.01 {
        ring.pop();
    }
    // Over the seam gap.
    let (last, first) = (ring[ring.len() - 1].clone(), ring[0].clone());
    let bridge = (distance(last.position, first.position) / SPACING).ceil() as usize;
    ring.extend((1..bridge).map(|k| between(&last, &first, k as f64 / bridge as f64)));
    // Starting at the spawn.
    let at = (0..ring.len())
        .min_by(|&a, &b| distance(ring[a].position, spawn).total_cmp(&distance(ring[b].position, spawn)))
        .unwrap_or(0);
    ring.rotate_left(at);
    let mut along = 0.0;
    let mut lengths = vec![0.0];
    for pair in ring.windows(2) {
        along += distance(pair[0].position, pair[1].position);
        lengths.push(along);
    }
    let length = along + distance(ring[ring.len() - 1].position, ring[0].position);
    for (point, d) in ring.iter_mut().zip(&lengths) {
        point.distance = *d;
    }
    let mut seam = ring[0].clone();
    seam.distance = length;
    ring.push(seam);
    // The notes: each at its place on the new lap, the keys far away.
    let shift =
        points.iter().position(|p| distance(p.position, ring[0].position) < 1e-9).map_or(0.0, |k| points[k].distance);
    let mut moved: Vec<PaceNote> = notes
        .iter()
        .map(|note| {
            let distance = match note.tag {
                TAG_START => FAR,
                TAG_FINISH => FAR + 10.0,
                TAG_END => FAR + 20.0,
                _ => (note.distance - shift).rem_euclid(length),
            };
            PaceNote { distance, ..note.clone() }
        })
        .collect();
    moved.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    (ring, moved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbr_formats::markers::TAG_SPLIT;

    /// A ring of radius `radius` (m) of `n` points, from angle `from` (turns) to `to`, counter-clockwise.
    fn arc(radius: f64, n: usize, from: f64, to: f64) -> Vec<DrivelinePoint> {
        let mut points: Vec<DrivelinePoint> = (0..n)
            .map(|k| {
                let a = (from + (to - from) * k as f64 / (n - 1) as f64) * std::f64::consts::TAU;
                DrivelinePoint {
                    position: [radius * a.cos(), radius * a.sin(), 0.0],
                    tangent: [-a.sin(), a.cos(), 0.0],
                    distance: 0.0,
                }
            })
            .collect();
        let mut along = 0.0;
        for k in 1..n {
            along += distance(points[k - 1].position, points[k].position);
            points[k].distance = along;
        }
        points
    }

    #[test]
    fn a_line_that_ends_where_it_starts_is_a_candidate_and_one_that_does_not_is_not() {
        assert!(is_candidate(&arc(500.0, 400, 0.0, 1.0)), "a closed ring");
        assert!(is_candidate(&arc(500.0, 400, 0.0, 0.9985)), "a ring with a gap of 4.7 m of 3140 m");
        assert!(!is_candidate(&arc(500.0, 400, 0.0, 0.995)), "a gap of 16 m");
        assert!(!is_candidate(&arc(500.0, 400, 0.0, 0.8)), "a gap of 600 m");
        assert!(!is_candidate(&arc(500.0, 400, 0.0, 0.5)), "a half circle");
        assert!(!is_candidate(&arc(10.0, 400, 0.0, 1.0)), "too short for a lap");
        assert!(!is_candidate(&[]));
    }

    #[test]
    fn a_line_of_several_laps_is_not_a_candidate() {
        assert!(is_candidate(&arc(500.0, 400, 0.0, 1.0)));
        assert!(!is_candidate(&arc(500.0, 800, 0.0, 2.0)), "two laps pass the start again");
    }

    #[test]
    fn a_line_that_comes_back_the_other_way_is_not_a_candidate() {
        let mut line = arc(500.0, 400, 0.0, 0.5);
        let back: Vec<DrivelinePoint> =
            line.iter().rev().map(|p| DrivelinePoint { tangent: p.tangent.map(|v| -v), ..p.clone() }).collect();
        line.extend(back);
        assert!(!is_candidate(&line));
    }

    fn note(tag: u32, distance: f64) -> PaceNote {
        PaceNote { tag, distance, flag: 0 }
    }

    #[test]
    fn closing_makes_a_ring_from_the_spawn_with_the_keys_far_away() {
        // 3 turns of an almost closed ring; the spawn is 100 points in.
        let line = arc(500.0, 400, 0.0, 0.995);
        let spawn = line[100].position;
        let notes = [
            note(TAG_START, 120.0),
            note(TAG_SPLIT, line[200].distance),
            note(3, line[50].distance),
            note(TAG_FINISH, 3000.0),
            note(TAG_END, 3010.0),
        ];
        let (ring, moved) = close(&line, &notes, spawn);
        // The ring: the spawn first, the last point the first's copy, the seam bridged, the spacing kept.
        assert!(distance(ring[0].position, spawn) < 1e-9);
        let last = ring.last().unwrap();
        assert_eq!((last.position, last.tangent), (ring[0].position, ring[0].tangent));
        assert!(ring.len() > line.len(), "the gap got points");
        let widest = ring.windows(2).map(|p| distance(p[0].position, p[1].position)).fold(0.0, f64::max);
        assert!(widest < 8.0, "no segment over the old spacing: {widest}");
        let total: f64 = ring.windows(2).map(|p| distance(p[0].position, p[1].position)).sum();
        assert!((last.distance - total).abs() < 1e-6 && (ring[1].distance - widest).abs() < 8.0);
        // The notes: the keys far away, the split 100 points on, the note before the spawn at the lap's end.
        let at = |tag| moved.iter().find(|n| n.tag == tag).unwrap().distance;
        assert_eq!((at(TAG_START), at(TAG_FINISH), at(TAG_END)), (FAR, FAR + 10.0, FAR + 20.0));
        let split = at(TAG_SPLIT);
        assert!((split - (line[200].distance - line[100].distance)).abs() < 1.0, "{split}");
        let before = at(3);
        let expected = total - (line[100].distance - line[50].distance);
        assert!((before - expected).abs() < 20.0, "{before}, not {expected}, of {total}");
        assert!(moved.windows(2).all(|p| p[0].distance <= p[1].distance), "sorted");
    }

    #[test]
    fn a_ring_that_repeats_its_first_point_is_not_doubled() {
        let mut line = arc(500.0, 1300, 0.0, 1.0);
        let n = line.len();
        line[n - 1] = line[0].clone();
        line[n - 1].distance = 3141.0;
        let (ring, _) = close(&line, &[], line[0].position);
        assert_eq!(ring.len(), line.len(), "no bridge, one copy of the first point at the end");
    }
}
