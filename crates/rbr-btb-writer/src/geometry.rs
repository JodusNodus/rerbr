//! Shared world geometry in metres, z up.

/// Side of the cells ReRBR merges drawn meshes into (`CELL` in its BTB stage), metres.
pub const SCENERY_CELL: f64 = 128.;
/// Side of the tiles collision meshes are grouped in, metres.
pub const COLLISION_TILE: f64 = 300.;
/// Nothing past this distance from the course is drawn: ReRBR's far plane (`coords::FAR`), metres.
pub const FAR_REACH: f64 = 10_000.;

/// One world triangle, corners in the mesh's winding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    pub pos: [[f64; 3]; 3],
    pub uv: [[f32; 2]; 3],
    /// Corner normals in the world (the mesh's, else the face's).
    pub normal: [[f32; 3]; 3],
    /// Source material index, or an RBR surface id after surface mapping.
    pub material: Option<u32>,
}

impl Triangle {
    pub fn normal(&self) -> [f64; 3] {
        let [a, b, c] = self.pos;
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if l > 0.0 {
            n.map(|x| x / l)
        } else {
            [0.0, 0.0, 0.0]
        }
    }

    /// The height of the triangle under `(x, y)`, if the point lies inside its plan view.
    pub fn height_at(&self, x: f64, y: f64) -> Option<f64> {
        let [a, b, c] = self.pos;
        let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if d.abs() < 1e-12 {
            return None;
        }
        let l0 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d;
        let l1 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d;
        let l2 = 1.0 - l0 - l1;
        let eps = -1e-9;
        (l0 >= eps && l1 >= eps && l2 >= eps).then(|| l0 * a[2] + l1 * b[2] + l2 * c[2])
    }
}

/// Triangles binned by plan-view cell for height queries.
pub struct Grid<'a> {
    cell: f64,
    bins: std::collections::HashMap<(i64, i64), Vec<u32>>,
    pub triangles: Vec<&'a Triangle>,
}

impl<'a> Grid<'a> {
    pub fn new(triangles: Vec<&'a Triangle>, cell: f64) -> Grid<'a> {
        let mut bins: std::collections::HashMap<(i64, i64), Vec<u32>> = std::collections::HashMap::new();
        for (i, t) in triangles.iter().enumerate() {
            let lo = |k: usize| t.pos.iter().map(|p| p[k]).fold(f64::MAX, f64::min);
            let hi = |k: usize| t.pos.iter().map(|p| p[k]).fold(f64::MIN, f64::max);
            let c = |v: f64| (v / cell).floor() as i64;
            for x in c(lo(0))..=c(hi(0)) {
                for y in c(lo(1))..=c(hi(1)) {
                    bins.entry((x, y)).or_default().push(i as u32);
                }
            }
        }
        Grid { cell, bins, triangles }
    }

    /// Every triangle over or under `(x, y)` with its height there.
    pub fn hits(&self, x: f64, y: f64) -> Vec<(f64, &'a Triangle)> {
        let key = ((x / self.cell).floor() as i64, (y / self.cell).floor() as i64);
        let mut out: Vec<(f64, &Triangle)> = self
            .bins
            .get(&key)
            .into_iter()
            .flatten()
            .filter_map(|&i| self.triangles[i as usize].height_at(x, y).map(|h| (h, self.triangles[i as usize])))
            .collect();
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    }

    /// The triangles in the cells under the plan box of `a` and `b`; one spanning several cells repeats.
    pub fn between(&self, a: [f64; 3], b: [f64; 3]) -> impl Iterator<Item = &'a Triangle> + '_ {
        let c = |v: f64| (v / self.cell).floor() as i64;
        (c(a[0].min(b[0]))..=c(a[0].max(b[0])))
            .flat_map(move |x| (c(a[1].min(b[1]))..=c(a[1].max(b[1]))).map(move |y| (x, y)))
            .filter_map(|key| self.bins.get(&key))
            .flatten()
            .map(|&i| self.triangles[i as usize])
    }

    /// The surface nearest to height `z` under `(x, y)`, among triangles facing up.
    pub fn ground(&self, x: f64, y: f64, z: f64) -> Option<(f64, &'a Triangle)> {
        self.hits(x, y)
            .into_iter()
            .filter(|(_, t)| t.normal()[2].abs() > 0.3)
            .min_by(|a, b| (a.0 - z).abs().total_cmp(&(b.0 - z).abs()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_inside_the_plan_view_only() {
        let t = Triangle {
            pos: [[0.0, 0.0, 1.0], [10.0, 0.0, 2.0], [0.0, 10.0, 3.0]],
            uv: [[0.0; 2]; 3],
            normal: [[0.0; 3]; 3],
            material: None,
        };
        assert!((t.height_at(5.0, 0.0).unwrap() - 1.5).abs() < 1e-9);
        assert!(t.height_at(6.0, 6.0).is_none());
        assert!(t.normal()[2] > 0.9);
        let grid = Grid::new(vec![&t], 4.0);
        assert_eq!(grid.hits(1.0, 1.0).len(), 1);
        assert!(grid.ground(1.0, 1.0, 0.0).is_some());
    }
}
