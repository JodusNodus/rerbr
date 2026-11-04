//! A lesson's drive: the filter bank fed from the stage loop as the original's listeners feed it
//! ([`rbr_gameplay::mode::ModeRules`]), and the verdict at the end.
//!
//! - Before each driving substep, the control payload (`CSigCarControl`, `E12_Cb::CbF30` 0x476f30) goes to
//!   [`school_tick`]: its throttle, brake and handbrake words.
//! - In the step, the brake-wall emits (`E12_Cb::CbF10` 0x476f10) go to [`checkpoint`], the car's general state
//!   (`E12_Cb::OnDrive` 0x4770d0) to [`school_incident`], then the driveline parameter's float emit
//!   (`E12_AnimSet::Cb930` 0x478930, outside state 10) to [`tracker`]. A `CSigRetireCar` is `E60Flow::M5662c0`'s
//!   school arm, `Set48_1`.
//! - Each driving frame, `E12_AnimSet::Tic570` (0x47a570) runs [`end_race`] with the frame's time step, then emits a
//!   retire's `CSigEndRace` once either trip latch is set.
//!
//! The flow's step counter (`+0xca8`, `M00565d20`) is 2 in a timed lesson's first load (`M5693f0` case 2) and 4 from
//! the drive's start (case 4; an untimed lesson loads in case 4). `+0xcb4` is set by the untimed lessons' load only.

use rbr_gameplay::car_damage::{self, CarriedDamage};
use rbr_gameplay::control::CarControl;
use rbr_gameplay::mode::{ModeEnd, ModeRules, StepView};

use super::bank::Bank;
use super::explain::{failure_reasons, Reason};
use super::judge::eval_filters;
use super::lesson::Lesson;
use super::step::{checkpoint, end_race, school_incident, school_tick, tracker, IncidentInput, SchoolEvent};

/// The countdown's application state, in which `Cb930` does not run the tracker.
const COUNTDOWN: u32 = 10;
/// The flow's step counter while a lesson is driven.
const DRIVE_STEP: u32 = 4;

/// A lesson's bank and the flow words its listeners read.
#[derive(Debug, Clone, PartialEq)]
pub struct SchoolRun {
    lesson: Lesson,
    bank: Bank,
    /// The lesson `.dls`'s table-7 string block (`rbr_formats::dls::DlsFile::string_bytes`): `Trigger`'s pair
    /// codes are the speech digit strings' offsets in it (the pair-code block base 0).
    strings: Vec<u8>,
    /// The flow's step counter (`+0xca8`).
    step: u32,
    /// The flow's `+0xcb4`.
    cb4: i32,
    /// The listeners' emits since the host last took them.
    events: Vec<SchoolEvent>,
}

impl SchoolRun {
    /// `lesson` with the bank its map's `.dls` `Driveline` set built (`Bank::from_stream`) and the `.dls`'s
    /// table-7 string block, in the state its load leaves the flow in.
    pub fn new(lesson: Lesson, bank: Bank, strings: Vec<u8>) -> Self {
        let untimed = lesson.untimed();
        Self {
            lesson,
            bank,
            strings,
            step: if untimed { DRIVE_STEP } else { 2 },
            cb4: i32::from(untimed),
            events: Vec::new(),
        }
    }

    pub fn lesson(&self) -> Lesson {
        self.lesson
    }

    pub fn bank(&self) -> &Bank {
        &self.bank
    }

    /// The incident and sound emits since the last call, in order (the HUD's and the speech's).
    pub fn take_events(&mut self) -> Vec<SchoolEvent> {
        std::mem::take(&mut self.events)
    }

    /// The speech ids one answer's `Sound` event queues: `Trigger` (`a == 2`) reads its pair code as the digit
    /// string's offset in the `.dls`'s string block (`E25School::M80c0x`, [`speech_ids`]). The other kinds' single
    /// ids are the viewer's, from the record's lesson and verdict.
    pub fn speech_ids(&self, event: &SchoolEvent) -> Vec<u32> {
        match event {
            SchoolEvent::Sound { a: 2, d, .. } => super::speech::speech_ids(&self.strings, *d as u32),
            _ => Vec::new(),
        }
    }

    /// The failed lesson's reasons (`explain::failure_reasons`) at race time `time`: the verdict panel's lines.
    /// Empty unless the drive failed; the viewer resolves the keys and skips the unknown ones.
    pub fn failure_reasons(&self, time: f32) -> Vec<Reason> {
        failure_reasons(&self.bank, time, &self.strings)
    }

    /// `EvalFilters` at the drive's end: 0 (failed) to 4 (perfect) for the race time `time` and the car's damage
    /// (`SeverityMax`: the 1.02 repair objects' levels, `car_damage::severity`). The ride is not judged (`M5693f0`
    /// case 5 skips lesson 5): its result is the award's 4.
    pub fn verdict(&self, time: f32, damage: &CarriedDamage) -> u32 {
        if self.lesson.ride() {
            return 4;
        }
        // The repair handler's 0x100 part ids, as `SeverityMax` asks `GetDamage` for them.
        let levels: [u32; 0x100] = std::array::from_fn(|id| u32::from(car_damage::severity(damage, id as u32)));
        eval_filters(&self.bank, time, self.lesson.index() as i32, &levels)
    }
}

impl ModeRules for SchoolRun {
    /// `M5693f0` case 4: the drive starts (`Clear40`, the step counter at 4).
    fn start(&mut self) {
        self.step = DRIVE_STEP;
        self.bank.ack = 0;
    }

    /// `SchoolTick` on the payload's words 0-2 (throttle, brake, handbrake).
    fn before_step(&mut self, control: &CarControl) {
        school_tick(&mut self.bank, [control.throttle, control.brake, control.handbrake], self.step);
    }

    /// `CDriveLine::Sample` (0x50b180) at the bank's `m_00` word as a float: the point lessons 0 and 10 measure
    /// the car's deviation from.
    fn drive_line_at(&self) -> Option<f32> {
        self.lesson.stands_by_the_line().then(|| f32::from_bits(self.bank.origin as u32))
    }

    /// The step's listeners in the step's order: the brake wall, the general state, the driveline float, then the
    /// retire's `Set48_1`.
    fn after_step(&mut self, step: &StepView) {
        let sample = &step.sample;
        for (hit, flags) in [(sample.wall_inside, 4), (sample.wall_crossed, 6)] {
            if hit {
                self.events.extend(checkpoint(&mut self.bank, flags, step.app_state));
            }
        }
        let incident = IncidentInput {
            speed_vec: sample.velocity,
            car_pos: sample.position,
            app_state: step.app_state,
            substate: self.step,
            driveline: step.drive_line,
            cls_gate: false,
        };
        self.events.extend(school_incident(&mut self.bank, &incident));
        if let Some(distance) = step.distance.filter(|_| step.app_state != COUNTDOWN) {
            self.events.extend(tracker(&mut self.bank, distance.to_bits(), self.step, self.cb4));
        }
        if sample.retire.is_some() {
            self.bank.latched_b = true;
        }
    }

    /// `Tic570`: `EndRace`'s emits (a finished `CSigEndRace` each), then a trip latch's retire. The ride
    /// ends as soon as the drive starts, with no judging.
    fn frame(&mut self, dt: f32, race_time: f32) -> Option<ModeEnd> {
        if self.lesson.ride() {
            return Some(ModeEnd::Finished);
        }
        let ends = end_race(&mut self.bank, dt, race_time.to_bits());
        let finished = ends.iter().any(|e| matches!(e, SchoolEvent::EndRace { .. }));
        self.events.extend(ends);
        if finished {
            Some(ModeEnd::Finished)
        } else if self.bank.latched_a || self.bank.latched_b {
            Some(ModeEnd::Retired)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FilterSub;
    use rbr_gameplay::game::StepSample;

    /// Lesson 2's stop box (map 83's third sub): a mode-4 sub at 160.4 m, 18.1 m either side.
    fn stop_box() -> Bank {
        let mut stop = FilterSub::init_sub();
        (stop.tag, stop.class, stop.flags, stop.center, stop.hi, stop.lo) = (4, 3, 1, 160.39, 18.1, 28.1);
        Bank { subs: vec![stop], perfect_time: 9.0, pass_time: 10.0, time_limit: 11.0, ..Bank::fresh() }
    }

    fn standing_at(distance: f32, retire: Option<u32>) -> StepView {
        StepView {
            sample: StepSample { retire, ..StepSample::default() },
            distance: Some(distance),
            drive_line: None,
            app_state: 1,
        }
    }

    #[test]
    fn standing_in_the_stop_box_ends_the_drive_and_passes_on_time() {
        // Lesson 4's verdict leaves the damage out (`SeverityMax` gives 4).
        let mut run = SchoolRun::new(Lesson::new(4).unwrap(), stop_box(), Vec::new());
        run.start();
        // The tracker enlists the box on one step; the standing test takes it on the next.
        run.after_step(&standing_at(150.0, None));
        assert_eq!(run.frame(0.01, 9.2), None);
        run.after_step(&standing_at(150.0, None));
        assert_eq!(run.frame(0.01, 9.4), Some(ModeEnd::Finished));
        let damage = CarriedDamage::default();
        assert_eq!(run.verdict(9.4, &damage), 3);
        assert_eq!(run.verdict(8.9, &damage), 4);
        assert_eq!(run.verdict(10.5, &damage), 0);
    }

    #[test]
    fn sound_events_resolve_their_digit_strings() {
        let block = [b"294\0".as_slice(), b"311 312\0"].concat();
        let run = SchoolRun::new(Lesson::new(2).unwrap(), stop_box(), block);
        assert_eq!(
            run.speech_ids(&SchoolEvent::Sound { a: 2, b: 0, c: 0, d: 0 }),
            [294],
            "the pair code is the string's offset"
        );
        assert_eq!(run.speech_ids(&SchoolEvent::Sound { a: 2, b: 1, c: 0, d: 4 }), [311, 312]);
        assert_eq!(run.speech_ids(&SchoolEvent::Incident { code: 1 }), Vec::<u32>::new());
        assert_eq!(
            run.speech_ids(&SchoolEvent::Sound { a: 1, b: 0, c: 0, d: 0 }),
            Vec::<u32>::new(),
            "other kinds are the viewer's"
        );
    }

    #[test]
    fn a_retire_trips_the_bank_and_fails() {
        let mut run = SchoolRun::new(Lesson::new(2).unwrap(), stop_box(), Vec::new());
        run.start();
        run.after_step(&standing_at(20.0, Some(1)));
        assert_eq!(run.frame(0.01, 3.0), Some(ModeEnd::Retired));
        assert_eq!(run.verdict(3.0, &CarriedDamage::default()), 0);
    }

    #[test]
    fn the_ride_ends_on_its_first_driving_frame() {
        let mut run = SchoolRun::new(Lesson::new(5).unwrap(), stop_box(), Vec::new());
        run.start();
        assert_eq!(run.frame(0.01, 0.0), Some(ModeEnd::Finished));
        assert_eq!(run.verdict(0.0, &CarriedDamage::default()), 4, "not judged: the award's 4");
    }
}
