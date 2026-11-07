//! Numerical calibration of the brush-v0 [`Calibration`][brush::Calibration].
//!
//! Research tooling (ours, experimental): solves the nominal-point
//! stiffnesses and friction levels from measured targets, so no NGP-derived
//! numbers live in committed code. The caller (e.g. `examples/compare.rs`)
//! feeds plain values — NGP's swept peak force, slide force and fitted slope
//! per axis — plus the nominal operating point from the probe dump; the solve
//! itself only evaluates the brush model. Lab code only.

use super::brush::{eval, Calibration, EvalInput};

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

/// Measured targets at load `fz0` and speed `v`: the peak force, the sliding
/// force (lateral: 30 deg slip angle; longitudinal: kappa = 0.5 drive, the
/// same Vs = 7.5 m/s sliding speed at 15 m/s) and the fitted small-slip slope
/// (lateral: least squares over alpha <= 2 deg; longitudinal: over
/// |kappa| <= 0.04).
#[derive(Clone, Copy, Debug)]
pub struct CalTargets {
    pub fz0: f64,
    pub v: f64,
    pub peak_lat: f64,
    pub slide_lat: f64,
    pub fit_lat: f64,
    pub peak_lon: f64,
    pub slide_lon: f64,
    pub fit_lon: f64,
}

#[allow(clippy::too_many_arguments)]
fn trial_cal(
    cy0: f64,
    cx0: f64,
    mu_ys: f64,
    mu_yk: f64,
    mu_xs: f64,
    mu_xk: f64,
    t: &CalTargets,
    p: &CalPoint,
) -> Calibration {
    Calibration {
        fz0: t.fz0,
        cy0,
        cx0,
        mu_ys,
        mu_yk,
        mu_xs,
        mu_xk,
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

fn lat_forces(cy0: f64, mu_yk: f64, mu_ys: f64, t: &CalTargets, p: &CalPoint) -> (f64, f64, f64) {
    let cal = trial_cal(cy0, 1.0, mu_ys, mu_yk, 1.0, 1.0, t, p);
    // Peak over a fine grid, slide at 30 deg, LS slope over alpha <= 2 deg.
    let mut peak = 0.0f64;
    let mut a = 0.0f64;
    while a <= 30.0 {
        peak = peak.max(eval(&lat_input(a.to_radians(), t.v, t, p), &cal).fy.abs());
        a += 0.05;
    }
    let slide = eval(&lat_input(30.0f64.to_radians(), t.v, t, p), &cal).fy.abs();
    let (mut sxy, mut sxx) = (0.0, 0.0);
    let mut k = 1;
    while k as f64 * 0.25 <= 2.0 + 1e-9 {
        let alpha = k as f64 * 0.25 * std::f64::consts::PI / 180.0;
        let f = eval(&lat_input(alpha, t.v, t, p), &cal).fy.abs();
        sxy += alpha * f;
        sxx += alpha * alpha;
        k += 1;
    }
    (peak, slide, sxy / sxx)
}

fn lon_forces(cx0: f64, mu_xk: f64, mu_xs: f64, t: &CalTargets, p: &CalPoint) -> (f64, f64, f64) {
    let cal = trial_cal(1.0, cx0, 1.0, 1.0, mu_xs, mu_xk, t, p);
    // Drive-side peak, slide at kappa = 0.5, LS slope over |kappa| <= 0.04.
    let mut peak = 0.0f64;
    let mut k = 0.0f64;
    while k <= 1.0 {
        peak = peak.max(eval(&lon_input(k, t.v, t, p), &cal).fx.abs());
        k += 0.005;
    }
    let slide = eval(&lon_input(0.5, t.v, t, p), &cal).fx.abs();
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for kk in [0.02, -0.02, 0.04, -0.04] {
        let f = eval(&lon_input(kk, t.v, t, p), &cal).fx.abs();
        sxy += kk.abs() * f;
        sxx += kk * kk;
    }
    (peak, slide, sxy / sxx)
}

/// Solve J^T d = r by partial-pivot Gaussian elimination, and take a damped
/// Damped Newton step: solve J^T d = r by partial-pivot Gaussian elimination,
/// then halve the step until the relative residual norm falls. The damping
/// only guards the iteration; the converged point is still Newton's root.
/// 2x2 damped Newton on (K, S) at fixed C for the peak and slide targets.
fn solve_ks(want: [f64; 2], k: &mut f64, s: &mut f64, peak_slide: &dyn Fn(f64, f64) -> [f64; 2]) {
    for _ in 0..50 {
        let r = {
            let v = peak_slide(*k, *s);
            [v[0] - want[0], v[1] - want[1]]
        };
        if (r[0] / want[0]).abs() < 1e-13 && (r[1] / want[1]).abs() < 1e-13 {
            break;
        }
        let dk = *k * 1e-6 + 1e-9;
        let ds = *s * 1e-6 + 1e-9;
        let rk = {
            let v = peak_slide(*k + dk, *s);
            [(v[0] - want[0] - r[0]) / dk, (v[1] - want[1] - r[1]) / dk]
        };
        let rs = {
            let v = peak_slide(*k, *s + ds);
            [(v[0] - want[0] - r[0]) / ds, (v[1] - want[1] - r[1]) / ds]
        };
        let det = rk[0] * rs[1] - rk[1] * rs[0];
        assert!(det != 0.0, "singular calibration Jacobian");
        let d = [(rs[1] * r[0] - rs[0] * r[1]) / det, (rk[0] * r[1] - rk[1] * r[0]) / det];
        let n0 = (r[0] / want[0]).powi(2) + (r[1] / want[1]).powi(2);
        let mut step = 1.0;
        loop {
            let k2 = *k - step * d[0];
            let s2 = *s - step * d[1];
            let v = peak_slide(k2, s2);
            let n = ((v[0] - want[0]) / want[0]).powi(2) + ((v[1] - want[1]) / want[1]).powi(2);
            if n < n0 || step < 1e-10 {
                *k = k2;
                *s = s2;
                break;
            }
            step *= 0.5;
        }
    }
}

/// Nested solve: outer fixed point on C (the fitted slope over C drifts only
/// slowly with C), inner 2x2 Newton on (K, S) for peak and slide at fixed C.
/// Robust where joint 3D Newton crawls along the peak/slide manifold.
fn solve_axis(
    want_peak: f64,
    want_slide: f64,
    want_fit: f64,
    start: [f64; 3],
    sweep: &dyn Fn(f64, f64, f64) -> (f64, f64, f64),
) -> (f64, f64, f64) {
    let (mut c, mut k, mut s) = (start[0], start[1], start[2]);
    for _ in 0..50 {
        solve_ks([want_peak, want_slide], &mut k, &mut s, &|kk, ss| {
            let (pk, sl, _) = sweep(c, kk, ss);
            [pk, sl]
        });
        let (_, _, ft) = sweep(c, k, s);
        let c_new = want_fit / (ft / c);
        if ((c_new - c) / c).abs() < 1e-13 {
            c = c_new;
            break;
        }
        c = c_new;
    }
    (c, k, s)
}

fn solve_lat(t: &CalTargets, p: &CalPoint) -> (f64, f64, f64) {
    solve_axis(t.peak_lat, t.slide_lat, t.fit_lat, [70000.0, 0.8, 1.3], &|c, k, s| lat_forces(c, k, s, t, p))
}

fn solve_lon(t: &CalTargets, p: &CalPoint) -> (f64, f64, f64) {
    solve_axis(t.peak_lon, t.slide_lon, t.fit_lon, [60000.0, 0.8, 1.5], &|c, k, s| lon_forces(c, k, s, t, p))
}

/// Solve the nominal-point stiffnesses and friction levels from measured
/// targets: per axis, the peak force, the sliding force and the fitted
/// small-slip slope. Deterministic nested iteration from round starts.
pub fn calibrate(t: &CalTargets, p: &CalPoint) -> Calibration {
    let (cy0, mu_yk, mu_ys) = solve_lat(t, p);
    let (cx0, mu_xk, mu_xs) = solve_lon(t, p);
    trial_cal(cy0, cx0, mu_ys, mu_yk, mu_xs, mu_xk, t, p)
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
    fn calibrate_hits_synthetic_peak_slide_and_slope() {
        // Round synthetic targets (not NGP's values).
        let t = CalTargets {
            fz0: 4000.0,
            v: 15.0,
            peak_lat: 5200.0,
            slide_lat: 4160.0,
            fit_lat: 70000.0,
            peak_lon: 5100.0,
            slide_lon: 4080.0,
            fit_lon: 56000.0,
        };
        let p = synth_point();
        let cal = calibrate(&t, &p);
        // Lateral: peak over an independent grid, slide at 30 deg, slope fit.
        let mut peak = 0.0f64;
        let mut a = 0.0f64;
        while a <= 30.0 {
            peak = peak.max(eval(&lat_input(a.to_radians(), t.v, &t, &p), &cal).fy.abs());
            a += 0.1;
        }
        assert!((peak / t.peak_lat - 1.0).abs() < 0.01, "peak {peak}");
        let slide = eval(&lat_input(30.0f64.to_radians(), t.v, &t, &p), &cal).fy.abs();
        assert!((slide / peak - 0.8).abs() < 0.01, "slide {slide} peak {peak}");
        let (mut sxy, mut sxx) = (0.0, 0.0);
        let mut k = 1;
        while k as f64 * 0.25 <= 2.0 + 1e-9 {
            let alpha = k as f64 * 0.25 * std::f64::consts::PI / 180.0;
            sxy += alpha * eval(&lat_input(alpha, t.v, &t, &p), &cal).fy.abs();
            sxx += alpha * alpha;
            k += 1;
        }
        assert!((sxy / sxx / t.fit_lat - 1.0).abs() < 0.02, "fit {}", sxy / sxx);
        // Longitudinal: drive peak, slide at kappa = 0.5, slope fit.
        let mut peak = 0.0f64;
        let mut k = 0.0f64;
        while k <= 1.0 {
            peak = peak.max(eval(&lon_input(k, t.v, &t, &p), &cal).fx.abs());
            k += 0.01;
        }
        assert!((peak / t.peak_lon - 1.0).abs() < 0.01, "peak {peak}");
        let slide = eval(&lon_input(0.5, t.v, &t, &p), &cal).fx.abs();
        assert!((slide / peak - 0.8).abs() < 0.01, "slide {slide} peak {peak}");
        let (mut sxy, mut sxx) = (0.0, 0.0);
        for kk in [0.02, -0.02, 0.04, -0.04] {
            let f = eval(&lon_input(kk, t.v, &t, &p), &cal).fx.abs();
            sxy += kk.abs() * f;
            sxx += kk * kk;
        }
        assert!((sxy / sxx / t.fit_lon - 1.0).abs() < 0.02, "fit {}", sxy / sxx);
    }
}
