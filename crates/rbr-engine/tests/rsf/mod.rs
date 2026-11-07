//! An installed RSF car's engine params, read as NGP's car init holds them: `common.lsp` and `r_gravel.lsp` of
//! `$RBR_RSF/rsfdata/cars/<folder>`, its group from `cars.json` and its year from `cars_data.json`.

use rbr_engine::{Car, Params};
use rbr_formats::lsp::{self, Node};
use rbr_formats::rsf::Install;
use std::path::Path;

fn atoms(nodes: &[Node], out: &mut Vec<String>) {
    for n in nodes {
        match n {
            Node::Atom(a) => out.push(a.clone()),
            Node::Quoted(q) => out.push(q.clone()),
            Node::List(l) => atoms(l, out),
        }
    }
}

/// The installed RSF car folders.
#[allow(dead_code)]
pub fn folders(root: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(root.join("rsfdata").join("cars"))
        .map(|d| d.filter_map(|e| e.ok()).filter(|e| e.path().join("common.lsp").exists()))
        .into_iter()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    v.sort();
    v
}

/// The year `cars_data.json` gives the car `id`.
fn year(root: &Path, id: u32) -> Option<u32> {
    let text = std::fs::read_to_string(root.join("rsfdata").join("cache").join("cars_data.json")).ok()?;
    let row = text.split('}').find(|r| r.contains(&format!("\"car_id\":\"{id}\"")))?;
    let at = row.find("\"year\":\"")? + 8;
    row[at..].split('"').next()?.parse().ok()
}

pub fn params(root: &Path, folder: &str) -> Result<Params, String> {
    let dir = root.join("rsfdata").join("cars").join(folder);
    let mut words = Vec::new();
    for f in ["common.lsp", "r_gravel.lsp"] {
        let text = std::fs::read(dir.join(f)).map_err(|e| format!("{folder}/{f}: {e}"))?;
        let nodes = lsp::parse(&String::from_utf8_lossy(&text)).map_err(|e| format!("{folder}/{f}: {e}"))?;
        atoms(&nodes, &mut words);
    }
    let get = |key: &str| -> Option<f32> {
        let i = words.iter().position(|w| w == key)?;
        words.get(i + 1)?.parse().ok()
    };
    let need = |key: &str| get(key).ok_or(format!("{folder}: no {key}"));
    let rad = std::f32::consts::PI / 30.0;
    let mut knots: Vec<(f32, f32)> = Vec::new();
    for i in 0..16 {
        let (Some(n), Some(t)) = (get(&format!("Tq_{i:02}_RPM_NGP")), get(&format!("Tq_{i:02}_NGP"))) else { continue };
        if (i == 0 || t > 0.0) && knots.last().is_none_or(|k| n * rad > k.0) {
            knots.push((n * rad, t));
        }
    }
    let install = Install::open(root)?;
    let car = install.cars.iter().find(|c| c.pack.trim_end_matches('/').ends_with(folder));
    Ok(Params {
        torque_knots: knots,
        max_torque: need("MaxTorque_NGP")?,
        rev_limit: need("RPMLimit")? * rad,
        idle_speed: need("IdleRPM_NGP")? * rad,
        brake_coefficient: need("BrakeCoefficient_NGP")? / 100.0,
        turbo: need("EngineType_NGP")? as u32 & 1 == 1,
        cylinder_volume: need("CylinderVolume_NGP")?,
        inlet_area: need("InletPipeArea_NGP")?,
        torque_cut: need("TorqueCut_NGP")?,
        inertia: need("EngineInertia")? + need("GearBoxInertiaIn")?,
        car: Car {
            folder: folder.to_owned(),
            group: car.and_then(|c| c.group.clone()),
            year: car.and_then(|c| year(root, c.id)),
        },
    })
}
