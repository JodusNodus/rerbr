//! A stage's reference time for a car class (ours): a speed plan along the driveline, each point's speed limited by
//! the curvature ahead and the class's grip on the surface, with the class's acceleration and braking between them.

use crate::config::Surface;

/// A car class's performance for the speed plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassPace {
    /// Lateral grip on dry tarmac (m/s²); the surface and wetness scale it.
    pub grip: f32,
    /// Acceleration from low speed (m/s²), falling to 0 at `top_speed`.
    pub accel: f32,
    /// Braking (m/s²), scaled with the grip.
    pub brake: f32,
    /// Top speed (m/s).
    pub top_speed: f32,
}

/// The pace of a catalog car group (`Rally2`, `WRC`, `Group B`, ...); unknown groups get a middle class.
pub fn class_pace(class: &str) -> ClassPace {
    let _ = class;
    todo!("rbr-events worker")
}

/// The winning time (s) of a stage over `driveline` (its points in order, m) on `surface`, `wetness` 0 (dry) to 1.
pub fn reference_time(driveline: &[[f32; 3]], surface: Surface, wetness: f32, class: &ClassPace) -> f32 {
    let _ = (driveline, surface, wetness, class);
    todo!("rbr-events worker")
}
