//! The Championship tab's drawing: what it draws with ([`Env`]), the helpers its screens share, and which screen it
//! is on: the entry and the builder (`builder`), or the championship under way (`running`, with `cards`).

use std::path::Path;

use egui::Ui;
use rbr_season::event::{run_stages, Conditions, Event, PlayerResult, Standing, Turn};
use rbr_season::field::AiResult;

use super::*;
use crate::pages::Issue;

/// What the page draws with.
pub struct Env<'a> {
    pub here: &'a Path,
    pub profile: &'a mut String,
    /// The catalog's cars that NGP drives.
    pub cars: &'a [Car],
    /// The car a new builder starts on (the Drive page's, the last driven).
    pub default_car: usize,
    /// Draws the builder's car field for the car; whether Browse was clicked.
    pub car_field: &'a mut dyn FnMut(&mut Ui, usize) -> bool,
    /// The car's saved setups on a surface (`SetupPage::saved_names`).
    pub saved: &'a dyn Fn(u32) -> Vec<(String, String)>,
    /// What blocks a stage's launch.
    pub blocker: Option<&'a Issue>,
    /// A new builder's seed.
    pub seed: u32,
    /// NGP's `physics.rbz`: the archive cars' physics, for the field's pace.
    pub archive: &'a Path,
    /// The pack cars' earlier keys and theirs now (`Catalog::renamed`), for a championship saved before.
    pub renamed: &'a [(String, String)],
}

/// The step of the championship's turn: "Before the start", "Stage 3 of 6", "Service before stage 3 of 6", ...
pub fn step(event: &Event) -> String {
    let count = |rally: usize| event.rallies.get(rally).map_or(0, |r| r.stages.len());
    match event.turn {
        Turn::Welcome { .. } => "Before the start".to_owned(),
        Turn::Stage { rally, stage } => format!("Stage {} of {}", stage + 1, count(rally)),
        Turn::Service { rally, stage, .. } => format!("Service before stage {} of {}", stage + 2, count(rally)),
        Turn::RallyOver { .. } => "Rally over".to_owned(),
        Turn::EventOver => "Over".to_owned(),
    }
}

/// The stage a car retired under Super Rally restarts on, while the turn is the overnight service before it.
pub(super) fn rejoin(event: &Event) -> Option<usize> {
    let Turn::Service { rally, stage, .. } = event.turn else { return None };
    event.charged(rally, stage, None).map(|_| stage + 1)
}

/// The rally the turn is in (the last one at the championship's end).
pub(super) fn rally_of(event: &Event) -> usize {
    match event.turn {
        Turn::Welcome { rally }
        | Turn::Stage { rally, .. }
        | Turn::Service { rally, .. }
        | Turn::RallyOver { rally } => rally,
        Turn::EventOver => event.rallies.len().saturating_sub(1),
    }
}

/// The rows of `rally`'s results the player or the field drove (the model keeps a row per stage).
pub(super) fn driven(event: &Event, rally: usize) -> &[(Option<PlayerResult>, Vec<AiResult>)] {
    let results = event.results.get(rally).map_or(&[][..], Vec::as_slice);
    &results[..run_stages(results)]
}

/// The player's place in the standings and their size, while the player has a total.
pub(super) fn place(lines: &[Standing]) -> Option<(usize, usize)> {
    let at = lines.iter().position(|l| l.entrant.is_none())?;
    lines[at].total.map(|_| (at + 1, lines.len()))
}

/// A driver's name: the field's, "You" for the player.
pub(super) fn driver(event: &Event, entrant: Option<usize>) -> String {
    entrant.and_then(|e| event.field.get(e)).map_or("You".to_owned(), |e| e.name.clone())
}

/// A catalog car key's name.
pub(super) fn car_name(cars: &[Car], key: &str) -> String {
    cars.iter().find(|c| c.key == key).map_or(key.to_owned(), |c| c.name.clone())
}

/// An entrant's (or the player's) time on stage `stage` of `rally`: none once retired or not run.
pub(super) fn stage_time(event: &Event, rally: usize, stage: usize, entrant: Option<usize>) -> Option<f32> {
    let (player, ai) = driven(event, rally).get(stage)?;
    match entrant {
        None => player.filter(|p| !p.retired).map(|p| p.time),
        Some(e) => match ai.get(e)? {
            AiResult::Time(t) => Some(*t),
            AiResult::Retired | AiResult::Out => None,
        },
    }
}

pub(super) fn road_word(conditions: Conditions) -> &'static str {
    match conditions {
        Conditions::Dry => "dry",
        Conditions::Damp => "damp",
        Conditions::Wet => "wet",
    }
}

pub(super) fn km(metres: f32) -> String {
    format!("{:.1} km", metres / 1000.0)
}

pub(super) fn label_class(class: &str) -> &str {
    if class.is_empty() {
        "unclassed"
    } else {
        class
    }
}

impl ChampionshipPage {
    /// The page: the presets, the builder or the championship under way.
    pub fn ui(&mut self, ui: &mut Ui, env: Env) -> Option<Request> {
        match self.view {
            View::Builder if self.config.is_some() => self.builder_ui(ui, env),
            View::Running if self.run.is_some() => self.event_ui(ui, env),
            _ => {
                self.view = View::Entry;
                self.entry_ui(ui, env);
                None
            }
        }
    }
}
