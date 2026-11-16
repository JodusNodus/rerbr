//! Sessions (ours, `notes/session.md`): the layer above the 1.02 stage loop that lets a rally stage, a hotlap and a
//! lap race be the same kind of thing. A [`Course`] is the geometry, an [`EndRule`] says when a session ends, and
//! [`TimingEvent`]s are what a format's timing tells the rest of the game (the HUD, the records, the ghost).
//!
//! `rbr-gameplay` stays the 1.02 loop, bit-exact against the recordings, and is the `RallyStage` format's engine
//! (its timekeeper's checkpoints become events here). The circuit's timing, [`LapTiming`], is pure: distances and times
//! in, events out.

pub mod book;
pub mod course;
pub mod end;
pub mod event;
pub mod format;
pub mod lap;

pub use book::{BestLap, LapBook};
pub use course::Course;
pub use end::EndRule;
pub use event::TimingEvent;
pub use format::LapRules;
pub use lap::LapTiming;
