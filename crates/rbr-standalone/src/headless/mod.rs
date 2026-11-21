//! The headless run (`--recording X --boot`): the gameplay loop from the recording's first
//! frame, the Rust physics from the image's step, and each step's car and wheels compared with the recording (the
//! crate's documentation describes the loop and the report).

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use rbr_counters::{Counts, Mark};
use rbr_formats::data::Data;
use rbr_formats::trk::DriveLineRecord;
use rbr_gameplay::control::{CarControl, QuantizedControl};
use rbr_gameplay::filter::Grid;
use rbr_gameplay::flow::{EndPage, RaceFlow, StageEnd};
use rbr_gameplay::game::{FrameInput, Game, PhysicsStep, StepSample};
use rbr_gameplay::ini::Ini;
use rbr_gameplay::markers::MarkerSettings;
use rbr_gameplay::race_clock::RaceClock;
use rbr_gameplay::result::StageResult;
use rbr_gameplay::stage::{Stage, State};
use rbr_gameplay::tick;
use rbr_recording::sound::TyreMax;
use rbr_recording::{Frame, CONTROL_BYTES};
use rbr_step::runner::{self, Compare, Playback};
use rbr_step::timekeeper::layout as keeper_layout;

use crate::args::Args;
use crate::boot;
use crate::physics::{boot_playback, chain, fit_physics, load_substeps, run_guarded, sample, why, Runner, DT};
use crate::stage::StageData;

mod report;
mod steer;
mod trace;

use report::report;
pub use trace::Hooks;
use trace::{Exact, Trace};

/// The load settle's `StepForward` count (state 0xd), which tells the settle frames in the recording.
const SETTLE_SUBSTEPS: u32 = tick::SETTLE_SUBSTEPS[0];

/// The original's physics as recorded: the frame's car block is the sample.
struct Recorded<'a> {
    car: &'a [u8],
    track: &'a [DriveLineRecord],
}

impl PhysicsStep for Recorded<'_> {
    fn step(&mut self, _: Option<CarControl>, _: u32, inside_step: &mut dyn FnMut(StepSample)) {
        inside_step(sample(self.car, self.track));
    }
}

/// One run.
#[derive(Default)]
pub struct Run {
    start: u32,
    /// What the boot built (`--boot`).
    built: Vec<(&'static str, u64)>,
    /// The physics step: the image's `MaxTimeStep`, or [`DT`] when the image has none (`dt_image` false).
    dt: f32,
    dt_image: bool,
    /// Steps run, and of them those whose car and wheels equal the recording.
    pub steps: usize,
    pub equal: usize,
    /// Wall time in the steps' `Game::frame` calls, and in the whole step loop (the steps and the compare).
    stepping: Duration,
    looping: Duration,
    /// Instructions, cycles and allocations in the steps' `Game::frame` calls.
    stepping_counts: Counts,
    /// The bindings the runner ran in the steps' `Game::frame` calls ([`rbr_step::runner::Runner::runs`]).
    binding_runs: u64,
    pointers: usize,
    allocator_pointers: usize,
    allocator_unwritten_reads: Vec<u32>,
    /// The run-noise candidates.
    noise: usize,
    /// Unmasked noise candidates released because either side changed.
    noise_released: usize,
    /// Noise candidates released because the recording changed while the run kept its start value.
    noise_released_by_recording: Vec<usize>,
    /// Those noise candidates outside the recording's static car-and-wheel mask.
    noise_unmasked: usize,
    /// Per-phase frame comparisons and physics work.
    phases: PhaseSummaries,
    /// Boot-noise candidates still holding the snapshot value after the synthetic countdown frames.
    noise_kept_after_countdown: Option<Vec<usize>>,
    /// Relocated boot pointers are reported apart from run noise.
    boot_pointer_candidates: usize,
    boot_pointer_candidates_unmasked: usize,
    boot_pointers_kept_after_countdown: Option<Vec<usize>>,
    /// Boot-noise candidates kept unchanged on every driving frame after the countdown.
    noise_kept_through_driving: Option<Vec<usize>>,
    /// Boot pointers still holding their relocated snapshot addresses through every driving frame.
    boot_pointers_kept_through_driving: Option<Vec<usize>>,
    /// Boot-noise candidates still holding the snapshot value when the run stopped.
    noise_kept_at_stop: Vec<usize>,
    /// Boot pointers still holding their relocated snapshot addresses when the run stopped.
    boot_pointers_kept_at_stop: Vec<usize>,
    /// The first step that differs: its step and frame words.
    first_diff: Option<(u32, Vec<usize>)>,
    /// The recording's and the run's words at the first differing step.
    first_diff_values: Vec<(usize, u32, u32)>,
    /// Every frame word that differs at some step: the first step it differs at and the number of steps.
    differing: BTreeMap<usize, (u32, usize)>,
    /// The stale-stack words (`runner::stale_mask`) that differ, with the steps each differs at. Counted apart: not failures.
    stale: BTreeMap<usize, usize>,
    /// The first step whose emitted control differs from the recorded payload: its step and word.
    control_diff: Option<(u32, usize)>,
    /// Where `Game`'s time card or clock first differs from the bound `CTimeKeeper`'s in `Mem`: at the boot (`None`)
    /// or after a step.
    keeper_diff: Option<(Option<u32>, String)>,
    /// `Game`'s application state and the image's at the boot, when they differ.
    state_diff: Option<(u32, u32)>,
    stop: Option<String>,
    /// Where the race flow ended the race, which ends the run (not a failure).
    ended: Option<String>,
    /// The outside-assistance emits (step, code), the penalties the race clock added (step, seconds; the jump start's
    /// apart) and the race clock after the last step.
    assists: Vec<(u32, u32)>,
    penalties: Vec<(u32, f32)>,
    jump_starts: Vec<(u32, f32)>,
    race_clock: Option<RaceClock>,
    /// The stage result after the last step.
    result: Option<StageResult>,
}

#[derive(Default)]
struct PhaseSummary {
    /// Recorded frame steps run by the runner.
    steps: usize,
    /// Of those, frame states equal to the recording.
    equal: usize,
    physics_substeps: usize,
    /// Synthetic `Game::frame` calls with no recording frame, such as the go countdown.
    no_step_game_frames: usize,
}

#[derive(Default)]
struct PhaseSummaries {
    settle: PhaseSummary,
    countdown: PhaseSummary,
    driving: PhaseSummary,
}

impl PhaseSummaries {
    fn get_mut(&mut self, state: State) -> Option<&mut PhaseSummary> {
        match state {
            State::Load => Some(&mut self.settle),
            State::Countdown => Some(&mut self.countdown),
            State::Driving => Some(&mut self.driving),
            State::Setup | State::Menu | State::Ended => None,
        }
    }
}

/// Where `Game`'s time card and clock differ from the bound keeper's and the physics clock in `mem`, field by field.
fn keeper_difference(game: &Game, mem: &rbr_physics::mem::Mem) -> Option<String> {
    let (ours, bound) = (game.time_card(), runner::time_keeper(mem).card);
    let mut out = vec![];
    let mut field = |name: &str, a: &dyn std::fmt::Debug, b: &dyn std::fmt::Debug| {
        let (a, b) = (format!("{a:?}"), format!("{b:?}"));
        if a != b {
            out.push(format!("{name} {a} (Game) vs {b} (Mem)"));
        }
    };
    field("start", &ours.start, &bound.start);
    field("finish", &ours.finish, &bound.finish);
    field("splits", &ours.splits(), &bound.splits());
    field("passed", &ours.passed, &bound.passed);
    field("cur", &ours.cur, &bound.cur);
    field("last", &ours.last, &bound.last);
    field("started", &ours.started, &bound.started);
    field("origin", &ours.origin, &bound.origin);
    field("finish_time", &ours.finish_time, &bound.finish_time);
    field("hold", &ours.hold, &bound.hold);
    field("clock", &game.elapsed(), &runner::clock(mem));
    (!out.is_empty() || ours != bound).then(|| out.join(", "))
}

/// A run's start: the image's playback, its step and that step's `StepForward` count, the compare, the image's step.
struct Start {
    playback: Playback,
    step: u32,
    substeps: u32,
    compare: Compare,
    dt: Option<f32>,
    built: Vec<(&'static str, u64)>,
}

/// The boot's start ([`boot_playback`]); its first `StepForward` count is the first recorded frame's.
fn boot_start(
    data: &Data,
    stage: &StageData,
    name: &str,
    race: &boot::Race,
    frames: &[Frame],
    mask: &[bool],
    dump: Option<&Path>,
) -> Result<Start, String> {
    let (playback, image) = boot_playback(data, stage, name, race, dump, true)?;
    let mem = playback.runner.image();
    let compare = Compare::at_start_of(mem, image.regions.clone(), mask.to_vec(), frames);
    let dt = keeper_layout::max_time_step(mem);
    Ok(Start {
        playback,
        step: 0,
        substeps: frames.first().ok_or("the recording has no frames")?.substeps,
        compare,
        dt: Some(dt),
        built: image.built,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    data: &Data,
    name: &str,
    race: &boot::Race,
    dump: Option<&Path>,
    frames: &[Frame],
    mask: &[bool],
    stage: &StageData,
    steps: usize,
    settings: MarkerSettings,
    hooks: Hooks,
) -> Result<Run, String> {
    let assist = race.assist_settings();
    let Start { mut playback, step: start, substeps: start_substeps, mut compare, dt: image_dt, built } =
        boot_start(data, stage, name, race, frames, mask, dump)?;
    compare.mask_ngp_padding();
    if let Some((profile, car)) = hooks.physics {
        fit_physics(&playback, profile, car)?;
    }
    let mut trace_out = hooks.trace_out.map(Trace::create).transpose()?;
    let mut steer_log = hooks.steer_breakdown.map(steer::SteerLog::create).transpose()?;
    playback.runner.keep_steer_torques = steer_log.is_some();
    // The first frame the Rust physics runs.
    let first = frames
        .iter()
        .position(|f| f.pre == start)
        .ok_or_else(|| format!("no recorded frame starts at step {start}"))?;
    // The camera's go arms the countdown after the load settle: at the first frame that is not a settle frame (the
    // frame headers' substeps).
    let go_frame = frames.iter().position(|f| f.substeps != SETTLE_SUBSTEPS).unwrap_or(frames.len());
    // A playback recording runs on 2 s past its last control without emitting and logs zeros for those steps: the run
    // stops at a trailing all-zero run instead of emitting them.
    let controls_end = frames.iter().rposition(|f| f.control != [0; CONTROL_BYTES]).map_or(frames.len(), |k| k + 1);
    let dt = image_dt.unwrap_or(DT);
    let values = stage.track_values.clone();
    let mut game = Game::new(
        Stage::quick_race(stage.number, stage.car).load_ready_with(load_substeps(playback.runner.image())),
        Grid::from_settings(&Ini::parse("")),
        &stage.markers,
        values,
        dt,
        0.0,
        settings,
    )
    .with_finish_zone(stage.finish_zone);
    game.set_assist_settings(assist);
    // A boot's rally-school lesson listens to the drive (`--school`).
    let mut school = race.school(&stage.replay, &stage.school_strings);
    let mut recorded = Recorded { car: &[], track: &stage.track };
    let mut physics = Runner {
        playback: &playback,
        track: &stage.track,
        own_step: true,
        word6: 0,
        substeps: 0,
        tyre_max: TyreMax::default(),
    };
    let mut out = Run {
        start,
        built,
        dt,
        dt_image: image_dt.is_some(),
        noise: compare.noise(),
        noise_unmasked: compare.kept_words(playback.runner.image()).iter().filter(|&&i| !mask[i]).count(),
        boot_pointer_candidates: compare.kept_boot_pointers(playback.runner.image()).len(),
        boot_pointer_candidates_unmasked: compare
            .kept_boot_pointers(playback.runner.image())
            .iter()
            .filter(|&&i| !mask[i])
            .count(),
        ..Run::default()
    };
    let mut now = 0.0f64;
    let mut noise_kept_through_driving: Option<BTreeSet<usize>> = None;
    let mut boot_pointers_kept_through_driving: Option<BTreeSet<usize>> = None;
    out.stop = Some("the recording ends".into());
    let looping = Instant::now();
    for (i, frame) in frames.iter().enumerate() {
        if i >= controls_end.max(go_frame) && i > first {
            out.stop = Some("the recorded controls end (a playback's tail logs zeros it never emitted)".into());
            break;
        }
        if i == go_frame {
            let go = FrameInput { camera_go: true, ..FrameInput::default() };
            // Neither frame steps; after the boot `Runner` is the physics all the same. The
            // second, where `Tic`'s countdown gives the driving, takes a step's time as in live play: the HUD ticks it
            // in the race state (`M004825b0` 0x4829e5), so the race clocks of both runs agree.
            for (input, frame_time) in [(go, 0.0), (FrameInput::default(), dt)] {
                now += f64::from(frame_time);
                if i > first {
                    let events = game.frame_with(now, input, &mut physics, &mut school);
                    if events.substeps.is_empty() {
                        if let Some(phase) = out.phases.get_mut(events.tick_state) {
                            phase.no_step_game_frames += 1;
                        }
                    }
                } else {
                    game.frame_with(now, input, &mut recorded, &mut school);
                }
            }
            if start == 0 && i > first {
                let kept = compare.kept_words(playback.runner.image());
                noise_kept_through_driving = Some(kept.iter().copied().collect());
                out.noise_kept_after_countdown = Some(kept);
                let pointers = compare.kept_boot_pointers(playback.runner.image());
                boot_pointers_kept_through_driving = Some(pointers.iter().copied().collect());
                out.boot_pointers_kept_after_countdown = Some(pointers);
            }
        }
        if i >= go_frame {
            now += f64::from(dt);
        }
        let words = frame.control_u32s();
        let replay_control = if i < go_frame {
            None
        } else {
            Some(
                QuantizedControl::from_payload(if hooks.recorded_payload {
                    trace::representable(words)
                } else {
                    words
                })
                .ok_or_else(|| format!("step {}: the recorded payload is not quantized", frame.pre))?,
            )
        };
        // What Game's control must equal: the payload, or with `--recorded-payload` the control its quantized bytes
        // give. An NGP live payload is unquantized and Game emits only quantized controls, so comparing it with the
        // recorded words would flag every step; the physics is fed the recorded words all the same (`Exact`).
        let expected = match replay_control.filter(|_| hooks.recorded_payload) {
            Some(q) => {
                let emitted = q.car_control().words();
                std::array::from_fn(|k| emitted[k].unwrap_or(words[k]))
            }
            None => words,
        };
        let input = FrameInput { replay_control, ..FrameInput::default() };
        if i < first {
            recorded.car = frame.car;
            game.frame_with(now, input, &mut recorded, &mut school);
            continue;
        }
        if i == first {
            out.keeper_diff = keeper_difference(&game, playback.runner.image()).map(|d| (None, d));
            let states = (game.stage().state().value(), runner::app_state(playback.runner.image()));
            out.state_diff = (states.0 != states.1).then_some(states);
        }
        if out.steps == steps {
            out.stop = None;
            break;
        }
        physics.word6 = words[6];
        physics.substeps = if i == first { start_substeps } else { frame.substeps };
        let runs = playback.runner.runs();
        let counting = Mark::now();
        let stepping = Instant::now();
        let extra = hooks.extra_payloads.and_then(|e| e.get(&frame.pre)).map_or(&[][..], Vec::as_slice);
        let mut exact = Exact { runner: &mut physics, words: hooks.recorded_payload.then_some(words), extra };
        if let Some(clock) = hooks.wall_clock {
            rbr_step::ngp::set_wall_clock(clock.get(&frame.pre).copied());
        }
        let events =
            match panic::catch_unwind(AssertUnwindSafe(|| game.frame_with(now, input, &mut exact, &mut school))) {
                Ok(events) => events,
                Err(e) => {
                    out.stop = Some(format!(
                        "step {} (frame {i}, {} substeps): {} in {}, after {} binding runs of the step",
                        frame.pre,
                        frame.substeps,
                        why(e.as_ref()),
                        chain(&playback.runner),
                        playback.runner.runs() - runs
                    ));
                    break;
                }
            };
        out.stepping += stepping.elapsed();
        out.stepping_counts += counting.elapsed();
        out.binding_runs += playback.runner.runs() - runs;
        out.steps += 1;
        out.assists.extend(events.substeps.iter().filter_map(|s| s.outside_assistance).map(|code| (frame.pre, code)));
        out.penalties.extend(events.clock_events.penalty.map(|p| (frame.pre, p)));
        out.jump_starts.extend(events.clock_events.jump_start.map(|p| (frame.pre, p)));
        out.race_clock = Some(events.race_clock);
        out.result = Some(game.result());
        // The fourth settle frame transitions the application state to Countdown
        // before `FrameEvents::tick_state` is sampled. Its 40 recorded substeps
        // still belong to settle; use the recording's settle boundary instead.
        let phase_state = if i < go_frame { State::Load } else { State::Driving };
        if let Some(phase) = out.phases.get_mut(phase_state) {
            phase.steps += 1;
            phase.physics_substeps += events.substeps.len();
        }
        if events.substeps.len() as u32 != frame.substeps {
            return Err(format!(
                "step {}: Game ran {} substeps, the recording {}",
                frame.pre,
                events.substeps.len(),
                frame.substeps
            ));
        }
        let control = events.substeps.iter().find_map(|s| s.control);
        if let Some(word) = control.and_then(|c| c.first_difference(expected)) {
            out.control_diff.get_or_insert((frame.pre, word));
        }
        let mem = playback.runner.image();
        if out.keeper_diff.is_none() {
            out.keeper_diff = keeper_difference(&game, mem).map(|d| (Some(frame.pre), d));
        }
        if let Some(log) = &mut steer_log {
            let controls: Vec<_> = events.substeps.iter().map(|s| s.control).collect();
            log.frame(mem, frame.pre, &playback.runner.take_steer_torques(), &controls)?;
        }
        if let Some(trace) = &mut trace_out {
            let state = game.stage().state();
            trace.write(frame, (!hooks.recorded_payload).then_some(control).flatten(), state, &hooks.ffb, mem)?;
        }
        let diff = compare.diff(mem, frame);
        out.pointers += diff.pointers;
        out.allocator_pointers += diff.allocator_pointers;
        for &i in &diff.stale {
            *out.stale.entry(i).or_default() += 1;
        }
        for &word in &diff.words {
            out.differing.entry(word).or_insert((frame.pre, 0)).1 += 1;
        }
        if diff.words.is_empty() {
            out.equal += 1;
            if let Some(phase) = out.phases.get_mut(phase_state) {
                phase.equal += 1;
            }
        } else if out.first_diff.is_none() {
            let want = runner::recorded_words(frame);
            let have = runner::frame_words(mem);
            out.first_diff_values = diff.words.iter().map(|&word| (word, want[word], have[word])).collect();
            out.first_diff = Some((frame.pre, diff.words));
        }
        // The race flow ends the run at the race's end (the finish timer or a retire), not the step count.
        if let Some(end) = game.flow().end() {
            out.stop = None;
            out.ended = Some(flow_end(game.flow(), &end, frame.pre));
            break;
        }
        if events.tick_state == State::Driving {
            if let Some(kept) = &mut noise_kept_through_driving {
                let still_kept: BTreeSet<usize> = compare.kept_words(mem).into_iter().collect();
                kept.retain(|i| still_kept.contains(i));
            }
            if let Some(kept) = &mut boot_pointers_kept_through_driving {
                let still_kept: BTreeSet<usize> = compare.kept_boot_pointers(mem).into_iter().collect();
                kept.retain(|i| still_kept.contains(i));
            }
        }
    }
    // Every frame run is not a stop: `--steps` past the recording's end runs it all.
    if out.stop.as_deref() == Some("the recording ends") && first + out.steps == frames.len() {
        out.stop = None;
    }
    out.looping = looping.elapsed();
    if let Some(trace) = trace_out {
        trace.finish()?;
    }
    if let Some(log) = steer_log {
        log.finish()?;
    }
    out.noise_kept_at_stop = compare.kept_words(playback.runner.image());
    out.noise_released = compare.released_noise_words().len();
    out.noise_released_by_recording = compare.recorded_noise_released_words();
    out.noise_kept_through_driving = noise_kept_through_driving.map(|kept| kept.into_iter().collect());
    out.boot_pointers_kept_at_stop = compare.kept_boot_pointers(playback.runner.image());
    out.boot_pointers_kept_through_driving = boot_pointers_kept_through_driving.map(|kept| kept.into_iter().collect());
    out.allocator_unwritten_reads = playback.runner.allocator_unwritten_reads();
    Ok(out)
}

/// How the race flow ended the race, and the run, at `step`.
fn flow_end(flow: &RaceFlow, end: &StageEnd, step: u32) -> String {
    format!(
        "the race ended at step {step} ({:?}, {}, race time {} s, end page {:#x})",
        end.cause,
        if end.finished() { "finished" } else { "retired" },
        end.end_race.time,
        EndPage::of(flow.mode(), end, true, false).value()
    )
}

/// Runs the recording `recording` from the boot's `race` and prints each run's report. Fails
/// when a run differs from the recording or stops.
pub fn run_all(args: &Args, data: &Data, stage: &StageData, race: &boot::Race, recording: &Path) -> ExitCode {
    let bytes = match std::fs::read(recording) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("{}: {e}", recording.display());
            return ExitCode::FAILURE;
        }
    };
    let frames: Vec<Frame> = match rbr_recording::read_all(&bytes) {
        Ok(frames) => frames,
        Err(e) => {
            eprintln!("{}: {e}", recording.display());
            return ExitCode::FAILURE;
        }
    };
    let mask = runner::static_mask(&frames);
    let car = crate::race::engine_car(args, Some(race), stage.car);
    let r = run_guarded("the run", || {
        let extra = match &args.extra_payloads {
            Some(path) => trace::read_extra(path)?,
            None => Default::default(),
        };
        let clock = args.wall_clock.as_deref().map(trace::read_wall_clock).transpose()?;
        let hooks = Hooks {
            recorded_payload: args.recorded_payload,
            extra_payloads: Some(&extra),
            wall_clock: clock.as_ref(),
            trace_out: args.trace_out.as_deref(),
            steer_breakdown: args.steer_breakdown.as_deref(),
            ffb: args.ffb_settings,
            physics: (!args.physics.is_ngp()).then_some((args.physics, &car)),
        };
        let dump = args.dump_image.as_deref();
        run(data, &args.stage, race, dump, &frames, &mask, stage, args.steps, args.marker_settings, hooks)
    });
    let ok = match r {
        Ok(r) => report("boot", &r, args, &mask, stage),
        Err(e) => {
            println!("boot: {e}");
            false
        }
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
