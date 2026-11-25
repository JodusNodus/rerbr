//! AC collides with either side of a physical face; RBR only with its front. Invisible AC wall sheets are
//! often wound away from the road, so a car would pass straight through them.
use rbr_btb_writer::{
    geometry::{Grid, Triangle},
    road::Line,
};
use std::collections::HashMap;

/// Walls further than this from the driveline keep their source winding.
const REACH: f64 = 20.;

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Whether the segment from `eye` along `d` reaches `t` at or before its end (Möller–Trumbore, either side).
fn blocks(t: &Triangle, eye: [f64; 3], d: [f64; 3]) -> bool {
    let (e1, e2) = (sub(t.pos[1], t.pos[0]), sub(t.pos[2], t.pos[0]));
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-12 {
        return false;
    }
    let s = sub(eye, t.pos[0]);
    let u = dot(s, p) / det;
    let q = cross(s, e1);
    let v = dot(d, q) / det;
    let at = dot(e2, q) / det;
    u >= 0. && v >= 0. && u + v <= 1. && at > 0. && at <= 1.02
}

/// Turn steep faces that show their back to the road, unless a front face already stands before them (the far
/// side of a box, a twin sheet). World triangles face a viewer their [`Triangle::normal`] points away from: the
/// mesh frame is left handed, the world right handed. Returns the number of faces turned.
pub fn face_road(collision: &mut [Triangle], line: &Line) -> usize {
    let cell = |p: &[f64; 3]| ((p[0] / REACH).floor() as i64, (p[1] / REACH).floor() as i64);
    let mut road = HashMap::<(i64, i64), Vec<[f64; 3]>>::new();
    for p in &line.points {
        road.entry(cell(p)).or_default().push(*p);
    }
    let grid = Grid::new(collision.iter().collect(), 4.);
    let turned: Vec<usize> = (0..collision.len())
        .filter(|&i| {
            let t = &collision[i];
            let normal = t.normal();
            if normal[2].abs() >= 0.3 {
                return false;
            }
            let centre = std::array::from_fn(|k| (t.pos[0][k] + t.pos[1][k] + t.pos[2][k]) / 3.);
            let (x, y) = cell(&centre);
            let distance = |p: &[f64; 3]| dot(sub(*p, centre), sub(*p, centre));
            let Some(nearest) = (-1..=1)
                .flat_map(|dx| (-1..=1).map(move |dy| (x + dx, y + dy)))
                .filter_map(|key| road.get(&key))
                .flatten()
                .filter(|p| distance(p) <= REACH * REACH)
                .min_by(|a, b| distance(a).total_cmp(&distance(b)))
            else {
                return false;
            };
            let eye = [nearest[0], nearest[1], nearest[2] + 0.5];
            // A face wholly below the road is an embankment side, facing away by design.
            if t.pos.iter().all(|p| p[2] < eye[2] - 0.2) {
                return false;
            }
            let d = sub(centre, eye);
            dot(normal, d) < 0. && !grid.between(eye, centre).any(|o| dot(o.normal(), d) > 0. && blocks(o, eye, d))
        })
        .collect();
    drop(grid);
    for &i in &turned {
        let t = &mut collision[i];
        t.pos.swap(1, 2);
        t.uv.swap(1, 2);
        t.normal.swap(1, 2);
    }
    turned.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4 m high wall sheet at `x`, beside a road along x = 0.
    fn sheet(x: f64, faces_road: bool) -> Triangle {
        let mut pos = [[x, 0., 0.], [x, 10., 0.], [x, 5., 4.]];
        if (x > 0.) != faces_road {
            pos.swap(1, 2);
        }
        Triangle { pos, uv: [[0.; 2]; 3], normal: [[0.; 3]; 3], material: Some(64) }
    }

    #[test]
    fn exposed_backs_turn_and_covered_low_or_distant_faces_stay() {
        let line = Line::new(vec![[0., -20., 0.], [0., 5., 0.], [0., 30., 0.]], vec![6.; 3]);
        // The mesh frame is left handed, the world right handed: a face seen from x = 0 has its world normal pointing away.
        assert!(sheet(5., true).normal()[0] > 0.);
        let embankment = Triangle { pos: [[-9., 0., -3.], [-9., 10., -3.], [-9., 5., 0.]], ..sheet(-9., false) };
        // A box (5 and 8), a lone back at -5, a drop below the road and a sheet out of reach.
        let source = vec![sheet(5., true), sheet(8., false), sheet(-5., false), embankment, sheet(40., false)];
        let mut collision = source.clone();
        assert_eq!(face_road(&mut collision, &line), 1);
        assert_eq!(collision[2].pos, sheet(-5., true).pos);
        for i in [0, 1, 3, 4] {
            assert_eq!(collision[i], source[i]);
        }
        // A twin sheet already covers its back.
        assert_eq!(face_road(&mut [sheet(-5., false), sheet(-5., true)], &line), 0);
    }
}
