//! The events in the launcher: the Championship tab's page (`Form::events_page`) and a stage's launch
//! (`Form::start_event_stage`), the way the Drive page starts any stage, on the itinerary's road and clock.

use std::path::Path;

use rbr_events::event::{Conditions, Turn};
use rbr_formats::saves::SaveDir;
use rbr_gameplay::race_options::{DamageLevel, RaceOptions};
use rbr_gameplay::weather::{Draws, Preset, Weather};

use super::view::Env;
use super::{wetness, EventStart, Request};
use crate::catalog::{self, TyreChoice};
use crate::command::clock_args;
use crate::pages::{Issue, FIELD};
use crate::settings::wetness_name;
use crate::{browser, car_browser, Form};

impl Form {
    /// The Championship tab outside the 1.02 season: the presets, the builder and the event. An event drives its
    /// own car (the Drive page's car follows it, as the 1.02 season's does).
    pub(crate) fn events_page(&mut self, ui: &mut egui::Ui) {
        if self.events.pool.is_none() {
            let items = super::pool::items(&self.catalog, &self.season);
            let cache = self.here().join(super::pool::CACHE);
            self.events.pool = Some(super::pool::Pool::start(self.data.clone(), cache, items));
        }
        let event_car = self.events.run.as_ref().map(|r| r.play.event().config.car.clone());
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
        let service = self.events.run.as_ref().and_then(|r| match r.play.event().turn {
            Turn::Service { minutes, .. } => Some(minutes),
            _ => None,
        });
        match service {
            Some(minutes) if self.events.park.is_none() && self.events.view == super::View::Event => {
                let car = self.car().cloned().unwrap_or_default();
                let archive = self.ngp_archive();
                self.events.open_park(&self.data, &car, Some(Path::new(&archive)), minutes);
            }
            None => self.events.park = None,
            _ => {}
        }
        self.select_setup();
        let blocker = if car_missing {
            Some(Issue {
                text: format!("the event's car {} is not installed", event_car.unwrap_or_default()),
                fix: None,
            })
        } else {
            self.launch_blocker()
        };
        let here = self.here();
        let mut profile = self.rally.profile.clone();
        let (rows, favourites) = (&self.car_browser, &self.favourite_cars);
        let mut car_field = |ui: &mut egui::Ui, car: usize| {
            rows.row(car).is_some_and(|row| {
                browser::field_button(ui, favourites.contains(&row.name), &row.title, &row.detail(), FIELD)
            })
        };
        let setups = &self.setup;
        let saved = |surface| setups.saved_names(surface);
        let seed = rbr_championship::season::seed_from_unix_ms(
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
        let request = egui::ScrollArea::vertical().id_salt("events").show(ui, |ui| self.events.ui(ui, env)).inner;
        self.rally.profile = profile;
        match request {
            Some(Request::Launch(start)) => self.start_event_stage(start),
            Some(Request::Setup) => self.page = crate::pages::Page::Setup,
            Some(Request::BrowseCar) => {
                let car = self.events.car.unwrap_or(self.car);
                self.car_browser.open_on(car_browser::Target::Event, car, &self.favourite_cars, None);
            }
            None => {}
        }
    }

    /// The stage and weather an event stage runs on: the map's weather row with the road's wetness (the nearest one
    /// otherwise), and whether the road needs `--wetness` over the row's.
    pub(crate) fn event_weather(
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

    /// Starts an event's stage like the Drive page's Start, on the itinerary's road and clock, the tyre and setup
    /// picked, the event's damage and the files `stage_start` named.
    pub fn start_event_stage(&mut self, start: EventStart) {
        let EventStart { map, conditions, clock, tyre, setup, args: mut extra } = start;
        match self.event_weather(map, conditions) {
            Ok((stage, weather, off)) => {
                self.map = self.catalog.map_index(map).unwrap_or(self.map);
                self.validate_weather_row();
                self.select_setup();
                if let Err(e) = self.setup.choose(&setup) {
                    eprintln!("launcher: setup {}: {e}; the stage keeps the current setup", setup.label());
                }
                let saves = SaveDir::new(&self.records()).path.display().to_string();
                extra.extend(["--saves".to_owned(), saves, "--profile".to_owned(), self.rally.profile.clone()]);
                if off {
                    extra.extend(["--wetness".to_owned(), wetness_name(weather.wetness).to_owned()]);
                }
                let minutes = (clock * 60.0).round() as u32 % (24 * 60);
                let damage =
                    match extra.iter().position(|a| a == "--championship-damage").and_then(|i| extra.get(i + 1)) {
                        Some(word) if word == "off" => DamageLevel::Off,
                        Some(word) if word == "reduced" => DamageLevel::Reduced,
                        _ => DamageLevel::Realistic,
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
            self.events.abort_stage_start();
        }
    }
}

/// `--screenshot`: the events on the double ([`super::fixture`]): the presets without and with a saved event, the
/// builder, and a season's welcome, a stage, a service, the standings later in the rally, the rally's end and the
/// event's end.
pub fn shots(form: &mut Form, dir: &Path) -> Result<(), String> {
    use super::fixture::{self, Double};
    use super::model::Play;
    use crate::capture::shot;
    use rbr_events::config::Preset;
    use rbr_events::event::PlayerResult;
    form.page = crate::pages::Page::Championship;
    form.events.model = &fixture::DOUBLE;
    form.events.pool = Some(super::pool::Pool::Ready(fixture::stages()));
    let profile = form.rally.profile.clone();
    form.events.screenshot_saved(&profile, None);
    shot(&dir.join("events.png"), |ctx| form.ui(ctx))?;
    let line = "Season · Rally 2 of 6 · Rally of the Lakes · Stage 3 of 6".to_owned();
    form.events.screenshot_saved(&profile, Some(line));
    shot(&dir.join("events-saved.png"), |ctx| form.ui(ctx))?;
    form.events.open_builder(Preset::Weekend, &form.catalog.cars, form.car, 20261105);
    shot(&dir.join("events-builder.png"), |ctx| form.ui(ctx))?;

    // A season of two rallies in the Drive page's car and its class's cars.
    let mut event = fixture::fixture_event();
    if let Some(car) = form.car() {
        (event.config.car, event.config.class) = (car.key.clone(), super::class_of(car));
    }
    let cars = super::class_cars(&form.catalog.cars, &event.config.class);
    for (k, entrant) in event.field.iter_mut().enumerate().filter(|_| !cars.is_empty()) {
        entrant.car = cars[k % cars.len()].clone();
    }
    let mut play = Double(event);
    // The player's pace against the reference: 3rd or so, slower on the wet stage.
    let drive = |play: &mut Double, n: usize| {
        for _ in 0..n {
            let rbr_events::event::Turn::Stage { rally, stage } = play.0.turn else { return };
            let s = &play.0.rallies[rally].stages[stage];
            let reference = s.stage.length / 22.0;
            let time = reference * [1.012, 1.004, 1.03, 0.998, 1.01, 1.006][stage % 6];
            play.stage_done(PlayerResult { time, penalties: 0.0, retired: false }, reference);
            if matches!(play.0.turn, rbr_events::event::Turn::Service { .. }) && n > 1 {
                play.turn_done();
            }
        }
    };
    let at = |form: &mut Form, play: &Double, name: &str| {
        form.events.screenshot_event(Box::new(Double(play.0.clone())));
        shot(&dir.join(format!("{name}.png")), |ctx| form.ui(ctx))
    };
    at(form, &play, "events-welcome")?;
    play.turn_done();
    drive(&mut play, 1);
    at(form, &play, "events-stage")?;
    drive(&mut play, 1);
    at(form, &play, "events-service")?;
    play.turn_done();
    drive(&mut play, 2);
    at(form, &play, "events-standings")?;
    drive(&mut play, 6);
    at(form, &play, "events-rally-over")?;
    play.turn_done();
    play.turn_done();
    drive(&mut play, 6);
    play.turn_done();
    at(form, &play, "events-over")?;
    form.events.screenshot_saved(&profile, None);
    form.events.view = super::View::Classic;
    Ok(())
}
