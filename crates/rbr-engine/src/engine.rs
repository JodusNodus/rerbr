//! The mean-value engine: its derived constants ([`Spec`]), its state and one step (Hendricks; Eriksson–Nielsen
//! MVEM as in Andersson 2017).

use crate::class::{Actuation, Class, Idle, Intake};
use crate::curve::Curve;
use crate::ecu::{self, AntiLag, Events};
use crate::friction::{self, Geometry};
use crate::gas::{self, AFR, LAMBDA, LHV, P_REF, R, T_REF};
use crate::turbo;
use libm::{exp, pow, sqrt};
use std::f64::consts::PI;

/// rpm per rad/s.
pub const RPM: f64 = 30.0 / PI;
/// Throttle discharge coefficients: one throttle 0.80, a throttle per cylinder 0.85 (0.8 from throttle-body fits; 0.85
/// guess).
const C_D_SINGLE: f64 = 0.80;
const C_D_PER_CYLINDER: f64 = 0.85;
/// A throttle per cylinder is at least 45 mm (guess).
const PER_CYLINDER_BORE: f64 = 0.045;
/// Manifold volume over displacement: one throttle 0.8, a throttle per cylinder 0.3, turbo after the throttle 1.0
/// (guess); the boost pipe 2.0 (guess).
const V_IM_SINGLE: f64 = 0.8;
const V_IM_PER_CYLINDER: f64 = 0.3;
const V_IM_TURBO: f64 = 1.0;
pub const V_BOOST: f64 = 2.0;
/// A turbo's volumetric efficiency (guess).
pub const ETA_V_TURBO: f64 = 0.95;
/// NA exhaust back pressure p_a + 0.15 bar·(W/W_rated)² (guess).
const NA_BACK_PRESSURE: f64 = 0.15e5;
/// The residual gas's ratio of specific heats in the ideal-cycle η_v (guess: the exhaust's 1.3 rather than air's 1.4).
const GAMMA_RESIDUAL: f64 = 1.3;
/// Below this speed (rad/s) friction and pumping fade to 0 with the speed's sign (inferred: no stiction).
const FRICTION_FADE: f64 = 5.0;

/// The engine's constants, derived from the car's params by the calibration.
#[derive(Clone, Debug)]
pub struct Spec {
    pub class: Class,
    pub geometry: Geometry,
    pub turbo: Option<turbo::Spec>,
    /// RPMLimit and IdleRPM, rpm.
    pub rpm_limit: f64,
    pub rpm_idle: f64,
    pub torque_cut: f64,
    /// C_D·A of the throttle(s) fully open, m², and the manifold's volume, m³.
    pub cda: f64,
    pub v_im: f64,
    /// η_v over rpm.
    pub eta_v: Curve,
    /// Throttle-closed areas as fractions of the full throttle: the leak, the idle air's feed-forward and ceiling.
    pub leak: f64,
    pub idle_air: f64,
    /// The control unit's overrun air while the overrun cut runs (0 without a control unit's idle).
    pub overrun_air: f64,
    pub idle_air_max: f64,
    /// The idle loops' gains: air (fraction per unit of relative speed error, and per second), spark (degrees).
    pub idle_kp: f64,
    pub idle_ki: f64,
    pub spark_kp: f64,
    /// The friction scale s_f at RPMLimit (see [`Self::friction`]).
    pub friction_scale: f64,
    /// K(n) over rpm, J per kg of air.
    pub k: Curve,
    /// The air flow at rated power, kg/s (NA back pressure).
    pub w_rated: f64,
}

impl Spec {
    /// The car's geometry and air path before calibration.
    pub fn new(class: Class, turbo: bool, v_d: f64, inlet_area: f64, rpm_limit: f64, rpm_idle: f64) -> Self {
        let geometry = Geometry::new(v_d, class.cylinders, class.compression);
        let (c_d, area, r_v) = match class.intake {
            Intake::PerCylinder => {
                let min = f64::from(class.cylinders) * PI / 4.0 * PER_CYLINDER_BORE * PER_CYLINDER_BORE;
                (C_D_PER_CYLINDER, inlet_area.max(min), V_IM_PER_CYLINDER)
            }
            Intake::Single => (C_D_SINGLE, inlet_area, if turbo { V_IM_TURBO } else { V_IM_SINGLE }),
        };
        Self {
            class,
            geometry,
            turbo: None,
            rpm_limit,
            rpm_idle,
            torque_cut: 1.0,
            cda: c_d * area,
            v_im: r_v * v_d,
            eta_v: Curve(vec![(0.0, if turbo { ETA_V_TURBO } else { 1.0 })]),
            leak: 0.0,
            idle_air: 0.0,
            overrun_air: 0.0,
            idle_air_max: gas::plate_area(ecu::IDLE_AIR_MAX_LIFT),
            idle_kp: 0.0,
            idle_ki: 0.0,
            spark_kp: 0.0,
            friction_scale: 1.0,
            k: Curve::default(),
            w_rated: 1.0,
        }
    }

    /// The residual gas's share of η_v, the ideal cycle's (r_c − (p_em/p_im)^(1/γ))/(r_c − 1) (Heywood; the
    /// pressure factor of arXiv 2012.02859's η_v): exhaust left in the cylinder at a low manifold pressure displaces
    /// fresh charge. 1 at p_em = p_im, at least 0.
    pub fn residual(&self, p: f64, p_em: f64) -> f64 {
        let r_c = self.geometry.r_c;
        ((r_c - pow(p_em.max(0.0) / p.max(1.0), 1.0 / GAMMA_RESIDUAL)) / (r_c - 1.0)).max(0.0)
    }

    /// The cylinders' air flow at manifold pressure `p` and exhaust pressure `p_em` (Pa), `rpm` and charge
    /// temperature `t`, kg/s.
    pub fn cylinder_flow(&self, p: f64, p_em: f64, rpm: f64, t: f64) -> f64 {
        self.charge(p, p_em, rpm, t) * rpm.max(0.0) / 120.0
    }

    /// The air trapped per cycle (all cylinders), kg.
    pub fn charge(&self, p: f64, p_em: f64, rpm: f64, t: f64) -> f64 {
        self.eta_v.at(rpm) * self.residual(p, p_em) * p * self.geometry.v_d / (R * t)
    }

    /// Friction, N·m (positive, against rotation), at `rpm`, viscosity scale `phi`, intake over ambient `p_ratio`:
    /// Sandoval's FMEP scaled by 1 + (s_f − 1)·(n/n_lim)², so s_f at the limit and Sandoval's own at low speed, where
    /// it meets road engines' fuel-cut braking (EPA; the share over Sandoval growing with n² is inferred).
    pub fn friction(&self, rpm: f64, phi: f64, p_ratio: f64) -> f64 {
        let x = rpm / self.rpm_limit;
        let scale = 1.0 + (self.friction_scale - 1.0) * x * x;
        scale * self.geometry.per_pa() * self.geometry.fmep(rpm, phi, p_ratio)
    }
}

/// One step's inputs, sanitised, in f64.
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub dt: f64,
    pub omega: f64,
    pub pedal: f64,
    pub shift_cut: bool,
    pub damage: f64,
    pub p_a: f64,
    pub t_a: f64,
    pub friction: f64,
    pub stalled: bool,
    pub t_oil: f64,
    /// The rev limiter acts (off only for the calibration's engine, whose table runs to the limit).
    pub limiter: bool,
}

impl Step {
    /// A fixed-speed step at the reference air and oil, for the calibration and the tests.
    pub fn at(omega: f64, pedal: f64, dt: f64) -> Self {
        Self {
            dt,
            omega,
            pedal,
            shift_cut: false,
            damage: 1.0,
            p_a: P_REF,
            t_a: T_REF,
            friction: 0.0,
            stalled: false,
            limiter: true,
            t_oil: friction::T_OIL_REF,
        }
    }

    /// The interface's inputs made safe: no NaN, infinity or negative step reaches the state.
    pub fn from_inputs(i: &crate::Inputs) -> Self {
        let finite = |v: f32, default: f64| if v.is_finite() { f64::from(v) } else { default };
        let dt = finite(i.dt, 0.0);
        Self {
            dt: if dt > 0.0 { dt.min(0.05) } else { 0.0 },
            omega: finite(i.speed, 0.0).clamp(-4000.0, 4000.0),
            pedal: finite(i.throttle, 0.0).clamp(0.0, 1.0),
            shift_cut: i.shift_cut,
            damage: finite(i.damage, 1.0).clamp(0.0, 1.0),
            p_a: if i.pressure > 0.0 { finite(i.pressure, P_REF).clamp(3e4, 1.2e5) } else { P_REF },
            t_a: finite(i.temperature, T_REF).clamp(200.0, 350.0),
            friction: finite(i.friction, 0.0).clamp(-1e4, 1e4),
            stalled: i.stalled,
            t_oil: f64::from(i.engine_temperature),
            limiter: true,
        }
    }
}

/// The engine's state between steps.
#[derive(Clone, Copy, Debug)]
pub struct State {
    /// Manifold and boost pipe pressure, Pa; the charge's temperature, K.
    pub p_im: f64,
    pub p_boost: f64,
    pub t_im: f64,
    /// The throttle plate's lift.
    pub plate: f64,
    /// The idle air loop's integral, the overrun air's share above the idle feed-forward (decaying once the overrun
    /// cut ends), the overrun cut and the firing events.
    pub idle_i: f64,
    pub overrun: f64,
    pub dfco: bool,
    pub events: Events,
    pub anti_lag: AntiLag,
    /// Turbo: shaft speed (rad/s), wastegate opening and its integral, exhaust temperature (K) and pressure (Pa),
    /// the compressor's last flow (kg/s).
    pub omega_t: f64,
    pub wastegate: f64,
    pub wastegate_i: f64,
    pub t_em: f64,
    pub p_em: f64,
    pub w_c: f64,
}

impl State {
    /// At rest in the reference air.
    pub fn new() -> Self {
        Self {
            p_im: P_REF,
            p_boost: P_REF,
            t_im: T_REF,
            plate: 0.0,
            idle_i: 0.0,
            overrun: 0.0,
            dfco: false,
            events: Events::new(),
            anti_lag: AntiLag::default(),
            omega_t: 0.0,
            wastegate: 0.0,
            wastegate_i: 0.0,
            t_em: 673.15,
            p_em: P_REF,
            w_c: 0.0,
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

/// What a step gives.
#[derive(Clone, Copy, Debug, Default)]
pub struct Out {
    /// Net crank torque and its parts, N·m.
    pub torque: f64,
    pub indicated: f64,
    pub friction: f64,
    pub pumping: f64,
    /// The share of firing events cut, the spark retard (°) and whether anti-lag runs.
    pub cut: f64,
    pub retard: f64,
    pub anti_lag: bool,
    /// The blow-off valve's opening at the step's end, 0..=1 (0 without a turbo), and the air's pressure, Pa.
    pub blow_off: f64,
    pub p_a: f64,
    /// The cylinders' air flow, kg/s, and the charge per cycle, kg.
    pub w_cyl: f64,
    pub charge: f64,
    /// The ECU's idle and overrun air, a share of the full throttle's area.
    pub idle_air: f64,
}

/// One implicit Euler step of dp/dt = f(p): the root of p' = p + h·f(p') between p and p + h·f(p) (stable and
/// monotone for any h when f falls with p, as every flow balance here does).
fn implicit(p: f64, h: f64, f: impl Fn(f64) -> f64) -> f64 {
    let explicit = p + h * f(p);
    if (explicit - p).abs() < 1e-6 {
        return explicit.max(100.0);
    }
    let x = crate::solve::root(p, explicit.max(100.0), |q| q - p - h * f(q), 1e-4, 1e-6 * p.abs());
    x.max(100.0)
}

/// One step of the engine at `x`: the state after it and its torque.
pub fn step(spec: &Spec, s: &mut State, x: &Step) -> Out {
    let dt = x.dt;
    let rpm = x.omega.max(0.0) * RPM;
    let pedal = x.pedal;
    let ecu_idle = spec.class.idle == Idle::Ecu;

    // Control unit: anti-lag, overrun cut, idle, spark, cuts.
    let anti_lag = match spec.turbo {
        Some(_) if spec.class.anti_lag => s.anti_lag.step(pedal, rpm, spec.rpm_limit, dt),
        _ => false,
    };
    if !ecu_idle || pedal >= ecu::PEDAL_CLOSED || rpm < spec.rpm_idle + ecu::DFCO_OFF {
        s.dfco = false;
    } else if rpm > spec.rpm_idle + ecu::DFCO_ON {
        s.dfco = true;
    }
    let dfco = s.dfco && !anti_lag;
    let error = (spec.rpm_idle - rpm) / spec.rpm_idle;
    // The overrun air at this speed (it holds OVERRUN_P·p_a): it holds the manifold while the fuel is cut, then hands
    // over to the idle loop by decaying.
    let overrun_air =
        spec.overrun_air * spec.eta_v.at(rpm) * rpm / (spec.eta_v.at(ecu::OVERRUN_RPM) * ecu::OVERRUN_RPM);
    if dfco {
        s.overrun = (overrun_air - spec.idle_air).max(0.0);
    } else {
        s.overrun *= exp(-dt / ecu::OVERRUN_TAU);
    }
    let idle_loop = ecu_idle && !dfco && pedal < ecu::PEDAL_CLOSED && rpm < spec.rpm_idle + ecu::IDLE_BAND;
    let idle_air = if idle_loop {
        // Anti-stall: the air above the feed-forward is whole down to idle − 300 rpm, then falls to none at half idle,
        // so an engine the clutch drags down (braking in gear) gets the air to catch a dip, not to push the car.
        let ceiling =
            spec.idle_air + (spec.idle_air_max - spec.idle_air).max(0.0) * ecu::anti_stall(rpm, spec.rpm_idle);
        // Over idle the loop closes the air down to the floor, not shut.
        let floor = (overrun_air * ecu::IDLE_FLOOR_P / ecu::OVERRUN_P).min(spec.idle_air).min(ceiling);
        let low = floor - spec.idle_air;
        let high = ceiling - spec.idle_air;
        // Conditional integration: above idle + 100 rpm (the overrun air still decaying, a fall towards idle) the
        // integral only bleeds what it gained, down to 0, so it neither winds down and undershoots nor holds a wind-up
        // from a clutch's bite above idle (inferred).
        let step = spec.idle_ki * error * dt;
        s.idle_i = if rpm < spec.rpm_idle + ecu::IDLE_INTEGRATE {
            s.idle_i + step
        } else if s.idle_i > 0.0 {
            (s.idle_i + step).max(0.0)
        } else {
            s.idle_i
        }
        .clamp(low, high);
        (spec.idle_air + spec.idle_kp * error + s.idle_i).clamp(floor, ceiling) + s.overrun
    } else {
        s.idle_i = 0.0;
        spec.idle_air + s.overrun
    };
    let retard = if anti_lag {
        ecu::ALS_RETARD
    } else if ecu_idle {
        let trim = if idle_loop { spec.spark_kp * error } else { 0.0 };
        (ecu::IDLE_RETARD - trim)
            .clamp(ecu::IDLE_RETARD - ecu::IDLE_ADVANCE_ROOM, ecu::IDLE_RETARD + ecu::IDLE_RETARD_ROOM)
    } else {
        0.0
    };
    // While the loop runs at idle the base retard holds the reserve; elsewhere (off idle) the spark is at MBT.
    let retard = if anti_lag || idle_loop { retard } else { 0.0 };
    let limiter = if x.limiter { ((rpm - spec.rpm_limit) / ecu::LIMITER_RANGE + 1.0).clamp(0.0, 1.0) } else { 0.0 };
    let mut cut = limiter;
    if x.shift_cut {
        cut = cut.max(1.0 - spec.torque_cut);
    }
    if anti_lag {
        cut = cut.max(ecu::ALS_CUT);
    }
    let fuel = !(dfco || x.stalled || (x.limiter && rpm > spec.rpm_limit + ecu::LIMITER_FUEL));

    // Throttle: the plate at the pedal (a cable, or a race DBW's 1:1 translation); a progressive linkage on cable
    // throttles per cylinder.
    let other = spec.leak + idle_air;
    let mut target = match (spec.class.actuation, spec.class.intake) {
        (Actuation::Cable, Intake::PerCylinder) => pow(pedal, 1.6),
        _ => pedal,
    };
    if anti_lag {
        target = target.max(gas::plate_lift(ecu::ALS_AREA));
    }
    s.plate = match spec.class.actuation {
        Actuation::Cable => target,
        Actuation::Dbw => ecu::etb(s.plate, target, dt),
    };
    let area = gas::plate_area(s.plate) + other;

    let boost_before = s.p_boost;
    // The exhaust pressure of the last step (it follows from this step's flow).
    let p_em = s.p_em;
    // Air path: boost pipe then manifold, sub-stepped when the manifold is fast.
    let eta_v = spec.eta_v.at(rpm);
    let tau = spec.v_im * 120.0 / (eta_v * spec.geometry.v_d * rpm.max(300.0));
    let n = ((3.0 * dt / tau).ceil() as usize).clamp(1, 16);
    let h = dt / n as f64;
    let mut compressor_power = 0.0;
    let mut blow_off = 0.0;
    for _ in 0..n {
        let t_im = s.t_im;
        if let Some(t) = &spec.turbo {
            let p_im = s.p_im;
            blow_off = if anti_lag {
                0.0
            } else {
                ((s.p_boost - p_im - turbo::BOV_OPEN) / (turbo::BOV_FULL - turbo::BOV_OPEN)).clamp(0.0, 1.0)
            };
            let bov = t.a_bov * blow_off;
            let rate = |p: f64| {
                let w_c = t.compressor(s.omega_t, p, x.t_a, x.p_a).0;
                let w_th = gas::orifice(spec.cda * area, p, p_im, t_im);
                let w_bov = gas::orifice(bov, p, x.p_a, t_im);
                R * t_im / t.v_boost * (w_c - w_th - w_bov)
            };
            s.p_boost = implicit(s.p_boost, h, rate);
            let (w_c, power, t_out) = t.compressor(s.omega_t, s.p_boost, x.t_a, x.p_a);
            s.w_c = w_c;
            compressor_power += power * h;
            s.t_im = turbo::intercooled(t_out, x.t_a);
        } else {
            s.t_im = x.t_a;
        }
        let p_up = if spec.turbo.is_some() { s.p_boost } else { x.p_a };
        let t_up = s.t_im;
        let rate = |p: f64| {
            R * t_up / spec.v_im
                * (gas::orifice(spec.cda * area, p_up, p, t_up) - spec.cylinder_flow(p, p_em, rpm, t_up))
        };
        s.p_im = implicit(s.p_im, h, rate);
    }
    let w_cyl = spec.cylinder_flow(s.p_im, p_em, rpm, s.t_im);
    let charge = spec.charge(s.p_im, p_em, rpm, s.t_im);

    // Firing events and indicated torque.
    let fired = s.events.step(f64::from(spec.class.cylinders) * rpm / 120.0 * dt, cut);
    let combustion = fuel && !x.stalled && rpm >= ecu::FIRE_RPM;
    let spark = ecu::spark_efficiency(retard);
    let indicated = if combustion { spec.k.at(rpm) * charge / (4.0 * PI) * spark * fired * x.damage } else { 0.0 };

    // Exhaust: NA back pressure, or the turbo.
    let trim = if anti_lag { 1.0 + ecu::ALS_TRIM } else { 1.0 };
    let w_fuel = if fuel { w_cyl * trim / (LAMBDA * AFR) } else { 0.0 };
    let w_exhaust = w_cyl + w_fuel;
    if let Some(t) = &spec.turbo {
        // The cylinders that fire leave the line's temperature, and the work their retard takes partly heats the
        // exhaust; those that do not leave the air motored, and their fuel partly burns in the manifold (anti-lag,
        // the cuts).
        let burnt = if combustion { fired } else { 0.0 };
        let heat = |power: f64| if w_exhaust > 0.0 { power / (w_exhaust * crate::gas::CP_EXHAUST) } else { 0.0 };
        let retard_heat = heat(burnt * (1.0 - spark) * spec.k.at(rpm) * w_cyl * turbo::RETARD_EXHAUST);
        let unburnt = heat((1.0 - burnt) * w_fuel * LHV * ecu::ALS_BURN);
        let line = t.t_em0 + t.t_em_k * w_exhaust;
        let t_ss =
            (turbo::T_EM_MOTORED + burnt * (line - turbo::T_EM_MOTORED) + retard_heat + unburnt).min(turbo::T_EM_MAX);
        s.t_em += (t_ss - s.t_em) * (1.0 - exp(-dt / turbo::TAU_EXHAUST));
        // The electronic gate's aim rises linearly with the throttle's aim position to the WOT boost at the position
        // where the plate stops controlling the air (`aim_open`), as race ECUs' boost aim tables over rpm and throttle
        // reach full aim before full throttle (shape inferred; the aim, not the plate, so a re-apply after anti-lag
        // does not dump the boost while the plate opens); a spring gate aims at the WOT boost whatever the pedal.
        let wot = t.pi_wot.at(rpm) * x.p_a;
        let aim = if anti_lag {
            x.p_a + ecu::ALS_BOOST
        } else if spec.class.actuation == Actuation::Dbw {
            let open = t.aim_open.at(rpm);
            x.p_a + (wot - x.p_a) * if open > 0.0 { (target / open).min(1.0) } else { target }
        } else {
            wot
        };
        let electronic = spec.class.actuation == Actuation::Dbw || anti_lag;
        let e = (s.p_boost - aim) / turbo::WASTEGATE_BAND;
        let command = if electronic {
            // The electronic gate's base opening at this speed (the steady WOT one) plus PI, as boost controllers
            // run a base duty table with closed-loop trim; the base pre-opens the gate as the boost comes up.
            let base = if anti_lag { 0.0 } else { t.gate_base.at(rpm) };
            // Conditional integration: the integral holds while the command is saturated the way it would grow.
            // A derivative on the boost (not the error, so no kick from the aim) opens the gate as the boost
            // rises fast, before it overshoots.
            let d = if dt > 0.0 {
                turbo::WASTEGATE_TD * (s.p_boost - boost_before) / (dt * turbo::WASTEGATE_BAND)
            } else {
                0.0
            };
            let raw = base + e + d + s.wastegate_i;
            if !((raw <= 0.0 && e < 0.0) || (raw >= 1.0 && e > 0.0)) {
                s.wastegate_i = (s.wastegate_i + e * dt / turbo::WASTEGATE_TI).clamp(-1.0, 1.0);
            }
            (base + e + d + s.wastegate_i).clamp(0.0, 1.0)
        } else {
            ((s.p_boost - aim) / turbo::SPRING_BAND).clamp(0.0, 1.0)
        };
        // The electronic gate moves through its actuator; the spring gate's diaphragm follows the boost directly.
        let tau = if electronic { turbo::TAU_WASTEGATE } else { turbo::TAU_SPRING };
        s.wastegate += (command - s.wastegate) * (1.0 - exp(-dt / tau));
        let (p_em, power) = t.turbine(w_exhaust, s.t_em, s.wastegate, x.p_a);
        s.p_em = p_em;
        let energy = 0.5 * turbo::INERTIA * s.omega_t * s.omega_t
            + (power * dt - compressor_power - turbo::losses(s.omega_t) * dt);
        s.omega_t = sqrt(2.0 * energy.max(0.0) / turbo::INERTIA);
    } else {
        let r = w_exhaust / spec.w_rated;
        s.p_em = x.p_a + NA_BACK_PRESSURE * r * r;
        s.p_boost = x.p_a;
    }

    // Friction and pumping.
    let fade = (x.omega / FRICTION_FADE).clamp(-1.0, 1.0);
    let phi = friction::phi(x.t_oil);
    let p_ratio = s.p_im / x.p_a;
    let fric = spec.friction(rpm, phi, p_ratio) * fade;
    let pumping = spec.geometry.per_pa() * spec.geometry.pmep(s.p_em, s.p_im, x.p_a, rpm) * fade;
    // The 1.02 sim's friction opposes the motion, as NGP applies it (`direction * friction_torque`): unsigned, an
    // engine dragged backwards would be pushed on backwards by it.
    let torque = indicated - fric - pumping - x.friction * fade;
    Out {
        torque,
        indicated,
        friction: fric,
        pumping,
        cut: if combustion { 1.0 - fired } else { 1.0 },
        retard,
        anti_lag,
        blow_off,
        p_a: x.p_a,
        w_cyl,
        charge,
        idle_air,
    }
}
