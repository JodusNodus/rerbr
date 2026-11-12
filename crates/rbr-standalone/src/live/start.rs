//! Starting a live run: from a snapshot ([`Live::new`]) or the boot image ([`Live::boot`]),
//! the load settle, and restarting it.
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

use super::picture::{damper_frame, dash, race_hud, split_record, Picture};
use super::{Live, Pending};
use crate::boot;
use crate::physics::{boot_playback, load_substeps, why, Pure, DT};
use crate::stage::StageData;
use rbr_formats::data::Data;
use rbr_gameplay::filter::Grid;
use rbr_gameplay::game::{FrameInput, Game};
use rbr_gameplay::ini::Ini;
use rbr_gameplay::markers::MarkerSettings;
use rbr_gameplay::stage::{Stage, State};
use rbr_gameplay::tick::SETTLE_SUBSTEPS;
use rbr_hybrid::pure::{self, Playback};
use rbr_hybrid::timekeeper::layout as keeper_layout;
use rbr_physics::snapshot::Snapshot;
use rbr_recording::clock::StepClock;
use rbr_recording::sound::TyreMax;
use rbr_recording::CONTROL_BYTES;
use rbr_school::SchoolRun;
use std::panic::{self, AssertUnwindSafe};

/// unless the stage is the rally's last (the season's stage index 5), and its text names the service or the next stage
/// (`CFailedSeasonFinishedWithBadCarUI::vf0c`, 0x5b6900).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RallyNext {
    #[default]
    Stage,
    Service,
    End,
}

impl RallyNext {
    pub fn parse(name: &str) -> Option<RallyNext> {
        match name {
            "stage" => Some(RallyNext::Stage),
            "service" => Some(RallyNext::Service),
            "end" => Some(RallyNext::End),
            _ => None,
        }
    }
}

impl<'a> Live<'a> {
    pub fn new(snap: &Snapshot, stage: &'a StageData, settings: MarkerSettings) -> Result<Self, String> {
        let h = snap.metadata();
        let playback = Playback::new(snap, false);
        crate::physics::require_ngp(&playback)?;
        let dt = pure::max_time_step(snap).unwrap_or(DT);
        let game = new_game(Stage::driving(stage.number, stage.car), stage, dt, settings);
        let settle = load_substeps(&playback.pure.mem).div_ceil(SETTLE_SUBSTEPS[0]) * SETTLE_SUBSTEPS[0];
        Ok(Self::with(playback, game, stage, StepClock { step: dt, settle }, h.step, Some(h.substeps), Vec::new()))
    }

    /// The boot's stepping (`--live --boot`): `race` on `name` from the stage start. The load settle runs here, as the
    /// headless boot runs it before its first driving frame. With `camera_go` the camera's start orbit gives the go
    /// during the payloads; without it the go is given here too.
    pub fn boot(
        data: &Data,
        name: &str,
        race: &boot::Race,
        stage: &'a StageData,
        settings: MarkerSettings,
        camera_go: bool,
    ) -> Result<Self, String> {
        let (playback, image) = boot_playback(data, stage, name, race, None, false)?;
        let built: Vec<String> = image.built.iter().map(|(what, n)| format!("{what} {n}")).collect();
        eprintln!("rbr-standalone: boot built {}", built.join(", "));
        let dt = keeper_layout::max_time_step(&playback.pure.mem);
        let mut game = new_game(
            Stage::quick_race(stage.number, stage.car).load_ready_with(load_substeps(&playback.pure.mem)),
            stage,
            dt,
            settings,
        );
        if camera_go {
            game = game.with_camera_go();
        }
        game.set_assist_settings(race.assist_settings());
        let mut school = race.school(&stage.replay, &stage.school_strings);
        // Without the camera, the go's two frames as the headless boot runs them: the second takes a step's time. A
        // lesson's brief waits for the first payload's confirm (`Stage::school_start`) instead, so its start, and the
        // ride's end at once, run through the answers as on the viewer.
        let go = !camera_go && !game.assist_settings().school();
        let now = if go { f64::from(dt) } else { 0.0 };
        let settled = settle(&playback, &mut game, &mut school, stage, go, now)?;
        let mut live =
            Self::with(playback, game, stage, StepClock { step: dt, settle: settled }, settled, None, image.detach);
        live.wetness = race.weather.wetness;
        live.now = now;
        live.boot_now = now;
        live.initial_school.clone_from(&school);
        live.school = school;
        Ok(live)
    }

    /// The race's wetness (`Weather::wetness`, 0 dry), for the test pilot's grip.
    pub fn wetness(&self) -> u32 {
        self.wetness
    }

    /// The first frame's records, before anything drove: the countdown repeats them.
    fn first_picture(playback: &Playback, game: &Game, stage: &StageData, detach: &[(u32, u32)]) -> Picture {
        let splits = split_record(game, stage, None);
        // Before any frame ran no reset has: the memory words stand as the step left them.
        let dampers = damper_frame(&playback.pure.mem, &[]);
        let mut last = Picture::of(
            &playback.pure.mem,
            race_hud(game.race_clock(), 0.0),
            &splits,
            stage.interactive_handle_end,
            &[],
            detach,
            dampers,
        );
        let track = stage.track_values.clone();
        // The countdown's frames repeat this one: its dash is the car's as it stands (not an empty record, whose
        // zero battery and engine would light the warnings).
        last.dash = dash(&playback.pure.mem, &stage.track, &track, game.markers().keys(), (None, None), &[], 0, None)
            .to_bytes();
        last
    }

    fn with(
        playback: Playback,
        game: Game,
        stage: &'a StageData,
        clock: StepClock,
        start: u32,
        own_substeps: Option<u32>,
        detach: Vec<(u32, u32)>,
    ) -> Self {
        let mut playback = playback;
        playback.pure.keep_tyre_particles = true;
        playback.pure.take_tyre_particles();
        playback.pure.take_particle_events();
        playback.pure.take_engine_fires();
        playback.pure.take_damper_signals();
        let initial = playback.pure.snapshot();
        let initial_game = game.clone();
        let last = Self::first_picture(&playback, &game, stage, &detach);
        let track = stage.track_values.clone();
        Self {
            playback,
            game,
            stage,
            now: 0.0,
            frame_seconds: f64::from(clock.step),
            clock,
            start,
            own_substeps,
            answered: 0,
            waited: 0,
            last,
            control: [0; CONTROL_BYTES],
            pending: Pending::default(),
            penalties: 0.0,
            wetness: 0,
            stopped: false,
            detach,
            track,
            time_of_day: None,
            initial_time_of_day: None,
            school: None,
            initial_school: None,
            school_verdict: None,
            school_state: None,
            saves: None,
            rally_next: RallyNext::default(),
            steer_log: None,
            aligning: None,
            initial,
            initial_game,
            boot_now: 0.0,
        }
    }

    /// Restarts the stage in process, as a fresh boot would start it: the memory and the settled game back to the
    /// stage start, and the race counters cleared. The player's setup (saves directory, profile, start clock,
    /// rally choice) stays, but the saves are re-armed (the next finish writes again, against the record as it now
    /// stands) and the start clock is restored to the stage's start.
    pub fn restart(&mut self) {
        self.playback.pure.restore(&self.initial);
        self.game = self.initial_game.clone();
        // The models' state is their own, not in the memory: back to the stage start's, the car's calibration kept.
        rbr_hybrid::ngp::restart_engine_model();
        rbr_hybrid::ngp::restart_tyre_model();
        self.school.clone_from(&self.initial_school);
        if let Some(saves) = self.saves.as_mut() {
            saves.rearm();
        }
        self.time_of_day = self.initial_time_of_day;
        self.playback.pure.take_tyre_particles();
        self.playback.pure.take_tyre_terms();
        self.playback.pure.take_particle_events();
        self.playback.pure.take_engine_fires();
        self.playback.pure.take_damper_signals();
        self.playback.pure.take_steer_torques();
        if let Some(aligning) = &mut self.aligning {
            aligning.restart();
        }
        self.now = self.boot_now;
        self.answered = 0;
        self.waited = 0;
        self.control = [0; CONTROL_BYTES];
        self.pending = Pending::default();
        self.penalties = 0.0;
        self.stopped = false;
        self.school_verdict = None;
        self.school_state = None;
        self.last = Self::first_picture(&self.playback, &self.game, self.stage, &self.detach);
    }
}

/// The game's `PC_InputFilter.ini` (empty without one: an RSF install renames it, so the built-in values apply).
pub(super) fn install_filter_ini() -> Ini {
    let text = Data::from_env()
        .and_then(|data| ["PC_InputFilter.ini", "Root/PC_InputFilter.ini"].into_iter().find_map(|f| data.resolve(f)))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    Ini::parse(&text)
}

/// The load settle the boot runs: the game from `Load` to the countdown, then with `go` the go's frames as the
/// headless boot runs them (the second at `now`, a step's time). Returns the settled steps, the live start's step.
fn settle(
    playback: &Playback,
    game: &mut Game,
    school: &mut Option<SchoolRun>,
    stage: &StageData,
    go: bool,
    now: f64,
) -> Result<u32, String> {
    let mut physics = Pure {
        playback,
        trace: None,
        track: &stage.track,
        own_step: true,
        word6: 0,
        substeps: SETTLE_SUBSTEPS[0],
        tyre_max: TyreMax::default(),
        twin: None,
    };
    let mut settled = 0;
    panic::catch_unwind(AssertUnwindSafe(|| {
        while game.stage().state() == State::Load {
            settled += game.frame_with(0.0, FrameInput::default(), &mut physics, school).substeps.len() as u32;
        }
        if go {
            let go = FrameInput { camera_go: true, ..FrameInput::default() };
            game.frame_with(0.0, go, &mut physics, school);
            game.frame_with(now, FrameInput::default(), &mut physics, school);
        }
    }))
    .map_err(|e| format!("the settle, step {settled}: {}", why(e.as_ref())))?;
    Ok(settled)
}

/// The game a live run steps: `stage` from `start`, with the input filter of the game's `PC_InputFilter.ini` (the
/// wheel without its speed fade, [`Grid::without_wheel_fade`]); a profile's grid replaces it
/// ([`Live::set_saves`]).
fn new_game(start: Stage, stage: &StageData, dt: f32, settings: MarkerSettings) -> Game {
    let values = stage.track_values.clone();
    let grid = Grid::from_settings(&install_filter_ini()).without_wheel_fade();
    let game = Game::new(start, grid, &stage.markers, values, dt, 0.0, settings).with_finish_zone(stage.finish_zone);
    if stage.btb {
        game.with_forced_jump_start()
    } else {
        game
    }
}
