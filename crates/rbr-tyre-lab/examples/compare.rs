//! Compare the brush-v1 candidate against NGP's swept tyre and published targets.
//!
//! Research tooling (ours, experimental): reads a `--tyre-probe` dump,
//! evaluates NGP's pure tyre functions (through `rbr-physics`, never copied)
//! Solves the brush-v1 [`Calibration`][brush::Calibration] at run time from
//! NGP's swept peaks/slopes and the probe's nominal point (no NGP-derived
//! numbers live in committed code), evaluates the candidate over the same
//! slips and loads, and writes CSVs plus `summary.md` into the output dir
//! (default `/tmp/tyre-lab/cand`). Lab code only: nothing here changes what
//! the game computes.
//!
//! Conventions: NGP signs are kept raw (positive lateral velocity gives
//! negative lateral force); peaks and mus below use absolute values. The
//! candidate's inputs are normalised to the nominal point (Ford Fiesta
//! Rally2, dry tarmac, 4 kN, 15 m/s), so every correction factor is 1 there
//! and the calibration reproduces NGP's peaks by construction.

use rbr_physics::tyre::patch_forces::{patch_forces, PatchForces, PatchInput};
use rbr_physics::tyre::surface::{surface_friction, SurfaceInput};
use rbr_tyre_lab::brush::{
    self, Calibration, EvalInput, EvalOutput, FILM_MAX, KX_OVER_KY, MU_C_OVER_S, M_CX, M_CY, N_MU_X, N_MU_Y, V_MIN, V_S,
};
use rbr_tyre_lab::calibrate::{calibrate, CalPoint, CalTargets};
use rbr_tyre_lab::ngp::{blend_at, patch_input, LoadBlend};
use rbr_tyre_lab::probe::{interp, parse_probe, patch_at, Params};
use rbr_tyre_lab::write;

// ---------------------------------------------------------------------------
// NGP evaluation glue (`rbr_tyre_lab::ngp`, with the surface friction given).
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn ngp_forces(
    p: &Params,
    b: &LoadBlend,
    stat: f32,
    kin: f32,
    fz: f32,
    patch: f32,
    vlat: f32,
    vlong: f32,
    spin: f32,
) -> PatchForces {
    patch_forces(&PatchInput {
        static_friction: stat,
        kinetic_friction: kin,
        ..patch_input(p, b, fz, patch, vlat, vlong, spin)
    })
}

// ---------------------------------------------------------------------------
// Candidate evaluation glue.
// ---------------------------------------------------------------------------

/// Candidate operating options: bulk temperature K, tread remaining (1 =
/// new), wetness 0..1, NGP temperature grip factor, surface static/kinetic mu.
#[derive(Clone, Copy)]
struct CandOpts {
    tb: f64,
    d: f64,
    w: f64,
    tg: f64,
    ss: f64,
    sk: f64,
}

#[allow(clippy::too_many_arguments)]
fn cand_forces(
    base: &Params,
    cloud: &[(f32, f32)],
    fz: f64,
    vlat: f64,
    vlong: f64,
    spin: f64,
    o: &CandOpts,
    cal: &Calibration,
) -> EvalOutput {
    let a = f64::from(patch_at(cloud, fz as f32, base.patch_length)) / 2.0;
    brush::eval(
        &EvalInput {
            fz,
            half_length: a,
            vx: vlong,
            vy: vlat,
            spin,
            radius: f64::from(base.effective_radius),
            bulk_temp: o.tb,
            tread_remaining: o.d,
            pressure: f64::from(base.air_pressure),
            pressure_factor: f64::from(base.stiffness_factor),
            wetness: o.w,
            temp_grip: o.tg,
            surf_static: o.ss,
            surf_kinetic: o.sk,
        },
        cal,
    )
}

// ---------------------------------------------------------------------------
// Small statistics helpers.
// ---------------------------------------------------------------------------

/// Least-squares slope through the origin.
fn fit_slope(xs: &[f64], ys: &[f64]) -> f64 {
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for (x, y) in xs.iter().zip(ys.iter()) {
        sxy += x * y;
        sxx += x * x;
    }
    sxy / sxx
}

/// Exponent of `vals ~ fz^n` (log-log least squares).
fn log_exponent(fzs: &[f64], vals: &[f64]) -> f64 {
    let n = fzs.len() as f64;
    let (mut sx, mut sy) = (0.0, 0.0);
    for (f, v) in fzs.iter().zip(vals.iter()) {
        sx += f.ln();
        sy += v.ln();
    }
    let (mx, my) = (sx / n, sy / n);
    let (mut sxx, mut sxy) = (0.0, 0.0);
    for (f, v) in fzs.iter().zip(vals.iter()) {
        sxx += (f.ln() - mx).powi(2);
        sxy += (f.ln() - mx) * (v.ln() - my);
    }
    sxy / sxx
}

struct LatRow {
    fz: f64,
    peak: f64,
    alpha_peak: f64,
    calpha: f64,
    f30: f64,
}

struct LonRow {
    fz: f64,
    peak_drive: f64,
    kappa_drive: f64,
    peak_brake: f64,
    kappa_brake: f64,
    cx: f64,
    f05: f64,
}

fn main() {
    let mut probe = String::from("/tmp/tyre-lab/x.probe");
    let mut out = std::path::PathBuf::from("/tmp/tyre-lab/cand");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--probe" => probe = args.next().expect("--probe PATH"),
            "--out" => out = args.next().expect("--out DIR").into(),
            _ => panic!("unknown arg {a}"),
        }
    }
    let text = std::fs::read_to_string(&probe).unwrap_or_else(|e| panic!("{probe}: {e}"));
    let (mut dumps, cloud) = parse_probe(&text);
    assert!(!dumps.is_empty(), "no wheel dumps in {probe}");
    dumps.retain(|d| d.load_axis.len() == 8 && d.crn_my.len() == 8 && d.static_friction > 0.0 && d.normal_force > 0.0);
    let base = dumps
        .iter()
        .find(|d| {
            d.wheel == 0 && d.normal_force > 0.0 && d.static_friction > 0.0 && d.patch_length > 0.0 && d.speed > 5.0
        })
        .or_else(|| dumps.iter().find(|d| d.wheel == 0 && d.static_friction > 0.0 && d.patch_length > 0.0))
        .expect("no wheel-0 dump")
        .clone();
    std::fs::create_dir_all(&out).unwrap_or_else(|e| panic!("{}: {e}", out.display()));
    println!(
        "base: step {} Fz {:.0} N patch {:.4} m R {:.4} m P {:.0} kPa tread {:.1} K wear {:.4} temp_grip {:.4}",
        base.step,
        base.normal_force,
        base.patch_length,
        base.effective_radius,
        base.air_pressure / 1000.0,
        base.tread_temp,
        base.wear_op,
        base.temperature_grip,
    );

    let loads = [2000.0f64, 3000.0, 4000.0, 5000.0, 6000.0];
    let r = f64::from(base.effective_radius);
    let v = 15.0;
    let fz0 = 4000.0;

    // --- lateral sweep, NGP ------------------------------------------------------
    let mut lat_csv = String::from("Fz_N,alpha_deg,Fy_ngp,trail_ngp\n");
    let mut lat_rows: Vec<LatRow> = Vec::new();
    for &fz in &loads {
        let b = blend_at(&base, fz as f32);
        let patch = patch_at(&cloud, fz as f32, base.patch_length);
        let mut best = (0.0f64, 0.0f64);
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        let mut f30 = 0.0;
        let mut na = 0;
        loop {
            let alpha = na as f64 * 0.25;
            if alpha > 30.0 {
                break;
            }
            na += 1;
            let rad = alpha.to_radians();
            let (vlat, vlong) = (v * rad.sin(), -v * rad.cos());
            let spin = -vlong / r;
            let n = ngp_forces(
                &base,
                &b,
                base.static_friction,
                base.kinetic_friction,
                fz as f32,
                patch,
                vlat as f32,
                vlong as f32,
                spin as f32,
            );
            let fyn = f64::from(n.lateral_force).abs();
            if fyn > best.0 {
                best = (fyn, alpha);
            }
            if (alpha - 30.0).abs() < 1e-9 {
                f30 = fyn;
            }
            if alpha <= 2.0 {
                xs.push(rad);
                ys.push(fyn);
            }
            lat_csv.push_str(&format!("{fz:.0},{alpha:.2},{:.2},{:.5}\n", n.lateral_force, n.pneumatic_trail,));
        }
        lat_rows.push(LatRow { fz, peak: best.0, alpha_peak: best.1, calpha: fit_slope(&xs, &ys), f30 });
    }

    // --- longitudinal sweep, NGP ---------------------------------------------------
    let mut lon_csv = String::from("Fz_N,kappa,Fx_ngp\n");
    let mut lon_rows: Vec<LonRow> = Vec::new();
    for &fz in &loads {
        let b = blend_at(&base, fz as f32);
        let patch = patch_at(&cloud, fz as f32, base.patch_length);
        let (mut bd, mut kd) = (0.0f64, 0.0f64);
        let (mut bb, mut kb) = (0.0f64, 0.0f64);
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        let mut f05 = 0.0;
        let mut nk = 0;
        loop {
            let kappa = -1.0 + nk as f64 * 0.02;
            if kappa > 1.0 + 1e-9 {
                break;
            }
            nk += 1;
            let spin = (1.0 + kappa) * v / r;
            let n = ngp_forces(
                &base,
                &b,
                base.static_friction,
                base.kinetic_friction,
                fz as f32,
                patch,
                0.0,
                -v as f32,
                spin as f32,
            );
            let fxn = f64::from(n.longitudinal_force).abs();
            if kappa >= 0.0 && fxn > bd {
                bd = fxn;
                kd = kappa;
            }
            if kappa <= 0.0 && fxn > bb {
                bb = fxn;
                kb = kappa;
            }
            if (kappa - 0.5).abs() < 1e-9 {
                f05 = fxn;
            }
            // Fit window |kappa| in {0.02, 0.04} by closeness: 0.02 is not
            // exact in binary, so the raw grid values overshoot 0.04 and a
            // plain <= comparison silently drops them (leaving a tangent).
            if ((kappa.abs() - 0.02).abs() < 1e-9 || (kappa.abs() - 0.04).abs() < 1e-9) && kappa != 0.0 {
                xs.push(kappa.abs());
                ys.push(fxn);
            }
            lon_csv.push_str(&format!("{fz:.0},{kappa:.3},{:.2}\n", n.longitudinal_force));
        }
        lon_rows.push(LonRow {
            fz,
            peak_drive: bd,
            kappa_drive: kd,
            peak_brake: bb,
            kappa_brake: kb,
            cx: fit_slope(&xs, &ys),
            f05,
        });
    }

    // --- runtime calibration ---------------------------------------------------------
    // Nominal point from the probe dump; NGP targets from the sweeps above.
    let point = CalPoint {
        t_cal: f64::from(base.tread_temp),
        d_cal: 1.0 - f64::from(base.wear_op),
        p_cal: f64::from(base.air_pressure),
        pf_cal: f64::from(base.stiffness_factor),
        w_cal: 0.0,
        surf_s_cal: f64::from(base.static_friction),
        surf_k_cal: f64::from(base.kinetic_friction),
        temp_grip_cal: f64::from(base.temperature_grip),
    };
    let targets = CalTargets {
        fz0,
        v,
        peak_lat: lat_rows[2].peak,
        fit_lat: lat_rows[2].calpha,
        peak_lon: lon_rows[2].peak_drive,
    };
    let cal = calibrate(&targets, &point);
    println!(
        "calibrated: Cy0 {:.1} N/rad, Cx0 {:.1} N (x {KX_OVER_KY}), mu_s/mu_k lateral {:.4}/{:.4}, longitudinal {:.4}/{:.4} (ratio {:.3})",
        cal.cy0,
        cal.cx0,
        cal.mu_ys,
        cal.mu_yk,
        cal.mu_xs,
        cal.mu_xk,
        MU_C_OVER_S,
    );

    // Dry nominal options: the probe's own surface and grip, so every factor
    // is 1 at the nominal point by construction.
    let dry_opts = || CandOpts {
        tb: f64::from(base.tread_temp),
        d: 1.0 - f64::from(base.wear_op),
        w: 0.0,
        tg: f64::from(base.temperature_grip),
        ss: f64::from(base.static_friction),
        sk: f64::from(base.kinetic_friction),
    };

    // --- lateral sweep, candidate --------------------------------------------------
    let mut lat_c_csv = String::from("Fz_N,alpha_deg,Fy_cand,mu_cand,trail_cand\n");
    let mut lat_cand_rows: Vec<LatRow> = Vec::new();
    for &fz in &loads {
        let o = dry_opts();
        let mut best = (0.0f64, 0.0f64);
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        let mut f30 = 0.0;
        let mut na = 0;
        loop {
            let alpha = na as f64 * 0.25;
            if alpha > 30.0 {
                break;
            }
            na += 1;
            let rad = alpha.to_radians();
            let (vlat, vlong) = (v * rad.sin(), -v * rad.cos());
            let spin = -vlong / r;
            let c = cand_forces(&base, &cloud, fz, vlat, vlong, spin, &o, &cal);
            if c.fy.abs() > best.0 {
                best = (c.fy.abs(), alpha);
            }
            if (alpha - 30.0).abs() < 1e-9 {
                f30 = c.fy.abs();
            }
            if alpha <= 2.0 {
                xs.push(rad);
                ys.push(c.fy.abs());
            }
            lat_c_csv.push_str(&format!("{fz:.0},{alpha:.2},{:.2},{:.4},{:.5}\n", c.fy, c.fy.abs() / fz, c.trail));
        }
        lat_cand_rows.push(LatRow { fz, peak: best.0, alpha_peak: best.1, calpha: fit_slope(&xs, &ys), f30 });
    }
    write(&out.join("lat_alpha.csv"), &lat_c_csv);

    // --- longitudinal sweep, candidate -----------------------------------------------
    let mut lon_c_csv = String::from("Fz_N,kappa,Fx_cand,mu_cand\n");
    let mut lon_cand_rows: Vec<LonRow> = Vec::new();
    for &fz in &loads {
        let o = dry_opts();
        let (mut cd, mut kd_c) = (0.0f64, 0.0f64);
        let (mut cb, mut kb_c) = (0.0f64, 0.0f64);
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        let mut f05 = 0.0;
        let mut nk = 0;
        loop {
            let kappa = -1.0 + nk as f64 * 0.02;
            if kappa > 1.0 + 1e-9 {
                break;
            }
            nk += 1;
            let spin = (1.0 + kappa) * v / r;
            let c = cand_forces(&base, &cloud, fz, 0.0, -v, spin, &o, &cal);
            if kappa >= 0.0 && c.fx.abs() > cd {
                cd = c.fx.abs();
                kd_c = kappa;
            }
            if kappa <= 0.0 && c.fx.abs() > cb {
                cb = c.fx.abs();
                kb_c = kappa;
            }
            if (kappa - 0.5).abs() < 1e-9 {
                f05 = c.fx.abs();
            }
            // Same closeness window as the NGP sweep (see above).
            if ((kappa.abs() - 0.02).abs() < 1e-9 || (kappa.abs() - 0.04).abs() < 1e-9) && kappa != 0.0 {
                xs.push(kappa.abs());
                ys.push(c.fx.abs());
            }
            lon_c_csv.push_str(&format!("{fz:.0},{kappa:.3},{:.2},{:.4}\n", c.fx, c.fx.abs() / fz));
        }
        lon_cand_rows.push(LonRow {
            fz,
            peak_drive: cd,
            kappa_drive: kd_c,
            peak_brake: cb,
            kappa_brake: kb_c,
            cx: fit_slope(&xs, &ys),
            f05,
        });
    }
    write(&out.join("long_kappa.csv"), &lon_c_csv);
    // Keep the NGP-only sweep files identical in shape to before.
    write(&out.join("lat_ngp.csv"), &lat_csv);
    write(&out.join("long_ngp.csv"), &lon_csv);

    // Load exponents.
    let fzs: Vec<f64> = loads.to_vec();
    let n_ngp = log_exponent(&fzs, &lat_rows.iter().map(|x| x.peak).collect::<Vec<_>>());
    let n_cand = log_exponent(&fzs, &lat_cand_rows.iter().map(|x| x.peak).collect::<Vec<_>>());
    let e_ngp = log_exponent(&fzs, &lat_rows.iter().map(|x| x.calpha).collect::<Vec<_>>());
    let e_cand = log_exponent(&fzs, &lat_cand_rows.iter().map(|x| x.calpha).collect::<Vec<_>>());
    let nx_ngp = log_exponent(&fzs, &lon_rows.iter().map(|x| x.peak_drive).collect::<Vec<_>>());
    let nx_cand = log_exponent(&fzs, &lon_cand_rows.iter().map(|x| x.peak_drive).collect::<Vec<_>>());
    let ex_ngp = log_exponent(&fzs, &lon_rows.iter().map(|x| x.cx).collect::<Vec<_>>());
    let ex_cand = log_exponent(&fzs, &lon_cand_rows.iter().map(|x| x.cx).collect::<Vec<_>>());

    // --- peak mu_y vs speed ----------------------------------------------------
    let mut speed_csv = String::from("V,peak_mu_ngp,peak_mu_cand\n");
    let mut speed_rows: Vec<(f64, f64, f64)> = Vec::new();
    for vv in [10.0, 15.0, 20.0, 30.0] {
        let b = blend_at(&base, fz0 as f32);
        let patch = patch_at(&cloud, fz0 as f32, base.patch_length);
        let o = dry_opts();
        let (mut bn, mut bc) = (0.0f64, 0.0f64);
        let mut na = 0;
        loop {
            let alpha = na as f64 * 0.5;
            if alpha > 30.0 {
                break;
            }
            na += 1;
            let rad = alpha.to_radians();
            let (vlat, vlong) = (vv * rad.sin(), -vv * rad.cos());
            let spin = -vlong / r;
            let n = ngp_forces(
                &base,
                &b,
                base.static_friction,
                base.kinetic_friction,
                fz0 as f32,
                patch,
                vlat as f32,
                vlong as f32,
                spin as f32,
            );
            let c = cand_forces(&base, &cloud, fz0, vlat, vlong, spin, &o, &cal);
            bn = bn.max(f64::from(n.lateral_force).abs() / fz0);
            bc = bc.max(c.fy.abs() / fz0);
        }
        speed_csv.push_str(&format!("{vv:.0},{bn:.4},{bc:.4}\n"));
        speed_rows.push((vv, bn, bc));
    }
    write(&out.join("speed.csv"), &speed_csv);

    // --- trail at 1 deg and at the peak (4 kN) -----------------------------------
    let mut trail_csv = String::from("alpha_deg,trail_ngp,trail_cand\n");
    let o = dry_opts();
    let b = blend_at(&base, fz0 as f32);
    let patch = patch_at(&cloud, fz0 as f32, base.patch_length);
    let mut na = 0;
    loop {
        let alpha = na as f64 * 0.25;
        if alpha > 30.0 {
            break;
        }
        na += 1;
        let rad = alpha.to_radians();
        let (vlat, vlong) = (v * rad.sin(), -v * rad.cos());
        let spin = -vlong / r;
        let n = ngp_forces(
            &base,
            &b,
            base.static_friction,
            base.kinetic_friction,
            fz0 as f32,
            patch,
            vlat as f32,
            vlong as f32,
            spin as f32,
        );
        let c = cand_forces(&base, &cloud, fz0, vlat, vlong, spin, &o, &cal);
        trail_csv.push_str(&format!("{alpha:.2},{:.5},{:.5}\n", n.pneumatic_trail, c.trail));
    }
    let at = |alpha: f64| -> (f64, f64) {
        let rad = alpha.to_radians();
        let (vlat, vlong) = (v * rad.sin(), -v * rad.cos());
        let spin = -vlong / r;
        let n = ngp_forces(
            &base,
            &b,
            base.static_friction,
            base.kinetic_friction,
            fz0 as f32,
            patch,
            vlat as f32,
            vlong as f32,
            spin as f32,
        );
        let c = cand_forces(&base, &cloud, fz0, vlat, vlong, spin, &o, &cal);
        (f64::from(n.pneumatic_trail), c.trail)
    };
    let (trail_1n, trail_1c) = at(1.0);
    // NGP's trail keeps falling past its force peak (sliding phase), while the
    // brush moment is 0 once saturated: each model at its own peak.
    let (trail_pn, _) = at(lat_rows[2].alpha_peak);
    let (_, trail_pc) = at(lat_cand_rows[2].alpha_peak);
    write(&out.join("trail.csv"), &trail_csv);

    // --- combined slip at fixed alpha --------------------------------------------
    let mut comb_csv = String::from("alpha_fix,kappa,Fx_ngp,Fy_ngp,Fx_cand,Fy_cand\n");
    let mut comb_rows: Vec<(f64, f64, f64, f64, f64, f64)> = Vec::new();
    for &afix in &[2.0f64, 4.0, 8.0] {
        let rad = afix.to_radians();
        let (vlat, vlong) = (v * rad.sin(), -v * rad.cos());
        for &fz in &[3000.0f64, 4000.0] {
            let b = blend_at(&base, fz as f32);
            let patch = patch_at(&cloud, fz as f32, base.patch_length);
            let o = dry_opts();
            let mut nk = 0;
            loop {
                let kappa = -0.5 + nk as f64 * 0.025;
                if kappa > 0.5 + 1e-9 {
                    break;
                }
                nk += 1;
                let spin = (1.0 + kappa) * (-vlong) / r;
                let n = ngp_forces(
                    &base,
                    &b,
                    base.static_friction,
                    base.kinetic_friction,
                    fz as f32,
                    patch,
                    vlat as f32,
                    vlong as f32,
                    spin as f32,
                );
                let c = cand_forces(&base, &cloud, fz, vlat, vlong, spin, &o, &cal);
                comb_csv.push_str(&format!(
                    "{afix:.1},{kappa:.3},{:.2},{:.2},{:.2},{:.2}\n",
                    n.longitudinal_force, n.lateral_force, c.fx, c.fy,
                ));
                if (fz - fz0).abs() < 1e-9 && [-0.25, 0.0, 0.25].contains(&kappa) {
                    comb_rows.push((
                        afix,
                        kappa,
                        f64::from(n.lateral_force).abs(),
                        f64::from(n.longitudinal_force).abs(),
                        c.fy.abs(),
                        c.fx.abs(),
                    ));
                }
            }
        }
    }
    write(&out.join("combined.csv"), &comb_csv);

    // --- relaxation lengths --------------------------------------------------------
    let k_y = f64::from(base.carcass_lat) * f64::from(base.stiffness_factor);
    let k_x = f64::from(base.carcass_tor) * f64::from(base.stiffness_factor) / (r * r);
    let cy_fz0 = brush::stiffness_at(fz0, point.t_cal, point.d_cal, point.pf_cal, cal.cy0, M_CY, &cal);
    let cx_fz0 = brush::stiffness_at(fz0, point.t_cal, point.d_cal, point.pf_cal, cal.cx0, M_CX, &cal);
    let ngp_sigma = lat_rows[2].calpha / k_y;
    let mut sigma_csv = String::from("V,sigma_ngp,sigma_y_cand,sigma_x_cand\n");
    let mut sigma_rows: Vec<(f64, f64, f64)> = Vec::new();
    for vv in [5.0, 10.0, 20.0, 30.0] {
        let (sy, sx) = brush::sigma_lengths(cy_fz0, cx_fz0, k_y, k_x);
        sigma_csv.push_str(&format!("{vv:.0},{ngp_sigma:.4},{sy:.4},{sx:.4}\n"));
        sigma_rows.push((vv, sy, sx));
    }
    write(&out.join("sigma.csv"), &sigma_csv);

    // --- cold vs hot bulk temperature -------------------------------------------------
    // NGP temperature grip: 0.4 tread + 0.6 surface (step.rs weights); the
    // surface term stays at the probe value, the tread term follows RubMyFct.
    let rub = |t: f32| interp(&base.temp_axis, &base.rub_my, t);
    let grip = |tb: f64| {
        f64::from(base.temperature_grip) + 0.4 * (f64::from(rub(tb as f32)) - f64::from(rub(point.t_cal as f32)))
    };
    let mut temp_csv = String::from("case,calpha_ngp,calpha_cand,mu_ngp,mu_cand\n");
    let mut temp_rows: Vec<(&str, f64, f64, f64, f64)> = Vec::new();
    for (name, tb) in [("cold 293K", 293.15), ("nominal", point.t_cal), ("hot 340K", 340.0)] {
        let g = grip(tb);
        let b = blend_at(&base, fz0 as f32);
        let patch = patch_at(&cloud, fz0 as f32, base.patch_length);
        // NGP: stiffness untouched, mu scales with the grip factor.
        let n_peak = {
            let mut best = 0.0f64;
            let mut na = 0;
            loop {
                let alpha = na as f64 * 0.5;
                if alpha > 30.0 {
                    break;
                }
                na += 1;
                let rad = alpha.to_radians();
                let (vlat, vlong) = (v * rad.sin(), -v * rad.cos());
                let spin = -vlong / r;
                let sc = (base.static_friction as f64 * g / f64::from(base.temperature_grip)) as f32;
                let kc = (base.kinetic_friction as f64 * g / f64::from(base.temperature_grip)) as f32;
                let n = ngp_forces(&base, &b, sc, kc, fz0 as f32, patch, vlat as f32, vlong as f32, spin as f32);
                best = best.max(f64::from(n.lateral_force).abs() / fz0);
            }
            best
        };
        let o = CandOpts { tb, tg: g, ..dry_opts() };
        let mut best = 0.0f64;
        let mut na = 0;
        loop {
            let alpha = na as f64 * 0.5;
            if alpha > 30.0 {
                break;
            }
            na += 1;
            let rad = alpha.to_radians();
            let (vlat, vlong) = (v * rad.sin(), -v * rad.cos());
            let spin = -vlong / r;
            best = best.max(cand_forces(&base, &cloud, fz0, vlat, vlong, spin, &o, &cal).fy.abs() / fz0);
        }
        let calpha_c = brush::stiffness_at(fz0, tb, point.d_cal, point.pf_cal, cal.cy0, M_CY, &cal);
        // NGP cornering stiffness has no bulk-temperature term.
        temp_csv.push_str(&format!("{name},{:.0},{:.0},{:.4},{:.4}\n", lat_rows[2].calpha, calpha_c, n_peak, best));
        temp_rows.push((name, lat_rows[2].calpha, calpha_c, n_peak, best));
    }
    write(&out.join("temp.csv"), &temp_csv);

    // --- new vs 60% worn ---------------------------------------------------------------
    // NGP: stiffness untouched; surface scales by the class-2 wear term at
    // 0.6. Candidate: own stiffness/friction wear factors at d = 0.4.
    let mut wear_csv = String::from("case,calpha_ngp,calpha_cand,mu_ngp,mu_cand\n");
    let mut wear_rows: Vec<(&str, f64, f64, f64, f64)> = Vec::new();
    for (name, d_cand, wear_ngp) in [("new", 1.0, 0.0f64), ("60pct worn", 0.4, 0.6f64)] {
        let class = 2usize.min(base.wear_ftr.len().saturating_sub(1));
        let wf = (base.wear_ftr[class] as f64 - 1.0) * wear_ngp + 1.0;
        let b = blend_at(&base, fz0 as f32);
        let patch = patch_at(&cloud, fz0 as f32, base.patch_length);
        let sc = (base.static_friction as f64 * wf) as f32;
        let kc = (base.kinetic_friction as f64 * wf) as f32;
        let mut best_n = 0.0f64;
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        let mut na = 0;
        loop {
            let alpha = na as f64 * 0.25;
            if alpha > 30.0 {
                break;
            }
            na += 1;
            let rad = alpha.to_radians();
            let (vlat, vlong) = (v * rad.sin(), -v * rad.cos());
            let spin = -vlong / r;
            let n = ngp_forces(&base, &b, sc, kc, fz0 as f32, patch, vlat as f32, vlong as f32, spin as f32);
            let fyn = f64::from(n.lateral_force).abs();
            best_n = best_n.max(fyn / fz0);
            if alpha <= 2.0 {
                xs.push(rad);
                ys.push(fyn);
            }
        }
        let o = CandOpts { d: d_cand, ..dry_opts() };
        let mut best_c = 0.0f64;
        let (mut xc, mut yc) = (Vec::new(), Vec::new());
        let mut na = 0;
        loop {
            let alpha = na as f64 * 0.25;
            if alpha > 30.0 {
                break;
            }
            na += 1;
            let rad = alpha.to_radians();
            let (vlat, vlong) = (v * rad.sin(), -v * rad.cos());
            let spin = -vlong / r;
            let fyc = cand_forces(&base, &cloud, fz0, vlat, vlong, spin, &o, &cal).fy.abs();
            best_c = best_c.max(fyc / fz0);
            if alpha <= 2.0 {
                xc.push(rad);
                yc.push(fyc);
            }
        }
        wear_csv.push_str(&format!(
            "{name},{:.0},{:.0},{:.4},{:.4}\n",
            fit_slope(&xs, &ys),
            fit_slope(&xc, &yc),
            best_n,
            best_c
        ));
        wear_rows.push((name, fit_slope(&xs, &ys), fit_slope(&xc, &yc), best_n, best_c));
    }
    write(&out.join("wear.csv"), &wear_csv);

    // --- wet: peak mu_y vs speed ----------------------------------------------------------
    // NGP surface: stage tag by wetness (damp 38 at W = 0.25, wet 39 at W =
    // 1) times the dry tyre's own wet slope / wear factor from
    // surface_friction. Speed-independent. Candidate: same surface mu plus
    // the hydroplaning-style loss.
    let mut wet_csv = String::from("W,V,d,peak_mu_ngp,peak_mu_cand\n");
    let mut wet_rows: Vec<(f64, f64, f64, f64, f64)> = Vec::new();
    for &w in &[0.25, 1.0] {
        let (tag_s, tag_k) = if w < 0.5 { (1.2025, 0.944) } else { (1.105, 0.8508) };
        // The dry tyre's own wet slope / wear factor (class 2, the probed
        // tarmac's class), through NGP's surface_friction.
        let class = 2usize.min(base.wet_slope.len().saturating_sub(1));
        for (dname, d_cand, wear_ngp) in [("new", 1.0, 0.0), ("worn", 0.4, 0.6)] {
            let tyre = surface_friction(&SurfaceInput {
                base_first: 1.0,
                base_second: 1.0,
                wet_slope_first: base.wet_slope[class],
                wet_slope_second: base.wet_slope[class],
                wear_factor_first: base.wear_ftr[class],
                wear_factor_second: base.wear_ftr[class],
                design_wetness: base.design_wetness,
                wetness_first: w as f32,
                wetness_second: w as f32,
                sector_wear: wear_ngp as f32,
            });
            let ss = tag_s * f64::from(tyre.first) * f64::from(base.temperature_grip);
            let sk = tag_k * f64::from(tyre.first) * f64::from(base.temperature_grip);
            for vv in [10.0, 15.0, 20.0, 30.0, 40.0] {
                let b = blend_at(&base, fz0 as f32);
                let patch = patch_at(&cloud, fz0 as f32, base.patch_length);
                let mut best_n = 0.0f64;
                let mut na = 0;
                loop {
                    let alpha = na as f64 * 0.5;
                    if alpha > 30.0 {
                        break;
                    }
                    na += 1;
                    let rad = alpha.to_radians();
                    let (vlat, vlong) = (vv * rad.sin(), -vv * rad.cos());
                    let spin = -vlong / r;
                    let n = ngp_forces(
                        &base,
                        &b,
                        ss as f32,
                        sk as f32,
                        fz0 as f32,
                        patch,
                        vlat as f32,
                        vlong as f32,
                        spin as f32,
                    );
                    best_n = best_n.max(f64::from(n.lateral_force).abs() / fz0);
                }
                let o = CandOpts { w, d: d_cand, ss, sk, ..dry_opts() };
                let mut best_c = 0.0f64;
                let mut na = 0;
                loop {
                    let alpha = na as f64 * 0.5;
                    if alpha > 30.0 {
                        break;
                    }
                    na += 1;
                    let rad = alpha.to_radians();
                    let (vlat, vlong) = (vv * rad.sin(), -vv * rad.cos());
                    let spin = -vlong / r;
                    best_c = best_c.max(cand_forces(&base, &cloud, fz0, vlat, vlong, spin, &o, &cal).fy.abs() / fz0);
                }
                wet_csv.push_str(&format!("{w:.2},{vv:.0},{dname},{best_n:.4},{best_c:.4}\n"));
                wet_rows.push((w, vv, d_cand, best_n, best_c));
            }
        }
    }
    write(&out.join("wet.csv"), &wet_csv);

    // --- locked wheel over peak mu_x at 10 m/s ----------------------------------------------
    // Vs = 10 m/s at lock (kappa = -1): NGP's sliding mu over its drive peak
    // vs the candidate's, dry and wet, against the published ratios in the
    // summary.
    let mut lock_csv = String::from("case,mu_peak_ngp,mu_lock_ngp,mu_peak_cand,mu_lock_cand\n");
    let mut lock_rows: Vec<(&str, f64, f64, f64, f64)> = Vec::new();
    for (name, ss, sk, w) in [
        ("dry", f64::from(base.static_friction), f64::from(base.kinetic_friction), 0.0),
        (
            "wet W=1",
            1.105 * (1.0 + f64::from(base.wet_slope[class_of(&base)]) * 1.0) * f64::from(base.temperature_grip),
            0.8508 * (1.0 + f64::from(base.wet_slope[class_of(&base)]) * 1.0) * f64::from(base.temperature_grip),
            1.0,
        ),
    ] {
        let vv = 10.0;
        let b = blend_at(&base, fz0 as f32);
        let patch = patch_at(&cloud, fz0 as f32, base.patch_length);
        // Drive peak at this speed.
        let mut peak_n = 0.0f64;
        let mut k = 0.0;
        while k <= 1.0 {
            let spin = (1.0 + k) * vv / r;
            let n = ngp_forces(&base, &b, ss as f32, sk as f32, fz0 as f32, patch, 0.0, -vv as f32, spin as f32);
            peak_n = peak_n.max(f64::from(n.longitudinal_force).abs() / fz0);
            k += 0.02;
        }
        let n_lock = ngp_forces(&base, &b, ss as f32, sk as f32, fz0 as f32, patch, 0.0, -vv as f32, 0.0);
        let lock_n = f64::from(n_lock.longitudinal_force).abs() / fz0;
        let o = CandOpts { w, ss, sk, ..dry_opts() };
        let mut peak_c = 0.0f64;
        let mut k = 0.0;
        while k <= 1.0 {
            let spin = (1.0 + k) * vv / r;
            peak_c = peak_c.max(cand_forces(&base, &cloud, fz0, 0.0, -vv, spin, &o, &cal).fx.abs() / fz0);
            k += 0.02;
        }
        let c_lock = cand_forces(&base, &cloud, fz0, 0.0, -vv, 0.0, &o, &cal);
        let lock_c = c_lock.fx.abs() / fz0;
        lock_csv.push_str(&format!("{name},{peak_n:.4},{lock_n:.4},{peak_c:.4},{lock_c:.4}\n"));
        lock_rows.push((name, peak_n, lock_n, peak_c, lock_c));
    }
    write(&out.join("locked.csv"), &lock_csv);

    // --- step steer: 2 deg at 10/20/30 m/s, distance to 63% -------------------------------
    let mut step_csv = String::from("V,sigma_m,x63_m\n");
    let mut step_rows: Vec<(f64, f64, f64)> = Vec::new();
    for vv in [10.0, 20.0, 30.0] {
        let alpha = 2.0f64.to_radians();
        let vsy = vv * alpha.sin();
        let vx = -vv * alpha.cos();
        let vr = vv * alpha.cos();
        let cy = brush::stiffness_at(fz0, point.t_cal, point.d_cal, point.pf_cal, cal.cy0, M_CY, &cal);
        let cx = brush::stiffness_at(fz0, point.t_cal, point.d_cal, point.pf_cal, cal.cx0, M_CX, &cal);
        let (sig_y, sig_x) = brush::sigma_lengths(cy, cx, k_y, k_x);
        let o = dry_opts();
        // Steady force at the step slip.
        let spin = -vx / r;
        let f_ss = cand_forces(&base, &cloud, fz0, vv * alpha.sin(), vx, spin, &o, &cal).fy.abs();
        let dt = 1.0 / 720.0;
        let mut s = brush::TransState::default();
        let mut dist = 0.0;
        for _ in 0..7200 {
            brush::step_transient(&mut s, 0.0, vsy, vr, sig_x, sig_y, dt);
            // Force through the transient slip: feed eval the sliding
            // velocity the transient state implies (exact in the linear
            // regime where the 63% point sits).
            let vy_in = s.uy / sig_y * vr;
            let f = cand_forces(&base, &cloud, fz0, vy_in, vx, spin, &o, &cal).fy.abs();
            dist += vv * dt;
            if f >= 0.63 * f_ss {
                break;
            }
        }
        step_csv.push_str(&format!("{vv:.0},{sig_y:.4},{dist:.4}\n"));
        step_rows.push((vv, sig_y, dist));
    }
    write(&out.join("step.csv"), &step_csv);

    // --- summary ------------------------------------------------------------------------------
    let mut md = String::new();
    md.push_str("# brush-v1 vs NGP vs published targets\n\n");
    md.push_str(&format!(
        "Nominal point: Ford Fiesta Rally2, dry tarmac, {:.0} N, V = {:.0} m/s, R {:.4} m, P {:.0} kPa, \
bulk {:.1} K, tread remaining {:.4}, NGP temp grip {:.4}. Calibration solved at run time from the probe dump \
and NGP's sweep (no NGP-derived numbers in committed code).\n\n",
        fz0,
        v,
        r,
        base.air_pressure / 1000.0,
        point.t_cal,
        point.d_cal,
        point.temp_grip_cal,
    ));
    md.push_str(&format!(
        "Calibrated to NGP's lateral peak and slope and longitudinal peak: Cy0 {:.1} N/rad; Cx0 {:.1} N \
({KX_OVER_KY} Cy0, published); mu_s/mu_k lateral {:.4}/{:.4}, longitudinal {:.4}/{:.4} (Coulomb/static \
{:.3}, published).\n\n",
        cal.cy0, cal.cx0, cal.mu_ys, cal.mu_yk, cal.mu_xs, cal.mu_xk, MU_C_OVER_S,
    ));
    md.push_str("Signs: NGP raw (positive lateral velocity -> negative force); peaks/mus are absolute values.\n\n");

    md.push_str("## peak mu_y vs load (V = 15 m/s)\n\n");
    md.push_str("| Fz (N) | NGP | brush-v1 | target |\n| --- | --- | --- | --- |\n");
    for (n, c) in lat_rows.iter().zip(lat_cand_rows.iter()) {
        md.push_str(&format!("| {:.0} | {:.3} | {:.3} | |\n", n.fz, n.peak / n.fz, c.peak / c.fz));
    }
    md.push_str(&format!(
        "\nLoad exponent peak Fy ~ Fz^n: NGP {n_ngp:.3} | brush-v1 {n_cand:.3} | ~0.87 \
(Singh & Sivaramakrishnan load sweep).\n\n"
    ));

    md.push_str("## cornering stiffness vs load (fit over alpha <= 2 deg)\n\n");
    md.push_str("| Fz (N) | NGP C_alpha | brush-v1 C_alpha |\n| --- | --- | --- |\n");
    for (n, c) in lat_rows.iter().zip(lat_cand_rows.iter()) {
        md.push_str(&format!("| {:.0} | {:.0} | {:.0} |\n", n.fz, n.calpha, c.calpha));
    }
    md.push_str(&format!(
        "\nLoad exponent C_alpha ~ Fz^m: NGP {e_ngp:.3} | brush-v1 {e_cand:.3} (M_CY = {M_CY}) | ~0.6 (same paper); \
0.47/0.69 (MF 6.1 255/55R20 example / Pacejka-book passenger set).\n\n"
    ));

    md.push_str("## peak slip angle (lateral) and peak slip ratio (longitudinal)\n\n");
    md.push_str(
        "| Fz (N) | NGP alpha | brush-v1 alpha | NGP kappa drive/brake | brush-v1 kappa drive/brake | target |\n",
    );
    md.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for (((n, c), l), lc) in lat_rows.iter().zip(lat_cand_rows.iter()).zip(lon_rows.iter()).zip(lon_cand_rows.iter()) {
        md.push_str(&format!(
            "| {:.0} | {:.2} deg | {:.2} deg | {:.3}/{:.3} | {:.3}/{:.3} | 6.5/8.5 deg, 0.094/0.139 at Fz0 (MF 6.1 / Pacejka-book sets) |\n",
            l.fz, n.alpha_peak, c.alpha_peak, l.kappa_drive, l.kappa_brake, lc.kappa_drive, lc.kappa_brake,
        ));
    }
    md.push_str(&format!(
        "\nLoad exponent peak Fx ~ Fz^n: NGP {nx_ngp:.3} | brush-v1 {nx_cand:.3} (N_MU_X = {N_MU_X}) | 0.85/0.95 \
(MF 6.1 / Pacejka-book sets, from Fz0 to 2 Fz0).\n"
    ));
    md.push_str("\n## traction stiffness vs load (fit over |kappa| <= 0.04) and brake peaks\n\n");
    md.push_str("| Fz (N) | NGP C_x | brush-v1 C_x | NGP brake peak (N) | brush-v1 brake peak (N) |\n| --- | --- | --- | --- | --- |\n");
    for (l, lc) in lon_rows.iter().zip(lon_cand_rows.iter()) {
        md.push_str(&format!(
            "| {:.0} | {:.0} | {:.0} | {:.0} @ {:.3} | {:.0} @ {:.3} |\n",
            l.fz, l.cx, lc.cx, l.peak_brake, l.kappa_brake, lc.peak_brake, lc.kappa_brake,
        ));
    }
    md.push_str(&format!(
        "\nC_x/Fz at 4 kN: NGP {:.1} | brush-v1 {:.1} | 32.9/18.4 at Fz0 (MF 6.1 / Pacejka-book sets). Load exponent \
C_x ~ Fz^m: NGP {ex_ngp:.3} | brush-v1 {ex_cand:.3} (M_CX = {M_CX}) | 1.31/1.08.\n",
        lon_rows[2].cx / fz0,
        lon_cand_rows[2].cx / fz0,
    ));
    md.push_str("\n## mu_slide(30 deg)/mu_peak at 15 m/s, 4 kN\n\n");
    md.push_str("| | NGP | brush-v1 | target |\n| --- | --- | --- | --- |\n");
    md.push_str(&format!(
        "| ratio | {:.3} | {:.3} | 0.83 full slide (Pacejka-book set); 0.73 asymptote (MF 6.1 set) |\n\n",
        lat_rows[2].f30 / lat_rows[2].peak,
        lat_cand_rows[2].f30 / lat_cand_rows[2].peak,
    ));
    md.push_str("Longitudinal F(kappa = 0.5)/peak: NGP ");
    md.push_str(&format!("{:.3}", lon_rows[2].f05 / lon_rows[2].peak_drive));
    md.push_str(" | brush-v1 ");
    md.push_str(&format!(
        "{:.3}; 0.71 at kappa 0.67 (Pacejka-book set), 0.65 asymptote (MF 6.1 set).\n\n",
        lon_cand_rows[2].f05 / lon_cand_rows[2].peak_drive
    ));

    md.push_str("## locked wheel over peak mu_x at 10 m/s (Vs = 10 m/s at lock)\n\n");
    md.push_str(
        "| | NGP peak | NGP locked | NGP ratio | brush-v1 peak | brush-v1 locked | brush-v1 ratio | target |\n",
    );
    md.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for (i, (name, pn, ln, pc, lc)) in lock_rows.iter().enumerate() {
        let target = if i == 0 {
            "0.65 (Burckhardt) to 0.885 (Kebbati et al.)"
        } else {
            "0.64 (Burckhardt), 0.69 (Kebbati et al.)"
        };
        md.push_str(&format!(
            "| {name} | {pn:.3} | {ln:.3} | {:.3} | {pc:.3} | {lc:.3} | {:.3} | {target} |\n",
            ln / pn,
            lc / pc,
        ));
    }
    md.push_str(
        "\nKebbati et al., Sustainability 15:6945, 2023: ABS over locked at 30-40 km/h, mean deceleration (secondary \
read). Burckhardt (1993) dry/wet asphalt curves at s = 1 over their peak, via three reproductions; test speed not \
stated.\n\n",
    );

    md.push_str("## peak mu_y vs speed (4 kN)\n\n");
    md.push_str("| V (m/s) | NGP | brush-v1 |\n| --- | --- | --- |\n");
    for (vv, bn, bc) in &speed_rows {
        md.push_str(&format!("| {vv:.0} | {bn:.3} | {bc:.3} |\n"));
    }
    md.push_str("\n## pneumatic trail at 4 kN (m)\n\n");
    md.push_str("| | NGP | brush-v1 |\n| --- | --- | --- |\n");
    md.push_str(&format!("| at 1 deg | {trail_1n:.4} | {trail_1c:.4} |\n"));
    md.push_str(&format!(
        "| at own peak ({:.2}/{:.2} deg) | {trail_pn:.4} | {trail_pc:.4} |\n\n",
        lat_rows[2].alpha_peak, lat_cand_rows[2].alpha_peak,
    ));

    md.push_str("## combined slip at 4 kN: |Fy|, |Fx| (N) at fixed slip angle\n\n");
    md.push_str(
        "| alpha | kappa | NGP Fy | brush-v1 Fy | NGP Fx | brush-v1 Fx |\n| --- | --- | --- | --- | --- | --- |\n",
    );
    for (afix, kappa, fyn, fxn, fyc, fxc) in &comb_rows {
        md.push_str(&format!("| {afix:.1} deg | {kappa:.2} | {fyn:.0} | {fyc:.0} | {fxn:.0} | {fxc:.0} |\n"));
    }
    md.push_str("\n## relaxation length sigma_y at 4 kN (m)\n\n");
    md.push_str("| V (m/s) | NGP (C_alpha/k_y) | brush-v1 sigma_y | sigma_y/R | brush-v1 sigma_x | sigma_x/R |\n");
    md.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for (vv, sy, sx) in &sigma_rows {
        md.push_str(&format!("| {vv:.0} | {ngp_sigma:.3} | {sy:.3} | {:.2} | {sx:.3} | {:.2} |\n", sy / r, sx / r));
    }
    md.push_str(
        "\nPublished sigma/R0 at the nominal load: lateral 1.01/1.84, longitudinal 1.19/1.44 (MF 5.2 truck / MF 6.1 \
255/55R20 example sets); 0.3-0.9 m growing with speed on a passenger tyre bench (Vera et al., Sensors 22:6380, \
2022). NGP is quasi-static here.\n\n",
    );

    md.push_str("## bulk temperature: cold 293 K vs hot 340 K (4 kN, 15 m/s)\n\n");
    md.push_str(
        "| | NGP C_alpha | brush-v1 C_alpha | NGP mu_peak | brush-v1 mu_peak |\n| --- | --- | --- | --- | --- |\n",
    );
    for (name, cn, cc, mn, mc) in &temp_rows {
        md.push_str(&format!("| {name} | {cn:.0} | {cc:.0} | {mn:.3} | {mc:.3} |\n"));
    }
    md.push_str("\nTarget: C_alpha drops 20-25% cold->hot (Singh Table 3).\n\n");

    md.push_str("## new vs 60% worn (4 kN, 15 m/s, dry)\n\n");
    md.push_str(
        "| | NGP C_alpha | brush-v1 C_alpha | NGP mu_peak | brush-v1 mu_peak |\n| --- | --- | --- | --- | --- |\n",
    );
    for (name, cn, cc, mn, mc) in &wear_rows {
        md.push_str(&format!("| {name} | {cn:.0} | {cc:.0} | {mn:.3} | {mc:.3} |\n"));
    }
    md.push_str("\nTarget: +30% C_alpha / +10% dry peak mu at 60% worn (Singh Table 3).\n\n");

    md.push_str("## wet: peak mu_y vs speed (4 kN)\n\n");
    md.push_str("| W | V | tread | NGP | brush-v1 |\n| --- | --- | --- | --- | --- |\n");
    for (w, vv, d, bn, bc) in &wet_rows {
        md.push_str(&format!("| {w:.2} | {vv:.0} | {} | {bn:.3} | {bc:.3} |\n", if *d > 0.9 { "new" } else { "worn" }));
    }
    md.push_str(
        "\nNGP stage values (damp tag 38 / wet tag 39) times the dry tyre's wet slope. \
Candidate: same surface mu plus the hydroplaning-style loss; the dry-wear gain fades with W. \
Published checks: a new summer tyre brakes from 100 km/h on a watered track at the same mean mu as from 80 \
(0.98/0.97); at 1.6 mm it keeps 0.69 of new from 80 km/h and 0.52 from 100 (Continental Contidrom, AUTO BILD).\n\n",
    );

    md.push_str("## step steer 2 deg: distance to 63% (brush-v1 transient)\n\n");
    md.push_str("| V (m/s) | sigma_y (m) | x63 (m) |\n| --- | --- | --- |\n");
    for (vv, sy, x) in &step_rows {
        md.push_str(&format!("| {vv:.0} | {sy:.3} | {x:.3} |\n"));
    }
    md.push_str("\n## constants and sources\n\n");
    md.push_str(&format!(
        "M_CY = {M_CY}, N_MU_Y = {N_MU_Y} (Singh & Sivaramakrishnan, Goodyear, arXiv 2305.18422, load sweep); \
M_CX = {M_CX}, KX_OVER_KY = {KX_OVER_KY}, N_MU_X = {N_MU_X} (means of the MF 6.1 255/55R20 example set and the \
Pacejka-book passenger set in Chrono's TMeasy); Stribeck g(Vs) with V_S = {V_S} m/s, Coulomb/static {:.3} \
(Canudas de Wit et al., VSD 39 (2003), Table 1); film {:.1} mm at W = 1 (measured road films), ref 0.5 mm, \
kept 0.4 and the quadratic ours; vp = 6.36 sqrt(p/kPa) (Horne) times (0.75 + 0.25 d) (ADAC 2026); dry-wear \
gain 1 + 0.167(1-d)(1-W) (Singh Table 3; the wet fade ours); brush ch. 3 and transient ch. 7 Pacejka 3rd ed.; \
sigma = C/k, no speed term (Pacejka ch. 5). V_MIN = {V_MIN} m/s floor; below 2 m/s out of scope.\n",
        MU_C_OVER_S,
        FILM_MAX * 1e3,
    ));
    write(&out.join("summary.md"), &md);
    println!("{md}");
    println!("csv + summary.md in {}", out.display());
}

fn class_of(base: &Params) -> usize {
    2usize.min(base.wet_slope.len().saturating_sub(1))
}
