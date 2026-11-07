//! Sweep NGP's pure tyre functions over the runtime parameters of a probed wheel.
//!
//! Research tooling: characterises the model as it runs. Reads a `--tyre-probe`
//! dump (line format, see `rbr-standalone/src/tyre_probe.rs`), takes one
//! wheel's frame at one step, and sweeps `patch_forces` / `friction_limits`
//! (through `rbr-physics`, never copied) over slip and load. Writes CSVs plus a
//! `summary.md` into the output dir (default `/tmp/tyre-lab/ngp`).
//!
//! Sign conventions (the model's, kept raw): with forward motion the contact's
//! longitudinal velocity is negative, free rolling spin is positive
//! (`spin = -vlong / R`), a positive lateral velocity gives a negative lateral
//! force, and driving (faster spin) gives a negative longitudinal force. Peaks
//! and mus below use absolute values.

use std::collections::BTreeMap;

use rbr_physics::ngp::{inclination, outputs};
use rbr_physics::tyre::friction_limits::{BlendPair, LimitsInput};
use rbr_physics::tyre::patch_forces::{PatchForces, PatchInput};
use rbr_physics::tyre::slip::slip_directions;
use rbr_physics::tyre::surface::{surface_friction, SurfaceInput};

/// One wheel's frame at one probe step: the setup tables and the operating point.
#[derive(Clone, Debug, Default)]
struct Params {
    step: u32,
    wheel: u32,
    reference_friction: f32,
    carcass_lat: f32,
    temp_axis: Vec<f32>,
    rub_my: Vec<f32>,
    load_axis: Vec<f32>,
    crn_stf: Vec<f32>,
    trc_stf: Vec<f32>,
    crn_my: Vec<f32>,
    trc_my: Vec<f32>,
    crn_pk: Vec<f32>,
    trc_pk: Vec<f32>,
    sfric: Vec<f32>,
    wet_slope: Vec<f32>,
    wear_ftr: Vec<f32>,
    design_wetness: f32,
    wet_wear_factor: f32,
    static_friction: f32,
    kinetic_friction: f32,
    temperature_grip: f32,
    normal_force: f32,
    patch_length: f32,
    effective_radius: f32,
    stiffness_factor: f32,
    air_pressure: f32,
    tread_temp: f32,
    water_factor: f32,
    wear_op: f32,
}

/// Linear interpolation of `values` on `axis` at `x` (clamped ends).
fn interp(axis: &[f32], values: &[f32], x: f32) -> f32 {
    let n = axis.len().min(values.len());
    if n == 0 {
        return f32::NAN;
    }
    if x <= axis[0] {
        return values[0];
    }
    if x >= axis[n - 1] {
        return values[n - 1];
    }
    for k in 0..n - 1 {
        if x <= axis[k + 1] {
            let f = (x - axis[k]) / (axis[k + 1] - axis[k]);
            return (1.0 - f) * values[k] + values[k + 1] * f;
        }
    }
    values[n - 1]
}

fn parse_floats(rest: &str) -> Vec<f32> {
    rest.split_whitespace().filter_map(|w| w.parse().ok()).collect()
}

/// Parse one probe file into `(step, wheel) -> (Params, patch cloud entry)`.
/// Returns the dumps in file order plus every wheel-0 `(Fz, patch)` pair with
/// clean soil, for the patch-length interpolation.
fn parse_probe(text: &str) -> (Vec<Params>, Vec<(f32, f32)>) {
    let mut dumps: BTreeMap<(u32, u32), BTreeMap<String, String>> = BTreeMap::new();
    let mut step = 0u32;
    let mut wheel = 0u32;
    let mut order: Vec<(u32, u32)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("# step") {
            step = line.split_whitespace().nth(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("wheel:") {
            wheel = rest.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
            dumps.entry((step, wheel)).or_default();
            order.push((step, wheel));
        } else if let Some((k, v)) = line.split_once(':') {
            if let Some(m) = dumps.get_mut(&(step, wheel)) {
                m.insert(k.trim().to_owned(), v.trim().to_owned());
            }
        }
    }
    let get = |m: &BTreeMap<String, String>, k: &str| m.get(k).cloned().unwrap_or_default();
    let table = |m: &BTreeMap<String, String>, k: &str| -> Vec<f32> {
        let v = parse_floats(&get(m, k));
        // Probe table lines start with the entry count.
        if !v.is_empty() && v[0] as usize == v.len() - 1 {
            v[1..].to_vec()
        } else {
            v
        }
    };
    let mut out = Vec::new();
    let mut cloud = Vec::new();
    for ((s, w), m) in &dumps {
        let surf = parse_floats(&get(m, "surface"));
        let op = parse_floats(&get(m, "op"));
        let temps = parse_floats(&get(m, "temps"));
        let misc = parse_floats(&get(m, "misc_setup"));
        let carc = parse_floats(&get(m, "carcass"));
        let cls = parse_floats(&get(m, "tyre_classes"));
        let mut sfric = vec![];
        let mut wslope = vec![];
        let mut wearf = vec![];
        for (i, c) in cls.chunks(3).enumerate() {
            if c.len() == 3 {
                sfric.push(c[0]);
                wslope.push(c[1]);
                wearf.push(c[2]);
            } else {
                let _ = i;
            }
        }
        let wear_line = parse_floats(&get(m, "wear"));
        let p = Params {
            step: *s,
            wheel: *w,
            reference_friction: get(m, "reference_friction").parse().unwrap_or(1.0),
            carcass_lat: carc.first().copied().unwrap_or(f32::NAN),
            temp_axis: table(m, "temperature_axis"),
            rub_my: table(m, "rubber_friction"),
            load_axis: table(m, "load_axis"),
            crn_stf: table(m, "cornering_stiffness"),
            trc_stf: table(m, "traction_stiffness"),
            crn_my: table(m, "cornering_friction"),
            trc_my: table(m, "traction_friction"),
            crn_pk: table(m, "cornering_peak_slip"),
            trc_pk: table(m, "traction_peak_slip"),
            sfric,
            wet_slope: wslope,
            wear_ftr: wearf,
            design_wetness: get(m, "design_wetness").parse().unwrap_or(0.0),
            wet_wear_factor: misc.get(2).copied().unwrap_or(1.0),
            static_friction: surf.first().copied().unwrap_or(f32::NAN),
            kinetic_friction: surf.get(1).copied().unwrap_or(f32::NAN),
            temperature_grip: surf.get(8).copied().unwrap_or(1.0),
            normal_force: op.first().copied().unwrap_or(f32::NAN),
            patch_length: op.get(1).copied().unwrap_or(f32::NAN),
            effective_radius: op.get(2).copied().unwrap_or(0.315),
            stiffness_factor: op.get(4).copied().unwrap_or(1.0),
            air_pressure: op.get(3).copied().unwrap_or(200000.0),
            tread_temp: temps.first().copied().unwrap_or(337.0),
            water_factor: surf.get(6).copied().unwrap_or(0.0),
            wear_op: wear_line.first().copied().unwrap_or(0.0),
        };
        // Patch cloud: wheel 0, clean soil (thickness field index 5 of surface line).
        if *w == 0 && surf.get(5).copied().unwrap_or(1.0) == 0.0 && p.normal_force > 0.0 {
            cloud.push((p.normal_force, p.patch_length));
        }
        out.push(p);
    }
    let _ = order;
    cloud.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    (out, cloud)
}

/// Patch length at `fz`: linear interpolation of the probe's clean-soil cloud,
/// else the probed operating patch length (gravel never runs clean soil).
fn patch_at(cloud: &[(f32, f32)], fz: f32, fallback: f32) -> f32 {
    if cloud.is_empty() {
        return fallback;
    }
    let xs: Vec<f32> = cloud.iter().map(|c| c.0).collect();
    let ys: Vec<f32> = cloud.iter().map(|c| c.1).collect();
    interp(&xs, &ys, fz)
}

struct LoadBlend {
    weight: f32,
    tables: [BlendPair; 4],
    crn_stf: f32,
    trc_stf: f32,
}

fn blend_at(p: &Params, fz: f32) -> LoadBlend {
    // Segment on the load axis (clamped ends, same order as `locate`).
    let ax = &p.load_axis;
    let n = ax.len();
    let (lo, hi, frac) = if fz <= ax[0] {
        (0, 0, 0.0)
    } else if fz >= ax[n - 1] {
        (n - 1, n - 1, 0.0)
    } else {
        let mut lo = 0;
        for k in 0..n - 1 {
            if fz >= ax[k] && fz <= ax[k + 1] {
                lo = k;
                break;
            }
        }
        let f = (fz - ax[lo]) / (ax[lo + 1] - ax[lo]);
        (lo, lo + 1, f)
    };
    let pair = |t: &[f32]| BlendPair { at_a: t[lo], at_b: t[hi] };
    LoadBlend {
        weight: frac,
        tables: [pair(&p.crn_my), pair(&p.trc_my), pair(&p.crn_pk), pair(&p.trc_pk)],
        crn_stf: ((1.0 - frac) * p.crn_stf[lo] + p.crn_stf[hi] * frac) * p.stiffness_factor,
        trc_stf: ((1.0 - frac) * p.trc_stf[lo] + p.trc_stf[hi] * frac) * p.stiffness_factor,
    }
}

fn patch_input(p: &Params, b: &LoadBlend, fz: f32, patch: f32, vlat: f32, vlong: f32, spin: f32) -> PatchInput {
    PatchInput {
        speed: (vlat * vlat + vlong * vlong).sqrt(),
        lateral_velocity: vlat,
        longitudinal_velocity: vlong,
        spin,
        effective_radius: p.effective_radius,
        static_friction: p.static_friction,
        kinetic_friction: p.kinetic_friction,
        reference_friction: p.reference_friction,
        normal_force: fz,
        cornering_stiffness: b.crn_stf,
        traction_stiffness: b.trc_stf,
        inclination_sin: 0.0,
        patch_length: patch,
        sink_depth: 0.0,
        weight: b.weight,
        tables: b.tables,
    }
}

fn limits_at(
    p: &Params,
    b: &LoadBlend,
    vlat: f32,
    vlong: f32,
    spin: f32,
) -> rbr_physics::tyre::friction_limits::FrictionLimits {
    let [lat_share, long_share, _] = slip_directions(spin, vlat, vlong, p.effective_radius);
    outputs::friction_limits(&LimitsInput {
        weight: b.weight,
        cornering_friction: b.tables[0],
        traction_friction: b.tables[1],
        cornering_peak_slip: b.tables[2],
        traction_peak_slip: b.tables[3],
        lateral_share: lat_share,
        longitudinal_share: long_share,
        static_scale: p.static_friction * p.reference_friction,
        kinetic_scale: p.kinetic_friction * p.reference_friction,
        cornering_stiffness: b.crn_stf,
        traction_stiffness: b.trc_stf,
    })
}

fn write(path: &std::path::Path, text: &str) {
    std::fs::write(path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

fn main() {
    let mut probe = String::new();
    let mut step = 0u32;
    let mut wheel = 0u32;
    let mut out = std::path::PathBuf::from("/tmp/tyre-lab/ngp");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--probe" => probe = args.next().expect("--probe PATH"),
            "--step" => step = args.next().expect("--step N").parse().expect("step"),
            "--wheel" => wheel = args.next().expect("--wheel K").parse().expect("wheel"),
            "--out" => out = args.next().expect("--out DIR").into(),
            _ => panic!("unknown arg {a}"),
        }
    }
    assert!(!probe.is_empty(), "--probe PATH required");
    let text = std::fs::read_to_string(&probe).unwrap_or_else(|e| panic!("{probe}: {e}"));
    let (mut dumps, cloud) = parse_probe(&text);
    assert!(!dumps.is_empty(), "no wheel dumps in {probe}");
    dumps.retain(|d| d.load_axis.len() == 8 && d.crn_my.len() == 8 && d.static_friction > 0.0 && d.normal_force > 0.0);
    assert!(!dumps.is_empty(), "no usable dumps (need setup tables)");
    let speed_of = |step: u32, wheel: u32| -> f32 {
        // Speed lives in the dump's slip_op line; re-scan the raw text.
        let mut s = 0u32;
        let mut w = 0u32;
        let mut speed = 0.0f32;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("# step") {
                s = line.split_whitespace().nth(2).and_then(|x| x.parse().ok()).unwrap_or(0);
            } else if let Some(rest) = line.strip_prefix("wheel:") {
                w = rest.split_whitespace().next().and_then(|x| x.parse().ok()).unwrap_or(0);
            } else if s == step && w == wheel {
                if let Some(rest) = line.strip_prefix("slip_op:") {
                    speed = rest.split_whitespace().next().and_then(|x| x.parse().ok()).unwrap_or(0.0);
                }
            }
        }
        speed
    };
    let base = if step == 0 {
        // First driving wheel-0 dump: the operating point at speed.
        dumps
            .iter()
            .find(|d| {
                d.wheel == 0
                    && d.normal_force > 0.0
                    && d.static_friction > 0.0
                    && d.patch_length > 0.0
                    && speed_of(d.step, d.wheel) > 5.0
            })
            .or_else(|| {
                dumps
                    .iter()
                    .find(|d| d.wheel == 0 && d.normal_force > 0.0 && d.static_friction > 0.0 && d.patch_length > 0.0)
            })
            .expect("no wheel-0 dump")
            .clone()
    } else {
        dumps.iter().find(|d| d.step == step && d.wheel == wheel).expect("step/wheel not dumped").clone()
    };
    std::fs::create_dir_all(&out).unwrap_or_else(|e| panic!("{}: {e}", out.display()));
    println!(
        "params: step {} wheel {} Fz {:.0} N static {:.4} kinetic {:.4} ref {:.2} temp_grip {:.4} patch {:.4} m R {:.4} m P {:.0} kPa",
        base.step,
        base.wheel,
        base.normal_force,
        base.static_friction,
        base.kinetic_friction,
        base.reference_friction,
        base.temperature_grip,
        base.patch_length,
        base.effective_radius,
        base.air_pressure / 1000.0,
    );

    let loads = [2000.0f32, 3000.0, 4000.0, 5000.0, 6000.0];
    let v = 15.0f32;
    let r = base.effective_radius;

    // --- lateral sweep: slip angle 0..30 deg --------------------------------
    let mut lat = String::from("Fz_N,alpha_deg,Fy_N,mu_y,Fx_N,trail_m,lat_phase,long_phase\n");
    let mut trail = String::from("Fz_N,alpha_deg,trail_m\n");
    // peak |Fy| per load, and the cornering-stiffness fit points (alpha <= 2 deg)
    let mut peaks: Vec<(f32, f32, f32)> = Vec::new();
    let mut peak_alphas: Vec<(f32, f32)> = Vec::new();
    for &fz in &loads {
        let b = blend_at(&base, fz);
        let patch = patch_at(&cloud, fz, base.patch_length);
        let mut best = (0.0f32, 0.0f32);
        let mut fit: Vec<(f32, f32)> = Vec::new();
        let mut na = 0;
        loop {
            let alpha = na as f32 * 0.25;
            if alpha > 30.0 {
                break;
            }
            na += 1;
            let rad = alpha.to_radians();
            let (vlat, vlong) = (v * rad.sin(), -v * rad.cos());
            let spin = -vlong / r;
            let out_f: PatchForces =
                rbr_physics::tyre::patch_forces::patch_forces(&patch_input(&base, &b, fz, patch, vlat, vlong, spin));
            let fy = out_f.lateral_force.abs();
            if fy > best.0 {
                best = (fy, alpha);
            }
            if alpha <= 2.0 {
                fit.push((rad, out_f.lateral_force));
            }
            lat.push_str(&format!(
                "{fz:.0},{alpha:.2},{:.2},{:.4},{:.2},{:.5},{:.4},{:.4}\n",
                out_f.lateral_force,
                out_f.lateral_force.abs() / fz,
                out_f.longitudinal_force,
                out_f.pneumatic_trail,
                out_f.lateral_phase,
                out_f.longitudinal_phase,
            ));
            trail.push_str(&format!("{fz:.0},{alpha:.2},{:.5}\n", out_f.pneumatic_trail));
        }
        // Cornering stiffness: least-squares slope of Fy over alpha in [0, 2 deg].
        let (mut sxy, mut sxx) = (0.0f64, 0.0f64);
        for (x, y) in &fit {
            // Fy is negative for positive alpha: slope magnitude.
            sxy += f64::from(*x) * f64::from(-*y);
            sxx += f64::from(*x) * f64::from(*x);
        }
        let calpha = (sxy / sxx) as f32;
        peaks.push((fz, best.0, calpha));
        peak_alphas.push((fz, best.1));
    }
    write(&out.join("lat_alpha.csv"), &lat);
    write(&out.join("trail_alpha.csv"), &trail);

    // --- longitudinal sweep: slip ratio -1..1 -------------------------------
    let mut lon = String::from("Fz_N,kappa,Fx_N,mu_x,Fy_N,trail_m,lat_phase,long_phase\n");
    let mut xpeaks: Vec<(f32, f32, f32)> = Vec::new();
    let mut xpeak_k: Vec<(f32, f32)> = Vec::new();
    for &fz in &loads {
        let b = blend_at(&base, fz);
        let patch = patch_at(&cloud, fz, base.patch_length);
        let mut best_drive = (0.0f32, 0.0f32);
        let mut best_brake = (0.0f32, 0.0f32);
        let mut nk = 0;
        loop {
            let kappa = -1.0 + nk as f32 * 0.02;
            if kappa > 1.0 + 1e-6 {
                break;
            }
            nk += 1;
            let vlong = -v;
            let spin = (1.0 + kappa) * v / r;
            let out_f: PatchForces =
                rbr_physics::tyre::patch_forces::patch_forces(&patch_input(&base, &b, fz, patch, 0.0, vlong, spin));
            let fx = out_f.longitudinal_force.abs();
            if kappa >= 0.0 && fx > best_drive.0 {
                best_drive = (fx, kappa);
            }
            if kappa <= 0.0 && fx > best_brake.0 {
                best_brake = (fx, kappa);
            }
            lon.push_str(&format!(
                "{fz:.0},{kappa:.3},{:.2},{:.4},{:.2},{:.5},{:.4},{:.4}\n",
                out_f.longitudinal_force,
                out_f.longitudinal_force.abs() / fz,
                out_f.lateral_force,
                out_f.pneumatic_trail,
                out_f.lateral_phase,
                out_f.longitudinal_phase,
            ));
        }
        // Report the larger of drive/brake peaks (they differ by the end-slip rule).
        let (fp, kp) = if best_drive.0 >= best_brake.0 { best_drive } else { best_brake };
        xpeaks.push((fz, fp, 0.0));
        xpeak_k.push((fz, kp));
    }
    write(&out.join("long_kappa.csv"), &lon);

    // --- combined slip: fixed alpha, sweep kappa -----------------------------
    let mut comb = String::from("alpha_fix_deg,kappa,Fx_N,Fy_N,mu_x,mu_y\n");
    for &afix in &[2.0f32, 4.0, 8.0] {
        let rad = afix.to_radians();
        let (vlat, vlong) = (v * rad.sin(), -v * rad.cos());
        for &fz in &[3000.0f32, 4000.0] {
            let b = blend_at(&base, fz);
            let patch = patch_at(&cloud, fz, base.patch_length);
            let mut nk = 0;
            loop {
                let kappa = -0.5 + nk as f32 * 0.025;
                if kappa > 0.5 + 1e-6 {
                    break;
                }
                nk += 1;
                // Keep the contact speed fixed: rescale spin around free rolling.
                let spin = (1.0 + kappa) * (-vlong) / r;
                let out_f: PatchForces = rbr_physics::tyre::patch_forces::patch_forces(&patch_input(
                    &base, &b, fz, patch, vlat, vlong, spin,
                ));
                comb.push_str(&format!(
                    "{afix:.1},{kappa:.3},{:.2},{:.2},{:.4},{:.4}\n",
                    out_f.longitudinal_force,
                    out_f.lateral_force,
                    out_f.longitudinal_force.abs() / fz,
                    out_f.lateral_force.abs() / fz,
                ));
            }
        }
    }
    write(&out.join("combined.csv"), &comb);

    // --- temperature grip factor vs temperature ------------------------------
    let mut tf = String::from("T_K,rub_my,combined_04_06\n");
    let mut t = 280.0f32;
    while t <= 410.0 {
        let g = interp(&base.temp_axis, &base.rub_my, t);
        tf.push_str(&format!("{t:.1},{g:.4},{:.4}\n", 0.4 * g + 0.6 * g));
        t += 2.0;
    }
    write(&out.join("temp_factor.csv"), &tf);

    // --- wet factor vs WaterFactor, tarmac_dry (kind of base) -----------------
    // surface_friction with both classes at the tyre's class for the probed
    // surface: base + slope * |design - W|, times the wear term at zero wear.
    let mut wf = String::from("W,dry_design_0,wet_design_1\n");
    // Use class 2 (the probed tarmac's class). Kinds compared: base kind (dry)
    // and wetness_design variants are inherent to the base tables; also show a
    // hypothetical wet-design tyre (design 1.0) with the same slopes.
    let class = 2usize.min(base.sfric.len().saturating_sub(1));
    let mut w = 0.0f32;
    while w <= 1.0 + 1e-6 {
        let dry = surface_friction(&SurfaceInput {
            base_first: base.sfric[class],
            base_second: base.sfric[class],
            wet_slope_first: base.wet_slope[class],
            wet_slope_second: base.wet_slope[class],
            wear_factor_first: 1.0,
            wear_factor_second: 1.0,
            design_wetness: base.design_wetness,
            wetness_first: w,
            wetness_second: w,
            sector_wear: 0.0,
        });
        let wet_design = surface_friction(&SurfaceInput {
            design_wetness: 1.0,
            ..SurfaceInput {
                base_first: base.sfric[class],
                base_second: base.sfric[class],
                wet_slope_first: base.wet_slope[class],
                wet_slope_second: base.wet_slope[class],
                wear_factor_first: 1.0,
                wear_factor_second: 1.0,
                design_wetness: 1.0,
                wetness_first: w,
                wetness_second: w,
                sector_wear: 0.0,
            }
        });
        wf.push_str(&format!("{w:.2},{:.4},{:.4}\n", dry.first, wet_design.first));
        w += 0.05;
    }
    write(&out.join("wet_factor.csv"), &wf);

    // --- temperature grip's wet-wear term vs water and wear -------------------
    let mut ww = String::from("water,base_wetwear_wear0,base_wetwear_wearop,gravel_wetwear09_wearop\n");
    let mut w2 = 0.0f32;
    while w2 <= 1.0 + 1e-6 {
        // (((wet_wear - 1) * water + 1) - 1) * wear + 1 (step.rs), for the
        // probed tyre (wet_wear 1.0) and a gravel-like one (0.9).
        let term = |wet_wear: f32, wear: f32| ((wet_wear - 1.0) * w2 + 1.0 - 1.0) * wear + 1.0;
        ww.push_str(&format!(
            "{w2:.2},{:.6},{:.6},{:.6}\n",
            term(base.wet_wear_factor, 0.0),
            term(base.wet_wear_factor, base.wear_op.max(0.001)),
            term(0.9, base.wear_op.max(0.001)),
        ));
        w2 += 0.05;
    }
    write(&out.join("wet_wear_factor.csv"), &ww);

    // --- wear factor (BlendTable wear term) vs tread wear ---------------------
    let mut wr = String::from("wear,blend_term_class\n");
    let mut w3 = 0.0f32;
    while w3 <= 1.0 + 1e-6 {
        let f = (base.wear_ftr[class] - 1.0) * w3 + 1.0;
        wr.push_str(&format!("{w3:.3},{f:.4}\n"));
        w3 += 0.02;
    }
    write(&out.join("wear_factor.csv"), &wr);

    // --- inclination grip scale vs camber --------------------------------------
    let mut inc = String::from("deg,k,heat_lerps\n");
    let mut d = -15.0f64;
    while d <= 15.0 {
        let (k, lerps) = inclination::grip_scale(d);
        inc.push_str(&format!("{d:.1},{k:.5},{lerps}\n"));
        d += 0.5;
    }
    write(&out.join("incl_factor.csv"), &inc);

    // --- mu_slide / mu_peak from the friction limits at 4 kN -------------------
    let b4 = blend_at(&base, 4000.0);
    let rad5 = 5.0f32.to_radians();
    let lim5 = limits_at(&base, &b4, v * rad5.sin(), -v * rad5.cos(), v * rad5.cos() / r);
    // Sliding friction at 30 deg (same load).
    let rad30 = 30.0f32.to_radians();
    let f30: PatchForces = rbr_physics::tyre::patch_forces::patch_forces(&patch_input(
        &base,
        &b4,
        4000.0,
        patch_at(&cloud, 4000.0, base.patch_length),
        v * rad30.sin(),
        -v * rad30.cos(),
        v * rad30.cos() / r,
    ));
    let mu_slide = f30.lateral_force.abs() / 4000.0;
    let mu_peak_lim = lim5.lateral_peak;

    // --- load exponent: peak force ~ Fz^n ---------------------------------------
    let (mut sx, mut sy, mut sxx, mut sxy, mut n) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0usize);
    for (fz, fp, _) in &peaks {
        sx += f64::from(*fz).ln();
        sy += f64::from(*fp).ln();
        n += 1;
    }
    let mx = sx / n as f64;
    let my = sy / n as f64;
    for (fz, fp, _) in &peaks {
        sxx += (f64::from(*fz).ln() - mx).powi(2);
        sxy += (f64::from(*fz).ln() - mx) * (f64::from(*fp).ln() - my);
    }
    let expo = sxy / sxx;

    // --- summary ----------------------------------------------------------------
    let ky = base.carcass_lat * base.stiffness_factor;
    let mut md = String::new();
    md.push_str("# NGP tyre (probed setup): pure-sliding sweep\n\n");
    md.push_str(&format!(
        "Probed wheel {} at step {} ({}): Fz {:.0} N, static {:.4}, kinetic {:.4}, ref {:.2}, \
temp grip {:.4} (tread {:.0} K), patch {:.4} m, R {:.4} m, P {:.0} kPa, water {:.3}, wear {:.4}.\n\n",
        base.wheel,
        base.step,
        probe,
        base.normal_force,
        base.static_friction,
        base.kinetic_friction,
        base.reference_friction,
        base.temperature_grip,
        base.tread_temp,
        patch_at(&cloud, base.normal_force, base.patch_length),
        base.effective_radius,
        base.air_pressure / 1000.0,
        base.water_factor,
        base.wear_op,
    ));
    md.push_str(&format!(
        "Sweep at V = {v:.0} m/s (sliding model), zero camber/sink, stiffness factor {:.4}, \
patch length interpolated from the probe's clean-soil cloud.\n\n",
        base.stiffness_factor
    ));
    md.push_str("## Peak lateral force and cornering stiffness vs load\n\n");
    md.push_str("| Fz (N) | peak Fy (N) | mu_peak | peak alpha (deg) | C_alpha (N/rad) | C_alpha/Fz (1/rad) |\n");
    md.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for (i, (fz, fp, ca)) in peaks.iter().enumerate() {
        md.push_str(&format!(
            "| {:.0} | {:.0} | {:.3} | {:.2} | {:.0} | {:.2} |\n",
            fz,
            fp,
            fp / fz,
            peak_alphas[i].1,
            ca,
            ca / fz,
        ));
    }
    md.push_str(&format!("\nLoad exponent: peak Fy ~ Fz^n, n = {expo:.3} (fit over 2-6 kN).\n\n"));
    md.push_str("## Peak longitudinal force vs load\n\n");
    md.push_str("| Fz (N) | peak |Fx| (N) | mu_peak | peak kappa |\n");
    md.push_str("| --- | --- | --- | --- |\n");
    for (i, (fz, fp, _)) in xpeaks.iter().enumerate() {
        md.push_str(&format!("| {:.0} | {:.0} | {:.3} | {:.3} |\n", fz, fp, fp / fz, xpeak_k[i].1));
    }
    md.push_str(&format!(
        "\nLimits at 4 kN, 5 deg pure lateral: lateral_peak {:.3}, lateral_kinetic {:.3}, long_peak {:.3} \
(long_kinetic {:.3}: the kinetic split follows the slip-velocity direction, so it is 0 with no longitudinal slip). \
mu_slide (30 deg) {:.3}, mu_slide/mu_peak {:.3}.\n\n",
        mu_peak_lim,
        lim5.lateral_kinetic,
        lim5.longitudinal_peak,
        lim5.longitudinal_kinetic,
        mu_slide,
        mu_slide / mu_peak_lim,
    ));
    md.push_str(&format!("Carcass lateral stiffness k_y = {:.0} N/m; relaxation length sigma = C_alpha/k_y: ", ky));
    let mut first = true;
    for (fz, _, ca) in &peaks {
        md.push_str(&format!("{}at {:.0} kN sigma {:.3} m", if first { "" } else { ", " }, fz / 1000.0, ca / ky));
        first = false;
    }
    md.push_str(" (quasi-static estimate; the model lags through its deflection states).\n");
    write(&out.join("summary.md"), &md);
    println!("{md}");
    println!("csv + summary.md in {}", out.display());
}
