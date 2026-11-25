//! The light the cabin's faces bounce onto each other, per vertex (experiment, ours: `RERBR_CABIN_BOUNCE=1`). In place
//! of the closed room of one mean albedo (`SunIn::bounce`), which gave every face the same bounce, each vertex gathers
//! what its own rays meet ([`super::Grid::sky_hits`]): the faces' light times their albedo, over [`BOUNCES`] bounces.
//! Two terms, per unit of the light that comes in through the glass: the sky's (each face lit by the share of the sky
//! it sees, as the window light lights it) and the sun's, which depends on where the sun is, as a linear function of
//! its direction in the car's model space ([`Linear`], fitted per face over [`SUN_DIRECTIONS`] directions past the
//! car's opaque faces). The sun's bounce is grey, coloured by the sky bounce's hue (the same faces give both back).
//! `cabin.wgsl` reads them (`CABIN_LOCAL_BOUNCE`) in place of the room's bounce.

use super::cabin::{inside, opaque, Albedos, Grid, LIFT, RAYS};
use crate::parallel::in_parallel;
use crate::stage::{Mesh, StageTexture, TextureRef, Vertex};
use glam::{Mat4, Vec3};
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Whether the cabin's bounce is baked per vertex (`RERBR_CABIN_BOUNCE` set and not `0`).
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RERBR_CABIN_BOUNCE").is_some_and(|v| v != "0"))
}

/// How many times the light goes round the cabin.
const BOUNCES: usize = 3;
/// The sun's directions a face's sun is fitted over.
const SUN_DIRECTIONS: usize = 64;

/// `c0 + c1 . d` for a direction `d`: a light's dependence on where the sun is, fitted over the sphere (the mean and three
/// times the mean of the light times the direction, spherical harmonics' first two bands).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Linear {
    pub c0: f32,
    pub c1: Vec3,
}

impl Linear {
    fn scaled(self, k: f32) -> Linear {
        Linear { c0: self.c0 * k, c1: self.c1 * k }
    }

    fn plus(self, other: Linear) -> Linear {
        Linear { c0: self.c0 + other.c0, c1: self.c1 + other.c1 }
    }

    /// Its value toward `d`, at least 0.
    pub fn at(self, d: Vec3) -> f32 {
        (self.c0 + self.c1.dot(d)).max(0.0)
    }
}

/// An interior face: its albedo, the sky share it sees, its sun past the car, its vertices (indices over all the
/// interior's vertices in order) and its area as `SunIn` weighs it (by how far it is inside the cabin).
struct Face {
    albedo: Vec3,
    sky: f32,
    sun: Linear,
    vertices: [usize; 3],
    weight: f32,
}

/// The normals of `interior`'s opaque faces in [`Grid`]'s order, by which a ray's hit counts only on a face's front.
pub(super) fn normals(interior: &[Mesh]) -> Vec<Vec3> {
    let normal = |mesh: &Mesh, t: &[u32]| {
        t.iter().map(|&i| Vec3::from(mesh.vertices[i as usize].normal)).sum::<Vec3>().normalize_or_zero()
    };
    interior
        .iter()
        .flat_map(|mesh| {
            mesh.draws
                .iter()
                .filter(|draw| opaque(draw))
                .flat_map(move |draw| mesh.indices[draw.indices.clone()].chunks_exact(3).map(move |t| normal(mesh, t)))
        })
        .collect()
}

/// The bounce each vertex of `interior` takes (all its meshes' vertices in order): the sky's per channel and the sun's,
/// from `skies` (each vertex's sky share) and `hits` (the faces each one's rays meet, in [`Grid`]'s order: the interior's
/// opaque faces first, then the exterior shell's, which give nothing back); and the faces' mean sun as fitted, weighed
/// as `SunIn` weighs them.
pub(super) fn bake(
    grid: &Grid,
    interior: &[Mesh],
    skies: &[(f32, Vec3)],
    hits: &[Vec<u32>],
    textures: &BTreeMap<TextureRef, StageTexture>,
) -> (Vec<(Vec3, Linear)>, Linear) {
    let mut albedos = Albedos::new(textures);
    let mut faces: Vec<(Face, Vec3, Vec3)> = Vec::new();
    let mut first = 0;
    for mesh in interior {
        for draw in mesh.draws.iter().filter(|draw| opaque(draw)) {
            for t in mesh.indices[draw.indices.clone()].chunks_exact(3) {
                let t = [t[0], t[1], t[2]];
                let v = t.map(|i| &mesh.vertices[i as usize]);
                let [a, b, c] = v.map(|v| Vec3::from(v.position));
                let normal = v.iter().map(|v| Vec3::from(v.normal)).sum::<Vec3>().normalize_or_zero();
                let vertices = t.map(|i| first + i as usize);
                let sky = vertices.iter().map(|&i| skies[i].0).sum::<f32>() / 3.0;
                let albedo = albedos.of(draw, v);
                let weight = inside(sky) * 0.5 * (b - a).cross(c - a).length();
                let face = Face { albedo, sky, sun: Linear::default(), vertices, weight };
                faces.push((face, (a + b + c) / 3.0, normal));
            }
        }
        first += mesh.vertices.len();
    }
    let directions: Vec<Vec3> = (0..SUN_DIRECTIONS)
        .map(|k| {
            let y = 1.0 - 2.0 * (k as f32 + 0.5) / SUN_DIRECTIONS as f32;
            let (r, angle) = ((1.0 - y * y).sqrt(), k as f32 * 2.399_963);
            Vec3::new(r * angle.cos(), y, r * angle.sin())
        })
        .collect();
    let suns = in_parallel(&faces, |(_, middle, normal)| {
        let sum = directions
            .iter()
            .filter(|&&d| normal.dot(d) > 0.0 && grid.open(*middle + *normal * LIFT, d))
            .fold(Linear::default(), |sum, &d| sum.plus(Linear { c0: 1.0, c1: 3.0 * d }.scaled(normal.dot(d))));
        sum.scaled(1.0 / SUN_DIRECTIONS as f32)
    });
    let faces: Vec<Face> = faces.into_iter().zip(suns).map(|((face, ..), sun)| Face { sun, ..face }).collect();
    let area: f32 = faces.iter().map(|f| f.weight).sum();
    let fitted = faces
        .iter()
        .fold(Linear::default(), |sum, f| sum.plus(f.sun.scaled(f.weight)))
        .scaled(if area > 0.0 { 1.0 / area } else { 0.0 });
    (gather(&faces, hits), fitted)
}

/// The light round the cabin, [`BOUNCES`] times: each face gives back its albedo times what it takes in (its own sky
/// and sun and the bounce at its vertices), each vertex takes in the mean over its [`RAYS`] rays of what the faces they
/// meet give back.
fn gather(faces: &[Face], hits: &[Vec<u32>]) -> Vec<(Vec3, Linear)> {
    let grey = |albedo: Vec3| albedo.dot(Vec3::new(0.2126, 0.7152, 0.0722));
    let mut given: Vec<(Vec3, Linear)> =
        faces.iter().map(|f| (f.albedo * f.sky, f.sun.scaled(grey(f.albedo)))).collect();
    let taken = |given: &[(Vec3, Linear)]| -> Vec<(Vec3, Linear)> {
        hits.iter()
            .map(|hits| {
                let (sky, sun) = hits
                    .iter()
                    .filter_map(|&h| given.get(h as usize))
                    .fold((Vec3::ZERO, Linear::default()), |(sky, sun), &(face_sky, face_sun)| {
                        (sky + face_sky, sun.plus(face_sun))
                    });
                (sky / RAYS as f32, sun.scaled(1.0 / RAYS as f32))
            })
            .collect()
    };
    for _ in 1..BOUNCES {
        let at = taken(&given);
        given = faces
            .iter()
            .map(|f| {
                let (sky, sun) = f.vertices.iter().fold((Vec3::ZERO, Linear::default()), |(sky, sun), &i| {
                    (sky + at[i].0 / 3.0, sun.plus(at[i].1.scaled(1.0 / 3.0)))
                });
                (f.albedo * (f.sky + sky), f.sun.plus(sun).scaled(grey(f.albedo)))
            })
            .collect();
    }
    taken(&given)
}

/// The most of the sky's bounce a vertex holds (`bounce[1]`, three 10-bit square roots).
const SKY_MOST: f32 = 1.0;
/// The sun bounce's direction term against its mean at most (`bounce[0]`, three 10-bit shares of -it to it).
const SUN_SLOPE: f32 = 2.0;

fn ten_bits(x: f32) -> u32 {
    (x.clamp(0.0, 1.0) * 1023.0).round() as u32
}

/// `vertex` with its bounce: the sky's in `bounce[1]`, the sun's mean in `sky_dir` (a 16-bit square root) and its
/// direction term against the mean in `bounce[0]`, as `shader.wgsl`'s `cabin_bounce` reads them.
pub(super) fn packed(vertex: Vertex, (sky, sun): (Vec3, Linear)) -> Vertex {
    let sky = (sky / SKY_MOST).max(Vec3::ZERO).powf(0.5);
    let c0 = sun.c0.max(0.0);
    let slope = if c0 > 0.0 { sun.c1 / c0 } else { Vec3::ZERO };
    Vertex {
        bounce: [slope_bits(slope), ten_bits(sky.x) | ten_bits(sky.y) << 10 | ten_bits(sky.z) << 20],
        sky_dir: (c0.min(1.0).sqrt() * 65535.0).round() as u32,
        ..vertex
    }
}

fn slope_bits(slope: Vec3) -> u32 {
    let bits = |x: f32| ten_bits(0.5 + 0.5 * x / SUN_SLOPE);
    bits(slope.x) | bits(slope.y) << 10 | bits(slope.z) << 20
}

fn slope_of(bits: u32) -> Vec3 {
    let share = |k: u32| ((bits >> k) & 1023) as f32 / 1023.0;
    Vec3::new(share(0), share(10), share(20)).map(|s| (2.0 * s - 1.0) * SUN_SLOPE)
}

/// `vertex`'s sun bounce turned by `matrix` with the part it is on (`cabin::turned`).
pub(super) fn turned(vertex: Vertex, matrix: Mat4) -> Vertex {
    let slope = matrix.transform_vector3(slope_of(vertex.bounce[0]));
    Vertex { bounce: [slope_bits(slope), vertex.bounce[1]], ..vertex }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytemuck::Zeroable;

    #[test]
    fn a_fitted_sun_takes_the_cosine_toward_the_sun() {
        // A face open to the whole sky: its mean n.l over the sphere is 1/4, the direction term half the normal.
        let n = Vec3::Y;
        let steps = 4000;
        let fit = (0..steps)
            .map(|k| {
                let y = 1.0 - 2.0 * (k as f32 + 0.5) / steps as f32;
                let (r, a) = ((1.0 - y * y).sqrt(), k as f32 * 2.399_963);
                Vec3::new(r * a.cos(), y, r * a.sin())
            })
            .filter(|d| n.dot(*d) > 0.0)
            .fold(Linear::default(), |sum, d| sum.plus(Linear { c0: 1.0, c1: 3.0 * d }.scaled(n.dot(d))))
            .scaled(1.0 / steps as f32);
        assert!((fit.c0 - 0.25).abs() < 1e-3 && (fit.c1 - 0.5 * n).length() < 1e-2, "{fit:?}");
        assert!(fit.at(Vec3::Y) > fit.at(Vec3::X) && fit.at(Vec3::NEG_Y) == 0.0);
    }

    #[test]
    fn two_faces_light_each_other_and_a_ray_out_takes_nothing() {
        let face = |sky: f32, vertices: [usize; 3]| Face {
            albedo: Vec3::splat(0.5),
            sky,
            sun: Linear { c0: 0.2, c1: Vec3::Y * 0.1 },
            vertices,
            weight: 1.0,
        };
        let faces = [face(1.0, [0, 0, 0]), face(0.0, [1, 1, 1])];
        // Vertex 0's rays meet face 1 on half of them; vertex 1's meet face 0 on all.
        let hits = vec![vec![1; RAYS / 2], vec![0; RAYS]];
        let at = gather(&faces, &hits);
        // Face 0 gives 0.5 of its full sky and its bounce; vertex 1 takes it from all its rays.
        assert!(at[1].0.x > 0.5 && at[1].0.x < 0.6, "{:?}", at[1]);
        // Face 1 sees no sky: it gives back only what vertex 1 takes, half of that reaches vertex 0.
        assert!(at[0].0.x > 0.0 && at[0].0.x < 0.5 * at[1].0.x, "{:?}", at[0]);
        assert!(at[1].1.c0 > 0.1 && at[1].1.c1.y > 0.0, "{:?}", at[1]);
        // A ray past the faces (the exterior shell) gives nothing.
        assert_eq!(gather(&faces, &[vec![7; RAYS], vec![7; RAYS]])[0], (Vec3::ZERO, Linear::default()));
    }

    #[test]
    fn the_packed_bounce_reads_back_and_turns() {
        let v =
            packed(Vertex::zeroed(), (Vec3::new(0.04, 0.09, 0.16), Linear { c0: 0.25, c1: Vec3::new(0.0, 0.25, 0.0) }));
        let sky = Vec3::new(0.2, 0.3, 0.4) * 1023.0;
        assert_eq!(v.bounce[1], sky.x.round() as u32 | (sky.y.round() as u32) << 10 | (sky.z.round() as u32) << 20);
        assert_eq!(v.sky_dir, 32768);
        assert!((slope_of(v.bounce[0]) - Vec3::Y).length() < 0.01);
        let turn = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2);
        assert!((slope_of(turned(v, turn).bounce[0]) - Vec3::NEG_X).length() < 0.01);
    }
}
