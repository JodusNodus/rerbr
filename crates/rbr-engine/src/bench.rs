//! A test bench for the tests and `examples/curves.rs`: the model at a fixed speed (a dyno), or free in neutral
//! with the car's inertia, integrated as NGP's drivetrain does (ω += T/1.05/I·dt, `notes/engine-model.md`).

use crate::engine::RPM;
use crate::realistic::Realistic;
use crate::{Inputs, Model};

/// NGP's step, s.
pub const DT: f32 = 1.0 / 720.0;

/// One bench sample.
#[derive(Clone, Copy, Debug, Default)]
pub struct Row {
    pub time: f64,
    pub rpm: f64,
    pub pedal: f64,
    pub torque: f64,
    pub manifold: f64,
    pub boost: f64,
    pub turbo: f64,
    pub plate: f64,
    pub cut: f64,
    pub anti_lag: bool,
}

/// The reference air, warm oil, an intact engine at `omega` with the pedal at `pedal`.
pub fn inputs(omega: f64, pedal: f64) -> Inputs {
    Inputs {
        dt: DT,
        speed: omega as f32,
        throttle: pedal as f32,
        damage: 1.0,
        pressure: 101_325.0,
        temperature: 293.15,
        engine_temperature: 363.0,
        ..Default::default()
    }
}

fn row(m: &Realistic, time: f64, omega: f64, pedal: f64, torque: f64) -> Row {
    let t = m.telemetry();
    Row {
        time,
        rpm: omega * RPM,
        pedal,
        torque,
        manifold: f64::from(t.manifold_pressure),
        boost: f64::from(t.boost_pressure),
        turbo: f64::from(t.turbo_speed),
        plate: f64::from(t.plate),
        cut: f64::from(t.cut),
        anti_lag: t.anti_lag,
    }
}

/// Runs the pedal program `pedal(t)` for `seconds`: at a fixed `rpm` when `inertia` is `None`, else free from
/// `rpm` with that inertia (kg·m²) and `dip` rpm taken off at `dip_at` s. One row per step.
pub fn run(
    m: &mut Realistic,
    rpm: f64,
    inertia: Option<f64>,
    seconds: f64,
    pedal: impl Fn(f64) -> f64,
    dip: Option<(f64, f64)>,
) -> Vec<Row> {
    let mut omega = rpm / RPM;
    let steps = (seconds / f64::from(DT)).round() as usize;
    let mut rows = Vec::with_capacity(steps);
    for i in 0..steps {
        let time = i as f64 * f64::from(DT);
        if let Some((at, by)) = dip {
            if i == (at / f64::from(DT)).round() as usize {
                omega -= by / RPM;
            }
        }
        let p = pedal(time);
        let torque = f64::from(m.torque(&inputs(omega, p)));
        rows.push(row(m, time, omega, p, torque));
        if let Some(j) = inertia {
            omega = (omega + torque / 1.05 / j * f64::from(DT)).max(0.0);
        }
    }
    rows
}
