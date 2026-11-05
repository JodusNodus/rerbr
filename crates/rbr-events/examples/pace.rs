//! The pace model's calibration: every installed stage's length and reference time per class, with the retail
//! stages' 1.02 AI beside the top class's time.
//!
//! Retail drivelines come from `RBR_DATA/Maps/track-*.trk`; the AI column runs the 1.02 championship AI's best
//! driver (`rbr-championship`'s `ai::stage_time`, level 2, dry, averaged over seeds) over the dry rows of
//! `RBR_DATA/Root/misc.rbz` (`Misc/ai/ai.ini`, `aidrv.ini`). The RSF stages (`RBR_RSF/Maps/<id>-<name>/`, when set)
//! print with their scanned surface. Run with `bash -c 'source scripts/dev-env.sh; cargo run -p rbr-events
//! --example pace'`.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use rbr_championship::ai::{self, Run, RunLog};
use rbr_events::pace::{class_pace, driveline_points, path_length, reference_time, timed_slice};
use rbr_formats::ai_ini;
use rbr_formats::trk::{self, DriveLineData};
use rbr_gameplay::crt::CrtRand;

/// The classes of the summary.
const CLASSES: [&str; 12] = [
    "WRC 2.0",
    "Group B",
    "Group A8",
    "Group RGT",
    "Rally 2",
    "Group N4",
    "Super 2000",
    "Rally 3",
    "Rally 4",
    "Super 1600",
    "Group 4",
    "Group 2",
];

fn read_trk(path: &Path) -> Option<DriveLineData> {
    let bytes = std::fs::read(path).ok()?;
    let file = trk::parse(&bytes).ok()?;
    file.chunks.into_iter().find_map(|c| match c.payload {
        trk::ChunkPayload::DriveLine(d) => Some(d),
        _ => None,
    })
}

fn main() {
    let data = std::env::var_os("RBR_DATA").map(PathBuf::from).expect("RBR_DATA");
    // The 1.02 AI's best driver: the highest technique, speed and condition in `aidrv.ini`.
    let misc = data.join("Root/misc.rbz");
    let mut zip = rbr_formats::rbz::open(&misc).expect("misc.rbz");
    let mut read = |name: &str| {
        let mut text = Vec::new();
        rbr_formats::rbz::by_name(&mut zip, name).expect(name).read_to_end(&mut text).expect(name);
        String::from_utf8_lossy(&text).into_owned()
    };
    let master = ai_ini::parse_master(&read("Misc/ai/ai.ini")).expect("ai.ini");
    let roster = ai_ini::parse_roster(&read("Misc/ai/aidrv.ini")).expect("aidrv.ini");
    let best = roster
        .drivers
        .iter()
        .max_by_key(|d| d.technique + d.speed + d.condition + d.skills.iter().sum::<i32>())
        .expect("drivers")
        .clone();
    let team = roster.teams.first().expect("teams").clone();
    let car = roster.cars.first().expect("cars").clone();
    // The AI times by retail map: the best driver's mean over seeds (level 2, dry).
    let mut driven: BTreeMap<u32, f32> = BTreeMap::new();
    let mut materials: BTreeMap<u32, [u32; 3]> = BTreeMap::new();
    for track in master.tracks.iter().filter(|t| t.wet == 0) {
        let mut total = 0.0f64;
        let mut runs = 0;
        for seed in 0..8u32 {
            let mut rng = CrtRand::srand(1000 + seed);
            let mut driver = ai::DriverState::new(best.clone());
            let mut car_state = ai::CarState::new(car.clone());
            let mut false_starts = 0;
            let mut log = RunLog::default();
            let run = Run { track, level: 2, stage: 0, extra: 0 };
            let time = ai::stage_time(
                &master,
                &run,
                0,
                &mut driver,
                &mut car_state,
                &team,
                &mut false_starts,
                &mut rng,
                &mut log,
            );
            if time > 0.0 {
                total += time as f64;
                runs += 1;
            }
        }
        if runs > 0 {
            driven.entry(track.stage).or_insert((total / runs as f64) as f32);
        }
        let entry = materials.entry(track.stage).or_insert([0; 3]);
        for s in &track.sections {
            entry[(s.material as usize).min(2)] += 1;
        }
    }
    println!("retail stages (RBR_DATA): key, surface, length, the 1.02 AI's best driver, then the reference per class");
    println!("{}", header());
    let mut maps: Vec<PathBuf> = std::fs::read_dir(data.join("Maps"))
        .expect("Maps")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "trk"))
        .collect();
    maps.sort();
    let mut totals: BTreeMap<&str, (f64, f64, usize)> = BTreeMap::new();
    for path in &maps {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let Some(rest) = stem.strip_prefix("track-") else { continue };
        let Some((map, _)) = rest.split_once('_') else { continue };
        let Ok(id) = map.parse::<u32>() else { continue };
        let Some(driveline) = read_trk(path) else { continue };
        let points = driveline_points(&driveline);
        let (start, finish) = stage_bounds(&path.with_extension("dls"), path_length(&points));
        let points = timed_slice(&points, start, finish);
        let length = path_length(&points);
        if length < 100.0 {
            continue;
        }
        let material = materials
            .get(&id)
            .map(|m| m.iter().enumerate().max_by_key(|(_, &n)| n).map(|(i, _)| i).unwrap_or(0))
            .unwrap_or(0);
        let surface = match material {
            1 => rbr_formats::rsf::Surface::Tarmac,
            2 => rbr_formats::rsf::Surface::Snow,
            _ => rbr_formats::rsf::Surface::Gravel,
        };
        let won = driven.get(&id).copied().unwrap_or(f32::NAN);
        let times: Vec<f32> = CLASSES.iter().map(|c| reference_time(&points, surface, 0.0, &class_pace(c))).collect();
        println!(
            "{stem:12} {:6} {:5.1}km {:>7.1}s {}",
            surface.name(),
            length / 1000.0,
            won,
            times.iter().map(|t| format!("{t:7.1}")).collect::<Vec<_>>().join(" ")
        );
        if won.is_finite() {
            let e = totals.entry(surface.name()).or_insert((0.0, 0.0, 0));
            e.0 += won as f64;
            e.1 += times[0] as f64;
            e.2 += 1;
        }
    }
    println!("\ncalibration (the 1.02 AI's best driver vs the top class, by surface):");
    for (surface, (won, wrc, n)) in &totals {
        println!("  {surface:6}: {n:3} stages, AI {won:9.1}s, model {wrc:9.1}s, ratio {:5.3}", wrc / won);
    }
    // The community stages.
    if let Some(rsf) = std::env::var_os("RBR_RSF").map(PathBuf::from) {
        println!("\ncommunity stages (RBR_RSF):");
        println!("{}", header());
        let mut shown = 0;
        for stage in rbr_formats::rsf::scan(&rsf) {
            let Ok(stage) = stage else { continue };
            let surface = stage.surface.unwrap_or(rbr_formats::rsf::Surface::Gravel);
            for letter in &stage.variants {
                let trk = stage.folder.join(format!("track-{}_{}.trk", stage.id, letter));
                let Some(driveline) = read_trk(&trk) else { continue };
                let points = driveline_points(&driveline);
                let (start, finish) = stage_bounds(&trk.with_extension("dls"), path_length(&points));
                let points = timed_slice(&points, start, finish);
                let length = path_length(&points);
                if length < 100.0 {
                    continue;
                }
                let times: Vec<f32> =
                    CLASSES.iter().map(|c| reference_time(&points, surface, 0.0, &class_pace(c))).collect();
                println!(
                    "{}-{:<10} {:6} {:5.1}km {:>7}  {}",
                    stage.id,
                    letter,
                    surface.name(),
                    length / 1000.0,
                    "-",
                    times.iter().map(|t| format!("{t:7.1}")).collect::<Vec<_>>().join(" ")
                );
                shown += 1;
            }
            if shown > 400 {
                break;
            }
        }
    }
}

fn header() -> String {
    format!("{:12} {:6} {:7} {:>7} {}", "stage", "surf", "length", "AI-best", CLASSES.join(" "))
}

/// The timed stage's bounds: the `.dls` start/finish markers (metres along the line), whole driveline without one.
fn stage_bounds(dls: &Path, length: f32) -> (f32, f32) {
    let Ok(bytes) = std::fs::read(dls) else { return (0.0, length) };
    let Ok(markers) = rbr_formats::markers::parse(&bytes) else { return (0.0, length) };
    let (start, finish) = (markers.start().unwrap_or(0.0), markers.finish().unwrap_or(length));
    if 0.0 <= start && start < finish {
        (start, finish.min(length))
    } else {
        (0.0, length)
    }
}
