//! Lists each car's lamps as a night drive loads them (`car::load_lit`): per kind, the lamp points that light when
//! switched on (headlights on main beam, brake pressed, reverse engaged), and the parts that glow, with their size and
//! colour, then flags what reads as a broken lamp. Usage: `lamp_survey [-v] [CAR...]` (retail folder names, RSF ids).
//! Flags: `no-light` (no point lights the stage), `no-glow` (no part glows), `dropped` (light of the kind lost when
//! the lamps are fitted to the renderer's eight, `lights::fit`), `steady-dark` (an always-drawn lit part with no
//! fitted lamp of its kind within reach: 1.5 m, 0.3 m for a lens, plus the lamp's spread), `white` (a tail or brake
//! part whose texels at its triangles' centres are not red; a pale red that the glow drives to white passes), `dim`
//! (glowing texels under a quarter bright), `same-place` (two lamps of a kind at one point), `bare` (a lit lamp with
//! no glowing part within 0.25 m: the game shows it by its flare alone, which we do not draw), `wrong-way` (a lamp
//! whose kind faces the other way: a forward lamp on a tail record). Night drives at Gabiria (2026-11-19) confirmed
//! `no-glow`, `bare` and `wrong-way`; `white` missed the pale tails the glow saturates (Fiesta Rally3, Celica ST185).

use glam::Vec3;
use rbr_formats::data::Data;
use rbr_render::car::{self, Car};
use rbr_render::lights::{self, Headlights, Inputs, Kind};
use rbr_render::stage::Image;
use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;

const KINDS: [Kind; 4] = [Kind::Front, Kind::Tail, Kind::Brake, Kind::Reverse];
const MAX_LAMPS: usize = 8;
const LAMP_NEAR: f32 = 1.5;
const LENS_NEAR: f32 = 0.3;

fn main() -> Result<(), Box<dyn Error>> {
    let data = Data::from_env().ok_or("RBR_DATA is not set")?;
    let rsf = std::env::var_os("RBR_RSF").map(PathBuf::from);
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = args.iter().any(|a| a == "-v");
    args.retain(|a| a != "-v");
    let mut cars: Vec<(String, Data, String)> = Vec::new();
    let retail = ["XSARA", "ACCENT", "ZR", "LANCER_EVO_VII", "PEUGEOT_206", "IMPREZA03", "COROLLA", "IMPREZA00"];
    for name in retail.into_iter().filter(|n| args.is_empty() || args.iter().any(|a| a.eq_ignore_ascii_case(n))) {
        cars.push((name.to_owned(), data.clone(), name.to_owned()));
    }
    if let Some(root) = &rsf {
        let install = rbr_formats::rsf::Install::open(root)?;
        for car in &install.cars {
            let key = car.id.to_string();
            if !args.is_empty() && !args.contains(&key) {
                continue;
            }
            match rbr_formats::rsf::select(data.clone(), &key, Some(root)) {
                Ok(selected) if selected.model.fallback.is_none() => {
                    let folder = selected.folder().to_owned();
                    cars.push((format!("rsf {} {}", car.id, car.name), selected.data, folder));
                }
                Ok(_) => {}
                Err(e) if e.contains("no pack") => {}
                Err(e) => println!("rsf {} {}: {e}", car.id, car.name),
            }
        }
    }
    let mut flagged: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (label, data, folder) in &cars {
        let car = match car::load_lit(data, folder, car::placement_at(None), true) {
            Ok(car) => car,
            Err(e) => {
                println!("{label}: {e:?}");
                continue;
            }
        };
        let (lines, flags) = survey(&car);
        println!("{label} [{folder}]: {}", if flags.is_empty() { "ok".to_owned() } else { flags.join(", ") });
        if verbose || !flags.is_empty() {
            lines.iter().for_each(|line| println!("    {line}"));
        }
        for flag in flags {
            flagged.entry(flag.split(' ').next().unwrap_or_default().to_owned()).or_default().push(label.clone());
        }
    }
    println!("\n{} cars", cars.len());
    for (flag, labels) in flagged {
        println!("{flag}: {} cars", labels.len());
    }
    Ok(())
}

/// A glowing part: its kind, vertices, area along the car, whether always drawn, its centre and mean glow colour
/// with the texels' brightness.
struct Glow {
    kind: Kind,
    name: String,
    vertices: usize,
    area: f32,
    steady: bool,
    lens: bool,
    centre: Vec3,
    positions: Vec<Vec3>,
    colour: Option<[f32; 4]>,
}

/// How near a glowing part's vertex must be to a lamp's point for the lamp to show (`bare` otherwise), m.
const GLOW_NEAR: f32 = 0.25;

fn survey(car: &Car) -> (Vec<String>, Vec<String>) {
    let on = Inputs { brake: 1.0, reverse: true, game: None, headlights: Headlights::Main };
    let lit: Vec<_> = car.lamp_points.iter().filter(|p| lights::on(&p.light, on)).collect();
    let lamps: Vec<_> =
        lit.iter().map(|p| lights::lamp(&p.light, p.position, p.normal, Vec3::NEG_Z, Vec3::Y, 1.0, true)).collect();
    let fitted = lights::fit(&lamps, MAX_LAMPS);
    let glows: Vec<Glow> = car
        .meshes
        .iter()
        .zip(&car.damage)
        .filter_map(|(mesh, damage)| {
            let switch = damage.lamp.filter(|s| s.lit && !s.glass)?;
            let draws: Vec<_> = mesh.draws.iter().filter(|d| d.state.shade.is_some_and(|s| s.glow.is_some())).collect();
            if draws.is_empty() {
                return None;
            }
            let positions: Vec<Vec3> = mesh.vertices.iter().map(|v| Vec3::from(v.position)).collect();
            let centre = positions.iter().sum::<Vec3>() / positions.len().max(1) as f32;
            let glow = draws[0].state.shade.and_then(|s| s.glow)?;
            let colour = if glow[..3] != [1.0; 3] {
                Some([glow[0], glow[1], glow[2], 1.0])
            } else {
                texel_colour(car, mesh, &draws)
            };
            Some(Glow {
                kind: switch.kind,
                name: mesh_name(car, mesh),
                vertices: mesh.vertices.len(),
                area: lights::projected_area(&positions, &mesh.indices, Vec3::Z),
                steady: switch.steady,
                lens: draws[0].state.shade.is_some_and(|s| s.glow_kind & lights::LENS != 0),
                centre,
                positions,
                colour,
            })
        })
        .collect();
    let mut lines = vec![format!(
        "order: {}",
        car.lamp_points.iter().map(|p| format!("{:?}:{}", p.light.kind, p.light.part)).collect::<Vec<_>>().join(" ")
    )];
    let mut flags = Vec::new();
    for kind in KINDS {
        let points: Vec<_> = car.lamp_points.iter().filter(|p| p.light.kind == kind).collect();
        let lighting: Vec<_> = lit.iter().filter(|p| p.light.kind == kind).collect();
        let light =
            |ls: &[lights::Lamp]| ls.iter().filter(|l| l.kind == kind).map(|l| l.colour.element_sum()).sum::<f32>();
        let dropped = light(&fitted) < 0.999 * light(&lamps);
        let merged = lamps.iter().filter(|l| l.kind == kind).count() - fitted.iter().filter(|l| l.kind == kind).count();
        let parts: Vec<_> = glows.iter().filter(|g| g.kind == kind).collect();
        let k = format!("{kind:?}");
        lines.push(format!(
            "{k}: points {}/{} lit {}{}",
            lighting.len(),
            points.len(),
            lighting
                .iter()
                .map(|p| format!("{}{}", p.light.part, if p.light.fixed.is_some() { "*" } else { "" }))
                .collect::<Vec<_>>()
                .join(" "),
            if merged == 0 { String::new() } else { format!(" ({merged} merged or dropped)") }
        ));
        for g in &parts {
            let colour =
                g.colour.map_or("?".to_owned(), |[r, g, b, v]| format!("({r:.2} {g:.2} {b:.2}) bright {v:.2}"));
            lines.push(format!(
                "    {} at ({:.2} {:.2} {:.2}) {} verts {} area {:.4} m2 {colour}",
                g.name,
                g.centre.x,
                g.centre.y,
                g.centre.z,
                if g.steady { "steady" } else { "switched" },
                g.vertices,
                g.area,
            ));
        }
        let needed = kind != Kind::Reverse || !points.is_empty();
        if lighting.is_empty() && needed {
            flags.push(format!("no-light {k}"));
        }
        if parts.is_empty() && needed {
            flags.push(format!("no-glow {k}"));
        }
        if dropped {
            flags.push(format!("dropped {k}"));
        }
        let near = |g: &Glow| {
            let reach = |l: &lights::Lamp| if g.lens { LENS_NEAR } else { LAMP_NEAR } + l.spread;
            fitted.iter().any(|l| l.kind == kind && g.positions.iter().any(|v| v.distance(l.position) < reach(l)))
        };
        if parts.iter().any(|g| g.steady && !near(g)) {
            flags.push(format!("steady-dark {k}"));
        }
        let red = |c: [f32; 4]| c[0] > 0.3 && c[0] > 1.6 * c[1] && c[0] > 1.6 * c[2];
        if matches!(kind, Kind::Tail | Kind::Brake) && parts.iter().any(|g| g.colour.is_some_and(|c| !red(c))) {
            flags.push(format!("white {k}"));
        }
        if !parts.is_empty() && parts.iter().all(|g| g.colour.is_some_and(|c| c[3] < 0.25)) {
            flags.push(format!("dim {k}"));
        }
        let same = lighting.iter().enumerate().any(|(i, a)| {
            lighting[i + 1..].iter().any(|b| a.position.distance(b.position) < 0.01 && a.light.lamp != b.light.lamp)
        });
        if same {
            flags.push(format!("same-place {k}"));
        }
        let shows = |p: &&&lights::LampPoint| {
            glows.iter().any(|g| g.positions.iter().any(|v| v.distance(p.position) < GLOW_NEAR))
        };
        let bare: Vec<_> = lighting.iter().filter(|p| !shows(p)).map(|p| p.light.part.as_str()).collect();
        if !bare.is_empty() {
            flags.push(format!("bare {k} {}", bare.join(" ")));
        }
        // The model's front is -z: a headlight points forward, the others back.
        let wrong: Vec<_> = lighting
            .iter()
            .filter(|p| (p.normal.z < -0.5) != (kind == Kind::Front) && p.normal.z.abs() > 0.5)
            .map(|p| p.light.part.as_str())
            .collect();
        if !wrong.is_empty() {
            flags.push(format!("wrong-way {k} {}", wrong.join(" ")));
        }
    }
    (lines, flags)
}

/// The mesh's part name, from its draws' order in the car (the meshes carry no name): its index.
fn mesh_name(car: &Car, mesh: &rbr_render::stage::Mesh) -> String {
    let index = car.meshes.iter().position(|m| std::ptr::eq(m, mesh)).unwrap_or(usize::MAX);
    format!("mesh {index}")
}

/// The mean colour and brightness (strongest channel) of `draws`' first texture at their triangles' centres.
fn texel_colour(car: &Car, mesh: &rbr_render::stage::Mesh, draws: &[&rbr_render::stage::Draw]) -> Option<[f32; 4]> {
    let mut sum = [0.0f32; 4];
    let mut n = 0.0;
    for draw in draws {
        let texture = car.textures.get(&draw.state.textures[0]?)?;
        let Some(Image::Dds(bytes)) = &texture.image else { continue };
        let dds = rbr_formats::dds::parse(bytes).ok()?;
        let rgba = rbr_render::upscale::decompress(dds.format, dds.data, dds.width, dds.height);
        for t in mesh.indices[draw.indices.clone()].chunks_exact(3) {
            let uv = t.iter().map(|&i| glam::Vec2::from(mesh.vertices[i as usize].uv0)).sum::<glam::Vec2>() / 3.0;
            let x = (uv.x.rem_euclid(1.0) * dds.width as f32) as usize % dds.width as usize;
            let y = (uv.y.rem_euclid(1.0) * dds.height as f32) as usize % dds.height as usize;
            let at = 4 * (y * dds.width as usize + x);
            let [r, g, b] = [0, 1, 2].map(|c| f32::from(rgba[at + c]) / 255.0);
            sum = [sum[0] + r, sum[1] + g, sum[2] + b, sum[3] + r.max(g).max(b)];
            n += 1.0;
        }
    }
    (n > 0.0).then(|| sum.map(|s| s / n))
}
