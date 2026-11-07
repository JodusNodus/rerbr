//! Candidate tyre model "brush-v0" (ours, experimental).
//!
//! A steady-state brush model with Pacejka's theoretical slips, a Grosch-like
//! sliding-speed friction drop, load-sensitive stiffness and friction, bulk
//! temperature / wear / pressure factors, a hydroplaning-style wet loss, and a
//! single-contact-point transient. Pure functions over plain values (`f64` is
//! fine here: nothing in the lab needs bit-exactness).
//!
//! Lab code only: nothing here is wired into the game, and nothing in
//! `rbr-physics` / `rbr-hybrid` / `rbr-standalone` changes.
//!
//! Sign conventions (checked against NGP's `slip.rs` / `patch_forces.rs` so
//! the forces oppose sliding exactly as NGP's do: a positive contact-point
//! lateral velocity gives a negative lateral force, and driving — a positive
//! slip velocity `Vx + wR` — gives a negative longitudinal force): with forward
//! motion the contact-point longitudinal velocity `vx` is negative, free
//! rolling spin is `spin = -vx / R`, and the theoretical slips take the sign
//! of the sliding velocities, `sx = Vsx / Vr`, `sy = Vsy / Vr`. (An earlier
//! draft had a minus sign here; that would make the brush push the sliding
//! contact forward instead of opposing it, contradicting NGP.)
//!
//! Below 2 m/s the model is out of scope for v0: there is no sticking
//! (bristle/LuGre) state, so creep and standstill are not represented. The
//! plan is to keep NGP's sticking model or add a bristle state later; the
//! comparison only runs at V >= 2 m/s.

// ---------------------------------------------------------------------------
// Calibration (nominal point): Ford Fiesta Rally2, dry tarmac, V = 15 m/s.
// ---------------------------------------------------------------------------

/// Normal load of the nominal point, N.
pub const FZ0: f64 = 4000.0;
/// Cornering stiffness at the nominal point, N per unit theoretical slip,
/// fitted so the brush's least-squares slope over slip angles <= 2 deg equals
/// NGP's swept slope (70412 N/rad at Fz0, V = 15 m/s, probe pressure /
/// temperature / wear). Numerical solve; see the calibration test.
pub const CY0: f64 = 81048.3276;
/// Traction stiffness at the nominal point, fitted the same way over
/// |kappa| <= 0.04 to NGP's 56228 N per unit slip ratio.
pub const CX0: f64 = 63791.4254;
/// Lateral friction at zero and infinite sliding speed: solved so the
/// candidate's peak mu_y at Fz0, V = 15 m/s equals NGP's sweep (1.265) and
/// mu(30 deg slip) / mu_peak = 0.80. Ratio mu_s / mu_k = 1.58: the surface
/// ratio (1.25) could not give a 0.80 drop with V_MU = 5 m/s, so the
/// calibration needs otherwise (documented, not tuned to pass).
pub const MU_Y_S: f64 = 1.3585940;
pub const MU_Y_K: f64 = 0.8575711;
/// Longitudinal friction, solved the same way to NGP's peak mu_x (1.295) and
/// F(kappa = 0.5) / peak = 0.80 (Vs = 7.5 m/s, the same sliding speed as a
/// 30 deg slip angle at 15 m/s). Ratio mu_s / mu_k = 1.90.
pub const MU_X_S: f64 = 1.5418187;
pub const MU_X_K: f64 = 0.8112668;
/// Bulk tread temperature of the nominal dump, K (probe `temps[0]`).
pub const T_CAL: f64 = 337.302;
/// Tread remaining of the nominal dump (1 - wear 0.000878).
pub const D_CAL: f64 = 0.999122;
/// Inflation pressure of the nominal dump, Pa, and NGP's StfFct factor there.
pub const P_CAL: f64 = 200000.0;
pub const PF_CAL: f64 = 0.966667;
/// NGP stage values at the nominal point (dry tarmac tag 37, from the DLL):
/// the inputs are normalised to these, so every factor is 1 at calibration.
pub const SURF_S_CAL: f64 = 1.30;
pub const SURF_K_CAL: f64 = 1.04;
/// NGP temperature grip (RubMyFct blend) at the nominal dump.
pub const TEMP_GRIP_CAL: f64 = 0.998834;

// ---------------------------------------------------------------------------
// Model constants (spec; assumptions flagged).
// ---------------------------------------------------------------------------

/// Load exponent of stiffness: C ~ Fz^M_C. Target 0.5-0.65 from the load
/// sweep of Singh & Sivaramakrishnan (Goodyear, arXiv 2305.18422).
pub const M_C: f64 = 0.6;
/// Load exponent of peak force: peak F ~ Fz^N_MU. Lateral target ~0.87 from
/// the same paper's load sweep; the longitudinal value is an assumption
/// (flagged: no separate source).
pub const N_MU: f64 = 0.87;
/// Sliding-speed scale of the Grosch-like drop, m/s. Direction (falling with
/// sliding speed) follows Grosch/Persson; the value and the Lorentzian form
/// are our assumption.
pub const V_MU: f64 = 5.0;
/// Floor of the rolling speed, m/s (keeps the theoretical slips finite).
pub const V_MIN: f64 = 0.5;
/// Relaxation speed coefficient K_V, s (Vera et al., Sensors 22:6380, 2022:
/// sigma_y = -0.14 + 0.021 Vx + 1.9e-4 Fz - 1.6e-8 Fz^2 for a passenger tyre;
/// transferring the Vx slope to a rally tyre is an assumption).
pub const K_V: f64 = 0.021;
/// Water film scale: film height h = W * 1.0 mm. Well-established direction
/// (deeper water / lower speed margin hurts); the 1.0 mm scale is ours.
pub const FILM_MAX: f64 = 1e-3;
/// Reference film in the loss blend, 0.5 mm (ours).
pub const FILM_REF: f64 = 0.5e-3;
/// Wet friction kept at full hydroplaning (ours).
pub const WET_KEPT: f64 = 0.4;
/// NGP's pressure (StfFct) table as dumped by `--tyre-probe` for this car:
/// factor over pressure, same lerp as NGP.
pub const STIFF_AXIS: [f64; 4] = [0.0, 150000.0, 300000.0, 500000.0];
pub const STIFF_VAL: [f64; 4] = [0.5, 0.9, 1.1, 1.2];

/// Bulk-temperature factor on stiffness: f_T = clamp(1 - 0.0045 (Tb -
/// 293.15), 0.6, 1.2). Same paper, Table 3: cornering stiffness drops 20-25%
/// from cold to hot, governed by the bulk temperature.
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
pub fn pressure_factor(p: f64) -> f64 {
    if p <= STIFF_AXIS[0] {
        return STIFF_VAL[0];
    }
    if p >= STIFF_AXIS[STIFF_AXIS.len() - 1] {
        return STIFF_VAL[STIFF_VAL.len() - 1];
    }
    for k in 0..STIFF_AXIS.len() - 1 {
        if p <= STIFF_AXIS[k + 1] {
            let f = (p - STIFF_AXIS[k]) / (STIFF_AXIS[k + 1] - STIFF_AXIS[k]);
            return (1.0 - f) * STIFF_VAL[k] + STIFF_VAL[k + 1] * f;
        }
    }
    STIFF_VAL[STIFF_VAL.len() - 1]
}

/// Stiffness at load `fz`: C0 (Fz/Fz0)^M_C, times the bulk-temperature, wear
/// and pressure factors normalised to the calibration point.
pub fn stiffness_at(fz: f64, tb: f64, d: f64, p: f64, c0: f64) -> f64 {
    c0 * (fz / FZ0).powf(M_C)
        * (temp_stiffness_factor(tb) / temp_stiffness_factor(T_CAL))
        * (wear_stiffness_factor(d) / wear_stiffness_factor(D_CAL))
        * (pressure_factor(p) / PF_CAL)
}

/// Hydroplaning speed: vp = 6.36 sqrt(p in kPa) km/h (NASA/Horne, reproduced
/// in Zhang et al., Lubricants 13:76, 2025), times (0.6 + 0.4 d) for tread
/// depth. The tread term is ours.
pub fn hydroplaning_speed(p: f64, d: f64) -> f64 {
    6.36 * (p / 1000.0).sqrt() * (0.6 + 0.4 * d) / 3.6
}

/// Wet loss factor on friction (our construction except the speed formula):
/// the film height is h = W times 1.0 mm, the loss is L = h/(h + 0.5 mm) times
/// min(1, (V/vp)^2), and friction is multiplied by (1 - 0.6 L), applied on top
/// of the wet surface mu. The direction (deeper water and lower speed margin
/// hurt) is well established; the film scale, the reference film and the 0.6
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

/// Friction magnitude of one axis at sliding speed `vs`: mu_k + (mu_s -
/// mu_k) / (1 + (Vs/V_MU)^2), times the load sensitivity (Fz/Fz0)^(N_MU-1),
/// the surface ratio, NGP's temperature grip factor, the dry-wear term
/// (1 + 0.167 (1 - d), normalised: +10% peak mu at 60% worn, same paper) and
/// the wet loss.
#[allow(clippy::too_many_arguments)]
pub fn mu_axis(mu_s: f64, mu_k: f64, vs: f64, fz: f64, surf_ratio: f64, temp_grip: f64, d: f64, wet: f64) -> f64 {
    let base = mu_k + (mu_s - mu_k) / (1.0 + (vs / V_MU).powi(2));
    base * (fz / FZ0).powf(N_MU - 1.0)
        * surf_ratio
        * (temp_grip / TEMP_GRIP_CAL)
        * ((1.0 + 0.167 * (1.0 - d)) / (1.0 + 0.167 * (1.0 - D_CAL)))
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
/// F = mu Fz; F_i = -F theta_i s_i / psi (0 at psi = 0). The friction ellipse
/// gives the directional mu: mu_dir = 1/sqrt((ex/mu_x)^2 + (ey/mu_y)^2) along
/// the slip direction e. Aligning moment (pure side-slip form, applied with
/// psi): Mz = mu Fz a theta_y sy (1 - 3 psi + 3 psi^2 - psi^3) for psi < 1,
/// else 0; the trail -Mz/Fy tends to a/3 at small slip.
pub fn eval(x: &EvalInput) -> EvalOutput {
    let cy = stiffness_at(x.fz, x.bulk_temp, x.tread_remaining, x.pressure, CY0);
    let cx = stiffness_at(x.fz, x.bulk_temp, x.tread_remaining, x.pressure, CX0);
    // Slip velocities (NGP slip.rs long_slip_velocity sign) and rolling speed.
    let vsx = x.vx + x.spin * x.radius;
    let vsy = x.vy;
    let vr = (x.spin * x.radius).abs().max(V_MIN);
    let sx = vsx / vr;
    let sy = vsy / vr;
    let vs = (vsx * vsx + vsy * vsy).sqrt();
    let v = (x.vx * x.vx + x.vy * x.vy).sqrt();
    let wet = wet_loss(x.wetness, v, x.pressure, x.tread_remaining);
    let mu_x = mu_axis(MU_X_S, MU_X_K, vs, x.fz, x.surf_kinetic / SURF_K_CAL, x.temp_grip, x.tread_remaining, wet);
    let mu_y = mu_axis(MU_Y_S, MU_Y_K, vs, x.fz, x.surf_static / SURF_S_CAL, x.temp_grip, x.tread_remaining, wet);
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

    fn cal_input() -> EvalInput {
        EvalInput {
            fz: FZ0,
            half_length: 0.14531 / 2.0,
            vx: -15.0,
            vy: 0.0,
            spin: 15.0 / 0.315,
            radius: 0.315,
            bulk_temp: T_CAL,
            tread_remaining: D_CAL,
            pressure: P_CAL,
            wetness: 0.0,
            temp_grip: TEMP_GRIP_CAL,
            surf_static: SURF_S_CAL,
            surf_kinetic: SURF_K_CAL,
        }
    }

    #[test]
    fn zero_slip_gives_zero_force_and_moment() {
        let o = eval(&cal_input());
        assert!(o.fx.abs() < 1e-9, "fx {}", o.fx);
        assert!(o.fy.abs() < 1e-9, "fy {}", o.fy);
        assert_eq!(o.mz, 0.0);
        assert_eq!(o.psi, 0.0);
    }

    #[test]
    fn small_slip_slope_equals_stiffness() {
        // Lateral: tiny slip angle, slope d|Fy|/dsy -> CY0.
        let mut x = cal_input();
        x.vy = 15.0 * 1e-7;
        x.vx = -(15.0f64 * 15.0f64 - x.vy * x.vy).sqrt();
        x.spin = -x.vx / x.radius;
        let o = eval(&x);
        let slope = o.fy.abs() / o.sy.abs();
        assert!((slope / CY0 - 1.0).abs() < 1e-6, "slope {slope} CY0 {CY0}");
        // Longitudinal: tiny kappa, slope d|Fx|/dsx -> CX0.
        let mut x = cal_input();
        let kappa = 1e-7;
        x.spin = (1.0 + kappa) * 15.0 / x.radius;
        let o = eval(&x);
        let slope = o.fx.abs() / o.sx.abs();
        assert!((slope / CX0 - 1.0).abs() < 1e-6, "slope {slope} CX0 {CX0}");
    }

    #[test]
    fn full_sliding_gives_mu_fz_along_slip() {
        // Near-90 deg slip: saturated, force opposes sliding at mu(Vs) Fz.
        let mut x = cal_input();
        let alpha = 80.0f64.to_radians();
        x.vy = 15.0 * alpha.sin();
        x.vx = -15.0 * alpha.cos();
        x.spin = -x.vx / x.radius;
        let o = eval(&x);
        assert!(o.psi > 1.0, "psi {}", o.psi);
        let f = (o.fx * o.fx + o.fy * o.fy).sqrt();
        let vs = (x.vy * x.vy + (x.vx + x.spin * x.radius).powi(2)).sqrt();
        let mu = MU_Y_K + (MU_Y_S - MU_Y_K) / (1.0 + (vs / V_MU).powi(2));
        assert!((f / (mu * x.fz) - 1.0).abs() < 1e-9, "f {f} mu Fz {}", mu * x.fz);
        // Direction: opposes the sliding velocity.
        assert!(o.fy < 0.0 && o.fx <= 0.0, "fx {} fy {}", o.fx, o.fy);
    }

    #[test]
    fn trail_tends_to_a_over_3_at_small_slip() {
        let mut x = cal_input();
        x.vy = 15.0 * (0.01f64.to_radians()).sin();
        x.vx = -(15.0f64 * 15.0f64 - x.vy * x.vy).sqrt();
        x.spin = -x.vx / x.radius;
        let o = eval(&x);
        assert!((o.trail / (x.half_length / 3.0) - 1.0).abs() < 0.005, "trail {}", o.trail);
    }

    #[test]
    fn lateral_symmetry() {
        let mut p = cal_input();
        p.vy = 2.0;
        p.spin = -p.vx / p.radius;
        let mut m = cal_input();
        m.vy = -2.0;
        m.spin = -m.vx / m.radius;
        let (op, om) = (eval(&p), eval(&m));
        assert!((op.fy + om.fy).abs() < 1e-9, "{} {}", op.fy, om.fy);
        assert!((op.mz + om.mz).abs() < 1e-9, "{} {}", op.mz, om.mz);
        assert!((op.fx - om.fx).abs() < 1e-9, "{} {}", op.fx, om.fx);
    }

    #[test]
    fn calibration_reproduces_ngp_peak_at_fz0() {
        // Lateral sweep: peak |Fy| is NGP's 5058.66 N within 0.5%.
        let mut best = 0.0f64;
        for k in 0..=600 {
            let alpha = k as f64 * 0.05 * std::f64::consts::PI / 180.0;
            let mut x = cal_input();
            x.vy = 15.0 * alpha.sin();
            x.vx = -15.0 * alpha.cos();
            x.spin = -x.vx / x.radius;
            best = best.max(eval(&x).fy.abs());
        }
        assert!((best / 5058.66 - 1.0).abs() < 0.005, "peak {best}");
        // Longitudinal sweep, drive side as calibrated: peak |Fx| is NGP's
        // 5180.26 N within 0.5%. (The braking side peaks ~6% higher: sx =
        // Vsx/Vr with Vr = |wR| saturates earlier when braking. NGP is
        // drive/brake symmetric; see the comparison.)
        let mut best = 0.0f64;
        let mut k = 0.0;
        while k <= 1.0 {
            let mut x = cal_input();
            x.spin = (1.0 + k) * 15.0 / x.radius;
            best = best.max(eval(&x).fx.abs());
            k += 0.005;
        }
        assert!((best / 5180.26 - 1.0).abs() < 0.005, "peak {best}");
    }

    #[test]
    fn transient_converges_and_reaches_63_percent_after_sigma() {
        // 1 deg step at 15 m/s: linear regime, distance to 63% ~= sigma_y.
        let v = 15.0;
        let alpha = 1.0f64.to_radians();
        let vsy = v * alpha.sin();
        let vx = -v * alpha.cos();
        let vr = v * alpha.cos();
        let cy = stiffness_at(FZ0, T_CAL, D_CAL, P_CAL, CY0);
        let k_y = 418798.281250 * PF_CAL;
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
