//! Numerical calibration of the brush-v1 [`Calibration`][brush::Calibration].
//!
//! Research tooling (ours, experimental): solves the nominal-point
//! cornering stiffness and the friction levels from measured targets, so no
//! NGP-derived numbers live in committed code. The caller (e.g.
//! `examples/compare.rs`) feeds plain values — NGP's swept peak force per
//! axis and fitted cornering slope — plus the nominal operating point from
//! the probe dump; the solve itself only evaluates the brush model. The
//! longitudinal stiffness (`KX_OVER_KY`) and the Coulomb over static ratio
//! (`MU_C_OVER_S`) are published values, not solved. Lab code only.

use super::brush::{eval, Calibration, EvalInput, KX_OVER_KY, MU_C_OVER_S};

/// The nominal operating point every factor is normalised to: the probe
/// dump's tread temperature, tread remaining, pressure, pressure factor,
/// wetness, surface values, temperature grip, and NGP's pressure table.
#[derive(Clone, Copy, Debug)]
pub struct CalPoint {
    pub t_cal: f64,
    pub d_cal: f64,
    pub p_cal: f64,
    pub pf_cal: f64,
    pub w_cal: f64,
    pub surf_s_cal: f64,
    pub surf_k_cal: f64,
    pub temp_grip_cal: f64,
    pub stiff_axis: [f64; 4],
    pub stiff_val: [f64; 4],
}

/// Measured targets at load `fz0` and speed `v`: the peak force per axis
/// (longitudinal: drive side) and the fitted cornering slope (least squares
/// over alpha <= 2 deg).
#[derive(Clone, Copy, Debug)]
pub struct CalTargets {
    pub fz0: f64,
    pub v: f64,
    pub peak_lat: f64,
    pub fit_lat: f64,
    pub peak_lon: f64,
}

/// The calibration for cornering stiffness `cy0` and static friction levels
/// `mu_ys`, `mu_xs`: Cx0 and the Coulomb levels follow from the published
/// ratios.
fn trial_cal(cy0: f64, mu_ys: f64, mu_xs: f64, t: &CalTargets, p: &CalPoint) -> Calibration {
    Calibration {
        fz0: t.fz0,
        cy0,
        cx0: KX_OVER_KY * cy0,
        mu_ys,
        mu_yk: MU_C_OVER_S * mu_ys,
        mu_xs,
        mu_xk: MU_C_OVER_S * mu_xs,
        t_cal: p.t_cal,
        d_cal: p.d_cal,
        p_cal: p.p_cal,
        pf_cal: p.pf_cal,
        w_cal: p.w_cal,
        surf_s_cal: p.surf_s_cal,
        surf_k_cal: p.surf_k_cal,
        temp_grip_cal: p.temp_grip_cal,
        stiff_axis: p.stiff_axis,
        stiff_val: p.stiff_val,
    }
}

fn lat_input(alpha: f64, v: f64, t: &CalTargets, p: &CalPoint) -> EvalInput {
    EvalInput {
        fz: t.fz0,
        half_length: 0.07,
        vx: -v * alpha.cos(),
        vy: v * alpha.sin(),
        // Unit radius: R cancels out of the slips (spin is set from vx).
        spin: v * alpha.cos(),
        radius: 1.0,
        bulk_temp: p.t_cal,
        tread_remaining: p.d_cal,
        pressure: p.p_cal,
        wetness: p.w_cal,
        temp_grip: p.temp_grip_cal,
        surf_static: p.surf_s_cal,
        surf_kinetic: p.surf_k_cal,
    }
}

fn lon_input(kappa: f64, v: f64, t: &CalTargets, p: &CalPoint) -> EvalInput {
    EvalInput {
        fz: t.fz0,
        half_length: 0.07,
        vx: -v,
        vy: 0.0,
        // Unit radius (see above).
        spin: (1.0 + kappa) * v,
        radius: 1.0,
        bulk_temp: p.t_cal,
        tread_remaining: p.d_cal,
        pressure: p.p_cal,
        wetness: p.w_cal,
        temp_grip: p.temp_grip_cal,
        surf_static: p.surf_s_cal,
        surf_kinetic: p.surf_k_cal,
    }
}

/// Lateral peak over a fine grid and the least-squares slope over alpha <=
/// 2 deg.
fn lat_peak_fit(cy0: f64, mu_ys: f64, t: &CalTargets, p: &CalPoint) -> (f64, f64) {
    let cal = trial_cal(cy0, mu_ys, 1.0, t, p);
    let mut peak = 0.0f64;
    let mut a = 0.0f64;
    while a <= 30.0 {
        peak = peak.max(eval(&lat_input(a.to_radians(), t.v, t, p), &cal).fy.abs());
        a += 0.05;
    }
    let (mut sxy, mut sxx) = (0.0, 0.0);
    let mut k = 1;
    while k as f64 * 0.25 <= 2.0 + 1e-9 {
        let alpha = k as f64 * 0.25 * std::f64::consts::PI / 180.0;
        let f = eval(&lat_input(alpha, t.v, t, p), &cal).fy.abs();
        sxy += alpha * f;
        sxx += alpha * alpha;
        k += 1;
    }
    (peak, sxy / sxx)
}

/// Longitudinal drive-side peak over a fine grid.
fn lon_peak(cy0: f64, mu_xs: f64, t: &CalTargets, p: &CalPoint) -> f64 {
    let cal = trial_cal(cy0, 1.0, mu_xs, t, p);
    let mut peak = 0.0f64;
    let mut k = 0.0f64;
    while k <= 1.0 {
        peak = peak.max(eval(&lon_input(k, t.v, t, p), &cal).fx.abs());
        k += 0.005;
    }
    peak
}

/// Fixed point `x <- x want / got(x)`: both the peak against the friction
/// level and the fitted slope against the stiffness are close to
/// proportional, so it converges in a few steps.
fn scale_to(want: f64, start: f64, got: &dyn Fn(f64) -> f64) -> f64 {
    let mut x = start;
    for _ in 0..100 {
        let r = want / got(x);
        x *= r;
        if (r - 1.0).abs() < 1e-13 {
            break;
        }
    }
    x
}

/// Solve the cornering stiffness and the two static friction levels from the
/// targets: the lateral pair nested (outer slope on Cy, inner peak on mu_ys),
/// then the longitudinal level at the resulting Cx. Deterministic.
pub fn calibrate(t: &CalTargets, p: &CalPoint) -> Calibration {
    let (mut cy0, mut mu_ys) = (70000.0, 1.3);
    for _ in 0..100 {
        mu_ys = scale_to(t.peak_lat, mu_ys, &|m| lat_peak_fit(cy0, m, t, p).0);
        let r = t.fit_lat / lat_peak_fit(cy0, mu_ys, t, p).1;
        cy0 *= r;
        if (r - 1.0).abs() < 1e-13 {
            break;
        }
    }
    let mu_ys = scale_to(t.peak_lat, mu_ys, &|m| lat_peak_fit(cy0, m, t, p).0);
    let mu_xs = scale_to(t.peak_lon, 1.3, &|m| lon_peak(cy0, m, t, p));
    trial_cal(cy0, mu_ys, mu_xs, t, p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth_point() -> CalPoint {
        CalPoint {
            t_cal: 300.0,
            d_cal: 1.0,
            p_cal: 200000.0,
            pf_cal: 1.0,
            w_cal: 0.0,
            surf_s_cal: 1.3,
            surf_k_cal: 1.0,
            temp_grip_cal: 1.0,
            stiff_axis: [0.0, 150000.0, 300000.0, 500000.0],
            stiff_val: [1.0, 1.0, 1.0, 1.0],
        }
    }

    #[test]
    fn calibrate_hits_synthetic_peaks_and_slope() {
        // Round synthetic targets (not NGP's values).
        let t = CalTargets { fz0: 4000.0, v: 15.0, peak_lat: 5200.0, fit_lat: 70000.0, peak_lon: 5100.0 };
        let p = synth_point();
        let cal = calibrate(&t, &p);
        // Lateral: peak over an independent grid, slope fit.
        let mut peak = 0.0f64;
        let mut a = 0.0f64;
        while a <= 30.0 {
            peak = peak.max(eval(&lat_input(a.to_radians(), t.v, &t, &p), &cal).fy.abs());
            a += 0.1;
        }
        assert!((peak / t.peak_lat - 1.0).abs() < 0.01, "peak {peak}");
        let (mut sxy, mut sxx) = (0.0, 0.0);
        let mut k = 1;
        while k as f64 * 0.25 <= 2.0 + 1e-9 {
            let alpha = k as f64 * 0.25 * std::f64::consts::PI / 180.0;
            sxy += alpha * eval(&lat_input(alpha, t.v, &t, &p), &cal).fy.abs();
            sxx += alpha * alpha;
            k += 1;
        }
        assert!((sxy / sxx / t.fit_lat - 1.0).abs() < 0.02, "fit {}", sxy / sxx);
        // Longitudinal: drive peak; stiffness and Coulomb level from the ratios.
        let mut peak = 0.0f64;
        let mut k = 0.0f64;
        while k <= 1.0 {
            peak = peak.max(eval(&lon_input(k, t.v, &t, &p), &cal).fx.abs());
            k += 0.01;
        }
        assert!((peak / t.peak_lon - 1.0).abs() < 0.01, "peak {peak}");
        assert_eq!(cal.cx0, KX_OVER_KY * cal.cy0);
        assert_eq!(cal.mu_yk, MU_C_OVER_S * cal.mu_ys);
        assert_eq!(cal.mu_xk, MU_C_OVER_S * cal.mu_xs);
    }
}
