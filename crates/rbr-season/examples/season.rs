//! Simulates a whole built season on the retail stages with a fixed player time (1.02 times the stage's reference):
//! the rally standings, the season points, the bulletins and the retirements. The AI engine's tables are read from
//! `RBR_DATA/Root/misc.rbz` at run time.
//!
//! `bash -c 'source scripts/dev-env.sh; cargo run --release -p rbr-season --example season [seed]'`

use std::collections::HashMap;
use std::path::PathBuf;

use rbr_formats::trk;
use rbr_season::config::{Length, Preset, StageRef, Surface};
use rbr_season::event::{Event, PlayerResult, Turn};
use rbr_season::field::AiResult;
use rbr_season::pace::{class_pace, path_length, timed_road, TimedRoad};
use rbr_season::track::{profile, StageProfile};

fn main() {
    let seed: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(2026);
    let data = rbr_formats::data::Data::from_env().expect("RBR_DATA");
    let (master, roster) = rbr_season::ai::load_ai(&data).expect("ai.ini");
    let root = std::env::var_os("RBR_DATA").map(PathBuf::from).unwrap();
    let class = class_pace("Rally 2");
    let mut stages = Vec::new();
    let mut profiles: HashMap<String, StageProfile> = HashMap::new();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(root.join("Maps"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "trk"))
        .collect();
    paths.sort();
    for path in paths {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let Some(map) =
            stem.strip_prefix("track-").and_then(|r| r.split_once('_')).and_then(|(m, _)| m.parse::<u32>().ok())
        else {
            continue;
        };
        let Some(retail) = master.tracks.iter().find(|t| t.stage == map) else { continue };
        let surface = match retail.sections[0].material {
            1 => Surface::Tarmac,
            2 => Surface::Snow,
            _ => Surface::Gravel,
        };
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Some(line) = trk::parse(&bytes).ok().and_then(|f| {
            f.chunks.into_iter().find_map(|c| match c.payload {
                trk::ChunkPayload::DriveLine(d) => Some(d),
                _ => None,
            })
        }) else {
            continue;
        };
        let markers = std::fs::read(path.with_extension("dls")).ok().and_then(|b| rbr_formats::markers::parse(&b).ok());
        let TimedRoad { points, splits } = timed_road(&line, markers.as_ref());
        let length = path_length(&points);
        if length < 500.0 {
            continue;
        }
        profiles.insert(stem.clone(), profile(&points, &splits, surface, &class));
        stages.push(StageRef {
            key: stem.clone(),
            name: format!("Map {map} {stem}"),
            family: (map / 10).to_string(),
            surface,
            community: false,
            length,
        });
    }
    let mut config = Preset::Season.config("Rally 2", "player-car", seed);
    config.length = Length::Season { rallies: 3, stages: 6 };
    config.team = 3;
    let cars: Vec<String> = (1..=6).map(|k| format!("car-{k}")).collect();
    let mut event = Event::build(config, &stages, &cars, &roster, &|s| profiles[&s.key].clone()).expect("event");
    println!("{} stages in the pool; team crew {:?}", stages.len(), event.crew());
    loop {
        match event.turn.clone() {
            Turn::Welcome { rally } => {
                println!("\n== {} ({} stages) ==", event.rallies[rally].name, event.rallies[rally].stages.len());
                event.turn_done();
            }
            Turn::Service { .. } => event.turn_done(),
            Turn::Stage { rally, stage } => {
                let it = &event.rallies[rally].stages[stage];
                let time =
                    it.reference * 1.02 * if it.conditions == rbr_season::event::Conditions::Wet { 1.09 } else { 1.0 };
                let wear = event.road_wear(rally, stage);
                println!(
                    "SS{} {:30} {:?} ref {:6.1}s player {:6.1}s road wear {wear}",
                    stage + 1,
                    it.stage.name,
                    it.conditions,
                    it.reference,
                    time
                );
                event.stage_done(&master, PlayerResult { time, penalties: 0.0, retired: false });
            }
            Turn::RallyOver { rally } => {
                println!("-- standings --");
                for (i, row) in event.rally_standings(rally).iter().enumerate() {
                    let who = row.entrant.map_or("PLAYER".to_owned(), |e| event.field[e].name.clone());
                    match row.total {
                        Some(t) => println!("{:2}. {who:16} {:7.1}s  {:2} pts", i + 1, t, row.points),
                        None => println!("{:2}. {who:16} retired", i + 1),
                    }
                }
                println!("-- bulletins --");
                for line in event.bulletins(rally) {
                    println!("{line}");
                }
                let out = event.results[rally]
                    .last()
                    .map_or(0, |(_, ai)| ai.iter().filter(|r| **r == AiResult::Retired).count());
                println!("-- {out} of {} entrants retired in the rally --", event.field.len());
                event.turn_done();
            }
            Turn::EventOver => break,
        }
    }
    println!("\n== season points ==");
    for (i, row) in event.points().iter().enumerate() {
        let who = row.entrant.map_or("PLAYER".to_owned(), |e| event.field[e].name.clone());
        println!("{:2}. {who:16} {:3} pts", i + 1, row.points);
    }
}
