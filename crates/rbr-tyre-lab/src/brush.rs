//! Candidate tyre model "brush-v1" (ours, experimental).
//!
//! A steady-state brush with Pacejka's theoretical slips, the Stribeck
//! friction of Canudas de Wit et al.'s distributed LuGre (brush) tyre model,
//! load-sensitive stiffness and friction, bulk temperature / wear / pressure
//! factors, a hydroplaning-style wet loss, and a single-contact-point
//! transient. Pure functions over plain values (`f64` is fine here: nothing
//! in the lab needs bit-exactness).
//!
//! Lab code only: nothing here is wired into the game, and nothing in
//! `rbr-physics` / `rbr-hybrid` / `rbr-standalone` changes.
//!
//! No NGP-derived numbers live in this file: the nominal-point stiffnesses,
//! friction levels, temperatures, pressures, surface values and the pressure
//! table all arrive at run time in [`Calibration`] (solved by
//! `crate::calibrate` from a probe dump and NGP's sweep, or synthetic in
//! tests). Only the spec's own constants are `const` here: they come from
//! papers or are ours.
//!
//! Sign conventions (checked against NGP's `slip.rs` / `patch_forces.rs` so
//! the forces oppose sliding exactly as NGP's do: a positive contact-point
//! lateral velocity gives a negative lateral force, and driving — a positive
//! slip velocity `Vsx = Vx + wR` — gives a negative longitudinal force): with
//! forward motion the contact-point longitudinal velocity `vx` is negative,
//! free rolling spin is `spin = -vx / R`, and the theoretical slips take the
//! sign of the sliding velocities, `sx = Vsx / Vr`, `sy = Vsy / Vr`. (An
//! earlier draft had a minus sign here; that would make the brush push the
//! sliding contact forward instead of opposing it, contradicting NGP.)
//!
//! Below 2 m/s the model is out of scope for v0: there is no sticking
//! (bristle/LuGre) state, so creep and standstill are not represented. The
//! plan is to keep NGP's sticking model or add a bristle state later; the
//! comparison only runs at V >= 2 m/s.

// ---------------------------------------------------------------------------
// Model constants (spec; from papers or ours — never NGP's).
// ---------------------------------------------------------------------------

// The two published Magic Formula sets used below for the longitudinal axis:
// the MF 6.1 example set (255/55R20, Fz0 6752 N, 260 kPa) shipped with
// MFeval.jl, read in full, and the passenger set Chrono's TMeasy
// `GuessPassCar70Par` normalises "from Pacejka book" data (provenance
// secondary). Means of the two are used; the note gives both.

/// Load exponent of the cornering stiffness, Cy ~ Fz^M_CY: 0.6 from the load
/// sweep of Singh & Sivaramakrishnan (Goodyear, arXiv 2305.18422); the two
/// sets give 0.47 and 0.69.
pub const M_CY: f64 = 0.6;
/// Load exponent of the longitudinal slip stiffness, Cx ~ Fz^M_CX: mean of the
/// sets' 1.31 and 1.08 (brush theory, C = 2 c_p a^2 with a^2 ~ Fz, gives 1).
pub const M_CX: f64 = 1.2;
/// Longitudinal over cornering stiffness at the nominal load: mean of the
/// sets' 1.62 and 1.15.
pub const KX_OVER_KY: f64 = 1.4;
/// Load exponent of the lateral peak force, Fy ~ Fz^N_MU_Y: ~0.87 from the
/// Goodyear load sweep.
pub const N_MU_Y: f64 = 0.87;
/// Load exponent of the longitudinal peak force: the sets' longitudinal
/// friction falls with load at 0.72 and 0.39 of the lateral rate (mean 0.55),
/// so 1 - 0.55 (1 - N_MU_Y) = 0.93.
pub const N_MU_X: f64 = 0.93;
/// Stribeck velocity of the friction curve g(Vs) = mu_C + (mu_S - mu_C)
/// exp(-sqrt(Vs / V_S)), m/s, and its Coulomb over static ratio: Canudas de
/// Wit, Tsiotras, Velenis, Basset & Gissinger, Vehicle System Dynamics 39
/// (2003) 189-226, eqs. 21-22 and Table 1 (mu_C 0.8, mu_S 1.55, v_s 6.57 m/s),
/// their distributed LuGre (brush) model fitted to straight-line braking on a
/// dry test track. The level (mu_S) is calibrated; shape and ratio are theirs.
pub const V_S: f64 = 6.57;
pub const MU_C_OVER_S: f64 = 0.8 / 1.55;
/// Floor of the rolling speed, m/s (keeps the theoretical slips finite).
pub const V_MIN: f64 = 0.5;
/// Relaxation speed coefficient K_V, s (Vera et al., Sensors 22:6380, 2022:
/// sigma_y = -0.14 + 0.021 Vx + 1.9e-4 Fz - 1.6e-8 Fz^2 on a car tyre bench).
/// Motorcycle tyres also relax over longer distances at speed (Cossalter, Lot
/// & Massaro, Motorcycle Dynamics, ch. 1); MF-Swift has no speed term (TNO,
/// 2013). Keeping the slope for a rally tyre is ours.
pub const K_V: f64 = 0.021;
/// Water film at W = 1: h = W times 0.7 mm. Measured films on drained
/// highways: 0.18 mm at 1.5 mm/h rain, 0.56-0.85 mm at 7.8-10.2 mm/h (PLOS
/// ONE 2025, Table 2); ASTM E274 wets to ~0.5 mm (secondary).
pub const FILM_MAX: f64 = 0.7e-3;
/// Reference film in the loss blend, 0.5 mm (ours).
pub const FILM_REF: f64 = 0.5e-3;
/// Wet friction kept at full hydroplaning (ours).
pub const WET_KEPT: f64 = 0.4;

// ---------------------------------------------------------------------------
// Run-time calibration (probed / solved values, never committed).
// ---------------------------------------------------------------------------

/// Everything the model needs from the nominal operating point: the fitted
/// stiffnesses, the solved friction levels, the operating conditions every
/// factor is normalised to, and NGP's pressure (StfFct) table as dumped by
/// `--tyre-probe`. Built at run time (see `crate::calibrate`); tests use
/// synthetic round numbers.
#[derive(Clone, Copy, Debug)]
pub struct Calibration {
    /// Normal load of the nominal point, N.
    pub fz0: f64,
    /// Cornering / traction stiffness at the nominal point, N per unit
    /// theoretical slip (fitted, not table values).
    pub cy0: f64,
    pub cx0: f64,
    /// Lateral friction at zero and infinite sliding speed.
    pub mu_ys: f64,
    pub mu_yk: f64,
    /// Longitudinal friction at zero and infinite sliding speed.
    pub mu_xs: f64,
    pub mu_xk: f64,
    /// Bulk tread temperature (K), tread remaining (1 = new), inflation
    /// pressure (Pa), wetness and NGP temperature grip of the nominal dump.
    pub t_cal: f64,
    pub d_cal: f64,
    pub p_cal: f64,
    pub pf_cal: f64,
    pub w_cal: f64,
    /// Surface static/kinetic mu and NGP temperature grip the inputs are
    /// normalised to (every factor is 1 at the nominal point).
    pub surf_s_cal: f64,
    pub surf_k_cal: f64,
    pub temp_grip_cal: f64,
    /// NGP's pressure factor table: factor over pressure, same lerp as NGP.
    pub stiff_axis: [f64; 4],
    pub stiff_val: [f64; 4],
}

/// Bulk-temperature factor on stiffness: f_T = clamp(1 - 0.0045 (Tb -
/// 293.15), 0.6, 1.2). Singh & Sivaramakrishnan, Table 3: cornering stiffness
/// drops 20-25% from cold to hot, governed by the bulk temperature.
pub fn temp_stiffness_factor(tb: f64) -> f64 {
    (1.0 - 0.0045 * (tb - 293.15)).clamp(0.6, 1.2)
}

/// Tread-remaining factor on stiffness: f_w = 2.6 / (d + 1.6). Same paper,
/// Table 3: 60% less tread depth gives +30% cornering stiffness
/// (2.6/2.0 = 1.3 at d = 0.4, normalised to 1 when new).
pub fn wear_stiffness_factor(d: f64) -> f64 {
    2.6 / (d + 1.6)
}

/// NGP's pressure factor: linear interpolation on the StfFct table, clamped
/// ends, as NGP lerps it.
pub fn pressure_factor(p: f64, cal: &Calibration) -> f64 {
    let (ax, val) = (&cal.stiff_axis, &cal.stiff_val);
    if p <= ax[0] {
        return val[0];
    }
    if p >= ax[ax.len() - 1] {
        return val[val.len() - 1];
    }
    for k in 0..ax.len() - 1 {
        if p <= ax[k + 1] {
            let f = (p - ax[k]) / (ax[k + 1] - ax[k]);
            return (1.0 - f) * val[k] + val[k + 1] * f;
        }
    }
    val[val.len() - 1]
}

/// Stiffness at load `fz`: C0 (Fz/Fz0)^m (`M_CY` or `M_CX`), times the
/// bulk-temperature, wear and pressure factors normalised to the calibration
/// point.
pub fn stiffness_at(fz: f64, tb: f64, d: f64, p: f64, c0: f64, m: f64, cal: &Calibration) -> f64 {
    c0 * (fz / cal.fz0).powf(m)
        * (temp_stiffness_factor(tb) / temp_stiffness_factor(cal.t_cal))
        * (wear_stiffness_factor(d) / wear_stiffness_factor(cal.d_cal))
        * (pressure_factor(p, cal) / cal.pf_cal)
}

/// Hydroplaning speed: vp = 6.36 sqrt(p in kPa) km/h (Horne's deep-water
/// law, NASA TN D-2056; metric form in Zhang et al., Lubricants 13:76, 2025),
/// times (0.75 + 0.25 d) for tread depth: ADAC's 2026 worn-tyre test lost
/// 17-22 % of the longitudinal aquaplaning speed at 2 of ~8 mm (d = 0.25).
pub fn hydroplaning_speed(p: f64, d: f64) -> f64 {
    6.36 * (p / 1000.0).sqrt() * (0.75 + 0.25 * d) / 3.6
}

/// Wet loss factor on friction, applied on top of the wet surface mu: the
/// loss is L = h/(h + 0.5 mm) times min(1, (V/vp)^2) with the film h = W times
/// 0.7 mm, and friction is multiplied by (1 - 0.6 L). Film and vp are
/// sourced (above); the ramp, its 0.5 mm, the quadratic shape and the 0.6
/// weight are ours.
pub fn wet_loss(w: f64, v: f64, p: f64, d: f64) -> f64 {
    if w <= 0.0 {
        return 1.0;
    }
    let h = w * FILM_MAX;
    let vp = hydroplaning_speed(p, d);
    let l = h / (h + FILM_REF) * (v / vp).powi(2).min(1.0);
    1.0 - (1.0 - WET_KEPT) * l
}

/// Dry-wear gain on friction: 1 + 0.167 (1 - d), fading with wetness (times
/// (1 - W)): worn tread grips more when dry (+10% peak mu at 60% worn, Singh
/// Table 3) and loses in the wet (carried by the hydroplaning speed there).
pub fn wear_gain(d: f64, w: f64) -> f64 {
    1.0 + 0.167 * (1.0 - d) * (1.0 - w)
}

/// Friction magnitude of one axis at sliding speed `vs`: mu_k (scaled by the
/// kinetic ratio) + (mu_s scaled by the static ratio - mu_k scaled by the
/// kinetic ratio) exp(-sqrt(Vs/V_S)), times the load sensitivity
/// (Fz/Fz0)^(n-1) (`N_MU_Y` or `N_MU_X`), NGP's temperature grip factor, the
/// wet-faded dry-wear gain (normalised) and the wet loss. Both axes scale
/// mu_s with the surface static ratio and mu_k with the surface kinetic
/// ratio, since the static scale sets the adhesion ceiling and the kinetic
/// scale the sliding floor.
#[allow(clippy::too_many_arguments)]
pub fn mu_axis(
    mu_s: f64,
    mu_k: f64,
    vs: f64,
    fz: f64,
    n: f64,
    stat_ratio: f64,
    kin_ratio: f64,
    temp_grip: f64,
    d: f64,
    w: f64,
    wet: f64,
    cal: &Calibration,
) -> f64 {
    let ks = mu_s * stat_ratio;
    let kk = mu_k * kin_ratio;
    let base = kk + (ks - kk) * (-(vs / V_S).sqrt()).exp();
    base * (fz / cal.fz0).powf(n - 1.0)
        * (temp_grip / cal.temp_grip_cal)
        * (wear_gain(d, w) / wear_gain(cal.d_cal, cal.w_cal))
        * wet
}

/// What one steady-state evaluation reads.
#[derive(Clone, Copy, Debug)]
pub struct EvalInput {
    /// Normal load, N.
    pub fz: f64,
    /// Contact half-length a, m (half of NGP's patch length vs Fz).
    pub half_length: f64,
    /// Contact-point velocities, m/s (NGP sign: forward motion is negative
    /// longitudinal).
    pub vx: f64,
    pub vy: f64,
    /// Wheel spin, rad/s, and effective radius, m.
    pub spin: f64,
    pub radius: f64,
    /// Bulk tread temperature, K; tread remaining 1 = new; pressure, Pa;
    /// surface wetness 0..1 (NGP WaterFactor).
    pub bulk_temp: f64,
    pub tread_remaining: f64,
    pub pressure: f64,
    pub wetness: f64,
    /// NGP's temperature grip factor (RubMyFct blend, kept as in NGP).
    pub temp_grip: f64,
    /// Surface static/kinetic mu (NGP stage values, e.g. dry 1.30/1.04).
    pub surf_static: f64,
    pub surf_kinetic: f64,
}

/// What one steady-state evaluation gives: forces opposing sliding in NGP's
/// sign convention, the aligning moment, and intermediates.
#[derive(Clone, Copy, Debug)]
pub struct EvalOutput {
    /// Longitudinal / lateral force, N (oppose the sliding velocity).
    pub fx: f64,
    pub fy: f64,
    /// Aligning moment, N m.
    pub mz: f64,
    /// Pneumatic trail -Mz/Fy, m (0 when Fy is 0).
    pub trail: f64,
    /// Combined-slip magnitude psi, the directional friction mu, and the
    /// theoretical slips.
    pub psi: f64,
    pub mu: f64,
    pub sx: f64,
    pub sy: f64,
}

/// Steady-state brush evaluation: Pacejka's theoretical brush model with
/// combined slip (Tire and Vehicle Dynamics, 3rd ed., ch. 3): theta_i =
/// C_i/(3 mu Fz) with the directional mu below, psi = sqrt((theta_x sx)^2 +
/// (theta_y sy)^2); for psi < 1, F = mu Fz (3 psi - 3 psi^2 + psi^3), else
/// F = mu Fz; F_i = -F theta_i s_i/psi (0 at psi = 0). The friction ellipse
/// gives the directional mu: mu_dir = 1/sqrt((ex/mu_x)^2 + (ey/mu_y)^2) along
/// the slip direction e. Aligning moment (pure side-slip form, applied with
/// psi): Mz = mu Fz a theta_y sy (1 - 3 psi + 3 psi^2 - psi^3) for psi < 1,
/// else 0; the trail -Mz/Fy tends to a/3 at small slip.
pub fn eval(x: &EvalInput, cal: &Calibration) -> EvalOutput {
    let cy = stiffness_at(x.fz, x.bulk_temp, x.tread_remaining, x.pressure, cal.cy0, M_CY, cal);
    let cx = stiffness_at(x.fz, x.bulk_temp, x.tread_remaining, x.pressure, cal.cx0, M_CX, cal);
    // Slip velocities (NGP slip.rs long_slip_velocity sign) and rolling speed.
    let vsx = x.vx + x.spin * x.radius;
    let vsy = x.vy;
    let vr = (x.spin * x.radius).abs().max(V_MIN);
    let sx = vsx / vr;
    let sy = vsy / vr;
    let vs = (vsx * vsx + vsy * vsy).sqrt();
    let v = (x.vx * x.vx + x.vy * x.vy).sqrt();
    let wet = wet_loss(x.wetness, v, x.pressure, x.tread_remaining);
    let mu_x = mu_axis(
        cal.mu_xs,
        cal.mu_xk,
        vs,
        x.fz,
        N_MU_X,
        x.surf_static / cal.surf_s_cal,
        x.surf_kinetic / cal.surf_k_cal,
        x.temp_grip,
        x.tread_remaining,
        x.wetness,
        wet,
        cal,
    );
    let mu_y = mu_axis(
        cal.mu_ys,
        cal.mu_yk,
        vs,
        x.fz,
        N_MU_Y,
        x.surf_static / cal.surf_s_cal,
        x.surf_kinetic / cal.surf_k_cal,
        x.temp_grip,
        x.tread_remaining,
        x.wetness,
        wet,
        cal,
    );
    // Friction ellipse along the theoretical-slip direction.
    let s_abs = (sx * sx + sy * sy).sqrt();
    let mu = if s_abs > 0.0 {
        let ex = sx / s_abs;
        let ey = sy / s_abs;
        1.0 / ((ex / mu_x).powi(2) + (ey / mu_y).powi(2)).sqrt()
    } else {
        mu_y
    };
    let theta_x = cx / (3.0 * mu * x.fz);
    let theta_y = cy / (3.0 * mu * x.fz);
    let psi = ((theta_x * sx).powi(2) + (theta_y * sy).powi(2)).sqrt();
    let (f, mz) = if psi < 1.0 && psi > 0.0 {
        let f = mu * x.fz * (3.0 * psi - 3.0 * psi * psi + psi * psi * psi);
        let mz = mu * x.fz * x.half_length * theta_y * sy * (1.0 - 3.0 * psi + 3.0 * psi * psi - psi * psi * psi);
        (f, mz)
    } else if psi >= 1.0 {
        (mu * x.fz, 0.0)
    } else {
        (0.0, 0.0)
    };
    let (fx, fy) = if psi > 0.0 { (-f * theta_x * sx / psi, -f * theta_y * sy / psi) } else { (0.0, 0.0) };
    let trail = if fy != 0.0 { -mz / fy } else { 0.0 };
    EvalOutput { fx, fy, mz, trail, psi, mu, sx, sy }
}

/// Relaxation lengths, m: sigma_y = Cy(Fz)/k_y + K_V |Vx| with NGP's carcass
/// lateral stiffness k_y (times the pressure factor); sigma_x = Cx(Fz)/k_x +
/// K_V |Vx| with k_x = NGP's torsional stiffness / R^2 (the torsion torque
/// -torsion * k_tors acts as -torsion/R * k_tors/R longitudinally). K_V from
/// Vera et al. (passenger tyre; transfer assumed).
pub fn sigma_lengths(cy_fz: f64, cx_fz: f64, k_y: f64, k_x: f64, vx: f64) -> (f64, f64) {
    (cy_fz / k_y + K_V * vx.abs(), cx_fz / k_x + K_V * vx.abs())
}

/// Single-contact-point transient state (Pacejka, ch. 7): lateral and
/// longitudinal carcass deflections, m.
#[derive(Clone, Copy, Debug, Default)]
pub struct TransState {
    pub ux: f64,
    pub uy: f64,
}

/// One transient step at dt = 1/720 s: dv/dt = Vs - (Vr/sigma) v per axis
/// (deflection grows with the sliding velocity and relaxes exponentially),
/// integrated with the exact exponential for the linear decay over dt, which
/// is stable for any sigma. The transient slips v/sigma replace sx/sy in
/// [`eval`].
pub fn step_transient(s: &mut TransState, vsx: f64, vsy: f64, vr: f64, sigma_x: f64, sigma_y: f64, dt: f64) {
    let decay_x = (-dt * vr / sigma_x).exp();
    let decay_y = (-dt * vr / sigma_y).exp();
    s.ux += (vsx * sigma_x / vr - s.ux) * (1.0 - decay_x);
    s.uy += (vsy * sigma_y / vr - s.uy) * (1.0 - decay_y);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic calibration with round numbers (not NGP's values).
    fn synth_cal() -> Calibration {
        Calibration {
            fz0: 4000.0,
            cy0: 80000.0,
            cx0: 60000.0,
            mu_ys: 1.3,
            mu_yk: 1.0,
            mu_xs: 1.3,
            mu_xk: 1.0,
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

    fn synth_input() -> EvalInput {
        EvalInput {
            fz: 4000.0,
            half_length: 0.07,
            vx: -15.0,
            vy: 0.0,
            spin: 15.0 / 0.3,
            radius: 0.3,
            bulk_temp: 300.0,
            tread_remaining: 1.0,
            pressure: 200000.0,
            wetness: 0.0,
            temp_grip: 1.0,
            surf_static: 1.3,
            surf_kinetic: 1.0,
        }
    }

    #[test]
    fn zero_slip_gives_zero_force_and_moment() {
        let cal = synth_cal();
        let o = eval(&synth_input(), &cal);
        assert!(o.fx.abs() < 1e-9, "fx {}", o.fx);
        assert!(o.fy.abs() < 1e-9, "fy {}", o.fy);
        assert_eq!(o.mz, 0.0);
        assert_eq!(o.psi, 0.0);
    }

    #[test]
    fn small_slip_slope_equals_stiffness() {
        let cal = synth_cal();
        // Lateral: tiny slip angle, slope d|Fy|/dsy -> cy0.
        let mut x = synth_input();
        x.vy = 15.0 * 1e-7;
        x.vx = -(15.0f64 * 15.0f64 - x.vy * x.vy).sqrt();
        x.spin = -x.vx / x.radius;
        let o = eval(&x, &cal);
        let slope = o.fy.abs() / o.sy.abs();
        assert!((slope / cal.cy0 - 1.0).abs() < 1e-6, "slope {slope} cy0 {}", cal.cy0);
        // Longitudinal: tiny kappa, slope d|Fx|/dsx -> cx0.
        let mut x = synth_input();
        let kappa = 1e-7;
        x.spin = (1.0 + kappa) * 15.0 / x.radius;
        let o = eval(&x, &cal);
        let slope = o.fx.abs() / o.sx.abs();
        assert!((slope / cal.cx0 - 1.0).abs() < 1e-6, "slope {slope} cx0 {}", cal.cx0);
    }

    #[test]
    fn full_sliding_gives_mu_fz_along_slip() {
        let cal = synth_cal();
        // Near-90 deg slip: saturated, force opposes sliding at mu(Vs) Fz.
        let mut x = synth_input();
        let alpha = 80.0f64.to_radians();
        x.vy = 15.0 * alpha.sin();
        x.vx = -15.0 * alpha.cos();
        x.spin = -x.vx / x.radius;
        let o = eval(&x, &cal);
        assert!(o.psi > 1.0, "psi {}", o.psi);
        let f = (o.fx * o.fx + o.fy * o.fy).sqrt();
        let vs = (x.vy * x.vy + (x.vx + x.spin * x.radius).powi(2)).sqrt();
        let mu = cal.mu_yk + (cal.mu_ys - cal.mu_yk) * (-(vs / V_S).sqrt()).exp();
        assert!((f / (mu * x.fz) - 1.0).abs() < 1e-9, "f {f} mu Fz {}", mu * x.fz);
        // Direction: opposes the sliding velocity.
        assert!(o.fy < 0.0 && o.fx <= 0.0, "fx {} fy {}", o.fx, o.fy);
    }

    #[test]
    fn trail_tends_to_a_over_3_at_small_slip() {
        let cal = synth_cal();
        let mut x = synth_input();
        x.vy = 15.0 * (0.01f64.to_radians()).sin();
        x.vx = -(15.0f64 * 15.0f64 - x.vy * x.vy).sqrt();
        x.spin = -x.vx / x.radius;
        let o = eval(&x, &cal);
        assert!((o.trail / (x.half_length / 3.0) - 1.0).abs() < 0.005, "trail {}", o.trail);
    }

    #[test]
    fn lateral_symmetry() {
        let cal = synth_cal();
        let mut p = synth_input();
        p.vy = 2.0;
        p.spin = -p.vx / p.radius;
        let mut m = synth_input();
        m.vy = -2.0;
        m.spin = -m.vx / m.radius;
        let (op, om) = (eval(&p, &cal), eval(&m, &cal));
        assert!((op.fy + om.fy).abs() < 1e-9, "{} {}", op.fy, om.fy);
        assert!((op.mz + om.mz).abs() < 1e-9, "{} {}", op.mz, om.mz);
        assert!((op.fx - om.fx).abs() < 1e-9, "{} {}", op.fx, om.fx);
    }

    #[test]
    fn static_scales_ceiling_and_kinetic_scales_floor() {
        // mu_s follows the static ratio, mu_k the kinetic ratio, on each axis:
        // doubling the static surface mu doubles the zero-speed mu, doubling
        // the kinetic one doubles the infinite-speed mu.
        let cal = synth_cal();
        let slow = mu_axis(1.3, 1.0, 0.0, 4000.0, N_MU_Y, 2.0, 1.0, 1.0, 1.0, 0.0, 1.0, &cal);
        assert!((slow / 2.6 - 1.0).abs() < 1e-12, "slow {slow}");
        let fast = mu_axis(1.3, 1.0, 1e9, 4000.0, N_MU_Y, 1.0, 2.0, 1.0, 1.0, 0.0, 1.0, &cal);
        assert!((fast / 2.0 - 1.0).abs() < 1e-12, "fast {fast}");
    }

    #[test]
    fn wear_gain_fades_with_wetness() {
        assert!((wear_gain(1.0, 0.0) - 1.0).abs() < 1e-12);
        assert!((wear_gain(0.4, 0.0) - 1.1002).abs() < 1e-12);
        assert!((wear_gain(0.4, 1.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn transient_converges_and_reaches_63_percent_after_sigma() {
        let cal = synth_cal();
        // 1 deg step at 15 m/s: linear regime, distance to 63% ~= sigma_y.
        let v = 15.0;
        let alpha = 1.0f64.to_radians();
        let vsy = v * alpha.sin();
        let vx = -v * alpha.cos();
        let vr = v * alpha.cos();
        let cy = stiffness_at(4000.0, 300.0, 1.0, 200000.0, cal.cy0, M_CY, &cal);
        let k_y = 400000.0;
        let (sig_y, _) = sigma_lengths(cy, cy, k_y, 1.0, vx);
        let dt = 1.0 / 720.0;
        let mut s = TransState::default();
        // Integrate until the transient slip reaches 63% of steady slip.
        let mut dist = 0.0;
        let mut ratio = 0.0;
        for _ in 0..7200 {
            step_transient(&mut s, 0.0, vsy, vr, 1.0, sig_y, dt);
            dist += v * dt;
            ratio = (s.uy / sig_y) / (vsy / vr);
            if ratio >= 0.63 {
                break;
            }
        }
        assert!(ratio >= 0.63, "never reached 63%");
        assert!((dist / sig_y - 1.0).abs() < 0.1, "dist {dist} sigma {sig_y}");
        // Converges to the steady-state slip.
        for _ in 0..72000 {
            step_transient(&mut s, 0.0, vsy, vr, 1.0, sig_y, dt);
        }
        assert!(((s.uy / sig_y) / (vsy / vr) - 1.0).abs() < 1e-6);
    }
}
