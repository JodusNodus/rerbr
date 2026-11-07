//! Air and exhaust gas: the constants, the compressible orifice and the throttle plate's open area
//! (`notes/engine-model.md` "Air path").

use libm::{acos, cos, pow, sqrt};

/// The gas constant of air, J/(kg·K).
pub const R: f64 = 287.0;
/// c_p of air, J/(kg·K).
pub const CP_AIR: f64 = 1005.0;
/// (γ − 1)/γ of air, γ = 1.4.
pub const KAPPA_AIR: f64 = 0.4 / 1.4;
/// c_p of exhaust gas, J/(kg·K), and its (γ − 1)/γ, γ = 1.33 (Heywood's burned-gas values).
pub const CP_EXHAUST: f64 = 1150.0;
pub const KAPPA_EXHAUST: f64 = 0.33 / 1.33;
/// The reference air the calibration runs in: 101325 Pa, 293.15 K.
pub const P_REF: f64 = 101_325.0;
pub const T_REF: f64 = 293.15;
/// Gasoline's lower heating value, J/kg, the stoichiometric air-fuel ratio and the full-load λ (generic; λ:
/// x-engineer).
pub const LHV: f64 = 43.4e6;
pub const AFR: f64 = 14.7;
pub const LAMBDA: f64 = 0.88;

/// The critical pressure ratio (2/(γ+1))^(γ/(γ−1)), γ = 1.4: below it the orifice is choked.
pub const PI_CHOKED: f64 = 0.528_281_787_717_174_2;
/// Ψ at and below [`PI_CHOKED`].
pub const PSI_CHOKED: f64 = 0.684_731_456_377_270_5;
/// Above this ratio Ψ is a line to 0 at Π = 1, so its slope stays finite near equal pressures (inferred).
const PI_LINEAR: f64 = 0.98;
/// Ψ(0.98).
const PSI_LINEAR: f64 = 0.197_845_550_570_656;
/// The closed plate's angle, degrees (US 6591667), and its cosine.
const THETA_CLOSED: f64 = 7.0;
const COS_CLOSED: f64 = 0.992_546_151_641_322;

/// Ψ(Π) from the isentropic orifice equation, γ = 1.4 (Andersson 2017 eq. 3.4–3.5).
fn psi_isentropic(pi: f64) -> f64 {
    sqrt((7.0 * (pow(pi, 1.0 / 0.7) - pow(pi, 2.4 / 1.4))).max(0.0))
}

/// Ψ(Π) for Π = p_down/p_up in 0..=1: choked below 0.528, isentropic to 0.98, then linear to 0 at Π = 1.
pub fn psi(pi: f64) -> f64 {
    if pi <= PI_CHOKED {
        PSI_CHOKED
    } else if pi < PI_LINEAR {
        psi_isentropic(pi)
    } else {
        (PSI_LINEAR * (1.0 - pi) / (1.0 - PI_LINEAR)).max(0.0)
    }
}

/// The mass flow through an orifice of effective area `cda` (m²) from `p_up` to `p_down` (Pa) at the upstream
/// temperature `t` (K), kg/s; negative when the flow runs back.
pub fn orifice(cda: f64, p_up: f64, p_down: f64, t: f64) -> f64 {
    if !(p_up > 0.0 && p_down > 0.0) {
        return 0.0;
    }
    if p_down <= p_up {
        cda * p_up / sqrt(R * t) * psi(p_down / p_up)
    } else {
        -cda * p_down / sqrt(R * t) * psi(p_up / p_down)
    }
}

/// The upstream pressure (Pa) that drives `flow` (kg/s) through `cda` (m²) into `p_down` at temperature `t`: the
/// inverse of [`orifice`] in its upstream pressure (closed form when choked, else bisection).
pub fn upstream_pressure(cda: f64, flow: f64, p_down: f64, t: f64) -> f64 {
    if flow <= 0.0 || cda <= 0.0 {
        return p_down;
    }
    // Ψ(Π)/Π = flow·√(RT)/(cda·p_down), decreasing in Π.
    let c = flow * sqrt(R * t) / (cda * p_down);
    if c >= PSI_CHOKED / PI_CHOKED {
        return flow * sqrt(R * t) / (cda * PSI_CHOKED);
    }
    let pi = crate::solve::root(PI_CHOKED, 1.0, |pi| psi(pi) / pi - c, 1e-9 * c, 0.0);
    p_down / pi
}

/// The open area of a butterfly as a fraction of the bore, 1 − cos θ/cos θ_cl with θ = 7° + 83°·`lift` (US 6591667,
/// `notes/engine-model.md`).
pub fn plate_area(lift: f64) -> f64 {
    let theta = (THETA_CLOSED + (90.0 - THETA_CLOSED) * lift.clamp(0.0, 1.0)).to_radians();
    1.0 - cos(theta) / COS_CLOSED
}

/// The lift that opens `area` (a fraction of the bore): the inverse of [`plate_area`].
pub fn plate_lift(area: f64) -> f64 {
    let theta = acos(((1.0 - area.clamp(0.0, 1.0)) * COS_CLOSED).clamp(-1.0, 1.0)).to_degrees();
    ((theta - THETA_CLOSED) / (90.0 - THETA_CLOSED)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_constants_are_their_formulas() {
        assert!((PI_CHOKED - pow(2.0 / 2.4, 3.5)).abs() < 1e-15);
        assert!((PSI_CHOKED - psi_isentropic(PI_CHOKED)).abs() < 1e-12);
        assert!((PSI_LINEAR - psi_isentropic(PI_LINEAR)).abs() < 1e-12);
        assert!((COS_CLOSED - cos(THETA_CLOSED.to_radians())).abs() < 1e-15);
        assert!((KAPPA_AIR - 0.285_714_285_714).abs() < 1e-9);
    }

    #[test]
    fn psi_is_choked_low_and_zero_at_one() {
        assert_eq!(psi(0.3), PSI_CHOKED);
        assert!(psi(1.0).abs() < 1e-12);
        assert!((psi(PI_LINEAR - 1e-9) - psi(PI_LINEAR)).abs() < 1e-6);
    }

    #[test]
    fn the_orifice_runs_both_ways_and_inverts() {
        let w = orifice(1e-3, 2e5, 1e5, 300.0);
        assert!(w > 0.0);
        assert_eq!(orifice(1e-3, 1e5, 2e5, 300.0), -w);
        for p in [1.01e5, 1.3e5, 2.5e5, 4e5] {
            let w = orifice(1e-3, p, 1e5, 900.0);
            assert!((upstream_pressure(1e-3, w, 1e5, 900.0) - p).abs() < 1.0, "{p}");
        }
    }

    #[test]
    fn the_plate_lift_inverts_its_area() {
        assert!(plate_area(0.0).abs() < 1e-12);
        assert!((plate_area(1.0) - 1.0).abs() < 1e-12);
        for lift in [0.0, 0.05, 0.3, 0.7, 1.0] {
            assert!((plate_lift(plate_area(lift)) - lift).abs() < 1e-9);
        }
    }
}
