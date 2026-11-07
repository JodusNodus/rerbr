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
//!       [--speed M/S] [--friction S,K]
//!
//! `--friction S,K` puts a surface's static and kinetic friction in place of the probed one on a hard surface (no
//! soil), for a material no run reaches (NGP's ice, 116).

use rbr_physics::tyre::patch_forces::patch_forces;
use rbr_tyre::{ForceInput, Model, Wheel};
use rbr_tyre_lab::ngp::{blend_at, patch_input};
use rbr_tyre_lab::probe::{parse_probe, patch_at, Params};

const DT: f32 = 1.0 / 720.0;
/// Steps the model's force dynamics settle at each slip (2 s).
const SETTLE: usize = 1440;
/// The loads of the load exponents, N, and the curves' loads (4 kN, and 6.7 kN of SAE 2010-01-0374).
const LOADS: [f32; 5] = [2000.0, 3000.0, 4000.0, 5000.0, 6000.0];
const CURVES: [(f32, &str); 2] = [(4000.0, "4k"), (6700.0, "6k7")];

/// The tyre under test: the probed wheel and the profile's model.
struct Tyre<'a> {
    p: &'a Params,
    cloud: &'a [(f32, f32)],
    soil: f32,
    model: Option<Box<dyn Model>>,
    speed: f32,
}

impl Tyre<'_> {
    /// Steady (Fx, Fy) at `fz` for the contact velocities and spin (NGP's signs).
    fn forces(&mut self, fz: f32, vlat: f32, vlong: f32, spin: f32) -> (f32, f32) {
        let p = self.p;
        let patch = patch_at(self.cloud, fz, p.patch_length);
        let ngp = patch_forces(&patch_input(p, &blend_at(p, fz), fz, patch, vlat, vlong, spin));
        let Some(model) = self.model.as_mut() else { return (ngp.longitudinal_force, ngp.lateral_force) };
        let x = ForceInput {
            dt: DT,
            touching: true,
            speed: (vlat * vlat + vlong * vlong).sqrt(),
            lateral_velocity: vlat,
            longitudinal_velocity: vlong,
            spin,
            effective_radius: p.effective_radius,
            normal_force: fz,
            patch_length: patch,
            grip: p.static_friction,
            stiffness_scale: 1.0,
            stiffness_factor: p.stiffness_factor,
            tread_temperature: p.tread_temp,
            tread_left: 1.0 - p.wear_op,
            soil_thickness: self.soil,
            lateral_stiffness: p.carcass_lat * p.stiffness_factor,
            torsional_stiffness: p.carcass_tor * p.stiffness_factor,
        };
        model.restart();
        let mut last = None;
        for _ in 0..SETTLE {
            last = model.forces(0, &x);
            if last.is_none() {
                break;
            }
        }
        match last {
            Some(o) => {
                let mix = |n: f32, m: f32| n + (m - n) * o.weight;
                (mix(ngp.longitudinal_force, o.longitudinal_force), mix(ngp.lateral_force, o.lateral_force))
            }
            None => (ngp.longitudinal_force, ngp.lateral_force),
        }
    }

    /// |Fy|/Fz free rolling at the slip angle `deg`.
    fn mu_y(&mut self, fz: f32, deg: f32) -> f32 {
        let (v, a) = (self.speed, deg.to_radians());
        let vlong = -v * a.cos();
        self.forces(fz, v * a.sin(), vlong, -vlong / self.p.effective_radius).1.abs() / fz
    }

    /// (|Fx|, |Fy|)/Fz locked (no spin) at the slip angle `deg`.
    fn locked(&mut self, fz: f32, deg: f32) -> (f32, f32) {
        let (v, a) = (self.speed, deg.to_radians());
        let (fx, fy) = self.forces(fz, v * a.sin(), -v * a.cos(), 0.0);
        (fx.abs() / fz, fy.abs() / fz)
    }

    /// |Fx|/Fz straight ahead at the slip ratio `kappa` (negative braking, -1 locked).
    fn mu_x(&mut self, fz: f32, kappa: f32) -> f32 {
        let v = self.speed;
        self.forces(fz, 0.0, -v, (1.0 + kappa) * v / self.p.effective_radius).0.abs() / fz
    }

    /// |Fy(κ)|/|Fy(0)| at the slip angle `deg`, the contact speed held (combined slip).
    fn combined(&mut self, fz: f32, deg: f32, kappa: f32) -> f32 {
        let (v, a) = (self.speed, deg.to_radians());
        let (vlat, vlong) = (v * a.sin(), -v * a.cos());
        let r = self.p.effective_radius;
        let fy = |t: &mut Self, k: f32| t.forces(fz, vlat, vlong, (1.0 + k) * (-vlong) / r).1.abs();
        fy(self, kappa) / fy(self, 0.0)
    }
}

/// The grid's peak (value, at) of `f` over `xs`.
fn peak(xs: impl Iterator<Item = f32>, mut f: impl FnMut(f32) -> f32) -> (f32, f32) {
    xs.map(|x| (f(x), x)).fold((0.0, 0.0), |best, (y, x)| if y > best.0 { (y, x) } else { best })
}

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

fn grid(from: f32, to: f32, step: f32) -> impl Iterator<Item = f32> {
    let n = ((to - from) / step).round() as i32;
    (0..=n).map(move |k| from + k as f32 * step)
}

fn main() {
    let (mut probe, mut step, mut wheel) = (String::new(), None, 0u32);
    let (mut model, mut speed, mut friction) = (String::from("ngp"), 15.0f32, None);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{a} needs a value"));
        match a.as_str() {
            "--probe" => probe = value(),
            "--step" => step = Some(value().parse::<u32>().expect("--step N")),
            "--wheel" => wheel = value().parse().expect("--wheel K"),
            "--model" => model = value(),
            "--speed" => speed = value().parse().expect("--speed M/S"),
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
    let mut t = Tyre { p: &p, cloud: &cloud, soil, model: built, speed };
    let out = |k: &str, v: f32| println!("{k} {v:.4}");
    println!(
        "# step {} wheel {} model {model} speed {speed} static {:.4} kinetic {:.4} soil {soil:.4} temp_grip {:.4}",
        t.p.step, t.p.wheel, t.p.static_friction, t.p.kinetic_friction, t.p.temperature_grip
    );

    let angles = || grid(0.0, 40.0, 0.25);
    let kappas = || grid(0.0, 1.0, 0.01);
    let fit_angles: Vec<f32> = grid(0.25, 2.0, 0.25).map(f32::to_radians).collect();
    let fit_kappas = [0.02f32, 0.04];
    let (mut lat_peaks, mut lon_peaks, mut calphas, mut kxs) = (vec![], vec![], vec![], vec![]);
    for &fz in &LOADS {
        lat_peaks.push(peak(angles(), |a| t.mu_y(fz, a)).0 * fz);
        lon_peaks.push(peak(kappas(), |k| t.mu_x(fz, -k)).0.max(peak(kappas(), |k| t.mu_x(fz, k)).0) * fz);
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
    out("calpha_per_fz_4k", calphas[2] / LOADS[2]);
    out("kx_per_fz_4k", kxs[2] / LOADS[2]);
    out("kx_over_ky_4k", kxs[2] / calphas[2]);
    out("comb_a2_4k", t.combined(4000.0, 2.0, -0.25));
    out("comb_a4_4k", t.combined(4000.0, 4.0, -0.25));

    for (fz, name) in CURVES {
        let lat: Vec<(f32, f32)> = angles().map(|a| (a, t.mu_y(fz, a))).collect();
        let (pk, at) = lat.iter().fold((0.0, 0.0), |b, &(a, m)| if m > b.0 { (m, a) } else { b });
        out(&format!("peak_mu_y_{name}"), pk);
        out(&format!("peak_alpha_{name}"), at);
        let a95 = lat.iter().find(|(_, m)| *m >= 0.95 * pk).map_or(f32::NAN, |x| x.0);
        out(&format!("alpha95_{name}"), a95);
        for (deg, m) in &lat {
            if [2.5, 5.0, 10.0, 15.0, 17.0, 20.0, 25.0, 30.0, 40.0].contains(deg) {
                out(&format!("mu_y_rel_a{deg}_{name}"), m / pk);
            }
        }
        let (drive, kd) = peak(kappas(), |k| t.mu_x(fz, k));
        let (brake, kb) = peak(kappas(), |k| t.mu_x(fz, -k));
        out(&format!("peak_mu_x_drive_{name}"), drive);
        out(&format!("peak_kappa_drive_{name}"), kd);
        out(&format!("peak_mu_x_brake_{name}"), brake);
        out(&format!("peak_kappa_brake_{name}"), kb);
        out(&format!("peak_kappa_{name}"), if drive >= brake { kd } else { kb });
        for k in [0.3f32, 0.45, 0.5] {
            out(&format!("mu_x_rel_k{k}_brake_{name}"), t.mu_x(fz, -k) / brake);
        }
        out(&format!("mu_x_rel_k0.5_drive_{name}"), t.mu_x(fz, 0.5) / drive);
        out(&format!("mu_x_rel_k1_drive_{name}"), t.mu_x(fz, 1.0) / drive);
        let (locked_x, _) = t.locked(fz, 0.0);
        out(&format!("mu_x_locked_{name}"), locked_x);
        out(&format!("mu_x_locked_rel_{name}"), locked_x / brake);
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
    }
}
