//! The Championship tab (ours, `rbr_season`): a rally or a season the player builds from a preset, on any installed
//! stage (retail, RSF, BTB: [`pool`]), against a field in the car's class. The tab opens on the presets (Quick rally,
//! Weekend, Season, Random), with Continue first when the profile has an event. A preset opens the builder with its settings; Start event builds the
//! event, and the page then runs its turns: the rally's welcome and itinerary, each stage (its conditions, start time,
//! tyres and setup, then Drive), the standings with gaps and the stage winner after each, the service park where the
//! itinerary has one (`common::park_card`, the damage carried), the rally's end with its points, and the event's
//! end.
//!
//! A stage runs as on the Drive page on the stage's own weather row of the itinerary's road (dry, damp or wet; the
//! nearest row with `--wetness` when the stage has none), at the itinerary's clock (`viewer --clock`,
//! `rbr-standalone --start-time`), as a championship stage (`--championship` on the event's skill, flow mode 3: no
//! restart) on the event's damage. The damage carried goes in with `--damage-in`; the stage's result and damage come back
//! through `--result-out` and `--damage-out` in `events/` in the player's folder.
//!
//! Between rallies the page offers the game's two trials as practice that is not recorded: a yard trial and a stage
//! trial on the next rally's first stage (its surface and road), in the event's car, with no championship flags.
//!
//! One event per profile: `SavedGames/event-<profile>.txt` (`Event::save`) in `SavedGames/`, with the carried
//! damage in `event-<profile>.dmg` and the service's late penalty still to add and the tyre on the car in
//! `event-<profile>.ini` (the event's API keeps neither).

use std::path::{Path, PathBuf};

use rbr_formats::ai_ini::{Master, Roster};
use rbr_formats::data::Data;
use rbr_formats::saves::{is_file_name_part, SaveDir};
use rbr_formats::strings::Strings;
use rbr_gameplay::car_damage::CarriedDamage;
use rbr_gameplay::crt::CrtRand;
use rbr_gameplay::flow::{EndCause, EndRace, StageEnd};
use rbr_gameplay::setup::SeasonSetup;
use rbr_season::config::{Damage, EventConfig, Length, Preset, Skill, StagePool, StageRef, Surface, Surfaces};
use rbr_season::event::{Conditions, Event, ItineraryStage, PlayerResult, Turn};
use rbr_season::pace;
use rbr_season::rally::{tyre_choices, StageReport};
use rbr_season::service::{ParkState, ServiceKind, ServicePark};

use crate::catalog::Car;

mod common;
pub mod fixture;
mod launch;

pub use launch::shots;
pub mod pool;
mod view;

use pool::Pool;

/// What the Championship tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// The presets, and Continue event.
    Entry,
    Builder,
    /// The event under way.
    Event,
}

/// The preset a builder started from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Builder {
    pub preset: Preset,
}

/// A stage to drive: its map, road and clock, the tyre and setup picked, the damage and the extra `rbr-standalone`
/// arguments (none for a practice trial).
#[derive(Debug, Clone, PartialEq)]
pub struct StageStart {
    pub map: u32,
    pub conditions: Conditions,
    /// Hours, 0 to 24.
    pub clock: f32,
    pub tyre: u32,
    pub setup: SeasonSetup,
    pub damage: Damage,
    pub args: Vec<String>,
}

/// What the page asks of the launcher.
pub enum Request {
    Launch(StageStart),
    /// Open the Setup page.
    Setup,
    /// Open the car browser on the builder's car.
    BrowseCar,
}

/// The event under way and what the launcher keeps beside it.
pub struct Run {
    pub event: Event,
    /// The damage the car carries (none: new).
    pub damage: Option<CarriedDamage>,
    /// The service's late penalty, added to the next stage's result.
    pub penalty: f32,
    /// The tyre on the car.
    pub tyre: u32,
}

/// The 1.02 AI's tables the field runs on: `ai.ini` and `aidrv.ini`.
pub type Ai = (Master, Roster);

pub struct ChampionshipPage {
    /// The AI tables, read once (or why they do not read: the event cannot start without them).
    pub ai: Result<Ai, String>,
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
    /// A practice trial is being driven (it is not recorded).
    practice: bool,
    /// The profile the event is saved under.
    pub profile: String,
    /// The bulletins shown in full, not the last few.
    news_all: bool,
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

/// The yard, the game's map for the yard trial.
const YARD: u32 = 72;

/// `rbr-standalone --championship-damage`'s word for the event's damage.
fn damage_word(damage: Damage) -> &'static str {
    match damage {
        Damage::Off => "off",
        Damage::Reduced => "reduced",
        Damage::Realistic => "realistic",
    }
}

/// The game's skill level an event skill runs as (`SSKILL`: Rookie, Professional, Champion).
fn race_skill(skill: Skill) -> rbr_gameplay::race_options::Skill {
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

impl ChampionshipPage {
    pub fn new(data: &Data) -> ChampionshipPage {
        ChampionshipPage {
            ai: rbr_season::ai::load_ai(data),
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
            practice: false,
            profile: "player".into(),
            news_all: false,
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
            let summary = found.map(|text| Event::load(&text).map(|event| place_line(&event)));
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
    pub fn screenshot_event(&mut self, event: Event) {
        let tyre = 3;
        self.run = Some(Run { event, damage: None, penalty: 0.0, tyre });
        (self.view, self.park, self.tyre, self.driving) = (View::Event, None, None, false);
    }

    /// The builder on `preset`'s settings for the car.
    pub fn open_builder(&mut self, preset: Preset, cars: &[Car], car: usize, seed: u32) {
        let car = self.car.filter(|&i| i < cars.len()).unwrap_or(car);
        let picked = cars.get(car).cloned().unwrap_or_default();
        self.car = Some(car);
        self.config = Some(preset.config(&class_of(&picked), &picked.key, seed));
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
        let class = pace::class_pace(&config.class);
        let stage_profile = |s: &StageRef| pool::profile(&cache, s, &class);
        let built = match &self.ai {
            Ok((_, roster)) => Event::build(config, stages, &field, roster, &stage_profile),
            Err(e) => Err(format!("the AI tables do not read: {e}")),
        };
        match built {
            Ok(event) => {
                self.run = Some(Run { event, damage: None, penalty: 0.0, tyre: 3 });
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
        match loaded.and_then(|text| Event::load(&text)) {
            Ok(event) => {
                let damage = std::fs::read(&dmg).ok().and_then(|b| CarriedDamage::from_bytes(&b));
                let state = std::fs::read_to_string(&state).unwrap_or_default();
                let value = |key: &str| {
                    state.lines().find_map(|l| l.strip_prefix(key)?.trim_start().strip_prefix('=')?.trim().parse().ok())
                };
                let (penalty, tyre) = (value("penalty").unwrap_or(0.0), value("tyre").map_or(3, |t: f32| t as u32));
                self.run = Some(Run { event, damage, penalty, tyre });
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
            std::fs::write(&save, run.event.save())?;
            match &run.damage {
                Some(damage) => std::fs::write(&dmg, damage.to_bytes())?,
                None => _ = std::fs::remove_file(&dmg),
            }
            std::fs::write(&state, format!("penalty = {}\ntyre = {}\n", run.penalty, run.tyre))
        };
        if let Err(e) = write() {
            self.status = format!("{}: {e}", save.display());
        }
        self.saved = Some((profile.to_owned(), Some(Ok(place_line(&run.event)))));
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
        let Turn::Stage { rally, stage } = run.event.turn else { return None };
        Some((rally, stage, run.event.rallies.get(rally)?.stages.get(stage)?))
    }

    /// Whether the tyres and setup can change before stage `stage`: at the rally's start and after a service.
    pub fn choice_before(&self, rally: usize, stage: usize) -> bool {
        let Some(run) = &self.run else { return true };
        stage == 0 || run.event.rallies[rally].stages.get(stage - 1).is_some_and(|s| s.service_after.is_some())
    }

    /// The tyre the next stage runs on: the pick (the road's by default) when it can change, else the car's.
    pub fn next_tyre(&self) -> u32 {
        let Some((rally, stage, next)) = self.next_stage() else { return 3 };
        let last = self.run.as_ref().map_or(3, |r| r.tyre);
        if !self.choice_before(rally, stage) {
            return last;
        }
        let kind = surface_kind(next.stage.surface);
        let choices = tyre_choices(kind);
        let road = rbr_gameplay::weather::tyre_kind(kind, wetness(next.conditions).1);
        self.tyre.or(road).filter(|t| choices.contains(t)).unwrap_or(choices[0])
    }

    /// Drive the next stage: the damage files, the championship flags on the event's damage, what follows it, and
    /// the tyre and setup.
    pub fn stage_start(&mut self, here: &Path) -> Result<StageStart, String> {
        let (rally, stage, next) = self.next_stage().ok_or("no stage is next")?;
        let next = next.clone();
        let run = self.run.as_ref().ok_or("no event")?;
        let event = &run.event;
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
            race_skill(event.config.skill).name(),
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
        let damage = event.config.damage;
        Ok(StageStart { map, conditions: next.conditions, clock: next.clock, tyre, setup, damage, args })
    }

    /// The stage never started: nothing changed, the player can retry.
    pub fn abort_stage_start(&mut self) {
        (self.driving, self.practice) = (false, false);
    }

    /// A practice trial between rallies (the game's `S_YARD_TRIAL` on map 72, `S_STAGE_TRIAL`): the yard, or the next
    /// rally's first stage on its road and clock, in the event's car and damage, on the road's tyre and the current
    /// setup. No championship flags and no files: it is not recorded.
    pub fn trial(&mut self, yard: bool) -> Result<StageStart, String> {
        let run = self.run.as_ref().ok_or("no event")?;
        let event = &run.event;
        let Turn::RallyOver { rally } = event.turn else { return Err("trials come between rallies".into()) };
        let next = event.rallies.get(rally + 1).and_then(|r| r.stages.first()).ok_or("no rally follows")?;
        let (map, surface, conditions, clock) = if yard {
            (YARD, Surface::Gravel, Conditions::Dry, 12.0)
        } else {
            let map = next.stage.key.parse().map_err(|_| format!("{}: not a map number", next.stage.key))?;
            (map, next.stage.surface, next.conditions, next.clock)
        };
        let kind = surface_kind(surface);
        let road = rbr_gameplay::weather::tyre_kind(kind, wetness(conditions).1);
        let choices = tyre_choices(kind);
        let tyre = road.filter(|t| choices.contains(t)).unwrap_or(choices[0]);
        let damage = event.config.damage;
        (self.driving, self.practice) = (true, true);
        Ok(StageStart { map, conditions, clock, tyre, setup: SeasonSetup::Current, damage, args: Vec::new() })
    }

    /// The viewer closed: the stage's result and damage from `rbr-standalone`'s files (a stage left before the
    /// finish, or a run without the files, is a retire), with the service's penalty, against the stage's reference;
    /// the event saved.
    pub fn stage_closed(&mut self, here: &Path, profile: &str) {
        if !std::mem::take(&mut self.driving) || std::mem::take(&mut self.practice) {
            return;
        }
        if self.next_stage().is_none() {
            return;
        }
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
        match &self.ai {
            Ok((master, _)) => run.event.stage_done(master, player),
            Err(e) => {
                self.status = format!("the AI tables do not read: {e}");
                return;
            }
        }
        self.tyre = None;
        self.save(here, profile);
    }

    /// The welcome, a service's end or the rally's end: the turn moves on (a rally starts on a new car).
    pub fn turn_done(&mut self, here: &Path, profile: &str) {
        let Some(run) = self.run.as_mut() else { return };
        if let Turn::Welcome { .. } = run.event.turn {
            (run.damage, run.penalty) = (None, 0.0);
        }
        run.event.turn_done();
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
        let (parts, rules) = common::repair_tree(data, car, ngp);
        match parts {
            Ok(parts) => {
                let damage = self.run.as_ref().and_then(|r| r.damage.clone()).unwrap_or_default();
                let kind = if minutes >= 40 { ServiceKind::Long } else { ServiceKind::Short };
                let crew = self.run.as_ref().map_or_else(Default::default, |r| r.event.crew());
                let mut park = ServicePark::open(kind, &parts, crew, rules, &mut self.rand, damage);
                if park.state == ParkState::Choosing {
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
pub fn place_line(event: &Event) -> String {
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
            team: 0,
            skill: Skill::Professional,
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

    /// A page on the fixture AI with a pool of the fixture's stages.
    fn page(here: &Path) -> ChampionshipPage {
        let mut page = ChampionshipPage::new(&Data::new(here.join("data"), None));
        page.ai = Ok(fixture::ai());
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
    fn drive(page: &mut ChampionshipPage, here: &Path, time: f32, damage: Option<&CarriedDamage>) -> StageStart {
        let start = page.stage_start(here).unwrap();
        let (_, out, result) = ChampionshipPage::stage_files(here);
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
        let turn = |page: &ChampionshipPage| page.run.as_ref().unwrap().event.turn.clone();
        assert_eq!(turn(&page), Turn::Welcome { rally: 0 });
        let (save, dmg, state) = ChampionshipPage::files(&here, "tester");
        assert!(save.is_file(), "saved at the welcome");
        page.turn_done(&here, "tester");
        // SS1: the road's tyre, the itinerary's clock and the flags; no damage carried yet.
        let start = drive(&mut page, &here, 300.0, None);
        let first = page.run.as_ref().unwrap().event.rallies[0].stages[0].clone();
        assert_eq!((start.map.to_string(), start.conditions), (first.stage.key.clone(), first.conditions));
        assert_eq!(start.tyre, rbr_gameplay::weather::tyre_kind(0, wetness(first.conditions).1).unwrap_or(3));
        let flag = |name: &str| start.args.iter().position(|a| a == name).map(|i| start.args[i + 1].as_str());
        assert_eq!(flag("--start-time"), Some(clock_hm(first.clock).as_str()));
        assert_eq!(flag("--championship-damage"), Some("realistic"));
        assert_eq!(flag("--rally-next"), Some("stage"));
        assert_eq!(flag("--damage-in"), None);
        // The fixture's Professional skill is the game's Professional.
        assert_eq!(flag("--championship"), Some("professional"));
        assert_eq!(turn(&page), Turn::Stage { rally: 0, stage: 1 });
        drive(&mut page, &here, 305.0, None);
        // SS3 leaves damage and a service follows.
        let mut damage = CarriedDamage::default();
        damage.service_refresh();
        let start = drive(&mut page, &here, 310.0, Some(&damage));
        assert!(start.args.iter().any(|a| a == "service"));
        assert_eq!(turn(&page), Turn::Service { rally: 0, stage: 2, minutes: 20 });
        assert!(dmg.is_file());
        page.service_done(&here, "tester", 20.0, damage.clone());
        assert_eq!(turn(&page), Turn::Stage { rally: 0, stage: 3 });
        assert!(std::fs::read_to_string(&state).unwrap().contains("penalty = 20"));
        // SS4 takes the damage in and the penalty into its result.
        let start = drive(&mut page, &here, 320.0, None);
        assert!(start.args.iter().any(|a| a == "--damage-in"));
        let event = &page.run.as_ref().unwrap().event;
        assert_eq!(event.results[0][3].0.unwrap().penalties, 20.0);
        assert_eq!(page.run.as_ref().unwrap().penalty, 0.0);
        // A stage left before the finish retires: the rally is over, its standings' player line has no total.
        page.stage_start(&here).unwrap();
        page.stage_closed(&here, "tester");
        assert_eq!(turn(&page), Turn::RallyOver { rally: 0 });
        let lines = page.run.as_ref().unwrap().event.rally_standings(0);
        assert_eq!(lines.iter().find(|l| l.entrant.is_none()).unwrap().total, None);
        page.turn_done(&here, "tester");
        assert_eq!(turn(&page), Turn::EventOver);
        // Continue reads it back with the damage; leaving the finished event removes its files.
        let mut again = ChampionshipPage::new(&Data::new(here.join("data"), None));
        again.ai = Ok(fixture::ai());
        assert!(again.saved_event(&here, "tester").is_some());
        again.continue_event(&here, "tester");
        assert!(again.run.as_ref().is_some_and(|r| r.damage.is_some()));
        again.leave_event(&here, "tester");
        assert!(!save.exists() && !dmg.exists() && !state.exists());
        assert_eq!(again.saved_event(&here, "tester"), None);
        std::fs::remove_dir_all(&here).unwrap();
    }

    #[test]
    fn a_trial_between_rallies_is_practice_that_is_not_recorded() {
        let here = crate::test_dir("events-trial");
        let mut page = page(&here);
        let mut event = fixture::fixture_event();
        page.screenshot_event(event.clone());
        assert!(page.trial(false).is_err(), "trials come between rallies");
        event.turn = Turn::RallyOver { rally: 0 };
        page.screenshot_event(event.clone());
        let stage = page.trial(false).unwrap();
        assert_eq!(stage.map.to_string(), event.rallies[1].stages[0].stage.key);
        assert!(stage.args.is_empty(), "no championship flags, no files");
        // Its end changes nothing: the turn stays, nothing is recorded.
        page.stage_closed(&here, "tester");
        assert_eq!(page.run.as_ref().unwrap().event.turn, Turn::RallyOver { rally: 0 });
        assert!(page.run.as_ref().unwrap().event.results[0].iter().all(|(player, _)| player.is_none()));
        assert_eq!(page.trial(true).unwrap().map, YARD);
        page.abort_stage_start();
        assert!(!page.driving && !page.practice);
        std::fs::remove_dir_all(&here).unwrap();
    }

    #[test]
    fn the_skill_levels_are_the_games_three_with_their_help_rules() {
        assert_eq!(assistance_note(Skill::Rookie), "10 s penalty");
        assert_eq!(assistance_note(Skill::Professional), "20 s penalty");
        assert!(assistance_note(Skill::Champion).starts_with("35 s penalty; a stuck car is not helped"));
    }

    #[test]
    fn a_stage_that_never_starts_changes_nothing() {
        let here = crate::test_dir("events-abort");
        let mut page = page(&here);
        page.turn_done(&here, "tester");
        page.stage_start(&here).unwrap();
        page.abort_stage_start();
        page.stage_closed(&here, "tester");
        assert_eq!(page.run.as_ref().unwrap().event.turn, Turn::Stage { rally: 0, stage: 0 });
        std::fs::remove_dir_all(&here).unwrap();
    }

    /// A Quick rally on the installed stages and the real `ai.ini`, played with a fixed player pace (`RBR_DATA`).
    #[test]
    fn a_quick_rally_on_the_real_stages_and_ai_plays_through() {
        use std::sync::atomic::{AtomicBool, AtomicUsize};
        let Some(data) = Data::from_env() else {
            eprintln!("skipping: RBR_DATA not set");
            return;
        };
        let install = std::env::var_os("RBR_DATA").map(PathBuf::from).unwrap();
        let season = rbr_gameplay::weather::Season::load(&data).unwrap();
        let catalog = crate::catalog::Catalog::scan(&install).unwrap();
        let here = crate::test_dir("events-real");
        let cache = here.join(pool::CACHE);
        let (done, stop) = (AtomicUsize::new(0), AtomicBool::new(false));
        let stages = pool::read(&data, &cache, pool::items(&catalog, &season), &done, &stop);
        assert!(stages.len() >= 20, "{} stages read", stages.len());
        let (master, roster) = rbr_season::ai::load_ai(&data).unwrap();
        assert!(!rbr_season::team::team_offers(&roster).is_empty());

        let config = Preset::QuickRally.config("Rally2", "", 11);
        let class = pace::class_pace(&config.class);
        let profile = |s: &StageRef| pool::profile(&cache, s, &class);
        let cars = ["Skoda Fabia Rally2".to_owned(), "Citroen C3 Rally2".to_owned()];
        let mut event = Event::build(config, &stages, &cars, &roster, &profile).unwrap();
        let field = event.field.len();
        assert_eq!(event.rallies[0].stages.len(), 6);
        event.turn_done();
        let mut player = 0.0;
        while let Turn::Stage { rally, stage } = event.turn {
            let order = event.start_order(rally, stage);
            assert_eq!(order.len(), field + 1);
            assert_eq!(order.iter().filter(|who| who.is_none()).count(), 1);
            assert!(event.road_wear(rally, stage) <= 2.0);
            let reference = event.rallies[rally].stages[stage].reference;
            assert!(reference > 30.0, "SS{} reference {reference}", stage + 1);
            let time = reference * 1.08;
            player += time;
            event.stage_done(&master, PlayerResult { time, penalties: 0.0, retired: false });
            if let Turn::Service { .. } = event.turn {
                event.turn_done();
            }
        }
        assert_eq!(event.turn, Turn::RallyOver { rally: 0 });
        let standings = event.rally_standings(0);
        assert_eq!(standings.len(), field + 1);
        let place = standings.iter().position(|row| row.entrant.is_none()).unwrap();
        let total = standings[place].total.expect("the player finished");
        assert!((total - player).abs() < 0.5, "{total} against {player}");
        // A 8 % slower player is not winning against a field of the class's pace, and is not last.
        assert!(place > 0 && place < field, "the player is {} of {}", place + 1, field + 1);
        let totals: Vec<f32> = standings.iter().filter_map(|row| row.total).collect();
        assert!(totals.windows(2).all(|w| w[0] <= w[1]), "sorted by total");
        assert!(standings[0].points > 0 && standings[0].points >= standings.last().unwrap().points);
        let news = event.bulletins(0);
        assert!(!news.is_empty() && news.iter().all(|line| !line.trim().is_empty()), "{news:?}");
        assert_eq!(Event::load(&event.save()).unwrap(), event);
        std::fs::remove_dir_all(&here).unwrap();
    }
}
