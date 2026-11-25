//! The ground a stage's course crosses: the cells near the line, and the ground under a point of it.
use crate::geometry::{Grid, Triangle};
use crate::road::Line;
use std::collections::HashSet;

/// Plan cells within `radius` of the line, `cell` metres square; a triangle is in when its plan box touches one.
pub struct Corridor {
    cells: HashSet<(i64, i64)>,
    cell: f64,
}

impl Corridor {
    pub fn new(line: &Line, radius: f64, cell: f64) -> Self {
        let r = (radius / cell).ceil() as i64 + 1;
        let mut cells = HashSet::new();
        for i in 0..=(line.length() / (cell * 0.5)).ceil() as usize {
            let p = line.sample((i as f64 * cell * 0.5).min(line.length())).0;
            let centre = ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64);
            for x in -r..=r {
                for y in -r..=r {
                    if x * x + y * y <= r * r {
                        cells.insert((centre.0 + x, centre.1 + y));
                    }
                }
            }
        }
        Self { cells, cell }
    }

    /// Whether the plan box of `points` touches a cell.
    pub fn contains(&self, points: &[[f64; 3]]) -> bool {
        let c = |v: f64| (v / self.cell).floor() as i64;
        let range = |k: usize| {
            let (a, b) = points.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[k]), b.max(p[k])));
            c(a)..=c(b)
        };
        range(0).any(|x| range(1).any(|y| self.cells.contains(&(x, y))))
    }
}

/// The ground under a driveline point within 3 m of its height. A collision mesh may have a gap a few centimetres
/// wide under the line (Nagao: 36 of 2 526 points, every one with ground within 0.3 m), so the nearest ground within
/// half a metre stands in for it.
pub fn ground_near<'a>(ground: &Grid<'a>, p: [f64; 3]) -> Option<(f64, &'a Triangle)> {
    let near = |x: f64, y: f64| ground.ground(x, y, p[2]).filter(|(z, _)| (*z - p[2]).abs() < 3.);
    near(p[0], p[1]).or_else(|| {
        [0.1, 0.25, 0.5].into_iter().find_map(|r| {
            (0..8).find_map(|k| {
                let a = f64::from(k) * std::f64::consts::FRAC_PI_4;
                near(p[0] + r * a.cos(), p[1] + r * a.sin())
            })
        })
    })
}
