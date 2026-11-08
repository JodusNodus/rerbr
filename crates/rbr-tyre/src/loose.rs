//! Loose surfaces, loose-v1 (ours, experimental): the brush's patch ([`brush::patch`]) on soil (gravel and snow), with
//! a flat friction and a ploughing berm in place of the hard surface's friction against sliding speed. The game takes
//! it on soil wheels under the brush ([`crate::car`], `--physics experimental-brush` only); NGP's soil lateral force
//! stays on top (`rbr_physics::tyre::model::apply`).
//!
//! - Level: NGP's. The free rolling peaks μ_y and μ_x at each load of the wheel's axis are NGP's own, swept from its
//!   tables on a unit surface ([`crate::fit::loose`]), interpolated over the load and scaled by NGP's grip (static
//!   friction times the temperature grip). With a flat friction the brush's peak is μ Fz exactly, so no solve is needed.
//! - Lateral: saturates without falling. Coutermarsh & Shoop (SAE 2010-01-0374, CRREL CIV, LT235/75R15, 241 kPa,
//!   6.7 kN, 1-1.5 m/s, Fig. 15, fig ±0.004): on pea stone and wet gravel μ_y saturates at 0.54-0.55 by 20-25° and does
//!   not fall to 38°; little hysteresis on soil. The cornering slope stays NGP's (about twice that road tyre's 3-5 per
//!   rad): the stiffness is solved for it under the flat friction ([`calibrate`]).
//! - Longitudinal: past the free rolling peak the braking force keeps rising, to [`PLOUGH`] times the peak at lock: the
//!   material builds up ahead of a locked tyre (a berm) and stops it harder than rolling (NHTSA DOT HS 808 875 /
//!   SAE 1999-01-1287, 9 cars, 56 km/h, 5 cm crushed limestone: locked over ABS decel 1.25 laden, 1.30 light, per car
//!   1.12-1.42; Macnabb ESV 98 1.21-1.59; Coutermarsh: locked fronts on pea stone Fx/Fz 0.85 against a free rolling
//!   lateral peak 0.54-0.57 and a traction coefficient 0.58-0.68). The berm grows with the braking slip
//!   ([`braking_slip`]) from [`BERM_FROM`] (or the patch's full slide, if later) to lock, a smoothstep; between the
//!   full slide and there the force stays at the peak. No intermediate points are published; NHTSA's mechanism is the
//!   locked tyre's berm, and a berm that grows from the full slide holds the wheels off lock at the Fiesta's full-pedal
//!   brake torque (note, "Loose surfaces: loose-v1"). Driving
//!   slip builds no berm here: the data are braked, and the motion resistance of a spinning tyre's sinkage is not.
//! - Combined: the berm acts against the sliding velocity, with the friction (Svendenius's collinear slide), so a
//!   locked tyre's side force is PLOUGH μ sin α, linear in the slip angle and below free rolling at every angle
//!   (Coutermarsh, Fig. 16, fig ±0.02: 0.72 sin α against a free rolling 0.55, 1.31 sin α of it; here 1.3 sin α).
//!
//! Ours: the smoothstep and its start at a braking slip of 0.8, no berm when driving, PLOUGH the published cars' 1.25-1.30, and the berm independent of the
//! soil's thickness (the measured layers are 5 cm and graded gravel; NGP's 9-30 mm) and of speed (no speed trend in the
//! locked-wheel data, 40-80 km/h).

use crate::brush::{self, Calibration, EvalInput, EvalOutput, KX_OVER_KY, M_CX, M_CY, V_MIN};

/// The braking force of a locked tyre over its free rolling peak on loose surfaces (NHTSA: 1.25 laden, 1.30 light).
pub const PLOUGH: f64 = 1.3;
/// The braking slip the berm starts to grow from (ours: near lock, where the tread stops rolling over the material).
pub const BERM_FROM: f64 = 0.8;

/// NGP's free rolling peak friction on a unit surface at each load: lateral and longitudinal (driving).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Levels {
    /// The loads, N, rising.
    pub load: Vec<f64>,
    pub lateral: Vec<f64>,
    pub longitudinal: Vec<f64>,
}

impl Levels {
    /// The lateral and longitudinal peak friction at `fz`: linear between the loads, the ends held.
    pub fn at(&self, fz: f64) -> (f64, f64) {
        let (l, y, x) = (&self.load, &self.lateral, &self.longitudinal);
        let n = l.len();
        if n == 0 {
            return (0.0, 0.0);
        }
        if fz <= l[0] {
            return (y[0], x[0]);
        }
        if fz >= l[n - 1] {
            return (y[n - 1], x[n - 1]);
        }
        let k = (0..n - 1).find(|&k| fz <= l[k + 1]).unwrap_or(n - 2);
        let f = (fz - l[k]) / (l[k + 1] - l[k]);
        (y[k] + (y[k + 1] - y[k]) * f, x[k] + (x[k + 1] - x[k]) * f)
    }
}

/// One wheel's loose-surface tyre: the brush's stiffness and factors (its friction levels unused) and NGP's levels.
#[derive(Clone, Debug)]
pub struct Loose {
    pub cal: Calibration,
    pub levels: Levels,
}

/// The braking slip, 0 to 1: the share of the contact point's longitudinal speed the patch slides along it (1 locked
/// or turning backwards), 0 when the slip drives. The speed is floored at [`V_MIN`], so it fades to 0 at standstill.
pub fn braking_slip(vsx: f64, vx: f64) -> f64 {
    if vsx * vx > 0.0 {
        (vsx.abs() / vx.abs().max(V_MIN)).min(1.0)
    } else {
        0.0
    }
}

/// The steady loose-surface tyre (see the module): the brush's patch at NGP's levels times the grip, plus the berm.
/// `mu` is the directional friction including the berm (the force's limit).
pub fn eval(x: &EvalInput, c: &Loose) -> EvalOutput {
    let (vsx, vsy, vr) = brush::slip_velocities(x);
    let (sx, sy) = (vsx / vr, vsy / vr);
    let cal = &c.cal;
    let cy = brush::stiffness_at(x.fz, x.bulk_temp, x.tread_remaining, x.pressure_factor, cal.cy0, M_CY, cal);
    let cx = brush::stiffness_at(x.fz, x.bulk_temp, x.tread_remaining, x.pressure_factor, cal.cx0, M_CX, cal);
    let (ly, lx) = c.levels.at(x.fz);
    let grip = (x.surf_static / cal.surf_s_cal) * (x.temp_grip / cal.temp_grip_cal);
    let (mu_y, mu_x) = (ly * grip, lx * grip);
    let mut out = brush::patch(x.fz, x.half_length, cx, cy, mu_x, mu_y, sx, sy);
    // The berm: from BERM_FROM, or the braking slip of the longitudinal full slide (|sx| = 3 mu_x Fz / Cx) if later, to
    // lock.
    let s_abs = sx.hypot(sy);
    let full = 3.0 * mu_x * x.fz / cx;
    let start = (full / (1.0 + full)).max(BERM_FROM);
    let b = braking_slip(vsx, x.vx);
    let t = if start < 1.0 { ((b - start) / (1.0 - start)).clamp(0.0, 1.0) } else { 0.0 };
    let berm = (PLOUGH - 1.0) * t * t * (3.0 - 2.0 * t);
    if berm > 0.0 && s_abs > 0.0 {
        let per_s = berm * out.mu * x.fz / s_abs;
        out.kx += per_s;
        out.ky += per_s;
        out.fx = -out.kx * sx;
        out.fy = -out.ky * sy;
        out.mu *= 1.0 + berm;
        out.trail = if out.fy != 0.0 { -out.mz / out.fy } else { 0.0 };
    }
    out
}

/// The input at the calibration point: load `cal.fz0`, speed `v`, slip angle `alpha` (rad), free rolling, unit radius,
/// the stiffness factors at 1 and a unit surface.
fn cal_input(cal: &Calibration, v: f64, alpha: f64) -> EvalInput {
    EvalInput {
        fz: cal.fz0,
        half_length: 0.07,
        vx: -v * libm::cos(alpha),
        vy: v * libm::sin(alpha),
        spin: v * libm::cos(alpha),
        radius: 1.0,
        bulk_temp: cal.t_cal,
        tread_remaining: cal.d_cal,
        pressure: 0.0,
        pressure_factor: cal.pf_cal,
        wetness: cal.w_cal,
        temp_grip: cal.temp_grip_cal,
        surf_static: cal.surf_s_cal,
        surf_kinetic: cal.surf_k_cal,
    }
}

/// The least-squares cornering slope over 0 to 2° (in 0.25°) at the calibration point, as `crate::fit` takes NGP's.
pub fn cornering_slope(c: &Loose, v: f64) -> f64 {
    let (sxy, sxx) = (1..=8)
        .map(|k| (f64::from(k) * 0.25).to_radians())
        .fold((0.0, 0.0), |(sxy, sxx), a| (sxy + a * eval(&cal_input(&c.cal, v, a), c).fy.abs(), sxx + a * a));
    sxy / sxx
}

/// The loose tyre on the brush's calibration `brush` with NGP's `levels`: the cornering stiffness solved so the slope
/// at speed `v` is `slope` (NGP's, as the brush's), the longitudinal stiffness [`KX_OVER_KY`] times it, as the brush's.
pub fn calibrate(brush: &Calibration, levels: Levels, slope: f64, v: f64) -> Loose {
    let mut c = Loose { cal: *brush, levels };
    // The slope is close to proportional to the stiffness: a fixed point converges in a few steps.
    for _ in 0..100 {
        let r = slope / cornering_slope(&c, v);
        c.cal.cy0 *= r;
        c.cal.cx0 = KX_OVER_KY * c.cal.cy0;
        if (r - 1.0).abs() < 1e-12 {
            break;
        }
    }
    c
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A loose tyre with round numbers (not NGP's): peaks 0.9 / 0.85 at 2 kN, 0.85 / 0.84 at 4 kN, 0.8 / 0.83 at 6 kN,
    /// cornering stiffness 9 per rad of load at 4 kN.
    pub(crate) fn loose() -> Loose {
        let cal = Calibration {
            fz0: 4000.0,
            cy0: 36000.0,
            cx0: KX_OVER_KY * 36000.0,
            mu_ys: 1.0,
            mu_yk: 0.5,
            mu_xs: 1.0,
            mu_xk: 0.5,
            t_cal: 350.0,
            d_cal: 1.0,
            pf_cal: 1.0,
            w_cal: 0.0,
            surf_s_cal: 1.0,
            surf_k_cal: 1.0,
            temp_grip_cal: 1.0,
        };
        let levels = Levels {
            load: vec![2000.0, 4000.0, 6000.0],
            lateral: vec![0.9, 0.85, 0.8],
            longitudinal: vec![0.85, 0.84, 0.83],
        };
        Loose { cal, levels }
    }

    /// At load `fz`, speed `v`, slip angle `alpha` (deg) and slip ratio `kappa` (-1 locked), on a surface of grip
    /// `grip`.
    fn at(c: &Loose, fz: f64, v: f64, alpha: f64, kappa: f64, grip: f64) -> EvalOutput {
        let a = alpha.to_radians();
        let vx = -v * a.cos();
        let x = EvalInput {
            fz,
            vx,
            vy: v * a.sin(),
            spin: (1.0 + kappa) * -vx / 0.3,
            radius: 0.3,
            surf_static: grip,
            surf_kinetic: grip,
            ..cal_input(&c.cal, v, a)
        };
        eval(&x, c)
    }

    #[test]
    fn the_free_rolling_peaks_are_the_levels_times_the_grip_at_each_load() {
        let c = loose();
        for (fz, grip) in [(2000.0, 0.9), (3000.0, 0.9), (4000.0, 0.9), (6000.0, 0.7), (7000.0, 1.0)] {
            let (ly, lx) = c.levels.at(fz);
            let lat = (0..=400).map(|k| at(&c, fz, 15.0, f64::from(k) * 0.1, 0.0, grip).fy.abs()).fold(0.0, f64::max);
            let lon = (0..=100).map(|k| at(&c, fz, 15.0, 0.0, f64::from(k) * 0.01, grip).fx.abs()).fold(0.0, f64::max);
            assert!((lat / (ly * grip * fz) - 1.0).abs() < 1e-6, "{fz} {lat}");
            assert!((lon / (lx * grip * fz) - 1.0).abs() < 1e-6, "{fz} {lon}");
        }
        assert_eq!(c.levels.at(3000.0), (0.875, 0.845));
    }

    #[test]
    fn the_side_force_saturates_without_falling() {
        let c = loose();
        let fy = |a: f64| at(&c, 4000.0, 15.0, a, 0.0, 1.0).fy.abs();
        let peak = 0.85 * 4000.0;
        let mut last = 0.0;
        for k in 0..=400 {
            let f = fy(f64::from(k) * 0.1);
            assert!(f >= last - 1e-9, "{k}");
            last = f;
        }
        assert!((fy(40.0) / peak - 1.0).abs() < 1e-9);
        // Normalised by the peak at 2.5/5/10/15°, at a stiffness of 9 per rad (NGP's gravel tyre's); Coutermarsh's
        // softer road tyre (3-5 per rad) has pea stone 0.56/0.81 of its peak at 5/10°.
        let shape: Vec<f64> = [2.5, 5.0, 10.0, 15.0].iter().map(|&a| fy(a) / peak).collect();
        assert!(shape[0] > 0.35 && shape[0] < 0.5 && shape[2] > 0.85 && shape[3] > 0.95, "{shape:?}");
    }

    #[test]
    fn the_braking_force_rises_past_the_peak_to_the_berm_at_lock() {
        let c = loose();
        for v in [5.0, 15.0, 25.0] {
            let fx = |kappa: f64| at(&c, 4000.0, v, 0.0, kappa, 0.9).fx;
            let peak = 0.84 * 0.9 * 4000.0;
            let mut last = 0.0;
            for k in 0..=1000 {
                let f = fx(-f64::from(k) * 0.001);
                assert!(f >= last - 1e-9, "{v} {k}: {f} after {last}");
                // Continuous: no step larger than the slope allows.
                assert!(f - last < 0.025 * peak, "{v} {k}");
                last = f;
            }
            assert!((fx(-1.0) / peak - PLOUGH).abs() < 1e-9, "{v}");
            // Flat at the peak from the full slide to BERM_FROM.
            assert!((fx(-0.5) / peak - 1.0).abs() < 1e-9 && (fx(-BERM_FROM) / peak - 1.0).abs() < 1e-9, "{v}");
            // Driving: flat at the peak, no berm.
            assert!((fx(1.0) / -peak - 1.0).abs() < 1e-9, "{v}");
            assert!((fx(0.3) / -peak - 1.0).abs() < 1e-9, "{v}");
        }
    }

    #[test]
    fn a_locked_tyre_side_force_follows_the_sliding_velocity_below_free_rolling() {
        let c = loose();
        let peak = 0.85 * 4000.0;
        for a in [5.0, 10.0, 20.0, 35.0] {
            let locked = at(&c, 4000.0, 15.0, a, -1.0, 1.0);
            let free = at(&c, 4000.0, 15.0, a, 0.0, 1.0);
            assert!(locked.fy.abs() < free.fy.abs(), "{a}");
            // PLOUGH mu sin(alpha), mu along the sliding velocity between the axes' peaks.
            let share = locked.fy.abs() / locked.fx.hypot(locked.fy);
            assert!((share - a.to_radians().sin()).abs() < 1e-9, "{a}");
            assert!((locked.fx.hypot(locked.fy) / (locked.mu * 4000.0) - 1.0).abs() < 1e-9, "{a}");
        }
        let ratio = at(&c, 4000.0, 15.0, 35.0, -1.0, 1.0).fy.abs() / peak;
        assert!((0.7..0.78).contains(&ratio), "{ratio}");
    }

    #[test]
    fn it_fades_to_nothing_at_standstill() {
        let c = loose();
        // Locked, the berm fades out over the last V_MIN of speed; stopped, nothing.
        let locked = |v: f64| at(&c, 4000.0, v, 0.0, -1.0, 1.0).fx / (0.84 * 4000.0);
        assert!((locked(0.5) - PLOUGH).abs() < 1e-9);
        assert!(locked(0.45) < PLOUGH && locked(0.45) > 1.0);
        assert_eq!(locked(0.25), 1.0);
        assert!(locked(0.01) < 0.3 && locked(0.01) > 0.0);
        let stopped = at(&c, 4000.0, 0.0, 0.0, 0.0, 1.0);
        assert_eq!((stopped.fx, stopped.fy), (0.0, 0.0));
        assert_eq!(braking_slip(0.0, 0.0), 0.0);
        assert_eq!(braking_slip(-0.2, -0.1), 0.4);
        assert_eq!(braking_slip(0.2, -10.0), 0.0);
    }

    #[test]
    fn the_calibration_takes_the_slope() {
        let c = loose();
        let solved = calibrate(&c.cal, c.levels.clone(), 30000.0, 15.0);
        assert!((cornering_slope(&solved, 15.0) / 30000.0 - 1.0).abs() < 1e-9);
        assert_eq!(solved.cal.cx0, KX_OVER_KY * solved.cal.cy0);
        assert!(solved.cal.cy0 > 30000.0, "the friction curves the slope below the stiffness");
    }
}
