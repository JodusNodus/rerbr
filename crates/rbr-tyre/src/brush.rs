//! Candidate tyre model "brush-v1" (ours, experimental).
//!
//! A steady-state brush with Pacejka's theoretical slips, the generalised
//! Stribeck friction of the LuGre brush tyre models (shape fitted to TRL locked-wheel data),
//! load-sensitive stiffness and friction, bulk temperature / wear / pressure
//! factors, a wet loss with speed (thin film and partial hydroplaning), and Rill's first-order force
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
//! and the game runs the brush down to it on hard surfaces ([`crate::car`]; on soil [`crate::loose`]).

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
/// Load exponent of the pneumatic trail at small slip over the half contact length, t0/a ∝ Fz^M_TRAIL: brush theory's
/// a/3 holds at one load only; the Magic Formula sets give 0.33 (TNO 205/60R15), 0.30 (Continental 225/50R18) and
/// 0.42 (MF 6.1 255/55R20) over 2-6 kN, a from their MF-Swift contact length (Q_RA1, Q_RA2), t0 from QBZ/QCZ/QDZ/QEZ;
/// with their a ∝ Fz^0.6 the trail grows ∝ Fz^0.93-1.01. The mean (ours).
pub const M_TRAIL: f64 = 0.35;

/// The half length the aligning moment takes at the load `fz`, from the contact patch's half length `half_length`
/// and the calibration load `fz0`: the patch's, times (Fz/Fz0)^[`M_TRAIL`], so the trail is a/3 at Fz0.
pub fn trail_half_length(half_length: f64, fz: f64, fz0: f64) -> f64 {
    half_length * libm::pow(fz / fz0, M_TRAIL)
}
/// The friction curve against sliding speed: the generalised Stribeck curve mu_C + (mu_S - mu_C) exp(-(Vs /
/// V_S)^STRIBECK_EXP) (Olsson, Astrom, Canudas de Wit, Gafvert & Lischinsky, "Friction models and friction
/// compensation", European Journal of Control 4, 1998) times Burckhardt's speed factor exp(-SPEED_DECAY Vs)
/// (Kiencke & Nielsen's c4, 0.02-0.04 s/m in their full model). Canudas de Wit et al. (VSD 39, 2003) fitted the
/// Stribeck curve alone, exponent 1/2, V_S 6.57 m/s, mu_C/mu_S 0.8/1.55, to braking on a dry track; their square
/// root falls fastest over the peak's few m/s of sliding, so the braked peak fell 9 % from 32 to 100 km/h and the
/// locked wheel kept 0.76 of it at 100. Ours: the four values fitted to Lambourn & Wesley, TRL PPR496 (2010), a car
/// tyre braked at constant speed to lock on dry asphalt (fig ±0.02): the peak holds with speed (0.97-1.20 times its
/// 32 km/h value at 100) while the locked friction falls, 0.73-0.82 of the peak at 32 km/h, 0.66-0.76 at 64,
/// 0.59-0.67 at 100. The level (mu_S) is calibrated. A rolling tyre takes it at a sliding speed scaled to its slip
/// above [`V_SHAPE`] ([`friction_sliding_speed`]): no open measurement has a rolling tyre losing with speed past the
/// peak as a locked wheel does (SAE 760029: no speed effect beyond 4°; TRR 621 at one speed).
pub const V_S: f64 = 7.0;
pub const STRIBECK_EXP: f64 = 3.0;
pub const MU_C_OVER_S: f64 = 0.8;
pub const SPEED_DECAY: f64 = 0.008;

/// The rolling speed, m/s, above which the friction's sliding speed scales with the theoretical slip instead of the
/// patch's sliding speed ([`friction_sliding_speed`]): MF 6.1's reference speed LONGVL (16.7 m/s, the example set
/// shipped with MFeval.jl), the speed its slip curves are fitted at. The published MF sets give a rolling tyre's force
/// against slip with no speed term (λ_μV, LMUV, 0 or absent in every set read: 27 `.tir` files on GitHub, MFeval,
/// Chrono), Pottinger, Marshall & Arnold (SAE 760029, 10-121 km/h) find the speed effect on lateral force
/// insignificant beyond about 4°, and TRR 621 (1976, flat belt, dry, 48 km/h) has a rolling tyre at 30° at its peak.
/// So above this speed a rolling tyre keeps its slip curve's shape, and the locked wheel (rolling speed 0) keeps the
/// TRL fit's fall with sliding speed. Ours: the scale; the choice of the knee at the reference speed.
pub const V_SHAPE: f64 = 16.7;

/// The sliding speed the friction takes, m/s, at the patch's sliding speed `vs` and the rolling speed `vr`: `vs` up to
/// [`V_SHAPE`], then `vs` times V_SHAPE / `vr` (the theoretical slip |s| = vs / vr times V_SHAPE).
pub fn friction_sliding_speed(vs: f64, vr: f64) -> f64 {
    if vr > V_SHAPE {
        vs * V_SHAPE / vr
    } else {
        vs
    }
}

/// The share of the friction's fall with sliding speed ([`friction_at`], with the wet Coulomb loss) the lateral axis
/// takes; the longitudinal axis takes it whole, so a locked wheel braking straight keeps the TRL fit. Past the peak a
/// rolling tyre's side force falls less than the locked wheel's braking friction: TRR 621 (1976, Calspan flat belt,
/// bias-belted, dry, 48 km/h) about 1.0 of the peak at 30° and 0.94 at 90°; Persson (J. Phys. Condens. Matter 23,
/// 2011, a model with flash temperature) 0.97 at 25°; NGP 0.865 at 30°; the road MF sets' full slide 0.73-0.83. With
/// the whole fall the brush gave 0.833 at 30° (4 kN, 15 m/s); this share gave 0.886 (a user decision, 2026-11-08).
/// The level at 30° is now the slide grip's, solved per wheel ([`crate::tuning::SLIDE_GRIP_MODEL`]); this share
/// shapes the fall with sliding speed on the way there.
pub const LATERAL_FALL: f64 = 0.65;

/// The friction at sliding speed `vs`, m/s, between its level at rest `mu_s` and its Coulomb level `mu_k`.
pub fn friction_at(mu_s: f64, mu_k: f64, vs: f64) -> f64 {
    (mu_k + (mu_s - mu_k) * libm::exp(-libm::pow(vs / V_S, STRIBECK_EXP))) * libm::exp(-SPEED_DECAY * vs)
}

/// Floor of the rolling speed, m/s (keeps the theoretical slips finite).
pub const V_MIN: f64 = 0.5;
/// Water film at W = 1: h = W times 0.7 mm. Measured films on drained
/// highways: 0.18 mm at 1.5 mm/h rain, 0.56-0.85 mm at 7.8-10.2 mm/h (PLOS
/// ONE 2025, Table 2); ASTM E274 wets to ~0.5 mm (secondary).
pub const FILM_MAX: f64 = 0.7e-3;
/// Groove depth of a new tyre, 8 mm: the new road tyres of Continental's Contidrom test (AUTO BILD) and ADAC's 2026
/// worn-tyre test; tread remaining `d` scales it.
pub const TREAD_NEW: f64 = 8.0e-3;
/// Mean profile depth of the road's macrotexture, 0.7 mm, which drains with the grooves and keeps a lifted patch's
/// asperities in contact ([`TEXTURE_LIFT`]): Swedish state roads MPD 0.4 (smooth) to 2 mm (rough) (Hammarström et
/// al., VTI rapport 748A, 2012, via Trupia et al., Int J LCA 2017); the value within that range is ours. It keeps a
/// bald tyre's friction at standstill.
pub const ROAD_TEXTURE: f64 = 0.7e-3;
/// Total hydroplaning speed, m/s: Horne's v_p = 6.36 sqrt(p in kPa) km/h at 200 kPa (90 km/h; NASA TN D-2056, metric
/// form in Zhang et al., Lubricants 13:76, 2025), for water deeper than the grooves. The patch lifts whole no slower
/// (the seam carries no pressure: a rally tyre's 2.0-2.3 bar hot gives 90-96 km/h).
pub const V_TOTAL: f64 = 24.98;
/// Drainage speed of the tread, m/s: partial hydroplaning sets in at v_on = DRAIN_SPEED times the drained depth D
/// (groove depth plus the road's texture) over the game's film [`FILM_MAX`], times (FILM_MAX / h)^[`FILM_EXP`] on a
/// film h ([`partial_lift`]). Ours, fitted with [`LIFT_AT`] and [`TEXTURE_LIFT`] (least squares on worn over new
/// friction, each set at its own film and texture) to three data sets: a drum without texture (Williams & Evans,
/// ASTM STP 793, 1983: 155 SR13 worn to ~1.5 mm, braking peak at 60-120 km/h on 0.2-1 mm, fig ±0.02), a Dutch
/// trailer on four roads (SWOV R-73-08, 1973: 2 mm, 50 / 100 km/h, sprayed 0.3 / 0.6 mm on texture ~0.6 mm, table)
/// and Dijks (TRR 621, 1977: UK peak, 2 / 1 / 0 mm, 0.5 mm, 50-130 km/h; German lateral, 2 / 0 mm, 1.5 mm, 40-80
/// km/h; fig ±0.02). On the game's road (0.7 mm film and texture) v_on is 24 m/s at 2 mm, 21 at 1.6 mm, 78 new and
/// 6.3 bald. Before 2026-11-09: v_on ∝ D / h (2.1 m/s, lifted at 5 v_on, fitted to ABS stops of unknown film,
/// Contidrom via AUTO BILD and Continental's report 2021_100), which lost earlier and more than all three sets (2 mm
/// on 0.7 mm at 40 / 60 / 80 km/h 0.91 / 0.75 / 0.59 of new, Dijks 0.99 / 0.94 / 0.90 on 1.5 mm).
pub const DRAIN_SPEED: f64 = 6.3;
/// The onset's film exponent: v_on ∝ h^-0.43, the drum's half-loss speed against the film (Williams & Evans: ~90 /
/// 80 / 67 km/h on 0.5 / 0.7 / 1 mm; Gallaway-Huebner's -0.26 for deep water, via Ketabdari et al., PLOS ONE 2022).
pub const FILM_EXP: f64 = 0.43;
/// Speed over v_on at which a patch on a road without texture is wholly lifted (the drum: half lost within ~1.6 v_on),
/// unless [`V_TOTAL`] is higher (ours, with [`DRAIN_SPEED`]).
pub const LIFT_AT: f64 = 2.2;
/// The road texture's share of the speed of whole lift, m/s, at a texture and film of 0.7 mm, ∝ texture and
/// h^-[`FILM_EXP`]: the texture's asperities pierce the film and keep a lifted patch in contact, so on a road the
/// loss grows slowly past the onset (Dijks: a bald tyre on 0.5 mm keeps 0.52 of new at 130 km/h, on 1.5 mm 0.73 at
/// 80) where the drum's collapses (ours, with [`DRAIN_SPEED`]).
pub const TEXTURE_LIFT: f64 = 50.0;
/// Wet friction kept at total hydroplaning: μ ≤ 0.05 (Horne & Dreher, NASA TN D-2056, 1963).
pub const WET_KEPT: f64 = 0.05;
/// The Coulomb level's loss on a wet surface at fast sliding, times the wetness W: water lubricates the sliding
/// rubber, the more the faster it slides, so the locked friction falls with speed faster wet than dry while the peak
/// holds. The loss at the sliding speed Vs is WET_COULOMB_LOSS (1 - exp(-Vs / V_S)) ([`wet_coulomb_loss`]): 0.18 at
/// 32 km/h, 0.23 at 64, 0.25 at 100. Ours, fitted to Lambourn & Wesley, TRL PPR496 (2010), the car tyre under ~1 mm
/// of water (ASTM E274), hot rolled and stone mastic asphalt: locked over braked peak 0.64-0.67 at 32 km/h, 0.52-0.57
/// at 64, 0.49-0.52 at 100 (dry 0.73-0.82, 0.66-0.76, 0.59-0.67; fig ±0.02). The scale is the Stribeck speed
/// [`V_S`]. A constant loss (0.18, before 2026-11-08) left the probed tyre 0.03-0.04 above TRL at 64 and 100 km/h.
pub const WET_COULOMB_LOSS: f64 = 0.25;

/// The Coulomb level's loss at the sliding speed `vs`, m/s, on a fully wet surface ([`WET_COULOMB_LOSS`]).
pub fn wet_coulomb_loss(vs: f64) -> f64 {
    WET_COULOMB_LOSS * (1.0 - libm::exp(-vs / V_S))
}

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
    /// Lateral friction's static and kinetic levels (the Stribeck curve's ends before `SPEED_DECAY`, which takes the
    /// friction toward 0 at high sliding speed).
    pub mu_ys: f64,
    pub mu_yk: f64,
    /// Longitudinal friction's static and kinetic levels (as the lateral ones).
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

/// The brush's slip stiffnesses over NGP's: the calibration ([`crate::fit::calibration`]) takes NGP's peaks and 1.3
/// times its cornering slope over 0 to 2°, the longitudinal stiffness [`KX_OVER_KY`] times the cornering one, so 1.3
/// times that of NGP's slope too (Kx/Ky 1.23 at 4 kN, as before the factor; the R888R's longitudinal peak near
/// κ 0.08-0.10). NGP's
/// Cα/Fz is 17.6 per rad at 4 kN with its peak at 9.5° (the scorecard's band 5.6-8.5°); a semi-slick at NGP's grip
/// level, Toyo R888R 195/50 R16 (MegaRide with Calspan 2024, MF 5.2 fits from track and flat-track data, fig ±5 %),
/// has 22.3 / 21.9 per rad there (1.27 / 1.24 times NGP's) and peaks near 6-7°. 1.3 is what a driver judged on the
/// Nordschleife (2026-11-08); the brush peaks at 7.0° at 4 kN with it (9.75° without; scorecard, 0.25° grid). On low
/// friction it fades out on both axes ([`low_mu_slip_stiffness`]).
pub const CORNERING_OVER_NGP: f64 = 1.3;

/// Slip stiffness on low-friction hard surfaces (ice): below the surface static friction [`LOW_MU_KNEE`] the brush's
/// slip stiffnesses scale with (mu / knee)^[`LOW_MU_EXPONENT`]; above it they do not move. Svendenius's brush fits at
/// 4 kN (PhD, Lund 2007, Table 7.1, VTI BV12), winter tyre: C0x 25-27.6 on dry and wet asphalt (mu 1.2 / 1.0, the wet
/// one stiffer: no fall over the tarmac range), 16 on wet basalt (mu 0.27), 6.25 on ice (mu 0.078), 0.58 and 0.23 of
/// asphalt's, which a knee 0.55 with an exponent 0.76 passes through; his summer tyre keeps 0.09 on ice (mu 0.077),
/// an exponent 1.2 with that knee. The exponent is 1 (ours, between the two tyres): below the knee the peak slip
/// stays the knee's, so ice (NGP's 0.10) peaks near 4°, as measured (4-5°: TR0713; SAE 2010-01-0374 Fig 8). The knee
/// is set on NGP's cornering slope: the brush's stiffness over NGP's fades out above it
/// ([`low_mu_slip_stiffness`]). Snow is soil: loose-v1 (`crate::loose`), not this factor.
pub const LOW_MU_KNEE: f64 = 0.55;
pub const LOW_MU_EXPONENT: f64 = 1.0;

/// The factor on the slip stiffnesses at the surface static friction `mu` (NGP's grip): 1 from [`LOW_MU_KNEE`] up,
/// and for a non-positive `mu`.
pub fn low_mu_stiffness_factor(mu: f64) -> f64 {
    if mu > 0.0 && mu < LOW_MU_KNEE {
        libm::pow(mu / LOW_MU_KNEE, LOW_MU_EXPONENT)
    } else {
        1.0
    }
}

/// A slip stiffness at the calibration point (cornering, N/rad, or longitudinal, N) on a hard surface of static
/// friction `mu` (NGP's grip), for the brush's `c0` and `c0_ngp` of NGP's own cornering slope
/// ([`crate::fit::ngp_calibration`]), to be taken with [`low_mu_stiffness_factor`]. On low friction the stiffness over
/// NGP's ([`CORNERING_OVER_NGP`]) goes: the product is min(`c0`, `c0_ngp` mu / [`LOW_MU_KNEE`]) at the exponent 1,
/// `c0` down to the knee times `c0` / `c0_ngp` (about 0.75: the brush's stiffnesses are 1.35-1.4 times NGP's slope's
/// for 1.3 times the slope), and below the knee what it was on NGP's slope (Svendenius's fall, above), so ice keeps
/// its peak slips.
pub fn low_mu_slip_stiffness(mu: f64, c0: f64, c0_ngp: f64) -> f64 {
    if mu > LOW_MU_KNEE {
        c0.min(c0_ngp * libm::pow(mu / LOW_MU_KNEE, LOW_MU_EXPONENT))
    } else if mu > 0.0 {
        c0_ngp.min(c0)
    } else {
        c0
    }
}

/// Camber stiffness over cornering stiffness, C_gamma / C_alpha: 0.05. MF 6.1's example set (255/55R20, 260 kPa) has
/// K_y,gamma0 = Fz (PKY6 + PKY7 dfz) at 0.047-0.052 of K_y,alpha from 2 to 6.75 kN at 2.0-2.6 bar (1.07 Fz per rad at
/// 4 kN, 2.6 bar); MF 5.2's truck set 0.061. Pacejka's brush gives eps a/(3R) for a parabolic camber deflection, with
/// the camber reduction factor eps 0.4-0.7 for car and truck tyres (Romano, Timpone, Bruzelius & Jacobson,
/// Meccanica 2021, as quoted): 0.031-0.055 at a 0.074 m half patch on a 0.315 m wheel. NGP's sliding model takes
/// 0.1 of the inclination's sine off the slip.
pub const CAMBER_OVER_CORNERING: f64 = 0.05;

/// The camber's share of the lateral slip velocity, m/s, at the inclination's sine `sin` (NGP's sign: negative with
/// the top leaning in) and the rolling speed `vr`, m/s: -[`CAMBER_OVER_CORNERING`] sin |vr|, an equivalent slip as
/// MF's horizontal shift is, so the thrust is C_gamma sin at small slip and goes once the patch slides whole. It grows
/// with rolling only: none at standstill or locked.
pub fn camber_slip_velocity(sin: f64, vr: f64) -> f64 {
    -CAMBER_OVER_CORNERING * sin * vr.abs()
}

/// Partial hydroplaning, the factor on friction at the speed `v`, m/s, with the groove depth `tread`, the road's
/// texture depth `texture` and the film `film`, m: the grooves and the texture drain the film below v_on =
/// [`DRAIN_SPEED`] D / [`FILM_MAX`] (FILM_MAX / h)^[`FILM_EXP`] (D = tread plus texture); above it the lifted share of
/// the patch grows linearly to the whole patch at [`LIFT_AT`] v_on plus the texture's [`TEXTURE_LIFT`] (texture /
/// FILM_MAX) (FILM_MAX / h)^FILM_EXP, or [`V_TOTAL`], the higher, where [`WET_KEPT`] of the friction is left. 1 with
/// no film.
pub fn partial_lift(v: f64, tread: f64, texture: f64, film: f64) -> f64 {
    if film <= 0.0 {
        return 1.0;
    }
    let thin = libm::pow(FILM_MAX / film, FILM_EXP);
    let v_on = DRAIN_SPEED * (tread.max(0.0) + texture) / FILM_MAX * thin;
    let v_whole = (LIFT_AT * v_on + TEXTURE_LIFT * texture / FILM_MAX * thin).max(V_TOTAL);
    let lifted = ((v - v_on) / (v_whole - v_on)).clamp(0.0, 1.0);
    1.0 - (1.0 - WET_KEPT) * lifted
}

/// Partial hydroplaning on the game's road ([`partial_lift`]) at the wetness `w` (film W times [`FILM_MAX`], texture
/// [`ROAD_TEXTURE`]), the speed `v`, m/s, and the tread remaining `d` (1 new, grooves d times [`TREAD_NEW`]). Nothing
/// on a dry surface, nothing for a new tyre to 280 km/h on the 0.7 mm film; worn over new at 2 mm 0.96 / 0.86 at 100
/// / 130 km/h, at 1.6 mm 0.98 / 0.91 / 0.80 at 80 / 100 / 130, bald 0.92 / 0.83 / 0.74 / 0.65 at 40 / 60 / 80 / 100.
/// Horne's speed only bounds the total lift (ADAC's 17-22 % of v_p lost at 2 of 8 mm is deep water, not modelled).
pub fn wet_loss(w: f64, v: f64, d: f64) -> f64 {
    if w <= 0.0 {
        return 1.0;
    }
    partial_lift(v, d * TREAD_NEW, ROAD_TEXTURE, w * FILM_MAX)
}

/// The wet peak's loss with speed on a thin film, per m/s over [`THIN_FILM_FROM`] at W = 1: the film is squeezed out
/// of the patch less completely the faster the patch meets it. Fitted to Reimpell & Sponagel (Fahrwerktechnik: Reifen
/// und Räder, 1986, via Müller et al., FAT-Schriftenreihe 299, Abb. 21b; fig ±0.02), a car tyre's peak μ, all films
/// ~0.755 at 60 km/h; at 100 / 140 km/h 0.71 / 0.655 on 0.5 mm, 0.675 / 0.59 on 1 mm; 0.745 / 0.73 on 0.2 mm.
/// Interpolated to the game's 0.7 mm film ([`FILM_MAX`]) that is 0.922 / 0.833 of the 60 km/h peak; least squares
/// through 60 km/h gives 0.0074 (0.918 / 0.836). Linear in W (ours): W 0.25 (0.175 mm) keeps 0.979 / 0.959 (0.2 mm
/// film 0.99 / 0.97). Flat to 60 km/h as Veith found the wet peak to ~50 km/h (ASTM STP 793, via Beautru 2012).
pub const THIN_FILM_LOSS: f64 = 0.0074;
/// The speed the thin-film loss starts from, m/s: 60 km/h, the curves' common point.
pub const THIN_FILM_FROM: f64 = 60.0 / 3.6;

/// The thin-film factor on friction at the wetness `w` and the speed `v`, m/s ([`THIN_FILM_LOSS`]), never below
/// [`WET_KEPT`].
pub fn thin_film(w: f64, v: f64) -> f64 {
    (1.0 - THIN_FILM_LOSS * w * (v - THIN_FILM_FROM).max(0.0)).max(WET_KEPT)
}

/// The wet factor on friction against speed at the wetness `w`, the speed `v`, m/s, and the tread remaining `d`:
/// the thin film's loss ([`thin_film`], a new tyre's, as Reimpell's curves are) times the partial hydroplaning of the
/// tread `d` relative to a new tread's ([`wet_loss`] at `d` over at 1). The thin film takes over the new tread's own
/// partial hydroplaning (both count the patch's film above 60 km/h, so a new tyre takes [`thin_film`] alone), and a
/// worn tread keeps its partial hydroplaning against new: [`wet_loss`]'s fit is to worn over new friction. `v`
/// is the speed over the road (the contact point's), the speed the patch meets the film at, rolling or locked:
/// Reimpell's curves are against travel speed, and the sliding speed at the peak (1-3 m/s) does not grow with it.
/// 1 on a dry surface.
pub fn wet_speed_factor(w: f64, v: f64, d: f64) -> f64 {
    if w <= 0.0 {
        return 1.0;
    }
    thin_film(w, v) * wet_loss(w, v, d) / wet_loss(w, v, 1.0)
}

/// The wetness of NGP's damp tarmac (`WaterFactor` 0.25), where [`DAMP_LOSS`] is whole.
pub const DAMP_W: f64 = 0.25;
/// The friction lost at damp ([`DAMP_W`]) on top of NGP's damp surface values, so that the brush's damp peak is 0.82
/// of dry (0.909 with NGP's values alone: μ 0.925 of dry through the tyre's wet slope, lab braked peak at 4 kN,
/// 50 km/h) and 1.04 of wet; wet (W 1) keeps NGP's 0.79. A user decision (2026-11-09) on this evidence: the loss from
/// dry comes with the first water in the texture, rubber sealing the water pools (Persson et al., Nature Materials 3,
/// 2004: wet 0.70-0.80 of dry at low speed); the thinnest water that wets a road grips like twice as much (SWOV
/// R-73-08, 0.3 against 0.6 mm sprayed: peak 0.995, lateral 1.00 at 50 km/h), as on the drums (0.2 against 0.7 mm at
/// 60 km/h: 1.055 Liedl & Köhler, 1.00 braking and 1.01 lateral Williams & Evans, ASTM STP 793); so damp is 1.00-1.06
/// of wet at ≤ 60 km/h and 0.79-0.84 of dry, inside Bachmann's 0.70-0.85 (diss. TU Darmstadt 1998, via FAT 299 and
/// Klempau 2004).
pub const DAMP_LOSS: f64 = 0.098;

/// The damp level, the factor on friction at the wetness `w`: 1 - [`DAMP_LOSS`] at [`DAMP_W`], 1 dry and wet, linear
/// in W between (a tent). A wheel over two materials gets the blend of their W and NGP blends their friction the same
/// way, so between dry and damp the level goes from dry's to damp's, between damp and wet from 0.82 to 0.79 of dry
/// with no dip; W 0.21 (map 85's damp tarmac blended with a dry patch) keeps 0.83 of the loss. Hard surfaces only.
pub fn damp_level(w: f64) -> f64 {
    1.0 - DAMP_LOSS * (w / DAMP_W).min((1.0 - w) / (1.0 - DAMP_W)).clamp(0.0, 1.0)
}

/// Dry-wear gain on friction: 1 + 0.167 (1 - d), fading with wetness (times
/// (1 - W)): worn tread grips more when dry (+10% peak mu at 60% worn, Singh
/// Table 3) and loses in the wet (carried by the partial hydroplaning there, [`wet_loss`]).
pub fn wear_gain(d: f64, w: f64) -> f64 {
    1.0 + 0.167 * (1.0 - d) * (1.0 - w)
}

/// Friction magnitude of one axis at sliding speed `vs`: mu_k (scaled by the
/// kinetic ratio) + (mu_s scaled by the static ratio - mu_k scaled by the
/// kinetic ratio) at the sliding speed ([`friction_at`]), times the load sensitivity
/// (Fz/Fz0)^(n-1) (`N_MU_Y` or `N_MU_X`), NGP's temperature grip factor, the
/// wet-faded dry-wear gain (normalised), the damp level ([`damp_level`], normalised) and the wet loss; the wetness `w`
/// lowers mu_k by its loss at the sliding speed ([`wet_coulomb_loss`], normalised). Both axes scale
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
    let loss = wet_coulomb_loss(vs);
    let kk = mu_k * kin_ratio * (1.0 - loss * w) / (1.0 - loss * cal.w_cal);
    let base = friction_at(ks, kk, vs);
    base * libm::pow(fz / cal.fz0, n - 1.0)
        * (temp_grip / cal.temp_grip_cal)
        * (wear_gain(d, w) / wear_gain(cal.d_cal, cal.w_cal))
        * (damp_level(w) / damp_level(cal.w_cal))
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
    /// Bulk tread temperature, K; tread remaining 1 = new; pressure, Pa (no term reads it: the seam carries none),
    /// and NGP's stiffness factor at it (StfFct); surface wetness 0..1 (NGP WaterFactor).
    pub bulk_temp: f64,
    pub tread_remaining: f64,
    pub pressure: f64,
    pub pressure_factor: f64,
    pub wetness: f64,
    /// Whether the wet loss with speed ([`wet_speed_factor`]: the thin film and the partial hydroplaning) applies (the
    /// game's brush: yes).
    pub hydroplaning: bool,
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
/// combined slip (Tire and Vehicle Dynamics, 3rd ed., ch. 3), with the
/// anisotropic stiffnesses split as Svendenius does (Tire Modeling and
/// Friction Estimation, PhD, Lund 2007, ch. 4: eqs. 4.14, 4.17, 4.18-4.20 and
/// 4.26-4.28). theta_i = C_i/(3 mu Fz) with the directional mu below, psi =
/// sqrt((theta_x sx)^2 + (theta_y sy)^2); the adhering part of the patch
/// gives F_a,i = -C_i s_i (1 - psi)^2, along the stiffness-weighted slip, the
/// sliding part F_s = mu Fz psi^2 (3 - 2 psi) against the sliding velocity
/// (the direction of (sx, sy)); from psi = 1 the patch slides whole, mu Fz.
/// For pure slip the sum is Pacejka's mu Fz (3 psi - 3 psi^2 + psi^3). The
/// friction ellipse gives the directional mu along the sliding velocity e,
/// mu_dir = 1/sqrt((ex/mu_x)^2 + (ey/mu_y)^2) (Svendenius's collinear slide
/// force, eqs. 4.19-4.20). Aligning moment M'_z (eqs. 4.33-4.35; the
/// deformation torque M''_z is left out): Mz = mu Fz a (1 - psi)^2 (theta_y sy
/// (1 - 4 psi) + 3 psi^2 sy/|s|) for psi < 1, else 0; for pure side slip mu Fz
/// a theta_y sy (1 - psi)^3, and the trail -Mz/Fy tends to a/3 at small slip. [`eval_slip`]
/// takes [`affine_trail`] in its place (the patch's M'_z stays loose-v1's).
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
/// the sliding speed is `vr |s|`, the friction's [`friction_sliding_speed`] of it.
pub fn eval_slip(x: &EvalInput, sx: f64, sy: f64, vr: f64, cal: &Calibration) -> EvalOutput {
    let cy = stiffness_at(x.fz, x.bulk_temp, x.tread_remaining, x.pressure_factor, cal.cy0, M_CY, cal);
    let cx = stiffness_at(x.fz, x.bulk_temp, x.tread_remaining, x.pressure_factor, cal.cx0, M_CX, cal);
    let s_abs = (sx * sx + sy * sy).sqrt();
    let vs = friction_sliding_speed(vr * s_abs, vr);
    let v = (x.vx * x.vx + x.vy * x.vy).sqrt();
    let wet = if x.hydroplaning { wet_speed_factor(x.wetness, v, x.tread_remaining) } else { 1.0 };
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
    let mu_y_at = |vs: f64| {
        mu_axis(
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
        )
    };
    let rest = mu_y_at(0.0);
    let mu_y = rest - LATERAL_FALL * (rest - mu_y_at(vs));
    affine_trail(patch(x.fz, x.half_length, cx, cy, mu_x, mu_y, sx, sy), x.half_length)
}

/// The aligning moment on a hard surface with the affine trail Hsu & Gerdes take (Y.-H. J. Hsu, PhD Stanford 2009,
/// eq. 2.13; Hsu, Laws & Gerdes, IEEE TCST 18:4, 2010): a/3 at zero slip, falling linearly to 0 where the patch
/// slides whole, t = a/3 (1 - psi) with psi the combined slip [`patch`] solves, and Mz = -t Fy. They found the
/// parabolic-pressure brush's trail (the patch's M'_z, t = a/3 (1 - psi)^3 / (1 - psi + psi^2/3)) "less accurate in
/// practice than the accompanying force model" and validated the affine one against their test car's steering
/// torque up to the limit on pavement and gravel. Same t0 and the same zero at full sliding; in between the trail
/// falls less, so Mz peaks at psi 0.37 instead of 0.25 and higher, 0.158 μ Fz a instead of 27/256 (the Magic Formula
/// sets: 0.19-0.26). Loose surfaces keep the patch's M'_z.
pub fn affine_trail(out: EvalOutput, half_length: f64) -> EvalOutput {
    let trail = if out.psi < 1.0 { half_length / 3.0 * (1.0 - out.psi) } else { 0.0 };
    EvalOutput { mz: -trail * out.fy, trail, ..out }
}

/// The brush's patch (see [`eval`]) at the load `fz`, half length `half_length`, slip stiffnesses `cx`, `cy`, axis
/// frictions `mu_x`, `mu_y` and theoretical slips `sx`, `sy`: the combined slip of the adhering and sliding parts, the
/// aligning moment and the secant stiffnesses. [`crate::loose`] takes it with its own frictions.
#[allow(clippy::too_many_arguments)]
pub fn patch(fz: f64, half_length: f64, cx: f64, cy: f64, mu_x: f64, mu_y: f64, sx: f64, sy: f64) -> EvalOutput {
    let s_abs = (sx * sx + sy * sy).sqrt();
    // Friction ellipse along the theoretical-slip direction.
    let mu = if s_abs > 0.0 {
        let (ex, ey) = (sx / s_abs / mu_x, sy / s_abs / mu_y);
        1.0 / (ex * ex + ey * ey).sqrt()
    } else {
        mu_y
    };
    let theta_x = cx / (3.0 * mu * fz);
    let theta_y = cy / (3.0 * mu * fz);
    let (px, py) = (theta_x * sx, theta_y * sy);
    let psi = (px * px + py * py).sqrt();
    // Adhesion along the stiffness-weighted slip, C_i s_i (1 - psi)^2; sliding along the sliding velocity (the
    // direction of (sx, sy)), mu Fz psi^2 (3 - 2 psi); both 0 and mu Fz from psi = 1.
    let adhesion = if psi < 1.0 { (1.0 - psi) * (1.0 - psi) } else { 0.0 };
    let sliding = if psi < 1.0 { mu * fz * psi * psi * (3.0 - 2.0 * psi) } else { mu * fz };
    let per_s = if s_abs > 0.0 { sliding / s_abs } else { 0.0 };
    // The secant slip stiffnesses -F_i/s_i: C_i at zero slip.
    let kx = cx * adhesion + per_s;
    let ky = cy * adhesion + per_s;
    let (fx, fy) = (-kx * sx, -ky * sy);
    // Aligning moment of each part (Svendenius eqs. 4.33-4.35, M'_z only): adhesion mu Fz a theta_y sy (1 - psi)^2
    // (1 - 4 psi), sliding 3 mu Fz a psi^2 (1 - psi)^2 times its lateral share sy/|s|.
    let mz = if psi < 1.0 && s_abs > 0.0 {
        mu * fz * half_length * adhesion * (theta_y * sy * (1.0 - 4.0 * psi) + 3.0 * psi * psi * sy / s_abs)
    } else {
        0.0
    };
    let trail = if fy != 0.0 { -mz / fy } else { 0.0 };
    EvalOutput { fx, fy, mz, trail, psi, mu, sx, sy, kx, ky }
}

/// Relaxation lengths, m: sigma_y = Cy(Fz)/k_y with NGP's carcass lateral
/// stiffness k_y (times the pressure factor), sigma_x = Cx(Fz)/k_x with k_x
/// the longitudinal carcass ([`carcass_x`]): the stretched string's sigma =
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
/// Longitudinal carcass stiffness at the calibration pressure, N/m: 450 kN/m (ours; no race or rally carcass data is
/// open, range 350-550). Published c_x at 4 kN: MF-Swift overall stiffness with sigma = K/c (Besselink, Schmeitz &
/// Pacejka, VSD 48 S1, 2010, eqs. 17-20), TNO 205/60R15 382, MF 6.1 255/55R20 358 (MFeval.jl's example; MFeval
/// 4.0's 399), IIT Madras 245/60R16 288 kN/m; measured, TU/e Flatplank 225/50R17 214-234 kN/m (op het Veld 2008,
/// Table 4.2), P245/70R15 static 163-188 kN/m (NHTSA "The Pneumatic Tire", Table 9.1). c_x/c_y is 1.3-2.75 in every
/// set. c_x rises only 5-9 % from 4 to 6 kN, so it is held constant and sigma_x = Cx/c_x grows with the load: about
/// 0.2 m at 4 kN. It replaces MF 6.1's sigma_x/R0 (1.44 at Fz0), which halved c_x: that ratio goes with the set's
/// Kx of 222 kN at Fz0 (Kx/Fz 32.9 at 6.75 kN), more than twice ours at 4 kN.
pub const CARCASS_X: f64 = 450_000.0;

/// The carcass stiffnesses against the inflation pressure, as powers of NGP's stiffness factor `pf` (StfFct, which
/// moves the slip stiffnesses and NGP's own carcass alike): the carcass stiffens faster than the slip stiffness
/// laterally and slower longitudinally. Per +0.5 bar c_y +9.5 % (op het Veld, MSc TU/e 2008, Flatplank, measured) and
/// +13-16 % (MF 6.1's PCFY3 at 2.6-2.0 bar), c_x +3.5 % and +4.2-4.4 % (PCFX3); NGP's factor (one table for every
/// tyre, 0.9 at 1.5 bar to 1.1 at 3 bar) rises 6.9 % from 2.0 bar, so the means, +12 % and +3.9 %, are its powers 1.7
/// and 0.6. So the lateral relaxation length C_y/c_y shortens with pressure, as both sources' does (-12 % and -21 to
/// -26 %), though less: their cornering stiffness falls with pressure at 4 kN, NGP's rises (the data disagree there).
pub const CARCASS_Y_PRESSURE: f64 = 1.7;
pub const CARCASS_X_PRESSURE: f64 = 0.6;

/// The longitudinal carcass stiffness, N/m, at the pressure factor `pf`: [`CARCASS_X`] times its pressure power
/// ([`CARCASS_X_PRESSURE`]). No load term.
pub fn carcass_x(cal: &Calibration, pf: f64) -> f64 {
    CARCASS_X * libm::pow(pf / cal.pf_cal, CARCASS_X_PRESSURE)
}

/// The lateral carcass stiffness, N/m, at the pressure factor `pf`, from NGP's `k_y` at it (which moves with `pf`):
/// moved to its pressure power ([`CARCASS_Y_PRESSURE`]).
pub fn carcass_y(cal: &Calibration, k_y: f64, pf: f64) -> f64 {
    k_y * libm::pow(pf / cal.pf_cal, CARCASS_Y_PRESSURE - 1.0)
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
    let steady_force = steady.fx.hypot(steady.fy);
    if elastic > limit && steady_force > 0.0 {
        // Sliding at the limit: the carcass holds the steady force's direction (the friction's, against the sliding
        // velocity). Scaling the deflections alone would keep the direction their integration took, weighted by the
        // carcass stiffnesses: a locked wheel's (v = 0, u' the sliding velocity) would point along c_x v_sx, c_y v_sy.
        let r = limit / steady_force;
        st.ux = -steady.fx * r / k_x;
        st.uy = -steady.fy * r / k_y;
        return (steady.fx * r, steady.fy * r);
    }
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
            hydroplaning: true,
            temp_grip: 1.0,
            surf_static: 1.3,
            surf_kinetic: 1.0,
        }
    }

    #[test]
    fn the_stiffness_falls_below_the_knee_only() {
        for mu in [0.0, -1.0, LOW_MU_KNEE, 0.68, 1.105, 1.3] {
            assert_eq!(low_mu_stiffness_factor(mu), 1.0, "{mu}");
        }
        // Between Svendenius's tyres on ice (mu 0.078): winter 0.23, summer 0.09 of asphalt's.
        let ice = low_mu_stiffness_factor(0.078);
        assert!(ice > 0.09 && ice < 0.23, "{ice}");
        assert!((low_mu_stiffness_factor(0.275) - 0.5).abs() < 1e-12);
        assert!(low_mu_stiffness_factor(0.5) < 1.0 && low_mu_stiffness_factor(0.5) > low_mu_stiffness_factor(0.1));
    }

    #[test]
    fn the_stiffness_over_ngp_s_goes_down_to_the_knee() {
        let (cy0, ngp) = (1.3e5, 1e5);
        let at = |mu: f64| low_mu_slip_stiffness(mu, cy0, ngp) * low_mu_stiffness_factor(mu);
        for mu in [0.0, -1.0, 0.72, 1.105] {
            assert_eq!(at(mu), cy0, "{mu}");
        }
        // NGP's slope's on ice, and continuous through both corners.
        for mu in [0.078, 0.3, LOW_MU_KNEE] {
            assert!((at(mu) / (ngp * low_mu_stiffness_factor(mu)) - 1.0).abs() < 1e-12, "{mu}");
        }
        assert!((at(LOW_MU_KNEE * 1.3) / cy0 - 1.0).abs() < 1e-12);
        assert!((at(0.6) / (ngp * 0.6 / LOW_MU_KNEE) - 1.0).abs() < 1e-12);
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
    fn above_the_shape_speed_a_rolling_tyre_keeps_its_slip_curve_and_a_locked_one_its_sliding_speed() {
        let cal = synth_cal();
        let mu_y = |v: f64, deg: f64| {
            let a = deg.to_radians();
            let mut x = synth_input();
            (x.vy, x.vx) = (v * a.sin(), -v * a.cos());
            x.spin = -x.vx / x.radius;
            -eval(&x, &cal).fy / x.fz
        };
        // Free rolling at 20° and 25 / 35 m/s (rolling speed over V_SHAPE): the same force.
        assert!((mu_y(25.0, 20.0) / mu_y(35.0, 20.0) - 1.0).abs() < 1e-9);
        // Locked (no spin, the rolling speed floored at V_MIN): the sliding speed is the patch's.
        assert_eq!(friction_sliding_speed(27.8, V_MIN), 27.8);
        assert_eq!(friction_sliding_speed(10.0, 33.4), 10.0 * V_SHAPE / 33.4);
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
        // The lateral axis takes LATERAL_FALL of the friction's fall with sliding speed.
        let mu = cal.mu_ys - LATERAL_FALL * (cal.mu_ys - friction_at(cal.mu_ys, cal.mu_yk, vs));
        assert!((f / (mu * x.fz) - 1.0).abs() < 1e-9, "f {f} mu Fz {}", mu * x.fz);
        // Direction: opposes the sliding velocity.
        assert!(o.fy < 0.0 && o.fx <= 0.0, "fx {} fy {}", o.fx, o.fy);
        // Locked straight (no spin, no side slip): the longitudinal axis keeps the whole fall.
        let mut x = synth_input();
        x.spin = 0.0;
        let o = eval(&x, &cal);
        let mu = friction_at(cal.mu_xs, cal.mu_xk, 15.0);
        assert!((o.fx.abs() / (mu * x.fz) - 1.0).abs() < 1e-9, "fx {} mu Fz {}", o.fx, mu * x.fz);
    }

    #[test]
    fn combined_slip_slides_along_the_sliding_velocity() {
        // Cx = 1.4 Cy (KX_OVER_KY): at 4 deg with 25 % slip the patch slides whole, so the force opposes the
        // sliding velocity and the side share is its sy/|s|, not the stiffness-weighted slip's (1/1.4 of it).
        let cal = Calibration { cx0: KX_OVER_KY * 80000.0, ..synth_cal() };
        let alpha = 4f64.to_radians();
        let (vlat, vlong) = (15.0 * alpha.sin(), -15.0 * alpha.cos());
        let mut x = synth_input();
        (x.vy, x.vx, x.spin) = (vlat, vlong, 1.25 * -vlong / x.radius);
        let o = eval(&x, &cal);
        assert!(o.psi > 1.0, "psi {}", o.psi);
        let share = -o.fy / (o.fx * o.fx + o.fy * o.fy).sqrt();
        let slide = o.sy / (o.sx * o.sx + o.sy * o.sy).sqrt();
        assert!((share / slide - 1.0).abs() < 1e-12, "share {share} sliding {slide}");
        // Partly sliding (1 deg, 1 % slip): between the stiffness-weighted and the sliding direction.
        let alpha = 1f64.to_radians();
        let (vlat, vlong) = (15.0 * alpha.sin(), -15.0 * alpha.cos());
        (x.vy, x.vx, x.spin) = (vlat, vlong, 1.01 * -vlong / x.radius);
        let o = eval(&x, &cal);
        assert!(o.psi > 0.0 && o.psi < 1.0, "psi {}", o.psi);
        let (along_slip, along_stiff) = (o.sy / o.sx, o.sy / (KX_OVER_KY * o.sx));
        assert!(o.fy / o.fx < along_slip && o.fy / o.fx > along_stiff, "{o:?}");
    }

    #[test]
    fn pure_slip_is_pacejkas_cubic() {
        let cal = synth_cal();
        for deg in [1.0f64, 3.0, 6.0, 12.0] {
            let alpha = deg.to_radians();
            let mut x = synth_input();
            (x.vy, x.vx) = (15.0 * alpha.sin(), -15.0 * alpha.cos());
            x.spin = -x.vx / x.radius;
            let o = eval(&x, &cal);
            let p = o.psi.min(1.0);
            let f = o.mu * x.fz * (3.0 * p - 3.0 * p * p + p * p * p);
            // The affine trail's moment; the patch's own M'_z is brush theory's.
            let mz = x.half_length / 3.0 * (1.0 - p) * f;
            assert!((-o.fy / f - 1.0).abs() < 1e-12 && o.fx.abs() < 1e-9, "{deg} {o:?} {f}");
            assert!((o.mz - mz).abs() < 1e-9, "{deg} {o:?} {mz}");
        }
    }

    #[test]
    fn affine_trail_peaks_later_and_higher_than_the_patch() {
        // Pure side slip at psi: brush theory's Mz/(mu Fz a) = psi (1 - psi)^3, peak 27/256 at 0.25; the affine
        // trail's (1 - psi)(3 psi - 3 psi^2 + psi^3)/3, peak 0.158 at 0.37.
        let (fz, a, c) = (4000.0, 0.07, 70000.0);
        let mut best = (0.0, 0.0, 0.0, 0.0);
        for i in 1..1000 {
            let sy = 3.0 * fz / c * f64::from(i) / 1000.0;
            let p = patch(fz, a, c, c, 1.0, 1.0, 0.0, sy);
            let q = affine_trail(p, a);
            if p.mz > best.0 {
                (best.0, best.1) = (p.mz, p.psi);
            }
            if q.mz > best.2 {
                (best.2, best.3) = (q.mz, q.psi);
            }
        }
        assert!((best.0 / (fz * a) - 27.0 / 256.0).abs() < 1e-4 && (best.1 - 0.25).abs() < 0.002, "{best:?}");
        assert!((best.2 / (fz * a) - 0.1577).abs() < 5e-4 && (best.3 - 0.37).abs() < 0.01, "{best:?}");
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
        // the kinetic one doubles the fast-sliding mu (past the Stribeck fall, under the speed decay alone).
        let cal = synth_cal();
        let slow = mu_axis(1.3, 1.0, 0.0, 4000.0, N_MU_Y, 2.0, 1.0, 1.0, 1.0, 0.0, 1.0, &cal);
        assert!((slow / 2.6 - 1.0).abs() < 1e-12, "slow {slow}");
        let fast = mu_axis(1.3, 1.0, 100.0, 4000.0, N_MU_Y, 1.0, 2.0, 1.0, 1.0, 0.0, 1.0, &cal);
        assert!((fast / (2.0 * libm::exp(-SPEED_DECAY * 100.0)) - 1.0).abs() < 1e-12, "fast {fast}");
    }

    /// Braked at constant speed `v` on wetness `w` to its peak and locked, as TRL's trailer does: (peak, locked)
    /// friction. The wet loss is off, as in the game.
    fn braked_peak_and_locked(v: f64, w: f64, cal: &Calibration) -> (f64, f64) {
        let x = EvalInput { vx: -v, wetness: w, hydroplaning: false, ..synth_input() };
        let at = |k: f64| eval(&EvalInput { spin: (1.0 - k) * v / x.radius, ..x }, cal).fx.abs() / x.fz;
        ((0..=200).map(|i| at(i as f64 * 0.005)).fold(0.0, f64::max), at(1.0))
    }

    #[test]
    fn the_braked_peak_holds_with_speed_and_the_locked_friction_falls_as_trl_measured() {
        // Lambourn & Wesley, TRL PPR496: a car tyre on asphalt, the peak 0.97-1.20 times its 32 km/h value at 100
        // (dry); locked over peak 0.73-0.82 at 32 km/h and 0.59-0.67 at 100 dry, 0.64-0.67 and 0.49-0.52 wet (fig
        // ±0.02).
        let cal = Calibration { cx0: KX_OVER_KY * 80000.0, mu_xk: MU_C_OVER_S * 1.3, ..synth_cal() };
        let (p32, l32) = braked_peak_and_locked(32.0 / 3.6, 0.0, &cal);
        let (p100, l100) = braked_peak_and_locked(100.0 / 3.6, 0.0, &cal);
        assert!((0.71..=0.84).contains(&(l32 / p32)), "32 km/h {l32} / {p32}");
        assert!((0.57..=0.69).contains(&(l100 / p100)), "100 km/h {l100} / {p100}");
        assert!((0.95..=1.2).contains(&(p100 / p32)), "peak {p100} / {p32}");
        // Wet: the same peak, the locked friction lower.
        let (wp32, wl32) = braked_peak_and_locked(32.0 / 3.6, 1.0, &cal);
        let (wp100, wl100) = braked_peak_and_locked(100.0 / 3.6, 1.0, &cal);
        assert!((wp32 / p32 - 1.0).abs() < 0.01 && (wp100 / p100 - 1.0).abs() < 0.01, "{wp32} {wp100}");
        assert!((0.62..=0.69).contains(&(wl32 / wp32)), "wet 32 km/h {wl32} / {wp32}");
        assert!((0.47..=0.58).contains(&(wl100 / wp100)), "wet 100 km/h {wl100} / {wp100}");
    }

    #[test]
    fn partial_hydroplaning_spares_a_new_tyre_and_takes_a_worn_one_as_measured() {
        // Worn over new at each set's tread, texture and film (mm), speed (km/h): measured, and the band the model
        // keeps to (±0.1, the sets' spread; SWOV's 0.6 mm cell, inferred from its means, and Dijks' 130 km/h ±0.15).
        let worn = |tread: f64, tex: f64, film: f64, kmh: f64| {
            let at = |t: f64| partial_lift(kmh / 3.6, t * 1e-3, tex * 1e-3, film * 1e-3);
            at(tread) / at(8.0)
        };
        #[rustfmt::skip]
        let sets: [(f64, f64, f64, f64, f64, f64); 16] = [
            // Williams & Evans' drum (no texture), ~1.5 mm worn.
            (1.5, 0.0, 0.5, 60.0, 0.96, 0.1), (1.5, 0.0, 0.5, 80.0, 0.68, 0.1), (1.5, 0.0, 0.5, 100.0, 0.34, 0.1),
            (1.5, 0.0, 0.7, 60.0, 0.85, 0.1), (1.5, 0.0, 0.7, 80.0, 0.49, 0.1), (1.5, 0.0, 0.7, 100.0, 0.24, 0.1),
            (1.5, 0.0, 1.0, 60.0, 0.67, 0.1), (1.5, 0.0, 1.0, 80.0, 0.20, 0.1),
            // SWOV, 2 mm on texture ~0.6 mm.
            (2.0, 0.6, 0.3, 100.0, 0.96, 0.1), (2.0, 0.6, 0.6, 100.0, 0.82, 0.16),
            // Dijks: UK 2 mm on 0.5 mm, German lateral 2 mm and bald on 1.5 mm.
            (2.0, 0.7, 0.5, 80.0, 0.97, 0.1), (2.0, 0.7, 0.5, 130.0, 0.83, 0.15),
            (2.0, 0.7, 1.5, 40.0, 0.99, 0.1), (2.0, 0.7, 1.5, 60.0, 0.94, 0.1), (2.0, 0.7, 1.5, 80.0, 0.90, 0.1),
            (0.0, 0.7, 1.5, 80.0, 0.73, 0.15),
        ];
        for (tread, tex, film, kmh, measured, band) in sets {
            let model = worn(tread, tex, film, kmh);
            assert!(
                (model - measured).abs() <= band,
                "{tread} mm on {film} mm at {kmh} km/h: {model} against {measured}"
            );
        }
        // The drum's thinnest film (0.2 mm): no loss to 80 km/h (measured above new).
        assert_eq!(worn(1.5, 0.0, 0.2, 80.0), 1.0);
        // Dry: nothing; new on the full film: nothing to 280 km/h.
        let (v80, v100) = (80.0 / 3.6, 100.0 / 3.6);
        assert_eq!(wet_loss(0.0, 60.0, 0.2), 1.0);
        assert_eq!(wet_loss(1.0, 280.0 / 3.6, 1.0), 1.0);
        // A thinner film drains to a higher speed; nothing below the onset.
        assert!(wet_loss(0.5, v100, 0.2) > wet_loss(1.0, v100, 0.2) && wet_loss(1.0, v100, 0.2) < 1.0);
        assert_eq!(wet_loss(1.0, v80 - 3.0, 0.2), 1.0);
        assert!((wet_loss(1.0, 1000.0, 0.2) - WET_KEPT).abs() < 1e-12);
    }

    #[test]
    fn a_bald_tyre_keeps_its_friction_at_standstill_and_loses_it_gradually() {
        // The road's texture drains under a bald tyre and keeps its lifted patch in contact: nothing lost at rest or
        // walking pace, a gradual fall (Dijks: 0.96 / 0.80 / 0.52 of new at 50 / 80 / 130 km/h on 0.5 mm).
        assert_eq!(wet_loss(1.0, 0.0, 0.0), 1.0);
        assert_eq!(wet_loss(1.0, 6.0, 0.0), 1.0);
        let at = |kmh: f64| wet_loss(1.0, kmh / 3.6, 0.0);
        assert!(at(50.0) > 0.85 && (0.65..0.8).contains(&at(80.0)) && (0.4..0.6).contains(&at(130.0)), "{}", at(80.0));
        assert!((wet_loss(1.0, 300.0, 0.0) - WET_KEPT).abs() < 1e-12);
        // Monotone in speed and in tread.
        for i in 0..80 {
            let v = i as f64;
            assert!(wet_loss(1.0, v + 1.0, 0.0) <= wet_loss(1.0, v, 0.0));
            assert!(wet_loss(1.0, v, 0.0) <= wet_loss(1.0, v, 0.2) && wet_loss(1.0, v, 0.2) <= wet_loss(1.0, v, 1.0));
        }
    }

    #[test]
    fn the_damp_level_is_whole_at_damp_and_none_dry_or_wet() {
        assert_eq!(damp_level(0.0), 1.0);
        assert_eq!(damp_level(1.0), 1.0);
        assert!((damp_level(DAMP_W) - (1.0 - DAMP_LOSS)).abs() < 1e-15);
        assert!((damp_level(0.125) - (1.0 - DAMP_LOSS / 2.0)).abs() < 1e-15);
        // NGP's levels (dry 1, damp 0.909, wet 0.788 of dry, lab) times the damp level: damp 0.82, and from damp to
        // wet the blend falls without a dip below wet.
        let ngp = |w: f64| if w <= DAMP_W { 1.0 - 0.091 * w / DAMP_W } else { 0.909 - 0.121 * (w - DAMP_W) / 0.75 };
        assert!((ngp(DAMP_W) * damp_level(DAMP_W) - 0.82).abs() < 0.001);
        let mut last = 1.0;
        for i in 0..=100 {
            let w = f64::from(i) / 100.0;
            let level = ngp(w) * damp_level(w);
            assert!(level <= last + 1e-12 && level >= 0.788 - 1e-12, "W {w}: {level}");
            last = level;
        }
    }

    #[test]
    fn the_wet_peak_falls_with_speed_as_reimpell_measured_and_a_worn_tread_keeps_its_loss_against_new() {
        let kmh = |v: f64| v / 3.6;
        // Reimpell & Sponagel at the game's 0.7 mm (W 1): 0.92 / 0.83 of the 60 km/h peak at 100 / 140 km/h (0.5-1
        // mm: 0.89-0.94, 0.78-0.87); 0.2 mm 0.99 / 0.97 against W 0.25's 0.175 mm (fig ±0.02).
        assert_eq!(wet_speed_factor(1.0, kmh(60.0), 1.0), 1.0);
        let (w100, w140) = (wet_speed_factor(1.0, kmh(100.0), 1.0), wet_speed_factor(1.0, kmh(140.0), 1.0));
        assert!((0.89..=0.94).contains(&w100) && (0.78..=0.87).contains(&w140), "{w100} {w140}");
        let (d100, d140) = (wet_speed_factor(0.25, kmh(100.0), 1.0), wet_speed_factor(0.25, kmh(140.0), 1.0));
        assert!((0.97..=1.0).contains(&d100) && (0.95..=0.99).contains(&d140), "{d100} {d140}");
        // Dry: nothing, bit for bit.
        assert_eq!(wet_speed_factor(0.0, 50.0, 0.2), 1.0);
        // A new tread takes the thin film alone; a worn one its partial hydroplaning against new on top.
        for v in [10.0, 20.0, 27.8, 40.0, 60.0] {
            assert!((wet_speed_factor(1.0, v, 1.0) - thin_film(1.0, v)).abs() < 1e-12);
            let worn = wet_speed_factor(1.0, v, 0.2) / wet_speed_factor(1.0, v, 1.0);
            assert!((worn - wet_loss(1.0, v, 0.2) / wet_loss(1.0, v, 1.0)).abs() < 1e-12);
        }
        assert!((thin_film(1.0, 1000.0) - WET_KEPT).abs() < 1e-12);
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

    #[test]
    fn the_lateral_relaxation_length_shortens_with_pressure() {
        // NGP's factor +6.9 % (2.0 to 2.5 bar): c_y +12 %, c_x +3.9 %, both unchanged at the calibration pressure.
        let (cal, pf) = (synth_cal(), 1.0667);
        assert_eq!((carcass_x(&cal, 1.0), carcass_y(&cal, 150e3, 1.0)), (CARCASS_X, 150e3));
        let (cy, cx) = (carcass_y(&cal, 150e3 * pf, pf) / 150e3, carcass_x(&cal, pf) / CARCASS_X);
        assert!((cy - 1.116).abs() < 0.002 && (cx - 1.0396).abs() < 0.001, "{cy} {cx}");
        // sigma_y = C_y/c_y: the slip stiffness moves with the factor, the carcass faster.
        assert!(pf / cy < 0.96);
    }

    /// The synthetic tyre's longitudinal carcass stiffness.
    fn c_x() -> f64 {
        carcass_x(&synth_cal(), 1.0)
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
        // Small slip: sigma_x = (Cx + V d_x)/c_x, C/c = Cx/CARCASS_X at the nominal load at any low speed (NGP's
        // torsional stiffness, a tenth below 10 m/s, made it ten times longer there).
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
            assert!((steady.kx / c / (synth_cal().cx0 / CARCASS_X) - 1.0).abs() < 0.03, "C/c {}", steady.kx / c);
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
