//! `route_N/trees.bin`: the trees and bushes a route places. Little endian.
//!
//! ```text
//! 0x00  u32 0, u32 12, u32 1
//! 0x0c  f32 x6        world box: min x y z, max x y z
//! 0x24  u32 types, u32 sum of the types' counts, u32 0
//! 0x30  u32 type table (0x40), u32 types, u32 item table, u32 items (the table's own count: Finland's has 3 fewer
//!       than the types' counts sum to)
//! type  u32 name offset, u32 index, f32 x6 local box of the type's mesh, u32 0, u32 items of this type   (40 bytes)
//! item  u32 type, u32 running index, f32 x9 the 3x3 (rows are the axes, scale included), f32 x3 position,
//!       f32 NaN, f32 1, u32 0, u32 0, u32 tag, u32 -1                                                       (80 bytes)
//! name  NUL terminated, a `trees.pssg` root's id without its ` Root` (`gre_olive_tree_01_b`)
//! ```
//! The item's matrix places the type's mesh (its own space, y up) in the world's frame like an `objects.ens`
//! transform. Layout read off Greece route_0 (120 types, 13394 items) and checked by the install tests: the counts, the
//! boxes and the placements on the terrain.
use crate::objects::Mat;

#[derive(Debug, Clone, PartialEq)]
pub struct Type {
    pub name: String,
    /// The mesh's box in its own space, `[min x, y, z, max x, y, z]`.
    pub bounds: [f32; 6],
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: usize,
    pub matrix: Mat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trees {
    /// The world's box, `[min x, y, z, max x, y, z]`.
    pub bounds: [f32; 6],
    pub types: Vec<Type>,
    pub items: Vec<Item>,
}

const TYPE_SIZE: usize = 40;
const ITEM_SIZE: usize = 80;

pub fn parse(bytes: &[u8]) -> Result<Trees, String> {
    let u32_at = |at: usize| {
        bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or("trees.bin: truncated")
    };
    let f32_at = |at: usize| u32_at(at).map(f32::from_bits);
    let boxes = |at: usize| -> Result<[f32; 6], String> {
        let mut b = [0.; 6];
        for (k, v) in b.iter_mut().enumerate() {
            *v = f32_at(at + 4 * k)?;
        }
        Ok(b)
    };
    let (types, items) = (u32_at(0x24)? as usize, u32_at(0x3c)? as usize);
    let (type_table, item_table) = (u32_at(0x30)? as usize, u32_at(0x38)? as usize);
    if u32_at(0x34)? as usize != types || u32_at(4)? != 12 {
        return Err("trees.bin: header counts disagree".into());
    }
    let mut out = Trees { bounds: boxes(0x0c)?, types: Vec::new(), items: Vec::new() };
    for k in 0..types {
        let at = type_table + k * TYPE_SIZE;
        let name_at = u32_at(at)? as usize;
        let name =
            bytes.get(name_at..).and_then(|b| b.split(|&c| c == 0).next()).ok_or("trees.bin: name past the end")?;
        out.types.push(Type {
            name: String::from_utf8_lossy(name).into_owned(),
            bounds: boxes(at + 8)?,
            count: u32_at(at + 36)?,
        });
    }
    for k in 0..items {
        let at = item_table + k * ITEM_SIZE;
        let kind = u32_at(at)? as usize;
        if kind >= types {
            return Err(format!("trees.bin: item {k} is of type {kind} of {types}"));
        }
        let f = |i: usize| f32_at(at + 8 + 4 * i);
        let mut matrix = [[0., 0., 0., 0.], [0., 0., 0., 0.], [0., 0., 0., 0.], [0., 0., 0., 1.]];
        for (i, row) in matrix.iter_mut().enumerate().take(4) {
            for (j, v) in row.iter_mut().enumerate().take(3) {
                *v = f(3 * i + j)?;
            }
        }
        out.items.push(Item { kind, matrix });
    }
    Ok(out)
}

pub fn read(route: &crate::install::Route) -> Result<Trees, String> {
    let path = route.dir.join("trees.bin");
    parse(&std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::tests::sample_routes;

    #[test]
    fn a_small_file_parses() {
        let mut b = vec![0u8; 0x40];
        let put = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut b, 4, 12);
        for (k, v) in [-1f32, -2., -3., 4., 5., 6.].iter().enumerate() {
            put(&mut b, 0x0c + 4 * k, v.to_bits());
        }
        let name_at = 0x40 + TYPE_SIZE + 2 * ITEM_SIZE;
        for (at, v) in [(0x24, 1), (0x28, 2), (0x30, 0x40), (0x34, 1), (0x38, 0x40 + 40), (0x3c, 2)] {
            put(&mut b, at, v);
        }
        let mut ty = vec![0u8; TYPE_SIZE];
        ty[..4].copy_from_slice(&(name_at as u32).to_le_bytes());
        ty[36..].copy_from_slice(&2u32.to_le_bytes());
        b.extend(ty);
        for k in 0..2 {
            let mut item = vec![0u8; ITEM_SIZE];
            for (i, v) in [2., 0., 0., 0., 1., 0., 0., 0., 1., 10. * k as f32, 20., 30.].iter().enumerate() {
                item[8 + 4 * i..12 + 4 * i].copy_from_slice(&f32::to_bits(*v).to_le_bytes());
            }
            b.extend(item);
        }
        b.extend(b"gre_tree\0");
        let trees = parse(&b).unwrap();
        assert_eq!(trees.bounds, [-1., -2., -3., 4., 5., 6.]);
        assert_eq!((trees.types[0].name.as_str(), trees.types[0].count), ("gre_tree", 2));
        assert_eq!(trees.items[1].matrix, [[2., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [10., 20., 30., 1.]]);
        assert!(parse(&b[..100]).is_err());
    }

    /// The heights of the collision surfaces under `(x, z)` (a road's banks overlap), by a plan lookup in cells of 8 m.
    struct Ground {
        cells: std::collections::HashMap<(i32, i32), Vec<[[f32; 3]; 3]>>,
    }
    impl Ground {
        fn new(faces: &[crate::collision::Face]) -> Ground {
            let mut cells = std::collections::HashMap::<_, Vec<_>>::new();
            for f in faces {
                let (x, z) = (f.pos.map(|p| p[0]), f.pos.map(|p| p[2]));
                let cell = |v: f32| (v / 8.).floor() as i32;
                for cx in
                    cell(x.iter().copied().fold(f32::MAX, f32::min))..=cell(x.iter().copied().fold(f32::MIN, f32::max))
                {
                    for cz in cell(z.iter().copied().fold(f32::MAX, f32::min))
                        ..=cell(z.iter().copied().fold(f32::MIN, f32::max))
                    {
                        cells.entry((cx, cz)).or_default().push(f.pos);
                    }
                }
            }
            Ground { cells }
        }
        fn heights(&self, x: f32, z: f32) -> Vec<f32> {
            let cell = (((x / 8.).floor() as i32), (z / 8.).floor() as i32);
            self.cells.get(&cell).map_or(Vec::new(), |c| {
                c.iter()
                    .filter_map(|t| {
                        let edge = |a: [f32; 3], b: [f32; 3]| (b[0] - a[0]) * (z - a[2]) - (b[2] - a[2]) * (x - a[0]);
                        let e = [edge(t[0], t[1]), edge(t[1], t[2]), edge(t[2], t[0])];
                        let inside = e.iter().all(|v| *v >= 0.) || e.iter().all(|v| *v <= 0.);
                        let sum: f32 = e.iter().sum();
                        (inside && sum != 0.).then(|| (e[1] * t[0][1] + e[2] * t[1][1] + e[0] * t[2][1]) / sum)
                    })
                    .collect()
            })
        }
    }

    #[test]
    fn the_trees_of_the_sample_routes_stand_inside_their_box_and_on_the_ground() {
        for route in sample_routes() {
            let trees = read(&route).unwrap();
            let [x0, y0, z0, x1, y1, z1] = trees.bounds;
            let mut counts = vec![0u32; trees.types.len()];
            let ground = Ground::new(&crate::collision::read(&route).unwrap());
            let (mut covered, mut near, mut worst) = (0, 0, 0f32);
            let mut offs = Vec::new();
            for item in &trees.items {
                counts[item.kind] += 1;
                let m = item.matrix;
                let [x, y, z] = [m[3][0], m[3][1], m[3][2]];
                assert!((x0..=x1).contains(&x) && (y0..=y1).contains(&y) && (z0..=z1).contains(&z), "{x} {y} {z}");
                assert!(mesh_determinant(&m) > 0.);
                // The distant tiles are placed at their middle, in the air.
                let dist = trees.types[item.kind].name.contains("_dist_");
                let off = ground.heights(x, z).into_iter().map(|h| (y - h).abs()).fold(f32::MAX, f32::min);
                if off < f32::MAX && !dist {
                    covered += 1;
                    offs.push(off);
                    near += usize::from(off < 1.5);
                    worst = worst.max(off);
                }
            }
            offs.sort_by(f32::total_cmp);
            eprintln!(
                "{}: {} types, {} items, {covered} over the collision surface, {near} within 1.5 m of it (worst {worst:.1} m)",
                route.id(),
                trees.types.len(),
                trees.items.len()
            );
            // The type counts are the items placed, but for a few the file leaves out (Finland: 3).
            let missing: u32 = trees.types.iter().zip(&counts).map(|(t, c)| t.count - c).sum();
            assert!(missing <= 3 && trees.types.iter().zip(&counts).all(|(t, c)| t.count >= *c));
            // Median 9 cm; the rest is slope under a coarse collision surface.
            assert!(covered > 1000 && near * 100 >= covered * 95 && offs[offs.len() / 2] < 0.2);
        }
    }

    fn mesh_determinant(m: &Mat) -> f32 {
        crate::mesh::determinant(m)
    }
}
