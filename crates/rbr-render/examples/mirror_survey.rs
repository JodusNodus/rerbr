//! Lists the mirrors `rbr_render::mirrors::find` finds in each retail car's interior set and, with `RBR_RSF`, each
//! RSF sample car's. Usage: `mirror_survey [CAR...]` (RSF cars by id or pack; a model's `.sgc` path under a `Cars`
//! folder for a car of the launcher's added packs). Each mirror with where the eye's line to its centre goes off the
//! glass (degrees outward from straight back, and up) and the share of its glass that shows the car's own body.

use rbr_formats::data::Data;
use rbr_render::car;
use rbr_render::mirrors;
use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let data = Data::from_env().ok_or("RBR_DATA is not set")?;
    let rsf = std::env::var_os("RBR_RSF").map(PathBuf::from);
    let mut cars: Vec<(String, Data, String)> = Vec::new();
    let retail = ["XSARA", "ACCENT", "ZR", "LANCER_EVO_VII", "PEUGEOT_206", "IMPREZA03", "COROLLA", "IMPREZA00"];
    let wanted: Vec<String> = std::env::args().skip(1).collect();
    for name in retail {
        if wanted.is_empty() || wanted.iter().any(|w| w.eq_ignore_ascii_case(name)) {
            cars.push((name.to_owned(), data.clone(), name.to_owned()));
        }
    }
    for path in wanted.iter().filter(|w| w.to_ascii_lowercase().ends_with(".sgc")) {
        let (root, entry) = rbr_formats::car_pack::model_of(std::path::Path::new(path))?;
        let folder = entry.file_name.split(['\\', '/']).rev().nth(1).unwrap_or_default().to_owned();
        cars.push((folder.clone(), data.clone().with_car(root, entry), folder));
    }
    if let Some(root) = &rsf {
        let install = rbr_formats::rsf::Install::open(root)?;
        for car in &install.cars {
            let key = car.id.to_string();
            if !wanted.is_empty() && !wanted.contains(&key) {
                continue;
            }
            match rbr_formats::rsf::select(data.clone(), &key, Some(root)) {
                Ok(selected) => {
                    let folder = selected.folder().to_owned();
                    cars.push((format!("rsf {} {}", car.id, car.name), selected.data, folder))
                }
                Err(e) if e.contains("no pack") => {}
                Err(e) => println!("rsf {} {}: {e}", car.id, car.name),
            }
        }
    }
    for (label, data, folder) in cars {
        let interior = match car::load_interior(&data, &folder, car::placement_at(None)) {
            Ok(interior) => interior,
            Err(e) => {
                println!("{label}: {e:?}");
                continue;
            }
        };
        let exterior = match car::load(&data, &folder, car::placement_at(None)) {
            Ok(exterior) => exterior,
            Err(e) => {
                println!("{label}: {e:?}");
                continue;
            }
        };
        let (lo, hi) = exterior.bounds().ok_or("no geometry")?;
        let mounts = rbr_render::views::Mounts::load(&data, &folder, lo, hi, &exterior.meshes, &interior.meshes)?;
        let found = mirrors::find(&interior.meshes, &interior.damage, mounts.cockpit.eye);
        let eye = mounts.cockpit.eye;
        let body: Vec<[glam::Vec3; 3]> = exterior
            .meshes
            .iter()
            .flat_map(|mesh| {
                mesh.indices
                    .chunks_exact(3)
                    .map(|t| [0, 1, 2].map(|i| glam::Vec3::from(mesh.vertices[t[i] as usize].position)))
            })
            .collect();
        let list: Vec<String> = found
            .mirrors
            .iter()
            .map(|m| {
                let off = |p: glam::Vec3| {
                    let seen = (p - eye).normalize_or_zero();
                    seen - 2.0 * seen.dot(m.aimed) * m.aimed
                };
                let out = off(m.centre);
                let outward = out.x * m.centre.x.signum();
                let own = (0..35)
                    .filter(|k| {
                        let at = |i: u32, n: u32| 0.9 * (2.0 * (i as f32 + 0.5) / n as f32 - 1.0);
                        let p = m.centre + m.axes[0] * m.half.x * at(k % 7, 7) + m.axes[1] * m.half.y * at(k / 7, 5);
                        body.iter().any(|&t| rbr_render::views::ray_hit(p + m.aimed * 0.05, off(p), t).is_some())
                    })
                    .count();
                format!(
                    "{:?} meshes {:?} ({} tris) at ({:.2} {:.2} {:.2}) n ({:.2} {:.2} {:.2}) {:.0}x{:.0} cm, looks {:.0} deg out {:.0} up, own body {:.0} %",
                    m.kind,
                    m.faces.iter().map(|f| f.0).collect::<std::collections::BTreeSet<_>>(),
                    m.faces.len(),
                    m.centre.x,
                    m.centre.y,
                    m.centre.z,
                    m.normal.x,
                    m.normal.y,
                    m.normal.z,
                    m.half.x * 200.0,
                    m.half.y * 200.0,
                    outward.atan2(out.z).to_degrees(),
                    out.y.atan2(out.z.hypot(out.x)).to_degrees(),
                    own as f32 / 0.35
                )
            })
            .collect();
        println!("{label} [{folder}]: {} mirrors, eye ({:.2} {:.2} {:.2})", found.mirrors.len(), eye.x, eye.y, eye.z);
        for line in list {
            println!("    {line}");
        }
        for u in &found.unfitted {
            println!(
                "    unfitted: mesh {} ({} tris) at ({:.2} {:.2} {:.2}) {:.0}x{:.0} cm, {:.1} mm off its plane",
                u.mesh,
                u.triangles,
                u.centre.x,
                u.centre.y,
                u.centre.z,
                u.size.x * 100.0,
                u.size.y * 100.0,
                u.off_plane * 1000.0
            );
        }
    }
    Ok(())
}
