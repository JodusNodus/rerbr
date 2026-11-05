//! The Championship tab's events (ours, `rbr_season`): a rally or a season the player builds from a preset, on any
//! installed stage (retail, RSF, BTB: [`pool`]), against a field in the car's class. The tab opens on the presets
//! (Quick rally, Weekend, Season, Random, and the Classic 1.02 season, which is `rally_page`'s season as it was), with
//! Continue event first when the profile has one. A preset opens the builder with its settings; Start event builds the
//! event, and the page then runs its turns: the rally's welcome and itinerary, each stage (its conditions, start time,
//! tyres and setup, then Drive), the standings with gaps and the stage winner after each, the service park where the
//! itinerary has one (`rally_page::park_card`, the damage carried), the rally's end with its points, and the event's
//! end.
//!
//! A stage runs as on the Drive page on the stage's own weather row of the itinerary's road (dry, damp or wet; the
//! nearest row with `--wetness` when the stage has none), at the itinerary's clock (`viewer --clock`,
//! `rbr-standalone --start-time`), as a championship stage (`--championship professional`, flow mode 3: no restart)
//! on the event's damage. The damage carried goes in with `--damage-in`; the stage's result and damage come back
//! through `--result-out` and `--damage-out` in `events/` in the player's folder, as the 1.02 season's do.
//!
//! One event per profile: `SavedGames/event-<profile>.txt` (`Event::save`) next to the 1.02 saves, with the carried
//! damage in `event-<profile>.dmg` and the service's late penalty still to add and the tyre on the car in
//! `event-<profile>.ini` (the event's API keeps neither).

use std::path::{Path, PathBuf};

use rbr_championship::service::{Crew, ServiceKind};
use rbr_championship::{ServicePark, StageReport};
use rbr_formats::data::Data;
use rbr_formats::saves::{is_file_name_part, SaveDir};
use rbr_formats::strings::Strings;
use rbr_gameplay::car_damage::CarriedDamage;
use rbr_gameplay::crt::CrtRand;
use rbr_gameplay::flow::{EndCause, EndRace, StageEnd};
use rbr_gameplay::setup::SeasonSetup;
use rbr_season::config::{Damage, EventConfig, Length, Preset, StagePool, StageRef, Surface, Surfaces};
use rbr_season::event::{Conditions, ItineraryStage, PlayerResult, Turn};

use crate::catalog::Car;

pub mod fixture;
mod launch;

pub use launch::shots;
pub mod model;
pub mod pool;
mod view;

use model::{Model, Play};
use pool::Pool;

/// What the Championship tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// The presets, and Continue event.
    Entry,
    /// The 1.02 season (`rally_page`).
    Classic,
    Builder,
    /// The event under way.
    Event,
}

/// The preset a builder started from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Builder {
    pub preset: Preset,
}

/// A stage to drive: its map, road and clock, the tyre and setup picked, and the extra `rbr-standalone` arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct EventStart {
    pub map: u32,
    pub conditions: Conditions,
    /// Hours, 0 to 24.
    pub clock: f32,
    pub tyre: u32,
    pub setup: SeasonSetup,
    pub args: Vec<String>,
}

/// What the page asks of the launcher.
pub enum Request {
    Launch(EventStart),
    /// Open the Setup page.
    Setup,
    /// Open the car browser on the builder's car.
    BrowseCar,
}

/// The event under way and what the launcher keeps beside it.
pub struct Run {
    pub play: Box<dyn Play>,
    /// The damage the car carries (none: new).
    pub damage: Option<CarriedDamage>,
    /// The service's late penalty, added to the next stage's result.
    pub penalty: f32,
    /// The tyre on the car.
    pub tyre: u32,
}

pub struct EventsPage {
    pub model: &'static Model,
    pub view: View,
    pub builder: Option<Builder>,
    /// The builder's settings.
    pub config: Option<EventConfig>,
    /// The builder's car (the catalog's cars).
    pub car: Option<usize>,
    pub run: Option<Run>,
    /// The installed stages, read once the tab opens.
    pub pool: Option<Pool>,
    /// The profile's saved event, read once per profile name: its summary, or why it does not read.
    saved: Option<(String, Option<Result<String, String>>)>,
    pub park: Option<ServicePark>,
    strings: Option<Strings>,
    rand: CrtRand,
    /// The tyre and setup picked for the next stage.
    tyre: Option<u32>,
    setup: SeasonSetup,
    /// A stage is being driven.
    driving: bool,
    pub status: String,
}

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

/// `rbr-standalone --championship-damage`'s word for the event's damage.
fn damage_word(damage: Damage) -> &'static str {
    match damage {
        Damage::Off => "off",
        Damage::Reduced => "reduced",
        Damage::Realistic => "realistic",
    }
}

/// `hours` as `H:MM`.
pub fn clock_hm(hours: f32) -> String {
    let minutes = (hours * 60.0).round() as u32 % (24 * 60);
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

impl EventsPage {
    pub fn new(data: &Data) -> EventsPage {
        EventsPage {
            model: &model::REAL,
            view: View::Entry,
            builder: None,
            config: None,
            car: None,
            run: None,
            pool: None,
            saved: None,
            park: None,
            strings: rbr_formats::strings::english(data).ok(),
            rand: CrtRand(1),
            tyre: None,
            setup: SeasonSetup::Current,
            driving: false,
            status: String::new(),
        }
    }

    /// The profile's event files: the save, the carried damage and the launcher's state.
    pub fn files(here: &Path, profile: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = SaveDir::new(here).path;
        let file = |ext: &str| dir.join(format!("event-{profile}.{ext}"));
        (file("txt"), file("dmg"), file("ini"))
    }

    /// The stage's run files: the damage carried in, the damage and the result out.
    fn stage_files(here: &Path) -> (PathBuf, PathBuf, PathBuf) {
        let dir = here.join("events");
        (dir.join("carried.dmg"), dir.join("stage.dmg"), dir.join("stage.txt"))
    }

    /// The profile's saved event's summary, read once per profile name; none without one.
    fn saved_event(&mut self, here: &Path, profile: &str) -> Option<Result<String, String>> {
        if self.saved.as_ref().is_none_or(|(name, _)| name != profile) {
            let (save, _, _) = Self::files(here, profile);
            let found = is_file_name_part(profile).then(|| std::fs::read_to_string(&save).ok()).flatten();
            let summary = found.map(|text| (self.model.load)(&text).map(|play| place_line(play.as_ref())));
            self.saved = Some((profile.to_owned(), summary));
        }
        self.saved.as_ref().and_then(|(_, s)| s.clone())
    }

    /// `--screenshot`: the entry with or without a saved event.
    pub fn screenshot_saved(&mut self, profile: &str, saved: Option<String>) {
        (self.run, self.view, self.park) = (None, View::Entry, None);
        self.saved = Some((profile.to_owned(), saved.map(Ok)));
    }

    /// `--screenshot`: `play` under way, unsaved.
    pub fn screenshot_event(&mut self, play: Box<dyn Play>) {
        let tyre = 3;
        self.run = Some(Run { play, damage: None, penalty: 0.0, tyre });
        (self.view, self.park, self.tyre, self.driving) = (View::Event, None, None, false);
    }

    /// The builder on `preset`'s settings for the car.
    pub fn open_builder(&mut self, preset: Preset, cars: &[Car], car: usize, seed: u32) {
        let car = self.car.filter(|&i| i < cars.len()).unwrap_or(car);
        let picked = cars.get(car).cloned().unwrap_or_default();
        self.car = Some(car);
        self.config = Some((self.model.config)(preset, &class_of(&picked), &picked.key, seed));
        self.builder = Some(Builder { preset });
        self.view = View::Builder;
    }

    /// The builder's car is now `index`: the class follows it.
    pub fn pick_car(&mut self, index: usize, cars: &[Car]) {
        self.car = Some(index);
        if let (Some(config), Some(car)) = (self.config.as_mut(), cars.get(index)) {
            config.car = car.key.clone();
            config.class = class_of(car);
        }
    }

    /// Start event: the event built on the pool, saved under the profile, at its welcome.
    pub fn start_event(&mut self, here: &Path, profile: &str, cars: &[Car], stages: &[StageRef]) {
        let Some(config) = self.config.clone() else { return };
        let field = class_cars(cars, &config.class);
        let cache = here.join(pool::CACHE);
        let class = config.class.clone();
        let reference = |s: &StageRef, wet: f32| pool::reference(&cache, s, wet, &class);
        match (self.model.build)(config, stages, &field, &reference) {
            Ok(play) => {
                self.run = Some(Run { play, damage: None, penalty: 0.0, tyre: 3 });
                (self.view, self.tyre, self.setup, self.park) = (View::Event, None, SeasonSetup::Current, None);
                self.status.clear();
                self.save(here, profile);
            }
            Err(e) => self.status = format!("The event cannot be built: {e}"),
        }
    }

    /// Continue event: the profile's saved event, its carried damage and state.
    pub fn continue_event(&mut self, here: &Path, profile: &str) {
        let (save, dmg, state) = Self::files(here, profile);
        let loaded = std::fs::read_to_string(&save).map_err(|e| format!("{}: {e}", save.display()));
        match loaded.and_then(|text| (self.model.load)(&text)) {
            Ok(play) => {
                let damage = std::fs::read(&dmg).ok().and_then(|b| CarriedDamage::from_bytes(&b));
                let state = std::fs::read_to_string(&state).unwrap_or_default();
                let value = |key: &str| {
                    state.lines().find_map(|l| l.strip_prefix(key)?.trim_start().strip_prefix('=')?.trim().parse().ok())
                };
                let (penalty, tyre) = (value("penalty").unwrap_or(0.0), value("tyre").map_or(3, |t: f32| t as u32));
                self.run = Some(Run { play, damage, penalty, tyre });
                (self.view, self.tyre, self.setup, self.park) = (View::Event, None, SeasonSetup::Current, None);
                self.status.clear();
            }
            Err(e) => self.status = format!("The saved event does not read: {e}"),
        }
    }

    /// Writes the event, its carried damage and state under the profile.
    fn save(&mut self, here: &Path, profile: &str) {
        let Some(run) = &self.run else { return };
        let (save, dmg, state) = Self::files(here, profile);
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(save.parent().unwrap_or(here))?;
            std::fs::write(&save, run.play.save())?;
            match &run.damage {
                Some(damage) => std::fs::write(&dmg, damage.to_bytes())?,
                None => _ = std::fs::remove_file(&dmg),
            }
            std::fs::write(&state, format!("penalty = {}\ntyre = {}\n", run.penalty, run.tyre))
        };
        if let Err(e) = write() {
            self.status = format!("{}: {e}", save.display());
        }
        self.saved = Some((profile.to_owned(), Some(Ok(place_line(run.play.as_ref())))));
    }

    /// The event over and left: its files removed, back to the presets.
    pub fn leave_event(&mut self, here: &Path, profile: &str) {
        for file in <[PathBuf; 3]>::from(Self::files(here, profile)) {
            let _ = std::fs::remove_file(file);
        }
        (self.run, self.view, self.park, self.saved) = (None, View::Entry, None, None);
    }

    /// The itinerary stage of the turn, if it is a stage.
    pub fn next_stage(&self) -> Option<(usize, usize, &ItineraryStage)> {
        let run = self.run.as_ref()?;
        let Turn::Stage { rally, stage } = run.play.event().turn else { return None };
        Some((rally, stage, run.play.event().rallies.get(rally)?.stages.get(stage)?))
    }

    /// Whether the tyres and setup can change before stage `stage`: at the rally's start and after a service.
    pub fn choice_before(&self, rally: usize, stage: usize) -> bool {
        let Some(run) = &self.run else { return true };
        stage == 0 || run.play.event().rallies[rally].stages.get(stage - 1).is_some_and(|s| s.service_after.is_some())
    }

    /// The tyre the next stage runs on: the pick (the road's by default) when it can change, else the car's.
    pub fn next_tyre(&self) -> u32 {
        let Some((rally, stage, next)) = self.next_stage() else { return 3 };
        let last = self.run.as_ref().map_or(3, |r| r.tyre);
        if !self.choice_before(rally, stage) {
            return last;
        }
        let kind = surface_kind(next.stage.surface);
        let choices = rbr_championship::tyre_choices(kind);
        let road = rbr_gameplay::weather::tyre_kind(kind, wetness(next.conditions).1);
        self.tyre.or(road).filter(|t| choices.contains(t)).unwrap_or(choices[0])
    }

    /// Drive the next stage: the damage files, the championship flags on the event's damage, what follows it, and
    /// the tyre and setup.
    pub fn stage_start(&mut self, here: &Path) -> Result<EventStart, String> {
        let (rally, stage, next) = self.next_stage().ok_or("no stage is next")?;
        let next = next.clone();
        let run = self.run.as_ref().ok_or("no event")?;
        let event = run.play.event();
        let last = stage + 1 == event.rallies[rally].stages.len();
        let follows = match next.service_after {
            _ if last => "end",
            Some(_) => "service",
            None => "stage",
        };
        let (carried, out, result) = Self::stage_files(here);
        let dir = carried.parent().unwrap_or(here);
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for path in [&carried, &out, &result] {
            let _ = std::fs::remove_file(path);
        }
        let path = |p: &Path| p.display().to_string();
        let mut args: Vec<String> = [
            "--championship",
            "professional",
            "--championship-damage",
            damage_word(event.config.damage),
            "--rally-next",
            follows,
        ]
        .map(str::to_owned)
        .to_vec();
        if let Some(damage) = &run.damage {
            std::fs::write(&carried, damage.to_bytes()).map_err(|e| format!("{}: {e}", carried.display()))?;
            args.extend(["--damage-in".to_owned(), path(&carried)]);
        }
        args.extend(["--damage-out".to_owned(), path(&out), "--result-out".to_owned(), path(&result)]);
        args.extend(["--start-time".to_owned(), clock_hm(next.clock)]);
        let tyre = self.next_tyre();
        let setup = if self.choice_before(rally, stage) { self.setup.clone() } else { SeasonSetup::Current };
        let map = next.stage.key.parse().map_err(|_| format!("{}: not a map number", next.stage.key))?;
        self.driving = true;
        Ok(EventStart { map, conditions: next.conditions, clock: next.clock, tyre, setup, args })
    }

    /// The stage never started: nothing changed, the player can retry.
    pub fn abort_stage_start(&mut self) {
        self.driving = false;
    }

    /// The viewer closed: the stage's result and damage from `rbr-standalone`'s files (a stage left before the
    /// finish, or a run without the files, is a retire), with the service's penalty, against the stage's reference;
    /// the event saved.
    pub fn stage_closed(&mut self, here: &Path, profile: &str) {
        if !std::mem::take(&mut self.driving) {
            return;
        }
        let Some((_, _, next)) = self.next_stage() else { return };
        let next = next.clone();
        let tyre = self.next_tyre();
        let Some(run) = self.run.as_mut() else { return };
        let (_, out, result) = Self::stage_files(here);
        let left = StageEnd { cause: EndCause::Left, end_race: EndRace { time: 0.0, splits: [0.0; 2], retired: true } };
        let report = std::fs::read_to_string(&result)
            .map_or(StageReport { end: left, drivable: true, false_start: false }, |t| StageReport::parse(&t));
        let damage = std::fs::read(&out).ok().and_then(|b| CarriedDamage::from_bytes(&b));
        run.damage = damage.or(run.damage.take());
        run.tyre = tyre;
        let race = report.end.end_race;
        let player =
            PlayerResult { time: race.time, penalties: run.penalty, retired: race.retired || !report.drivable };
        run.penalty = 0.0;
        let class = run.play.event().config.class.clone();
        let reference = pool::reference(&here.join(pool::CACHE), &next.stage, wetness(next.conditions).0, &class);
        run.play.stage_done(player, reference);
        self.tyre = None;
        self.save(here, profile);
    }

    /// The welcome, a service's end or the rally's end: the turn moves on (a rally starts on a new car).
    pub fn turn_done(&mut self, here: &Path, profile: &str) {
        let Some(run) = self.run.as_mut() else { return };
        if let Turn::Welcome { .. } = run.play.event().turn {
            (run.damage, run.penalty) = (None, 0.0);
        }
        run.play.turn_done();
        (self.tyre, self.setup) = (None, SeasonSetup::Current);
        self.save(here, profile);
    }

    /// The service closed with its late penalty and the car as repaired.
    pub fn service_done(&mut self, here: &Path, profile: &str, penalty: f32, damage: CarriedDamage) {
        if let Some(run) = self.run.as_mut() {
            run.penalty += penalty;
            run.damage = Some(damage);
        }
        self.park = None;
        self.turn_done(here, profile);
    }

    /// Opens the park for the service up next, `minutes` long, on the car's repair tree.
    pub fn open_park(&mut self, data: &Data, car: &Car, ngp: Option<&Path>, minutes: u32) {
        let (parts, rules) = crate::rally_page::repair_tree(data, car, ngp);
        match parts {
            Ok(parts) => {
                let damage = self.run.as_ref().and_then(|r| r.damage.clone()).unwrap_or_default();
                let kind = if minutes >= 40 { ServiceKind::Long } else { ServiceKind::Short };
                let mut park = ServicePark::open(kind, &parts, Crew::default(), rules, &mut self.rand, damage);
                if park.state == rbr_championship::ParkState::Choosing {
                    park.time_left = minutes as f32 * 60.0;
                }
                self.park = Some(park);
            }
            Err(e) => self.status = format!("repairs: {e}"),
        }
    }

    /// The event is at a service (the park's sound plays).
    pub fn in_service(&self) -> bool {
        self.view == View::Event && self.park.is_some()
    }
}

/// Where the event stands, for the Continue card: "Season · Rally 2 of 6 · Stage 3 of 6" or "Event over".
pub fn place_line(play: &dyn Play) -> String {
    let event = play.event();
    let kind = match event.config.length {
        Length::Rally { .. } => "Rally",
        Length::Weekend { .. } => "Weekend",
        Length::Season { .. } => "Season",
    };
    let mut parts = vec![kind.to_owned()];
    let rally = match event.turn {
        Turn::EventOver => return format!("{kind} · over"),
        Turn::Welcome { rally }
        | Turn::Stage { rally, .. }
        | Turn::Service { rally, .. }
        | Turn::RallyOver { rally } => rally,
    };
    if event.rallies.len() > 1 {
        parts.push(format!("Rally {} of {}", rally + 1, event.rallies.len()));
    }
    if let Some(r) = event.rallies.get(rally) {
        parts.push(r.name.clone());
    }
    parts.push(view::step(event));
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbr_season::config::{Daylight, Weather};

    fn config() -> EventConfig {
        EventConfig {
            class: "Rally2".into(),
            car: "rsf-1".into(),
            length: Length::Rally { stages: 6 },
            pool: StagePool::All,
            surfaces: Surfaces::Only(Surface::Gravel),
            daylight: Daylight::Day,
            weather: Weather::Changeable,
            damage: Damage::Realistic,
            difficulty: 0.6,
            adaptive: true,
            field_size: 12,
            seed: 1,
        }
    }

    fn stage(key: &str, surface: Surface, community: bool, length: f32) -> StageRef {
        StageRef { key: key.into(), name: key.into(), surface, community, length }
    }

    #[test]
    fn the_summary_reads_the_event_at_a_glance() {
        let stages = [
            stage("1", Surface::Gravel, false, 10_000.0),
            stage("2", Surface::Gravel, true, 14_000.0),
            stage("3", Surface::Tarmac, false, 30_000.0),
        ];
        assert_eq!(summary(&config(), Some(&stages)), "6 stages · 72 km · gravel · 12 Rally2 drivers");
        assert_eq!(summary(&config(), None), "6 stages · gravel · 12 Rally2 drivers");
        let retail = EventConfig { pool: StagePool::Retail, field_size: 1, ..config() };
        assert_eq!(summary(&retail, Some(&stages)), "6 stages · 60 km · gravel · 1 Rally2 driver");
        let season = EventConfig {
            length: Length::Season { rallies: 2, stages: 3 },
            surfaces: Surfaces::Mixed,
            class: String::new(),
            ..config()
        };
        assert_eq!(summary(&season, Some(&stages)), "2 rallies · 6 stages · 108 km · mixed surfaces · 12 drivers");
        // No stage fits: no length.
        let snow = EventConfig { surfaces: Surfaces::Only(Surface::Snow), ..config() };
        assert_eq!(summary(&snow, Some(&stages)), "6 stages · snow · 12 Rally2 drivers");
        assert_eq!([0.0, 0.3, 0.6, 1.0].map(difficulty_word), ["Gentle", "Club", "National", "Pro"]);
        assert_eq!(clock_hm(20.5), "20:30");
    }

    /// A page on the double with a pool of the fixture's stages.
    fn page(here: &Path) -> EventsPage {
        let mut page = EventsPage::new(&Data::new(here.join("data"), None));
        page.model = &fixture::DOUBLE;
        let cars = [Car {
            key: "rsf-1".into(),
            group: Some("Rally2".into()),
            model: Ok(rbr_formats::car_pack::Model {
                entry: rbr_formats::car_pack::model_entry("a", "a"),
                fallback: None,
            }),
            ..Car::default()
        }];
        page.open_builder(Preset::QuickRally, &cars, 0, 3);
        page.start_event(here, "tester", &cars, &fixture::stages());
        page
    }

    /// Drives the next stage: `rbr-standalone`'s files as it leaves them.
    fn drive(page: &mut EventsPage, here: &Path, time: f32, damage: Option<&CarriedDamage>) -> EventStart {
        let start = page.stage_start(here).unwrap();
        let (_, out, result) = EventsPage::stage_files(here);
        let race = EndRace { time, splits: [time / 3.0, time * 2.0 / 3.0], retired: false };
        let report = StageReport {
            end: StageEnd { cause: EndCause::Finish, end_race: race },
            drivable: true,
            false_start: false,
        };
        std::fs::write(&result, report.to_text()).unwrap();
        if let Some(damage) = damage {
            std::fs::write(&out, damage.to_bytes()).unwrap();
        }
        page.stage_closed(here, "tester");
        start
    }

    #[test]
    fn a_rally_runs_its_turns_with_service_damage_and_the_late_penalty() {
        let here = crate::test_dir("events-turns");
        let mut page = page(&here);
        let turn = |page: &EventsPage| page.run.as_ref().unwrap().play.event().turn.clone();
        assert_eq!(turn(&page), Turn::Welcome { rally: 0 });
        let (save, dmg, state) = EventsPage::files(&here, "tester");
        assert!(save.is_file(), "saved at the welcome");
        page.turn_done(&here, "tester");
        // SS1: the road's tyre, the itinerary's clock and the flags; no damage carried yet.
        let start = drive(&mut page, &here, 300.0, None);
        assert_eq!((start.map, start.conditions, start.tyre), (20, Conditions::Dry, 3));
        let flag = |name: &str| start.args.iter().position(|a| a == name).map(|i| start.args[i + 1].as_str());
        assert_eq!(flag("--start-time"), Some("8:30"));
        assert_eq!(flag("--championship-damage"), Some("realistic"));
        assert_eq!(flag("--rally-next"), Some("stage"));
        assert_eq!(flag("--damage-in"), None);
        assert_eq!(turn(&page), Turn::Stage { rally: 0, stage: 1 });
        // SS2 leaves damage and a service follows.
        let mut damage = CarriedDamage::default();
        damage.service_refresh();
        let start = drive(&mut page, &here, 310.0, Some(&damage));
        assert!(start.args.iter().any(|a| a == "service"));
        assert_eq!(turn(&page), Turn::Service { rally: 0, stage: 1, minutes: 20 });
        assert!(dmg.is_file());
        page.service_done(&here, "tester", 20.0, damage.clone());
        assert_eq!(turn(&page), Turn::Stage { rally: 0, stage: 2 });
        assert!(std::fs::read_to_string(&state).unwrap().contains("penalty = 20"));
        // SS3 takes the damage in and the penalty into its result.
        let start = drive(&mut page, &here, 320.0, None);
        assert!(start.args.iter().any(|a| a == "--damage-in"));
        let event = page.run.as_ref().unwrap().play.event();
        assert_eq!(event.results[0][2].0.unwrap().penalties, 20.0);
        assert_eq!(page.run.as_ref().unwrap().penalty, 0.0);
        // A stage left before the finish retires: the rally is over, its standings' player line has no total.
        page.stage_start(&here).unwrap();
        page.stage_closed(&here, "tester");
        assert_eq!(turn(&page), Turn::RallyOver { rally: 0 });
        let lines = page.run.as_ref().unwrap().play.rally_standings(0);
        assert_eq!(lines.iter().find(|l| l.entrant.is_none()).unwrap().total, None);
        page.turn_done(&here, "tester");
        assert_eq!(turn(&page), Turn::EventOver);
        // Continue reads it back with the damage; leaving the finished event removes its files.
        let mut again = EventsPage::new(&Data::new(here.join("data"), None));
        again.model = &fixture::DOUBLE;
        assert!(again.saved_event(&here, "tester").is_some());
        again.continue_event(&here, "tester");
        assert!(again.run.as_ref().is_some_and(|r| r.damage.is_some()));
        again.leave_event(&here, "tester");
        assert!(!save.exists() && !dmg.exists() && !state.exists());
        assert_eq!(again.saved_event(&here, "tester"), None);
        std::fs::remove_dir_all(&here).unwrap();
    }

    #[test]
    fn a_stage_that_never_starts_changes_nothing() {
        let here = crate::test_dir("events-abort");
        let mut page = page(&here);
        page.turn_done(&here, "tester");
        page.stage_start(&here).unwrap();
        page.abort_stage_start();
        page.stage_closed(&here, "tester");
        assert_eq!(page.run.as_ref().unwrap().play.event().turn, Turn::Stage { rally: 0, stage: 0 });
        std::fs::remove_dir_all(&here).unwrap();
    }
}
