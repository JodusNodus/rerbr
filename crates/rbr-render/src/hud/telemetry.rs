//! The HUD's telemetry parts, each off unless asked for (`viewer --hud-show`): the tyres' temperatures (the live
//! process's TYR1), the trace of the controls over the last [`TRACE_SECONDS`] and the car's acceleration in g, the
//! last two from the drive's frames, so a recorded drive and a replay have them too. Ours; pure.

use rbr_recording::pose::CarPose;
use rbr_recording::tyre::Tyre;

use crate::playback::Sample;

/// The trace's span, s, and the most points it keeps (one per `TRACE_SECONDS / TRACE_POINTS` of drive time).
pub const TRACE_SECONDS: f64 = 10.0;
const TRACE_POINTS: f64 = 300.0;
/// The acceleration is the velocity's change over this long, s: short enough to follow a flick, long enough to smooth
/// the steps' jitter.
pub const G_SECONDS: f64 = 0.15;
const GRAVITY: f32 = 9.806_65;
/// The grip loss that takes a tyre's colour all the way cold or hot (NGP's `RubMyFct` gives 0.84 at 20 °C and 0.85 at
/// 130 °C against 1.0 at its best, `notes/tyre-model.md`).
const GRIP_LOSS: f32 = 0.15;

/// The controls at a moment of the trace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TracePoint {
    /// Seconds before the shown frame.
    pub age: f32,
    /// Throttle, brake, handbrake and clutch, 0 to 1, and the steering, -1 (left) to 1.
    pub control: [f32; 5],
}

/// The trace of `frames` (each with the drive time at its end, in order, the last one the shown frame): the last
/// frame of each slot of drive time, so the points stay put as the trace scrolls.
pub fn trace<'a>(frames: impl Iterator<Item = (f64, &'a Sample)>) -> Vec<TracePoint> {
    let slot = |end: f64| (end * TRACE_POINTS / TRACE_SECONDS).floor();
    let mut points: Vec<(f64, [f32; 5])> = Vec::new();
    for (end, sample) in frames {
        let [throttle, brake, handbrake, clutch, steering, ..] = sample.control;
        let point = (end, [throttle, brake, handbrake, clutch, steering]);
        match points.last_mut() {
            Some(last) if slot(last.0) == slot(end) => *last = point,
            _ => points.push(point),
        }
    }
    let now = points.last().map_or(0.0, |p| p.0);
    points.into_iter().map(|(end, control)| TracePoint { age: (now - end) as f32, control }).collect()
}

/// What the crew feels between two frames `seconds` apart, in g: to the right and forward, in the car's own frame.
/// The velocity's change with gravity taken out, so a car standing on a slope reads the slope.
pub fn g_force(from: &CarPose, to: &CarPose, seconds: f64) -> Option<[f32; 2]> {
    if seconds <= 0.0 {
        return None;
    }
    let felt: [f32; 3] = std::array::from_fn(|i| {
        (to.velocity[i] - from.velocity[i]) / seconds as f32 + if i == 2 { GRAVITY } else { 0.0 }
    });
    let along = |axis: [f32; 3]| {
        let axis = to.rotate(axis);
        (felt[0] * axis[0] + felt[1] * axis[1] + felt[2] * axis[2]) / GRAVITY
    };
    // The car drives along body -Y with +Z up (`rbr_recording::pose`), so its right is -X.
    Some([along([-1.0, 0.0, 0.0]), along([0.0, -1.0, 0.0])])
}

/// A tyre as the HUD shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TyreView {
    /// The temperature its grip follows, degrees C: 0.6 of the surface's and 0.4 of the tread's, the tyre step's
    /// weights.
    pub celsius: f32,
    /// How far its temperature costs grip: -1 cold to 0 at its best to 1 hot.
    pub heat: f32,
}

/// The view of `tyre`, `None` for a wheel not built.
pub fn tyre_view(tyre: &Tyre) -> Option<TyreView> {
    if tyre.tread <= 0.0 {
        return None;
    }
    let kelvin = 0.6 * tyre.surface + 0.4 * tyre.tread;
    let loss = if tyre.grip > 0.0 { ((1.0 - tyre.grip) / GRIP_LOSS).clamp(0.0, 1.0) } else { 0.0 };
    Some(TyreView { celsius: kelvin - 273.15, heat: if kelvin < tyre.optimum { -loss } else { loss } })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(velocity: [f32; 3]) -> CarPose {
        CarPose { position: [0.0; 3], orientation: [0.0, 0.0, 0.0, 1.0], velocity }
    }

    #[test]
    fn a_tyre_reads_cold_below_its_best_temperature_and_hot_above() {
        let tyre = |kelvin: f32, grip: f32| Tyre { surface: kelvin, tread: kelvin, grip, optimum: 348.15 };
        assert_eq!(tyre_view(&Tyre::default()), None);
        let cold = tyre_view(&tyre(293.15, 0.85)).unwrap();
        assert!((cold.celsius - 20.0).abs() < 1e-3 && (cold.heat + 1.0).abs() < 1e-3, "{cold:?}");
        assert_eq!(tyre_view(&tyre(348.15, 1.0)).unwrap().heat, 0.0);
        let hot = tyre_view(&tyre(383.15, 0.925)).unwrap();
        assert!((hot.heat - 0.5).abs() < 1e-3, "{hot:?}");
    }

    #[test]
    fn the_g_force_is_in_the_cars_frame_without_gravity() {
        // Unrotated, the car faces -Y with its right at -X.
        assert_eq!(g_force(&pose([0.0; 3]), &pose([0.0; 3]), 0.0), None);
        let [right, forward] = g_force(&pose([0.0, -10.0, 0.0]), &pose([0.0, -10.0, 0.0]), 0.1).unwrap();
        assert!(right.abs() < 1e-6 && forward.abs() < 1e-6);
        let [right, forward] = g_force(&pose([0.0, -10.0, 0.0]), &pose([-GRAVITY * 0.1, -10.0, 0.0]), 0.1).unwrap();
        assert!((right - 1.0).abs() < 1e-5 && forward.abs() < 1e-6);
        let [_, forward] = g_force(&pose([0.0, -10.0, 0.0]), &pose([0.0, -10.0 + GRAVITY * 0.05, 0.0]), 0.1).unwrap();
        assert!((forward + 0.5).abs() < 1e-5);
    }
}
