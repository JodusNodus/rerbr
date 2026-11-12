//! The answer's records after a step: the end page, the race end, the pause menu, the school,
//! flow and dash records.
//! Live mode (`rbr-standalone --live --snapshot Y --stage NAME`): the viewer's stepping
//! process, speaking `harness/live.py`'s protocol (`rbr-render`'s `live`). Per 32-byte control payload on stdin it
//! writes one FRM1 frame, a 32-byte HUD2 record (`rbr_recording::hud`: live.py's HUD1 and the race clock) and a DMG1
//! record (`rbr_recording::damage`: the damage zones and the last damage event) on stdout, then the tagged records
//! (`rbr_recording::live`): the step's tyre-particle payloads (PTC1, `rbr_recording::particles`), the detached parts
//! (DET1, `rbr_recording::detach`), the attached doors' hinge angles (DOR1, `rbr_recording::detach::DOR1`), the
//! sound inputs (SND1, `rbr_recording::sound`), the step's pace-note callouts (PCN1, `rbr_recording::pacenote`), the splits (SPL1, `rbr_recording::splits`), the HUD's dash values (DSH1,
//! `rbr_recording::dash`), the camera's mode and start orbit (CAM1, `rbr_recording::camera`), the race flow's events
//! (FLW1, `rbr_recording::flow`) and the steering force (FFB1, `rbr_recording::ffb`), the step clock (CLK1,
//! `rbr_recording::clock`), under NGP its sound words (NSD1, `rbr_recording::sound::NgpSound`, after FFB1), the gear
//! ratios and drive speed (GBX1, `rbr_recording::gearbox`), the shift points (SHF1, `rbr_recording::shift`), the tyres' hub offsets (HUB1, `rbr_recording::hub`) and temperatures (TYR1, `rbr_recording::tyre`), the pause
//! menu's (MNU1, `rbr_recording::menu`), what NGP's FMOD block takes from the car (FMB1,
//! `rbr_recording::sound::FmodBlock`), the damage handler's carried words for the HUD's car status (CDM1,
//! `rbr_recording::damage::CDM1`), the car's control the last step took (CTL1, `rbr_recording::live::CTL1`), and
//! last the SCN1 scenery poses: the frame's pre and post count from the snapshot's step, and the car and wheels after
//! the payload's step.
//!
//! The first payload is the snapshot's own step, whose control the game had already emitted, so its bytes are not
//! used. Each later payload is one `Game::frame`: the gameplay loop emits the payload and the pure physics runs
//! `StepForward(1)` on the snapshot's memory for each of the frame's substeps, as the headless run does
//! (`crate::physics::Pure`). A payload is one step of wall time (one substep) unless [`Live::pace`] gives it a frame time: the
//! viewer's `--live` sends one per 7 ms (1.02's step), which under NGP's 1/720 s step is a frame of 5 or 6 substeps
//! (the frame clock carries the rest), so every clock the game ticks by the frame's time (the countdown, the camera's
//! orbit, the race clock, the finish timer) runs at real speed under both. Such a frame's records are the frame's: the
//! car after its last step, its events, and each wheel's last tyre payload. The game starts
//! driving: a snapshot taken mid-drive has its load and countdown behind it, and the game's clock and time card do
//! not feed the physics.
//!
//! `--live --boot --stage NAME --car N` starts from the boot's image instead (`boot::build`, the
//! stage start): before the first payload the game runs the load settle as the headless boot run does. With the
//! camera's go timer (`rbr_gameplay::camera_go`, `--live --boot`), the payloads are then the frames of the countdown
//! (state 10: the camera's start orbit, no physics step; answered with the car as it stands, pre equal to post and no
//! substeps) until the camera gives the go and the driving starts. Each payload stands for its frame time, as in the
//! drive, so the go comes after `pi / 1.2` s of frames (375 of 7 ms). In a championship (modes 3 to 5) the orbit gives
//! no go: the player's confirm, an input payload's `ACCEPT_MESSAGE` (a recorded payload confirms at once), arms the 2 s
//! countdown (`FrameInput::confirm`). Without the timer (the autopilot
//! and the recorded-boot checks) the go comes before the first payload, as the headless boot gives it at the
//! recording's step; a lesson's brief still waits for the first payload's confirm. Each payload of the drive is one frame of one step, from the settle's last step.
//!
//! A payload with the call-for-help flag (quantized 0x20, the viewer's call for help) gives the game that request
//! (`FrameInput::help_call`) instead of the flag, so the race clock's gate applies; the assist and its penalty are
//! logged to stderr.
//!
//! A retire's message box (`MSG_RETIRE_BYSTAND`, `MSG_RETIRE_OFFSTAGE`) pauses the game (state 2): the payloads are
//! answered without a step, FLW1 says the box is up, until an input payload with `ACCEPT_MESSAGE` accepts it and the
//! race ends. A recorded payload (the autopilot, the recorded checks) accepts it at once: ours, a recorded drive has no
//! player to read the box.
//!
//! The viewer's pause menu (`rbr_gameplay::pause_menu`) pauses by sending nothing. Each answer ends with a pause-menu
//! record (MNU1, `rbr_recording::menu`, before SCN1): the mode, whether the game is driving with no box up, whether the
//! call for help is offered and whether the race is over. An input payload with `RETIRE` (the menu's confirmed
//! retire) ends the race retired on its frame, with no step.
//!
//! The race flow runs as in the headless run: past the finish line the car is still driven until the finish timer
//! (20 s) or the end-of-stage marker ends the race. From then on (the application state 9, the end page) no payload is
//! stepped: each is answered with the last frame, whose split record (SPL1) says the race is over and how it ended.
//!
//! A call the pure host cannot run stops the run. The stop goes to stderr once, and from then on every payload is
//! answered with the last good frame and HUD, so the car freezes and the viewer stays up. A payload that is not
//! quantized is a protocol error and ends the run.

use super::picture::dash;
use super::{Live, RallyNext};
use crate::physics::{chain, why};
use crate::rally;
use rbr_gameplay::flow::{EndPage, FlowEvents, Message, StageEnd};
use rbr_gameplay::markers::PaceCallout;
use rbr_gameplay::stage::State;
use rbr_recording::flow::{Flow, MessageBox};
use rbr_recording::menu::Menu;
use rbr_recording::school::{self, School};
use rbr_school::SchoolEvent;
use std::any::Any;

impl<'a> Live<'a> {
    /// The end page the race's end opens (`CApplication::M00480810`): `Test3B20` on the car as it is now, and the
    /// rally's last stage from `--rally-next`.
    pub(super) fn end_page(&self, end: &StageEnd) -> EndPage {
        let drivable = rally::drivable(&self.playback.pure.mem);
        EndPage::of(self.game.flow().mode(), end, drivable, self.rally_next == RallyNext::End)
    }

    /// The race's end: the log, and a finished race's times offered to the stage record and the personal best
    /// (`M00567330`, with `--saves`).
    pub(super) fn race_end(&mut self, step: u32, end: &StageEnd) {
        let page = self.end_page(end);
        eprintln!("step {step}: the race ended: {:?}, {:?}, end page {:#x}", end.cause, end.end_race, page.value());
        if let Some(school) = &self.school {
            // `EvalFilters` on the HUD's time and the car's damage; a lesson's drive keeps no stage record.
            let damage = rally::stage_out(&self.playback.pure.mem, *end, false).damage;
            let (lesson, verdict) = (school.lesson(), school.verdict(end.end_race.time, &damage));
            eprintln!("step {step}: rally school lesson {} verdict {verdict} of 4", lesson.index());
            self.school_verdict = Some(verdict);
            if let Some(saves) = self.saves.as_mut() {
                match saves.school(lesson, verdict) {
                    Ok(results) => eprintln!("step {step}: the school results are {results:?}"),
                    Err(e) => eprintln!("step {step}: the school result was not saved: {e}"),
                }
            }
            return;
        }
        let Some(saves) = self.saves.as_mut().filter(|s| end.finished() && !s.written()) else { return };
        let times = [end.end_race.splits[0], end.end_race.splits[1], end.end_race.time];
        match saves.finish(times) {
            Ok(update) => {
                let standing = saves.standing();
                eprintln!(
                    "step {step}: new stage record {}, new personal best {}; the record is {:.2} s by {:?}, the personal best {:?}",
                    update.record,
                    update.personal,
                    standing.record.time,
                    standing.record.holder,
                    standing.personal.map(|p| p.time),
                );
            }
            Err(e) => eprintln!("step {step}: the records were not saved: {e}"),
        }
    }

    /// Where the race stands for the viewer's pause menu (MNU1).
    pub(super) fn menu(&self) -> Menu {
        let flow = self.game.flow();
        let state = self.game.stage().state();
        let driving = state == State::Driving && flow.end().is_none() && flow.message().is_none();
        Menu {
            mode: flow.mode().value(),
            driving,
            help: driving && self.game.help_offered(),
            over: state == State::Ended,
        }
    }

    /// The school record (SCH1) after MNU1, only for a lesson's drive: the lesson, where it stands, the kept
    /// verdict (none before the end), the race time and the lesson's times, this answer's last incident code, the
    /// stop when the flow moved since the last answer, the ids `Trigger` queued this answer, and the failed end's
    /// reasons (only once the verdict is known and failed). Takes the answer's events, as the viewer's panels
    /// and speech read them.
    pub(super) fn school_record(&mut self) -> Option<Vec<u8>> {
        let school = self.school.as_mut()?;
        let events = school.take_events();
        let state = match self.game.stage().state() {
            State::Countdown => school::STATE_BRIEF,
            State::Ended => school::STATE_ENDED,
            _ => school::STATE_DRIVING,
        };
        let incident = events
            .iter()
            .rev()
            .find_map(|event| match event {
                SchoolEvent::Incident { code } => Some(*code as u32),
                _ => None,
            })
            .unwrap_or(0);
        let speech: Vec<u32> = events.iter().flat_map(|event| school.speech_ids(event)).collect();
        let stop = self.school_state != Some(state);
        self.school_state = Some(state);
        let verdict = self.school_verdict.unwrap_or(school::NO_VERDICT);
        let time = self.game.race_clock().time;
        let reasons = if state == school::STATE_ENDED && verdict == 0 {
            school
                .failure_reasons(time)
                .into_iter()
                .map(|reason| school::Reason { lesson: reason.lesson, key: reason.key })
                .collect()
        } else {
            Vec::new()
        };
        Some(
            School {
                lesson: school.lesson().index(),
                state,
                verdict,
                time,
                perfect: school.bank().perfect_time,
                pass: school.bank().pass_time,
                incident,
                stop,
                speech,
                reasons,
            }
            .record(),
        )
    }

    /// The FLW1 record of a frame: its events, and where the flow stands after it.
    pub(super) fn flow_record(&self, events: &FlowEvents) -> Vec<u8> {
        let flow = self.game.flow();
        let tints = flow.tints();
        Flow {
            helicopter_called: events.helicopter_called,
            rally_incident: events.rally_incident,
            finish_line: events.finish_line,
            ended: events.end.is_some(),
            page: events.end.map_or(0, |end| self.end_page(&end).value()),
            helicopter_active: flow.helicopter_active(),
            message: flow.message().map(|m| match m {
                Message::RetireBystand => MessageBox::RetireBystand,
                Message::RetireOffstage => MessageBox::RetireOffstage,
            }),
            service_next: self.rally_next == RallyNext::Service,
            red: tints.red,
            black: tints.black,
        }
        .to_bytes()
    }

    /// The dash record now (`dash`), with the step's signals and callouts.
    pub(super) fn dash(&self, signals: (Option<[u32; 2]>, Option<u32>), pace: &[&PaceCallout]) -> Vec<u8> {
        let jumped = self.game.race_clock().jumped;
        dash(
            &self.playback.pure.mem,
            &self.stage.track,
            &self.track,
            self.game.markers().keys(),
            signals,
            pace,
            self.game.assist_settings().false_start_warning(jumped),
            self.time_of_day,
        )
        .to_bytes()
    }

    /// Where a step's panic stopped the run: the step, the reason, the call chain and the bindings run in the step.
    pub(super) fn stop_text(&self, e: &(dyn Any + Send), runs: u64) -> String {
        format!(
            "step {}: {} in {}, after {} binding runs of the step",
            self.start + self.answered,
            why(e),
            chain(&self.playback.pure),
            self.playback.pure.runs() - runs
        )
    }
}
