//! The brush's calibration on one wheel: NGP's peak forces and cornering slope, swept with its own patch forces
//! (`rbr_physics::tyre::patch_forces`) from the wheel's tables at the nominal load, then solved for
//! ([`crate::calibrate`]), and at each load of the axis for the brush's load level. The point is a unit surface (static
//! friction 1; kinetic the brush's own Coulomb ratio, [`HARD_KINETIC`]), so the game scales the brush's friction by
//! NGP's static friction there; the stiffness factor is 1, and the tread new and at the temperature of the rubber's
//! best friction.

use rbr_physics::tyre::friction_limits::BlendPair;
use rbr_physics::tyre::patch_forces::{patch_forces, PatchForces, PatchInput};

use crate::brush::{Calibration, MU_C_OVER_S};
use crate::calibrate::{calibrate, load_level, CalPoint, CalTargets};
use crate::loose::{self, Levels, Loose};
use crate::Wheel;

/// The sweeps' speed, m/s (ours, as the lab's).
const SPEED: f64 = 15.0;
/// The nominal load's entry in the load axis.
const NOMINAL_LOAD: usize = 3;
/// The kinetic friction of the brush's sweeps (static 1): the brush's own Coulomb over static ratio. NGP's driving peak
/// takes the kinetic friction at light loads (Fiesta Rally2, kinetic 1 against 0.8: 5 % higher at 1 kN, 3 % at 2 kN,
/// the same from 3 kN), so a sweep at kinetic 1 put the brush's light-load longitudinal level above NGP's on tarmac
/// (1.04/1.30 = 0.8).
const HARD_KINETIC: f32 = MU_C_OVER_S as f32;
/// The loose tyre's sweeps' kinetic friction: the unit surface.
const LOOSE_KINETIC: f32 = 1.0;

/// The brush's calibration for `w`.
pub fn calibration(w: &Wheel) -> Result<Calibration, String> {
    let fail = |what: &str| format!("tyre at {:#010x}: {what}", w.tyre);
    let nominal = w.load_axis.get(NOMINAL_LOAD.min(w.load_axis.len().max(1) - 1)).copied();
    let fz0 = nominal.filter(|&fz| fz > 0.0).ok_or_else(|| fail(&format!("nominal load {nominal:?}")))?;
    // The first temperature of the highest friction.
    let best = w.rubber_friction.iter().zip(&w.temperature_axis).fold(None, |best, (&f, &t)| match best {
        Some((m, _)) if f > m => Some((f, t)),
        None => Some((f, t)),
        _ => best,
    });
    let t_cal = f64::from(best.ok_or_else(|| fail("no rubber friction table"))?.1);
    let nominal = targets(w, fz0, HARD_KINETIC)?;
    let point = CalPoint {
        t_cal,
        d_cal: 1.0,
        // The pressure only enters the wet loss, which the dry point does not run.
        p_cal: 0.0,
        pf_cal: 1.0,
        w_cal: 0.0,
        surf_s_cal: 1.0,
        surf_k_cal: 1.0,
        temp_grip_cal: 1.0,
    };
    let cal = calibrate(&nominal, &point);
    // The axis's positive loads and half the first (NGP's tables run on to the load 0 entry); past the ends the power
    // laws alone.
    let loads = load_targets(w, HARD_KINETIC)?;
    let below = targets(w, loads[0].fz0 as f32 / 2.0, HARD_KINETIC)?;
    Ok(Calibration { load_level: load_level(&cal, &[&[below], &loads[..]].concat(), &point), ..cal })
}

/// NGP's targets at each positive load of the wheel's axis, at the kinetic friction `kinetic`.
fn load_targets(w: &Wheel, kinetic: f32) -> Result<Vec<CalTargets>, String> {
    w.load_axis.iter().filter(|&&fz| fz > 0.0).map(|&fz| targets(w, fz, kinetic)).collect()
}

/// The loose-surface tyre for `w` on its brush calibration `cal` ([`crate::loose`]): NGP's free rolling peaks at each
/// positive load of the wheel's axis, on the unit surface, and its cornering slope at the nominal load.
pub fn loose(w: &Wheel, cal: &Calibration) -> Result<Loose, String> {
    let mut levels = Levels::default();
    for t in load_targets(w, LOOSE_KINETIC)? {
        levels.load.push(t.fz0);
        levels.lateral.push(t.peak_lat / t.fz0);
        levels.longitudinal.push(t.peak_lon / t.fz0);
    }
    let slope = targets(w, cal.fz0 as f32, LOOSE_KINETIC)?.fit_lat;
    Ok(loose::calibrate(cal, levels, slope, SPEED))
}

/// NGP's targets at `fz0`: the lateral peak over 0 to 30° of slip angle, the least-squares cornering slope over 0 to
/// 2°, and the longitudinal peak over a slip ratio of 0 to 1 when driving, at the kinetic friction `kinetic`.
fn targets(w: &Wheel, fz0: f32, kinetic: f32) -> Result<CalTargets, String> {
    let base = PatchInput { kinetic_friction: kinetic, ..patch_at(w, fz0)? };
    let at = |vlat: f64, vlong: f64, spin: f64| -> PatchForces {
        let (vlat, vlong) = (vlat as f32, vlong as f32);
        patch_forces(&PatchInput {
            speed: (vlat * vlat + vlong * vlong).sqrt(),
            lateral_velocity: vlat,
            longitudinal_velocity: vlong,
            spin: spin as f32,
            ..base
        })
    };
    let (mut peak_lat, mut sxy, mut sxx) = (0.0f64, 0.0, 0.0);
    for k in 0..=120 {
        let alpha = (f64::from(k) * 0.25).to_radians();
        let vlong = -SPEED * libm::cos(alpha);
        let fy = f64::from(at(SPEED * libm::sin(alpha), vlong, -vlong).lateral_force).abs();
        peak_lat = peak_lat.max(fy);
        if k <= 8 {
            sxy += alpha * fy;
            sxx += alpha * alpha;
        }
    }
    let peak_lon = (0..=50)
        .map(|k| f64::from(at(0.0, -SPEED, (1.0 + f64::from(k) * 0.02) * SPEED).longitudinal_force).abs())
        .fold(0.0, f64::max);
    if !(peak_lat > 0.0 && peak_lon > 0.0 && sxy > 0.0) {
        return Err(format!("tyre at {:#010x}: NGP's peaks {peak_lat} N, {peak_lon} N", w.tyre));
    }
    Ok(CalTargets { fz0: f64::from(fz0), v: SPEED, peak_lat, fit_lat: sxy / sxx, peak_lon })
}

/// NGP's patch input at `fz` on a flat hard road and a unit surface: the load tables blended at `fz` (clamped to the
/// axis, as `locate` clamps), the stiffness factor 1 and a unit radius (the slips take `spin · radius`). The patch length
/// only sets the trail.
fn patch_at(w: &Wheel, fz: f32) -> Result<PatchInput, String> {
    let ax = &w.load_axis;
    let n = ax.len();
    let (lo, hi, frac) = if fz <= ax[0] {
        (0, 0, 0.0)
    } else if fz >= ax[n - 1] {
        (n - 1, n - 1, 0.0)
    } else {
        let lo = (0..n - 1).find(|&k| fz <= ax[k + 1]).unwrap_or(0);
        (lo, lo + 1, (fz - ax[lo]) / (ax[lo + 1] - ax[lo]))
    };
    let entry = |t: &[f32], k: usize| {
        t.get(k).copied().ok_or_else(|| format!("tyre at {:#010x}: a load table shorter than its axis", w.tyre))
    };
    let pair = |t: &[f32]| -> Result<BlendPair, String> { Ok(BlendPair { at_a: entry(t, lo)?, at_b: entry(t, hi)? }) };
    let lerp = |t: &[f32]| -> Result<f32, String> { Ok((1.0 - frac) * entry(t, lo)? + entry(t, hi)? * frac) };
    Ok(PatchInput {
        speed: 0.0,
        lateral_velocity: 0.0,
        longitudinal_velocity: 0.0,
        spin: 0.0,
        effective_radius: 1.0,
        static_friction: 1.0,
        kinetic_friction: 1.0,
        reference_friction: w.reference_friction,
        normal_force: fz,
        cornering_stiffness: lerp(&w.cornering_stiffness)?,
        traction_stiffness: lerp(&w.traction_stiffness)?,
        inclination_sin: 0.0,
        patch_length: 0.1,
        sink_depth: 0.0,
        weight: frac,
        tables: [
            pair(&w.cornering_friction)?,
            pair(&w.traction_friction)?,
            pair(&w.cornering_peak_slip)?,
            pair(&w.traction_peak_slip)?,
        ],
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::brush::{eval, EvalInput};

    /// A wheel with round, made-up tables (not NGP's or a car's).
    pub(crate) fn wheel() -> Wheel {
        Wheel {
            tyre: 0x1000,
            reference_friction: 1.0,
            load_axis: vec![0.0, 1000.0, 2500.0, 4000.0, 6000.0, 8000.0],
            cornering_friction: vec![1.1, 1.05, 1.0, 0.95, 0.9, 0.85],
            traction_friction: vec![1.1, 1.05, 1.0, 0.95, 0.9, 0.85],
            cornering_peak_slip: vec![0.10, 0.11, 0.12, 0.13, 0.14, 0.15],
            traction_peak_slip: vec![0.10, 0.11, 0.12, 0.13, 0.14, 0.15],
            cornering_stiffness: vec![20000.0, 40000.0, 60000.0, 75000.0, 90000.0, 100000.0],
            traction_stiffness: vec![20000.0, 40000.0, 60000.0, 75000.0, 90000.0, 100000.0],
            temperature_axis: vec![250.0, 300.0, 350.0, 400.0],
            rubber_friction: vec![0.8, 0.95, 1.0, 0.9],
        }
    }

    #[test]
    fn the_brush_takes_ngp_s_lateral_peak_and_slope_at_the_nominal_load() {
        let w = wheel();
        let cal = calibration(&w).unwrap();
        let t = targets(&w, 4000.0, HARD_KINETIC).unwrap();
        assert_eq!((cal.fz0, cal.t_cal, cal.pf_cal, cal.surf_s_cal), (4000.0, 350.0, 1.0, 1.0));
        let lateral = |alpha: f64| {
            let x = EvalInput {
                fz: 4000.0,
                half_length: 0.05,
                vx: -SPEED * alpha.cos(),
                vy: SPEED * alpha.sin(),
                spin: SPEED * alpha.cos(),
                radius: 1.0,
                bulk_temp: 350.0,
                tread_remaining: 1.0,
                pressure: 0.0,
                pressure_factor: 1.0,
                wetness: 0.0,
                hydroplaning: false,
                temp_grip: 1.0,
                surf_static: 1.0,
                surf_kinetic: 1.0,
            };
            eval(&x, &cal).fy.abs()
        };
        let peak = (0..=300).map(|k| lateral((f64::from(k) * 0.1).to_radians())).fold(0.0, f64::max);
        assert!((peak / t.peak_lat - 1.0).abs() < 0.01, "peak {peak} NGP {}", t.peak_lat);
        // The least-squares slope over 0 to 2°, as the targets take NGP's.
        let alphas = (1..=8).map(|k| (f64::from(k) * 0.25).to_radians());
        let (sxy, sxx) = alphas.fold((0.0, 0.0), |(sxy, sxx), a| (sxy + a * lateral(a), sxx + a * a));
        assert!((sxy / sxx / t.fit_lat - 1.0).abs() < 0.01, "slope {} NGP {}", sxy / sxx, t.fit_lat);
    }

    #[test]
    fn the_brush_takes_ngp_s_peaks_at_each_load() {
        let w = wheel();
        let cal = calibration(&w).unwrap();
        assert_eq!(cal.load_level.load[..cal.load_level.n], [500.0, 1000.0, 2500.0, 4000.0, 6000.0, 8000.0]);
        let at = |fz: f64, vy: f64, vx: f64, spin: f64| EvalInput {
            fz,
            half_length: 0.05,
            vx,
            vy,
            spin,
            radius: 1.0,
            bulk_temp: 350.0,
            tread_remaining: 1.0,
            pressure: 0.0,
            pressure_factor: 1.0,
            wetness: 0.0,
            hydroplaning: false,
            temp_grip: 1.0,
            surf_static: 1.0,
            surf_kinetic: 1.0,
        };
        for fz in [500.0, 1000.0, 2500.0, 6000.0, 8000.0] {
            let t = targets(&w, fz as f32, HARD_KINETIC).unwrap();
            let lat = (0..=300)
                .map(|k| (f64::from(k) * 0.1).to_radians())
                .map(|a| eval(&at(fz, SPEED * a.sin(), -SPEED * a.cos(), SPEED * a.cos()), &cal).fy.abs())
                .fold(0.0, f64::max);
            let lon = (0..=200)
                .map(|k| eval(&at(fz, 0.0, -SPEED, (1.0 + f64::from(k) * 0.005) * SPEED), &cal).fx.abs())
                .fold(0.0, f64::max);
            assert!((lat / t.peak_lat - 1.0).abs() < 0.002, "{fz} lateral {lat} NGP {}", t.peak_lat);
            assert!((lon / t.peak_lon - 1.0).abs() < 0.002, "{fz} longitudinal {lon} NGP {}", t.peak_lon);
        }
    }

    #[test]
    fn the_loose_tyre_takes_ngp_s_peaks_at_each_load_and_its_slope() {
        let w = wheel();
        let cal = calibration(&w).unwrap();
        let l = loose(&w, &cal).unwrap();
        assert_eq!(l.levels.load, [1000.0, 2500.0, 4000.0, 6000.0, 8000.0]);
        for (k, &fz) in l.levels.load.iter().enumerate() {
            let t = targets(&w, fz as f32, LOOSE_KINETIC).unwrap();
            assert_eq!((l.levels.lateral[k], l.levels.longitudinal[k]), (t.peak_lat / fz, t.peak_lon / fz));
        }
        let slope = targets(&w, 4000.0, LOOSE_KINETIC).unwrap().fit_lat;
        assert!((crate::loose::cornering_slope(&l, SPEED) / slope - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_wheel_without_tables_is_refused() {
        assert!(calibration(&Wheel { load_axis: vec![], ..wheel() }).is_err());
        assert!(calibration(&Wheel { rubber_friction: vec![], ..wheel() }).is_err());
        assert!(calibration(&Wheel { cornering_friction: vec![1.0], ..wheel() }).is_err());
    }
}
