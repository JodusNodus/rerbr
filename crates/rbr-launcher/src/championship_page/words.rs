//! The championship's settings in the page's words, and as the stage's flags and the game take them.

use rbr_season::config::{Damage, EventConfig, Length, Skill, StagePool, StageRef, Surface, Surfaces};
use rbr_season::event::Conditions;

use super::pool;
use crate::catalog::Car;

/// The difficulty's word.
pub fn difficulty_word(difficulty: f32) -> &'static str {
    match difficulty {
        d if d < 0.25 => "Gentle",
        d if d < 0.5 => "Club",
        d if d < 0.75 => "National",
        _ => "Pro",
    }
}

/// The rallies and the stages per rally.
pub fn shape(length: Length) -> (usize, usize) {
    match length {
        Length::Rally { stages } | Length::Weekend { stages } => (1, stages as usize),
        Length::Season { rallies, stages } => (rallies as usize, stages as usize),
    }
}

/// The pool's stages the config draws from.
pub fn eligible<'a>(config: &EventConfig, stages: &'a [StageRef]) -> Vec<&'a StageRef> {
    stages
        .iter()
        .filter(|s| match config.pool {
            StagePool::Retail => !s.community,
            StagePool::Community => s.community,
            StagePool::All => true,
        })
        .filter(|s| match config.surfaces {
            Surfaces::Only(surface) => s.surface == surface,
            Surfaces::Mixed => true,
        })
        .collect()
}

/// The builder's summary: "6 stages · 74 km · gravel · 12 Rally2 drivers", the length the pool's average stage
/// gives (none before the pool is read or without a stage that fits).
pub fn summary(config: &EventConfig, stages: Option<&[StageRef]>) -> String {
    let (rallies, per) = shape(config.length);
    let mut parts = Vec::new();
    if rallies > 1 {
        parts.push(format!("{rallies} rallies"));
    }
    let count = rallies * per;
    parts.push(format!("{count} stage{}", if count == 1 { "" } else { "s" }));
    let fit = stages.map(|s| eligible(config, s)).filter(|s| !s.is_empty());
    if let Some(fit) = fit {
        let average = fit.iter().map(|s| s.length).sum::<f32>() / fit.len() as f32;
        parts.push(format!("{:.0} km", average * count as f32 / 1000.0));
    }
    parts.push(match config.surfaces {
        Surfaces::Only(surface) => pool::surface_word(surface).to_owned(),
        Surfaces::Mixed => "mixed surfaces".to_owned(),
    });
    let class = if config.class.is_empty() { String::new() } else { format!("{} ", config.class) };
    let drivers = config.field_size;
    parts.push(format!("{drivers} {class}driver{}", if drivers == 1 { "" } else { "s" }));
    parts.join(" · ")
}

/// The car's class: its catalog group, "" for none.
pub fn class_of(car: &Car) -> String {
    car.group.clone().unwrap_or_default()
}

/// The classes of `cars`, sorted.
pub fn classes(cars: &[Car]) -> Vec<String> {
    let mut classes: Vec<String> = cars.iter().filter(|c| c.unavailable().is_none()).map(class_of).collect();
    classes.sort();
    classes.dedup();
    classes
}

/// The keys of the cars of `class` that can start: the field's cars.
pub fn class_cars(cars: &[Car], class: &str) -> Vec<String> {
    cars.iter().filter(|c| c.unavailable().is_none() && class_of(c) == class).map(|c| c.key.clone()).collect()
}

/// A road's wetness for the reference time (0 dry to 1 wet), and as the game's 0, 1, 2.
pub fn wetness(conditions: Conditions) -> (f32, u32) {
    match conditions {
        Conditions::Dry => (0.0, 0),
        Conditions::Damp => (0.5, 1),
        Conditions::Wet => (1.0, 2),
    }
}

/// The game's surface number of `surface` (0 gravel, 1 tarmac, 2 snow).
pub fn surface_kind(surface: Surface) -> u32 {
    match surface {
        Surface::Gravel => 0,
        Surface::Tarmac => 1,
        Surface::Snow => 2,
    }
}

/// `rbr-standalone --championship-damage`'s word for the championship's damage.
pub(super) fn damage_word(damage: Damage) -> &'static str {
    match damage {
        Damage::Off => "off",
        Damage::Reduced => "reduced",
        Damage::Realistic => "realistic",
    }
}

/// The game's skill level a championship skill runs as (`SSKILL`: Rookie, Professional, Champion).
pub(super) fn race_skill(skill: Skill) -> rbr_gameplay::race_options::Skill {
    use rbr_gameplay::race_options::Skill as Game;
    match skill {
        Skill::Rookie => Game::Rookie,
        Skill::Professional => Game::Professional,
        Skill::Champion => Game::Champion,
    }
}

/// What the skill level does to outside assistance (`M576710`'s penalty, the gate `E60Flow::M566640`).
pub fn assistance_note(skill: Skill) -> String {
    use rbr_gameplay::race_options::{ChampionshipOptions, RaceOptions};
    let options = ChampionshipOptions { skill: race_skill(skill), ..ChampionshipOptions::default() };
    let settings = options.assist_settings(&RaceOptions::default());
    let penalty = settings.penalty();
    if rbr_gameplay::flow::assistance_gate(settings) {
        format!("{penalty} s penalty")
    } else {
        format!("{penalty} s penalty; a stuck car is not helped back and retires")
    }
}

/// The skill level's word.
pub fn skill_word(skill: Skill) -> &'static str {
    match skill {
        Skill::Rookie => "Rookie",
        Skill::Professional => "Professional",
        Skill::Champion => "Champion",
    }
}

/// `hours` as `H:MM`.
pub fn clock_hm(hours: f32) -> String {
    let minutes = (hours * 60.0).round() as u32 % (24 * 60);
    format!("{}:{:02}", minutes / 60, minutes % 60)
}
