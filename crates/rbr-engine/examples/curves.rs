//! Steady torque maps and transients of the `realistic` model as CSV: `cargo run --release -p rbr-engine --example
//! curves -- OUT_DIR [CAR_FOLDER ...]`. Without folders it runs the synthetic samples; with them (needs `RBR_RSF`)
//! the installed RSF cars'. Writes `OUT_DIR/<car>_steady.csv` (rpm, pedal, the model's net torque, NGP's
//! (1 − t)·B + t·V at t = pedal) and `OUT_DIR/<car>_transients.csv` (scenario and the bench's rows).

#[path = "../tests/rsf/mod.rs"]
mod rsf;

use rbr_engine::bench::{self, Row};
use rbr_engine::calibrate;
use rbr_engine::engine::RPM;
use rbr_engine::realistic::Realistic;
use rbr_engine::{samples, Params};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// NGP's torque at a fixed speed: (1 − t)·B + t·V, B = −(min(n, limit)/limit)·Brake·MaxTorque, V = Tq(n)·MaxTorque
/// (0 above the limit) (`rbr_physics::ngp::engine::torque`, friction and density left out).
fn ngp(p: &Params, omega: f64, t: f64) -> f64 {
    let limit = f64::from(p.rev_limit);
    let max = f64::from(p.max_torque);
    let b = -(omega.min(limit) / limit) * f64::from(p.brake_coefficient) * max;
    let knots: Vec<(f64, f64)> = p.torque_knots.iter().map(|&(w, q)| (f64::from(w), f64::from(q))).collect();
    let tq = rbr_engine::curve::Curve(knots).at(omega);
    let v = if omega > limit { 0.0 } else { tq * max };
    (1.0 - t) * b + t * v
}

fn steady(p: &Params, m: &Realistic) -> String {
    let mut out = String::from("rpm,pedal,torque,ngp\n");
    let top = f64::from(p.rev_limit) * RPM + 500.0;
    for k in 0..=10 {
        let pedal = f64::from(k) / 10.0;
        let mut rpm = 500.0;
        while rpm <= top {
            let t = calibrate::part(m.spec(), rpm, pedal).1.torque;
            let _ = writeln!(out, "{rpm},{pedal},{t:.3},{:.3}", ngp(p, rpm / RPM, pedal));
            rpm += 250.0;
        }
    }
    out
}

fn transients(p: &Params, model: &Realistic) -> String {
    let idle = f64::from(p.idle_speed) * RPM;
    let inertia = f64::from(p.inertia);
    let mut out = String::from("scenario,time,rpm,pedal,torque,manifold,boost,turbo,plate,cut,anti_lag\n");
    let mut put = |name: &str, rows: Vec<Row>| {
        for r in rows.iter().step_by(2) {
            let _ = writeln!(
                out,
                "{name},{:.4},{:.1},{:.3},{:.2},{:.0},{:.0},{:.0},{:.4},{:.2},{}",
                r.time,
                r.rpm,
                r.pedal,
                r.torque,
                r.manifold,
                r.boost,
                r.turbo,
                r.plate,
                r.cut,
                u8::from(r.anti_lag)
            );
        }
    };
    for rpm in [3000.0, 5000.0] {
        let mut m = model.clone();
        let rows = bench::run(&mut m, rpm, None, 5.0, |t| if (1.0..3.0).contains(&t) { 1.0 } else { 0.0 }, None);
        put(&format!("tip-in {rpm:.0}"), rows);
    }
    let mut m = model.clone();
    put("idle", bench::run(&mut m, idle * 1.3, Some(inertia), 8.0, |_| 0.0, Some((5.0, 300.0))));
    let mut m = model.clone();
    put(
        "free rev",
        bench::run(&mut m, idle, Some(inertia), 4.0, |t| if (0.5..0.8).contains(&t) { 1.0 } else { 0.0 }, None),
    );
    let mut m = model.clone();
    put("limiter", bench::run(&mut m, idle, Some(inertia), 3.0, |t| if t >= 0.2 { 1.0 } else { 0.0 }, None));
    put("standing start", bench::standing(model.clone(), idle, inertia, 3.0, true));
    if model.spec().class.anti_lag {
        put("standing no start", bench::standing(model.clone(), idle, inertia, 3.0, false));
    }
    if p.turbo {
        for rpm in [2500.0, 4000.0] {
            let mut m = model.clone();
            put(
                &format!("spool {rpm:.0}"),
                bench::run(&mut m, rpm, None, 4.0, |t| if t >= 1.0 { 1.0 } else { 0.0 }, None),
            );
        }
        let program = |t: f64| if (2.0..3.0).contains(&t) { 0.0 } else { 1.0 };
        let mut m = model.clone();
        put("lift and re-apply 4500", bench::run(&mut m, 4500.0, None, 4.5, program, None));
        let mut m = model.clone();
        m.spec_mut().class.anti_lag = false;
        put("lift and re-apply 4500 no anti-lag", bench::run(&mut m, 4500.0, None, 4.5, program, None));
    }
    // Neutral with the car's inertia: idle, a 300 rpm dip at 3 s, a full blip at 6.0–6.3 s.
    let mut m = model.clone();
    let blip = |t: f64| if (6.0..6.3).contains(&t) { 1.0 } else { 0.0 };
    put("neutral", bench::run(&mut m, idle, Some(inertia), 10.0, blip, Some((3.0, 300.0))));
    out
}

/// The params the plots need for NGP's formula: `key,value` rows, then `knot,rpm,share` rows.
fn params(p: &Params) -> String {
    let mut out = String::from("key,a,b\n");
    let _ = writeln!(out, "max_torque,{},", p.max_torque);
    let _ = writeln!(out, "rev_limit,{},", f64::from(p.rev_limit) * RPM);
    let _ = writeln!(out, "idle,{},", f64::from(p.idle_speed) * RPM);
    let _ = writeln!(out, "brake,{},", p.brake_coefficient);
    let _ = writeln!(out, "inertia,{},", p.inertia);
    let _ = writeln!(out, "litres,{},", 4.0 * p.cylinder_volume * 1000.0);
    for &(w, q) in &p.torque_knots {
        let _ = writeln!(out, "knot,{},{q}", f64::from(w) * RPM);
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(dir) = args.first() else {
        eprintln!("usage: curves OUT_DIR [CAR_FOLDER ...]");
        std::process::exit(2);
    };
    fs::create_dir_all(dir).expect("the output folder");
    let cars: Vec<(String, Params)> = if args.len() > 1 {
        let root = std::env::var_os("RBR_RSF").expect("RBR_RSF for RSF cars");
        args[1..].iter().map(|f| (f.clone(), rsf::params(Path::new(&root), f).expect("the car"))).collect()
    } else {
        samples::all().into_iter().map(|(n, p)| (n.to_owned(), p)).collect()
    };
    for (name, p) in cars {
        let m = Realistic::new(&p);
        eprintln!("{name}: {:?}", m.calibration());
        fs::write(Path::new(dir).join(format!("{name}_steady.csv")), steady(&p, &m)).expect("write");
        fs::write(Path::new(dir).join(format!("{name}_transients.csv")), transients(&p, &m)).expect("write");
        fs::write(Path::new(dir).join(format!("{name}_params.csv")), params(&p)).expect("write");
    }
}
