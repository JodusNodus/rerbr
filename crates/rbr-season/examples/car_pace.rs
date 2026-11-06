//! The car models' pace within a field (`rbr_season::car_pace`): each installed RSF pack's straight line and
//! downforce from its physics scripts, then for a few fields the time each model takes over the field's mean car on
//! the retail stages' roads, gravel and tarmac apart (the France maps, 4x, are the tarmac ones), and every pack's
//! over the Fiesta WRC 2019's (the `vs,` lines, to set against the RallySimFans hot laps' car effects).
//!
//! Run with `bash -c 'source scripts/dev-env.sh; cargo run -p rbr-season --example car_pace'` (`RBR_DATA`,
//! `RBR_RSF`).
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rbr_formats::car_specs::CarSpecs;
use rbr_formats::rsf::Surface;
use rbr_formats::trk::{self, DriveLineData};
use rbr_season::car_pace::{self, CarPace};
use rbr_season::pace::{class_pace, reference_time, timed_road};

/// The fields: a class and the pack folders whose names start with each prefix.
const FIELDS: [(&str, &[&str]); 5] = [
    (
        "WRC 2.0",
        &[
            "Ford_Fiesta_WRC_2019",
            "Hyundai_i20_Coupe_WRC_2020",
            "Ford_Fiesta_RS_WRC_2014",
            "Ford_Focus_Mk_II",
            "Citroen_Xsara_WRC",
        ],
    ),
    ("Rally 2", &["Ford_Fiesta_Rally2", "Toyota_GR_Yaris_Rally2", "Ford_Fiesta_R5", "VW_Polo_GTI_R5"]),
    ("Group B", &["Audi_quattro_A2", "Porsche_911_SC_RS_GrpB", "Lada_VFTS"]),
    (
        "Group A8",
        &[
            "Subaru_Impreza_GC8",
            "Mitsubishi_Lancer_Evo_II",
            "Ford_Escort_Mk_V",
            "Toyota_Celica",
            "Lancia_Delta",
            "BMW_M3_E30",
        ],
    ),
    ("WRC 2.0", &["Audi_quattro_A2", "Ford_Fiesta_WRC_2019", "Ford_Fiesta_Rally2", "Ford_Escort_Mk_V"]),
];

/// The RallySimFans groups, for the class lines.
const CLASSES: [&str; 22] = [
    "WRC 1.6",
    "WRC 2.0",
    "Rally 2",
    "Group R5",
    "Group R4",
    "Group N4",
    "Super 2000",
    "Rally 3",
    "Group R3",
    "Rally 4",
    "Group R2",
    "Super 1600",
    "Rally 5",
    "Group R1",
    "Group A8",
    "Group A7",
    "Group A6",
    "Group A5",
    "Group B",
    "Group 4",
    "Group 2",
    "Group RGT",
];

/// The car every pack's time is compared with at the end.
const VERSUS: &str = "Ford_Fiesta_WRC_2019";

fn read_trk(path: &Path) -> Option<DriveLineData> {
    let file = trk::parse(&std::fs::read(path).ok()?).ok()?;
    file.chunks.into_iter().find_map(|c| match c.payload {
        trk::ChunkPayload::DriveLine(d) => Some(d),
        _ => None,
    })
}

fn main() {
    let data = std::env::var_os("RBR_DATA").map(PathBuf::from).expect("RBR_DATA");
    let rsf = std::env::var_os("RBR_RSF").map(PathBuf::from).expect("RBR_RSF");
    let mut cars: BTreeMap<String, CarPace> = BTreeMap::new();
    for entry in std::fs::read_dir(rsf.join("rsfdata/cars")).expect("rsfdata/cars").flatten() {
        let dir = entry.path();
        let read = |name: &str| std::fs::read(dir.join(name)).ok().map(|b| String::from_utf8_lossy(&b).into_owned());
        if let (Some(common), Some(surface)) = (read("common.lsp"), read("r_tarmac.lsp")) {
            if let Some(specs) = CarSpecs::parse(&common, &surface) {
                cars.insert(entry.file_name().to_string_lossy().into_owned(), CarPace::of(&specs));
            }
        }
    }
    let mut roads: Vec<(Surface, Vec<[f32; 3]>)> = Vec::new();
    for entry in std::fs::read_dir(data.join("Maps")).expect("Maps").flatten() {
        let stem = entry.path().file_stem().unwrap().to_string_lossy().into_owned();
        let Some(map) = stem.strip_prefix("track-").and_then(|r| r.split('_').next()?.parse::<u32>().ok()) else {
            continue;
        };
        if let Some(line) = read_trk(&entry.path()) {
            let surface = if map / 10 == 4 { Surface::Tarmac } else { Surface::Gravel };
            roads.push((surface, timed_road(&line, None).points));
        }
    }
    println!("{} packs, {} retail roads", cars.len(), roads.len());
    for (class, prefixes) in FIELDS {
        let field: Vec<(&String, CarPace)> =
            cars.iter().filter(|(k, _)| prefixes.iter().any(|p| k.starts_with(p))).map(|(k, c)| (k, *c)).collect();
        let Some(mean) = car_pace::mean(&field.iter().map(|f| f.1).collect::<Vec<_>>()) else { continue };
        let class = class_pace(class);
        println!(
            "\nfield ({} cars) on the {} class: time over the mean car, gravel and tarmac",
            field.len(),
            prefixes[0]
        );
        for (key, car) in &field {
            let ratio = |surface: Surface| {
                let (mut own, mut base) = (0.0, 0.0);
                for (_, points) in roads.iter().filter(|r| r.0 == surface) {
                    own += reference_time(points, surface, 0.0, &car_pace::for_car(class, *car, mean));
                    base += reference_time(points, surface, 0.0, &car_pace::for_car(class, mean, mean));
                }
                100.0 * (own / base * car.handling(surface) / mean.handling(surface) - 1.0)
            };
            println!(
                "  {key:45} accel {:5.2} top {:4.1} aero@150 {:4.2}  gravel {:+5.1} %  tarmac {:+5.1} %",
                car.straight.accel,
                car.straight.top,
                car.aero * 41.67 * 41.67,
                ratio(Surface::Gravel),
                ratio(Surface::Tarmac)
            );
        }
    }
    // Each class's pace over the top class's, for the leaderboards' class effects.
    println!("\nclass over WRC: class, gravel %, tarmac %");
    for name in CLASSES {
        let ratio = |surface: Surface| {
            let (mut own, mut top) = (0.0, 0.0);
            for (_, points) in roads.iter().filter(|r| r.0 == surface) {
                own += reference_time(points, surface, 0.0, &class_pace(name));
                top += reference_time(points, surface, 0.0, &class_pace("WRC"));
            }
            100.0 * (own / top - 1.0)
        };
        println!("class,{name},{:.2},{:.2}", ratio(Surface::Gravel), ratio(Surface::Tarmac));
    }
    // Every pack against the reference car on the top class's pace, for the leaderboards' car effects.
    let Some(base) = cars.iter().find(|(k, _)| k.starts_with(VERSUS)).map(|(_, c)| *c) else { return };
    let class = class_pace("WRC");
    println!("\nversus {VERSUS}: car, plan gravel %, plan tarmac %, gravel %, tarmac %, snow %, accel, top");
    for (key, car) in &cars {
        let ratio = |surface: Surface| {
            let (mut own, mut other) = (0.0, 0.0);
            for (_, points) in roads.iter().filter(|r| r.0 == surface) {
                own += reference_time(points, surface, 0.0, &car_pace::for_car(class, *car, base));
                other += reference_time(points, surface, 0.0, &car_pace::for_car(class, base, base));
            }
            own / other
        };
        // The speed plan alone, then with the handling (snow takes the gravel roads' plan: no retail snow stage).
        let (gravel, tarmac) = (ratio(Surface::Gravel), ratio(Surface::Tarmac));
        let with = |plan: f32, s: Surface| 100.0 * (plan * car.handling(s) / base.handling(s) - 1.0);
        println!(
            "vs,{key},{:.2},{:.2},{:.2},{:.2},{:.2},{:.3},{:.2}",
            100.0 * (gravel - 1.0),
            100.0 * (tarmac - 1.0),
            with(gravel, Surface::Gravel),
            with(tarmac, Surface::Tarmac),
            with(gravel, Surface::Snow),
            car.straight.accel,
            car.straight.top
        );
    }
}
