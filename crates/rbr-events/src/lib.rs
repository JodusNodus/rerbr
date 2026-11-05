//! Rally events (ours, not 1.02's): a rally or a season the player builds from presets, on any installed stage
//! (retail, RSF, BTB), against a field in the player's car class. The faithful 1.02 season stays in
//! `rbr-championship`; the launcher offers it as the "Classic 1.02 season" preset beside these.
//!
//! - [`config`]: what the player picks (class, car, length, stage pool, surface, time of day, weather, difficulty),
//!   and the presets that fill it.
//! - [`pace`]: a stage's reference time for a car class, from its driveline's curvature, length and surface.
//! - [`field`]: the AI entrants in the player's class and their stage times around the reference.
//! - [`event`]: the built itinerary, the results, the standings and points, the player's turn, and its save.
//!
//! The physics and the stage gameplay are the game's, run as on the Drive page; only this layer around them is ours.

pub mod config;
pub mod event;
pub mod field;
pub mod pace;
