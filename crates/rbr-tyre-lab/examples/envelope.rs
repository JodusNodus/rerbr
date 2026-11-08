//! The force envelope of a profile's tyre model against NGP's on a probed wheel: peak μ_y, μ_x driving and braking
//! against load and speed, against tread temperature at the probed grip, the friction circle at the nominal point, and
//! the peaks at the loads and speeds the probe's frames ran (`--frames`). Prints only.
//!
//! Research tooling (ours, experimental): both sides are [`steady::Tyre`]s on the same wheel, NGP's (`ngp`) and the
//! model's (`--model`, default `brush`), so the ratios are the model's level against NGP's where the game runs it.
//!
//!   cargo run --release -p rbr-tyre-lab --example envelope -- --probe P [--step N --wheel K] [--model brush]
//!       [--frames]

use rbr_tyre_lab::probe::{parse_probe, wheel, Params};
use rbr_tyre_lab::steady::{grid, peak, Tyre};

/// Peak (μ_y, μ_x driving, μ_x braking) at `fz` and the tyre's speed: slip angle 0-20°, slip ratio 0-0.6.
fn peaks(t: &mut Tyre, fz: f32) -> [f32; 3] {
    [
        peak(grid(0.0, 20.0, 0.25), |a| t.mu_y(fz, a)).0,
        peak(grid(0.0, 0.6, 0.01), |k| t.mu_x(fz, k)).0,
        peak(grid(0.0, 0.6, 0.01), |k| t.mu_x(fz, -k)).0,
    ]
}

/// The friction circle at `fz`: the largest |F|/Fz per direction of the force, 0° driving, 90° lateral, 180° braking,
/// in 15° bins, over slip angles 0-15° and slip ratios -0.5 to 0.5.
fn circle(t: &mut Tyre, fz: f32) -> [f32; 13] {
    let (v, r) = (t.speed, t.p.effective_radius);
    let mut best = [0.0f32; 13];
    for a in grid(0.0, 15.0, 0.5) {
        let (vlat, vlong) = (v * a.to_radians().sin(), -v * a.to_radians().cos());
        for k in grid(-0.5, 0.5, 0.01) {
            let (fx, fy) = t.forces(fz, vlat, vlong, (1.0 + k) * (-vlong) / r);
            // Driving gives a negative Fx (NGP's signs).
            let dir = (fy.abs()).atan2(-fx).to_degrees();
            let bin = ((dir / 15.0).round() as usize).min(12);
            best[bin] = best[bin].max(fx.hypot(fy) / fz);
        }
    }
    best
}

fn row(label: &str, n: &[f32], m: &[f32]) {
    let cells: Vec<String> =
        n.iter().zip(m).map(|(a, b)| format!("{a:.3} {b:.3} {:+5.1}%", (b / a - 1.0) * 100.0)).collect();
    println!("{label:>10} | {}", cells.join(" | "));
}

fn main() {
    let (mut probe, mut step, mut wheel_k, mut model, mut frames) =
        (String::new(), None, 0u32, "brush".to_string(), false);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{a} needs a value"));
        match a.as_str() {
            "--probe" => probe = value(),
            "--step" => step = Some(value().parse::<u32>().expect("--step N")),
            "--wheel" => wheel_k = value().parse().expect("--wheel K"),
            "--model" => model = value(),
            "--frames" => frames = true,
            _ => panic!("unknown arg {a}"),
        }
    }
    assert!(!probe.is_empty(), "--probe PATH required");
    let text = std::fs::read_to_string(&probe).unwrap_or_else(|e| panic!("{probe}: {e}"));
    let (dumps, cloud) = parse_probe(&text);
    let usable = |d: &&Params| d.load_axis.len() == 8 && d.crn_my.len() == 8 && d.static_friction > 0.0;
    let p = match step {
        Some(s) => dumps.iter().filter(usable).find(|d| d.step == s && d.wheel == wheel_k),
        None => dumps.iter().filter(usable).find(|d| d.wheel == wheel_k && d.speed > 5.0 && d.patch_length > 0.0),
    }
    .unwrap_or_else(|| panic!("{probe}: no usable dump for wheel {wheel_k} at step {step:?}"))
    .clone();
    /// The probed wheel `p` under the model `name`, built from its own tables, at 15 m/s.
    fn tyre<'a>(p: &'a Params, cloud: &'a [(f32, f32)], name: &str) -> Tyre<'a> {
        let model = rbr_tyre::model(name, &[wheel(p)]).unwrap_or_else(|e| panic!("{e}"));
        Tyre { p, cloud, soil: 0.0, model, speed: 15.0, water: 0.0 }
    }
    let (mut n, mut m) = (tyre(&p, &cloud, "ngp"), tyre(&p, &cloud, &model));
    println!(
        "# step {} wheel {} model {model} static {:.4} tread {:.1} K, load axis[3] {:.0} N; cells NGP {model} ratio",
        p.step, p.wheel, p.static_friction, p.tread_temp, p.load_axis[3]
    );

    println!("\npeak mu_y | mu_x drive | mu_x brake against load (N), 15 m/s");
    for fz in grid(1500.0, 8000.0, 500.0) {
        row(&format!("{fz:.0}"), &peaks(&mut n, fz), &peaks(&mut m, fz));
    }
    println!("\nthe same against speed (m/s), 4000 N");
    for v in [5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 40.0, 50.0] {
        (n.speed, m.speed) = (v, v);
        row(&format!("{v:.0}"), &peaks(&mut n, 4000.0), &peaks(&mut m, 4000.0));
    }
    (n.speed, m.speed) = (15.0, 15.0);
    println!("\nthe same against tread temperature (K), the probed grip held, 4000 N, 15 m/s");
    for tb in grid(300.0, 390.0, 15.0) {
        let q = Params { tread_temp: tb, ..p.clone() };
        let (mut n, mut m) = (tyre(&q, &cloud, "ngp"), tyre(&q, &cloud, &model));
        row(&format!("{tb:.0}"), &peaks(&mut n, 4000.0), &peaks(&mut m, 4000.0));
    }
    println!(
        "\nsmall slips, 4000 N, 15 m/s: |Fx|/Fz driving at slip ratio, |Fy|/Fz and |Fx|/Fz free rolling at slip angle"
    );
    for k in [0.01f32, 0.02, 0.05] {
        row(&format!("k {k}"), &[n.mu_x(4000.0, k)], &[m.mu_x(4000.0, k)]);
    }
    for deg in [1.0f32, 2.0, 4.0] {
        let drag = |t: &mut Tyre| {
            let (v, a) = (t.speed, deg.to_radians());
            let (fx, fy) = t.forces(4000.0, v * a.sin(), -v * a.cos(), v * a.cos() / t.p.effective_radius);
            [fy.abs() / 4000.0, fx.abs() / 4000.0]
        };
        row(&format!("a {deg}°"), &drag(&mut n), &drag(&mut m));
    }
    println!("\nfriction circle |F|/Fz by force direction (0 drive, 90 lateral, 180 brake), 4000 N, 15 m/s");
    let (cn, cm) = (circle(&mut n, 4000.0), circle(&mut m, 4000.0));
    for (k, (a, b)) in cn.iter().zip(&cm).enumerate() {
        row(&format!("{}°", k * 15), &[*a], &[*b]);
    }

    if frames {
        // The probe's frames on a hard surface, moving: the peaks at each frame's load and speed, the frame's tables.
        let run: Vec<&Params> =
            dumps.iter().filter(usable).filter(|d| d.speed > 2.0 && d.normal_force > 500.0).collect();
        let mut sum = [[0.0f64; 3]; 2];
        let mut loads: Vec<f32> = run.iter().map(|d| d.normal_force).collect();
        loads.sort_by(f32::total_cmp);
        for d in &run {
            let (mut n, mut m) = (tyre(d, &cloud, "ngp"), tyre(d, &cloud, &model));
            (n.speed, m.speed) = (d.speed, d.speed);
            let (a, b) = (peaks(&mut n, d.normal_force), peaks(&mut m, d.normal_force));
            for k in 0..3 {
                sum[0][k] += f64::from(a[k]);
                sum[1][k] += f64::from(b[k]);
            }
        }
        let q = |f: f32| loads[((loads.len() - 1) as f32 * f) as usize];
        println!(
            "\nframes {}: load 10/50/90 % {:.0}/{:.0}/{:.0} N; mean peaks at each frame's load and speed",
            run.len(),
            q(0.1),
            q(0.5),
            q(0.9)
        );
        let mean = |s: [f64; 3]| s.map(|x| (x / run.len() as f64) as f32);
        row("mean", &mean(sum[0]), &mean(sum[1]));
    }
}
