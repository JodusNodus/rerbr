//! A built championship (ours): the itinerary drawn from the stage pool, the field, the results so far, the standings
//! and points, the player's next turn, and the save in the player's folder.

use crate::ai::{self, Bulletin, RunLog, TeamCosts};
use crate::config::{Daylight, EventConfig, Length, StagePool, StageRef, Surfaces, Weather};
use crate::field::{self, AiResult, Contender, Entrant};
use crate::road::road_wear;
use crate::service::Crew;
use crate::team::crew_of;
use crate::track::{StageProfile, PAR_SCALE};
use rbr_formats::ai_ini::{Master, Roster, Section, Team, Track};
use rbr_gameplay::crt::CrtRand;

mod build;
pub use build::rally_name;
mod run;
mod save;
mod standings;
#[cfg(test)]
mod tests;

/// One stage of a rally's itinerary.
#[derive(Debug, Clone, PartialEq)]
pub struct ItineraryStage {
    pub stage: StageRef,
    /// The start clock (hours, 0 to 24): the launcher passes it as the stage's time of day.
    pub clock: f32,
    pub conditions: Conditions,
    /// The dry reference time (s) of the stage for the field's class.
    pub reference: f32,
    /// The stage's AI track (dry, at the field's best pace): the 1.02 AI engine drives it.
    pub track: Track,
    /// The service after this stage (minutes), none when the next stage follows directly.
    pub service_after: Option<u32>,
    /// Each entrant's time on this stage over the track's for its car model (`car_pace`), 1 where none is set.
    pub car_factors: Vec<f32>,
}

impl ItineraryStage {
    /// The leg ends after this stage: the overnight service follows.
    pub fn ends_leg(&self) -> bool {
        self.service_after.is_some_and(|m| m >= build::SERVICE_NIGHT)
    }

    /// Entrant `e`'s car factor on this stage.
    pub fn car_factor(&self, e: usize) -> f32 {
        self.car_factors.get(e).copied().unwrap_or(1.0)
    }
}

/// A stage's road and sky, which the launcher maps to the stage's own weathers (a dry, damp or wet row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conditions {
    Dry,
    Damp,
    Wet,
}

/// One rally as planned, before its clocks, conditions and services: its name and each leg's stages in order. The
/// seed draws a plan ([`Event::plan`]); the player may edit it before the event is built from it.
#[derive(Debug, Clone, PartialEq)]
pub struct RallyPlan {
    pub name: String,
    pub legs: Vec<Vec<StageRef>>,
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
    /// The power stage's points, included in `points`.
    pub bonus: u32,
}

/// The points for the top 10 of a rally, modern 25-18-15-12-10-8-6-4-2-1.
pub const POINTS: [u32; 10] = [25, 18, 15, 12, 10, 8, 6, 4, 2, 1];

/// The power stage's bonus points for its five fastest classified finishers.
pub const POWER_POINTS: [u32; 5] = [5, 4, 3, 2, 1];

/// Super Rally's penalty (s) on the fastest time of each stage a restarting crew did not complete (WRC: 10 minutes).
pub const RESTART_CHARGE: f32 = 600.0;

/// The field's pace over its tracks' pars at difficulty `d`: 1 at the top, about 9 % slower at the bottom.
fn pace_for(difficulty: f32) -> f32 {
    1.0 / (1.0 - 0.08 * (1.0 - difficulty.clamp(0.0, 1.0)))
}

/// The pace the field may be adapted to (times the pars).
const PACE_RANGE: (f32, f32) = (0.97, 1.18);

/// An event under way.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub config: EventConfig,
    pub rallies: Vec<Rally>,
    pub field: Vec<Entrant>,
    /// What the 1.02 AI engine carries for each entrant (parallel to `field`): driver and car state, team, false starts.
    pub contenders: Vec<Contender>,
    /// The roster's teams and their repair spending (`ai::TeamCosts`).
    pub teams: Vec<Team>,
    pub costs: Vec<TeamCosts>,
    /// The field's pace over its tracks' pars: the difficulty sets it and the adaptive field moves it.
    pub pace: f32,
    /// Per rally, per stage: the player's result and each entrant's.
    pub results: Vec<Vec<(Option<PlayerResult>, Vec<AiResult>)>>,
    /// Per rally, per entrant: the late penalties (s) of its services.
    pub penalties: Vec<Vec<f32>>,
    /// Per rally: what the bulletins reported, in order.
    pub news: Vec<Vec<String>>,
    pub turn: Turn,
}

impl Conditions {
    fn wetness(self) -> f32 {
        match self {
            Conditions::Dry => 0.0,
            Conditions::Damp => 0.5,
            Conditions::Wet => 1.0,
        }
    }
}

impl Event {
    /// The player's service crew: the engineer and mechanics of the team the config picked.
    pub fn crew(&self) -> Crew {
        self.teams.get(self.config.team as usize).map_or_else(Crew::default, crew_of)
    }

    /// The rally and stage the results are being driven on, if the turn is a stage.
    fn at(&self) -> Option<(usize, usize)> {
        match self.turn {
            Turn::Stage { rally, stage } => Some((rally, stage)),
            _ => None,
        }
    }

    /// The service (or welcome, or rally over) is done: the turn moves on.
    pub fn turn_done(&mut self) {
        match self.turn.clone() {
            Turn::Welcome { rally } => {
                if self.rallies.get(rally).is_some_and(|r| !r.stages.is_empty()) {
                    self.turn = Turn::Stage { rally, stage: 0 };
                }
            }
            Turn::Service { rally, stage, .. } => {
                let len = self.rallies.get(rally).map_or(0, |r| r.stages.len());
                self.turn =
                    if stage + 1 < len { Turn::Stage { rally, stage: stage + 1 } } else { Turn::RallyOver { rally } };
            }
            Turn::RallyOver { rally } => {
                self.turn =
                    if rally + 1 < self.rallies.len() { Turn::Welcome { rally: rally + 1 } } else { Turn::EventOver };
            }
            Turn::Stage { .. } | Turn::EventOver => {}
        }
    }
}

/// The stages run in a rally's results: every row the player or the field drove.
pub fn run_stages(results: &[(Option<PlayerResult>, Vec<AiResult>)]) -> usize {
    results.iter().filter(|(p, ai)| p.is_some() || !ai.is_empty()).count()
}

/// The turn after stage `stage` of `rally` ran.
fn next_turn(rallies: &[Rally], rally: usize, stage: usize) -> Turn {
    let len = rallies.get(rally).map_or(0, |r| r.stages.len());
    if stage + 1 < len {
        if let Some(m) = rallies[rally].stages[stage].service_after {
            return Turn::Service { rally, stage, minutes: m };
        }
        return Turn::Stage { rally, stage: stage + 1 };
    }
    Turn::RallyOver { rally }
}
