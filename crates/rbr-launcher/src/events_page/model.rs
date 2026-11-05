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
    /// The news since the player's last stage in `rally`, one line each ("Since your last stage").
    fn bulletins(&self, rally: usize) -> Vec<String>;
    /// The road order of a stage: the field's entrants (`None` is the player) from first on the road to last. Empty
    /// when the model keeps none.
    fn start_order(&self, rally: usize, stage: usize) -> Vec<Option<usize>>;
    /// How worn the road is for the player at the stage, 0 (clean) to 1 (heavily worn).
    fn road_wear(&self, rally: usize, stage: usize) -> f32;
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
    // wired at merge: bulletins, the road order and the road's wear come with `rbr_season`'s next version.
    fn bulletins(&self, _: usize) -> Vec<String> {
        Vec::new()
    }
    // wired at merge
    fn start_order(&self, _: usize, _: usize) -> Vec<Option<usize>> {
        Vec::new()
    }
    // wired at merge
    fn road_wear(&self, _: usize, _: usize) -> f32 {
        0.0
    }
}

/// A team (service crew) the builder offers: its index, name and what its crew does, in words.
pub type TeamOffer = (usize, String, String);

/// A stage's reference time for the class at a wetness (0 to 1).
pub type Reference<'a> = &'a dyn Fn(&StageRef, f32) -> f32;

/// Builds an event: the config, the pool, the class's cars and the reference.
pub type Build = fn(EventConfig, &[StageRef], &[String], Reference) -> Result<Box<dyn Play>, String>;

/// The presets, the build and the load.
pub struct Model {
    pub config: fn(Preset, &str, &str, u32) -> EventConfig,
    pub build: Build,
    pub load: fn(&str) -> Result<Box<dyn Play>, String>,
    /// The teams the builder offers for the service crew.
    pub team_offers: fn() -> Vec<TeamOffer>,
}

/// `rbr_season` itself.
pub const REAL: Model = Model {
    config: |preset, class, car, seed| preset.config(class, car, seed),
    build: |config, stages, cars, reference| Ok(Box::new(Event::build(config, stages, cars, reference)?)),
    load: |text| Ok(Box::new(Event::load(text)?)),
    // wired at merge: the model's teams.
    team_offers: Vec::new,
};
