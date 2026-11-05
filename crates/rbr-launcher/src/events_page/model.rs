//! The page's way into `rbr_season`: the presets, building and loading an event, and an event's turns, standings and
//! save. The page runs on [`REAL`]; the screenshots and the tests run on the fixture's double
//! ([`super::fixture::DOUBLE`]), which keeps the same turn order on a simple field.

use rbr_season::config::{EventConfig, Preset, StageRef};
use rbr_season::event::{Event, PlayerResult, Standing};

/// An event under way, as the page drives it.
pub trait Play {
    /// The event's settings, itinerary, field, results and turn.
    fn event(&self) -> &Event;
    fn stage_done(&mut self, result: PlayerResult, reference: f32);
    fn turn_done(&mut self);
    fn rally_standings(&self, rally: usize) -> Vec<Standing>;
    fn points(&self) -> Vec<Standing>;
    fn save(&self) -> String;
}

impl Play for Event {
    fn event(&self) -> &Event {
        self
    }
    fn stage_done(&mut self, result: PlayerResult, reference: f32) {
        Event::stage_done(self, result, reference)
    }
    fn turn_done(&mut self) {
        Event::turn_done(self)
    }
    fn rally_standings(&self, rally: usize) -> Vec<Standing> {
        Event::rally_standings(self, rally)
    }
    fn points(&self) -> Vec<Standing> {
        Event::points(self)
    }
    fn save(&self) -> String {
        Event::save(self)
    }
}

/// A stage's reference time for the class at a wetness (0 to 1).
pub type Reference<'a> = &'a dyn Fn(&StageRef, f32) -> f32;

/// Builds an event: the config, the pool, the class's cars and the reference.
pub type Build = fn(EventConfig, &[StageRef], &[String], Reference) -> Result<Box<dyn Play>, String>;

/// The presets, the build and the load.
pub struct Model {
    pub config: fn(Preset, &str, &str, u32) -> EventConfig,
    pub build: Build,
    pub load: fn(&str) -> Result<Box<dyn Play>, String>,
}

/// `rbr_season` itself.
pub const REAL: Model = Model {
    config: |preset, class, car, seed| preset.config(class, car, seed),
    build: |config, stages, cars, reference| Ok(Box::new(Event::build(config, stages, cars, reference)?)),
    load: |text| Ok(Box::new(Event::load(text)?)),
};
