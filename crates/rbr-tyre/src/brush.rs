//! Candidate tyre model "brush-v1" (ours, experimental).
//!
//! A steady-state brush with Pacejka's theoretical slips, the Stribeck
//! friction of Canudas de Wit et al.'s distributed LuGre (brush) tyre model,
//! load-sensitive stiffness and friction, bulk temperature / wear / pressure
//! factors, a hydroplaning-style wet loss, and Rill's first-order force
//! dynamics (carcass spring and damper, relaxation from the secant slip
//! stiffness). Pure functions over plain values, in `f64` with libm's `exp` and
//! `pow`: the same bits on every platform, as replays need. The game steps it
//! through [`crate::car`]; the lab (`rbr-tyre-lab`) compares it with NGP.
//!
//! No NGP-derived numbers live in this file: the nominal-point stiffnesses,
//! friction levels, temperatures, pressure factors and surface values all
//! arrive at run time in [`Calibration`] (solved by `crate::calibrate` from
//! NGP's sweep of the wheel's own tables, or synthetic in tests). Only the
//! spec's own constants are `const` here: they come from papers or are ours.
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
//! The force dynamics reach standstill (Rill's transport speed restored: a
//! stopped wheel holds its deflection, a locked one's follows the sliding),
//! but the game still hands the patch to NGP's sticking model below 2-3 m/s
//! ([`crate::car`]).

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
/// stiffnesses, the solved friction levels and the operating conditions every
/// factor is normalised to. Built at run time (see `crate::calibrate`); tests
/// use synthetic round numbers.
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
    /// Bulk tread temperature (K), tread remaining (1 = new), NGP's
    /// stiffness factor at the inflation pressure (StfFct) and wetness of the
    /// nominal point.
    pub t_cal: f64,
    pub d_cal: f64,
    pub pf_cal: f64,
    pub w_cal: f64,
    /// Surface static/kinetic mu and NGP temperature grip the inputs are
    /// normalised to (every factor is 1 at the nominal point).
    pub surf_s_cal: f64,
    pub surf_k_cal: f64,
    pub temp_grip_cal: f64,
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

/// Stiffness at load `fz`: C0 (Fz/Fz0)^m (`M_CY` or `M_CX`), times the
/// bulk-temperature and wear factors and NGP's pressure factor `pf` (StfFct),
/// each normalised to the calibration point.
pub fn stiffness_at(fz: f64, tb: f64, d: f64, pf: f64, c0: f64, m: f64, cal: &Calibration) -> f64 {
    c0 * libm::pow(fz / cal.fz0, m)
        * (temp_stiffness_factor(tb) / temp_stiffness_factor(cal.t_cal))
        * (wear_stiffness_factor(d) / wear_stiffness_factor(cal.d_cal))
        * (pf / cal.pf_cal)
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
    let l = h / (h + FILM_REF) * ((v / vp) * (v / vp)).min(1.0);
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
    let base = kk + (ks - kk) * libm::exp(-(vs / V_S).sqrt());
    base * libm::pow(fz / cal.fz0, n - 1.0)
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
    /// Bulk tread temperature, K; tread remaining 1 = new; pressure, Pa, and
    /// NGP's stiffness factor at it (StfFct); surface wetness 0..1 (NGP
    /// WaterFactor).
    pub bulk_temp: f64,
    pub tread_remaining: f64,
    pub pressure: f64,
    pub pressure_factor: f64,
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
    /// The secant slip stiffnesses -F_i/s_i, N per unit theoretical slip (the slip stiffnesses at zero slip, falling
    /// as the patch slides).
    pub kx: f64,
    pub ky: f64,
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
    let (vsx, vsy, vr) = slip_velocities(x);
    eval_slip(x, vsx / vr, vsy / vr, vr, cal)
}

/// The slip velocities `Vsx = Vx + wR` (NGP `slip.rs` long_slip_velocity
/// sign) and `Vsy = Vy`, and the rolling speed `Vr = |wR|`, at least `V_MIN`.
pub fn slip_velocities(x: &EvalInput) -> (f64, f64, f64) {
    (x.vx + x.spin * x.radius, x.vy, (x.spin * x.radius).abs().max(V_MIN))
}

/// [`eval`] at the theoretical slips `sx`, `sy` and the rolling speed `vr`:
/// the sliding speed is `vr |s|`.
pub fn eval_slip(x: &EvalInput, sx: f64, sy: f64, vr: f64, cal: &Calibration) -> EvalOutput {
    let cy = stiffness_at(x.fz, x.bulk_temp, x.tread_remaining, x.pressure_factor, cal.cy0, M_CY, cal);
    let cx = stiffness_at(x.fz, x.bulk_temp, x.tread_remaining, x.pressure_factor, cal.cx0, M_CX, cal);
    let s_abs = (sx * sx + sy * sy).sqrt();
    let vs = vr * s_abs;
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
    let mu = if s_abs > 0.0 {
        let (ex, ey) = (sx / s_abs / mu_x, sy / s_abs / mu_y);
        1.0 / (ex * ex + ey * ey).sqrt()
    } else {
        mu_y
    };
    let theta_x = cx / (3.0 * mu * x.fz);
    let theta_y = cy / (3.0 * mu * x.fz);
    let (px, py) = (theta_x * sx, theta_y * sy);
    let psi = (px * px + py * py).sqrt();
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
    // F/psi tends to 3 mu Fz at zero slip, so the secants tend to the stiffnesses.
    let per_psi = if psi > 0.0 { f / psi } else { 3.0 * mu * x.fz };
    EvalOutput { fx, fy, mz, trail, psi, mu, sx, sy, kx: per_psi * theta_x, ky: per_psi * theta_y }
}

/// Relaxation lengths, m: sigma_y = Cy(Fz)/k_y with NGP's carcass lateral
/// stiffness k_y (times the pressure factor), sigma_x = Cx(Fz)/k_x with k_x =
/// NGP's torsional stiffness / R^2 (the torsion torque -torsion * k_tors acts
/// as -torsion/R * k_tors/R longitudinally): the stretched string's sigma =
/// C/k (Pacejka, ch. 5). No speed term: the one measured (0.021 s times V,
/// Vera et al., Sensors 22:6380, 2022) is a passenger tyre's with its
/// excitation frequency mixed in, and puts a rally tyre above Pacejka's 0.12-
/// 0.45 m (0.6 m at 20 m/s); MF-Swift has none either (TNO, 2013). These are
/// the lengths at zero slip without the carcass damping, whose V d/k
/// [`step_dynamic`] adds (Rill's first-order dynamics).
pub fn sigma_lengths(cy_fz: f64, cx_fz: f64, k_y: f64, k_x: f64) -> (f64, f64) {
    (cy_fz / k_y, cx_fz / k_x)
}

/// Carcass damping against the deflection's rate, N/(m/s), longitudinal and lateral: Rill's passenger-tyre values,
/// d_x 500 and 1500 N/(m/s) with c_x 160 and 180 kN/m, d_y 250 and 600 N/(m/s) with c_y 180 and 160 kN/m (Vehicle
/// Dynamics, lecture notes, Hochschule Regensburg 2009, Figs. 4.3, 4.4, 3.45; Wheel Dynamics, DINAME 2007); the
/// means of each pair (ours: no rally tyre's are published).
pub const DAMPING_X: f64 = 1000.0;
pub const DAMPING_Y: f64 = 425.0;
/// Longitudinal relaxation length at the nominal load over the tyre's radius: MF 6.1's (the example 255/55R20 set
/// shipped with MFeval.jl), 1.44. The carcass's longitudinal stiffness follows from it, c_x = Cx(Fz0)/(1.44 R), and
/// stays fixed at every speed and load, so sigma_x = Cx/c_x grows with the load (Pacejka: "longer at higher ...
/// load"). On the Golf II and Fiesta Rally2 c_x is 200-250 kN/m, between Rill's passenger carcasses (160 and 180
/// kN/m) and MF 6.1's own (Kx/Fz 32.9 over 1.44 R0: about 390 kN/m, R0 inferred from the size), and sigma_x 0.45 m at
/// small slip and the nominal load, at the top of Pacejka's 0.12-0.45 m and inside Rill's 0.3-0.8 m. Taking one road
/// set's ratio for a rally tyre is ours (no rally tyre's is published). It replaces NGP's speed-scaled torsional
/// stiffness (a tenth below 10 m/s: sigma_x 0.9 m at 5 % slip, 1.0-1.6 m at 2 %, on the Golf's fronts).
pub const SIGMA_X_OVER_R: f64 = 1.44;

/// The longitudinal carcass stiffness, N/m, of a tyre of radius `radius` (m) at the pressure factor `pf` (NGP's
/// StfFct, as the slip stiffnesses take it): [`SIGMA_X_OVER_R`].
pub fn carcass_x(cal: &Calibration, radius: f64, pf: f64) -> f64 {
    cal.cx0 * (pf / cal.pf_cal) / (SIGMA_X_OVER_R * radius)
}

/// The transient state: the carcass deflections along the slips, m (Rill's x_e, y_e). The force they hold is
/// c u + d u' per axis.
#[derive(Clone, Copy, Debug, Default)]
pub struct TransState {
    pub ux: f64,
    pub uy: f64,
}

/// One step of first-order tyre dynamics (Rill, Vehicle Dynamics 2009, section 3.4.2, eqs. 3.164-3.172): per axis
/// the force is F_D = c u + d u', with the deflection u following (v d + f) u' = f v_N s - v c u, where s is the
/// theoretical slip of the steady brush `steady`, made with the rolling speed floored at [`V_MIN`] (v_N, Rill's
/// modified slip), f = F_S/s its secant (`kx`, `ky`), c the carcass stiffness (`k_x`, `k_y`), d the damping
/// ([`DAMPING_X`], [`DAMPING_Y`]) and v the rolling speed `vr` itself (Rill's eqs. 4.20-4.21: the transport speed
/// restored, so a locked wheel's deflection follows the sliding, u' = -v_s, and at standstill it holds, u' = 0). The
/// relaxation length is then v d/c + f/c: the stretched string's C/c at small slip (Pacejka, ch. 5), falling as the
/// patch slides, as the measured lateral relaxation length does (Rill's Fig. 3.43: about 0.6 m at 0° to 0.1 m at 10°,
/// 4 kN). The linear decay is integrated with its exact exponential over `dt` (stable for any length), and the
/// combined force is limited to the steady friction mu Fz (`limit`), the deflections with it, as Rill limits them
/// (eq. 4.22: "supplemented by appropriate if statements"). Gives the longitudinal and lateral forces in NGP's signs
/// (they oppose the sliding).
pub fn step_dynamic(
    st: &mut TransState,
    steady: &EvalOutput,
    vr: f64,
    k_x: f64,
    k_y: f64,
    limit: f64,
    dt: f64,
) -> (f64, f64) {
    let vr = vr.abs();
    let vn = vr.max(V_MIN);
    let axis = |u: &mut f64, f: f64, s: f64, c: f64, d: f64| {
        // u' = drive - rate u, with drive = f v_N s / (v d + f).
        let den = vr * d + f;
        let (rate, drive) = if den > 0.0 { (vr * c / den, f * vn * s / den) } else { (0.0, 0.0) };
        let x = rate * dt;
        // The exact solution over dt: u e^-x + drive (1 - e^-x)/rate, the last term tending to drive dt as rate -> 0.
        let held = if x > 1e-12 { -libm::expm1(-x) / rate } else { dt };
        let next = *u * libm::exp(-x) + drive * held;
        let force = c * next + d * (drive - rate * next);
        *u = next;
        -force
    };
    let fx = axis(&mut st.ux, steady.kx, steady.sx, k_x, DAMPING_X);
    let fy = axis(&mut st.uy, steady.ky, steady.sy, k_y, DAMPING_Y);
    let elastic = (k_x * st.ux).hypot(k_y * st.uy);
    if elastic > limit && elastic > 0.0 {
        st.ux *= limit / elastic;
        st.uy *= limit / elastic;
    }
    let f = fx.hypot(fy);
    if f > limit && f > 0.0 {
        (fx * limit / f, fy * limit / f)
    } else {
        (fx, fy)
    }
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
            pf_cal: 1.0,
            w_cal: 0.0,
            surf_s_cal: 1.3,
            surf_k_cal: 1.0,
            temp_grip_cal: 1.0,
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
            pressure_factor: 1.0,
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

    /// The input rolling at 15 m/s with the theoretical slips `sx`, `sy`.
    fn at_slips(sx: f64, sy: f64) -> EvalInput {
        let mut x = synth_input();
        // vr = spin R = 15; Vsx = vx + spin R = sx vr; Vsy = sy vr.
        x.spin = 15.0 / x.radius;
        x.vx = (sx - 1.0) * 15.0;
        x.vy = sy * 15.0;
        x
    }

    /// Steps the dynamics on a fixed steady state until the force along `axis` reaches 63 % of the steady one; gives
    /// the distance rolled at 15 m/s, m, and the state after `settle` more steps.
    fn rise(steady: &EvalOutput, k: f64, lateral: bool, settle: usize) -> (f64, (f64, f64)) {
        let dt = 1.0 / 720.0;
        let target = if lateral { steady.fy } else { steady.fx };
        let mut st = TransState::default();
        let mut dist = 0.0;
        let mut f = (0.0, 0.0);
        let mut reached = None;
        for n in 0..72000 {
            f = step_dynamic(&mut st, steady, 15.0, k, k, 1e9, dt);
            dist += 15.0 * dt;
            let along = if lateral { f.1 } else { f.0 };
            if reached.is_none() && along / target >= 0.63 {
                reached = Some(dist);
            }
            if reached.is_some() && n > settle {
                break;
            }
        }
        (reached.expect("never reached 63%"), f)
    }

    #[test]
    fn the_dynamics_settle_on_the_steady_force_over_their_relaxation_length() {
        // 1 deg: linear range, the relaxation length C/k + V d/k (Rill eq. 3.172 at f = C).
        let cal = synth_cal();
        let steady = eval(&at_slips(0.0, 1.0f64.to_radians().tan()), &cal);
        let k = 400000.0;
        let sigma = (steady.ky + 15.0 * DAMPING_Y) / k;
        let (dist, f) = rise(&steady, k, true, 7200);
        assert!((dist / sigma - 1.0).abs() < 0.1, "dist {dist} sigma {sigma}");
        assert!((f.1 / steady.fy - 1.0).abs() < 1e-9 && f.0.abs() < 1e-9, "{f:?} {steady:?}");
    }

    #[test]
    fn sliding_shortens_the_relaxation_length() {
        // Wheelspin at sx 0.5 (saturated): the secant F/s is a fraction of the stiffness, so the force follows the
        // slip over a fraction of the small-slip length (the measured lateral length falls the same way, Rill Fig.
        // 3.43).
        let cal = synth_cal();
        let k = 70000.0;
        let small = eval(&at_slips(0.01, 0.0), &cal);
        let spin = eval(&at_slips(0.5, 0.0), &cal);
        assert!(spin.psi > 1.0 && spin.kx < 0.2 * small.kx, "{spin:?}");
        let (d_small, _) = rise(&small, k, false, 0);
        let (d_spin, f) = rise(&spin, k, false, 7200);
        assert!(d_spin < 0.3 * d_small, "spin {d_spin} small {d_small}");
        assert!((f.0 / spin.fx - 1.0).abs() < 1e-9, "{f:?} {spin:?}");
    }

    #[test]
    fn the_force_turns_with_the_slip_within_a_few_steps_when_sliding() {
        // From settled wheelspin (sx +0.5) to a lock-up's braking slip (-0.5): the force reverses within 20 ms at
        // 5 m/s on the low-speed carcass stiffness.
        let cal = synth_cal();
        let k = 70000.0;
        let drive = eval(&at_slips(0.5, 0.0), &cal);
        let brake = eval(&at_slips(-0.5, 0.0), &cal);
        let dt = 1.0 / 720.0;
        let mut st = TransState::default();
        for _ in 0..7200 {
            step_dynamic(&mut st, &drive, 5.0, k, k, 1e9, dt);
        }
        let steps = (1..).find(|_| step_dynamic(&mut st, &brake, 5.0, k, k, 1e9, dt).0 > 0.0).unwrap();
        assert!(f64::from(steps) * dt < 0.02, "{steps} steps");
    }

    #[test]
    fn the_combined_force_is_limited_to_the_friction() {
        let cal = synth_cal();
        let steady = eval(&at_slips(0.3, 0.3), &cal);
        let mut st = TransState { ux: 1.0, uy: 1.0 };
        let (fx, fy) = step_dynamic(&mut st, &steady, 15.0, 70000.0, 70000.0, 5000.0, 1.0 / 720.0);
        assert!(((fx * fx + fy * fy).sqrt() / 5000.0 - 1.0).abs() < 1e-12, "{fx} {fy}");
    }

    /// The synthetic tyre's longitudinal carcass stiffness at radius 0.3 m.
    fn c_x() -> f64 {
        carcass_x(&synth_cal(), 0.3, 1.0)
    }

    /// The steady brush at car speed `v` (m/s) and wheel surface speed `wr` (m/s), radius 0.3 m, straight ahead.
    fn rolling(v: f64, wr: f64) -> EvalOutput {
        let mut x = synth_input();
        x.vx = -v;
        x.spin = wr / x.radius;
        eval(&x, &synth_cal())
    }

    #[test]
    fn the_longitudinal_relaxation_length_is_the_carcass_s_at_any_speed() {
        // Small slip: sigma_x = (Cx + V d_x)/c_x, C/c 1.44 R at the nominal load at any low speed (NGP's torsional
        // stiffness, a tenth below 10 m/s, made it ten times longer there).
        let dt = 1.0 / 720.0;
        let c = c_x();
        for v in [1.0, 3.0, 8.0] {
            let steady = rolling(v, v * 1.002);
            let mut st = TransState::default();
            let n = (1..100000)
                .find(|_| step_dynamic(&mut st, &steady, v * 1.002, c, 400000.0, 1e9, dt).0 / steady.fx >= 0.63)
                .unwrap();
            let sigma = (steady.kx + v * 1.002 * DAMPING_X) / c;
            let dist = f64::from(n) * dt * v * 1.002;
            assert!((steady.kx / c / (SIGMA_X_OVER_R * 0.3) - 1.0).abs() < 0.03, "C/c {}", steady.kx / c);
            // The damper's share of the force, d u', comes at once, so 63 % comes a little early at speed.
            assert!((0.8..1.05).contains(&(dist / sigma)), "V {v}: {dist} m against {sigma} m");
        }
    }

    #[test]
    fn a_locked_wheel_s_deflection_follows_the_sliding_and_a_stopped_one_holds() {
        // Rill eqs. 4.20-4.22: at zero rolling speed the deflection grows with the sliding speed (u' = -v_s, here
        // 0.2 m/s for 10 ms: 2 mm) and stays when the sliding stops, holding the force.
        let dt = 1.0 / 720.0;
        let mut st = TransState::default();
        let sliding = rolling(0.2, 0.0);
        for _ in 0..(720 / 100) {
            step_dynamic(&mut st, &sliding, 0.0, c_x(), 400000.0, 1e9, dt);
        }
        assert!((st.ux / (-0.2 * 7.0 * dt) - 1.0).abs() < 0.01, "ux {}", st.ux);
        let held = st.ux;
        let stopped = rolling(0.0, 0.0);
        let mut f = (0.0, 0.0);
        for _ in 0..7200 {
            f = step_dynamic(&mut st, &stopped, 0.0, c_x(), 400000.0, 1e9, dt);
        }
        assert_eq!(st.ux, held);
        assert!((f.0 / (-c_x() * held) - 1.0).abs() < 1e-12, "{f:?}");
    }

    #[test]
    fn the_deflection_is_limited_with_the_force() {
        // A locked wheel sliding on: the deflection stops at the friction's, so the force does not outlast the slide.
        let dt = 1.0 / 720.0;
        let mut st = TransState::default();
        let sliding = rolling(3.0, 0.0);
        let limit = sliding.mu * 4000.0;
        for _ in 0..720 {
            step_dynamic(&mut st, &sliding, 0.0, c_x(), 400000.0, limit, dt);
        }
        assert!((st.ux.abs() * c_x() / limit - 1.0).abs() < 1e-9, "ux {}", st.ux);
    }

    #[test]
    fn a_wheel_stepped_explicitly_settles_without_alternating_from_5_m_s_to_rest() {
        // The reason NGP cuts its torsional stiffness at low speed is its explicit wheel step. Rill's carcass with its
        // damping, stepped the same way at 720 Hz (inertia 0.6 kg m², radius 0.3 m, a drive torque of 1 kN at the
        // contact), settles on the steady slip with no step-to-step alternation at 0.5 to 5 m/s.
        let dt = 1.0 / 720.0;
        for v in [0.5, 1.0, 2.0, 5.0] {
            let (r, inertia, torque) = (0.3, 0.6, 1000.0 * 0.3);
            let mut st = TransState::default();
            let mut w = v / r;
            let mut slip = Vec::new();
            for _ in 0..1440 {
                let steady = rolling(v, w * r);
                let (fx, _) = step_dynamic(&mut st, &steady, w * r, c_x(), 400000.0, steady.mu * 4000.0, dt);
                w += dt * (torque + r * fx) / inertia;
                slip.push(w * r - v);
            }
            let tail = &slip[1080..];
            let mean = tail.iter().sum::<f64>() / tail.len() as f64;
            let alternation = tail.windows(3).map(|s| (s[2] - 2.0 * s[1] + s[0]).abs()).fold(0.0, f64::max);
            let spread = tail.iter().map(|s| (s - mean).abs()).fold(0.0, f64::max);
            assert!(mean > 0.0 && spread < 0.01 * mean, "V {v}: slip {mean} spread {spread}");
            assert!(alternation < 1e-4 * mean, "V {v}: alternation {alternation}");
        }
    }
}
