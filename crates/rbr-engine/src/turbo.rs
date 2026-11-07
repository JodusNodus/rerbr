//! The turbocharger: a normalised compressor map, the turbine and wastegate nozzles, the shaft, the exhaust
//! temperature and the restrictor (Andersson 2017 / Eriksson–Nielsen MVEM).

use crate::curve::Curve;
use crate::gas::{self, CP_AIR, CP_EXHAUST, KAPPA_AIR, KAPPA_EXHAUST, PSI_CHOKED, P_REF, R, T_REF};
use libm::{pow, sqrt};

/// The speed line's droop a and the flow of its peak w0, of W/(W*·ν) (normalised map after GT-Power; guess).
const DROOP: f64 = 0.6;
const W0: f64 = 0.7;
const SURGE_BAND: f64 = 0.05;
/// Compressor efficiency: peak 0.75 at 0.8 W* and 0.9 Π*, 0.6 at the map's edges, at least 0.4 (guess).
const ETA_C_MAX: f64 = 0.75;
const ETA_C_EDGE: f64 = 0.15;
const ETA_C_MIN: f64 = 0.4;
/// The design shaft speed, rad/s (guess).
pub const OMEGA_STAR: f64 = 15_000.0;
/// The turbine's efficiency, turbine and bearings' mechanical losses apart (guess).
pub const ETA_T: f64 = 0.65;
/// Shaft inertia, kg·m² (Chalmers T45 1.16e-5, GT1548 5.5e-6).
pub const INERTIA: f64 = 1e-5;
/// Bearing losses: a·ω + b·ω² + 100 W static (rF2 `TurboFriction`, `TurboStaticFrictionWatts`).
const LOSS_A: f64 = 0.002;
const LOSS_B: f64 = 2.5e-6;
const LOSS_STATIC: f64 = 100.0;
/// The intercooler's effectiveness (guess).
pub const INTERCOOLER: f64 = 0.7;
/// The restrictor's discharge coefficient and its loss k_r at choke: p_in = p_a·(1 − k_r·(W/W_choke)²) (guess;
/// k_r inferred: a venturi recovers most of its throat's drop).
const RESTRICTOR_CD: f64 = 0.95;
const RESTRICTOR_LOSS: f64 = 0.2;
/// Exhaust temperature: first order 0.3 s; 950 °C at the rated flow (guess), 700 °C at no flow (inferred: from
/// 400 °C the line's slope gave the steady WOT boost two branches; `notes/engine-model.md`), at most
/// 1150 °C (MoTeC anti-lag limits).
pub const TAU_EXHAUST: f64 = 0.3;
const T_EM_IDLE: f64 = 973.15;
const T_EM_RATED: f64 = 1223.15;
pub const T_EM_MAX: f64 = 1423.15;
/// The wastegate: the electronic gate's actuator, s (Gorzelic 2012, 125 ms for an electronically commanded gate); its
/// proportional band, Pa (fully open 1 bar over the aim, around its base opening), integral time and derivative time
/// on the boost, s; the spring gate's band (fully open 0.5 bar over its aim) and its lag, s. Bands and times guessed:
/// the narrowest band the samples hold without a limit cycle through the turbo, and the derivative time that keeps a
/// tip-in within 5 % overshoot, settled in 1 s (tuning practice; 0.05–0.12 s all do at 3000 and 5000 rpm, 0.08 the
/// least overshoot). A spring gate's diaphragm feels the boost through its hose, with no solenoid or motor: the
/// pressure crosses 3 m of 6 mm hose within 10 ms (Thomasson 2014), the dome fills in a few ms (inferred: a 60 cm³
/// dome through a 3 mm port follows a 7 bar/s boost rise about 2 kPa, 3 ms, behind) and the valve's spring and mass
/// ring at about 60 Hz (inferred), so 10 ms. Spring gates spike about 1 psi for a split second (enginebasics); the
/// cable_turbo sample's fixed-speed tip-in spikes 0.4–0.5 psi at 5000–6500 rpm (2.2–2.9 psi with the 125 ms lag).
pub const TAU_WASTEGATE: f64 = 0.125;
pub const WASTEGATE_BAND: f64 = 1.0e5;
pub const WASTEGATE_TI: f64 = 0.5;
pub const WASTEGATE_TD: f64 = 0.08;
pub const SPRING_BAND: f64 = 0.5e5;
pub const TAU_SPRING: f64 = 0.01;
/// The plate's pressure ratio p_im/p_b where it stops controlling the air (inferred).
pub const AIM_OPEN_RATIO: f64 = 0.95;
/// The blow-off valve opens over 0.3–0.4 bar across the throttle and empties the boost pipe in about 0.1 s (inferred).
pub const BOV_OPEN: f64 = 0.3e5;
pub const BOV_FULL: f64 = 0.4e5;
const BOV_EMPTY: f64 = 0.1;

/// One car's turbo, from the calibration.
#[derive(Clone, Debug, Default)]
pub struct Spec {
    /// The boost target over rpm, absolute over the reference atmosphere.
    pub pi_wot: Curve,
    /// The design point: the compressor's pressure ratio and flow (kg/s).
    pub pi_star: f64,
    pub w_star: f64,
    /// The design shaft speed, rad/s.
    pub omega_star: f64,
    /// The turbine's nozzle and the wastegate's full area, m².
    pub a_turbine: f64,
    pub a_wastegate: f64,
    /// The electronic wastegate's base opening over rpm (its steady WOT opening; empty for a spring gate).
    pub gate_base: Curve,
    /// The electronic gate's full-aim throttle position over rpm: where the plate, at the steady WOT boost and flow,
    /// holds the manifold at `AIM_OPEN_RATIO` of the boost (empty for a spring gate).
    pub aim_open: Curve,
    /// The blow-off valve's area, m².
    pub a_bov: f64,
    /// The restrictor's choked flow at the reference air, kg/s (infinite without one).
    pub w_choke: f64,
    /// The boost pipe's volume, m³ (2·V_d).
    pub v_boost: f64,
    /// Exhaust temperature T0 + k·W (K, K·s/kg).
    pub t_em0: f64,
    pub t_em_k: f64,
}

impl Spec {
    /// The restrictor's choked flow for a bore `d` (m) at the reference air.
    pub fn choke(d: Option<f64>) -> f64 {
        match d {
            Some(d) => RESTRICTOR_CD * std::f64::consts::PI / 4.0 * d * d * P_REF / sqrt(R * T_REF) * PSI_CHOKED,
            None => f64::INFINITY,
        }
    }

    /// The exhaust temperature line through 700 °C at no flow and 950 °C at `w_rated` kg/s.
    pub fn exhaust_line(w_rated: f64) -> (f64, f64) {
        (T_EM_IDLE, (T_EM_RATED - T_EM_IDLE) / w_rated.max(1e-6))
    }

    /// The blow-off valve's area: empties the pipe (`v_boost` at π_max) in 0.1 s.
    pub fn bov_area(v_boost: f64, pi_max: f64) -> f64 {
        let p = pi_max.max(1.1) * P_REF;
        let mass = p * v_boost / (R * T_REF);
        mass / BOV_EMPTY / (p / sqrt(R * T_REF) * gas::psi(1.0 / pi_max.max(1.1)))
    }

    /// The compressor's inlet pressure behind the restrictor at the flow `w` and ambient `p_a`.
    pub fn inlet(&self, w: f64, p_a: f64) -> f64 {
        if self.w_choke.is_finite() {
            let r = (w / (self.w_choke * p_a / P_REF)).min(1.0);
            p_a * (1.0 - RESTRICTOR_LOSS * r * r)
        } else {
            p_a
        }
    }

    /// The speed line's flow (kg/s) at shaft speed ratio `nu` and pressure ratio `ratio`: Π − 1 = (Π* − 1)·(ν² −
    /// a·(w − w0·ν)²), w = W/W*, the fan laws' W ∝ ν (§5.1 eq. 3.31), its choke side; past the line's peak (surge) the
    /// flow falls to 0 over 5 % of the design rise (continuous, so the boost pipe's balance has one root; inferred).
    fn line(&self, nu: f64, ratio: f64) -> f64 {
        let q = (ratio - 1.0) / (self.pi_star - 1.0).max(0.05);
        let w = if q <= nu * nu {
            nu * W0 + sqrt((nu * nu - q) / DROOP)
        } else {
            nu * W0 * (1.0 - (q - nu * nu) / SURGE_BAND).max(0.0)
        };
        w * self.w_star
    }

    /// The compressor at shaft speed `omega` (rad/s) delivering into `p_out` (Pa) from the ambient `p_a` at `t_in`
    /// (K) through the restrictor: its flow (kg/s, at most the restrictor's choke; the restrictor's loss and the speed
    /// line solved together by bisection on the flow), the power it takes (W) and its outlet temperature (K).
    pub fn compressor(&self, omega: f64, p_out: f64, t_in: f64, p_a: f64) -> (f64, f64, f64) {
        let nu = omega / self.omega_star;
        let cap = self.w_choke * p_a / P_REF;
        let w = if cap.is_finite() {
            if self.line(nu, p_out / self.inlet(cap, p_a)) >= cap {
                cap
            } else {
                crate::solve::root(0.0, cap, |w| w - self.line(nu, p_out / self.inlet(w, p_a)), 1e-7 * cap, 0.0)
            }
        } else {
            self.line(nu, p_out / p_a)
        };
        if w <= 0.0 {
            return (0.0, 0.0, t_in);
        }
        let ratio = p_out / self.inlet(w, p_a);
        // The efficiency ellipse over flow only: with its pressure axis (0.6 at Π = 1) the steady WOT boost had two
        // branches (`notes/engine-model.md`).
        let x = (w / self.w_star - 0.8) / 0.8;
        let eta = (ETA_C_MAX - ETA_C_EDGE * x * x).max(ETA_C_MIN);
        let dh = CP_AIR * t_in * (pow(ratio, KAPPA_AIR) - 1.0);
        let power = if dh > 0.0 { w * dh / eta } else { w * dh * eta };
        (w, power, t_in + dh.max(0.0) / (CP_AIR * eta))
    }

    /// The exhaust manifold pressure that passes `w` (kg/s) at `t_em` (K) through the turbine and the wastegate open
    /// `wastegate` (0..=1) into `p_a`, and the turbine's power (W).
    pub fn turbine(&self, w: f64, t_em: f64, wastegate: f64, p_a: f64) -> (f64, f64) {
        let area = self.a_turbine + wastegate.clamp(0.0, 1.0) * self.a_wastegate;
        let p_em = gas::upstream_pressure(area, w, p_a, t_em);
        let w_t = w * self.a_turbine / area;
        let power = ETA_T * w_t * CP_EXHAUST * t_em * (1.0 - pow(p_a / p_em, KAPPA_EXHAUST));
        (p_em, power.max(0.0))
    }
}

/// The shaft's bearing losses at `omega`, W.
pub fn losses(omega: f64) -> f64 {
    LOSS_STATIC + LOSS_A * omega + LOSS_B * omega * omega
}

/// The compressor's outlet temperature after the intercooler, K.
pub fn intercooled(t_out: f64, t_a: f64) -> f64 {
    t_out - INTERCOOLER * (t_out - t_a)
}

/// The steady compressor outlet temperature at ratio `pi` and efficiency 0.75 after the intercooler, for the
/// calibration's estimates.
pub fn intercooled_at(pi: f64, t_a: f64) -> f64 {
    intercooled(t_a * (1.0 + (pow(pi.max(1.0), KAPPA_AIR) - 1.0) / ETA_C_MAX), t_a)
}
