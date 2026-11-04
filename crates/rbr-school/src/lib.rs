//! The rally school (flow mode 7): the lessons ([`lesson`]), the filter bank their `.dls` stream builds ([`bank`],
//! [`filter`]), its listeners on the drive ([`step`], fed by [`SchoolRun`] as a [`rbr_gameplay::mode::ModeRules`])
//! and the verdict ([`judge`]). Ports of `RBR_E19_Filter.cpp` and `RBR_E19_School.cpp` (exe `0x497b10-0x499360`);
//! where the C++ reconstruction disagrees with the exe, the exe wins, and each function names its deviation.
//!
//! Nothing here writes car or physics state: the bank observes the drive, emits events and ends it.

pub mod bank;
pub mod filter;
pub mod judge;
pub mod lesson;
pub mod run;
pub mod speech;
pub mod step;

pub use bank::{Bank, Check, Lead, Pair};
pub use filter::{max_abs, FilterSub};
pub use judge::{eval_filters, severity_max};
pub use lesson::Lesson;
pub use run::SchoolRun;
pub use speech::{briefing, end_page, speech_ids};
pub use step::{checkpoint, end_race, school_incident, school_tick, tracker, trigger, IncidentInput, SchoolEvent};
