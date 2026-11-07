//! Every installed RSF car calibrates and reproduces its table; prints each car's s_f, η_ig·η_v, class and flags.
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
        let worst = c
            .wot
            .iter()
            .filter(|k| k.0 >= 150.0)
            .map(|k| (k.2 - k.1).abs() / k.1.abs().max(1e-9))
            .fold(0.0, f64::max);
        let class = c.class.unwrap();
        println!(
            "{folder}: s_f {:.2} (raw {:.2}) η_ig·η_v {:.2} worst {:.2} % {:?} {:?} {:?} ALS {} cyl {} flags {:?}",
            c.friction_scale,
            c.friction_scale_raw,
            c.eta_ig_v,
            100.0 * worst,
            class.actuation,
            class.intake,
            class.idle,
            class.anti_lag,
            class.cylinders,
            c.flags
        );
        if worst > 0.005 {
            misses.push(format!("{folder}: WOT off by {:.2} %", 100.0 * worst));
        }
    }
    assert!(misses.is_empty(), "{misses:#?}");
}
