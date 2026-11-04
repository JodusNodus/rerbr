//! A lesson's drive: the filter bank fed from the game's frame, the way the original's listeners feed it, and the
//! verdict at the end.
//!
//! - Before each driving substep, the control payload (`CSigCarControl`, `E12_Cb::CbF30` 0x476f30) goes to
//!   [`school_tick`]: its throttle, brake and handbrake words.
//! - In the step, the brake-wall emits (`E12_Cb::CbF10` 0x476f10) go to [`checkpoint`], the car's general state
//!   (`E12_Cb::OnDrive` 0x4770d0) to [`school_incident`], then the driveline parameter's float emit
//!   (`E12_AnimSet::Cb930` 0x478930, outside state 10) to [`tracker`] as `E12_SampleX` of it. A `CSigRetireCar` is
//!   `E60Flow::M5662c0`'s school arm, `Set48_1`.
//! - Each driving frame, `E12_AnimSet::Tic570` (0x47a570) runs [`end_race`] with the frame's time step, then emits a
//!   retire's `CSigEndRace` once either trip latch is set.
//!
//! The flow's step counter (`+0xca8`, `M00565d20`) is 2 in a timed lesson's first load (`M5693f0` case 2) and 4 from
//! the drive's start (case 4; an untimed lesson loads in case 4). `+0xcb4` is set by the untimed lessons' load only.

use crate::car_damage::{self, CarriedDamage};

use super::bank::Bank;
use super::judge::eval_filters;
use super::lesson::Lesson;
use super::step::{checkpoint, end_race, school_incident, school_tick, tracker, IncidentInput, SchoolEvent};

/// The countdown's application state, in which `Cb930` does not run the tracker.
const COUNTDOWN: u32 = 10;
/// The flow's step counter while a lesson is driven.
const DRIVE_STEP: u32 = 4;

/// What a step of a lesson's drive gave the listeners: the brake-wall flags, the general state and the driveline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepInput {
    /// The step's brake-wall emits: one from inside an emitting wall (flag bits 4), one from a crossing (6).
    pub wall_inside: bool,
    pub wall_crossed: bool,
    /// `CSigCarGeneralState`'s velocity (+0x30) and position (+0x10).
    pub velocity: [f32; 3],
    pub position: [f32; 3],
    /// `E12_SampleX` of the driveline parameter (`Cb930`'s argument), if it is on the table.
    pub distance: Option<f32>,
    /// `CDriveLine::Sample` (0x50b180) at the bank's `m_00` word as a float: the driveline point lessons 0 and 10
    /// measure the car's deviation from. The host computes it at [`SchoolRun::sample_at`].
    pub driveline: Option<[f32; 3]>,
    /// The application state the step ran in.
    pub app_state: u32,
}

/// A lesson's bank and the flow words its listeners read.
#[derive(Debug, Clone, PartialEq)]
pub struct SchoolRun {
    lesson: Lesson,
    bank: Bank,
    /// The flow's step counter (`+0xca8`).
    step: u32,
    /// The flow's `+0xcb4`.
    cb4: i32,
    /// The listeners' emits since the host last took them.
    events: Vec<SchoolEvent>,
}

impl SchoolRun {
    /// `lesson` with the bank its map's `.dls` `Driveline` set built (`Bank::from_stream`), in the state its load
    /// leaves the flow in.
    pub fn new(lesson: Lesson, bank: Bank) -> Self {
        let untimed = lesson.untimed();
        Self { lesson, bank, step: if untimed { DRIVE_STEP } else { 2 }, cb4: i32::from(untimed), events: Vec::new() }
    }

    pub fn lesson(&self) -> Lesson {
        self.lesson
    }

    pub fn bank(&self) -> &Bank {
        &self.bank
    }

    /// `M5693f0` case 4: the drive starts (`Clear40`, the step counter at 4).
    pub fn start(&mut self) {
        self.step = DRIVE_STEP;
        self.bank.clear_ack();
    }

    /// The distance [`StepInput::driveline`] samples the drive line at, when the lesson needs it (0 and 10).
    pub fn sample_at(&self) -> Option<f32> {
        matches!(self.lesson.index(), 0 | 10).then(|| f32::from_bits(self.bank.origin as u32))
    }

    /// `SchoolTick` on a driving substep's control payload words 0-2 (throttle, brake, handbrake).
    pub fn before_step(&mut self, throttle: f32, brake: f32, handbrake: f32) {
        school_tick(&mut self.bank, [throttle, brake, handbrake], true, self.step);
    }

    /// The step's listeners in the step's order: the brake wall, the general state, the driveline float, then the
    /// retire's `Set48_1`.
    pub fn in_step(&mut self, input: &StepInput, retire: bool) {
        for (hit, flags) in [(input.wall_inside, 4), (input.wall_crossed, 6)] {
            if hit {
                self.events.extend(checkpoint(&mut self.bank, flags, true, input.app_state));
            }
        }
        let incident = IncidentInput {
            speed_vec: input.velocity,
            car_pos: input.position,
            is_school: true,
            app_state: input.app_state,
            substate: self.step,
            lesson: self.lesson.index(),
            driveline: input.driveline,
            cls_gate: false,
        };
        self.events.extend(school_incident(&mut self.bank, &incident));
        if let Some(distance) = input.distance.filter(|_| input.app_state != COUNTDOWN) {
            self.events.extend(tracker(&mut self.bank, distance.to_bits(), true, self.step, self.cb4));
        }
        if retire {
            self.bank.set_latched_b_1();
        }
    }

    /// `Tic570` in a driving frame of `dt` with the HUD's race time: `EndRace`'s emits (a finished `CSigEndRace`
    /// each), and whether the trip latches end the drive (a retire's `CSigEndRace`).
    pub fn frame(&mut self, dt: f32, time: f32) -> (bool, bool) {
        let ends = end_race(&mut self.bank, dt, time.to_bits(), true);
        let finished = ends.iter().any(|e| matches!(e, SchoolEvent::EndRace { .. }));
        self.events.extend(ends);
        (finished, self.bank.latched_a() != 0 || self.bank.latched_b() != 0)
    }

    /// The incident and sound emits since the last call, in order (the HUD's and the speech's).
    pub fn take_events(&mut self) -> Vec<SchoolEvent> {
        std::mem::take(&mut self.events)
    }

    /// `EvalFilters` at the drive's end: 0 (failed) to 4 (perfect) for the race time `time` and the car's damage
    /// (`SeverityMax`: the 1.02 repair objects' levels, `car_damage::severity`).
    pub fn verdict(&self, time: f32, damage: &CarriedDamage) -> u32 {
        // The repair handler's 0x100 part ids, as `SeverityMax` asks `GetDamage` for them.
        let levels: [u32; 0x100] = std::array::from_fn(|id| u32::from(car_damage::severity(damage, id as u32)));
        eval_filters(&self.bank, time, self.lesson.index() as i32, &levels)
    }
}
