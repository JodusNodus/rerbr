//! The quick race's weather (`rbr_gameplay::weather`) on the retail data.
//!
//! Runs only when `RBR_DATA` names the install and `RBR_PATCH` the 1.02 patch files (the exe). Skipped otherwise.

use rbr_formats::data::Data;
use rbr_formats::mat;
use rbr_gameplay::weather::{self, Draws, Preset, Season, Weather};

fn data() -> Option<Data> {
    let data = Data::from_env()?;
    data.patch()?;
    Some(data)
}

/// The maps the install has stages of (`Maps/track-<map>_<letter>.mat`).
fn retail_maps(data: &Data) -> Vec<u32> {
    let mut maps: Vec<u32> = std::fs::read_dir(data.install().join("Maps"))
        .unwrap()
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            name.strip_prefix("track-")?.strip_suffix(".mat")?.split_once('_')?.0.parse().ok()
        })
        .collect();
    maps.sort_unstable();
    maps.dedup();
    maps
}

/// Every weather of `maps`: good, bad and each row of random, with each wear they can take.
fn every_weather(season: &Season, maps: &[u32]) -> Vec<(u32, Preset, Result<Weather, String>)> {
    let mut out = vec![];
    for &map in maps {
        for wear in 0..=2 {
            for preset in [Preset::Good, Preset::Bad] {
                out.push((map, preset, weather::quick_race(season, map, preset, Draws { row: None, wear })));
            }
            for row in season.rows_of(map).filter(|_| wear > 0) {
                let draws = Draws { row: Some(row), wear };
                out.push((map, Preset::Random, weather::quick_race(season, map, Preset::Random, draws)));
            }
        }
    }
    out
}

#[test]
fn every_retail_weather_selects_a_mat_table_of_its_stage() {
    let Some(data) = data() else {
        eprintln!("RBR_DATA or RBR_PATCH unset; skipped");
        return;
    };
    let season = Season::load(&data).expect("the season");
    assert_eq!(season.rows.len(), weather::SEASON_ROWS);
    let maps = retail_maps(&data);
    let (mut checked, mut stops, mut loaded) = (0, vec![], vec![]);
    for (map, preset, w) in every_weather(&season, &maps) {
        let w = match w {
            Ok(w) => w,
            Err(_) => {
                stops.push((map, preset));
                continue;
            }
        };
        let stage = w.stage();
        let path = data
            .resolve(&format!("Maps/{stage}.mat"))
            .unwrap_or_else(|| panic!("map {map} {preset:?}: {stage}.mat is missing"));
        let file = mat::parse(&std::fs::read(path).unwrap()).unwrap();
        let (wear, wetness) = w.mat_words();
        assert!(
            mat::select(&file.entries, wear, wetness).is_some(),
            "map {map} {preset:?}: {stage}.mat has no {wear} {wetness} entry"
        );
        checked += 1;
        loaded.push(stage);
    }
    stops.sort_unstable_by_key(|&(m, p)| (m, p.id()));
    stops.dedup();
    loaded.sort_unstable();
    loaded.dedup();
    eprintln!("{checked} weathers of {} maps checked; no row (int3): {stops:?}", maps.len());
    eprintln!("{} stages loaded by some weather: {loaded:?}", loaded.len());
    // Map 68 is the only retail map with a row and no listed one; ours gives it that row (`Season::unlisted`).
    assert_eq!(stops, []);
}

#[test]
fn every_quick_race_stage_has_a_good_and_a_bad_weather() {
    let Some(data) = data() else {
        eprintln!("RBR_DATA or RBR_PATCH unset; skipped");
        return;
    };
    let season = Season::load(&data).expect("the season");
    // The rally maps (Arctic, British, Australia, Mont Blanc, Japan, USA).
    for map in [10..=15, 20..=25, 31..=36, 41..=46, 51..=56, 61..=66].into_iter().flatten() {
        for preset in [Preset::Good, Preset::Bad] {
            let w = weather::quick_race(&season, map, preset, Draws::default()).unwrap();
            assert!(
                data.resolve(&format!("Maps/{}.lbs", w.stage())).is_some(),
                "map {map} {preset:?}: {} has no .lbs",
                w.stage()
            );
        }
    }
}

#[test]
fn every_retail_row_names_a_track_settings_section() {
    let Some(data) = data() else {
        eprintln!("skipping: RBR_DATA and RBR_PATCH not set");
        return;
    };
    let season = Season::load(&data).unwrap();
    let text = std::fs::read(data.install().join("Maps/TrackSettings.ini")).unwrap();
    let sections = rbr_formats::ini::parse(&String::from_utf8_lossy(&text)).unwrap();
    let mut missing = vec![];
    let mut named = 0;
    for map in retail_maps(&data).into_iter().filter(|&m| m != 4) {
        for row in season.rows_of(map) {
            let race = weather::quick_race(&season, map, Preset::Random, Draws { row: Some(row), wear: 1 }).unwrap();
            if !data.install().join(format!("Maps/{}.lbs", race.stage())).exists() {
                continue;
            }
            let section = race.section(&season).unwrap();
            named += 1;
            if !sections.iter().any(|s| s.name == section) {
                missing.push(section);
            }
        }
    }
    assert!(named > 100, "only {named} rows of installed stages");
    let lacking: Vec<String> = ["M", "N", "E"]
        .iter()
        .flat_map(|l| ["crisp_clear", "hazy_clear", "crisp_partcloud"].map(|w| format!("69S_{l}_{w}")))
        .collect();
    assert_eq!(missing, lacking);
}
