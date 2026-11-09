//! The calibration: from the car's params to the engine's [`Spec`], at the reference air and oil, by running the
//! model's own step to steady states (`notes/engine-model.md`), and its report.

use crate::class::{self, Actuation, Class, Idle};
use crate::curve::Curve;
use crate::ecu;
use crate::engine::{self, Out, Spec, State, Step, ETA_V_TURBO, RPM, V_BOOST};
use crate::gas::{self, AFR, LAMBDA, LHV, PSI_CHOKED, P_REF, R, T_REF};
use crate::turbo;
use crate::Params;
use libm::{exp, log};
use std::f64::consts::PI;

/// The calibration's step, s (NGP's).
pub const DT: f64 = 1.0 / 720.0;
/// The boost target's assumptions: η_ig 0.38, η_v 0.95, and the cap without a class one (η_ig 0.38 a textbook example;
/// guess).
const PI_ETA_IG: f64 = 0.38;
const PI_CAP: f64 = 2.5;
/// The restrictor's flow margin (inferred: the boost target stays just under the restrictor's choke).
const RESTRICTOR_MARGIN: f64 = 0.95;
/// The turbine nozzle's search range, m², and the wastegate port in nozzles.
const TURBINE_MIN: f64 = 5e-5;
const TURBINE_MAX: f64 = 1e-2;
const WASTEGATE_PORTS: f64 = 3.0;
/// The largest exhaust over intake manifold pressure at steady WOT (inferred: matched turbos run 1–2; a smaller
/// nozzle buys low-rpm boost with back pressure no real turbine is sized for).
const DRIVE_RATIO_MAX: f64 = 2.0;
/// A steady WOT boost at least this share of its target is on boost.
const ON_BOOST: f64 = 0.98;
/// K's grid between the knots, rpm (inferred: the turbo's boost rises faster between knots than K could follow
/// linearly from the knots alone).
const GRID: f64 = 250.0;
/// The implied η_ig·η_v's plausible band (guess).
const ETA_BAND: (f64, f64) = (0.30, 0.55);
/// A turbo's friction scale at most: Sandoval's engine itself. NGP's braking (BrakeCoefficient·MaxTorque) grows with the
/// boosted torque, friction does not: every installed turbo's fit is 2.07–4.39, and at 2 the Rally2's friction and
/// pumping at 6000 rpm (5.1 bar) were over the 3.8 its restrictor leaves, its closed pedal over EPA's road engines'
/// (turbos among them) and its friction's growth over the published 2.1–2.7 (`notes/engine-model.md`).
const TURBO_FRICTION_MAX: f64 = 1.0;
/// The fuel–air cycle's indicated efficiency at φ 1/λ = 1.136: 0.344 at r_c 6, 0.383 at 8, 0.400 at 9.5 and 0.419
/// at 11 (Heywood fig. 5-9, p_1 1 atm, T_1 388 K, 5 % residual), the most any engine converts.
const ETA_FUEL_AIR: [(f64, f64); 4] = [(6.0, 0.344), (8.0, 0.383), (9.5, 0.400), (11.0, 0.419)];

/// The most work per kg of air K may give at compression ratio `r_c`, J/kg: the fuel–air cycle's efficiency on the
/// fuel at λ, LHV/(AFR·λ) per kg of air.
pub fn k_max(r_c: f64) -> f64 {
    Curve(ETA_FUEL_AIR.to_vec()).at(r_c) * LHV / (AFR * LAMBDA)
}

/// What the calibration found, for the curves, the tests and the log.
#[derive(Clone, Debug, Default)]
pub struct Calibration {
    pub class: Option<Class>,
    /// Displacement (m³) and bore = stroke (mm).
    pub displacement: f64,
    pub bore: f64,
    /// The friction scale and its value before the clamp (0.6–2.0, a turbo's at most 1).
    pub friction_scale: f64,
    pub friction_scale_raw: f64,
    /// The leak and the idle air, fractions of the throttle's area; the idle loops' gains.
    pub leak: f64,
    pub idle_air: f64,
    pub overrun_air: f64,
    pub idle_kp: f64,
    pub idle_ki: f64,
    pub spark_kp: f64,
    /// The idle's steady net torque with the idle air at 0 (positive: the leak alone runs it faster), N·m.
    pub idle_surplus: f64,
    /// K at each knot, rpm and J/kg.
    pub k: Vec<(f64, f64)>,
    /// Turbo: the boost target at each knot, the turbine's nozzle (m²), the design flow (kg/s) and ratio.
    pub pi_wot: Vec<(f64, f64)>,
    pub turbine_area: f64,
    pub design_flow: f64,
    pub design_ratio: f64,
    /// η_ig·η_v implied at the torque peak.
    pub eta_ig_v: f64,
    /// Steady WOT at each knot without the rev limiter: rpm, the table's torque and the model's (N·m).
    pub wot: Vec<(f64, f64, f64)>,
    /// Turbo: the grid points (rpm) where the boost misses its target and K is held, with the table's torque and the
    /// model's (N·m); the model falls short of the table there.
    pub held: Vec<(f64, f64, f64)>,
    /// Anything outside its expected range.
    pub flags: Vec<String>,
}

/// The params made usable: physical fallbacks where a value is missing or not finite.
struct Car {
    knots: Vec<(f64, f64)>,
    /// The knots and every 250 rpm between them, with the table's torque: where K is calibrated.
    grid: Vec<(f64, f64)>,
    max_torque: f64,
    rpm_limit: f64,
    rpm_idle: f64,
    brake: f64,
    v_d: f64,
    inlet: f64,
    torque_cut: f64,
}

fn positive(v: f32, default: f64) -> f64 {
    if v.is_finite() && v > 0.0 {
        f64::from(v)
    } else {
        default
    }
}

impl Car {
    fn new(p: &Params) -> Self {
        let max_torque = positive(p.max_torque, 200.0);
        let mut knots: Vec<(f64, f64)> = p
            .torque_knots
            .iter()
            .filter(|k| k.0.is_finite() && k.1.is_finite() && k.0 >= 0.0)
            .map(|&(w, t)| (f64::from(w) * RPM, f64::from(t).max(0.0) * max_torque))
            .collect();
        knots.dedup_by(|b, a| b.0 <= a.0);
        if knots.is_empty() {
            knots.push((0.0, max_torque));
        }
        // Inferred: V_d from MaxTorque at 125 N·m/L (NA) or 200 (turbo) without a volume.
        let v_d = 4.0 * positive(p.cylinder_volume, max_torque / if p.turbo { 200.0 } else { 125.0 } / 4000.0);
        let table = Curve(knots.clone());
        let (first, last) = (knots[0].0, knots[knots.len() - 1].0);
        let mut grid: Vec<(f64, f64)> = knots.clone();
        let mut rpm = (first / GRID).floor() * GRID + GRID;
        while rpm < last {
            grid.push((rpm, table.at(rpm)));
            rpm += GRID;
        }
        grid.sort_by(|a, b| a.0.total_cmp(&b.0));
        grid.dedup_by(|b, a| b.0 - a.0 < 1.0);
        Self {
            knots,
            grid,
            max_torque,
            rpm_limit: positive(p.rev_limit, 7000.0 / RPM) * RPM,
            rpm_idle: positive(p.idle_speed, 1000.0 / RPM) * RPM,
            brake: positive(p.brake_coefficient, 0.25),
            v_d,
            inlet: positive(p.inlet_area, 0.0018),
            torque_cut: if p.torque_cut.is_finite() { f64::from(p.torque_cut).clamp(0.0, 1.0) } else { 1.0 },
        }
    }
}

/// Runs the engine at a fixed `rpm` and `pedal` for `seconds` from `state`; the last step's output.
pub fn settle(spec: &Spec, state: &mut State, rpm: f64, pedal: f64, seconds: f64) -> Out {
    run(spec, state, &Step::at(rpm / RPM, pedal, DT), seconds)
}

fn run(spec: &Spec, state: &mut State, x: &Step, seconds: f64) -> Out {
    let mut out = Out::default();
    for _ in 0..(seconds / DT).ceil() as usize {
        out = engine::step(spec, state, x);
    }
    out
}

/// Runs the engine at a fixed `rpm` and `pedal` from `state` until it is steady: the pressures and the turbo's
/// speed move less than 1e-4 of themselves (and 1 Pa, 10 rad/s) over 0.25 s (at least 0.5 s, at most 30 s).
pub fn steady(spec: &Spec, state: &mut State, rpm: f64, pedal: f64) -> Out {
    steady_at(spec, state, &Step::at(rpm / RPM, pedal, DT))
}

fn steady_at(spec: &Spec, state: &mut State, x: &Step) -> Out {
    let mut out = run(spec, state, x, 0.25);
    for _ in 0..120 {
        let before = *state;
        out = run(spec, state, x, 0.25);
        let moved = |a: f64, b: f64, floor: f64| (a - b).abs() > 1e-4 * a.abs() + floor;
        if !(moved(state.p_im, before.p_im, 1.0)
            || moved(state.p_boost, before.p_boost, 1.0)
            || moved(state.omega_t, before.omega_t, 10.0))
        {
            break;
        }
    }
    out
}

/// The steady closed pedal at `rpm` with the fuel cut (no combustion; ECU cars hold their overrun air).
pub fn closed(spec: &Spec, rpm: f64) -> (State, Out) {
    let mut s = State::new();
    let x = Step { stalled: true, ..Step::at(rpm / RPM, 0.0, DT) };
    let out = steady_at(spec, &mut s, &x);
    (s, out)
}

/// The steady `pedal` at `rpm` that a tip-in from a fresh engine's steady closed pedal reaches (the turbo's boost has
/// two branches near its threshold; this is the one a spool reaches).
pub fn part(spec: &Spec, rpm: f64, pedal: f64) -> (State, Out) {
    let mut s = State::new();
    steady(spec, &mut s, rpm, 0.0);
    let out = steady(spec, &mut s, rpm, pedal);
    (s, out)
}

/// The steady WOT at `rpm` (see [`part`]).
pub fn wot(spec: &Spec, rpm: f64) -> (State, Out) {
    part(spec, rpm, 1.0)
}

/// The steady WOT of the engine without its rev limiter (see [`part`]): the table runs to RPMLimit, and the limiter
/// cuts from 200 rpm below it.
fn bare_wot(spec: &Spec, rpm: f64) -> (State, Out) {
    let mut s = State::new();
    let x = Step { limiter: false, ..Step::at(rpm / RPM, 0.0, DT) };
    steady_at(spec, &mut s, &x);
    let out = steady_at(spec, &mut s, &Step { pedal: 1.0, ..x });
    (s, out)
}

/// The steady WOT runs at every grid point: their states and outputs.
fn wot_runs(spec: &Spec, car: &Car) -> Vec<(State, Out)> {
    parallel(&car.grid, |&(rpm, _)| bare_wot(spec, rpm))
}

/// `f` over `items` on the machine's threads, in order (each item's run is independent, so the result is the same
/// bits as a serial map).
fn parallel<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(items.len().max(1));
    let chunk = items.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> =
            items.chunks(chunk).map(|c| scope.spawn(|| c.iter().map(&f).collect::<Vec<R>>())).collect();
        handles.into_iter().flat_map(|h| h.join().expect("a calibration thread")).collect()
    })
}

/// K at each grid point from the identity T_wot = T_ig − T_fric − T_pump (inferred: the table holds exactly).
fn identity(car: &Car, runs: &[(State, Out)]) -> Curve {
    Curve(
        car.grid
            .iter()
            .zip(runs)
            .map(|(&(rpm, t_wot), (_, out))| {
                (rpm, 4.0 * PI * (t_wot + out.friction + out.pumping) / out.charge.max(1e-9))
            })
            .collect(),
    )
}

/// The NA η_v(n): the shape of T_ig,wot = table + friction, peak 1, at least 0.5 (inferred: the table's shape is mostly
/// breathing).
fn na_eta_v(spec: &Spec, car: &Car) -> Curve {
    let t: Vec<(f64, f64)> = car.knots.iter().map(|&(rpm, tq)| (rpm, tq + spec.friction(rpm, 1.0, 1.0))).collect();
    let max = t.iter().map(|k| k.1).fold(f64::MIN, f64::max).max(1e-9);
    Curve(t.iter().map(|&(rpm, v)| (rpm, (v / max).max(0.5))).collect())
}

/// The boost target at each knot from T_wot with η_ig 0.38 and η_v 0.95, clipped to [1, cap] and by the
/// restrictor.
fn boost_targets(spec: &Spec, car: &Car, w_choke: f64) -> Curve {
    let fuel_mep = P_REF / (R * T_REF) * LHV / (LAMBDA * AFR);
    Curve(
        car.knots
            .iter()
            .map(|&(rpm, t_wot)| {
                let mut pi = 1.0;
                for _ in 0..6 {
                    let t_im = turbo::intercooled_at(pi, T_REF);
                    let imep = (t_wot + spec.friction(rpm, 1.0, pi)) / spec.geometry.per_pa();
                    pi = imep / (fuel_mep * PI_ETA_IG * ETA_V_TURBO) * t_im / T_REF;
                }
                (rpm, limit_boost(spec, w_choke, rpm, pi))
            })
            .collect(),
    )
}

/// A boost target clipped to [1, cap] and by the restrictor's choke.
fn limit_boost(spec: &Spec, w_choke: f64, rpm: f64, pi: f64) -> f64 {
    let mut pi = pi.clamp(1.0, spec.class.boost_cap.unwrap_or(PI_CAP));
    if rpm > 0.0 && w_choke.is_finite() {
        let t_im = turbo::intercooled_at(pi, T_REF);
        let w_max = RESTRICTOR_MARGIN * w_choke;
        pi = pi.min(w_max * 120.0 * R * t_im / (ETA_V_TURBO * P_REF * spec.geometry.v_d * rpm)).max(1.0);
    }
    pi
}

/// The boost targets made consistent with one K: each knot's boost scaled by its K over the knots' median from idle up
/// (the charge a flat K needs), then limited. The first estimate's η_ig leaves out the turbine's back pressure. The
/// boost scaled is the one the steady WOT made, if under the target: the charge, and so K, came from it. Scaling the
/// target there instead inflated an off-boost knot's target (turbo sample 1.90 bar at 1000 rpm for 1.06 made), and the
/// targets interpolated from it up to the next knot asked for more charge than one K needs, so K dipped on boost
/// (1.32e6 at 1750 against 1.58e6).
fn retarget(spec: &mut Spec, car: &Car) {
    let k: Vec<(f64, f64, f64)> = car
        .knots
        .iter()
        .map(|&(rpm, t_wot)| {
            let (s, out) = bare_wot(spec, rpm);
            (rpm, (t_wot + out.friction + out.pumping) / out.charge.max(1e-12), s.p_boost / P_REF)
        })
        .collect();
    let mut above: Vec<f64> = k.iter().filter(|k| k.0 >= car.rpm_idle).map(|k| k.1).collect();
    if above.is_empty() {
        return;
    }
    above.sort_by(f64::total_cmp);
    let k_ref = above[above.len() / 2];
    let t = spec.turbo.as_ref().expect("a turbo");
    let w_choke = t.w_choke;
    let pi: Vec<(f64, f64)> = k
        .iter()
        .map(|&(rpm, k, made)| (rpm, limit_boost(spec, w_choke, rpm, t.pi_wot.at(rpm).min(made) * k / k_ref)))
        .collect();
    spec.turbo.as_mut().expect("a turbo").pi_wot = Curve(pi);
}

/// The turbo before its turbine is sized: targets, design point, restrictor, pipe, valves, exhaust line.
fn turbo_spec(spec: &Spec, car: &Car) -> turbo::Spec {
    let w_choke = turbo::Spec::choke(spec.class.restrictor);
    let pi_wot = boost_targets(spec, car, w_choke);
    let pi_max = pi_wot.max();
    let &(rpm_power, _) = car.knots.iter().max_by(|a, b| (a.0 * a.1).total_cmp(&(b.0 * b.1))).expect("a knot");
    let pi_power = pi_wot.at(rpm_power);
    let t_im = turbo::intercooled_at(pi_power, T_REF);
    let w_star = (ETA_V_TURBO * pi_power * P_REF * spec.geometry.v_d * rpm_power / (120.0 * R * t_im)).max(1e-3);
    let v_boost = V_BOOST * spec.geometry.v_d;
    let (t_em0, t_em_k) = turbo::Spec::exhaust_line(w_star * (1.0 + 1.0 / (LAMBDA * AFR)));
    let mut t = turbo::Spec {
        pi_wot,
        pi_star: 1.0,
        w_star,
        omega_star: turbo::OMEGA_STAR,
        a_turbine: 1e-3,
        a_wastegate: 1e-3,
        gate_base: Curve::default(),
        aim_open: Curve::default(),
        a_bov: turbo::Spec::bov_area(v_boost, pi_max),
        w_choke,
        v_boost,
        t_em0,
        t_em_k,
    };
    t.pi_star = (pi_power * P_REF / t.inlet(w_star, P_REF)).max(1.1);
    t
}

/// The turbine's nozzle that makes K as flat as the turbo allows: K is the fuel's energy times the combustion's
/// efficiency per kg of air, nearly constant over rpm, so the boost (not K) should carry the table's shape. Minimises
/// the spread of ln K (sum of squares about its mean) at the knots from idle up, each from a fresh engine's tip-in,
/// with the exhaust manifold at most [`DRIVE_RATIO_MAX`] times the intake's; a scan over the nozzle's logarithm, then
/// golden-section around the best. The wastegate port is 3 nozzles (inferred: room to hold the target at rated flow;
/// larger ports raised the boost loop's gain into a limit cycle).
fn size_turbine(spec: &mut Spec, car: &Car, cal: &mut Calibration, around: Option<f64>) {
    // The knots and every 500 rpm between them from idle up: points between the knots keep the nozzle off the edge
    // where a spool just fails to reach a knot's boost.
    let knots: Vec<(f64, f64)> = car
        .grid
        .iter()
        .copied()
        .filter(|k| k.0 >= car.rpm_idle && (k.0 % 500.0 == 0.0 || car.knots.contains(k)))
        .collect();
    let spread = |x: f64| {
        let mut s = spec.clone();
        set_turbine(&mut s, exp(x));
        let runs = parallel(&knots, |&(rpm, _)| bare_wot(&s, rpm));
        let mut logs = Vec::with_capacity(knots.len());
        for (&(_, t_wot), (st, out)) in knots.iter().zip(runs) {
            if st.p_em > DRIVE_RATIO_MAX * st.p_im {
                return f64::INFINITY;
            }
            let k = (t_wot + out.friction + out.pumping) / out.charge.max(1e-12);
            if k <= 0.0 {
                return f64::INFINITY;
            }
            logs.push(log(k));
        }
        let mean = logs.iter().sum::<f64>() / logs.len().max(1) as f64;
        logs.iter().map(|l| (l - mean) * (l - mean)).sum::<f64>()
    };
    let (start, end) = (log(TURBINE_MIN), log(TURBINE_MAX));
    // A scan over the whole range, or (a refit) a narrow bracket around the last nozzle.
    let (mut best, h, steps) = match around {
        None => {
            let n = 12;
            let h = (end - start) / f64::from(n);
            let mut best = (start, f64::INFINITY);
            for i in 0..=n {
                let x = start + h * f64::from(i);
                let f = spread(x);
                if f < best.1 {
                    best = (x, f);
                }
            }
            (best, h, 8)
        }
        Some(a) => ((log(a), f64::INFINITY), 0.2, 5),
    };
    // Golden section in the bracket around the best.
    let g = 0.5 * (5f64.sqrt() - 1.0);
    let (mut a, mut b) = ((best.0 - h).max(start), (best.0 + h).min(end));
    let (mut c, mut d) = (b - g * (b - a), a + g * (b - a));
    let (mut fc, mut fd) = (spread(c), spread(d));
    for _ in 0..steps {
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - g * (b - a);
            fc = spread(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + g * (b - a);
            fd = spread(d);
        }
    }
    for (x, f) in [(c, fc), (d, fd)] {
        if f < best.1 {
            best = (x, f);
        }
    }
    if best.0 <= start + 1e-9 || best.0 >= end - 1e-9 {
        cal.flags.push(format!("turbine nozzle at its range's end ({:.2e} m²)", exp(best.0)));
    }
    set_turbine(spec, exp(best.0));
}

fn set_turbine(spec: &mut Spec, area: f64) {
    let t = spec.turbo.as_mut().expect("a turbo");
    t.a_turbine = area;
    t.a_wastegate = WASTEGATE_PORTS * area;
}

/// The idle air whose steady idle at IdleRPM has net 0, bisection; the surplus at 0 air.
fn idle_air(spec: &Spec) -> (f64, f64) {
    let mut st = State::new();
    let mut net = |a: f64| {
        let mut s = spec.clone();
        s.idle_air = a;
        steady(&s, &mut st, spec.rpm_idle, 0.0).torque
    };
    let surplus = net(0.0);
    if surplus >= 0.0 {
        return (0.0, surplus);
    }
    let (mut lo, mut hi) = (0.0, spec.idle_air_max);
    if net(hi) < 0.0 {
        return (hi, surplus);
    }
    for _ in 0..30 {
        let mid = 0.5 * (lo + hi);
        if net(mid) < 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (0.5 * (lo + hi), surplus)
}

/// The idle loops' gains: a PI on the idle air and a proportional spark trim, each from the steady torque's
/// sensitivity at the idle point and the crank's inertia, for the loop's bandwidth (crossover J·ω_c/b, the PI's
/// zero a quarter below it).
fn idle_gains(spec: &mut Spec, inertia: f64) {
    if spec.class.idle != Idle::Ecu {
        return;
    }
    let omega_idle = spec.rpm_idle / RPM;
    let at = |a: f64| {
        let mut s = spec.clone();
        s.idle_air = a;
        let mut st = State::new();
        steady(&s, &mut st, spec.rpm_idle, 0.0)
    };
    let a = spec.idle_air;
    let d = (0.05 * a).max(1e-5);
    let b_air = (at(a + d).torque - at((a - d).max(0.0)).torque) / (a + d - (a - d).max(0.0));
    let base = at(a);
    let b_spark = base.indicated * 2.0 * ecu::SPARK_C * ecu::IDLE_RETARD / ecu::spark_efficiency(ecu::IDLE_RETARD);
    let w_air = 2.0 * PI * ecu::IDLE_AIR_HZ;
    let w_spark = 2.0 * PI * ecu::IDLE_SPARK_HZ;
    if b_air > 0.0 {
        spec.idle_kp = inertia * w_air / b_air * omega_idle;
        spec.idle_ki = spec.idle_kp * w_air / 4.0;
    }
    if b_spark > 0.0 {
        spec.spark_kp = inertia * w_spark / b_spark * omega_idle;
    }
}

/// The engine's spec for `params` and the calibration's report.
pub fn calibrate(params: &Params) -> (Spec, Calibration) {
    let car = Car::new(params);
    let class = class::classify(&params.car, params.turbo);
    let mut spec = Spec::new(class, params.turbo, car.v_d, car.inlet, car.rpm_limit, car.rpm_idle);
    spec.torque_cut = car.torque_cut;
    let mut cal =
        Calibration { class: Some(class), displacement: car.v_d, bore: spec.geometry.bore, ..Default::default() };
    let inertia = positive(params.inertia, 0.15);
    let peak_power = car.knots.iter().map(|k| k.0 * k.1).fold(0.0, f64::max);
    let rpm_power = car.knots.iter().find(|k| k.0 * k.1 >= peak_power).map_or(car.rpm_limit, |k| k.0);
    spec.w_rated = (spec.cylinder_flow(P_REF, P_REF, rpm_power, T_REF)).max(1e-4);
    if params.turbo {
        spec.turbo = Some(turbo_spec(&spec, &car));
    }

    // Friction scale, η_v and the overrun air depend on each other: three rounds. The plate's own leak is 0 (its area
    // formula closes at 7°): the idle air is all the air a closed pedal passes.
    for _ in 0..3 {
        if !params.turbo {
            spec.eta_v = na_eta_v(&spec, &car);
            spec.w_rated = spec.cylinder_flow(P_REF, P_REF, rpm_power, T_REF).max(1e-4);
        }
        let choked = spec.cda * P_REF / (R * T_REF).sqrt() * PSI_CHOKED;
        let total = spec.cylinder_flow(ecu::OVERRUN_P * P_REF, P_REF, ecu::OVERRUN_RPM, T_REF) / choked;
        spec.overrun_air = total;
        let (_, out) = closed(&spec, car.rpm_limit);
        // At the limit the scale is s_f itself.
        let fric = out.friction / spec.friction_scale;
        cal.friction_scale_raw = (car.brake * car.max_torque - out.pumping) / fric.max(1e-9);
        spec.friction_scale = cal.friction_scale_raw.clamp(0.6, if params.turbo { TURBO_FRICTION_MAX } else { 2.0 });
    }
    if cal.friction_scale_raw != spec.friction_scale {
        cal.flags.push(format!("friction scale {:.2} clamped", cal.friction_scale_raw));
    }

    if spec.turbo.is_some() {
        // The nozzle, then the targets one K asks for, then the nozzle again for those targets.
        size_turbine(&mut spec, &car, &mut cal, None);
        for _ in 0..2 {
            retarget(&mut spec, &car);
            let a = spec.turbo.as_ref().expect("a turbo").a_turbine;
            size_turbine(&mut spec, &car, &mut cal, Some(a));
        }
    }
    // K, an NA engine's held under the fuel–air cycle by its charge, then the idle air (which needs K), then K again
    // with the idle air in the WOT area.
    let mut runs = wot_runs(&spec, &car);
    spec.k = identity(&car, &runs);
    for _ in 0..3 {
        if !breathe(&mut spec, &car) {
            break;
        }
        spec.w_rated = spec.cylinder_flow(P_REF, P_REF, rpm_power, T_REF).max(1e-4);
        let choked = spec.cda * P_REF / (R * T_REF).sqrt() * PSI_CHOKED;
        spec.overrun_air = spec.cylinder_flow(ecu::OVERRUN_P * P_REF, P_REF, ecu::OVERRUN_RPM, T_REF) / choked;
        runs = wot_runs(&spec, &car);
        spec.k = identity(&car, &runs);
    }
    hold_off_boost(&mut spec, &car, &runs);
    let (a, surplus) = idle_air(&spec);
    spec.idle_air = a;
    cal.idle_surplus = surplus;
    if surplus >= 0.0 {
        cal.flags.push(format!("idle air below 0: the leak alone idles with {surplus:.1} N·m to spare"));
    }
    if spec.idle_air >= spec.idle_air_max {
        cal.flags.push("idle air at its ceiling".into());
    }
    runs = wot_runs(&spec, &car);
    spec.k = identity(&car, &runs);
    let mut held = hold_off_boost(&mut spec, &car, &runs);
    if spec.class.actuation == Actuation::Dbw {
        if let Some(t) = spec.turbo.as_mut() {
            t.gate_base = Curve(car.grid.iter().zip(&runs).map(|(k, (s, _))| (k.0, s.wastegate)).collect());
        }
    }
    idle_gains(&mut spec, inertia);
    if spec.turbo.is_some() {
        // K again, last: the hold, the gate's base and the idle gains (the idle air a closed-pedal start leaves) move
        // the steady state near the boost threshold, and the charge's residual gas follows the exhaust pressure (up
        // to 0.2 % off the table otherwise).
        runs = wot_runs(&spec, &car);
        spec.k = identity(&car, &runs);
        held = hold_off_boost(&mut spec, &car, &runs);
    }
    if spec.class.actuation == Actuation::Dbw {
        let open = aim_open(&spec, &car, &runs);
        if let Some(t) = spec.turbo.as_mut() {
            t.aim_open = open;
        }
    }

    // The report.
    cal.friction_scale = spec.friction_scale;
    cal.leak = spec.leak;
    cal.overrun_air = if class.idle == Idle::Ecu { spec.overrun_air } else { 0.0 };
    cal.idle_air = spec.idle_air;
    cal.idle_kp = spec.idle_kp;
    cal.idle_ki = spec.idle_ki;
    cal.spark_kp = spec.spark_kp;
    cal.k = spec.k.0.clone();
    if let Some(t) = &spec.turbo {
        cal.pi_wot = t.pi_wot.0.clone();
        cal.turbine_area = t.a_turbine;
        cal.design_flow = t.w_star;
        cal.design_ratio = t.pi_star;
    }
    // Steady WOT at the knots with the final spec, and η_ig·η_v at the torque peak.
    let runs: Vec<(State, Out)> = car.knots.iter().map(|&(rpm, _)| bare_wot(&spec, rpm)).collect();
    cal.wot = car.knots.iter().zip(&runs).map(|(k, (_, out))| (k.0, k.1, out.torque)).collect();
    let (i, &(rpm_peak, t_peak)) =
        car.knots.iter().enumerate().max_by(|a, b| a.1 .1.total_cmp(&b.1 .1)).expect("a knot");
    let (s, out) = &runs[i];
    let imep = (t_peak + out.friction + out.pumping) / spec.geometry.per_pa();
    let rho = s.p_im / (R * s.t_im);
    cal.eta_ig_v = imep / (rho * LHV / (LAMBDA * AFR));
    if !(ETA_BAND.0..=ETA_BAND.1).contains(&cal.eta_ig_v) {
        cal.flags.push(format!("η_ig·η_v {:.2} at {rpm_peak:.0} rpm outside 0.30–0.55", cal.eta_ig_v));
    }
    cal.held = parallel(&held, |&(rpm, t)| (rpm, t, bare_wot(&spec, rpm).1.torque));
    (spec, cal)
}

/// The throttle position at each grid point where the plate stops controlling the air: at the steady WOT boost and
/// cylinder flow, the lift whose area passes that flow at p_im/p_b = `AIM_OPEN_RATIO` (inferred: race ECUs' boost
/// aim tables reach full aim before full throttle; this places that point where the plate stops limiting).
fn aim_open(spec: &Spec, car: &Car, runs: &[(State, Out)]) -> Curve {
    Curve(
        car.grid
            .iter()
            .zip(runs)
            .map(|(g, (s, out))| {
                let unit = gas::orifice(spec.cda, s.p_boost, turbo::AIM_OPEN_RATIO * s.p_boost, s.t_im);
                let area = if unit > 0.0 { out.w_cyl / unit - spec.leak } else { 1.0 };
                (g.0, gas::plate_lift(area).max(0.05))
            })
            .collect(),
    )
}

/// An NA engine's K over [`k_max`] goes to the charge: η_v at that grid point scaled by K/K_max (inferred: no engine
/// beats the fuel–air cycle, and race NA engines breathe over 1). A turbo's K stays: there the excess comes with the
/// boost cap, the restrictor, the turbine's sizing and the friction scale together (`notes/engine-model.md`, open
/// items). Returns whether η_v moved.
fn breathe(spec: &mut Spec, car: &Car) -> bool {
    if spec.turbo.is_some() {
        return false;
    }
    let k_max = k_max(spec.geometry.r_c);
    let moved = spec.k.0.iter().any(|&(_, k)| k > k_max * (1.0 + 1e-4));
    if moved {
        spec.eta_v = Curve(
            car.grid
                .iter()
                .zip(&spec.k.0)
                .map(|(g, &(_, k))| (g.0, spec.eta_v.at(g.0) * (k / k_max).max(1.0)))
                .collect(),
        );
    }
    moved
}

/// A turbo's K where its steady WOT boost misses the target (the turbo cannot make that boost at that speed) is held
/// at the median of the on-boost knots' K, so the table is an upper bound there rather than met by an inflated K
/// (`notes/engine-model.md`). Where the identity gives less than the median (a table weaker than
/// physics) K stays. The held grid points with the table's torque.
fn hold_off_boost(spec: &mut Spec, car: &Car, runs: &[(State, Out)]) -> Vec<(f64, f64)> {
    let Some(t) = &spec.turbo else { return Vec::new() };
    let on = |rpm: f64, s: &State| s.p_boost >= ON_BOOST * t.pi_wot.at(rpm) * P_REF;
    let mut ks: Vec<f64> = car
        .grid
        .iter()
        .zip(runs)
        .zip(&spec.k.0)
        .filter(|((g, (s, _)), _)| g.0 >= ecu::FIRE_RPM && car.knots.contains(g) && on(g.0, s))
        .map(|(_, k)| k.1)
        .collect();
    if ks.is_empty() {
        return Vec::new();
    }
    ks.sort_by(f64::total_cmp);
    let median = ks[ks.len() / 2];
    let mut held = Vec::new();
    let k: Vec<(f64, f64)> = car
        .grid
        .iter()
        .zip(runs)
        .zip(&spec.k.0)
        .map(|((g, (s, _)), &(rpm, k))| {
            if g.0 >= ecu::FIRE_RPM && !on(g.0, s) && k > median {
                held.push(*g);
                (rpm, median)
            } else {
                (rpm, k)
            }
        })
        .collect();
    spec.k = Curve(k);
    held
}
