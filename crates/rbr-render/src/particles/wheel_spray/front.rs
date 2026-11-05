//! Ours: what the front wheels throw where the in-car views see it, and the air the car drags along.
//!
//! - A front tyre that slides sideways (steered or the car sliding) scrubs its stones and dust out the way it slides:
//!   [`slide`] gives that direction and speed from the patch's velocity across the tyre.
//! - The air about the car keeps the speed the car had a moment ago ([`wake`]): when the car brakes, the dust it throws
//!   keeps that speed and rolls past it ([`carried`]).

use glam::Vec3;

/// The seconds the air about the car takes to follow the car's speed (ours: a braking car's dust overtakes it as it
/// stops, as on a dusty road).
pub const WAKE_LAG: f32 = 0.8;

/// The air about the car after `dt` seconds more of the car at `velocity`: it follows the car with [`WAKE_LAG`].
/// `None` (no frame yet) starts it at the car's.
pub fn wake(air: Option<Vec3>, velocity: Vec3, dt: f32) -> Vec3 {
    air.map_or(velocity, |air| air.lerp(velocity, 1.0 - (-dt / WAKE_LAG).exp()))
}

/// The velocity the car's spray is carried along with: the car's, plus the air's lead along `forward` where the car
/// slows faster than its air (never less than the car's own: an accelerating car leaves its spray as before).
pub fn carried(velocity: Vec3, air: Vec3, forward: Vec3) -> Vec3 {
    velocity + forward * (air - velocity).dot(forward).max(0.0)
}

/// The patch's sideways slide: its `velocity` along the axle (`edges`, across the tread), flattened, as a direction and
/// a speed. `None` when the tyre does not slide sideways.
pub fn slide(edges: [Vec3; 2], velocity: Vec3, up: Vec3) -> Option<(Vec3, f32)> {
    let across = (edges[1] - edges[0]).reject_from(up).try_normalize()?;
    let speed = velocity.dot(across);
    (speed.abs() > 1e-3).then(|| (across * speed.signum(), speed.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_air_lags_the_car_and_carries_a_braking_cars_spray_ahead() {
        let forward = -Vec3::Y;
        let mut air = wake(None, forward * 22.0, 1.0 / 60.0);
        assert_eq!(air, forward * 22.0, "starts at the car's");
        // Braking at 9 m/s² to a stop.
        let mut speed = 22.0;
        while speed > 0.0 {
            speed = (speed - 9.0 / 60.0_f32).max(0.0);
            air = wake(Some(air), forward * speed, 1.0 / 60.0);
        }
        let ahead = carried(Vec3::ZERO, air, forward).dot(forward);
        assert!(ahead > 4.0 && ahead < 15.0, "a stopped car's dust still moves on: {ahead}");
        // Steady or speeding up, the spray goes with the car's own velocity, exactly.
        let v = forward * 20.0 + Vec3::X * 3.0;
        assert_eq!(carried(v, forward * 18.0, forward), v);
        assert_eq!(carried(v, v, forward), v);
    }

    #[test]
    fn a_tyre_slides_the_way_its_patch_moves_across_it() {
        let edges = [Vec3::new(-0.1, 0.0, 0.0), Vec3::new(0.1, 0.0, 0.0)];
        // Rolling straight along -y: no slide.
        assert!(slide(edges, Vec3::new(0.0, -20.0, 0.0), Vec3::Z).is_none());
        // Sliding toward -x at 4 m/s while rolling.
        let (dir, speed) = slide(edges, Vec3::new(-4.0, -20.0, 0.5), Vec3::Z).unwrap();
        assert!((dir - Vec3::NEG_X).length() < 1e-6 && (speed - 4.0).abs() < 1e-5, "{dir} {speed}");
        // Edges given either way round give the same.
        let (back, _) = slide([edges[1], edges[0]], Vec3::new(-4.0, -20.0, 0.0), Vec3::Z).unwrap();
        assert!((back - dir).length() < 1e-6);
        assert!(slide([Vec3::ZERO; 2], Vec3::X, Vec3::Z).is_none(), "no axle, no slide");
    }
}
