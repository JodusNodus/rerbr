//! A car's headline specs from its NGP physics scripts, without running the physics: the mass, the drag, the engine's
//! power and the driven wheels. `common.lsp`'s `CAR_ROOT` holds the body (`InverseMass`, `FrontArea`,
//! `DragCoefficient`) and its `Engine`/`ControlUnit` the torque table (`Tq_NN_RPM_NGP` and `Tq_NN_NGP`, a 0-1 shape)
//! and `RPMLimit`; an `r_<surface>.lsp` the peak torque (`MaxTorque_NGP`, N·m, the table's scale as
//! `rbr_physics::ngp::engine` applies it), the drivetrain (`Features_NGP`'s low nibble: 4 front, 5 rear, 6 and 7
//! all-wheel drive) and `DriveTrainLoss_NGP` (%). The downforce is `CAR_ROOT`'s `FrontDownforce_NGP` and `RearDownforce_NGP`, newtons at 150 km/h
//! (`rbr_physics::ngp::downforce`). Keys in `data/ngp-lsp-keys.md`.

use crate::lsp::{self, Node};

/// The driven wheels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drive {
    Front,
    Rear,
    All,
}

/// A car's specs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarSpecs {
    /// kg.
    pub mass: f32,
    /// The drag area `Cd·A` (m²).
    pub drag_area: f32,
    /// The engine's peak power at the crank (W), the torque table's best `torque × rpm` up to the rev limit.
    pub power: f32,
    /// The rpm of the peak power.
    pub power_rpm: f32,
    /// The share of the power reaching the wheels (1 − `DriveTrainLoss_NGP` %).
    pub efficiency: f32,
    pub drive: Drive,
    /// The front and rear downforce together at 150 km/h (N); it grows with the square of the speed.
    pub downforce: f32,
}

impl CarSpecs {
    /// The specs from `common.lsp` and an `r_<surface>.lsp`; none when a key the specs need is missing (the retail
    /// 1.02 cars have no NGP engine keys).
    pub fn parse(common: &str, surface: &str) -> Option<CarSpecs> {
        let common = lsp::parse(common).ok()?;
        let surface = lsp::parse(surface).ok()?;
        let root = find_list(&common, "CAR_ROOT")?;
        let own = |key: &str| own_value(root, key);
        let mass = 1.0 / own("InverseMass").filter(|m| *m > 0.0)?;
        let drag_area = own("FrontArea")? * own("DragCoefficient")?;
        let limit = deep_value(root, "RPMLimit")?;
        let scale = deep_value(&surface, "MaxTorque_NGP")?;
        let mut power = (0.0f32, 0.0f32);
        for k in 0..16 {
            let (Some(rpm), Some(shape)) =
                (deep_value(root, &format!("Tq_{k:02}_RPM_NGP")), deep_value(root, &format!("Tq_{k:02}_NGP")))
            else {
                continue;
            };
            let watts = shape * scale * rpm * std::f32::consts::TAU / 60.0;
            if rpm <= limit && watts > power.0 {
                power = (watts, rpm);
            }
        }
        let features = deep_value(&surface, "Features_NGP")? as u32;
        let drive = match features & 0xF {
            4 => Drive::Front,
            5 => Drive::Rear,
            _ => Drive::All,
        };
        let loss = deep_value(&surface, "DriveTrainLoss_NGP").unwrap_or(0.0);
        let downforce = own("FrontDownforce_NGP").unwrap_or(0.0) + own("RearDownforce_NGP").unwrap_or(0.0);
        (power.0 > 0.0 && mass > 0.0).then_some(CarSpecs {
            mass,
            drag_area,
            power: power.0,
            power_rpm: power.1,
            efficiency: 1.0 - loss / 100.0,
            drive,
            downforce,
        })
    }
}

/// The list named `name` (its first item quoted), searched depth first.
fn find_list<'a>(nodes: &'a [Node], name: &str) -> Option<&'a [Node]> {
    for node in nodes {
        if let Node::List(items) = node {
            if matches!(items.first(), Some(Node::Quoted(q)) if q == name) {
                return Some(items);
            }
            if let Some(found) = find_list(items, name) {
                return Some(found);
            }
        }
    }
    None
}

/// The number after the atom `key` among `items` themselves, not their sublists.
fn own_value(items: &[Node], key: &str) -> Option<f32> {
    items.windows(2).find_map(|w| match w {
        [Node::Atom(k), Node::Atom(v)] if k == key => v.parse().ok(),
        _ => None,
    })
}

/// The number after the atom `key` in `items` or any sublist, the first found.
fn deep_value(items: &[Node], key: &str) -> Option<f32> {
    own_value(items, key).or_else(|| {
        items.iter().find_map(|n| match n {
            Node::List(sub) => deep_value(sub, key),
            _ => None,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMON: &str = r#"
("Car"
 ("CAR_ROOT"
  InverseMass 0.0008
  FrontArea 2.0
  DragCoefficient 0.4
  FrontDownforce_NGP 900.0
  RearDownforce_NGP 1300.0
  ("Engine"
   Tq_00_RPM_NGP 0
   Tq_01_RPM_NGP 4000
   Tq_02_RPM_NGP 6000
   Tq_03_RPM_NGP 9000
   Tq_00_NGP 0.3
   Tq_01_NGP 1.0
   Tq_02_NGP 0.9
   Tq_03_NGP 0.9)
  ("ControlUnit" RPMLimit 7000.0)))
("HingedBody" ("CAR_DOOR_L" InverseMass 0.05))
"#;

    #[test]
    fn the_specs_read_the_body_the_engine_and_the_drive() {
        let surface = "(\"(null)\" DriveTrainLoss_NGP 12 Features_NGP 16950) (\"(null)\" MaxTorque_NGP 400)";
        let specs = CarSpecs::parse(COMMON, surface).unwrap();
        assert_eq!(specs.mass, 1250.0);
        assert!((specs.drag_area - 0.8).abs() < 1e-6);
        // 0.9 × 400 N·m at 6000 rpm: the 9000 rpm point is past the limiter.
        assert_eq!(specs.power_rpm, 6000.0);
        assert!((specs.power - 0.9 * 400.0 * 6000.0 * std::f32::consts::TAU / 60.0).abs() < 1.0);
        assert!((specs.efficiency - 0.88).abs() < 1e-6);
        assert_eq!(specs.drive, Drive::All);
        assert_eq!(specs.downforce, 2200.0);
        let rear = surface.replace("16950", "16949");
        assert_eq!(CarSpecs::parse(COMMON, &rear).unwrap().drive, Drive::Rear);
        assert_eq!(CarSpecs::parse(COMMON, "(\"(null)\" Features_NGP 4)"), None, "no peak torque");
    }

    /// Every RSF pack's specs read, within what a rally car can be (`RBR_RSF`).
    #[test]
    fn every_rsf_pack_reads() {
        let Some(root) = std::env::var_os("RBR_RSF") else {
            eprintln!("skipping: RBR_RSF not set");
            return;
        };
        let cars = std::path::Path::new(&root).join("rsfdata/cars");
        let mut read = 0;
        for entry in std::fs::read_dir(&cars).unwrap().flatten() {
            let dir = entry.path();
            let (Ok(common), Ok(surface)) =
                (std::fs::read(dir.join("common.lsp")), std::fs::read(dir.join("r_tarmac.lsp")))
            else {
                continue;
            };
            let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
            let specs = CarSpecs::parse(&text(&common), &text(&surface))
                .unwrap_or_else(|| panic!("{}: the specs do not read", dir.display()));
            let kw = specs.power / 1000.0;
            assert!((600.0..2500.0).contains(&specs.mass), "{}: {specs:?}", dir.display());
            assert!((40.0..600.0).contains(&kw), "{}: {specs:?}", dir.display());
            assert!((0.2..2.0).contains(&specs.drag_area), "{}: {specs:?}", dir.display());
            read += 1;
        }
        assert!(read > 0);
    }
}
