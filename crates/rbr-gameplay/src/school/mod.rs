//! The rally school (flow mode 7): the lessons ([`lesson`]) and the judging core, the E19 filter bank and its in-step
//! and judge methods (`RBR_E19_Filter.cpp`, `RBR_E19_School.cpp`, exe `0x4980c0-0x499360`).
//!
//! The bank is read from the stage's `.dls` file, not built by code (`E12_AnimSet::Load020` 0x479020 allocates a
//! 0x60-byte bank per animation record and runs `InitState` 0x4980c0 and `ReadStream` 0x498840 on its word-16
//! stream). Nothing here writes car or physics state: the bank is observers plus emitted events plus the retire
//! flag (`Set48_1`, via `E60Flow::M5662c0` 0x5662c0).
//!
//! Where the C++ reconstruction disagrees with the exe, the exe wins (each deviation is named at its function):
//! `Trigger` uses a fresh `1 << (i & 31)` mask and aborts on `v < thr`; `Publish` snapshots the BSS mirrors into
//! the bank (the reconstruction has its body backwards); `EvalFilters` returns 0 on the trip latches with a strict
//! time ladder; `ReadStream` stores the header's times reversed (`m_08` word 6, `m_10` word 4); `InRange`
//! subtracts `m_04`; `SetThresh` stores `m_14 = b, m_10 = a`; `IsArmed` tests `m_14`/`m_18` ordered-nonzero; `Trip`
//! fires once when armed; `Classify` case 3 reads the low byte of `m_0c`; `Category` returns 0 above the bound;
//! `Over2` reinterprets the int bits; `RallySchoolSound` emits `{1, 0, lesson, 0}`; `Checkpoint` emits unless the
//! timer is positive; `SchoolIncident`'s enlisted loop runs four slots; `MaxAbs` keeps the first NaN.

pub mod bank;
pub mod filter;
pub mod judge;
pub mod lesson;
pub mod run;
pub mod step;

pub use bank::{Bank, Check, Lead, Mirrors, Pair, CHECK_SPREAD, STREAM_MAGIC};
pub use filter::{max_abs, FilterSub, WINDOW};
pub use judge::{eval_filters, severity_max};
pub use lesson::Lesson;
pub use run::{SchoolRun, StepInput};
pub use step::{
    checkpoint, end_race, rally_sound, school_incident, school_tick, tracker, trigger, IncidentInput, SchoolEvent,
    CHECKPOINT_REARM, DEVIATION_LIMIT, INPUT_GATE, SPEED_STOP,
};
