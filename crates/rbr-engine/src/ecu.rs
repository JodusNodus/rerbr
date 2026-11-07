//! The control unit: idle, overrun fuel cut, the rev limiter's firing events, the shift cut, anti-lag and the
//! electronic throttle (sources in `notes/engine-model.md`).

use libm::exp;

/// Spark torque ratio η_ign = max(0, 1 − 2.8e-4·Δθ²), Δθ degrees from MBT (MIT 2.61: 1 % at 5–7°; quadratic, US
/// 6363317).
pub const SPARK_C: f64 = 2.8e-4;
pub fn spark_efficiency(retard: f64) -> f64 {
    (1.0 - SPARK_C * retard * retard).max(0.0)
}

/// Idle: 27° base retard, a torque reserve with η_ign about 0.8 (an MPC idle's spark-efficiency setpoint 0.75,
/// arXiv 2012.02859; idle control uses at most about 30 % of the reserve, US 7418943), the spark free 10° earlier and
/// 15° later (MoTeC idle setup); the loops' bandwidths, Hz (inferred), and the idle air's ceiling, the plate's area at
/// 10 % position (Haltech's DBW idle maximum starts at about 10 %, HPA webinar 138).
pub const IDLE_RETARD: f64 = 27.0;
pub const IDLE_ADVANCE_ROOM: f64 = 10.0;
pub const IDLE_RETARD_ROOM: f64 = 15.0;
pub const IDLE_AIR_HZ: f64 = 0.5;
pub const IDLE_SPARK_HZ: f64 = 3.0;
pub const IDLE_AIR_MAX_LIFT: f64 = 0.10;
/// The idle loops run with the pedal closed below idle + 800 rpm (the overrun cut's band, inferred).
pub const IDLE_BAND: f64 = 800.0;
/// The idle air loop integrates only below idle + 100 rpm (inferred: no wind-down while the overrun air decays).
pub const IDLE_INTEGRATE: f64 = 100.0;
/// Overrun fuel cut: pedal under 0.02 above idle + 800 rpm, resumed below idle + 300 rpm or on the pedal (stealth316,
/// MSExtra, Hondata).
pub const PEDAL_CLOSED: f64 = 0.02;
pub const DFCO_ON: f64 = 800.0;
pub const DFCO_OFF: f64 = 300.0;
/// The overrun air: with the fuel cut the manifold holds 0.2·p_a (decel vacuum 17–23 kPa), sized at 3000 rpm and
/// opened in proportion to the cylinders' flow at other speeds, so the hold is the same at every speed.
pub const OVERRUN_P: f64 = 0.2;
pub const OVERRUN_RPM: f64 = 3000.0;
/// The overrun air's decay once the cut ends, s: a dashpot's or idle valve's hand-over to the idle loop (inferred).
pub const OVERRUN_TAU: f64 = 0.5;
/// DBW: throttle aim = pedal^1.3 (HPA webinar 138: 40 % pedal about 30 % throttle; inferred exponent through it).
pub const DBW_EXPONENT: f64 = 1.3;
/// Rev limiter: ignition cut 0 → 100 % over 200 rpm above the limit, then fuel cut (MoTeC M1 defaults, HPA).
pub const LIMITER_RANGE: f64 = 200.0;
/// No combustion below this speed, rpm (cranking speeds 150–250 rpm, turboford).
pub const FIRE_RPM: f64 = 150.0;
/// Anti-lag (MoTeC anti-lag setup, Link forum): armed above 0.45·RPMLimit, active with the pedal under 0.1 for at most
/// 3 s, re-armed by a pedal over 0.3; the throttle open 20 % of its area (Link forum: about 20 % kick, 25 % in the
/// closed-pedal cells), 60° retard, +20 % fuel, 30 % cut, 1.2 bar boost aim, 60 % of the unburnt fuel burning in the
/// manifold. The kick is an area: a 15 % lift opens 5 % of the area and lets the turbo run down.
pub const ALS_ARM: f64 = 0.45;
pub const ALS_LIFT: f64 = 0.1;
pub const ALS_REARM: f64 = 0.3;
pub const ALS_TIMEOUT: f64 = 3.0;
pub const ALS_AREA: f64 = 0.2;
pub const ALS_RETARD: f64 = 60.0;
pub const ALS_TRIM: f64 = 0.2;
pub const ALS_CUT: f64 = 0.3;
pub const ALS_BOOST: f64 = 1.2e5;
pub const ALS_BURN: f64 = 0.6;
/// The electronic throttle: 30 ms first order, 40 ms full travel (Gorzelic 2012).
const ETB_TAU: f64 = 0.03;
const ETB_TRAVEL: f64 = 0.04;

/// The electronic throttle's position after `dt` towards `target`.
pub fn etb(position: f64, target: f64, dt: f64) -> f64 {
    let step = (target - position) * (1.0 - exp(-dt / ETB_TAU));
    (position + step.clamp(-dt / ETB_TRAVEL, dt / ETB_TRAVEL)).clamp(0.0, 1.0)
}

/// The firing events: a phase accumulator over steps and deterministic error diffusion of the cut share.
#[derive(Clone, Copy, Debug, Default)]
pub struct Events {
    phase: f64,
    error: f64,
    /// The fired share of the last step that had events.
    pub fired: f64,
}

impl Events {
    pub fn new() -> Self {
        Self { phase: 0.0, error: 0.0, fired: 1.0 }
    }

    /// Advances by `events` firing events (fractional), each cut with the share `cut`, and gives the fired share of
    /// this step's events (the last step's without one).
    pub fn step(&mut self, events: f64, cut: f64) -> f64 {
        self.phase += events.clamp(0.0, 64.0);
        let (mut total, mut fired) = (0u32, 0u32);
        while self.phase >= 1.0 {
            self.phase -= 1.0;
            total += 1;
            self.error += cut;
            if self.error >= 0.5 {
                self.error -= 1.0;
            } else {
                fired += 1;
            }
        }
        if total > 0 {
            self.fired = f64::from(fired) / f64::from(total);
        }
        self.fired
    }
}

/// Anti-lag's timer: whether it runs this step.
#[derive(Clone, Copy, Debug, Default)]
pub struct AntiLag {
    left: f64,
}

impl AntiLag {
    pub fn step(&mut self, pedal: f64, rpm: f64, rpm_limit: f64, dt: f64) -> bool {
        if pedal > ALS_REARM {
            self.left = ALS_TIMEOUT;
        }
        let active = pedal < ALS_LIFT && rpm > ALS_ARM * rpm_limit && self.left > 0.0;
        if active {
            self.left -= dt;
        }
        active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_diffusion_cuts_its_share() {
        let mut e = Events::new();
        let (mut fired, mut n) = (0.0, 0);
        for _ in 0..1000 {
            fired += e.step(1.0, 0.3);
            n += 1;
        }
        assert!((fired / f64::from(n) - 0.7).abs() < 0.01);
        assert_eq!(Events::new().step(0.4, 1.0), 1.0);
    }

    #[test]
    fn the_spark_curve_loses_one_percent_at_six_degrees() {
        assert!((spark_efficiency(6.0) - 0.99).abs() < 0.001);
        assert_eq!(spark_efficiency(60.0), 0.0);
    }
}
