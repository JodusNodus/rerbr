//! The autopilot's runs: `--autopilot`'s drive to a playback, and `--live --pilot`'s payloads.

use std::io::Write;
use std::path::Path;

use rbr_gameplay::control::QuantizedControl;
use rbr_gameplay::markers::MarkerSettings;
use rbr_hybrid::pure;
use rbr_physics::snapshot::Snapshot;
use rbr_recording::{read_u32, Frame, CONTROL_BYTES, HEADER_BYTES};

use super::*;
use crate::live::Live;
use crate::physics::payload;
use crate::stage::StageData;

/// 1.02 steps between the live pilot's progress lines (10 s).
const PILOT_REPORT: u32 = 1430;

/// `--live --pilot`: what drives a live run instead of the viewer's payloads (ours, for unattended live runs: the
/// viewer paces and draws them as it does a player's).
pub enum Pilot<'a> {
    /// Each step's control from a recording, by the step it feeds; the countdown's frames, which step nothing, take
    /// the first one, as `live::tests::a_paced_live_boot_with_the_recorded_controls_is_the_recording` feeds them.
    Recording(std::collections::HashMap<u32, [u8; CONTROL_BYTES]>),
    /// The controller, once a frame from the last answer: on the handbrake, revving in neutral, until the start
    /// countdown is out (a jump start would add its penalty), then along the driveline.
    Autopilot { driver: Driver<'a>, report: u32 },
}

impl<'a> Pilot<'a> {
    /// `autopilot` for `live`'s car, or the recording at `path`.
    pub fn new(what: &str, stage: &'a StageData, live: &Live) -> Result<Self, String> {
        if what == "autopilot" {
            let driver = Driver::new(&stage.track, stage.track_values.clone(), TOP_SPEED, None)
                .with_step(live.step_clock().step)
                .with_limit(rev_limit(live))
                .with_obstacles(&stage.obstacles);
            return Ok(Pilot::Autopilot { driver: with_lesson(driver, live, stage), report: 0 });
        }
        let bytes = std::fs::read(what).map_err(|e| format!("--pilot {what}: {e}"))?;
        let frames = rbr_recording::read_all(&bytes).map_err(|e| format!("--pilot {what}: {e}"))?;
        Ok(Pilot::Recording(frames.into_iter().map(|f| (f.pre, f.control)).collect()))
    }

    /// The payload for `live`'s next frame, after `last` (its last answer); `None` keeps the viewer's.
    pub fn payload(&mut self, live: &Live, last: Option<&[u8]>) -> Option<[u8; CONTROL_BYTES]> {
        match self {
            Pilot::Recording(controls) => controls.get(&live.next_step()).copied(),
            Pilot::Autopilot { driver, report } => {
                let q = if live.game().race_clock().start_pending(live.game().assist_settings()) {
                    QuantizedControl { handbrake: 255, throttle: 255, ..QuantizedControl::default() }
                } else if driver.gave_up.is_some() {
                    QuantizedControl { brake: 255, ..QuantizedControl::default() }
                } else {
                    // A lesson ends by the school, not the card (lesson 10's map has no finish, its card a finish time).
                    let finished = live.school().is_none() && pure::time_keeper(live.mem()).card.finish_time.is_some();
                    let (step, sense) = (live.next_step(), sense(last?, live));
                    if step >= *report {
                        *report = step + driver.steps(PILOT_REPORT);
                        eprintln!(
                            "pilot: step {step}: at {:.1} of {:.1} m, {:.1} m/s, gear {}, engine {:.0} rad/s",
                            distance(&driver.values, sense.parameter),
                            driver.values.last().unwrap_or(&0.0),
                            sense.pose.speed(),
                            sense.gear,
                            sense.engine
                        );
                    }
                    driver.control(step, &sense, finished)
                };
                Some(payload(&q.car_control(), 0))
            }
        }
    }
}

/// How an autopilot run ended.
pub struct Drove {
    pub start: u32,
    pub steps: u32,
    pub finish: Option<u32>,
    pub end: String,
    /// The stage's end for the rally flow, read after the last step.
    pub out: Option<crate::rally::StageOut>,
}

/// `--aim`'s coast: `steps` frames after `step` (the one the pure step stopped at), each `frame` (the last good step's,
/// without its HUD record) with its steps and the control `q` without gear pulses. Returns the frames written.
fn coast(
    file: &mut impl Write,
    frame: &[u8],
    q: &QuantizedControl,
    word6: u32,
    step: u32,
    steps: u32,
) -> std::io::Result<u32> {
    let control = payload(&QuantizedControl { flags: 0, ..*q }.car_control(), word6);
    let mut frame = frame.to_vec();
    frame[HEADER_BYTES..HEADER_BYTES + CONTROL_BYTES].copy_from_slice(&control);
    for k in 0..steps {
        frame[4..8].copy_from_slice(&(step + k).to_le_bytes());
        frame[8..12].copy_from_slice(&(step + k + 1).to_le_bytes());
        file.write_all(&frame)?;
    }
    Ok(steps)
}

/// The start of a snapshot run: the live stepping from the snapshot at `path`, the playback's head (`bytes`, the drive's
/// recording, up to the snapshot's step) and the snapshot step's recorded control.
pub fn from_snapshot<'a>(
    path: &str,
    bytes: &'a [u8],
    frames: &[Frame],
    stage: &'a StageData,
    settings: MarkerSettings,
) -> Result<(Live<'a>, &'a [u8], [u8; CONTROL_BYTES]), String> {
    let snap = Snapshot::open(path).map_err(|e| format!("{path}: {e}"))?;
    let start = snap.metadata().step;
    let first = frames
        .iter()
        .find(|f| f.pre == start)
        .ok_or_else(|| format!("{path}: no recorded frame starts at step {start}"))?;
    Ok((Live::new(&snap, stage, settings).map_err(|e| format!("{path}: {e}"))?, &bytes[..first.offset], first.control))
}

/// Drives `live` from its next step for at most `steps` steps, at most `top` m/s ([`TOP_SPEED`] if `None`), and writes
/// the playback to `out`: `head`, then the live steps. The first step takes `control`; the controller's payloads keep
/// its word 6.
#[allow(clippy::too_many_arguments)]
pub fn drive(
    mut live: Live,
    head: &[u8],
    mut control: [u8; CONTROL_BYTES],
    stage: &StageData,
    steps: u32,
    top: Option<f32>,
    aim: Option<Aim>,
    out: &Path,
) -> Result<Drove, String> {
    let start = live.next_step();
    let mut file = std::io::BufWriter::new(std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?);
    let failed = |e: std::io::Error| format!("{}: {e}", out.display());
    file.write_all(head).map_err(failed)?;

    eprintln!(
        "autopilot: {} m of driveline; start {:?}, finish {:?}, end {:?}",
        stage.track.last().map_or(0.0, |r| r.value),
        stage.markers.start(),
        stage.markers.finish(),
        stage.markers.end()
    );
    let limit = rev_limit(&live);
    eprintln!("autopilot: rev limit {limit} rpm");
    let mut driver = with_lesson(
        Driver::new(&stage.track, stage.track_values.clone(), top.unwrap_or(TOP_SPEED), aim)
            .with_step(live.step_clock().step)
            .with_limit(limit)
            .with_obstacles(&stage.obstacles),
        &live,
        stage,
    );
    eprintln!("autopilot: {} heavy props by the driveline", driver.props.len());
    let mut hit: Option<u32> = None;
    let mut last: Option<(Vec<u8>, QuantizedControl)> = None;
    let word6 = read_u32(&control, 24).expect("payload word 6");
    let mut drove = Drove { start, steps: 0, finish: None, end: format!("the step limit ({steps})"), out: None };
    let mut off_road = 0;
    for k in 0..steps {
        let step = start + k;
        let answer = live.answer(&control)?;
        if live.stopped() {
            drove.end = format!("the pure step stopped at step {step} (stderr)");
            if let (Some(aim), Some((frame, q))) = (aim, &last) {
                drove.steps += coast(&mut file, frame, q, word6, step, aim.coast).map_err(failed)?;
                drove.end += &format!(", then {} steps of its last control", aim.coast);
            }
            break;
        }
        file.write_all(&answer[..FRAME_BYTES]).map_err(failed)?;
        drove.steps += 1;
        // A lesson ends by the school or at the race's end, not the card (lesson 10's map has no finish, its card a
        // finish time).
        let school = live.school().is_some();
        let finished = !school && pure::time_keeper(live.mem()).card.finish_time.is_some();
        if finished && drove.finish.is_none() {
            drove.finish = Some(step);
        }
        if live.has_saves() || school {
            if live.game().stage().state() == rbr_gameplay::stage::State::Ended {
                drove.end = "the race's end".to_owned();
                break;
            }
        } else if drove.finish.is_some_and(|f| step >= f + driver.steps(AFTER_FINISH)) {
            drove.end = format!("{AFTER_FINISH} steps after the finish");
            break;
        }
        let sense = sense(&answer, &live);
        let speed = sense.pose.speed();
        let off = sub(sense.pose.position, point(driver.track, sense.parameter));
        let height = off[2];
        let off = off[0].hypot(off[1]);
        off_road = if off > OFF_ROAD { off_road + 1 } else { 0 };
        if let Some(why) = &driver.gave_up {
            drove.end = why.clone();
            break;
        }
        if off_road == driver.steps(OFF_ROAD_STEPS) && aim.is_none() {
            drove.end = format!(
                "off the road at step {step} (over {OFF_ROAD} m from the driveline for {OFF_ROAD_STEPS} steps)"
            );
            break;
        }
        if let Some(aim) = aim {
            let to = sub([aim.at[0], aim.at[1], sense.pose.position[2]], sense.pose.position);
            if hit.is_none() && to[0].hypot(to[1]) < AIM_HIT {
                eprintln!("autopilot: step {step}: within {AIM_HIT} m of the aim point");
                hit = Some(step);
            }
            if hit.is_some_and(|h| step >= h + driver.steps(AIM_AFTER)) {
                drove.end = format!("{AIM_AFTER} steps after reaching the aim point");
                break;
            }
        }
        // The drive waits on the handbrake until the start countdown is out (none in the rally school), as a jump start
        // would add its penalty (and its playback would take it again under `--live --pilot` or in the game), with the
        // engine revved in neutral (NGP's clutch help stalls a car put into first at idle, `notes/ngp.md`).
        let q = if live.game().race_clock().start_pending(live.game().assist_settings()) {
            QuantizedControl { handbrake: 255, throttle: 255, ..QuantizedControl::default() }
        } else {
            driver.control(step + 1, &sense, finished)
        };
        last = Some((answer[..FRAME_BYTES].to_vec(), q));
        if k % 500 == 0 {
            eprintln!(
                "autopilot: step {step}: at {:.1} of {:.1} m, {off:.1} m off the driveline, {speed:.1} m/s (target {:.1}), gear {}, engine {:.0} rad/s, steering {}, {height:.1} m above the driveline, at {:.0},{:.0},{:.1}",
                distance(&driver.values, sense.parameter),
                driver.values.last().unwrap(),
                driver.target_speed(distance(&driver.values, sense.parameter)),
                sense.gear,
                sense.engine,
                q.steering,
                sense.pose.position[0],
                sense.pose.position[1],
                sense.pose.position[2]
            );
        }
        control = payload(&q.car_control(), word6);
    }
    file.flush().map_err(failed)?;
    drove.out = Some(crate::rally::stage_out(live.mem(), live.game().end_now(), live.game().race_clock().jumped));
    Ok(drove)
}

/// `driver` for `live`'s rally school lesson: it holds the bank's judged speed bands (the checks with both speed
/// bounds, which trip the verdict outside them; lesson 1 cruises at 108 km/h through its 90 km/h band), and, if the
/// lesson has a stop box (its bank's first mode-4 sub, `SchoolIncident`'s standing test), it stops in the box,
/// keeping off the brakes where a no-brake sub (mode 6 on the brake word, flag 4: braking fails) watches, or runs
/// down the driveline to a box behind the start. Both the headless drive and the viewer's `--pilot autopilot` take
/// it.
fn with_lesson<'a>(driver: Driver<'a>, live: &Live, stage: &StageData) -> Driver<'a> {
    // The wet track-22_O hairpin at 3796 m holds 7.2 m/s dry and slides wide at 8.6 m/s wet.
    let grip = match live.wetness() {
        0 => 1.0,
        1 => 0.8,
        _ => 0.6,
    };
    let driver = driver.with_grip(grip);
    let Some(bank) = live.school().map(|s| s.bank()) else { return driver };
    let bands: Vec<SpeedBand> = bank
        .checks
        .iter()
        .filter(|c| c.hi != 0.0 && c.lo != 0.0)
        .map(|c| SpeedBand { from: c.pos, to: c.pos + c.span, hi: (c.hi - 2.0) * 1000.0 / 3600.0 })
        .collect();
    if !bands.is_empty() {
        eprintln!("autopilot: the lesson's judged speed bands: {bands:.1?}");
    }
    let driver = driver.with_bands(bands);
    let Some(stop) = bank.subs.iter().find(|sub| sub.tag == 4) else { return driver };
    let brake_from = bank
        .subs
        .iter()
        .filter(|sub| sub.tag == 6 && sub.flags & 4 != 0)
        .map(|sub| sub.center + sub.hi)
        .fold(0.0, f32::max);
    let behind = stage.markers.start().is_some_and(|start| stop.center < start);
    eprintln!(
        "autopilot: the lesson's stop box at {} m{}, no brakes before {brake_from} m",
        stop.center,
        if behind { " (behind the start)" } else { "" }
    );
    if behind {
        driver.with_stop_behind(stop.center)
    } else {
        driver.with_stop(brake_from, stop.center)
    }
}
