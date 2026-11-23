//! A route's collision: the quadtree cells (`.vcqtc`) of its `track.jpk`, their triangles and surface codes. A cell,
//! little endian: its box (`f32` min and max, x y z), the triangle and vertex counts (stored negative), the code count
//! (8), the offsets of its vertices, its quadtree nodes, its triangles and their references, then the codes (four
//! characters each, `DRT+`). A vertex is 8 bytes, big endian: x in 24 bits, y in 16, z in 24, over the box. A
//! triangle is 4 bytes: its first vertex in 10 bits, a sheet (3 bits, unused here) and the code's slot (3 bits), then
//! the second and third vertex as offsets from the first. Layout as Ego Engine Modding's `Formats/TrackQuadTree` reads
//! DiRT Showdown's (cited in `notes/references/README.md`); on the install, every cell's triangles decode within its
//! vertices and the gate centres lie on them (`notes/dirt-rally.md`). Positions are in DiRT Rally's frame.
use crate::install::Route;

/// One collision triangle with its surface code (`GHD+`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    pub pos: [[f32; 3]; 3],
    pub code: [u8; 4],
}

impl Face {
    pub fn code(&self) -> String {
        String::from_utf8_lossy(&self.code).into_owned()
    }
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    let w = b.get(at..at + 4).ok_or("cell truncated")?;
    Ok(u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
}

fn f32_at(b: &[u8], at: usize) -> Result<f32, String> {
    u32_at(b, at).map(f32::from_bits)
}

/// The triangles of one cell.
pub fn cell(b: &[u8]) -> Result<Vec<Face>, String> {
    let min: [f32; 3] = [f32_at(b, 0)?, f32_at(b, 4)?, f32_at(b, 8)?];
    let max: [f32; 3] = [f32_at(b, 12)?, f32_at(b, 16)?, f32_at(b, 20)?];
    let triangles = (u32_at(b, 24)? as i32).unsigned_abs() as usize;
    let vertices = (u32_at(b, 28)? as i32).unsigned_abs() as usize;
    let codes = u32_at(b, 32)? as usize;
    let (vertex_at, triangle_at) = (u32_at(b, 36)? as usize, u32_at(b, 44)? as usize);
    let code: Vec<[u8; 4]> = (0..codes.min(8))
        .map(|i| b.get(52 + 4 * i..56 + 4 * i).map(|c| [c[0], c[1], c[2], c[3]]).ok_or("cell codes truncated"))
        .collect::<Result<_, _>>()?;
    let points = b.get(vertex_at..vertex_at + 8 * vertices).ok_or("cell vertices truncated")?;
    let scale = [16_777_216., 65_536., 16_777_216.];
    let points: Vec<[f32; 3]> = points
        .chunks_exact(8)
        .map(|v| {
            let q = [
                u32::from_be_bytes([0, v[0], v[1], v[2]]),
                u32::from(u16::from_be_bytes([v[3], v[4]])),
                u32::from_be_bytes([0, v[5], v[6], v[7]]),
            ];
            std::array::from_fn(|k| min[k] + (max[k] - min[k]) * (q[k] as f32 / scale[k]))
        })
        .collect();
    let faces = b.get(triangle_at..triangle_at + 4 * triangles).ok_or("cell triangles truncated")?;
    faces
        .chunks_exact(4)
        .map(|t| {
            let first = (usize::from(t[0]) << 2) | usize::from(t[1] >> 6);
            let corners = [first, first + usize::from(t[2]), first + usize::from(t[3])];
            let slot = usize::from(t[1] & 7);
            let pos = corners.map(|i| points.get(i).copied());
            match (pos, code.get(slot)) {
                ([Some(a), Some(b), Some(c)], Some(code)) => Ok(Face { pos: [a, b, c], code: *code }),
                _ => Err(format!("cell triangle {corners:?} slot {slot} past its {} vertices", points.len())),
            }
        })
        .collect()
}

/// Every collision triangle of the route.
pub fn read(route: &Route) -> Result<Vec<Face>, String> {
    let path = route.dir.join("track.jpk");
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut faces = Vec::new();
    for entry in crate::jpak::entries(&bytes).map_err(|e| format!("{}: {e}", path.display()))? {
        if entry.name.ends_with(".vcqtc") {
            faces.extend(
                cell(&bytes[entry.data.clone()]).map_err(|e| format!("{}: {}: {e}", path.display(), entry.name))?,
            );
        }
    }
    Ok(faces)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cell over the box (0,0,0)..(16,1,16) with three vertices and one triangle of slot 1.
    fn synthetic() -> Vec<u8> {
        let mut b = Vec::new();
        for f in [0f32, 0., 0., 16., 1., 16.] {
            b.extend(f.to_le_bytes());
        }
        let (vertex_at, triangle_at) = (84u32, 84 + 24);
        for w in [-1i32 as u32, -3i32 as u32, 8, vertex_at, triangle_at, triangle_at, triangle_at + 4] {
            b.extend(w.to_le_bytes());
        }
        for code in ["GHD+", "SWC+", "DEFA", "DEFA", "DEFA", "DEFA", "DEFA", "DEFA"] {
            b.extend(code.as_bytes());
        }
        // x 0, 8, 0 m (2^23 = half the box); y half; z 0, 0, 4 m.
        for (x, z) in [(0u32, 0u32), (1 << 23, 0), (0, 1 << 22)] {
            b.extend(&x.to_be_bytes()[1..]);
            b.extend(0x8000u16.to_be_bytes());
            b.extend(&z.to_be_bytes()[1..]);
        }
        b.extend([0, 1, 1, 2]);
        b
    }

    #[test]
    fn vertices_are_quantised_over_the_box_and_triangles_name_their_slot() {
        let faces = cell(&synthetic()).unwrap();
        assert_eq!(faces, [Face { pos: [[0., 0.5, 0.], [8., 0.5, 0.], [0., 0.5, 4.]], code: *b"SWC+" }]);
        let mut bad = synthetic();
        *bad.last_mut().unwrap() = 9;
        assert!(cell(&bad).is_err());
    }

    /// Over the install (`RBR_DIRT_RALLY`, the game folder): every route's cells decode, and each gate centre of
    /// ghymno_rally_01 route_0 lies on a collision triangle within 0.5 m of its height, which faces up.
    #[test]
    fn the_install_collision_lies_under_the_gates() {
        let Some(game) = std::env::var_os("RBR_DIRT_RALLY") else { return };
        let install = crate::install::Install::at(std::path::Path::new(&game)).unwrap();
        let mut total = 0;
        for route in install.routes().iter().filter(|r| r.dir.join("track.jpk").is_file()) {
            total += read(route).unwrap_or_else(|e| panic!("{e}")).len();
        }
        assert!(total > 10_000_000, "{total} triangles");
        let route = install.route("greece/ghymno_rally_01/route_0").unwrap();
        let faces = read(&route).unwrap();
        assert_eq!(faces.len(), 815_180);
        let course = crate::route::course(&route).unwrap();
        let (mut on, mut up) = (0, 0);
        for gate in &course.gates {
            let [x, y, z] = gate.crossing;
            let hit = faces.iter().find_map(|f| height(f, x, z).filter(|h| (h - y).abs() < 0.5).map(|_| f));
            if let Some(f) = hit {
                on += 1;
                up += usize::from(normal_y(f) > 0.);
            }
        }
        assert_eq!(on, course.gates.len());
        eprintln!("{up}/{on} road triangles face +y in the file's order");
    }

    fn height(f: &Face, x: f32, z: f32) -> Option<f32> {
        let [a, b, c] = f.pos;
        let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
        if d.abs() < 1e-9 {
            return None;
        }
        let l0 = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / d;
        let l1 = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / d;
        let l2 = 1. - l0 - l1;
        (l0 >= -1e-6 && l1 >= -1e-6 && l2 >= -1e-6).then(|| l0 * a[1] + l1 * b[1] + l2 * c[1])
    }

    fn normal_y(f: &Face) -> f32 {
        let [a, b, c] = f.pos;
        (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2])
    }
}
