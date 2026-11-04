//! The Richard Burns Rally 1.02 championship (flow mode 3) around the stage loop of `rbr-gameplay`: the season
//! ([`season`]: its creation, forecast, field and points) and its event queue ([`events`]), the AI drivers' stages and
//! services ([`ai`]), the rally's stages ([`rally`]), the service park between them ([`service`]) and the season in
//! the profile's save ([`store`]). Pure; the host (the launcher) owns the season and runs each stage through
//! `rbr-standalone --championship`, whose stage rules are the flow's mode 3 (`rbr_gameplay::flow::Mode`).

pub mod ai;
pub mod events;
pub mod rally;
pub mod season;
pub mod service;
pub mod store;

pub use events::Turn;
pub use rally::{
    choice_before, tyre_choices, Championship, Rally, RallyOutcome, RallyPhase, RallyStage, StageReport, StageTimes,
};
pub use service::{ParkState, ServicePark};
pub use store::ProfileSeason;
