//! The HUD's telemetry parts, each off unless asked for (`viewer --hud-show`): the tyres' temperatures and wear (the
//! live process's TYR1) and the trace of the controls over the last [`TRACE_SECONDS`], from the drive's frames, so a
//! recorded drive and a replay have it too. Ours; pure.

use rbr_recording::tyre::Tyre;

use crate::playback::Sample;

/// The trace's span, s, and the most points it keeps (one per `TRACE_SECONDS / TRACE_POINTS` of drive time).
pub const TRACE_SECONDS: f64 = 10.0;
const TRACE_POINTS: f64 = 300.0;
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

/// A tyre as the HUD shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TyreView {
    /// The temperature its grip follows, degrees C: 0.6 of the surface's and 0.4 of the tread's, the tyre step's
    /// weights.
    pub celsius: f32,
    /// How far its temperature costs grip: -1 cold to 0 at its best to 1 hot.
    pub heat: f32,
    /// The tread worn, percent (the mean of its sectors; a stage wears about 1 %, `notes/tyre-model.md`).
    pub worn: f32,
}

/// The view of `tyre`, `None` for a wheel not built.
pub fn tyre_view(tyre: &Tyre) -> Option<TyreView> {
    if tyre.tread <= 0.0 {
        return None;
    }
    let kelvin = 0.6 * tyre.surface + 0.4 * tyre.tread;
    let loss = if tyre.grip > 0.0 { ((1.0 - tyre.grip) / GRIP_LOSS).clamp(0.0, 1.0) } else { 0.0 };
    let heat = if kelvin < tyre.optimum { -loss } else { loss };
    Some(TyreView { celsius: kelvin - 273.15, heat, worn: tyre.wear * 100.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tyre_reads_cold_below_its_best_temperature_and_hot_above() {
        let tyre = |kelvin: f32, grip: f32| Tyre { surface: kelvin, tread: kelvin, grip, optimum: 348.15, wear: 0.02 };
        assert_eq!(tyre_view(&Tyre::default()), None);
        let cold = tyre_view(&tyre(293.15, 0.85)).unwrap();
        assert!((cold.celsius - 20.0).abs() < 1e-3 && (cold.heat + 1.0).abs() < 1e-3, "{cold:?}");
        assert_eq!(tyre_view(&tyre(348.15, 1.0)).unwrap().heat, 0.0);
        assert_eq!(cold.worn, 2.0);
        let hot = tyre_view(&tyre(383.15, 0.925)).unwrap();
        assert!((hot.heat - 0.5).abs() < 1e-3, "{hot:?}");
    }
}
