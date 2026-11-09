//! Steady force sweeps per model on a probed wheel, as CSV: the lateral force over the slip angle at fixed slip ratios
//! (default), or with `--long` the longitudinal force over the slip ratio at fixed slip angles. The curves of
//! `docs/experimental-physics.md`'s tyre figures (`harness/tyre_curves.py`).
//!
//! Research tooling (ours, experimental): the wheel as `scorecard` sets it up (the probe's dump, the soil under it),
//! NGP's tyre (`ngp`) and the profile models (`brush`) as `steady::Tyre`s at one load and contact speed. Forces in N,
//! signed as a plot reads them: a positive slip angle gives a positive lateral force; κ < 0 brakes (negative Fx, -1
//! locked), κ > 0 drives.
//!
//!   cargo run --release -p rbr-tyre-lab --example force_sweep -- --probe P [--step N --wheel K] [--fz 4000]
//!       [--speed 15] [--models ngp,brush] [--kappas 0,-0.05,-0.1,-0.2,0.1 --to 40 --by 0.25]
//!       [--long --alphas 0,2,5,10]

use rbr_tyre::Wheel;
use rbr_tyre_lab::probe::{parse_probe, Params};
use rbr_tyre_lab::steady::{grid, Tyre};

fn main() {
    let (mut probe, mut step, mut wheel) = (String::new(), None, 0u32);
    let (mut fz, mut speed, mut to, mut by) = (4000.0f32, 15.0f32, 40.0f32, 0.25f32);
    let mut kappas = vec![0.0f32, -0.05, -0.1, -0.2, 0.1];
    let (mut long, mut alphas) = (false, vec![0.0f32, 2.0, 5.0, 10.0]);
    let mut models = vec![String::from("ngp"), String::from("brush")];
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{a} needs a value"));
        match a.as_str() {
            "--probe" => probe = value(),
            "--step" => step = Some(value().parse::<u32>().expect("--step N")),
            "--wheel" => wheel = value().parse().expect("--wheel K"),
            "--fz" => fz = value().parse().expect("--fz N"),
            "--speed" => speed = value().parse().expect("--speed M/S"),
            "--to" => to = value().parse().expect("--to DEG"),
            "--by" => by = value().parse().expect("--by DEG"),
            "--kappas" => kappas = value().split(',').map(|k| k.parse().expect("--kappas K,...")).collect(),
            "--models" => models = value().split(',').map(str::to_owned).collect(),
            "--long" => long = true,
            "--alphas" => alphas = value().split(',').map(|a| a.parse().expect("--alphas DEG,...")).collect(),
            _ => panic!("unknown arg {a}"),
        }
    }
    assert!(!probe.is_empty(), "--probe PATH required");
    let text = std::fs::read_to_string(&probe).unwrap_or_else(|e| panic!("{probe}: {e}"));
    let (dumps, cloud) = parse_probe(&text);
    let usable = |d: &&Params| d.load_axis.len() == 8 && d.crn_my.len() == 8 && d.static_friction > 0.0;
    let p = match step {
        Some(s) => dumps.iter().filter(usable).find(|d| d.step == s && d.wheel == wheel),
        None => dumps.iter().filter(usable).find(|d| d.wheel == wheel && d.speed > 5.0 && d.patch_length > 0.0),
    }
    .unwrap_or_else(|| panic!("{probe}: no usable dump for wheel {wheel} at step {step:?}"))
    .clone();
    // The soil thickness under the wheel: the surface line's sixth field (as `scorecard`).
    let soil = text
        .split("# step")
        .filter(|block| block.split_whitespace().next().and_then(|s| s.parse::<u32>().ok()) == Some(p.step))
        .flat_map(|block| block.split("wheel:").skip(1))
        .find(|w| w.split_whitespace().next().and_then(|s| s.parse::<u32>().ok()) == Some(p.wheel))
        .and_then(|w| w.lines().find_map(|l| l.trim().strip_prefix("surface:")))
        .and_then(|s| s.split_whitespace().nth(5)?.parse::<f32>().ok())
        .unwrap_or(0.0);
    let w = Wheel {
        tyre: 0,
        reference_friction: p.reference_friction,
        load_axis: p.load_axis.clone(),
        cornering_friction: p.crn_my.clone(),
        traction_friction: p.trc_my.clone(),
        cornering_peak_slip: p.crn_pk.clone(),
        traction_peak_slip: p.trc_pk.clone(),
        cornering_stiffness: p.crn_stf.clone(),
        traction_stiffness: p.trc_stf.clone(),
        temperature_axis: p.temp_axis.clone(),
        rubber_friction: p.rub_my.clone(),
    };
    println!(
        "# step {} wheel {} fz {fz} speed {speed} static {:.4} soil {soil:.4}",
        p.step, p.wheel, p.static_friction
    );
    println!("model,kappa,alpha_deg,fy_n,fx_n");
    let r = p.effective_radius;
    for model in &models {
        let built = rbr_tyre::model(model, std::slice::from_ref(&w), &rbr_tyre::Tuning::MODEL)
            .unwrap_or_else(|e| panic!("{e}"));
        let mut t = Tyre { p: &p, cloud: &cloud, soil, model: built, speed, water: 0.0, camber: 0.0 };
        // (slip angle, slip ratio) pairs: one curve per fixed value, swept over the other.
        let curves: Vec<Vec<(f32, f32)>> = if long {
            alphas.iter().map(|&a| grid(-1.0, 1.0, 0.005).map(|k| (a, k)).collect()).collect()
        } else {
            kappas.iter().map(|&k| grid(-to, to, by).map(|a| (a, k)).collect()).collect()
        };
        for (a, k) in curves.into_iter().flatten() {
            let rad = a.to_radians();
            let (vlat, vlong) = (speed * rad.sin(), -speed * rad.cos());
            let (fx, fy) = t.forces(fz, vlat, vlong, (1.0 + k) * (-vlong) / r);
            println!("{model},{k},{a},{:.1},{:.1}", -fy, -fx);
        }
    }
}
