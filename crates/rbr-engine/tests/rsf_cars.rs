//! Every installed RSF car calibrates, reproduces its table and idles under its idle air's ceiling; prints each car's
//! s_f, η_ig·η_v, class and flags.
//! Ignored unless `RBR_RSF` is set: `RBR_RSF=... cargo test -p rbr-engine --test rsf_cars -- --nocapture`.

mod rsf;

use rbr_engine::realistic::Realistic;
use std::path::PathBuf;

#[test]
fn every_installed_car_calibrates_and_reproduces_its_table() {
    let Some(root) = std::env::var_os("RBR_RSF").map(PathBuf::from) else {
        eprintln!("RBR_RSF not set: skipped");
        return;
    };
    let mut misses = Vec::new();
    for folder in rsf::folders(&root) {
        let p = match rsf::params(&root, &folder) {
            Ok(p) => p,
            Err(e) => {
                misses.push(e);
                continue;
            }
        };
        let m = Realistic::new(&p);
        let c = m.calibration();
        let on = |k: &&(f64, f64, f64)| k.0 >= 150.0 && !c.held.iter().any(|h| (h.0 - k.0).abs() < 1.0);
        let worst = c.wot.iter().filter(on).map(|k| (k.2 - k.1).abs() / k.1.abs().max(1e-9)).fold(0.0, f64::max);
        let over = c.wot.iter().filter(|k| k.0 >= 150.0).map(|k| (k.2 - k.1) / k.1.abs().max(1e-9)).fold(0.0, f64::max);
        let short = c.held.iter().map(|h| (h.1 - h.2) / h.1.abs().max(1e-9)).fold(0.0, f64::max);
        let span = c
            .held
            .first()
            .zip(c.held.last())
            .map(|(a, b)| format!(" held {:.0}..{:.0} rpm short {:.1} %", a.0, b.0, 100.0 * short));
        let idle = f64::from(p.idle_speed) * rbr_engine::engine::RPM;
        let map = rbr_engine::calibrate::part(m.spec(), idle, 0.0).0.p_im / 1e3;
        let class = c.class.unwrap();
        println!(
            "{folder}: s_f {:.2} (raw {:.2}) η_ig·η_v {:.2} worst {:.2} %{} idle MAP {map:.1} kPa {:?} {:?} {:?} ALS {} cyl {} flags {:?}",
            c.friction_scale,
            c.friction_scale_raw,
            c.eta_ig_v,
            100.0 * worst,
            span.unwrap_or_default(),
            class.actuation,
            class.intake,
            class.idle,
            class.anti_lag,
            class.cylinders,
            c.flags
        );
        if c.flags.iter().any(|f| f.contains("idle air at its ceiling")) {
            misses.push(format!("{folder}: idle air at its ceiling (no anti-stall reserve)"));
        }
        if over > 0.001 {
            misses.push(format!("{folder}: WOT over the table by {:.2} %", 100.0 * over));
        }
        if worst > 0.005 {
            misses.push(format!("{folder}: WOT off by {:.2} %", 100.0 * worst));
        }
    }
    assert!(misses.is_empty(), "{misses:#?}");
}
