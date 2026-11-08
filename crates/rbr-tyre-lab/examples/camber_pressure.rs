//! Camber and inflation pressure on a probed wheel: the lateral force against the inclination and the slip angle, and
//! the slip stiffnesses, peaks and lateral relaxation against the pressure, under NGP's tyre and a profile's model
//! (steady forces as the `scorecard` example takes them, `rbr_tyre_lab::steady`).
//!
//! Research tooling (ours, experimental).
//!
//!   cargo run --release -p rbr-tyre-lab --example camber_pressure -- --probe P [--step N --wheel K]
//!       [--model ngp|brush] [--speed M/S]
//!
//! Signs: the wheel's frame, NGP's: a positive slip angle (lateral velocity) gives a negative Fy, and a negative
//! inclination (top leaning in) pushes inwards, so it adds to |Fy| here. NGP's own step holds no force where the
//! inclination's offset slip and the sliding disagree (`grip::clamp_deflection`); the patch force printed is the
//! sweep's.

use rbr_tyre::Wheel;
use rbr_tyre_lab::probe::{interp, parse_probe, wheel, Params};
use rbr_tyre_lab::steady::{grid, peak, Tyre};

const LOADS: [f32; 3] = [2000.0, 4000.0, 6000.0];
const CAMBERS: [f32; 7] = [-4.0, -2.0, -1.0, 0.0, 1.0, 2.0, 4.0];
const ALPHAS: [f32; 6] = [0.0, 1.0, 2.0, 4.0, 8.0, 12.0];

fn main() {
    let (mut probe, mut step, mut at_wheel) = (String::new(), None, 0u32);
    let (mut model, mut speed) = (String::from("ngp"), 15.0f32);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{a} needs a value"));
        match a.as_str() {
            "--probe" => probe = value(),
            "--step" => step = Some(value().parse::<u32>().expect("--step N")),
            "--wheel" => at_wheel = value().parse().expect("--wheel K"),
            "--model" => model = value(),
            "--speed" => speed = value().parse().expect("--speed M/S"),
            _ => panic!("unknown arg {a}"),
        }
    }
    assert!(!probe.is_empty(), "--probe PATH required");
    let text = std::fs::read_to_string(&probe).unwrap_or_else(|e| panic!("{probe}: {e}"));
    let (dumps, cloud) = parse_probe(&text);
    let usable = |d: &&Params| d.load_axis.len() == 8 && d.crn_my.len() == 8 && d.static_friction > 0.0;
    let p = match step {
        Some(s) => dumps.iter().filter(usable).find(|d| d.step == s && d.wheel == at_wheel),
        None => dumps.iter().filter(usable).find(|d| d.wheel == at_wheel && d.speed > 5.0 && d.patch_length > 0.0),
    }
    .unwrap_or_else(|| panic!("{probe}: no usable dump for wheel {at_wheel} at step {step:?}"))
    .clone();
    let w = wheel(&p);
    println!(
        "# step {} wheel {} model {model} speed {speed} static {:.4} pressure {:.0} Pa stiffness factor {:.4}",
        p.step, p.wheel, p.static_friction, p.air_pressure, p.stiffness_factor
    );

    let mut t = build(&p, &cloud, &model, &w, speed);
    let r = p.effective_radius;
    println!("\n## Fy (N) against the inclination (rows, deg) and the slip angle (columns), free rolling");
    for fz in LOADS {
        println!("\nFz {fz:.0} N; in brackets Fy - Fy(0 deg)");
        println!("incl | {}", ALPHAS.map(|a| format!("{a:>16.0}")).join(" "));
        let base: Vec<f32> = ALPHAS.iter().map(|&a| fy(&mut t, fz, a, 0.0, r)).collect();
        for g in CAMBERS {
            let row: Vec<String> = ALPHAS
                .iter()
                .zip(&base)
                .map(|(&a, &b)| {
                    let f = fy(&mut t, fz, a, g, r);
                    format!("{f:>8.0} ({:>+5.0})", f - b)
                })
                .collect();
            println!("{g:>+4.0} | {}", row.join(" "));
        }
    }

    println!("\n## Camber stiffness (Fy change per rad of the inclination's sine at 1 deg of slip)");
    println!("Fz N | C_gamma/Fz /rad | C_alpha/Fz /rad | C_gamma/C_alpha");
    for fz in LOADS {
        let (cg, ca) = (t.camber_per_fz(fz), t.calpha_per_fz(fz));
        println!("{fz:.0} | {cg:.3} | {ca:.2} | {:.4}", cg / ca);
    }

    println!("\n## Pressure: changes against the probed pressure (NGP's stiffness factor; patch length the probe's)");
    println!("dp bar | factor | Fz N | C_alpha/Fz | Kx/Fz | peak mu_y | peak mu_x | x63_y at 1 deg (m)");
    for dp in [-0.5f32, 0.0, 0.5] {
        let mut q = p.clone();
        q.stiffness_factor = interp(&p.press_axis, &p.stf_val, p.air_pressure + dp * 1e5);
        let mut tq = build(&q, &cloud, &model, &w, speed);
        for fz in LOADS {
            let my = peak(grid(0.0, 30.0, 0.25), |a| tq.mu_y(fz, a)).0;
            let mx = peak(grid(0.0, 1.0, 0.01), |k| tq.mu_x(fz, -k)).0;
            println!(
                "{dp:+.1} | {:.4} | {fz:.0} | {:.3} | {:.3} | {my:.4} | {mx:.4} | {:.4}",
                q.stiffness_factor,
                tq.calpha_per_fz(fz),
                tq.kx_per_fz(fz),
                tq.x63_y(fz, 1.0)
            );
        }
    }
}

/// The tyre under test: `p` under the model `name` built from the wheel's tables `w`.
fn build<'a>(p: &'a Params, cloud: &'a [(f32, f32)], name: &str, w: &Wheel, speed: f32) -> Tyre<'a> {
    let model = rbr_tyre::model(name, std::slice::from_ref(w)).unwrap_or_else(|e| panic!("{e}"));
    Tyre { p, cloud, soil: 0.0, model, speed, water: 0.0, camber: 0.0 }
}

/// Fy (N, signed) at `fz`, free rolling at the slip angle `deg` with the inclination `incl` (deg).
fn fy(t: &mut Tyre, fz: f32, deg: f32, incl: f32, r: f32) -> f32 {
    t.camber = incl.to_radians().sin();
    let (v, a) = (t.speed, deg.to_radians());
    let vlong = -v * a.cos();
    let f = t.forces(fz, v * a.sin(), vlong, -vlong / r).1;
    t.camber = 0.0;
    f
}
