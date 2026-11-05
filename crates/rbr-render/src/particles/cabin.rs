//! Ours: the car's shape the in-car view draws no particles in (`cabin_shown` in `particles.wgsl`, which follows
//! [`CabinMask::shown`]). Seen from the seat the spray under and inside the body would show through the interior's gaps
//! (the footwells), but outside the doors, over the bonnet and at the front corners it is what a driver sees.
//!
//! In the car's model frame (x across, y up, front toward -z): the body below the belt line, the full length and width
//! of the car's box, and the room above it from the windscreen's foot back to the roof. Particles in either are hidden;
//! outside they fade in over [`EDGE`].

use glam::Vec3;

/// The belt line (the bonnet's and the doors' top) below the cockpit's eye, m.
const BELT_BELOW_EYE: f32 = 0.25;
/// The windscreen's foot ahead of the cockpit's eye, m (the retail and RSF dashes run 0.6 to 0.9 m ahead of it).
const SCREEN_AHEAD: f32 = 0.8;
/// The body reaches this far under the car's box, m: the spray under the floor stays hidden.
const UNDER: f32 = 0.5;
/// Outside the shape a particle fades in over this many metres.
pub const EDGE: f32 = 0.15;

/// The two boxes, model frame: the body and the room above it, each `[lo, hi]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CabinMask {
    pub body: [Vec3; 2],
    pub room: [Vec3; 2],
}

impl CabinMask {
    /// From the box `lo`..`hi` around the car's exterior and the cockpit's `eye`, model frame.
    pub fn new(lo: Vec3, hi: Vec3, eye: Vec3) -> CabinMask {
        let belt = eye.y - BELT_BELOW_EYE;
        CabinMask {
            body: [Vec3::new(lo.x, lo.y - UNDER, lo.z), Vec3::new(hi.x, belt, hi.z)],
            room: [Vec3::new(lo.x, belt, eye.z - SCREEN_AHEAD), hi],
        }
    }

    /// How far `p` (model frame) lies outside the shape, m (negative inside).
    pub fn distance(&self, p: Vec3) -> f32 {
        box_distance(p, self.body).min(box_distance(p, self.room))
    }

    /// The share of a particle at `p` (model frame) drawn: 0 inside, 1 from [`EDGE`] outside.
    pub fn shown(&self, p: Vec3) -> f32 {
        let t = (self.distance(p) / EDGE).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

/// The signed distance from `p` to the box `[lo, hi]`.
fn box_distance(p: Vec3, [lo, hi]: [Vec3; 2]) -> f32 {
    let q = (p - (lo + hi) * 0.5).abs() - (hi - lo) * 0.5;
    q.max(Vec3::ZERO).length() + q.max_element().min(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A car of 1.8 by 1.4 by 4.0 m on the ground, the eye 1.1 m up and 0.2 m behind the middle, left of it.
    fn car() -> (CabinMask, Vec3) {
        let eye = Vec3::new(0.35, 1.1, 0.2);
        (CabinMask::new(Vec3::new(-0.9, 0.0, -2.0), Vec3::new(0.9, 1.4, 2.0), eye), eye)
    }

    #[test]
    fn spray_inside_the_body_is_hidden_outside_it_shows() {
        let (mask, eye) = car();
        // The footwell and the front wheel's arch, under the dash.
        assert_eq!(mask.shown(eye + Vec3::new(0.0, -0.8, -0.9)), 0.0);
        assert_eq!(mask.shown(Vec3::new(0.7, 0.2, -1.3)), 0.0, "the front wheel's arch");
        assert_eq!(mask.shown(Vec3::new(0.0, -0.3, 0.0)), 0.0, "under the floor");
        assert_eq!(mask.shown(eye + Vec3::new(-0.5, 0.1, 0.4)), 0.0, "in the cabin beside the eye");
        // Outside the doors, over the bonnet, off the front corners.
        assert_eq!(mask.shown(Vec3::new(1.2, 0.6, 0.0)), 1.0, "outside the door");
        assert_eq!(mask.shown(Vec3::new(0.0, 1.1, -1.5)), 1.0, "over the bonnet");
        assert_eq!(mask.shown(Vec3::new(-1.1, 0.3, -2.2)), 1.0, "off the front corner");
        // A soft edge, not a cut.
        let edge = mask.shown(Vec3::new(0.9 + EDGE * 0.5, 0.6, 0.0));
        assert!(edge > 0.2 && edge < 0.8, "{edge}");
    }

    #[test]
    fn the_box_distance_is_signed() {
        let b = [Vec3::splat(-1.0), Vec3::splat(1.0)];
        assert_eq!(box_distance(Vec3::ZERO, b), -1.0);
        assert_eq!(box_distance(Vec3::new(3.0, 0.0, 0.0), b), 2.0);
        assert!((box_distance(Vec3::new(2.0, 2.0, 0.0), b) - 2f32.sqrt()).abs() < 1e-6);
    }
}
