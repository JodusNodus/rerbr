//! The solid props of a route as collision: the physics hulls DiRT Rally gives each entity (`objects::Shape`), placed
//! with the instance's matrix, as closed low-polygon solids (boxes, eight-sided prisms) in the stage's world. Vertical
//! axes are y in DiRT Rally's frame; the triangles go into the stage's collision with the ground's.
use crate::frame;
use crate::mesh::{self, Mat};
use crate::objects::{Hull, Shape};
use rbr_ac_tracks::btb::geometry::Triangle;

/// Sides of a round hull (cylinder, capsule, sphere, cone) over `THIN` metres of radius, and of a thinner one (a trunk).
const SIDES: usize = 8;
const THIN_SIDES: usize = 6;
const THIN: f32 = 0.4;

type P = [f32; 3];

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: P, b: P) -> P {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: P, b: P) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A prism, frustum or cone about y, from `y0` to `y1`, with the radii at both ends (0 is an apex), and its end caps
/// (the lower one only `below`). Its polygon is `THIN_SIDES` sided where the radius is under `THIN`, else `SIDES`.
fn prism(y0: f32, y1: f32, r0: f32, r1: f32, below: bool) -> (Vec<P>, Vec<[usize; 3]>) {
    let sides = if r0.max(r1) < THIN { THIN_SIDES } else { SIDES };
    // A polygon of this vertex radius keeps the round hull's radius at the edges' middles to 5 to 10 %.
    let corner = 1. / (std::f32::consts::PI / sides as f32).cos();
    let ring = |y: f32, r: f32, v: &mut Vec<P>| {
        (0..sides).for_each(|i| {
            let a = std::f32::consts::TAU * i as f32 / sides as f32;
            v.push([r * corner * a.cos(), y, r * corner * a.sin()]);
        })
    };
    let mut v = Vec::new();
    ring(y0, r0, &mut v);
    ring(y1, r1, &mut v);
    v.push([0., y0, 0.]);
    v.push([0., y1, 0.]);
    let (c0, c1) = (2 * sides, 2 * sides + 1);
    let mut t = Vec::new();
    for i in 0..sides {
        let j = (i + 1) % sides;
        t.push([i, j, sides + j]);
        t.push([i, sides + j, sides + i]);
        if below {
            t.push([c0, j, i]);
        }
        t.push([c1, sides + i, sides + j]);
    }
    (v, t)
}

/// The hull as vertices and triangles about its own centre, outward facing (right handed: the normal by `(b-a)×(c-a)`
/// points out), degenerate ones (an apex's) left out.
fn hull(h: &Hull, below: bool) -> (Vec<P>, Vec<[usize; 3]>) {
    let (v, t) = match *h {
        Hull::Box([w, h, l]) => {
            let v: Vec<P> = (0..8)
                .map(|i| [(i & 1) as f32 - 0.5, ((i >> 1) & 1) as f32 - 0.5, ((i >> 2) & 1) as f32 - 0.5])
                .map(|p| [p[0] * w, p[1] * h, p[2] * l])
                .collect();
            let quads = [[0, 1, 3, 2], [4, 5, 7, 6], [0, 1, 5, 4], [2, 3, 7, 6], [0, 2, 6, 4], [1, 3, 7, 5]];
            (v, quads.iter().flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect())
        }
        Hull::Cylinder { height, radius } => prism(-height / 2., height / 2., radius, radius, below),
        Hull::Capsule { height, radius } => prism(-height / 2. - radius, height / 2. + radius, radius, radius, below),
        Hull::Sphere { radius } => prism(-radius, radius, radius, radius, below),
        Hull::Cone { height, radius } => prism(-height / 2., height / 2., radius, 0., below),
    };
    let t = t
        .into_iter()
        .filter_map(|[a, b, c]| {
            let n = cross(sub(v[b], v[a]), sub(v[c], v[a]));
            if dot(n, n) < 1e-12 {
                return None;
            }
            // Convex about the origin: the face points away from it.
            let out = dot(n, [v[a][0] + v[b][0] + v[c][0], v[a][1] + v[b][1] + v[c][1], v[a][2] + v[b][2] + v[c][2]]);
            Some(if out < 0. { [a, c, b] } else { [a, b, c] })
        })
        .collect();
    (v, t)
}

/// The triangles of `shape` of an entity placed at `instance`, in the stage's world as the collision's (the BTB mesh's
/// winding), with no surface (the stage's default).
pub fn triangles(shape: &Shape, instance: &Mat) -> Vec<Triangle> {
    let matrix = mesh::mul(&shape.matrix, instance);
    let mirrored = mesh::determinant(&matrix) < 0.;
    // An upright prism's foot is in the ground: no lower cap.
    let row: P = matrix[1][..3].try_into().unwrap_or_default();
    let upright = row[1] > 0.9 * dot(row, row).sqrt();
    let (v, t) = hull(&shape.hull, !upright);
    let world: Vec<[f64; 3]> = v.iter().map(|&p| frame::world(mesh::point(&matrix, p))).collect();
    t.into_iter()
        .map(|[a, b, c]| {
            // The file's winding is counter-clockwise about +y, the BTB mesh's the other way ([`frame`]); a mirrored
            // placement turned it already.
            let corners = if mirrored { [a, b, c] } else { [a, c, b] };
            Triangle { pos: corners.map(|i| world[i]), uv: [[0.; 2]; 3], normal: [[0.; 3]; 3], material: None }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(hull: Hull) -> Shape {
        Shape { hull, matrix: mesh::IDENTITY, material: "sWood".into(), impulse: None }
    }

    /// Volume by the divergence theorem: positive for outward faces, the solid's own for a closed one.
    fn volume(t: &[Triangle]) -> f64 {
        t.iter()
            .map(|t| {
                let [a, b, c] = t.pos;
                let n = [
                    (b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1]),
                    (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]),
                    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]),
                ];
                (a[0] * n[0] + a[1] * n[1] + a[2] * n[2]) / 6.
            })
            .sum()
    }

    #[test]
    fn a_box_is_closed_and_its_size() {
        let t = triangles(&shape(Hull::Box([2., 3., 4.])), &mesh::IDENTITY);
        assert_eq!(t.len(), 12);
        // The BTB winding is the other way round: the volume is the box's, negative.
        assert!((volume(&t) + 24.).abs() < 1e-4, "{}", volume(&t));
    }

    #[test]
    fn a_cylinder_is_a_prism_about_its_radius_and_a_mirror_keeps_the_winding_of_the_world() {
        // Lying (turned a quarter about x), so it has both caps.
        let mut lying = mesh::IDENTITY;
        (lying[1][1], lying[1][2], lying[2][1], lying[2][2]) = (0., 1., -1., 0.);
        let cylinder = Shape { matrix: lying, ..shape(Hull::Cylinder { height: 4., radius: 0.5 }) };
        let t = triangles(&cylinder, &mesh::IDENTITY);
        assert_eq!(t.len(), 4 * SIDES);
        let v = -volume(&t);
        assert!((1.0..1.1).contains(&(v / (std::f64::consts::PI * 0.25 * 4.))), "{v}");
        let mut mirror = mesh::IDENTITY;
        mirror[0][0] = -1.;
        let m = triangles(&cylinder, &mirror);
        assert!((volume(&m) + v).abs() < 1e-4, "{} {v}", volume(&m));
    }

    #[test]
    fn an_upright_trunk_is_a_hexagonal_prism_open_below() {
        let trunk = shape(Hull::Cylinder { height: 5., radius: 0.25 });
        assert_eq!(triangles(&trunk, &mesh::IDENTITY).len(), 3 * THIN_SIDES);
    }

    #[test]
    fn a_cone_has_no_degenerate_faces_and_the_matrix_places_it() {
        let cone = shape(Hull::Cone { height: 2., radius: 0.5 });
        assert_eq!(triangles(&cone, &mesh::IDENTITY).len(), SIDES);
        let mut at = mesh::IDENTITY;
        at[3] = [10., 1., -5., 1.];
        let top = triangles(&cone, &at).iter().flat_map(|t| t.pos).fold(f64::MIN, |m, p| m.max(p[2]));
        // DR's y is the world's z: apex at 1 + 1.
        assert!((top - 2.).abs() < 1e-5, "{top}");
    }

    #[test]
    fn what_the_data_keeps_solid() {
        let mut s = shape(Hull::Sphere { radius: 1. });
        assert!(s.solid());
        s.impulse = Some("bArmco".into());
        assert!(s.solid());
        s.impulse = Some("bPhantom".into());
        assert!(!s.solid());
        s.impulse = None;
        s.material = "sSoftVegetation".into();
        assert!(!s.solid());
    }

    /// The hulls stand where the props are drawn (the axes, the centring and the transform all read right): the middle
    /// of a solid hull lies within a metre of its prop's own mesh bounds, for the sample routes of `RBR_DIRT_RALLY`.
    #[test]
    fn solid_hulls_stand_on_their_drawn_props() {
        for route in crate::objects::tests::sample_routes() {
            let layout = crate::objects::read(&route).unwrap();
            let mut files = std::collections::HashMap::new();
            let (mut checked, mut off) = (0, Vec::new());
            for entity in &layout.entities {
                let mut bounds = Vec::<([f32; 3], [f32; 3])>::new();
                for (file, node) in &entity.parts {
                    let types = files.entry(file.clone()).or_insert_with(|| {
                        let dir =
                            if file.starts_with("route_") { route.dir.clone() } else { route.area_dir().to_owned() };
                        mesh::types(&crate::pssg::Pssg::read(&dir.join(file)).unwrap()).unwrap()
                    });
                    let Some(&at) = types.index.get(node) else { continue };
                    for m in &types.meshes[at] {
                        let fold = |f: fn(f32, f32) -> f32, init: f32| {
                            std::array::from_fn(|k| m.positions.iter().fold(init, |a, p| f(a, p[k])))
                        };
                        if !m.positions.is_empty() {
                            bounds.push((fold(f32::min, f32::MAX), fold(f32::max, f32::MIN)));
                        }
                    }
                }
                let Some(lo) = bounds.iter().map(|b| b.0).reduce(|a, b| std::array::from_fn(|k| a[k].min(b[k]))) else {
                    continue;
                };
                let hi = bounds.iter().map(|b| b.1).fold([f32::MIN; 3], |a, b| std::array::from_fn(|k| a[k].max(b[k])));
                for shape in entity.shapes.iter().filter(|s| s.solid()) {
                    let (v, _) = hull(&shape.hull, true);
                    let c = mesh::point(&shape.matrix, [0.; 3]);
                    checked += 1;
                    if (0..3).any(|k| c[k] < lo[k] - 1. || c[k] > hi[k] + 1.) {
                        off.push(format!(
                            "{} {:?} at {c:?} not in {lo:?}..{hi:?} ({} corners)",
                            entity.name,
                            shape.hull,
                            v.len()
                        ));
                    }
                }
            }
            eprintln!("{}: {checked} solid hulls checked, {} off: {off:?}", route.id(), off.len());
            assert!(checked > 50 && off.len() * 50 <= checked, "{off:?}");
        }
    }
}
