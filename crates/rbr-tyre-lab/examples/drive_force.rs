//! The lateral force a probed wheel keeps at a given drive or braking force, NGP's tyre against a profile's model:
//! for each Fx/Fz the slip ratio that gives it is solved at each slip angle, and |Fy|/Fz and that slip ratio printed.
//! The power-oversteer and lift-off question of a rear-driven car (`notes/tyre-model.md`, "Player drives"): the
//! throttle sets the drive force, not the slip ratio. Prints only.
//!
//! Research tooling (ours, experimental), on [`steady::Tyre`] as `envelope` is.
//!
//!   cargo run --release -p rbr-tyre-lab --example drive_force -- --probe P [--wheel K] [--model brush]
//!       [--fz N,N...] [--speed M/S]

use rbr_tyre_lab::probe::{parse_probe, wheel, Params};
use rbr_tyre_lab::steady::Tyre;

/// Slip angles, degrees.
const ANGLES: [f32; 11] = [1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 10.0, 12.0, 15.0, 20.0, 30.0];
/// Drive force over the load (negative braking or engine braking).
const DRIVES: [f32; 6] = [-0.3, -0.1, 0.0, 0.15, 0.3, 0.45];

/// (|Fy|/Fz, κ) at the slip angle `deg` with the drive force `drive`·Fz, or `None` past what the tyre gives there.
fn at_drive(t: &mut Tyre, fz: f32, deg: f32, drive: f32) -> Option<(f32, f32)> {
    let (v, a) = (t.speed, deg.to_radians());
    let (vlat, vlong) = (v * a.sin(), -v * a.cos());
    let r = t.p.effective_radius;
    // Driving gives a negative Fx (NGP's signs): `push` is the drive force over the load.
    let mut push = |k: f32| {
        let (fx, fy) = t.forces(fz, vlat, vlong, (1.0 + k) * (-vlong) / r);
        (-fx / fz, fy.abs() / fz)
    };
    // The stable branch: between the braking and the driving peak of the drive force (κ -0.5 to 0.5, 0.02 apart).
    let scan: Vec<(f32, f32)> = (-25..=25).map(|i| i as f32 * 0.02).map(|k| (k, push(k).0)).collect();
    let max = scan.iter().copied().fold((0.0, f32::MIN), |b, c| if c.1 > b.1 { c } else { b });
    let min = scan.iter().copied().fold((0.0, f32::MAX), |b, c| if c.1 < b.1 { c } else { b });
    if drive > max.1 || drive < min.1 {
        return None;
    }
    let (mut lo, mut hi) = (min.0, max.0);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if push(mid).0 < drive {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let k = 0.5 * (lo + hi);
    let (x, y) = push(k);
    ((x - drive).abs() < 0.02).then_some((y, k))
}

fn main() {
    let (mut probe, mut wheel_k, mut model, mut loads, mut speed) =
        (String::new(), 2u32, "brush".to_string(), vec![3000.0f32, 4000.0, 5500.0], 30.0f32);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{a} needs a value"));
        match a.as_str() {
            "--probe" => probe = value(),
            "--wheel" => wheel_k = value().parse().expect("--wheel K"),
            "--model" => model = value(),
            "--fz" => loads = value().split(',').map(|s| s.parse().expect("--fz N,N")).collect(),
            "--speed" => speed = value().parse().expect("--speed M/S"),
            _ => panic!("unknown arg {a}"),
        }
    }
    assert!(!probe.is_empty(), "--probe PATH required");
    let text = std::fs::read_to_string(&probe).unwrap_or_else(|e| panic!("{probe}: {e}"));
    let (dumps, cloud) = parse_probe(&text);
    let usable = |d: &&Params| d.load_axis.len() == 8 && d.crn_my.len() == 8 && d.static_friction > 0.0;
    let p = dumps
        .iter()
        .filter(usable)
        .rfind(|d| d.wheel == wheel_k && d.speed > 2.0 && d.patch_length > 0.0)
        .unwrap_or_else(|| panic!("{probe}: no usable dump for wheel {wheel_k}"))
        .clone();
    let tyre = |name: &str| {
        let model = rbr_tyre::model(name, &[wheel(&p)]).unwrap_or_else(|e| panic!("{e}"));
        Tyre { p: &p, cloud: &cloud, soil: 0.0, model, speed, water: 0.0, camber: 0.0 }
    };
    let (mut n, mut m) = (tyre("ngp"), tyre(&model));
    println!(
        "# step {} wheel {} static {:.4} tread {:.1} K, {speed} m/s; cells |Fy|/Fz (kappa) NGP | {model}; - past the \
         tyre's drive force there",
        p.step, p.wheel, p.static_friction, p.tread_temp
    );
    let cell = |c: Option<(f32, f32)>| c.map_or("     -        ".to_string(), |(y, k)| format!("{y:.3} ({k:+.3})"));
    for &fz in &loads {
        println!("\nFz {fz:.0} N");
        print!("{:>8}", "Fx/Fz");
        for a in ANGLES {
            print!(" | {:>29}", format!("{a}°"));
        }
        println!();
        for drive in DRIVES {
            print!("{drive:>+8.2}");
            for a in ANGLES {
                print!(" | {} {}", cell(at_drive(&mut n, fz, a, drive)), cell(at_drive(&mut m, fz, a, drive)));
            }
            println!();
        }
    }
}
