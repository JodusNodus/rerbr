//! brush-v1 and NGP at the conditions of the racing-tyre and combined-slip data in `notes/tyre-model.md`
//! ("Racing and rally targets").
//!
//! Research tooling (ours, experimental): reads a `--tyre-probe` dump, calibrates brush-v1 at run time exactly as
//! `compare` does (NGP's lateral peak and slope and longitudinal peak at 4 kN, 15 m/s) and prints, for both models:
//! the force after the peak at the slips the racing data reach (12 and 17 deg, kappa 0.2/0.5/1.0), the slip angle
//! where the side force first reaches 95 % of its peak, the peak load exponent over 4-8 kN, and the side force
//! kept at 4 deg with 25 % slip, beside the share the sliding velocity's direction gives at full slide. Prints only.

use rbr_physics::tyre::patch_forces::{patch_forces, PatchForces};
use rbr_tyre_lab::brush::{self, Calibration, EvalInput, EvalOutput};
use rbr_tyre_lab::calibrate::{calibrate, CalPoint, CalTargets};
use rbr_tyre_lab::ngp::{blend_at, patch_input};
use rbr_tyre_lab::probe::{parse_probe, patch_at, Params};

struct Lab {
    base: Params,
    cloud: Vec<(f32, f32)>,
    cal: Option<Calibration>,
}

impl Lab {
    fn r(&self) -> f64 {
        f64::from(self.base.effective_radius)
    }

    fn ngp(&self, fz: f64, vlat: f64, vlong: f64, spin: f64) -> PatchForces {
        let b = blend_at(&self.base, fz as f32);
        let patch = patch_at(&self.cloud, fz as f32, self.base.patch_length);
        patch_forces(&patch_input(&self.base, &b, fz as f32, patch, vlat as f32, vlong as f32, spin as f32))
    }

    fn brush(&self, fz: f64, vlat: f64, vlong: f64, spin: f64) -> EvalOutput {
        let p = &self.base;
        brush::eval(
            &EvalInput {
                fz,
                half_length: f64::from(patch_at(&self.cloud, fz as f32, p.patch_length)) / 2.0,
                vx: vlong,
                vy: vlat,
                spin,
                radius: self.r(),
                bulk_temp: f64::from(p.tread_temp),
                tread_remaining: 1.0 - f64::from(p.wear_op),
                pressure: f64::from(p.air_pressure),
                pressure_factor: f64::from(p.stiffness_factor),
                wetness: 0.0,
                temp_grip: f64::from(p.temperature_grip),
                surf_static: f64::from(p.static_friction),
                surf_kinetic: f64::from(p.kinetic_friction),
            },
            self.cal.as_ref().expect("calibrated"),
        )
    }

    /// |Fy|, |Fx| of NGP (`ngp`) or brush-v1 at slip angle `alpha` (deg), slip ratio `kappa`, speed `v`.
    fn forces(&self, ngp: bool, fz: f64, alpha: f64, kappa: f64, v: f64) -> (f64, f64) {
        let rad = alpha.to_radians();
        let (vlat, vlong) = (v * rad.sin(), -v * rad.cos());
        let spin = (1.0 + kappa) * (-vlong) / self.r();
        if ngp {
            let n = self.ngp(fz, vlat, vlong, spin);
            (f64::from(n.lateral_force).abs(), f64::from(n.longitudinal_force).abs())
        } else {
            let c = self.brush(fz, vlat, vlong, spin);
            (c.fy.abs(), c.fx.abs())
        }
    }

    /// Lateral sweep 0-30 deg in 0.25 deg: (peak, slip angle at peak, first slip angle at 95 % of peak).
    fn lat_peak(&self, ngp: bool, fz: f64, v: f64) -> (f64, f64, f64) {
        let sweep: Vec<(f64, f64)> =
            (0..=120).map(|i| f64::from(i) * 0.25).map(|a| (a, self.forces(ngp, fz, a, 0.0, v).0)).collect();
        let &(a_peak, peak) = sweep.iter().max_by(|x, y| x.1.total_cmp(&y.1)).expect("sweep");
        let a95 = sweep.iter().find(|x| x.1 >= 0.95 * peak).map_or(a_peak, |x| x.0);
        (peak, a_peak, a95)
    }

    /// Longitudinal drive sweep 0-1 in 0.02 (as `compare`): (peak, kappa at peak).
    fn lon_peak(&self, ngp: bool, fz: f64, v: f64) -> (f64, f64) {
        (0..=50)
            .map(|i| f64::from(i) * 0.02)
            .map(|k| (self.forces(ngp, fz, 0.0, k, v).1, k))
            .max_by(|x, y| x.0.total_cmp(&y.0))
            .expect("sweep")
    }
}

fn main() {
    let mut probe = String::from("/tmp/tyre-lab/x.probe");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--probe" => probe = args.next().expect("--probe PATH"),
            _ => panic!("unknown arg {a}"),
        }
    }
    let text = std::fs::read_to_string(&probe).unwrap_or_else(|e| panic!("{probe}: {e}"));
    let (dumps, cloud) = parse_probe(&text);
    let base = dumps
        .iter()
        .find(|d| {
            d.wheel == 0
                && d.load_axis.len() == 8
                && d.crn_my.len() == 8
                && d.normal_force > 0.0
                && d.static_friction > 0.0
                && d.patch_length > 0.0
                && d.speed > 5.0
        })
        .expect("no usable wheel-0 dump")
        .clone();
    let mut lab = Lab { base, cloud, cal: None };

    // Calibration as in `compare`: NGP's lateral peak and slope (alpha <= 2 deg) and drive peak at 4 kN, 15 m/s.
    let (fz0, v0) = (4000.0, 15.0);
    let (peak_lat, _, _) = lab.lat_peak(true, fz0, v0);
    let (xs, ys): (Vec<f64>, Vec<f64>) =
        (1..=8).map(|i| f64::from(i) * 0.25).map(|a| (a.to_radians(), lab.forces(true, fz0, a, 0.0, v0).0)).unzip();
    let fit_lat = xs.iter().zip(&ys).map(|(x, y)| x * y).sum::<f64>() / xs.iter().map(|x| x * x).sum::<f64>();
    let (peak_lon, _) = lab.lon_peak(true, fz0, v0);
    let p = &lab.base;
    let point = CalPoint {
        t_cal: f64::from(p.tread_temp),
        d_cal: 1.0 - f64::from(p.wear_op),
        p_cal: f64::from(p.air_pressure),
        pf_cal: f64::from(p.stiffness_factor),
        w_cal: 0.0,
        surf_s_cal: f64::from(p.static_friction),
        surf_k_cal: f64::from(p.kinetic_friction),
        temp_grip_cal: f64::from(p.temperature_grip),
    };
    lab.cal = Some(calibrate(&CalTargets { fz0, v: v0, peak_lat, fit_lat, peak_lon }, &point));

    for (name, ngp) in [("NGP", true), ("brush-v1", false)] {
        println!("## {name} (4 kN)");
        for v in [11.2, 15.0, 25.0] {
            let (peak, a_peak, a95) = lab.lat_peak(ngp, fz0, v);
            let f12 = lab.forces(ngp, fz0, 12.0, 0.0, v).0 / peak;
            let f17 = lab.forces(ngp, fz0, 17.0, 0.0, v).0 / peak;
            let (xpk, k_peak) = lab.lon_peak(ngp, fz0, v);
            let lon = |k: f64| lab.forces(ngp, fz0, 0.0, k, v).1 / xpk;
            println!(
                "V {v:4.1}: lateral mu {:.3} at {a_peak:.2} deg, 95 % at {a95:.2} deg, F12/peak {f12:.3}, \
F17/peak {f17:.3}; drive mu {:.3} at kappa {k_peak:.2}, F(0.2/0.5/1.0)/peak {:.3}/{:.3}/{:.3}",
                peak / fz0,
                xpk / fz0,
                lon(0.2),
                lon(0.5),
                lon(1.0),
            );
        }
        // Peak load exponent over the Milliken figure's loads (4.0-8.0 kN).
        let loads = [4000.0, 6000.0, 8000.0];
        let peaks: Vec<f64> = loads.iter().map(|&fz| lab.lat_peak(ngp, fz, v0).0).collect();
        let n = (peaks[2] / peaks[0]).ln() / (loads[2] / loads[0]).ln();
        let alphas: Vec<String> = loads.iter().map(|&fz| format!("{:.2}", lab.lat_peak(ngp, fz, v0).1)).collect();
        println!(
            "peak mu_y at 4/6/8 kN {:.3}/{:.3}/{:.3} (Fy ~ Fz^{n:.3}), peak slip angle {} deg",
            peaks[0] / 4000.0,
            peaks[1] / 6000.0,
            peaks[2] / 8000.0,
            alphas.join("/"),
        );
        // Combined slip: side force kept at fixed slip angle with 25 % slip, and its share of the total force.
        for alpha in [2.0, 4.0, 8.0] {
            for kappa in [-0.25, 0.25] {
                let (fy0, _) = lab.forces(ngp, fz0, alpha, 0.0, v0);
                let (fy, fx) = lab.forces(ngp, fz0, alpha, kappa, v0);
                // Direction of the sliding velocity: Vsx = kappa V cos(alpha), Vsy = V sin(alpha).
                let rad = alpha.to_radians();
                let share_slide = rad.sin() / ((kappa * rad.cos()).powi(2) + rad.sin().powi(2)).sqrt();
                println!(
                    "alpha {alpha:.0} kappa {kappa:+.2}: Fy {fy:.0} N, Fy/Fy(kappa 0) {:.3}, Fy/|F| {:.3} \
(sliding direction {share_slide:.3})",
                    fy / fy0,
                    fy / (fx * fx + fy * fy).sqrt(),
                );
            }
        }
        println!();
    }
}
