//! The generated AI tracks against the install's own `ai.ini` (read at run time, nothing kept): the championship
//! never reads `ai.ini`'s per-stage tracks, so the generator must stay close to them on the retail stages. The full
//! table is `examples/track.rs`. Skipped without `RBR_DATA`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rbr_formats::trk;
use rbr_season::config::Surface;
use rbr_season::pace::{class_pace, timed_road};
use rbr_season::track::{profile, PAR_SCALE};

#[test]
fn generated_tracks_stay_near_the_retail_pars() {
    let Some(data) = rbr_formats::data::Data::from_env() else {
        eprintln!("RBR_DATA unset: skipped");
        return;
    };
    let (master, _) = rbr_season::ai::load_ai(&data).expect("ai.ini");
    let maps = std::env::var_os("RBR_DATA").map(PathBuf::from).unwrap().join("Maps");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&maps)
        .expect("Maps")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "trk"))
        .collect();
    files.sort();
    let class = class_pace("WRC 2.0");
    let mut ratios: BTreeMap<&str, Vec<f32>> = BTreeMap::new();
    for retail in master.tracks.iter().filter(|t| t.wet == 0) {
        let prefix = format!("track-{}_", retail.stage);
        let Some(path) = files.iter().find(|p| p.file_stem().unwrap().to_string_lossy().starts_with(&prefix)) else {
            continue;
        };
        let line = trk::parse(&std::fs::read(path).unwrap()).ok().and_then(|f| {
            f.chunks.into_iter().find_map(|c| match c.payload {
                trk::ChunkPayload::DriveLine(d) => Some(d),
                _ => None,
            })
        });
        let Some(line) = line else { continue };
        let markers = std::fs::read(path.with_extension("dls")).ok().and_then(|b| rbr_formats::markers::parse(&b).ok());
        let road = timed_road(&line, markers.as_ref());
        let surface = match retail.sections[0].material {
            1 => Surface::Tarmac,
            2 => Surface::Snow,
            _ => Surface::Gravel,
        };
        let generated = profile(&road.points, &road.splits, surface, &class).track(retail.stage, PAR_SCALE);
        for (g, r) in generated.sections.iter().zip(&retail.sections) {
            assert_eq!(g.material, r.material, "map {} material", retail.stage);
        }
        let total = |t: &rbr_formats::ai_ini::Track| t.sections.iter().map(|s| s.par).sum::<f32>();
        ratios.entry(surface.name()).or_default().push(total(&generated) / total(retail));
    }
    assert!(ratios.values().map(Vec::len).sum::<usize>() >= 30, "too few retail maps compared: {ratios:?}");
    // The anchor matches the retail pars over every map; the surfaces apart follow the NGP hot laps, which hold snow
    // and gravel faster against tarmac than 1.02's pars do (`notes/season.md` "Calibration").
    let all: Vec<f32> = ratios.values().flatten().copied().collect();
    let mean = all.iter().sum::<f32>() / all.len() as f32;
    assert!((0.95..=1.05).contains(&mean), "generated / retail total par {mean:.3} over {} maps", all.len());
    for (surface, v) in &ratios {
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        assert!(
            (0.88..=1.12).contains(&mean),
            "{surface}: generated / retail total par {mean:.3} over {} maps",
            v.len()
        );
    }
}
