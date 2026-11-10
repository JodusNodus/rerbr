//! Flat-out standing starts (ours, `--flat-out` on NGP cars): upshifts at the thrust crossover and drive-slip
//! throttle control. Pure functions over typed values; `super`'s controller learns the gears' engine speed per m/s
//! of ground speed, reads the torque curve and gear ratios once per run, and logs the shift points once per run.

/// The rev-limit backstop: shift at the limit less this (300 rpm, rad/s), whatever the thrust says.
pub const LIMIT_MARGIN: f32 = 31.4;

/// The flat-out start's torque curve and gear ratios, read from the live car once per run: the knots are NGP's
/// engine definition knots (engine speed rad/s, torque as a fraction of the max) and the ratios the gearbox record's
/// (`rbr_recording::gearbox`), by HUD gear (0 reverse, 1 neutral).
#[derive(Clone, Debug)]
pub struct Curve {
    pub knots: Vec<(f32, f32)>,
    pub ratios: Vec<f32>,
}

/// The flat-out start's drive-slip controller (`--drive-slip`): its slip target K and the integral, the pedal it
/// settles on. It starts at full throttle: under the target the pedal stays there.
#[derive(Clone, Copy, Debug)]
pub struct Drive {
    pub target: f32,
    pub held: f32,
}

impl Drive {
    pub fn new(target: f32) -> Self {
        Self { target, held: 1.0 }
    }
}

/// NGP's full-throttle torque at engine speed `w` (rad/s), as a fraction of the max: the idle table's lookup over
/// the definition's knots (`rbr_physics::ngp::engine::idle_table_lookup`, the port of the curve lookup). At full
/// throttle the braking term is 0 and the damage, density and 1.05 scales multiply every gear alike, so the fraction
/// alone orders the gears.
pub fn torque_at(knots: &[(f32, f32)], w: f32) -> f32 {
    if knots.len() < 2 {
        return knots.first().map_or(0.0, |&(_, y)| y);
    }
    rbr_physics::ngp::engine::idle_table_lookup(
        knots.len(),
        |k| f64::from(knots[k as usize].0),
        |k| f64::from(knots[k as usize].1),
        f64::from(w),
    ) as f32
}

/// The thrust in a gear of `ratio` at engine speed `w`: the wheel torque up to the shared final drive and wheel
/// radius, which cancel across gears at the same ground speed.
pub fn thrust(knots: &[(f32, f32)], ratio: f32, w: f32) -> f32 {
    torque_at(knots, w) * ratio
}

/// Whether to shift up: the next gear's thrust at the ground speed wins, or the engine is at the rev limit less
/// `LIMIT_MARGIN`. `w` is the engine speed, `w_next` the next gear's engine speed at the car's ground speed: a
/// spinning car holds its gear until the next gear pulls harder at the speed the car actually does.
pub fn shift_up(knots: &[(f32, f32)], ratio_now: f32, ratio_next: f32, w: f32, w_next: f32, limit: f32) -> bool {
    thrust(knots, ratio_next, w_next) >= thrust(knots, ratio_now, w) || w >= limit - LIMIT_MARGIN
}

/// The engine speed per m/s of ground speed in `gear`: the learnt `gains` there, else the nearest learnt gear's
/// scaled by the gears' ratios (they share the final drive and wheel radius, so they scale exactly). `None` with no
/// learnt gear or a zero ratio.
pub fn gain(gains: &[f32], ratios: &[f32], gear: u32) -> Option<f32> {
    let ratio = ratios.get(gear as usize).copied().filter(|&r| r > 0.0)?;
    gains
        .iter()
        .enumerate()
        .filter(|(_, &k)| k.is_finite())
        .filter_map(|(g, &k)| ratios.get(g).copied().filter(|&r| r > 0.0).map(|r| (g, k * ratio / r)))
        .min_by_key(|&(g, _)| g.abs_diff(gear as usize))
        .map(|(_, k)| k)
}

/// The shift points: for each gear below the top, the engine rpm at which `shift_up` fires under no slip (the next
/// gear's engine speed from this gear's by their ratios). The 1 rpm scan reads the crossover, or the limit margin
/// where the torque keeps rising to the limiter.
pub fn shift_points(knots: &[(f32, f32)], ratios: &[f32], first: u32, top: u32, limit: f32) -> Vec<(u32, f32)> {
    const RPM: f32 = 30.0 / std::f32::consts::PI;
    let mut out = Vec::new();
    for g in first..top {
        let (Some(&now), Some(&next)) = (ratios.get(g as usize), ratios.get(g as usize + 1)) else { continue };
        if now <= 0.0 || next <= 0.0 {
            continue;
        }
        let mut w = 60.0;
        while !shift_up(knots, now, next, w, w * next / now, limit) {
            w += 1.0;
        }
        out.push((g, w * RPM));
    }
    out
}

/// The power peak's rpm: the maximum of T·ω over the knots' span, scanned at 1 rad/s.
pub fn power_peak(knots: &[(f32, f32)]) -> f32 {
    const RPM: f32 = 30.0 / std::f32::consts::PI;
    let (Some(&(lo, _)), Some(&(hi, _))) = (knots.first(), knots.last()) else { return 0.0 };
    let mut w = lo;
    let (mut peak, mut at) = (f32::NEG_INFINITY, lo);
    while w <= hi {
        let p = torque_at(knots, w) * w;
        if p > peak {
            (peak, at) = (p, w);
        }
        w += 1.0;
    }
    at * RPM
}

/// The most-slipping wheel's drive slip ratio: (|ωR| − v) / |ωR| with the raw rim speed, 0 when |ωR| <= v (a
/// standing wheel with the car moving, or both standing).
pub fn drive_slip(wheels: [f32; 4], v: f32) -> f32 {
    wheels.iter().map(|&w| if w > v { (w - v) / w } else { 0.0 }).fold(0.0, f32::max)
}

/// The drive-slip controller's step: `Brake::Slip`'s PI law (`super::SLIP_I`, `super::SLIP_P`) on the drive slip, the
/// pedal settling where the slip holds `target`. Returns the pedal and the integral.
pub fn drive_step(target: f32, held: f32, slip: f32, dt: f32) -> (f32, f32) {
    let error = target - slip;
    let held = (held + super::SLIP_I * error * dt).clamp(0.0, 1.0);
    ((held + super::SLIP_P * error).clamp(0.0, 1.0), held)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A curve peaking at 5500 rpm and falling steeply past it to 0.5 by the 7500 rpm limit (a restricted turbo
    /// past its power peak).
    fn knots() -> Vec<(f32, f32)> {
        let rpm = |r: f32| r * std::f32::consts::PI / 30.0;
        [(1000.0, 0.55), (3500.0, 0.9), (5500.0, 1.0), (6000.0, 0.9), (6500.0, 0.75), (7000.0, 0.6), (7500.0, 0.5)]
            .into_iter()
            .map(|(r, t)| (rpm(r), t))
            .collect()
    }

    /// Four forward ratios 3.2, 2.1, 1.5, 1.1 by HUD gear (0 reverse, 1 neutral).
    fn ratios() -> Vec<f32> {
        vec![-3.0, 0.0, 3.2, 2.1, 1.5, 1.1]
    }

    #[test]
    fn the_lookup_lerps_inside_and_clamps_outside() {
        let knots = knots();
        let rpm = |r: f32| r * std::f32::consts::PI / 30.0;
        assert!((torque_at(&knots, rpm(4500.0)) - 0.95).abs() < 1e-6);
        assert_eq!(torque_at(&knots, rpm(500.0)), 0.55);
        assert_eq!(torque_at(&knots, rpm(9000.0)), 0.5);
    }

    #[test]
    fn shifts_come_at_the_thrust_crossover() {
        let (knots, ratios) = (knots(), ratios());
        let rpm = |r: f32| r * std::f32::consts::PI / 30.0;
        let limit = rpm(7500.0);
        // Deep in second's power: the next gear's thrust at the ground speed loses.
        let w = rpm(4000.0);
        assert!(!shift_up(&knots, ratios[2], ratios[3], w, w * ratios[3] / ratios[2], limit));
        // Past the crossover (~6950 rpm): third's thrust at the ground speed wins.
        let w = rpm(7100.0);
        assert!(shift_up(&knots, ratios[2], ratios[3], w, w * ratios[3] / ratios[2], limit));
        // Shifting never moves to a lower thrust: at the crossover the next gear wins by construction.
        for g in [2, 3, 4] {
            let (now, next) = (ratios[g], ratios[g + 1]);
            let mut w = rpm(1000.0);
            while !shift_up(&knots, now, next, w, w * next / now, limit) {
                w += 10.0;
            }
            assert!(
                thrust(&knots, next, w * next / now) >= thrust(&knots, now, w),
                "gear {g} shifts into a lower thrust at {w:.0} rad/s"
            );
        }
        // At the limit margin the shift fires whatever the thrust says.
        assert!(shift_up(&knots, ratios[4], ratios[5], rpm(3000.0), rpm(2000.0), rpm(3000.0) + LIMIT_MARGIN));
        assert!(!shift_up(&knots, ratios[4], ratios[5], rpm(3000.0), rpm(2000.0), rpm(3000.0) + LIMIT_MARGIN + 1.0));
    }

    #[test]
    fn wheelspin_holds_the_gear_until_the_next_gear_pulls() {
        let (knots, ratios) = (knots(), ratios());
        let rpm = |r: f32| r * std::f32::consts::PI / 30.0;
        let limit = rpm(7500.0);
        // Spinning in second at 7000 rpm with the car at 40 km/h: third at the ground speed bogs, so hold
        // (20 rad/s per m/s is second's no-slip gain).
        let (w, v) = (rpm(7000.0), 40.0 / 3.6);
        let w_next = v * 20.0 * ratios[3] / ratios[2];
        assert!(!shift_up(&knots, ratios[2], ratios[3], w, w_next, limit));
        // The car at 120 km/h: third at the ground speed pulls harder, so shift despite the spinning engine.
        let w_next = 120.0 / 3.6 * 20.0 * ratios[3] / ratios[2];
        assert!(shift_up(&knots, ratios[2], ratios[3], w, w_next, limit));
    }

    #[test]
    fn the_gain_comes_from_the_nearest_learnt_gear() {
        let ratios = ratios();
        let gains = [f32::INFINITY, f32::INFINITY, 20.0, f32::INFINITY, f32::INFINITY, f32::INFINITY];
        // Third from second's: the ratios' scale.
        assert!((gain(&gains, &ratios, 3).unwrap() - 20.0 * ratios[3] / ratios[2]).abs() < 1e-6);
        // Nothing learnt: none.
        assert!(gain(&[f32::INFINITY; 6], &ratios, 3).is_none());
        // A zero ratio (neutral): none.
        assert!(gain(&gains, &ratios, 1).is_none());
    }

    #[test]
    fn the_shift_points_sit_past_the_power_peak() {
        let (knots, ratios) = (knots(), ratios());
        let rpm = |r: f32| r * std::f32::consts::PI / 30.0;
        let limit = rpm(7500.0);
        let peak = power_peak(&knots);
        assert!((peak - 5500.0).abs() < 20.0, "{peak}");
        for (g, at) in shift_points(&knots, &ratios, 2, 5, limit) {
            assert!(at > peak, "gear {g} shifts at {at:.0} rpm, below the {peak:.0} rpm peak");
            assert!(at <= 7500.0 - 300.0 + 10.0, "gear {g} shifts at {at:.0} rpm");
        }
    }

    #[test]
    fn the_drive_controller_holds_its_target() {
        let dt = 1.0 / 720.0;
        // Over the target the pedal drops and the integral falls.
        let (pedal, held) = drive_step(0.1, 1.0, 0.5, dt);
        assert!(pedal < 1.0 && held < 1.0, "{pedal} {held}");
        // Under the target from empty the pedal rises towards full.
        let (up, _) = drive_step(0.1, 0.0, 0.0, dt);
        let (pedal, held) = drive_step(0.1, 1.0, 0.0, dt);
        assert!(up > 0.0 && pedal == 1.0 && held == 1.0, "{up} {pedal} {held}");
        // At the target it holds steady.
        let (pedal, held) = drive_step(0.1, 0.6, 0.1, dt);
        assert!((pedal - 0.6).abs() < 1e-6 && (held - 0.6).abs() < 1e-6, "{pedal} {held}");
    }

    #[test]
    fn the_drive_slip_reads_the_most_slipping_wheel() {
        assert_eq!(drive_slip([10.0, 11.0, 12.0, 13.0], 10.0), (13.0 - 10.0) / 13.0);
        assert_eq!(drive_slip([8.0, 9.0, 9.5, 10.0], 10.0), 0.0);
        assert_eq!(drive_slip([0.0; 4], 0.0), 0.0);
    }
}
