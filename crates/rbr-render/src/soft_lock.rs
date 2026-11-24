//! The soft lock (ours): full lock before the wheel's own end when the wheel turns further than the drive's rotation.
//! Without [`crate::ffb::Options::wheel_rotation`], `crate::wheel_rotation` sets the drive's rotation on the wheel
//! where its Linux driver takes it, and the wheel stops there. With it (the player's wheel software sets the rotation:
//! Windows, macOS, a driver without `range`; or by choice, set on the wheel where the driver takes it) the steering
//! axis is scaled so full lock comes at the drive's rotation, and past it the force-feedback thread pushes back
//! ([`stop`]). A drive's
//! rotation above the wheel's takes the wheel's: full lock at its end.

/// The wheel turns [`SoftLock::wheel`] degrees lock to lock, full lock comes at [`SoftLock::lock`] (less).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoftLock {
    pub wheel: u16,
    pub lock: u16,
}

impl SoftLock {
    /// The soft lock for a drive at `rotation` (`crate::ffb::Options::rotation_for`) on a wheel that turns `wheel`
    /// degrees; `None` when the wheel's end is full lock anyway.
    pub fn new(rotation: Option<u16>, wheel: Option<u16>) -> Option<SoftLock> {
        let (rotation, wheel) = (rotation?, wheel?);
        (rotation < wheel).then_some(SoftLock { wheel, lock: rotation })
    }

    /// The axis (-1..1) over the wheel's rotation to the steering over the lock's, unclamped: past -1 and 1 the wheel
    /// is past full lock.
    pub fn steering(&self, axis: f32) -> f32 {
        axis * f32::from(self.wheel) / f32::from(self.lock)
    }

    /// How far past full lock the wheel is at `steering` ([`SoftLock::steering`]), degrees, signed as it (0 inside).
    pub fn past(&self, steering: f32) -> f32 {
        steering.signum() * (steering.abs() - 1.0).max(0.0) * f32::from(self.lock) / 2.0
    }
}

/// Degrees past full lock where the stop reaches the wheel's full force.
pub const RAMP: f32 = 4.0;
/// The stop's damping: the full force at this speed into the stop, degrees per second; out of it, as much less.
pub const DAMPING: f32 = 400.0;

/// The stop's force, -1..1 of the wheel's full force, signed as the game's magnitude that turns the wheel back to the
/// centre (the same sign as the steering: `rbr_gameplay::ffb::magnitude` of a car's centring force), `past` degrees
/// past full lock moving at `rate` degrees per second. It never pulls the wheel further out.
pub fn stop(past: f32, rate: f32) -> f32 {
    if past == 0.0 {
        return 0.0;
    }
    let push = past.abs() / RAMP + rate * past.signum() / DAMPING;
    past.signum() * push.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lock_only_when_the_wheel_turns_further() {
        // A car of 540 (or a limit of 540) on a wheel set to 900 in its software.
        assert_eq!(SoftLock::new(Some(540), Some(900)), Some(SoftLock { wheel: 900, lock: 540 }));
        // A car of 1050 on a 900 wheel: full lock at the wheel's end.
        assert_eq!(SoftLock::new(Some(1050), Some(900)), None);
        assert_eq!(SoftLock::new(Some(900), Some(900)), None);
        // The wheel's rotation unknown (the driver's), or no rotation asked for.
        assert_eq!(SoftLock::new(Some(540), None), None);
        assert_eq!(SoftLock::new(None, Some(900)), None);
    }

    #[test]
    fn full_lock_comes_at_the_lock_and_the_rest_is_past_it() {
        let lock = SoftLock { wheel: 900, lock: 540 };
        // 270 degrees right of the 450 the axis spans each way: full lock.
        assert!((lock.steering(0.6) - 1.0).abs() < 1e-6);
        assert_eq!(lock.past(lock.steering(0.6)), 0.0);
        assert_eq!(lock.past(0.5), 0.0);
        // The wheel's end, 180 degrees past.
        assert!((lock.past(lock.steering(-1.0)) + 180.0).abs() < 1e-3);
    }

    #[test]
    fn the_stop_pushes_back_harder_further_in_and_never_pulls() {
        assert_eq!(stop(0.0, 500.0), 0.0);
        assert_eq!(stop(RAMP / 2.0, 0.0), 0.5);
        assert_eq!(stop(-RAMP / 2.0, 0.0), -0.5);
        assert_eq!(stop(3.0 * RAMP, 0.0), 1.0);
        // Moving in, it pushes harder; on the way out, softer, and never the other way.
        assert!(stop(1.0, 100.0) > stop(1.0, 0.0));
        assert!(stop(-1.0, -100.0) < stop(-1.0, 0.0));
        assert_eq!(stop(1.0, -DAMPING), 0.0);
    }
}
