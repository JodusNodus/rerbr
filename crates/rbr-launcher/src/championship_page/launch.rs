//! The championship in the launcher: the Championship tab's page (`Form::championship_page`) and a stage's launch
//! (`Form::start_championship_stage`), the way the Drive page starts any stage, on the itinerary's road and clock.

use std::path::Path;

use rbr_formats::saves::SaveDir;
use rbr_gameplay::car_damage::CarriedDamage;
use rbr_gameplay::race_options::{DamageLevel, RaceOptions};
use rbr_gameplay::weather::{Draws, Preset, Weather};
use rbr_season::config::Damage;
use rbr_season::event::{Conditions, Turn};

use super::view::Env;
use super::{wetness, Request, StageStart};
use crate::catalog::{self, TyreChoice};
use crate::command::clock_args;
use crate::pages::{Issue, FIELD};
use crate::settings::wetness_name;
use crate::{browser, car_browser, Form};

impl Form {
    /// The Championship tab: the presets, the builder and the championship under way. A championship drives its own
    /// car (the Drive page's car follows it).
    pub(crate) fn championship_page(&mut self, ui: &mut egui::Ui) {
        if self.no_cars_yet(ui) {
            return;
        }
        if self.car_browser.open && self.car_browser.target != car_browser::Target::Drive {
            return self.car_browser_page(ui);
        }
        if self.championship.pool.is_none() {
            let items = super::pool::items(&self.catalog, &self.season);
            let cache = self.here().join(super::pool::CACHE);
            self.championship.pool = Some(super::pool::Pool::start(self.data.clone(), cache, items));
        }
        let event_car = self.championship.run.as_ref().map(|r| r.event.config.car.clone());
        let car_missing = match &event_car {
            Some(key) => match self.catalog.cars.iter().position(|c| c.key == *key) {
                Some(i) => {
                    self.car = i;
                    false
                }
                None => true,
            },
            None => false,
        };
        // The service's park opens on the car's repair tree.
        let service = self.championship.run.as_ref().and_then(|r| match r.event.turn {
            Turn::Service { minutes, .. } => Some(minutes),
            _ => None,
        });
        match service {
            Some(minutes) if self.championship.park.is_none() && self.championship.view == super::View::Running => {
                let car = self.car().cloned().unwrap_or_default();
                let archive = self.ngp_archive();
                self.championship.open_park(&self.data, &car, Some(Path::new(&archive)), minutes);
            }
            None => self.championship.park = None,
            _ => {}
        }
        self.select_setup();
        let blocker = if car_missing {
            Some(Issue {
                text: format!("the championship's car {} is not installed", event_car.unwrap_or_default()),
                fix: None,
            })
        } else {
            self.launch_blocker()
        };
        let here = self.here();
        let mut profile = self.championship.profile.clone();
        let (rows, favourites) = (&self.car_browser, &self.favourite_cars);
        let mut car_field = |ui: &mut egui::Ui, car: usize| {
            rows.row(car).is_some_and(|row| {
                browser::field_button(ui, favourites.contains(&row.name), &row.title, &row.detail(), FIELD)
            })
        };
        let setups = &self.setup;
        let saved = |surface| setups.saved_names(surface);
        let seed = rbr_season::seed::seed_from_unix_ms(
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64),
        );
        let env = Env {
            here: &here,
            profile: &mut profile,
            cars: &self.catalog.cars,
            default_car: self.car,
            car_field: &mut car_field,
            saved: &saved,
            blocker: blocker.as_ref(),
            seed,
        };
        let request =
            egui::ScrollArea::vertical().id_salt("championship").show(ui, |ui| self.championship.ui(ui, env)).inner;
        self.championship.profile = profile;
        match request {
            Some(Request::Launch(start)) => self.start_championship_stage(start),
            Some(Request::Setup) => self.page = crate::pages::Page::Setup,
            Some(Request::BrowseCar) => {
                let car = self.championship.car.unwrap_or(self.car);
                self.car_browser.open_on(car_browser::Target::Championship, car, &self.favourite_cars, None);
            }
            None => {}
        }
    }

    /// The stage and weather a championship stage runs on: the map's weather row with the road's wetness (the nearest one
    /// otherwise), and whether the road needs `--wetness` over the row's.
    pub(crate) fn championship_weather(
        &self,
        map: u32,
        conditions: Conditions,
    ) -> Result<(catalog::Stage, Weather, bool), String> {
        let entry = self.catalog.map(map).ok_or(format!("map {map}: not installed"))?;
        let season = entry.season(&self.season);
        let want = wetness(conditions).1;
        let mut rows: Vec<usize> = season.rows_of(map).collect();
        rows.sort_by_key(|&k| season.rows[k].wetness.min(2).abs_diff(want));
        let mut last = format!("map {map} has no weather row");
        for k in rows {
            match self.catalog.stage_weather(&self.season, map, Preset::Random, Draws { row: Some(k), wear: 1 }) {
                Ok((stage, weather)) => {
                    let off = weather.wetness.min(2) != want;
                    return Ok((stage, Weather { wetness: want, wear: 1, ..weather }, off));
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// Starts a championship's stage like the Drive page's Start, on the itinerary's road and clock, the tyre and setup
    /// picked, the championship's damage and the files `stage_start` named.
    pub fn start_championship_stage(&mut self, start: StageStart) {
        let StageStart { map, conditions, clock, tyre, setup, damage, args: mut extra } = start;
        match self.championship_weather(map, conditions) {
            Ok((stage, weather, off)) => {
                self.map = self.catalog.map_index(map).unwrap_or(self.map);
                self.validate_weather_row();
                self.select_setup();
                if let Err(e) = self.setup.choose(&setup) {
                    eprintln!("launcher: setup {}: {e}; the stage keeps the current setup", setup.label());
                }
                // A championship stage is offered to the stage record and the personal best; a trial is practice.
                if extra.iter().any(|a| a == "--championship") {
                    let saves = SaveDir::new(&self.records()).path.display().to_string();
                    extra.extend([
                        "--saves".to_owned(),
                        saves,
                        "--profile".to_owned(),
                        self.championship.profile.clone(),
                    ]);
                }
                if off {
                    extra.extend(["--wetness".to_owned(), wetness_name(weather.wetness).to_owned()]);
                }
                let minutes = (clock * 60.0).round() as u32 % (24 * 60);
                let damage = match damage {
                    Damage::Off => DamageLevel::Off,
                    Damage::Reduced => DamageLevel::Reduced,
                    Damage::Realistic => DamageLevel::Realistic,
                };
                let options = RaceOptions { weather: Preset::Random, damage, ..self.options };
                let chosen = std::mem::replace(&mut self.tyre, TyreChoice::Fixed(tyre));
                if let Some(args) = self.launch_args(stage, options, weather, extra, clock_args(Some(minutes))) {
                    self.spawn(args);
                }
                self.tyre = chosen;
            }
            Err(e) => self.status = e,
        }
        if self.child.is_none() {
            self.championship.abort_stage_start();
        }
    }
}

/// `--screenshot`: the Championship tab on a championship built from the fixture's stages ([`super::fixture`]): the presets without and with a saved
/// championship, the builder, and a season's welcome, a stage, a service (on the car of `damage`, a damage file),
/// the standings later in the rally, the rally's end with its trials and the season's end.
pub fn shots(form: &mut Form, dir: &Path, damage: Option<&Path>) -> Result<(), String> {
    use super::fixture;
    use crate::capture::shot;
    use rbr_season::config::Preset;
    use rbr_season::event::PlayerResult;
    form.page = crate::pages::Page::Championship;
    form.championship.ai = Ok(fixture::ai());
    form.championship.pool = Some(super::pool::Pool::Ready(fixture::stages()));
    let profile = form.championship.profile.clone();
    form.championship.screenshot_saved(&profile, None);
    shot(&dir.join("championship.png"), |ctx| form.ui(ctx))?;
    let line = "Season · Rally 2 of 6 · Rally of the Lakes · Stage 3 of 6".to_owned();
    form.championship.screenshot_saved(&profile, Some(line));
    shot(&dir.join("championship-saved.png"), |ctx| form.ui(ctx))?;
    form.championship.open_builder(Preset::Weekend, &form.catalog.cars, form.car, 20261105);
    shot(&dir.join("championship-builder.png"), |ctx| form.ui(ctx))?;

    // A season of two rallies in the Drive page's car and its class's cars.
    let mut event = fixture::fixture_event();
    if let Some(car) = form.car() {
        (event.config.car, event.config.class) = (car.key.clone(), super::class_of(car));
    }
    let cars = super::class_cars(&form.catalog.cars, &event.config.class);
    for (k, entrant) in event.field.iter_mut().enumerate().filter(|_| !cars.is_empty()) {
        entrant.car = cars[k % cars.len()].clone();
    }
    let master = fixture::ai().0;
    // The player's pace against the reference: 3rd or so, slower on the wet stage.
    let drive = |event: &mut rbr_season::event::Event, n: usize| {
        for _ in 0..n {
            let rbr_season::event::Turn::Stage { rally, stage } = event.turn else { return };
            let reference = event.rallies[rally].stages[stage].reference;
            let time = reference * [1.012, 1.004, 1.03, 0.998, 1.01, 1.006][stage % 6];
            event.stage_done(&master, PlayerResult { time, penalties: 0.0, retired: false });
            if matches!(event.turn, rbr_season::event::Turn::Service { .. }) && n > 1 {
                event.turn_done();
            }
        }
    };
    let at = |form: &mut Form, event: &rbr_season::event::Event, name: &str| {
        form.championship.screenshot_event(event.clone());
        shot(&dir.join(format!("{name}.png")), |ctx| form.ui(ctx))
    };
    at(form, &event, "championship-welcome")?;
    event.turn_done();
    drive(&mut event, 1);
    at(form, &event, "championship-stage")?;
    drive(&mut event, 1);
    at(form, &event, "championship-service")?;
    if let Some(path) = damage {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let carried = CarriedDamage::from_bytes(&bytes).ok_or("not a damage file")?;
        if let Some(run) = form.championship.run.as_mut() {
            run.damage = Some(carried);
        }
        form.championship.park = None;
        shot(&dir.join("championship-service-damaged.png"), |ctx| form.ui(ctx))?;
    }
    event.turn_done();
    drive(&mut event, 2);
    at(form, &event, "championship-standings")?;
    drive(&mut event, 6);
    at(form, &event, "championship-rally-over")?;
    event.turn_done();
    event.turn_done();
    drive(&mut event, 6);
    event.turn_done();
    at(form, &event, "championship-over")?;
    form.championship.screenshot_saved(&profile, None);
    Ok(())
}
