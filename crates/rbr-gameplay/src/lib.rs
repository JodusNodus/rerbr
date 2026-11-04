//! The Richard Burns Rally 1.02 gameplay loop in Rust (milestone G, `notes/rust-plan.md`; the
//! original's path in `notes/gameplay.md`): the input to the car
//! control, the frame clock and the substeps, the stage's states, timing and markers, and the race flow. The
//! championship around it (the season, the AI, the service park, the saves) is `rbr-championship`.
//! `rbr-gameplay/README.md` lists each module, the original functions it ports and how it is checked.

pub mod camera_go;
pub mod car_damage;
pub mod control;
pub mod crt;
pub mod driveline;
pub mod ffb;
pub mod filter;
pub mod flow;
pub mod game;
pub mod ghost;
pub mod ini;
pub mod markers;
pub mod mode;
pub mod pause_menu;
pub mod poll;
pub mod race_clock;
pub mod race_options;
pub mod record;
pub mod result;
pub mod setup;
pub mod stage;
pub mod tick;
pub mod timing;
pub mod weather;
