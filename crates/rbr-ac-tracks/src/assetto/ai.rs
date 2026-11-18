//! Fast-lane AI v7. The point records are sufficient for the course; trailing AI grid data stays unread.
use crate::btb::road::Line;
use std::path::Path;

/// Physics/world axes, z up: AC `(x,y,z)` becomes `(x,-z,y)`. AC's frame is right handed with z south: Monza's
/// AI line runs anticlockwise in `(x,z)` but clockwise on the ground, and `AC_START_0`'s x axis (the car's left)
/// lies clockwise of its z axis (forward). The rotation about x puts north on +y; a plain swap mirrors the track.
pub fn world(p: [f32; 3]) -> [f64; 3] {
    [f64::from(p[0]), -f64::from(p[2]), f64::from(p[1])]
}

pub fn read(path: &Path) -> Result<Line, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&data).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn parse(data: &[u8]) -> Result<Line, String> {
    if data.len() < 16 {
        return Err("truncated AI header".into());
    }
    let word = |offset| u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
    if word(0) != 7 {
        return Err(format!("unsupported AI version {}", word(0)));
    }
    let n = word(4) as usize;
    if n < 2 || n > (data.len() - 16) / 20 {
        return Err("invalid AI point count".into());
    }
    let mut points = Vec::with_capacity(n);
    for i in 0..n {
        let at = 16 + i * 20;
        let p = std::array::from_fn(|k| f32::from_le_bytes(data[at + k * 4..at + k * 4 + 4].try_into().unwrap()));
        if !p.iter().all(|v| v.is_finite()) || word(at + 16) as usize != i {
            return Err(format!("invalid AI point {i}"));
        }
        let p = world(p);
        if points.last().is_some_and(|q: &[f64; 3]| distance(*q, p) < 1e-6) {
            continue;
        }
        points.push(p);
    }
    if points.len() < 2 {
        return Err("AI line has no length".into());
    }
    if points.windows(2).any(|w| distance(w[0], w[1]) > 50.) {
        return Err("AI line has a discontinuity over 50 m".into());
    }
    Ok(Line::new(points.clone(), vec![6.; points.len()]))
}

pub fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>().sqrt()
}

/// Discard a final recording point that retraces the preceding road. Keep real hairpins: the
/// return must reverse over 150 degrees and land within 0.5 m of the last 30 m in three dimensions.
pub fn trim_terminal_return(line: &mut Line) -> usize {
    let n = line.points.len();
    if n < 4 {
        return 0;
    }
    let [a, b, c] = [line.points[n - 3], line.points[n - 2], line.points[n - 1]];
    let u = std::array::from_fn::<_, 3, _>(|k| b[k] - a[k]);
    let v = std::array::from_fn::<_, 3, _>(|k| c[k] - b[k]);
    let len = distance(b, c);
    let dot = (0..3).map(|k| u[k] * v[k]).sum::<f64>();
    if !(1. ..=30.).contains(&len) || dot / (distance(a, b) * len).max(1e-9) > -0.866 {
        return 0;
    }
    for i in (1..n - 1).rev() {
        if line.at[n - 2] - line.at[i] > 30. {
            break;
        }
        let (p, q) = (line.points[i - 1], line.points[i]);
        let d = std::array::from_fn::<_, 3, _>(|k| q[k] - p[k]);
        let t = ((0..3).map(|k| (c[k] - p[k]) * d[k]).sum::<f64>() / d.iter().map(|x| x * x).sum::<f64>().max(1e-9))
            .clamp(0., 1.);
        if distance(c, std::array::from_fn(|k| p[k] + t * d[k])) < 0.5 {
            line.points.pop();
            line.width.pop();
            *line = Line::new(std::mem::take(&mut line.points), std::mem::take(&mut line.width));
            return 1;
        }
    }
    0
}

/// The shortest lap ReRBR plays as a circuit, metres.
const MIN_LAP: f64 = 500.;

/// A closed AI loop (ends within 5 m) as a ring: the first point is repeated at the end.
pub fn close_loop(line: &Line) -> Result<Line, String> {
    let gap = distance(line.points[0], line.points[line.points.len() - 1]);
    if gap > 5. {
        return Err(format!("--circuit needs a closed AI loop; its ends are {gap:.1} m apart"));
    }
    let mut points = line.points.clone();
    let mut width = line.width.clone();
    points.push(points[0]);
    width.push(width[0]);
    let ring = Line::new(points, width);
    if ring.length() < MIN_LAP {
        return Err(format!("AI loop is only {:.0} m long", ring.length()));
    }
    Ok(ring)
}

/// Some older AI recordings begin just after the timing gate. Extend only along the endpoint's
/// existing direction, at most 30 m and within 2 m of the gate in plan. Collision checks later
/// verify every added sample. Interior gates and unrelated nearby roads leave the line unchanged.
pub fn cover_endpoint(line: &mut Line, gate: [f64; 3], start: bool) -> f64 {
    let (s, error) = project(line, gate);
    if !(1. ..=30.).contains(&error) || (start && s > 0.01) || (!start && s < line.length() - 0.01) {
        return 0.;
    }
    let (end, other) = if start {
        (line.points[0], line.points[1])
    } else {
        let n = line.points.len();
        (line.points[n - 1], line.points[n - 2])
    };
    let direction = std::array::from_fn::<_, 3, _>(|k| end[k] - other[k]);
    let den = direction[0] * direction[0] + direction[1] * direction[1];
    if den < 1e-8 {
        return 0.;
    }
    let t = ((gate[0] - end[0]) * direction[0] + (gate[1] - end[1]) * direction[1]) / den;
    let p = std::array::from_fn(|k| end[k] + t * direction[k]);
    if t <= 0. || distance(p, end) > 30. || (p[0] - gate[0]).hypot(p[1] - gate[1]) > 2. {
        return 0.;
    }
    let added = distance(p, end);
    if start {
        line.points.insert(0, p);
        line.width.insert(0, line.width[0]);
    } else {
        line.points.push(p);
        line.width.push(*line.width.last().unwrap());
    }
    *line = Line::new(std::mem::take(&mut line.points), std::mem::take(&mut line.width));
    added
}

/// Project in plan, since timing gates can sit below the road surface.
pub fn project(line: &Line, p: [f64; 3]) -> (f64, f64) {
    let mut best = (0., f64::MAX);
    for i in 1..line.points.len() {
        let (a, b) = (line.points[i - 1], line.points[i]);
        let v = [b[0] - a[0], b[1] - a[1]];
        let den = v[0] * v[0] + v[1] * v[1];
        let t = if den > 1e-9 { (((p[0] - a[0]) * v[0] + (p[1] - a[1]) * v[1]) / den).clamp(0., 1.) } else { 0. };
        let d = (p[0] - a[0] - t * v[0]).hypot(p[1] - a[1] - t * v[1]);
        if d < best.1 {
            best = (line.at[i - 1] + t * (line.at[i] - line.at[i - 1]), d);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_return_is_removed_but_a_separate_hairpin_is_kept() {
        let points = vec![[0., 0., 0.], [0., 90., 0.], [0., 100., 0.], [0., 87., 0.]];
        let mut line = Line::new(points.clone(), vec![6.; 4]);
        assert_eq!(trim_terminal_return(&mut line), 1);
        assert_eq!(line.length(), 100.);
        let mut points = points;
        points[3][0] = 2.;
        let mut line = Line::new(points, vec![6.; 4]);
        assert_eq!(trim_terminal_return(&mut line), 0);
        assert_eq!(line.points.len(), 4);
    }
    #[test]
    fn endpoint_extension_is_short_and_aligned_with_the_existing_road() {
        let mut line = Line::new(vec![[0., 0., 2.], [0., 100., 3.]], vec![6.; 2]);
        assert!(cover_endpoint(&mut line, [0., -20., 0.], true) > 20.);
        assert_eq!(line.points[0], [0., -20., 1.8]);
        assert_eq!(cover_endpoint(&mut line, [0., -100., 0.], true), 0.);
        assert_eq!(cover_endpoint(&mut line, [10., 120., 0.], false), 0.);
        assert!(cover_endpoint(&mut line, [0., 120., 0.], false) > 20.);
    }
    #[test]
    fn parses_points_in_world_axes_and_recomputes_distance() {
        let mut b = vec![];
        for n in [7u32, 2, 0, 0] {
            b.extend(n.to_le_bytes());
        }
        for (i, p) in [[1f32, 2., 3.], [4., 2., 7.]].iter().enumerate() {
            for v in p {
                b.extend(v.to_le_bytes());
            }
            b.extend(999f32.to_le_bytes());
            b.extend((i as u32).to_le_bytes());
        }
        let line = parse(&b).unwrap();
        assert_eq!(line.points[0], [1., -3., 2.]);
        assert_eq!(line.length(), 5.);
        assert!(parse(&b[..b.len() - 1]).is_err());
        assert_eq!(project(&line, [1., -3., -100.]), (0., 0.));
    }
    #[test]
    fn a_closed_loop_becomes_a_ring() {
        // A 200 m square sampled every 4 m, its last point 4 m short of the first: 800 m round.
        let square = |scale: f64| -> Vec<[f64; 3]> {
            (0..200)
                .map(|i| {
                    let (side, d) = (i / 50, f64::from(i % 50) * 4. * scale);
                    let far = 200. * scale;
                    match side {
                        0 => [d, 0., 0.],
                        1 => [far, d, 0.],
                        2 => [far - d, far, 0.],
                        _ => [0., far - d, 0.],
                    }
                })
                .collect()
        };
        let points = square(1.);
        let ring = close_loop(&Line::new(points.clone(), vec![6.; 200])).unwrap();
        assert_eq!(ring.points.len(), 201);
        assert_eq!((ring.points[0], ring.points[200]), (points[0], points[0]));
        assert!((ring.length() - 800.).abs() < 1e-9);
        let open = Line::new(vec![[0., 0., 0.], [300., 0., 0.], [600., 0., 0.]], vec![6.; 3]);
        assert!(close_loop(&open).is_err());
        let small = Line::new(square(0.1), vec![6.; 200]);
        assert!(close_loop(&small).is_err(), "80 m is no lap");
    }
}
