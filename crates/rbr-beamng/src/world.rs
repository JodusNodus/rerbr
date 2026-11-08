//! Placed shapes as world triangles: a level's `TSStatic`s read through [`crate::cdae`], each object's mesh for the
//! wanted detail level transformed by its node and the static's transform, in BeamNG's frame (z up, metres).

use crate::cdae::{self, Shape};
use crate::level::Static;

/// Which meshes of a shape to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// The most detailed drawn level (the largest non-negative size).
    Visible,
    /// The collision levels (negative size, `Collision-*`), all of them.
    Collision,
    /// The least detailed drawn level that holds triangles (per subshape).
    Lowest,
}

/// One world triangle, corners in the mesh's winding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    pub pos: [[f64; 3]; 3],
    pub uv: [[f32; 2]; 3],
    /// Corner normals in the world (the mesh's, else the face's).
    pub normal: [[f32; 3]; 3],
    /// Index into [`Part::materials`]; `None` for `NoMaterial`.
    pub material: Option<u32>,
}

/// The triangles of one shape object.
#[derive(Debug, Clone, Default)]
pub struct Part {
    pub object: String,
    pub triangles: Vec<Triangle>,
    /// The shape's material names (the mesh material list).
    pub materials: Vec<String>,
}

/// The detail levels to read for `want`, as indices into the shape's details.
fn details(shape: &Shape, want: Detail) -> Vec<usize> {
    match want {
        Detail::Collision => (0..shape.details.len()).filter(|&d| shape.details[d].size < 0.0).collect(),
        Detail::Lowest => {
            let filled = |d: usize| {
                (0..shape.objects.len()).any(|o| {
                    shape
                        .object_meshes(o)
                        .iter()
                        .any(|(m, dd)| std::ptr::eq(*dd, &shape.details[d]) && !shape.meshes[*m].indices.is_empty())
                })
            };
            let mut out: Vec<usize> = Vec::new();
            for (i, d) in shape.details.iter().enumerate() {
                if d.size < 0.0 || !filled(i) {
                    continue;
                }
                match out.iter_mut().find(|j| shape.details[**j].sub_shape == d.sub_shape) {
                    Some(j) if shape.details[*j].size > d.size => *j = i,
                    Some(_) => {}
                    None => out.push(i),
                }
            }
            out
        }
        Detail::Visible => {
            // Per subshape, the largest drawn size.
            let mut out: Vec<usize> = Vec::new();
            for (i, d) in shape.details.iter().enumerate() {
                if d.size < 0.0 {
                    continue;
                }
                match out.iter_mut().find(|j| shape.details[**j].sub_shape == d.sub_shape) {
                    Some(j) if shape.details[*j].size < d.size => *j = i,
                    Some(_) => {}
                    None => out.push(i),
                }
            }
            out
        }
    }
}

/// The parts of a placed shape for `want`.
pub fn parts(shape: &Shape, placed: &Static, want: Detail) -> Vec<Part> {
    let wanted = details(shape, want);
    let world = placed.transform();
    let mut out = Vec::new();
    for (object, o) in shape.objects.iter().enumerate() {
        let node = cdae::compose(&world, &shape.node_transform(usize::try_from(o.node).unwrap_or(0)));
        let mut triangles = Vec::new();
        for (mesh, _) in shape
            .object_meshes(object)
            .into_iter()
            .filter(|(_, d)| wanted.iter().any(|&w| std::ptr::eq(&shape.details[w], *d)))
        {
            let data = shape.mesh_vertices(mesh);
            let pos: Vec<[f64; 3]> = data.verts.iter().map(|v| cdae::apply(&node, v.map(f64::from))).collect();
            let norms: Vec<[f32; 3]> = if data.norms.len() == data.verts.len() {
                data.norms.iter().map(|n| rotate(&node, *n)).collect()
            } else {
                Vec::new()
            };
            for (t, word) in shape.meshes[mesh].triangles() {
                let corner = |k: usize| t[k] as usize;
                if (0..3).any(|k| corner(k) >= pos.len()) {
                    continue;
                }
                let uv = |k: usize| data.tverts.get(corner(k)).copied().unwrap_or([0.0; 2]);
                let material = cdae::Primitive { start: 0, count: 0, material: word }.material_index();
                let mut t = Triangle {
                    pos: [pos[corner(0)], pos[corner(1)], pos[corner(2)]],
                    uv: [uv(0), uv(1), uv(2)],
                    normal: [[0.0; 3]; 3],
                    material: material.map(|m| m as u32),
                };
                t.normal = if norms.is_empty() {
                    [t.normal().map(|v| v as f32); 3]
                } else {
                    [norms[corner(0)], norms[corner(1)], norms[corner(2)]]
                };
                triangles.push(t);
            }
        }
        if !triangles.is_empty() {
            out.push(Part { object: shape.name(o.name).to_owned(), triangles, materials: shape.materials.clone() });
        }
    }
    out
}

/// A direction through the transform's 3x3 part, unit length.
fn rotate(m: &[[f64; 4]; 3], n: [f32; 3]) -> [f32; 3] {
    let v: [f64; 3] =
        std::array::from_fn(|i| m[i][0] * f64::from(n[0]) + m[i][1] * f64::from(n[1]) + m[i][2] * f64::from(n[2]));
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 0.0 {
        v.map(|x| (x / l) as f32)
    } else {
        [0.0, 0.0, 1.0]
    }
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
