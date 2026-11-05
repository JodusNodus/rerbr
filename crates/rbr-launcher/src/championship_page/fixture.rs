//! What the screenshots and the tests build a championship on without game files: a few stages, a small `ai.ini` /
//! `aidrv.ini` stand-in (the 1.02 AI engine's own test fixtures) and a championship built by the real `Event::build`.

use rbr_formats::ai_ini::{Car, Roster, Team};
use rbr_season::ai::fixtures;
use rbr_season::config::{EventConfig, Length, Preset, StageRef, Surface, Surfaces};
use rbr_season::event::Event;
use rbr_season::pace::class_pace;
use rbr_season::track::StageProfile;

use super::Ai;

/// The AI tables: the engine's test master and a roster of three teams with different crews.
pub fn ai() -> Ai {
    let team =
        |name: &str, mechanics, engineering| Team { name: name.into(), mechanics, engineering, ..fixtures::team() };
    let car = |name: &str, power| Car { name: name.into(), control: 4, power, handling: 4, durability: 3 };
    let roster = Roster {
        drivers: Vec::new(),
        teams: vec![team("Privateer", 3, 2), team("Works team", 6, 3), team("Factory", 8, 5)],
        cars: vec![car("Skoda Fabia Rally2", 4), car("Citroen C3 Rally2", 5), car("Ford Fiesta Rally2", 4)],
    };
    (fixtures::master(), roster)
}

/// The stages the screenshots draw a championship on.
pub fn stages() -> Vec<StageRef> {
    [
        ("Harwood Forest", 9_870.0, false),
        ("Mustaselka", 7_120.0, false),
        ("Falstone", 12_420.0, false),
        ("Kaihuavaara", 6_840.0, false),
        ("Sipirkakim", 10_090.0, false),
        ("Ouninpohja", 14_310.0, true),
        ("Chirdonhead", 8_350.0, false),
        ("Pirka Menoko", 11_750.0, false),
    ]
    .iter()
    .enumerate()
    .map(|(k, &(name, length, community))| StageRef {
        key: (20 + k).to_string(),
        name: name.to_owned(),
        family: "fixture".to_owned(),
        surface: Surface::Gravel,
        community,
        length,
    })
    .collect()
}

/// A built championship on [`stages`] (a season of two rallies of six stages) at its welcome.
pub fn fixture_event() -> Event {
    let config = EventConfig {
        length: Length::Season { rallies: 2, stages: 6 },
        surfaces: Surfaces::Only(Surface::Gravel),
        ..Preset::Season.config("Rally2", "", 7)
    };
    let cars = ["Skoda Fabia Rally2".to_owned(), "Citroen C3 Rally2".to_owned()];
    let class = class_pace("Rally2");
    let profile = |s: &StageRef| StageProfile::flat(s.length, s.surface, &class);
    Event::build(config, &stages(), &cars, &ai().1, &profile).unwrap()
}
