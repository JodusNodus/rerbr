//! The car drawn at the physics' newest step (live play, ours). An answer comes every 7 ms of game time and a frame
//! every 10 to 20 ms, so drawn at its answers alone the car moves on by up to 7 ms more or less than the screen does
//! from one frame to the next: a judder of up to 19 cm at 100 km/h. The step records say which step the physics is
//! at (every 1/720 s under NGP); the car is drawn there, moved on by its velocity and turned at the rate it turned
//! between the last two answers. Over at most [`MOST_LEAD`] that is off by under a millimetre at 1 g.

use glam::{Quat, Vec3};
use rbr_recording::pose::CarPose;

/// The longest lead, s: past it (the answers stalled) the car waits at the lead's end.
const MOST_LEAD: f32 = 0.015;
/// A faster turn between two answers, rad/s, is a jump (the car put back on the road), and the car is not turned on.
const MOST_TURN_RATE: f32 = 10.0;

/// The car `now` (its step and pose) `steps` steps of `step` s on, turned at the rate since `before` (the answer before
/// it, if any).
pub(super) fn lead(before: Option<(u32, CarPose)>, now: (u32, CarPose), steps: u32, step: f32) -> CarPose {
    let time = (steps as f32 * step).min(MOST_LEAD);
    let car = now.1;
    let orientation = Quat::from_array(car.orientation).normalize();
    let turn = before.and_then(|(before, was)| {
        let between = now.0.checked_sub(before).filter(|&n| n > 0)? as f32 * step;
        let (axis, angle) = (orientation * Quat::from_array(was.orientation).normalize().inverse()).to_axis_angle();
        // The short way round.
        let angle = if angle > std::f32::consts::PI { angle - std::f32::consts::TAU } else { angle };
        (angle.abs() <= MOST_TURN_RATE * between).then(|| Quat::from_axis_angle(axis, angle * time / between))
    });
    CarPose {
        position: (Vec3::from(car.position) + Vec3::from(car.velocity) * time).to_array(),
        orientation: (turn.unwrap_or(Quat::IDENTITY) * orientation).normalize().to_array(),
        ..car
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: f32 = 1.0 / 720.0;

    /// The car at `step` going 30 m/s along x and turning 2 rad/s about z.
    fn at(step: u32) -> (u32, CarPose) {
        let time = step as f32 * STEP;
        let car = CarPose {
            position: [30.0 * time, 0.0, 0.0],
            orientation: Quat::from_rotation_z(2.0 * time).to_array(),
            velocity: [30.0, 0.0, 0.0],
        };
        (step, car)
    }

    #[test]
    fn the_car_moves_and_turns_on_to_the_newest_step() {
        let led = lead(Some(at(5)), at(10), 3, STEP);
        let want = at(13).1;
        assert!(Vec3::from(led.position).distance(Vec3::from(want.position)) < 1e-5, "{led:?}");
        assert!(Quat::from_array(led.orientation).angle_between(Quat::from_array(want.orientation)) < 1e-5);
    }

    #[test]
    fn a_jump_is_not_turned_on_and_a_stall_waits() {
        let mut jumped = at(5);
        jumped.1.orientation = Quat::from_rotation_x(1.5).to_array();
        let led = lead(Some(jumped), at(10), 3, STEP);
        assert!(Quat::from_array(led.orientation).angle_between(Quat::from_array(at(10).1.orientation)) < 1e-6);
        let stalled = lead(None, at(10), 100, STEP);
        assert!((stalled.position[0] - at(10).1.position[0] - 30.0 * MOST_LEAD).abs() < 1e-5);
    }
}
