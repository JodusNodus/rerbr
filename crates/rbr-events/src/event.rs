//! A built event (ours): the itinerary drawn from the stage pool, the field, the results so far, the standings and
//! points, the player's next turn, and the save in the player's folder.

use crate::config::{EventConfig, StageRef};
use crate::field::{AiResult, Entrant};

/// One stage of a rally's itinerary.
#[derive(Debug, Clone, PartialEq)]
pub struct ItineraryStage {
    pub stage: StageRef,
    /// The start clock (hours, 0 to 24): the launcher passes it as the stage's time of day.
    pub clock: f32,
    pub conditions: Conditions,
    /// The service after this stage (minutes), none when the next stage follows directly.
    pub service_after: Option<u32>,
}

/// A stage's road and sky, which the launcher maps to the stage's own weathers (a dry, damp or wet row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conditions {
    Dry,
    Damp,
    Wet,
}

/// One rally of the event.
#[derive(Debug, Clone, PartialEq)]
pub struct Rally {
    pub name: String,
    pub stages: Vec<ItineraryStage>,
}

/// The player's result on a stage, as the stage's run reports it (`--result-out`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerResult {
    pub time: f32,
    pub penalties: f32,
    pub retired: bool,
}

/// What the player does next.
#[derive(Debug, Clone, PartialEq)]
pub enum Turn {
    /// The rally's start: its itinerary, before its first stage.
    Welcome {
        rally: usize,
    },
    Stage {
        rally: usize,
        stage: usize,
    },
    /// A service after `stage`, `minutes` long.
    Service {
        rally: usize,
        stage: usize,
        minutes: u32,
    },
    /// The rally is over; the next one, if any, follows.
    RallyOver {
        rally: usize,
    },
    EventOver,
}

/// A standings row.
#[derive(Debug, Clone, PartialEq)]
pub struct Standing {
    /// `None` for the player.
    pub entrant: Option<usize>,
    /// The total time (s) with penalties; `None` once retired.
    pub total: Option<f32>,
    pub points: u32,
}

/// An event under way.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub config: EventConfig,
    pub rallies: Vec<Rally>,
    pub field: Vec<Entrant>,
    /// Per rally, per stage: the player's result and each entrant's.
    pub results: Vec<Vec<(Option<PlayerResult>, Vec<AiResult>)>>,
    pub turn: Turn,
}

impl Event {
    /// Builds the event: the itinerary from `stages` (the installed pool) by the config's length, pool, surfaces,
    /// daylight and weather, and the field from `class_cars` (catalog keys of the config's class).
    /// `reference` gives a stage's reference time for the class at a wetness (`pace::reference_time` over its
    /// driveline).
    pub fn build(
        config: EventConfig,
        stages: &[StageRef],
        class_cars: &[String],
        reference: &dyn Fn(&StageRef, f32) -> f32,
    ) -> Result<Event, String> {
        let _ = (config, stages, class_cars, reference);
        todo!("rbr-events worker")
    }

    /// The player's stage result: the field drives the same stage, and the turn moves on.
    pub fn stage_done(&mut self, result: PlayerResult, reference: f32) {
        let _ = (result, reference);
        todo!("rbr-events worker")
    }

    /// The service (or welcome, or rally over) is done: the turn moves on.
    pub fn turn_done(&mut self) {
        todo!("rbr-events worker")
    }

    /// Rally `rally`'s standings after the stages run so far, best first.
    pub fn rally_standings(&self, rally: usize) -> Vec<Standing> {
        let _ = rally;
        todo!("rbr-events worker")
    }

    /// The event's points standings (a season's; a single rally's are its rally standings), best first.
    pub fn points(&self) -> Vec<Standing> {
        todo!("rbr-events worker")
    }

    /// The event as text for the player's folder.
    pub fn save(&self) -> String {
        todo!("rbr-events worker")
    }

    /// An event read back from [`Event::save`]'s text.
    pub fn load(text: &str) -> Result<Event, String> {
        let _ = text;
        todo!("rbr-events worker")
    }
}
