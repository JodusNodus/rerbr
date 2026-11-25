//! How much sky each vertex of the interior set sees (renderer addition, ours: [`crate::renderer::Enhance::cabin`]).
//! The game lights the whole cabin with one ambient; here each interior vertex gets the share of its hemisphere that
//! reaches out of the car past its opaque faces, both sets (`i_*` and the exterior shell), the glass counting as open.
//! The cabin's ambient follows it (`cabin.wgsl`): footwells, the dash under its top and the seat folds go darker, faces
//! by the windows lighter. Measured once at load, at rest, so it costs nothing a frame.
//! Each vertex also keeps which way it sees the sky ([`seen`]): the mean direction of its rays that get out. The cabin
//! takes the light of what lies that way outside (`cabin.wgsl`), so a face turned to a sunlit or a sunset window is lit
//! by it and one that sees out only toward the forest is darker; its sheen reflects only what lies within that cone.
//!
//! The vertices also say which they are, in the whole part of `specular[1]` (the car's draws have no specular map, so
//! the field is free): [`INTERIOR`] for the interior set's opaque faces (the cabin light touches nothing else of the
//! car, so not the glass the outside is seen through), [`GAUGE`] for its dial faces ([`mark_gauges`] (in [`gauges`])) and [`MIRROR`]
//! for the glass of its mirrors.
//!
//! A mirror's glass ([`Mirror`]) is a `*mirrorglass*` part, or in a car without one the faces of its `*mirror*` parts
//! turned to the back of the car ([`FACING`]). The game has no mirror image: it draws that face with its texture, and
//! many RSF models leave it a flat texel of white or paint (the Impreza GC8 555's `i_mirrorglass_*` disc, the Lancer Evo
//! II's `i_mirror_*`). Here it mirrors the surroundings (`cabin_mirror` in `cabin.wgsl`), as the interior's opaque faces
//! of the glass class do (the BMW M3 E30's `i_body_tukor`).
//!
//! The grid and the bake are in [`bake`], the light the faces bounce onto each other in [`bounce`], the dial faces in
//! [`gauges`].

mod bake;
mod bounce;
mod gauges;

pub(crate) use bake::rgba_at_most;
pub use bake::{bake, SunIn};
pub use gauges::{gauge_faces, mark_gauges, Face};

use crate::stage::Vertex;
use glam::{Mat4, Vec3};

/// `specular[1]` of a baked interior vertex.
pub const INTERIOR: f32 = 1.0;
/// `specular[1]` of an interior vertex on a dial face.
pub const GAUGE: f32 = 2.0;
/// `specular[1]` of an interior vertex on a mirror's glass.
pub const MIRROR: f32 = 3.0;
/// A mirror housing's face is its glass when its normal's share along the car's back (`+z`) is above this: the glass
/// turned toward the driver, not the housing's sides and front.
const FACING: f32 = 0.7;

/// A mirror's part, by its node name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirror {
    /// `*mirrorglass*`: all of it is glass.
    Glass,
    /// Any other `*mirror*` part: its glass is its faces turned to the back, unless the car has [`Mirror::Glass`] parts.
    Housing,
}

impl Mirror {
    pub fn of_part(name: &str) -> Option<Mirror> {
        let name = name.to_ascii_lowercase();
        match () {
            _ if name.contains("mirrorglass") => Some(Mirror::Glass),
            _ if name.contains("mirror") => Some(Mirror::Housing),
            _ => None,
        }
    }
}

/// Where a baked interior vertex ([`INTERIOR`] or [`GAUGE`]) sees the sky ([`bake`]), in the car's model space: x and
/// y in `uv2` (the car's draws have no light map), z in the fraction of `specular[1]` ([`SEEN_Z`]).
pub fn seen(vertex: &Vertex) -> Vec3 {
    let z = (vertex.specular[1].fract() - SEEN_Z[0]) / SEEN_Z[1];
    Vec3::new(vertex.uv2[0], vertex.uv2[1], z)
}

/// `vertex`, [`INTERIOR`] or [`GAUGE`], with [`seen`] `dir`.
pub fn with_seen(vertex: Vertex, dir: Vec3) -> Vertex {
    let mut specular = vertex.specular;
    specular[1] = specular[1].floor() + SEEN_Z[0] + SEEN_Z[1] * dir.z.clamp(-1.0, 1.0);
    Vertex { uv2: [dir.x, dir.y], specular, ..vertex }
}

/// `vertex` placed by `matrix` as the playback places the car's parts: where it sees the sky turns with it (the
/// position and the normal are the caller's).
pub fn turned(vertex: &Vertex, matrix: Mat4) -> Vertex {
    if vertex.specular[1] < INTERIOR {
        return *vertex;
    }
    bounce::turned(with_seen(*vertex, matrix.transform_vector3(seen(vertex))), matrix)
}

/// The fraction of `specular[1]` that holds [`seen`]'s z: `SEEN_Z[0] + SEEN_Z[1] * z`, inside (0, 1) so that the whole
/// part stays the vertex's mark (`cabin.wgsl` reads both).
const SEEN_Z: [f32; 2] = [0.5, 0.45];

#[cfg(test)]
mod tests {
    use super::*;
    use bytemuck::Zeroable;

    #[test]
    fn where_a_vertex_sees_the_sky_keeps_its_mark_and_turns_with_the_car() {
        let vertex = Vertex { specular: [0.0, GAUGE, 0.25], ..Vertex::zeroed() };
        let dir = Vec3::new(0.3, -0.2, 0.6);
        let seen_at = with_seen(vertex, dir);
        assert_eq!(seen_at.specular[1].floor(), GAUGE);
        assert_eq!(seen_at.specular[2], 0.25);
        assert!((seen(&seen_at) - dir).length() < 1e-5);
        let turn = Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2) * Mat4::from_translation(Vec3::X);
        assert!((seen(&turned(&seen_at, turn)) - turn.transform_vector3(dir)).length() < 1e-5);
        // Not the interior's: left as it is.
        let glass = Vertex { uv2: [0.5, 0.5], ..Vertex::zeroed() };
        assert_eq!(turned(&glass, turn), glass);
    }

    #[test]
    fn mirror_parts_go_by_their_names() {
        assert_eq!(Mirror::of_part("i_mirrorglass_l"), Some(Mirror::Glass));
        assert_eq!(Mirror::of_part("i_mirror2_r"), Some(Mirror::Housing));
        assert_eq!(Mirror::of_part("i_RoomMirror"), Some(Mirror::Housing));
        assert_eq!(Mirror::of_part("i_body_tukor"), None, "its glass is of the glass class");
    }
}
