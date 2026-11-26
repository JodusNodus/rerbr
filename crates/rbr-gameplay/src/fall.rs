//! Ours: a car that fell off the world gets the call for help's assist by itself. The original has no such test: a
//! car through a hole in a stage's ground, or past its edge, falls for ever, and the pause menu's call for help cannot
//! save it (its motion probe answers MSG_CARSTOP above 1.5 m/s).
//!
//! The car has fallen when it is [`DEPTH`] below the road at its drive line distance and still falling faster than
//! [`SPEED`]. A jump never lands that far below the road, and a car that rolls down a bank is not falling that fast.
//! Physics is z-up (the drive line's frame, the car's position at +0x140).

/// How far below the road the car must be, m.
pub const DEPTH: f32 = 50.0;
/// How fast the car must be falling, m/s (about 2.5 s of free fall).
pub const SPEED: f32 = 25.0;

/// Whether the car at `position`, moving at `velocity`, has fallen off the world below `road` (the drive line's point
/// at the car's distance).
pub fn fallen(position: [f32; 3], velocity: [f32; 3], road: [f32; 3]) -> bool {
    position[2] < road[2] - DEPTH && velocity[2] < -SPEED
}

/// The rescue's latch: one assist for each fall, so the steps before the assist puts the car back (its 0.5 s respawn
/// countdown) do not call it again.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FallRescue {
    falling: bool,
}

impl FallRescue {
    /// The latch after a step whose car is `fallen`, and whether the assist is due now.
    pub fn step(self, fallen: bool) -> (Self, bool) {
        (Self { falling: fallen }, fallen && !self.falling)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROAD: [f32; 3] = [10.0, 20.0, 300.0];

    #[test]
    fn a_car_far_below_the_road_and_falling_fast_has_fallen() {
        assert!(fallen([0.0, 0.0, 240.0], [0.0, 0.0, -30.0], ROAD));
    }

    /// A long jump comes down fast, but above the road or just below it.
    #[test]
    fn a_jump_has_not_fallen() {
        assert!(!fallen([0.0, 0.0, 305.0], [30.0, 0.0, -20.0], ROAD));
        assert!(!fallen([0.0, 0.0, 290.0], [30.0, 0.0, -28.0], ROAD));
    }

    /// Far below the road but at rest or sliding down a bank: the car is on ground, the call for help is the player's.
    #[test]
    fn a_car_on_ground_below_the_road_has_not_fallen() {
        assert!(!fallen([0.0, 0.0, 200.0], [0.0, 0.0, 0.0], ROAD));
        assert!(!fallen([0.0, 0.0, 200.0], [15.0, 0.0, -10.0], ROAD));
    }

    #[test]
    fn one_assist_for_each_fall() {
        let steps = [false, true, true, true, false, true];
        let due: Vec<bool> = steps
            .iter()
            .scan(FallRescue::default(), |rescue, &fallen| {
                let (next, due) = rescue.step(fallen);
                *rescue = next;
                Some(due)
            })
            .collect();
        assert_eq!(due, [false, true, false, false, false, true]);
    }
}
