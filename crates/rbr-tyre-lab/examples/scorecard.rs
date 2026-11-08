//! The tyre-level numbers of `harness/tyre_scorecard.py`: one probed wheel's steady forces under the tyre model a
//! physics profile runs, printed as `key value` lines.
//!
//! Research tooling (ours, experimental). `--model` is the profile's `rbr_tyre` model name (`ngp`, `brush`): the
//! model is built by `rbr_tyre::model` from the wheel's tables, as the game builds it, and asked at each slip; where it
//! gives `None` (NGP's: `ngp`, or soil under the brush) NGP's own `patch_forces` answer, as in the game's seam. The
//! model's force dynamics settle for `SETTLE` steps at each slip. Flat road, no sinking, no soil force: NGP's side is
//! the pure sweep (`sweep`), not its whole step (`ngp_transient --ramps-only`).
//!
//!   cargo run --release -p rbr-tyre-lab --example scorecard -- --probe P [--step N --wheel K] [--model ngp|brush]
//!       [--speed M/S] [--friction S,K] [--water W]
//!
//! `--friction S,K` puts a surface's static and kinetic friction in place of the probed one on a hard surface (no
//! soil), for a material no run reaches (NGP's ice, 116). `--water W` gives the model the surface's `WaterFactor` W
//! (the probed friction level kept).

use rbr_tyre::Wheel;
use rbr_tyre_lab::probe::{parse_probe, patch_at, Params};
use rbr_tyre_lab::steady::{grid, peak, Tyre};

/// The loads of the load exponents, N, and the curves' loads (4 kN, and 6.7 kN of SAE 2010-01-0374).
const LOADS: [f32; 5] = [2000.0, 3000.0, 4000.0, 5000.0, 6000.0];
const CURVES: [(f32, &str); 2] = [(4000.0, "4k"), (6700.0, "6k7")];

/// Exponent n of `ys ∝ xs^n` (log-log least squares).
fn exponent(xs: &[f32], ys: &[f32]) -> f32 {
    let lx: Vec<f64> = xs.iter().map(|x| f64::from(*x).ln()).collect();
    let ly: Vec<f64> = ys.iter().map(|y| f64::from(*y).ln()).collect();
    let n = lx.len() as f64;
    let (mx, my) = (lx.iter().sum::<f64>() / n, ly.iter().sum::<f64>() / n);
    let sxy: f64 = lx.iter().zip(&ly).map(|(x, y)| (x - mx) * (y - my)).sum();
    let sxx: f64 = lx.iter().map(|x| (x - mx).powi(2)).sum();
    (sxy / sxx) as f32
}

/// Least-squares slope through the origin of `f` over `xs`.
fn slope(xs: &[f32], mut f: impl FnMut(f32) -> f32) -> f32 {
    let (sxy, sxx) = xs.iter().fold((0.0, 0.0), |(sxy, sxx), &x| (sxy + x * f(x), sxx + x * x));
    sxy / sxx
}

fn main() {
    let (mut probe, mut step, mut wheel) = (String::new(), None, 0u32);
    let (mut model, mut speed, mut friction, mut water) = (String::from("ngp"), 15.0f32, None, 0.0f32);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{a} needs a value"));
        match a.as_str() {
            "--probe" => probe = value(),
            "--step" => step = Some(value().parse::<u32>().expect("--step N")),
            "--wheel" => wheel = value().parse().expect("--wheel K"),
            "--model" => model = value(),
            "--speed" => speed = value().parse().expect("--speed M/S"),
            "--water" => water = value().parse().expect("--water W"),
            "--friction" => {
                let v = value();
                let (s, k) = v.split_once(',').expect("--friction STATIC,KINETIC");
                friction = Some((s.parse::<f32>().expect("static"), k.parse::<f32>().expect("kinetic")));
            }
            _ => panic!("unknown arg {a}"),
        }
    }
    assert!(!probe.is_empty(), "--probe PATH required");
    let text = std::fs::read_to_string(&probe).unwrap_or_else(|e| panic!("{probe}: {e}"));
    let (dumps, cloud) = parse_probe(&text);
    let usable = |d: &&Params| d.load_axis.len() == 8 && d.crn_my.len() == 8 && d.static_friction > 0.0;
    let mut p = match step {
        Some(s) => dumps.iter().filter(usable).find(|d| d.step == s && d.wheel == wheel),
        None => dumps.iter().filter(usable).find(|d| d.wheel == wheel && d.speed > 5.0 && d.patch_length > 0.0),
    }
    .unwrap_or_else(|| panic!("{probe}: no usable dump for wheel {wheel} at step {step:?}"))
    .clone();
    // The soil thickness under the wheel: the surface line's sixth field.
    let soil = text
        .split("# step")
        .filter(|block| block.split_whitespace().next().and_then(|s| s.parse::<u32>().ok()) == Some(p.step))
        .flat_map(|block| block.split("wheel:").skip(1))
        .find(|w| w.split_whitespace().next().and_then(|s| s.parse::<u32>().ok()) == Some(p.wheel))
        .and_then(|w| w.lines().find_map(|l| l.trim().strip_prefix("surface:")))
        .and_then(|s| s.split_whitespace().nth(5)?.parse::<f32>().ok())
        .unwrap_or(0.0);
    let soil = if let Some((s, k)) = friction {
        (p.static_friction, p.kinetic_friction) = (s, k);
        0.0
    } else {
        soil
    };
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
    let built = rbr_tyre::model(&model, std::slice::from_ref(&w)).unwrap_or_else(|e| panic!("{e}"));
    let mut t = Tyre { p: &p, cloud: &cloud, soil, model: built, speed, water };
    let out = |k: &str, v: f32| println!("{k} {v:.4}");
    println!(
        "# step {} wheel {} model {model} speed {speed} static {:.4} kinetic {:.4} soil {soil:.4} water {water} temp_grip {:.4}",
        t.p.step, t.p.wheel, t.p.static_friction, t.p.kinetic_friction, t.p.temperature_grip
    );

    let angles = || grid(0.0, 40.0, 0.25);
    let kappas = || grid(0.0, 1.0, 0.01);
    // Braking peaks are taken over the braking slip 0 to loose-v1's `BERM_FROM` (0.8): the free rolling side's peak,
    // before the berm, so under loose-v1 they are not the lock (that is `mu_x_locked_*`, κ -1). NGP's and the brush's
    // braking peaks lie below 0.8 on every probed surface.
    let brake_kappas = || grid(0.0, rbr_tyre::loose::BERM_FROM as f32, 0.01);
    let fit_angles: Vec<f32> = grid(0.25, 2.0, 0.25).map(f32::to_radians).collect();
    let fit_kappas = [0.02f32, 0.04];
    let (mut lat_peaks, mut lon_peaks, mut calphas, mut kxs) = (vec![], vec![], vec![], vec![]);
    for &fz in &LOADS {
        lat_peaks.push(peak(angles(), |a| t.mu_y(fz, a)).0 * fz);
        lon_peaks.push(peak(brake_kappas(), |k| t.mu_x(fz, -k)).0.max(peak(kappas(), |k| t.mu_x(fz, k)).0) * fz);
        calphas.push(slope(&fit_angles, |a| t.mu_y(fz, a.to_degrees()) * fz));
        kxs.push(slope(&fit_kappas, |k| (t.mu_x(fz, k) + t.mu_x(fz, -k)) / 2.0 * fz));
    }
    out("n_mu_y", exponent(&LOADS, &lat_peaks));
    out("n_mu_x", exponent(&LOADS, &lon_peaks));
    out("m_calpha", exponent(&LOADS, &calphas));
    out("m_kx", exponent(&LOADS, &kxs));
    for (k, name) in [(0, "2k"), (2, "4k"), (4, "6k")] {
        out(&format!("peak_mu_y_{name}"), lat_peaks[k] / LOADS[k]);
    }
    out("peak_mu_x_6k_over_2k", lon_peaks[4] / LOADS[4] / (lon_peaks[0] / LOADS[0]));
    out("kx_per_fz_4k", kxs[2] / LOADS[2]);
    out("kx_over_ky_4k", kxs[2] / calphas[2]);
    out("comb_a2_4k", t.combined(4000.0, 2.0, -0.25));
    out("comb_a4_4k", t.combined(4000.0, 4.0, -0.25));

    // The aligning moment the game applies (trail torque less the carcass offset; `Tyre::aligning`), 4 kN.
    let fz = 4000.0;
    let a = patch_at(&cloud, fz, p.patch_length) / 2.0;
    let mzs: Vec<(f32, f32)> = angles().map(|d| (d, t.mz(fz, d, 0.0))).collect();
    let (mz_pk, mz_at) = mzs.iter().fold((0.0, 0.0), |b, &(d, m)| if m > b.0 { (m, d) } else { b });
    let (fy_pk, fy_at) = peak(angles(), |d| t.mu_y(fz, d));
    let small = 0.5f32;
    let trail0 = t.mz(fz, small, 0.0) / (t.mu_y(fz, small) * fz);
    out("trail0_4k", trail0);
    out("trail0_over_a_4k", trail0 / a);
    out("mz_peak_alpha_4k", mz_at);
    out("mz_peak_alpha_over_fy_4k", mz_at / fy_at);
    out("mz_peak_per_mu_fz_a_4k", mz_pk / (fy_pk * fz * a));
    let zero = mzs.iter().find(|(d, m)| *d > mz_at && *m <= 0.0).map_or(f32::NAN, |x| x.0);
    out("mz_zero_alpha_over_fy_4k", zero / fy_at);
    let mz_peaks: Vec<f32> = LOADS.iter().map(|&l| peak(grid(0.0, 15.0, 0.25), |d| t.mz(l, d, 0.0)).0).collect();
    out("n_mz_peak", exponent(&LOADS, &mz_peaks));
    let trails: Vec<f32> = LOADS.iter().map(|&l| t.mz(l, small, 0.0) / (t.mu_y(l, small) * l)).collect();
    out("n_trail0", exponent(&LOADS, &trails));
    let halves: Vec<f32> = LOADS.iter().map(|&l| patch_at(&cloud, l, p.patch_length) / 2.0).collect();
    out("n_half_length", exponent(&LOADS, &halves));
    for deg in [2.0f32, 4.0] {
        let pure = t.mz(fz, deg, 0.0);
        for (k, name) in [(-0.05f32, "brake"), (0.05, "drive")] {
            out(&format!("mz_comb_a{deg}_{name}_k0.05_4k"), t.mz(fz, deg, k) / pure);
        }
    }

    for (fz, name) in CURVES {
        let lat: Vec<(f32, f32)> = angles().map(|a| (a, t.mu_y(fz, a))).collect();
        let (pk, at) = lat.iter().fold((0.0, 0.0), |b, &(a, m)| if m > b.0 { (m, a) } else { b });
        out(&format!("peak_mu_y_{name}"), pk);
        out(&format!("peak_alpha_{name}"), at);
        let a95 = lat.iter().find(|(_, m)| *m >= 0.95 * pk).map_or(f32::NAN, |x| x.0);
        out(&format!("alpha95_{name}"), a95);
        let a90 = lat.iter().find(|(_, m)| *m >= 0.9 * pk).map_or(f32::NAN, |x| x.0);
        out(&format!("alpha90_{name}"), a90);
        out(&format!("calpha_per_fz_{name}"), slope(&fit_angles, |a| t.mu_y(fz, a.to_degrees())));
        for (deg, m) in &lat {
            if [2.5, 5.0, 10.0, 14.0, 15.0, 16.0, 17.0, 20.0, 25.0, 30.0, 40.0].contains(deg) {
                out(&format!("mu_y_rel_a{deg}_{name}"), m / pk);
            }
        }
        let (drive, kd) = peak(kappas(), |k| t.mu_x(fz, k));
        let (brake, kb) = peak(brake_kappas(), |k| t.mu_x(fz, -k));
        out(&format!("peak_mu_x_drive_{name}"), drive);
        out(&format!("peak_kappa_drive_{name}"), kd);
        out(&format!("peak_mu_x_brake_{name}"), brake);
        out(&format!("peak_kappa_brake_{name}"), kb);
        out(&format!("peak_kappa_{name}"), if drive >= brake { kd } else { kb });
        for k in [0.3f32, 0.45, 0.5] {
            out(&format!("mu_x_rel_k{k}_brake_{name}"), t.mu_x(fz, -k) / brake);
        }
        out(&format!("mu_x_k0.4_over_k0.1_brake_{name}"), t.mu_x(fz, -0.4) / t.mu_x(fz, -0.1));
        out(&format!("mu_x_rel_k0.5_drive_{name}"), t.mu_x(fz, 0.5) / drive);
        out(&format!("mu_x_rel_k1_drive_{name}"), t.mu_x(fz, 1.0) / drive);
        let (locked_x, _) = t.locked(fz, 0.0);
        out(&format!("mu_x_locked_{name}"), locked_x);
        out(&format!("mu_x_locked_rel_{name}"), locked_x / brake);
        out(&format!("mu_x_locked_over_drive_peak_{name}"), locked_x / drive);
        out(&format!("mu_x_locked_over_peak_mu_y_{name}"), locked_x / pk);
        // The locked tyre's side force against the free rolling one's, 1-40° of slip angle.
        let worst = lat
            .iter()
            .filter(|(a, _)| *a >= 1.0)
            .step_by(4)
            .map(|&(a, m)| t.locked(fz, a).1 / m)
            .fold(0.0f32, f32::max);
        out(&format!("locked_mu_y_over_free_max_{name}"), worst);
        out(&format!("locked_mu_y_a35_over_peak_{name}"), t.locked(fz, 35.0).1 / pk);
        let (l20, l30) = (t.locked(fz, 20.0).1, t.locked(fz, 30.0).1);
        out(&format!("locked_mu_y_a20_{name}"), l20);
        out(&format!("locked_mu_y_a30_{name}"), l30);
        // The mean over 20° and 30° of the locked side force over the free rolling one's.
        let free = |deg: f32| lat.iter().find(|(a, _)| *a == deg).map_or(f32::NAN, |x| x.1);
        out(&format!("locked_mu_y_over_free_a20_30_{name}"), (l20 / free(20.0) + l30 / free(30.0)) / 2.0);
    }
}
