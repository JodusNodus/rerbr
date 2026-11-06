//! The championship season (ours): a rally or a season the player builds from presets, on any installed stage
//! (retail, RSF, BTB), against a field in the player's car class, run on the 1.02 AI engine. It is the one championship
//! mode; the classic 1.02 season is gone.
//!
//! - [`config`]: what the player picks (class, car, length, stage pool, surface, time of day, weather, difficulty),
//!   and the presets that fill it.
//! - [`pace`]: a stage's reference time for a car class, from its driveline's curvature, length and surface.
//! - [`track`]: a stage's AI track (three sections, two checks each) from its driveline.
//! - [`field`]: the AI entrants in the player's class, driven by the 1.02 AI engine ([`ai`]).
//! - [`event`]: the built itinerary, the results, the standings and points, the start order, the player's turn, the
//!   bulletins, and the save.
//! - [`team`]: the teams whose service crew the player can pick ([`service`] runs the service park).
//! - [`rally`], [`road`], [`seed`]: the stage report, the road's wear and the seed the launcher draws from the clock.
//!
//! The physics and the stage gameplay are the game's, run as on the Drive page; only this layer around them is ours.

pub mod ai;
pub mod car_pace;
pub mod config;
pub mod event;
pub mod field;
pub mod pace;
pub mod rally;
pub mod road;
pub mod seed;
pub mod service;
pub mod team;
pub mod track;
