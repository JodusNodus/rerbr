//! The car meeting the verge, for the sound (ours, `rbr_sound::verge::Verge`): each wheel's share in bushes
//! from the frame's tyre payloads, and the bushes and bendable trees the body is in from the stage's roadside index
//! (`crate::roadside`, whose posts keep the vegetation the echoes leave out). Read after each frame's step, never
//! written (physics frame, z up).
//!
//! The body is taken as a box about the car's centre ([`HALF_LENGTH`], [`HALF_WIDTH`], [`BODY`]) turned to the car's
//! heading; a placement's upright cylinder, widened by its foliage ([`FOLIAGE`]), touches it where it reaches past the
//! box's outline, by as much (its depth), and the box's height band meets the cylinder's.

use glam::{Vec2, Vec3};
use rbr_recording::particles::TyreParticles;
use rbr_recording::pose::CarPose;
use rbr_sound::verge::{Touch, Verge, TOUCHES};

use crate::roadside::{Growth, Roadside};

/// The body's half length and half width, m (a rally car's 4 m by 1.8 m), and its band about the car's centre, m below
/// and above: from the floor to the roof.
const HALF_LENGTH: f32 = 2.0;
const HALF_WIDTH: f32 = 0.9;
const BODY: [f32; 2] = [-0.35, 0.9];
/// How far the leaves reach past a placement's collision mesh, m: the `.trk`'s bush and bendable tree meshes are their
/// cores and trunks (0.1 to 0.4 m across on the retail stages), the foliage the body brushes is wider.
const FOLIAGE: f32 = 0.5;

/// Each wheel's share in the bushes and bendable trees a stage marks in the ground itself (the surface under the
/// wheel, not a `.trk` placement), times their density ([`crate::roadside::growth`]).
pub fn brush(payloads: &[TyreParticles]) -> [f32; 4] {
    shares(payloads, |id| match u8::try_from(id).ok().and_then(crate::roadside::growth) {
        Some(Growth::Bush(density)) => density,
        Some(Growth::Branches) => 1.0,
        None => 0.0,
    })
}

/// Each wheel's blend of `share` over its two surfaces in the frame's `payloads` (the last of a wheel's).
fn shares(payloads: &[TyreParticles], share: impl Fn(u32) -> f32) -> [f32; 4] {
    let mut out = [0.0; 4];
    for p in payloads.iter().filter(|p| p.wheel < 4) {
        let blend = p.blend.clamp(0.0, 1.0);
        out[p.wheel as usize] =
            if p.contact { (1.0 - blend) * share(p.surfaces[0]) + blend * share(p.surfaces[1]) } else { 0.0 };
    }
    out
}

/// How far a post of `radius` at `at` (in the car's frame: ahead, right) reaches into the body's outline, m; 0 or less
/// when it does not touch.
fn depth(at: Vec2, radius: f32) -> f32 {
    let out = Vec2::new(at.x.abs() - HALF_LENGTH, at.y.abs() - HALF_WIDTH);
    let gap = if out.x > 0.0 || out.y > 0.0 { out.max(Vec2::ZERO).length() } else { out.x.max(out.y) };
    radius - gap
}

/// The vegetation of `roadside` the body of the car at `position` facing `forward` is in, the deepest
/// [`TOUCHES`].
pub fn touches(roadside: &Roadside, position: Vec3, forward: Vec3) -> [Option<Touch>; TOUCHES] {
    let mut out = [None; TOUCHES];
    let ahead = forward.truncate().normalize_or_zero();
    if ahead == Vec2::ZERO {
        return out;
    }
    let right = Vec2::new(ahead.y, -ahead.x);
    let p = position.truncate();
    let band = [position.z + BODY[0], position.z + BODY[1]];
    let reach = Vec2::new(HALF_LENGTH, HALF_WIDTH).length() + FOLIAGE;
    let mut found: Vec<Touch> = Vec::new();
    for (id, post) in roadside.near(p, reach) {
        let Some(growth) = post.growth else { continue };
        if post.top < band[0] || post.bottom > band[1] {
            continue;
        }
        let to = post.at - p;
        let depth = depth(Vec2::new(to.dot(ahead), to.dot(right)), post.radius + FOLIAGE);
        if depth <= 0.0 {
            continue;
        }
        let (density, branches) = match growth {
            Growth::Bush(density) => (density, false),
            Growth::Branches => (1.0, true),
        };
        found.push(Touch { id, depth, density, branches });
    }
    found.sort_by(|a, b| b.depth.total_cmp(&a.depth));
    for (slot, touch) in out.iter_mut().zip(found) {
        *slot = Some(touch);
    }
    out
}

/// The car at the verge: each wheel's share in bushes (`brush`, [`brush`]) and the vegetation of `roadside` its body
/// is in, for the car at `car`.
pub fn verge(brush: [f32; 4], roadside: Option<&Roadside>, car: &CarPose) -> Verge {
    let touches = roadside.map_or([None; TOUCHES], |r| touches(r, Vec3::from(car.position), Vec3::from(car.forward())));
    Verge { brush, touches }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roadside::Post;

    fn post(x: f32, y: f32, radius: f32, growth: Option<Growth>) -> Post {
        Post { at: Vec2::new(x, y), radius, bottom: -1.0, top: 3.0, hardness: 0.0, growth }
    }

    /// The car at the origin facing +y: its right is +x.
    fn at(roadside: &Roadside) -> Vec<Touch> {
        touches(roadside, Vec3::ZERO, Vec3::Y).into_iter().flatten().collect()
    }

    #[test]
    fn the_body_in_a_bush_or_a_tree_touches_and_open_road_does_not() {
        // A bush whose foliage is 0.5 m into the right side, a bendable tree's branches at the front left corner, a bush clear of the
        // left side, a tree trunk (it reflects, it is no vegetation) against the body, and a bush over the roof.
        let bush = post(1.2, 0.0, 0.3, Some(Growth::Bush(0.8)));
        let tree = post(-1.0, 2.2, 0.1, Some(Growth::Branches));
        let clear = post(-2.0, 0.0, 0.3, Some(Growth::Bush(1.0)));
        let trunk = Post { hardness: 0.5, ..post(1.0, 1.0, 0.3, None) };
        let high = Post { bottom: 1.5, ..post(0.0, 0.0, 1.0, Some(Growth::Bush(1.0))) };
        let roadside = Roadside::new(vec![bush, tree, clear, trunk, high], None);
        let found = at(&roadside);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!((found[0].id, found[0].branches, found[0].density), (0, false, 0.8));
        assert!((found[0].depth - 0.5).abs() < 1e-5, "{found:?}");
        assert_eq!((found[1].id, found[1].branches), (1, true));
        assert!(found[1].depth > 0.0 && found[1].depth < 0.4);
        // Open road: nothing.
        assert!(at(&Roadside::new(vec![clear, trunk], None)).is_empty());
        assert!(at(&Roadside::default()).is_empty());
        // Turned round, the bush is on the left and still touches.
        assert_eq!(touches(&roadside, Vec3::ZERO, Vec3::NEG_Y).iter().flatten().count(), 2);
    }

    #[test]
    fn a_wheel_on_a_bush_surface_brushes_by_its_density() {
        let payload = |wheel: u32, surfaces: [u32; 2], blend: f32, contact: bool| TyreParticles {
            wheel,
            surfaces,
            blend,
            contact,
            ..TyreParticles::default()
        };
        // Sparse bush and bendable tree half each; gravel; a wheel in the air; a later payload replaces an earlier.
        let payloads = [
            payload(0, [0xa8, 0xa8], 0.0, true),
            payload(1, [0xa6, 0x79], 0.5, true),
            payload(2, [0x15, 0x15], 0.0, true),
            payload(3, [0xa8, 0xa8], 0.0, false),
            payload(0, [0x15, 0xa7], 0.0, true),
        ];
        assert_eq!(brush(&payloads), [0.0, 0.75, 0.0, 0.0]);
        assert_eq!(brush(&[]), [0.0; 4]);
    }
}
