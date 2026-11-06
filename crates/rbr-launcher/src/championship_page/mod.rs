//! The Championship tab (ours, `rbr_season`): a rally or a season the player builds from a preset, on any installed
//! stage (retail, RSF, BTB: [`pool`]), against a field in the car's class. The tab opens on the presets (Quick rally,
//! Weekend, Season, Random), with Continue first when the profile has a championship. A preset opens the builder with its settings; Start builds the
//! championship, and the page then runs its turns: the rally's welcome and itinerary, each stage (its conditions, start time,
//! tyres and setup, then Drive), the standings with gaps and the stage winner after each, the service park where the
//! itinerary has one (`common::park_card`, the damage carried), the rally's end with its points, and the championship's
//! end.
//!
//! A stage runs as on the Drive page on the stage's own weather row of the itinerary's road (dry, damp or wet; the
//! nearest row with `--wetness` when the stage has none), at the itinerary's clock (`viewer --clock`,
//! `rbr-standalone --start-time`), as a championship stage (`--championship` on the championship's skill, flow mode 3: no
//! restart) on the championship's damage. The damage carried goes in with `--damage-in`; the stage's result and damage come back
//! through `--result-out` and `--damage-out` in `championship/` in the player's folder.
//!
//! Before each rally (at its welcome, and at the end of the one before) the page offers the game's two trials as
//! practice that is not recorded: a yard trial and a stage trial on the rally's first stage (its surface and road), in
//! the championship's car, with no championship flags.
//!
//! One championship per profile: `SavedGames/championship-<profile>.txt` (`Event::save`) in `SavedGames/`, with the carried
//! damage in `championship-<profile>.dmg` and the service's late penalty still to add and the tyre on the car in
//! `championship-<profile>.ini` (the championship's API keeps neither).

use std::path::{Path, PathBuf};

use rbr_formats::ai_ini::{Master, Roster};
use rbr_formats::data::Data;
use rbr_formats::saves::{is_file_name_part, SaveDir};
use rbr_formats::strings::Strings;
use rbr_gameplay::car_damage::CarriedDamage;
use rbr_gameplay::crt::CrtRand;
use rbr_gameplay::flow::{EndCause, EndRace, StageEnd};
use rbr_gameplay::setup::SeasonSetup;
use rbr_season::config::{Damage, EventConfig, Length, Preset, StageRef, Surface};
use rbr_season::event::{Conditions, Event, ItineraryStage, PlayerResult, Turn};
use rbr_season::pace;
use rbr_season::rally::{tyre_choices, StageReport};
use rbr_season::service::{ParkState, ServiceKind, ServicePark};

use crate::catalog::Car;

mod common;
pub mod fixture;
mod launch;

pub use launch::shots;
mod builder;
mod cards;
pub mod pool;
mod results;
mod running;
mod view;
mod words;

use pool::Pool;
use words::*;

/// What the Championship tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// The presets, and Continue.
    Entry,
    Builder,
    /// The championship under way.
    Running,
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

/// The championship under way and what the launcher keeps beside it.
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
    /// The AI tables, read once (or why they do not read: the championship cannot start without them).
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
    /// The profile's saved championship, read once per profile name: its summary, or why it does not read.
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
    /// The profile the championship is saved under.
    pub profile: String,
    /// The bulletins shown in full, not the last few.
    news_all: bool,
    /// The stage whose times the result card shows, picked on the itinerary; none for the last one driven.
    shown: Option<(usize, usize)>,
    pub status: String,
}

/// The yard, the game's map for the yard trial.
const YARD: u32 = 72;

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
            shown: None,
            status: String::new(),
        }
    }

    /// The profile's championship files: the save, the carried damage and the launcher's state.
    pub fn files(here: &Path, profile: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = SaveDir::new(here).path;
        let file = |ext: &str| dir.join(format!("championship-{profile}.{ext}"));
        (file("txt"), file("dmg"), file("ini"))
    }

    /// The stage's run files: the damage carried in, the damage and the result out.
    fn stage_files(here: &Path) -> (PathBuf, PathBuf, PathBuf) {
        let dir = here.join("championship");
        (dir.join("carried.dmg"), dir.join("stage.dmg"), dir.join("stage.txt"))
    }

    /// The profile's saved championship's summary, read once per profile name; none without one.
    fn saved_event(&mut self, here: &Path, profile: &str) -> Option<Result<String, String>> {
        if self.saved.as_ref().is_none_or(|(name, _)| name != profile) {
            let (save, _, _) = Self::files(here, profile);
            let found = is_file_name_part(profile).then(|| std::fs::read_to_string(&save).ok()).flatten();
            let summary = found.map(|text| Event::load(&text).map(|event| place_line(&event)));
            self.saved = Some((profile.to_owned(), summary));
        }
        self.saved.as_ref().and_then(|(_, s)| s.clone())
    }

    /// `--screenshot`: the entry with or without a saved championship.
    pub fn screenshot_saved(&mut self, profile: &str, saved: Option<String>) {
        (self.run, self.view, self.park) = (None, View::Entry, None);
        self.saved = Some((profile.to_owned(), saved.map(Ok)));
    }

    /// `--screenshot`: `play` under way, unsaved.
    pub fn screenshot_event(&mut self, event: Event) {
        let tyre = 3;
        self.run = Some(Run { event, damage: None, penalty: 0.0, tyre });
        (self.view, self.park, self.tyre, self.driving, self.shown) = (View::Running, None, None, false, None);
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

    /// Start: the championship built on the pool, saved under the profile, at its welcome.
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
                (self.view, self.tyre, self.setup, self.park) = (View::Running, None, SeasonSetup::Current, None);
                self.shown = None;
                self.status.clear();
                self.save(here, profile);
            }
            Err(e) => self.status = format!("The championship cannot be built: {e}"),
        }
    }

    /// Continue: the profile's saved championship, its carried damage and state.
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
                (self.view, self.tyre, self.setup, self.park) = (View::Running, None, SeasonSetup::Current, None);
                self.shown = None;
                self.status.clear();
            }
            Err(e) => self.status = format!("The saved championship does not read: {e}"),
        }
    }

    /// Writes the championship, its carried damage and state under the profile.
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

    /// The championship over and left: its files removed, back to the presets.
    pub fn leave_event(&mut self, here: &Path, profile: &str) {
        for file in <[PathBuf; 3]>::from(Self::files(here, profile)) {
            let _ = std::fs::remove_file(file);
        }
        (self.run, self.view, self.park, self.saved) = (None, View::Entry, None, None);
    }

    /// The itinerary stage of the turn, if it is a stage.
    fn next_stage(&self) -> Option<(usize, usize, &ItineraryStage)> {
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

    /// Drive the next stage: the damage files, the championship flags on the championship's damage, what follows it, and
    /// the tyre and setup.
    pub fn stage_start(&mut self, here: &Path) -> Result<StageStart, String> {
        let (rally, stage, next) = self.next_stage().ok_or("no stage is next")?;
        let next = next.clone();
        let run = self.run.as_ref().ok_or("no championship")?;
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

    /// A practice trial before a rally, at its welcome or the end of the one before (the game's `S_YARD_TRIAL` on map
    /// 72, `S_STAGE_TRIAL`): the yard, or the rally's first stage on its road and clock, in the championship's car and
    /// damage, on the road's tyre and the current setup. No championship flags and no files: it is not recorded.
    pub fn trial(&mut self, yard: bool) -> Result<StageStart, String> {
        let run = self.run.as_ref().ok_or("no championship")?;
        let event = &run.event;
        let rally = match event.turn {
            Turn::Welcome { rally } => rally,
            Turn::RallyOver { rally } => rally + 1,
            _ => return Err("trials come before a rally".into()),
        };
        let next = event.rallies.get(rally).and_then(|r| r.stages.first()).ok_or("no rally follows")?;
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
    /// a car retired under Super Rally rebuilt; the championship saved.
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
        // Under Super Rally a retired car is rebuilt for its rejoin after the service.
        if player.retired && matches!(run.event.turn, Turn::Service { .. }) {
            run.damage = None;
        }
        (self.tyre, self.shown) = (None, None);
        self.save(here, profile);
    }

    /// The welcome, a service's end or the rally's end: the turn moves on (a rally starts on a new car).
    pub fn turn_done(&mut self, here: &Path, profile: &str) {
        let Some(run) = self.run.as_mut() else { return };
        if let Turn::Welcome { .. } = run.event.turn {
            (run.damage, run.penalty) = (None, 0.0);
        }
        run.event.turn_done();
        (self.tyre, self.setup, self.shown) = (None, SeasonSetup::Current, None);
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

    /// The championship is at a service (the park's sound plays).
    pub fn in_service(&self) -> bool {
        self.view == View::Running && self.park.is_some()
    }
}

/// Where the championship stands, for the Continue card: "Season · Rally 2 of 6 · Stage 3 of 6" or "Season · over".
fn place_line(event: &Event) -> String {
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
mod tests;
