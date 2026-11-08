//! The command line (the crate's documentation describes the flags) and `--help`'s text.

use std::path::PathBuf;

use rbr_formats::{car_pack, rsf, stage_files};
use rbr_gameplay::markers::MarkerSettings;
use rbr_gameplay::race_options::{ChampionshipOptions, DamageLevel, DriverAids, Skill};
use rbr_school::Lesson;

use crate::profile::Profile;
use crate::{autopilot, live};

/// The command line. `--live` takes no value; the other flags take one.
#[derive(Default)]
pub struct Args {
    pub recording: Option<PathBuf>,
    pub live: bool,
    pub autopilot: Option<PathBuf>,
    /// `--pilot autopilot|RECORDING` (with `--live`): what drives instead of the viewer's payloads
    /// (`autopilot::Pilot`).
    pub pilot: Option<String>,
    /// `--frames-out FILE` (with `--live`): the clock's frames logged (`live::serve_realtime`); `--frames FILE|-`:
    /// frames from a log or stdin instead of the clock, each answered and a log's compared (`live::serve_frames`).
    pub frames_out: Option<PathBuf>,
    pub frames: Option<String>,
    /// `--replay-out FILE` (with `--live`): the clock's frames appended to a replay the viewer started
    /// (`rbr_recording::replay`); `--replay FILE`: a replay's frames instead of the clock, on the clock or back to back
    /// (`--replay-pace clock|free`, `live::serve_replay`), writing no files.
    pub replay_out: Option<PathBuf>,
    pub replay: Option<PathBuf>,
    pub replay_pace: live::Pace,
    pub top_speed: Option<f32>,
    pub aim: Option<autopilot::Aim>,
    /// `--flat-out` (with `--autopilot`): hold full throttle without braking for pace measurement (the steering
    /// and the gear shifts stay the controller's; `Driver::flat_out`).
    pub flat_out: bool,
    /// `--brake-from M/S` (with `--autopilot`): full foot brake once the speed reaches this, to a standstill
    /// (`Driver::brake_at`, a pace-measurement brake test; combine with `--flat-out`).
    pub brake_from: Option<f32>,
    /// `--brake-pedal P` (with `--brake-from`): the brake test holds this pedal (0-1, over 0) instead of full.
    pub brake_pedal: Option<f32>,
    /// `--brake-slip K` (with `--brake-from`, no `--brake-pedal`): threshold braking, the pedal modulated so the
    /// most-slipping wheel's slip ratio holds near K (0-1, over 0; `autopilot::Brake::Slip`).
    pub brake_slip: Option<f32>,
    /// `--brake-abs K` (with `--brake-from`, no `--brake-pedal` or `--brake-slip`): a per-wheel ABS, the full pedal
    /// with each wheel's pressure modulated so its slip ratio holds near K (0-1, over 0; `autopilot::Brake::Abs`).
    pub brake_abs: Option<f32>,
    /// `--brake-hold S` (with `--brake-from`): the brake stays on S seconds after the stop before the run ends.
    pub brake_hold: Option<f32>,
    /// `--brake-declutch` (with `--brake-from` and `--clutch-help off`): the brake test holds the clutch pedal in and
    /// shifts no gear, so the engine does not drive the braked wheels. NGP's clutch help sets the clutch from the
    /// engine speed in place of the pedal (`rbr_physics::vcu::clutch`), so the pedal needs it off.
    pub brake_declutch: bool,
    /// `--circle X,Y,R` (with `--autopilot`): drive the steady-state circle of centre X,Y and radius R (m),
    /// counter-clockwise, the target speed ramping up until the car lets go (`Driver::circle`).
    pub circle: Option<autopilot::Circle>,
    /// `--circle-ramp RATE` (with `--circle`): the circle target's rise in m/s per s (`CIRCLE_RAMP` unless given,
    /// for repeat runs at a gentler or steeper ramp).
    pub circle_ramp: Option<f32>,
    /// `--circle-past T` (with `--circle`, limit tests): past the circle's limit, hold throttle T (0-1) with no brake
    /// or gear change while the pursuit steers on (`autopilot::Driver::with_circle_past`).
    pub circle_past: Option<f32>,
    /// `--plan LAT,BRAKE` (with `--autopilot`, limit runs): the speed plan's dry lateral and braking acceleration in
    /// m/s² in place of the controller's own (`autopilot::Plan`).
    pub plan: Option<autopilot::Plan>,
    /// `--trace-speed FILE` (with `--autopilot`): each step's time, speed, pedals, gear, position and wheel speeds.
    pub trace_speed: Option<PathBuf>,
    /// `--tyre-probe FILE` (with `--autopilot`): read-only dumps of each wheel's tyre frame (research tooling).
    pub tyre_probe: Option<PathBuf>,
    /// `--tyre-probe-every N` (with `--tyre-probe`): a dump every N steps (`tyre_probe::EVERY` unless given).
    pub tyre_probe_every: Option<u32>,
    /// `--tyre-image FILE` (with `--autopilot`): a memory image of the wheels' tyre footprints at
    /// `--tyre-image-step N` (the final step unless given) for the transient lab (research tooling).
    pub tyre_image: Option<PathBuf>,
    pub tyre_image_step: Option<u32>,
    pub boot: bool,
    pub snapshots: Vec<String>,
    /// The stage's name or path (`stage_files`); for a community stage chosen by folder or id, filled in `main` once
    /// the weather picks its variant.
    pub stage: String,
    /// The community stage of `--stage-dir`, `--rsf-stage` or a `--stage` path in a stage folder.
    pub rsf: Option<rsf::Stage>,
    pub data: PathBuf,
    pub car: Option<u32>,
    pub steps: usize,
    pub read_set: Option<PathBuf>,
    pub marker_settings: MarkerSettings,
    pub dump_image: Option<PathBuf>,
    /// `--twin KIND` (headless runs): a second playback of the image, compared after every step (`twin.rs`).
    pub twin: Option<crate::twin::Kind>,
    pub weather: Option<u32>,
    pub tyre: Option<u32>,
    pub wear: Option<u32>,
    /// `--wetness dry|damp|wet`: the road's wetness over the weather row's (ours: the game takes the row's).
    pub wetness: Option<u32>,
    /// `--surface gravel|tarmac|snow`: the map's surface (`E14_4a1330`'s) overridden, as a stage that plays under another
    /// map's files (a BTB stage's `track.ini` `physics=`).
    pub surface: Option<&'static str>,
    /// `--btb MAP`: the stage is a BTB stage, a folder stage of its own number that plays the weather rows of retail
    /// map `MAP` (`Season::of_btb`) and starts with the jump start armed whatever its number.
    pub btb: Option<u32>,
    pub season_row: Option<usize>,
    pub rand_seed: Option<u32>,
    pub damage: Option<DamageLevel>,
    /// `--championship`: a championship stage (flow mode 3) on these options.
    pub championship: Option<ChampionshipOptions>,
    /// `--championship-false-starts`: the player's false starts so far in the rally.
    pub false_starts: u8,
    /// `--school N`: rally school lesson N (flow mode 7) on its map, with car 5 unless `--car`, `--rsf-car` or `--car-pack` gives
    /// another (`boot::Race::in_school`).
    pub lesson: Option<Lesson>,
    /// `--start-time`: the start board's clock (a championship stage's, or a quick race's time of day), s after midnight.
    pub start_time: Option<f32>,
    /// `--dispatch direct` (with `--live`, the default): the pure host runs the leaf calls of the entity walk, wheel,
    /// collision and rigid body in place (B1's Mem-direct dispatch, `notes/physics.md` "Store seam"); `pure`, the fact-check,
    /// makes every call through the host.
    pub direct: bool,
    /// `--rally-next`: what follows the championship stage.
    pub rally_next: Option<live::RallyNext>,
    pub auto_gears: Option<bool>,
    pub aids: DriverAids,
    pub setup: Option<PathBuf>,
    /// NGP's `physics.rbz` (`boot::Race::ngp_physics`).
    pub ngp_physics: Option<PathBuf>,
    /// `--rsf-car`: an RSF car (an id or pack name in `RBR_RSF`) under `--ngp-physics`, instead of a retail `--car`.
    pub rsf_car: Option<String>,
    /// A pack car under `--ngp-physics`, instead of a retail `--car` ([`car_pack::Flags`]). The launcher resolves
    /// every pack car to these; `--rsf-car` stays as the developer entry for an RSF install.
    pub pack_car: car_pack::Flags,
    /// `--ffb-sensitivity G,T,S`: NGP's force-feedback sensitivity percentages (`boot::Race::ffb_sensitivity`).
    pub ffb_sensitivity: Option<[i32; 3]>,
    /// `--head-gasket-durability N`: NGP's `headGasketDurability` (`boot::Race::head_gasket_durability`).
    pub head_gasket_durability: Option<i32>,
    /// `--material-as ID` (with `--autopilot --boot`): a test surface, every material of the stage takes material ID's
    /// values (`autopilot::Measure::material_as`; 116 ices the stage), with `--brake-from` from the brake onset.
    pub material_as: Option<u32>,
    /// The rally flow's files (`rally.rs`): the damage carried in and out, and the stage's result.
    pub damage_in: Option<PathBuf>,
    pub damage_out: Option<PathBuf>,
    pub result_out: Option<PathBuf>,
    /// The save files' directory and the profile in it (`saves.rs`): the stage record and the personal best.
    pub saves: Option<PathBuf>,
    pub profile: Option<String>,
    /// `--events-out`: the recording's particle and lamp records through the live boot (`events_out.rs`).
    pub events_out: Option<PathBuf>,
    /// `--recorded-payload` and `--trace-out` (headless runs, `headless/trace.rs`).
    pub recorded_payload: bool,
    pub extra_payloads: Option<PathBuf>,
    /// `--wall-clock FILE` (with `--recorded-payload`): the recording's wall clock per step for NGP's starter.
    pub wall_clock: Option<PathBuf>,
    pub trace_out: Option<PathBuf>,
    /// `--ffb-settings ENABLED,STRENGTH,INVERSE,DEAD_ZONE,THRESHOLD` (with `--trace-out`): the player's settings the
    /// trace's magnitude takes (`rbr_gameplay::ffb::Settings`; default on, 1, normal, no dead zone).
    pub ffb_settings: rbr_gameplay::ffb::Settings,
    /// `--physics ngp|experimental|experimental-brush`: the physics profile (`profile`); `ngp`, the default, steps NGP
    /// 7's as it is, `experimental` ReRBR experimental physics (built on NGP 7), `experimental-brush` it with the brush
    /// tyre (in development, not released).
    pub physics: Profile,
    /// `--car-group NAME` and `--car-year N`: the car's group and year for the engine model (`rbr_engine::Car`), over
    /// the RSF install's.
    pub car_group: Option<String>,
    pub car_year: Option<u32>,
}

impl Args {
    /// Whether the run writes its records (`--saves`: the stage record, the personal best, a lesson's verdict): not a
    /// replay (`--replay`), and not with ReRBR experimental physics (a user decision); the records are still
    /// compared.
    pub fn keeps_records(&self) -> bool {
        self.replay.is_none() && self.physics.is_ngp()
    }
}

/// `value` for `flag` as a number: the flag names a bad value.
fn parsed<T: std::str::FromStr>(flag: &str, value: &str) -> Result<T, String> {
    value.parse().map_err(|_| format!("{flag} {value}"))
}

/// A pedal or slip ratio: over 0, at most 1.
fn fraction(flag: &str, value: &str) -> Result<f32, String> {
    let x: f32 = parsed(flag, value)?;
    if x > 0.0 && x <= 1.0 {
        Ok(x)
    } else {
        Err(format!("{flag} {value}: over 0, at most 1"))
    }
}

/// An aid's `on` or `off`.
fn on_off(flag: &str, value: &str) -> Result<bool, String> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("{flag} {value}: on or off")),
    }
}

/// `--help`'s text.
pub const USAGE: &str = "usage: rbr-standalone --recording X --stage NAME --snapshot Y [--snapshot Z...] [--steps N] [--data DIR] [--read-set DIR]
       rbr-standalone --recording X --stage NAME --boot [--steps N] [--data DIR] [--dump-image FILE] [--events-out FILE] [--twin mem|mem-direct] [--recorded-payload [--extra-payloads FILE] [--wall-clock FILE]] [--trace-out FILE [--ffb-settings E,S,I,DZ,T]] [WEATHER]
       rbr-standalone --live --stage NAME (--snapshot Y | --boot [WEATHER]) [--car N] [--data DIR] [--callout-distance F] [--marker-threshold F]
       WEATHER: [--weather 0|1|2] [--tyre N] [--wear new|normal|worn] [--wetness dry|damp|wet] [--surface gravel|tarmac|snow] [--btb MAP] [--season-row N] [--rand-seed N] [--setup FILE]
       NGP (required with --boot): --ngp-physics FILE [--rsf-car ID|PACK | --car-pack DIR --car-model PATH [--car-bank PATH] [--car-key KEY]] [--ffb-sensitivity G,T,S] [--head-gasket-durability N]
       --physics ngp|experimental|experimental-brush: NGP 7's physics (the default), ReRBR experimental physics built on NGP 7 (its engine model in place of NGP's torque formula), or it with the brush tyre in development (in place of NGP's sliding model), on live, replayed, autopilot and headless runs, not with --twin (no effect without NGP) [--car-group NAME] [--car-year N] (the car's, over the RSF install's); experimental writes no records (--saves compares only)
       OPTIONS (with --boot): [--damage off|safe|reduced|realistic] [--auto-gears on|off] [--gear-guard on|off] [--neutral-lock on|off] [--clutch-help on|off] [--championship rookie|professional|champion [--championship-damage off|safe|reduced|realistic] [--championship-false-starts N]]
       a rally school lesson (with --boot): --school 0-16 (its map's stage unless --stage names it; the lesson's tyre; car 5 with the lesson's setup, or --car/--rsf-car/--car-pack with its own)
       rbr-standalone --autopilot OUT (--recording X --snapshot Y | --boot [--car N] [WEATHER]) --stage NAME [--steps N] [--top-speed M/S] [--aim X,Y[,COAST]] [--flat-out] [--brake-from M/S [--brake-pedal P | --brake-slip K | --brake-abs K] [--brake-hold S] [--brake-declutch (with --clutch-help off)]] [--circle X,Y,R [--circle-ramp RATE] [--circle-past T]] [--plan LAT,BRAKE (the speed plan's m/s²)] [--material-as ID (a test surface: every material takes ID's values, from the brake onset with --brake-from)] [--trace-speed FILE] [--tyre-probe FILE [--tyre-probe-every N]] [--tyre-image FILE [--tyre-image-step N]] [--data DIR]
       --stage NAME can be --stage-dir FOLDER or --rsf-stage ID [--rsf-root DIR] (a RallySimFans stage)
       with --live: [--frames-out FILE | --frames FILE|- | --replay-out FILE | --replay FILE [--replay-pace clock|free]] [--pilot autopilot|RECORDING] [--saves DIR] [--profile NAME] [--dispatch direct|pure] [--start-time H:MM]
       a championship stage (with --boot): [--rally-next stage|service|end] [--damage-in FILE] [--damage-out FILE] [--result-out FILE]
       -V, --version prints the version
       -h, --help prints this text";

pub fn args() -> Result<Args, String> {
    parse(std::env::args().skip(1))
}

/// `args()` of `argv`: the command line as `Args`.
fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    // The command line so far; the stage, the data and the championship options are computed below.
    let mut args = Args { steps: 30, direct: true, ..Args::default() };
    let mut stage: Option<String> = None;
    let (mut stage_dir, mut rsf_stage): (Option<PathBuf>, Option<u32>) = (None, None);
    let mut rsf_root = std::env::var_os("RBR_RSF").map(PathBuf::from);
    let mut data = std::env::var_os("RBR_DATA").map(PathBuf::from);
    let (mut skill, mut championship_damage): (Option<Skill>, Option<DamageLevel>) = (None, None);
    let mut false_starts: Option<u8> = None;
    let mut ffb_settings: Option<rbr_gameplay::ffb::Settings> = None;
    while let Some(flag) = it.next() {
        if flag == "--help" || flag == "-h" {
            println!("{USAGE}");
            std::process::exit(0);
        }
        if flag == "--version" || flag == "-V" {
            println!("rbr-standalone {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        if flag == "--live" || flag == "--boot" || flag == "--flat-out" {
            args.live |= flag == "--live";
            args.boot |= flag == "--boot";
            args.flat_out |= flag == "--flat-out";
            continue;
        }
        if flag == "--brake-declutch" {
            args.brake_declutch = true;
            continue;
        }
        if flag == "--recorded-payload" {
            args.recorded_payload = true;
            continue;
        }
        let value = it.next().ok_or(format!("{flag}: no value"))?;
        if args.pack_car.take(&flag, &value) {
            continue;
        }
        match flag.as_str() {
            "--recording" => args.recording = Some(PathBuf::from(value)),
            "--autopilot" => args.autopilot = Some(PathBuf::from(value)),
            "--pilot" => args.pilot = Some(value),
            "--frames-out" => args.frames_out = Some(PathBuf::from(value)),
            "--frames" => args.frames = Some(value),
            "--replay-out" => args.replay_out = Some(PathBuf::from(value)),
            "--replay" => args.replay = Some(PathBuf::from(value)),
            "--replay-pace" => {
                args.replay_pace = match value.as_str() {
                    "clock" => live::Pace::Clock,
                    "free" => live::Pace::Free,
                    _ => return Err(format!("--replay-pace {value}: clock or free")),
                }
            }
            "--dispatch" => {
                args.direct = match value.as_str() {
                    "pure" => false,
                    "direct" => true,
                    _ => return Err(format!("--dispatch {value}: pure or direct")),
                }
            }
            "--top-speed" => args.top_speed = Some(parsed("--top-speed", &value)?),
            "--brake-from" => args.brake_from = Some(parsed("--brake-from", &value)?),
            "--brake-pedal" => args.brake_pedal = Some(fraction("--brake-pedal", &value)?),
            "--brake-slip" => args.brake_slip = Some(fraction("--brake-slip", &value)?),
            "--brake-abs" => args.brake_abs = Some(fraction("--brake-abs", &value)?),
            "--brake-hold" => {
                args.brake_hold = Some(parsed("--brake-hold", &value).and_then(|s: f32| {
                    (s.is_finite() && s >= 0.0).then_some(s).ok_or(format!("--brake-hold {value}: seconds, 0 or more"))
                })?)
            }
            "--circle" => args.circle = Some(value.parse()?),
            "--circle-ramp" => args.circle_ramp = Some(parsed("--circle-ramp", &value)?),
            "--circle-past" => {
                args.circle_past = Some(parsed("--circle-past", &value).and_then(|t: f32| {
                    (0.0..=1.0).contains(&t).then_some(t).ok_or(format!("--circle-past {value}: a throttle, 0-1"))
                })?)
            }
            "--plan" => args.plan = Some(value.parse()?),
            "--trace-speed" => args.trace_speed = Some(PathBuf::from(value)),
            "--tyre-probe" => args.tyre_probe = Some(PathBuf::from(value)),
            "--tyre-probe-every" => args.tyre_probe_every = Some(parsed("--tyre-probe-every", &value)?),
            "--tyre-image" => args.tyre_image = Some(PathBuf::from(value)),
            "--tyre-image-step" => args.tyre_image_step = Some(parsed("--tyre-image-step", &value)?),
            "--aim" => args.aim = Some(value.parse()?),
            "--snapshot" => args.snapshots.push(value),
            "--stage" => stage = Some(value),
            "--stage-dir" => stage_dir = Some(PathBuf::from(value)),
            "--rsf-stage" => rsf_stage = Some(parsed("--rsf-stage", &value)?),
            "--rsf-root" => rsf_root = Some(PathBuf::from(value)),
            "--data" => data = Some(PathBuf::from(value)),
            "--car" => args.car = Some(parsed("--car", &value)?),
            "--read-set" => args.read_set = Some(PathBuf::from(value)),
            "--dump-image" => args.dump_image = Some(PathBuf::from(value)),
            "--twin" => args.twin = Some(crate::twin::Kind::parse(&value)?),
            "--steps" => args.steps = parsed("--steps", &value)?,
            "--weather" => args.weather = Some(parsed("--weather", &value)?),
            "--tyre" => args.tyre = Some(parsed("--tyre", &value)?),
            "--wear" => {
                args.wear = Some(
                    ["new", "normal", "worn"]
                        .iter()
                        .position(|w| *w == value)
                        .ok_or(format!("--wear {value}: new, normal or worn"))? as u32,
                )
            }
            "--wetness" => {
                args.wetness = Some(
                    ["dry", "damp", "wet"]
                        .iter()
                        .position(|w| *w == value)
                        .ok_or(format!("--wetness {value}: dry, damp or wet"))? as u32,
                )
            }
            "--surface" => {
                args.surface = Some(
                    ["gravel", "tarmac", "snow"]
                        .into_iter()
                        .find(|s| *s == value)
                        .ok_or(format!("--surface {value}: gravel, tarmac or snow"))?,
                )
            }
            "--btb" => args.btb = Some(parsed(&flag, &value)?),
            "--setup" => args.setup = Some(PathBuf::from(value)),
            "--ngp-physics" => args.ngp_physics = Some(PathBuf::from(value)),
            "--rsf-car" => args.rsf_car = Some(value),
            "--car-group" => args.car_group = Some(value),
            "--car-year" => args.car_year = Some(parsed("--car-year", &value)?),
            "--physics" => {
                let names = Profile::ALL.map(|p| p.name).join(", ");
                args.physics = Profile::named(&value).ok_or(format!("--physics {value}: one of {names}"))?
            }
            "--ffb-sensitivity" => {
                args.ffb_sensitivity =
                    Some(percentages(&value).ok_or(format!("--ffb-sensitivity {value}: three percentages, G,T,S"))?)
            }
            "--head-gasket-durability" => {
                args.head_gasket_durability =
                    Some(value.parse().map_err(|_| format!("--head-gasket-durability {value}: an integer"))?)
            }
            "--material-as" => {
                args.material_as = Some(value.parse().map_err(|_| format!("--material-as {value}: a material id"))?)
            }
            "--damage-in" => args.damage_in = Some(PathBuf::from(value)),
            "--damage-out" => args.damage_out = Some(PathBuf::from(value)),
            "--result-out" => args.result_out = Some(PathBuf::from(value)),
            "--saves" => args.saves = Some(PathBuf::from(value)),
            "--profile" => args.profile = Some(value.to_owned()),
            "--events-out" => args.events_out = Some(PathBuf::from(value)),
            "--trace-out" => args.trace_out = Some(PathBuf::from(value)),
            "--ffb-settings" => {
                ffb_settings = Some(ffb_settings_of(&value).ok_or(format!(
                    "--ffb-settings {value}: ENABLED,STRENGTH,INVERSE,DEAD_ZONE,THRESHOLD (0 or 1, then numbers)"
                ))?)
            }
            "--extra-payloads" => args.extra_payloads = Some(PathBuf::from(value)),
            "--wall-clock" => args.wall_clock = Some(PathBuf::from(value)),
            "--season-row" => args.season_row = Some(parsed("--season-row", &value)?),
            "--rand-seed" => args.rand_seed = Some(parsed("--rand-seed", &value)?),
            "--damage" => {
                args.damage = Some(
                    DamageLevel::from_name(&value)
                        .ok_or(format!("--damage {value}: off, safe, reduced or realistic"))?,
                )
            }
            "--championship" => {
                skill = Some(
                    Skill::from_name(&value)
                        .ok_or(format!("--championship {value}: rookie, professional or champion"))?,
                )
            }
            "--championship-damage" => {
                championship_damage = Some(
                    DamageLevel::from_name(&value)
                        .ok_or(format!("--championship-damage {value}: off, safe, reduced or realistic"))?,
                )
            }
            "--championship-false-starts" => {
                false_starts =
                    Some(value.parse::<u8>().map_err(|e| format!("--championship-false-starts {value}: {e}"))?)
            }
            "--school" => {
                args.lesson = Some(
                    value.parse().ok().and_then(Lesson::new).ok_or(format!("--school {value}: a lesson, 0 to 16"))?,
                )
            }
            "--start-time" => args.start_time = Some(clock_time(&value).ok_or(format!("--start-time {value}: H:MM"))?),
            "--rally-next" => {
                args.rally_next =
                    Some(live::RallyNext::parse(&value).ok_or(format!("--rally-next {value}: stage, service or end"))?)
            }
            "--auto-gears" => args.auto_gears = Some(on_off(&flag, &value)?),
            "--gear-guard" => args.aids.gear_guard = on_off(&flag, &value)?,
            "--neutral-lock" => args.aids.neutral_lock = on_off(&flag, &value)?,
            "--clutch-help" => args.aids.clutch_help = on_off(&flag, &value)?,
            "--callout-distance" => args.marker_settings.callout_distance = parsed("--callout-distance", &value)?,
            "--marker-threshold" => args.marker_settings.marker_threshold = parsed("--marker-threshold", &value)?,
            _ => return Err(format!("unknown argument {flag}")),
        }
    }
    match (args.live, args.recording.is_some(), args.snapshots.len()) {
        _ if args.boot && !args.snapshots.is_empty() => return Err("--boot takes no --snapshot".into()),
        (true, true, _) => return Err("--live takes no --recording".into()),
        (true, false, 1) => {}
        (true, false, 0) if args.boot => {}
        (true, false, _) => return Err("--live takes one --snapshot".into()),
        (false, false, _) if !(args.boot && args.autopilot.is_some()) => return Err("--recording is required".into()),
        (false, false, _) => {}
        (false, true, _) => {}
    }
    if args.rsf_car.is_some() && args.car.is_some() {
        return Err("--rsf-car replaces --car".into());
    }
    if args.pack_car.pack.is_some() && args.car.is_some() {
        return Err("--car-pack replaces --car".into());
    }
    args.pack_car.check(args.rsf_car.is_some())?;
    if args.autopilot.is_some() && (args.live || args.snapshots.len() != usize::from(!args.boot)) {
        return Err("--autopilot takes one --snapshot or --boot, and no --live".into());
    }
    // Flags that only make sense with `--boot`, in the order their errors used to read.
    for (flag, present) in [("--setup", args.setup.is_some()), ("--ngp-physics", args.ngp_physics.is_some())] {
        if present && !args.boot {
            return Err(format!("{flag} needs --boot"));
        }
    }
    if args.boot && args.ngp_physics.is_none() {
        return Err("--boot needs --ngp-physics FILE (NGP's physics.rbz): ReRBR steps NGP physics only".into());
    }
    for (flag, present) in [
        ("--rsf-car", args.rsf_car.is_some()),
        ("--car-pack", args.pack_car.pack.is_some()),
        ("--ffb-sensitivity", args.ffb_sensitivity.is_some()),
        ("--head-gasket-durability", args.head_gasket_durability.is_some()),
        ("--material-as", args.material_as.is_some()),
        ("--damage-in", args.damage_in.is_some()),
        ("--championship", skill.is_some()),
    ] {
        if present && !args.boot {
            return Err(format!("{flag} needs --boot"));
        }
    }
    if (args.damage_out.is_some() || args.result_out.is_some()) && args.autopilot.is_none() && !(args.live && args.boot)
    {
        return Err("--damage-out and --result-out need --autopilot or --live --boot".into());
    }
    if (championship_damage.is_some() || false_starts.is_some() || args.rally_next.is_some()) && skill.is_none() {
        return Err("--championship-damage, --championship-false-starts and --rally-next need --championship".into());
    }
    if let Some(lesson) = args.lesson {
        if !args.boot {
            return Err("--school needs --boot".into());
        }
        for (flag, present) in [
            ("--championship", skill.is_some()),
            ("--setup", args.setup.is_some()),
            ("--tyre", args.tyre.is_some()),
            ("--surface", args.surface.is_some()),
            ("--stage-dir", stage_dir.is_some()),
            ("--rsf-stage", rsf_stage.is_some()),
        ] {
            if present {
                return Err(format!("--school takes no {flag} (the lesson sets it)"));
            }
        }
        // The lesson's own car unless another is given (ours: the original drives car 5 only).
        if args.rsf_car.is_none() && args.pack_car.pack.is_none() {
            args.car = Some(args.car.unwrap_or(rbr_school::lesson::CAR));
        }
        let map = lesson.map();
        match &stage {
            Some(name) if stage_files::parts(name).is_none_or(|(number, _)| number != map) => {
                return Err(format!("--school {}: lesson's stage is map {map}, not {name}", lesson.index()));
            }
            Some(_) => {}
            None => stage = Some(format!("track-{map}_M")),
        }
    }
    let championship = skill.map(|skill| ChampionshipOptions {
        skill,
        damage: championship_damage.unwrap_or(ChampionshipOptions::default().damage),
    });
    if args.events_out.is_some() && (args.live || !args.boot || args.recording.is_none() || args.autopilot.is_some()) {
        return Err("--events-out needs --boot and --recording, and no --live or --autopilot".into());
    }
    if (args.recorded_payload || args.trace_out.is_some())
        && (args.live || args.recording.is_none() || args.autopilot.is_some())
    {
        return Err("--recorded-payload and --trace-out need --recording, and no --live or --autopilot".into());
    }
    if ffb_settings.is_some() && args.trace_out.is_none() {
        return Err("--ffb-settings needs --trace-out".into());
    }
    if (args.extra_payloads.is_some() || args.wall_clock.is_some()) && !args.recorded_payload {
        return Err("--extra-payloads and --wall-clock need --recorded-payload".into());
    }
    if args.twin.is_some() && !args.physics.is_ngp() {
        return Err("--twin takes no --physics experimental (the twin would step the run's models again)".into());
    }
    if args.pilot.is_some() && !args.live {
        return Err("--pilot needs --live".into());
    }
    if (args.frames_out.is_some() || args.frames.is_some()) && !args.live {
        return Err("--frames-out and --frames need --live".into());
    }
    if args.frames.is_some() && args.frames_out.is_some() {
        return Err("--frames takes no --frames-out (the frames are given)".into());
    }
    if (args.replay_out.is_some() || args.replay.is_some()) && !args.live {
        return Err("--replay-out and --replay need --live".into());
    }
    let logs = [args.frames_out.is_some(), args.frames.is_some(), args.replay_out.is_some(), args.replay.is_some()];
    if logs.into_iter().filter(|&on| on).count() > 1 {
        return Err("one of --frames-out, --frames, --replay-out and --replay".into());
    }
    if args.replay.is_some() && args.pilot.is_some() {
        return Err("--replay takes no --pilot (the replay drives)".into());
    }
    if args.aim.is_some() && args.autopilot.is_none() {
        return Err("--aim needs --autopilot".into());
    }
    if (args.flat_out || args.trace_speed.is_some() || args.tyre_probe.is_some() || args.tyre_image.is_some())
        && args.autopilot.is_none()
    {
        return Err("--flat-out, --trace-speed, --tyre-probe and --tyre-image need --autopilot".into());
    }
    if args.tyre_probe_every.is_some_and(|n| n == 0) || (args.tyre_probe_every.is_some() && args.tyre_probe.is_none()) {
        return Err("--tyre-probe-every needs --tyre-probe and a count over 0".into());
    }
    if args.tyre_image_step.is_some() && args.tyre_image.is_none() {
        return Err("--tyre-image-step needs --tyre-image".into());
    }
    if args.material_as.is_some() && args.autopilot.is_none() {
        return Err("--material-as needs --autopilot".into());
    }
    if args.circle.is_some() && args.autopilot.is_none() {
        return Err("--circle needs --autopilot".into());
    }
    if args.circle_ramp.is_some() && args.circle.is_none() {
        return Err("--circle-ramp needs --circle".into());
    }
    if args.circle_past.is_some() && args.circle.is_none() {
        return Err("--circle-past needs --circle".into());
    }
    if args.brake_from.is_some() && args.autopilot.is_none() {
        return Err("--brake-from needs --autopilot".into());
    }
    if (args.brake_pedal.is_some()
        || args.brake_slip.is_some()
        || args.brake_abs.is_some()
        || args.brake_hold.is_some()
        || args.brake_declutch)
        && args.brake_from.is_none()
    {
        return Err(
            "--brake-pedal, --brake-slip, --brake-abs, --brake-hold and --brake-declutch need --brake-from".into()
        );
    }
    if args.brake_declutch && args.aids.clutch_help {
        return Err(
            "--brake-declutch needs --clutch-help off (clutch help sets the clutch in place of the pedal)".into()
        );
    }
    if args.brake_pedal.is_some() && args.brake_slip.is_some() {
        return Err("--brake-slip takes no --brake-pedal (it moves the pedal itself)".into());
    }
    if args.brake_abs.is_some() && (args.brake_pedal.is_some() || args.brake_slip.is_some()) {
        return Err("--brake-abs takes no --brake-pedal or --brake-slip (it holds the full pedal)".into());
    }
    if (args.flat_out || args.circle.is_some() || args.brake_from.is_some()) && args.lesson.is_some() {
        return Err("--flat-out, --circle and --brake-from take no --school (a lesson brakes and stops)".into());
    }
    if args.circle.is_some() && args.aim.is_some() {
        return Err("--circle takes no --aim (it steers the circle itself)".into());
    }
    if args.plan.is_some() && args.autopilot.is_none() {
        return Err("--plan needs --autopilot".into());
    }
    if args.plan.is_some() && (args.flat_out || args.circle.is_some()) {
        return Err("--plan takes no --flat-out or --circle (they plan no speed)".into());
    }
    let rsf = match (&stage, stage_dir, rsf_stage) {
        (None, Some(dir), None) => Some(rsf::open(&dir)?),
        (None, None, Some(id)) => Some(rsf::find(&rsf_root.ok_or("--rsf-stage needs --rsf-root or RBR_RSF")?, id)?),
        (Some(name), None, None) => match rsf::folder_id(name) {
            Some(_) => Some(rsf::open(stage_files::folder(name).expect("a folder stage"))?),
            None => None,
        },
        (None, None, None) => return Err("--stage, --stage-dir or --rsf-stage is required".into()),
        _ => return Err("give one of --stage, --stage-dir and --rsf-stage".into()),
    };
    if let Some(problem) = rsf.as_ref().and_then(|s| s.problem.as_ref()) {
        return Err(format!("RSF stage {}: {problem}", rsf.as_ref().map_or(0, |s| s.id)));
    }
    if rsf.is_some() && stage.is_none() && !args.boot {
        return Err("--stage-dir and --rsf-stage need --boot (the weather picks the variant)".into());
    }
    Ok(Args {
        stage: stage.unwrap_or_default(),
        rsf,
        data: data.ok_or("--data or RBR_DATA is required")?,
        championship,
        false_starts: false_starts.unwrap_or(0),
        ffb_settings: ffb_settings.unwrap_or_default(),
        ..args
    })
}

/// `--ffb-settings`' five values.
fn ffb_settings_of(value: &str) -> Option<rbr_gameplay::ffb::Settings> {
    let v: Vec<f32> = value.split(',').map(|v| v.trim().parse().ok()).collect::<Option<_>>()?;
    let flag = |x: f32| (x == 0.0 || x == 1.0).then_some(x == 1.0);
    match v[..] {
        [enabled, strength, inverse, dead_zone, dead_zone_threshold] => Some(rbr_gameplay::ffb::Settings {
            enabled: flag(enabled)?,
            strength,
            inverse: flag(inverse)?,
            dead_zone,
            dead_zone_threshold,
        }),
        _ => None,
    }
}

/// `G,T,S` as three whole percentages.
fn percentages(value: &str) -> Option<[i32; 3]> {
    let values: Vec<i32> = value.split(',').map(|v| v.trim().parse().ok()).collect::<Option<_>>()?;
    values.try_into().ok()
}

/// `H:MM` as s after midnight.
fn clock_time(value: &str) -> Option<f32> {
    let (hour, minute) = value.split_once(':')?;
    let (hour, minute) = (hour.parse::<u32>().ok()?, minute.parse::<u32>().ok()?);
    (hour < 24 && minute < 60).then_some((hour * 3600 + minute * 60) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::Mutex;

    #[test]
    fn a_start_time_reads_hours_and_minutes() {
        assert_eq!(clock_time("9:36"), Some(34560.0));
        assert_eq!([clock_time("24:00"), clock_time("9:60"), clock_time("936")], [None; 3]);
    }

    /// `RBR_DATA`/`RBR_RSF` held at `data`/`rsf` for the call, restored after.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        data: Option<OsString>,
        rsf: Option<OsString>,
    }

    impl EnvGuard {
        fn hold(data: Option<&str>, rsf: Option<&str>) -> Self {
            let guard = EnvGuard { data: std::env::var_os("RBR_DATA"), rsf: std::env::var_os("RBR_RSF") };
            match data {
                Some(v) => std::env::set_var("RBR_DATA", v),
                None => std::env::remove_var("RBR_DATA"),
            }
            match rsf {
                Some(v) => std::env::set_var("RBR_RSF", v),
                None => std::env::remove_var("RBR_RSF"),
            }
            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.data.take() {
                Some(v) => std::env::set_var("RBR_DATA", v),
                None => std::env::remove_var("RBR_DATA"),
            }
            match self.rsf.take() {
                Some(v) => std::env::set_var("RBR_RSF", v),
                None => std::env::remove_var("RBR_RSF"),
            }
        }
    }

    fn run(argv: &[&str], data: Option<&str>, rsf: Option<&str>) -> Result<Args, String> {
        let _lock = ENV_LOCK.lock().unwrap();
        let _env = EnvGuard::hold(data, rsf);
        parse(argv.iter().map(|s| s.to_string()))
    }

    /// `run` of a whitespace-separated command line (no value holds a space).
    fn run_str(argv: &str, data: Option<&str>) -> Result<Args, String> {
        run(&argv.split_whitespace().collect::<Vec<_>>(), data, None)
    }

    /// The modes' bases: the recording's, the boot's and the live run's.
    const REC: &str = "--recording R --stage track-54_E --snapshot S --data D";
    /// The boot reading the recording (twin runs, the events run).
    const BOOT_REC: &str = "--boot --recording R --stage track-54_E --data D --ngp-physics N";
    const BOOT: &str = "--boot --stage track-54_E --data D --ngp-physics N --autopilot OUT";
    const LIVE: &str = "--live --stage track-54_E --snapshot S --data D";

    fn recording(a: &Args) {
        assert_eq!(a.recording, Some(PathBuf::from("R")));
        assert_eq!(a.stage, "track-54_E");
        assert_eq!(a.snapshots, ["S"]);
        assert_eq!(a.data, PathBuf::from("D"));
        assert!(!a.live && !a.boot && !a.recorded_payload && a.direct);
        assert_eq!(a.steps, 30);
        assert_eq!(a.replay_pace, live::Pace::Clock);
        assert_eq!(a.false_starts, 0);
        assert_eq!(a.damage, None);
        assert_eq!(a.championship, None);
    }

    fn boot(a: &Args) {
        assert!(a.boot && !a.live);
        assert_eq!(a.stage, "track-54_E");
        assert_eq!(a.ngp_physics, Some(PathBuf::from("N")));
        assert_eq!(a.car, None);
        assert_eq!(a.data, PathBuf::from("D"));
    }

    fn live(a: &Args) {
        assert!(a.live && !a.boot);
        assert_eq!(a.snapshots, ["S"]);
    }

    /// A good command line and its checks on the `Args`.
    type Good = (String, fn(&Args));

    /// Every flag's good values, off the mode's base.
    #[test]
    fn good_flags_parse_as_before() {
        let good: Vec<Good> = vec![
            (REC.into(), recording),
            (BOOT.into(), boot),
            (LIVE.into(), live),
            (format!("{REC} --autopilot OUT"), |a| assert_eq!(a.autopilot, Some(PathBuf::from("OUT")))),
            (format!("{LIVE} --pilot autopilot"), |a| assert_eq!(a.pilot.as_deref(), Some("autopilot"))),
            (LIVE.into(), |a| assert_eq!(a.physics, Profile::NGP)),
            (format!("{LIVE} --physics experimental"), |a| assert_eq!(a.physics, Profile::EXPERIMENTAL)),
            (format!("{BOOT} --physics ngp"), |a| assert_eq!(a.physics, Profile::NGP)),
            (format!("{REC} --physics experimental"), |a| assert_eq!(a.physics, Profile::EXPERIMENTAL)),
            (LIVE.into(), |a| assert!(a.keeps_records())),
            (format!("{LIVE} --physics ngp"), |a| assert!(a.keeps_records())),
            (format!("{LIVE} --physics experimental"), |a| assert!(!a.keeps_records())),
            (format!("{LIVE} --replay F"), |a| assert!(!a.keeps_records())),
            (format!("{LIVE} --car-group Rally_2 --car-year 2019"), |a| {
                assert_eq!((a.car_group.as_deref(), a.car_year), (Some("Rally_2"), Some(2019)))
            }),
            (format!("{REC} --top-speed 12.5"), |a| assert_eq!(a.top_speed, Some(12.5))),
            (format!("{REC} --autopilot OUT --aim 1,2"), |a| {
                assert_eq!(a.aim, Some(autopilot::Aim { at: [1.0, 2.0], coast: 0 }))
            }),
            (format!("{REC} --autopilot OUT --aim 1,2,7"), |a| {
                assert_eq!(a.aim, Some(autopilot::Aim { at: [1.0, 2.0], coast: 7 }))
            }),
            (format!("{BOOT} --flat-out --trace-speed T"), |a| {
                assert!(a.flat_out);
                assert_eq!(a.trace_speed, Some(PathBuf::from("T")));
            }),
            (format!("{BOOT} --tyre-probe P"), |a| {
                assert_eq!(a.tyre_probe, Some(PathBuf::from("P")));
                assert_eq!(a.tyre_probe_every, None);
            }),
            (format!("{BOOT} --tyre-probe P --tyre-probe-every 10"), |a| assert_eq!(a.tyre_probe_every, Some(10))),
            (format!("{BOOT} --tyre-image IMG --tyre-image-step 4000"), |a| {
                assert_eq!(a.tyre_image, Some(PathBuf::from("IMG")));
                assert_eq!(a.tyre_image_step, Some(4000));
            }),
            (format!("{BOOT} --flat-out --brake-from 27.78"), |a| {
                assert_eq!(a.brake_from, Some(27.78));
                assert_eq!((a.brake_pedal, a.brake_slip, a.brake_hold), (None, None, None));
            }),
            (format!("{BOOT} --brake-from 27.78 --brake-pedal 0.5 --brake-hold 2"), |a| {
                assert_eq!((a.brake_pedal, a.brake_slip, a.brake_hold), (Some(0.5), None, Some(2.0)));
            }),
            (format!("{BOOT} --brake-from 27.78 --brake-slip 0.15"), |a| {
                assert_eq!((a.brake_pedal, a.brake_slip, a.brake_declutch), (None, Some(0.15), false));
            }),
            (format!("{BOOT} --brake-from 27.78 --brake-abs 0.15"), |a| {
                assert_eq!((a.brake_pedal, a.brake_slip, a.brake_abs), (None, None, Some(0.15)));
            }),
            (format!("{BOOT} --brake-from 27.78 --brake-declutch --clutch-help off"), |a| assert!(a.brake_declutch)),
            (format!("{BOOT} --circle -260,120,40 --top-speed 60"), |a| {
                assert_eq!(
                    a.circle,
                    Some(autopilot::Circle { center: [-260.0, 120.0], radius: 40.0 })
                );
            }),
            (format!("{BOOT} --circle -260,120,40 --material-as 116"), |a| assert_eq!(a.material_as, Some(116))),
            (format!("{BOOT} --circle -260,120,40 --circle-ramp 0.4"), |a| {
                assert_eq!(a.circle_ramp, Some(0.4));
            }),
            (format!("{BOOT} --circle -260,120,40 --circle-past 0"), |a| assert_eq!(a.circle_past, Some(0.0))),
            (format!("{BOOT} --plan 8.5,6"), |a| {
                assert_eq!(a.plan, Some(autopilot::Plan { lateral: 8.5, braking: 6.0 }));
            }),
            (BOOT.into(), |a| assert_eq!(a.plan, None)),
            (format!("{LIVE} --replay F --replay-pace free"), |a| {
                assert_eq!(a.replay_pace, live::Pace::Free)
            }),
            (format!("{BOOT} --dispatch pure"), |a| assert!(!a.direct)),
            (format!("{BOOT} --car 5"), |a| assert_eq!(a.car, Some(5))),
            (format!("{REC} --steps 0"), |a| assert_eq!(a.steps, 0)),
            (
                format!(
                    "{BOOT} --weather 2 --tyre 7 --wear worn --wetness wet --season-row 3 --rand-seed 9 --setup SET"
                ),
                |a| {
                    assert_eq!(a.weather, Some(2));
                    assert_eq!(a.tyre, Some(7));
                    assert_eq!(a.wear, Some(2));
                    assert_eq!(a.wetness, Some(2));
                    assert_eq!(a.season_row, Some(3));
                    assert_eq!(a.rand_seed, Some(9));
                    assert_eq!(a.setup, Some(PathBuf::from("SET")));
                },
            ),
            (format!("{BOOT} --surface snow"), |a| assert_eq!(a.surface, Some("snow"))),
            (format!("{BOOT} --btb 41"), |a| assert_eq!(a.btb, Some(41))),
            (format!("{BOOT} --damage realistic"), |a| assert_eq!(a.damage, Some(DamageLevel::Realistic))),
            (format!("{BOOT} --championship champion"), |a| {
                assert!(matches!(
                    a.championship,
                    Some(c) if c.skill == Skill::Champion && c.damage == DamageLevel::Realistic
                ))
            }),
            (
                format!(
                    "{BOOT} --championship rookie --championship-damage safe --championship-false-starts 2 --rally-next service"
                ),
                |a| {
                    assert!(matches!(
                        a.championship,
                        Some(c) if c.skill == Skill::Rookie && c.damage == DamageLevel::Safe
                    ));
                    assert_eq!(a.false_starts, 2);
                    assert_eq!(a.rally_next, Some(live::RallyNext::Service));
                },
            ),
            (
                format!("{BOOT} --auto-gears on --gear-guard off --neutral-lock on --clutch-help off"),
                |a| {
                    assert_eq!(a.auto_gears, Some(true));
                    assert!(!a.aids.gear_guard && a.aids.neutral_lock && !a.aids.clutch_help);
                },
            ),
            (format!("{LIVE} --callout-distance 150 --marker-threshold 0.5"), |a| {
                assert_eq!(a.marker_settings.callout_distance, 150.0);
                assert_eq!(a.marker_settings.marker_threshold, 0.5);
            }),
            (format!("{BOOT_REC} --twin mem"), |a| assert_eq!(a.twin, Some(crate::twin::Kind::Mem))),
            (format!("{BOOT_REC} --twin mem-direct"), |a| {
                assert_eq!(a.twin, Some(crate::twin::Kind::MemDirect))
            }),
            (format!("{BOOT} --damage-in IN --damage-out OUT2 --result-out RES"), |a| {
                assert_eq!(a.damage_in, Some(PathBuf::from("IN")));
                assert_eq!(a.damage_out, Some(PathBuf::from("OUT2")));
                assert_eq!(a.result_out, Some(PathBuf::from("RES")));
            }),
            (
                format!(
                    "{REC} --recorded-payload --extra-payloads EP --wall-clock WC --trace-out T --ffb-settings 1,0.5,0,0.1,0.2"
                ),
                |a| {
                    assert!(a.recorded_payload);
                    assert_eq!(a.extra_payloads, Some(PathBuf::from("EP")));
                    assert_eq!(a.wall_clock, Some(PathBuf::from("WC")));
                    assert_eq!(a.trace_out, Some(PathBuf::from("T")));
                    assert!(a.ffb_settings.enabled && !a.ffb_settings.inverse);
                    assert_eq!(a.ffb_settings.strength, 0.5);
                },
            ),
            (format!("{BOOT_REC} --events-out E"), |a| assert_eq!(a.events_out, Some(PathBuf::from("E")))),
            (format!("{LIVE} --frames F"), |a| assert_eq!(a.frames.as_deref(), Some("F"))),
            (format!("{LIVE} --frames-out F --start-time 9:36"), |a| {
                assert_eq!(a.frames_out, Some(PathBuf::from("F")));
                assert_eq!(a.start_time, Some(34560.0));
            }),
            (
                format!(
                    "{BOOT} --rsf-car PACK --ffb-sensitivity 90,80,70 --head-gasket-durability 3 --saves SV --profile P --read-set RS --dump-image IMG"
                ),
                |a| {
                    assert_eq!(a.rsf_car.as_deref(), Some("PACK"));
                    assert_eq!(a.ffb_sensitivity, Some([90, 80, 70]));
                    assert_eq!(a.head_gasket_durability, Some(3));
                    assert_eq!(a.saves, Some(PathBuf::from("SV")));
                    assert_eq!(a.profile.as_deref(), Some("P"));
                    assert_eq!(a.read_set, Some(PathBuf::from("RS")));
                    assert_eq!(a.dump_image, Some(PathBuf::from("IMG")));
                },
            ),
        ];
        for (argv, check) in &good {
            match run_str(argv, None) {
                Ok(args) => check(&args),
                Err(e) => panic!("{argv}: error {e}, want Args"),
            }
        }
        // `--data` from the environment.
        match run_str("--recording R --stage track-54_E --snapshot S", Some("ENV")) {
            Ok(args) => assert_eq!(args.data, PathBuf::from("ENV")),
            Err(e) => panic!("RBR_DATA fallback: error {e}"),
        }
    }

    /// Every flag's bad values and broken combinations: the command line and its exact error.
    #[test]
    fn bad_flags_fail_as_before() {
        let bad: Vec<(String, &str)> = vec![
            ("--car".into(), "--car: no value"),
            ("--warp x".into(), "unknown argument --warp"),
            ("--recording R --rsf-stage x --snapshot S --data D".into(), "--rsf-stage x"),
            ("--recording R --rsf-stage 101 --snapshot S --data D".into(), "--rsf-stage needs --rsf-root or RBR_RSF"),
            (format!("{REC} --top-speed fast"), "--top-speed fast"),
            (format!("{REC} --steps x"), "--steps x"),
            (format!("{REC} --car x"), "--car x"),
            (format!("{BOOT} --weather x"), "--weather x"),
            (format!("{BOOT} --tyre x"), "--tyre x"),
            (format!("{BOOT} --season-row x"), "--season-row x"),
            (format!("{BOOT} --rand-seed x"), "--rand-seed x"),
            (format!("{REC} --autopilot OUT --aim 1"), "--aim 1: want X,Y[,COAST]"),
            (format!("{REC} --autopilot OUT --aim 1,2,x"), "--aim 1,2,x"),
            (format!("{LIVE} --callout-distance x"), "--callout-distance x"),
            (format!("{LIVE} --marker-threshold x"), "--marker-threshold x"),
            (format!("{BOOT_REC} --twin both"), "--twin both: mem or mem-direct"),
            (
                format!("{BOOT_REC} --twin mem --physics experimental"),
                "--twin takes no --physics experimental (the twin would step the run's models again)",
            ),
            (format!("{LIVE} --replay F --replay-pace fast"), "--replay-pace fast: clock or free"),
            (format!("{BOOT} --dispatch warp"), "--dispatch warp: pure or direct"),
            (format!("{LIVE} --physics fast"), "--physics fast: one of ngp, experimental, experimental-brush"),
            (format!("{LIVE} --car-year new"), "--car-year new"),
            (format!("{BOOT} --wear muddy"), "--wear muddy: new, normal or worn"),
            (format!("{BOOT} --wetness humid"), "--wetness humid: dry, damp or wet"),
            (format!("{BOOT} --surface ice"), "--surface ice: gravel, tarmac or snow"),
            (format!("{BOOT} --damage flat"), "--damage flat: off, safe, reduced or realistic"),
            (format!("{BOOT} --championship easy"), "--championship easy: rookie, professional or champion"),
            (
                format!("{BOOT} --championship rookie --championship-damage flat"),
                "--championship-damage flat: off, safe, reduced or realistic",
            ),
            (
                format!("{BOOT} --championship-false-starts x"),
                "--championship-false-starts x: invalid digit found in string",
            ),
            (format!("{BOOT} --championship rookie --rally-next soon"), "--rally-next soon: stage, service or end"),
            (format!("{BOOT} --auto-gears maybe"), "--auto-gears maybe: on or off"),
            (format!("{BOOT} --ffb-sensitivity 90,80"), "--ffb-sensitivity 90,80: three percentages, G,T,S"),
            (format!("{BOOT} --head-gasket-durability x"), "--head-gasket-durability x: an integer"),
            (
                format!("{REC} --recorded-payload --trace-out T --ffb-settings 1,2"),
                "--ffb-settings 1,2: ENABLED,STRENGTH,INVERSE,DEAD_ZONE,THRESHOLD (0 or 1, then numbers)",
            ),
            (
                format!("{REC} --recorded-payload --trace-out T --ffb-settings 2,1,0,0,0"),
                "--ffb-settings 2,1,0,0,0: ENABLED,STRENGTH,INVERSE,DEAD_ZONE,THRESHOLD (0 or 1, then numbers)",
            ),
            (format!("{LIVE} --start-time 9"), "--start-time 9: H:MM"),
            (format!("{BOOT} --snapshot S"), "--boot takes no --snapshot"),
            (format!("{LIVE} --recording R"), "--live takes no --recording"),
            ("--live --stage track-54_E --data D".into(), "--live takes one --snapshot"),
            (format!("{LIVE} --snapshot T"), "--live takes one --snapshot"),
            ("--stage track-54_E --snapshot S --data D".into(), "--recording is required"),
            ("--recording R --stage track-54_E --snapshot S".into(), "--data or RBR_DATA is required"),
            (format!("{LIVE} --autopilot OUT"), "--autopilot takes one --snapshot or --boot, and no --live"),
            (
                format!("{REC} --autopilot OUT --snapshot T"),
                "--autopilot takes one --snapshot or --boot, and no --live",
            ),
            (format!("{REC} --setup SET"), "--setup needs --boot"),
            (format!("{REC} --ngp-physics N"), "--ngp-physics needs --boot"),
            (
                "--boot --stage track-54_E --data D --autopilot OUT".into(),
                "--boot needs --ngp-physics FILE (NGP's physics.rbz): ReRBR steps NGP physics only",
            ),
            (format!("{REC} --rsf-car PACK"), "--rsf-car needs --boot"),
            (format!("{BOOT} --car 5 --rsf-car PACK"), "--rsf-car replaces --car"),
            (format!("{REC} --ffb-sensitivity 90,80,70"), "--ffb-sensitivity needs --boot"),
            (format!("{REC} --head-gasket-durability 3"), "--head-gasket-durability needs --boot"),
            (format!("{REC} --damage-in IN"), "--damage-in needs --boot"),
            (format!("{REC} --damage-out OUT"), "--damage-out and --result-out need --autopilot or --live --boot"),
            (
                format!("{BOOT} --rally-next end"),
                "--championship-damage, --championship-false-starts and --rally-next need --championship",
            ),
            (format!("{REC} --championship rookie"), "--championship needs --boot"),
            (
                format!("{REC} --events-out E"),
                "--events-out needs --boot and --recording, and no --live or --autopilot",
            ),
            (
                format!("{LIVE} --recorded-payload"),
                "--recorded-payload and --trace-out need --recording, and no --live or --autopilot",
            ),
            (format!("{REC} --recorded-payload --ffb-settings 1,1,0,0,0"), "--ffb-settings needs --trace-out"),
            (format!("{REC} --extra-payloads EP"), "--extra-payloads and --wall-clock need --recorded-payload"),
            (format!("{REC} --pilot autopilot"), "--pilot needs --live"),
            (format!("{REC} --frames-out F"), "--frames-out and --frames need --live"),
            (format!("{LIVE} --frames F --frames-out G"), "--frames takes no --frames-out (the frames are given)"),
            (format!("{REC} --replay-out F"), "--replay-out and --replay need --live"),
            (format!("{LIVE} --frames F --replay G"), "one of --frames-out, --frames, --replay-out and --replay"),
            (format!("{LIVE} --replay F --pilot autopilot"), "--replay takes no --pilot (the replay drives)"),
            (format!("{REC} --aim 1,2"), "--aim needs --autopilot"),
            (format!("{REC} --flat-out"), "--flat-out, --trace-speed, --tyre-probe and --tyre-image need --autopilot"),
            (
                format!("{REC} --trace-speed T"),
                "--flat-out, --trace-speed, --tyre-probe and --tyre-image need --autopilot",
            ),
            (
                format!("{REC} --tyre-probe P"),
                "--flat-out, --trace-speed, --tyre-probe and --tyre-image need --autopilot",
            ),
            (
                format!("{REC} --tyre-image IMG"),
                "--flat-out, --trace-speed, --tyre-probe and --tyre-image need --autopilot",
            ),
            (format!("{BOOT} --tyre-image-step 9"), "--tyre-image-step needs --tyre-image"),
            (format!("{REC} --circle 0,0,40"), "--circle needs --autopilot"),
            (format!("{REC} --material-as 116"), "--material-as needs --boot"),
            (format!("{REC} --material-as ice"), "--material-as ice: a material id"),
            (format!("{REC} --circle-ramp 0.4"), "--circle-ramp needs --circle"),
            (format!("{REC} --circle-past 1"), "--circle-past needs --circle"),
            (format!("{REC} --plan 8,6"), "--plan needs --autopilot"),
            (format!("{BOOT} --plan 8"), "--plan 8: want LAT,BRAKE in m/s², both over 0"),
            (format!("{BOOT} --plan 8,0"), "--plan 8,0: want LAT,BRAKE in m/s², both over 0"),
            (format!("{BOOT} --plan 8,6 --flat-out"), "--plan takes no --flat-out or --circle (they plan no speed)"),
            (
                format!("{BOOT} --plan 8,6 --circle 0,0,40"),
                "--plan takes no --flat-out or --circle (they plan no speed)",
            ),
            (format!("{REC} --brake-from 27.78"), "--brake-from needs --autopilot"),
            (
                format!("{BOOT} --brake-pedal 0.5"),
                "--brake-pedal, --brake-slip, --brake-abs, --brake-hold and --brake-declutch need --brake-from",
            ),
            (
                format!("{BOOT} --brake-slip 0.1"),
                "--brake-pedal, --brake-slip, --brake-abs, --brake-hold and --brake-declutch need --brake-from",
            ),
            (
                format!("{BOOT} --brake-hold 2"),
                "--brake-pedal, --brake-slip, --brake-abs, --brake-hold and --brake-declutch need --brake-from",
            ),
            (
                format!("{BOOT} --brake-from 20 --brake-declutch"),
                "--brake-declutch needs --clutch-help off (clutch help sets the clutch in place of the pedal)",
            ),
            (
                format!("{BOOT} --brake-declutch"),
                "--brake-pedal, --brake-slip, --brake-abs, --brake-hold and --brake-declutch need --brake-from",
            ),
            (
                format!("{BOOT} --brake-from 20 --brake-pedal 0.5 --brake-slip 0.1"),
                "--brake-slip takes no --brake-pedal (it moves the pedal itself)",
            ),
            (
                format!("{BOOT} --brake-abs 0.15"),
                "--brake-pedal, --brake-slip, --brake-abs, --brake-hold and --brake-declutch need --brake-from",
            ),
            (
                format!("{BOOT} --brake-from 20 --brake-slip 0.1 --brake-abs 0.15"),
                "--brake-abs takes no --brake-pedal or --brake-slip (it holds the full pedal)",
            ),
            (format!("{BOOT} --brake-from 20 --brake-abs 1.5"), "--brake-abs 1.5: over 0, at most 1"),
            (format!("{BOOT} --brake-from 20 --brake-pedal 0"), "--brake-pedal 0: over 0, at most 1"),
            (format!("{BOOT} --brake-from 20 --brake-pedal 1.5"), "--brake-pedal 1.5: over 0, at most 1"),
            (format!("{BOOT} --brake-from 20 --brake-slip -0.1"), "--brake-slip -0.1: over 0, at most 1"),
            (format!("{BOOT} --brake-from 20 --brake-slip x"), "--brake-slip x"),
            (format!("{BOOT} --brake-from 20 --brake-hold -1"), "--brake-hold -1: seconds, 0 or more"),
            (format!("{REC} --autopilot OUT --circle 0,0"), "--circle 0,0: want X,Y,R with R > 0"),
            (format!("{REC} --autopilot OUT --circle 0,0,-5"), "--circle 0,0,-5: want X,Y,R with R > 0"),
            (
                format!("{REC} --autopilot OUT --circle 0,0,40 --aim 1,2"),
                "--circle takes no --aim (it steers the circle itself)",
            ),
            ("--recording R --snapshot S --data D".into(), "--stage, --stage-dir or --rsf-stage is required"),
            (format!("{REC} --stage-dir DIR"), "give one of --stage, --stage-dir and --rsf-stage"),
        ];
        for (argv, want) in &bad {
            match run_str(argv, None) {
                Err(e) => assert_eq!(&e, want, "{argv}"),
                Ok(_) => panic!("{argv}: parsed, want error {want}"),
            }
        }
    }

    /// `--school`: the lesson's stage and car 5 filled in unless another car is given, its own flags refused.
    #[test]
    fn school_sets_the_lessons_stage_and_car() {
        let school = "--boot --data D --ngp-physics N --autopilot OUT --school";
        let a = run_str(&format!("{school} 2"), None).unwrap();
        assert_eq!((a.lesson.map(Lesson::index), a.stage.as_str(), a.car), (Some(2), "track-83_M", Some(5)));
        assert_eq!(run_str(&format!("{school} 2 --stage track-83_M --car 5"), None).unwrap().stage, "track-83_M");
        assert_eq!(run_str(&format!("{school} 2 --car 6"), None).unwrap().car, Some(6));
        let rsf = run_str(&format!("{school} 2 --rsf-car PACK"), None).unwrap();
        assert_eq!((rsf.car, rsf.rsf_car.as_deref()), (None, Some("PACK")));
        let pack = run_str(&format!("{school} 2 --car-pack P --car-model /R/Cars/evo/evo.sgc"), None).unwrap();
        assert_eq!((pack.car, pack.pack_car.pack), (None, Some(PathBuf::from("P"))));
        for (argv, want) in [
            (format!("{school} 17"), "--school 17: a lesson, 0 to 16"),
            (format!("{school} 2 --stage track-54_E"), "--school 2: lesson's stage is map 83, not track-54_E"),
            (format!("{school} 2 --tyre 4"), "--school takes no --tyre (the lesson sets it)"),
            (format!("{school} 2 --championship rookie"), "--school takes no --championship (the lesson sets it)"),
            (format!("{REC} --school 2"), "--school needs --boot"),
            (
                format!("{school} 2 --flat-out"),
                "--flat-out, --circle and --brake-from take no --school (a lesson brakes and stops)",
            ),
        ] {
            assert_eq!(run_str(&argv, None).err().as_deref(), Some(want), "{argv}");
        }
    }

    /// The pack-car flags forward to `car_pack::Flags` (`car_pack` tests their combinations); `--rsf-car`
    /// beside them fails there.
    #[test]
    fn pack_car_flags_forward_and_check() {
        let args = run_str(
            &format!("{BOOT} --car-pack P --car-model /R/Cars/evo/evo.sgc --car-bank B --car-key rsf-24"),
            None,
        )
        .unwrap();
        assert_eq!(
            args.pack_car,
            car_pack::Flags {
                pack: Some(PathBuf::from("P")),
                model: Some(PathBuf::from("/R/Cars/evo/evo.sgc")),
                bank: Some(PathBuf::from("B")),
                key: Some("rsf-24".into()),
            }
        );
        match run_str(&format!("{BOOT} --car-pack P --car-model /R/Cars/evo/evo.sgc --rsf-car X"), None) {
            Err(e) => assert_eq!(e, "give one of --rsf-car and --car-pack"),
            Ok(_) => panic!("a pack car beside --rsf-car parses"),
        }
        match run_str(&format!("{BOOT} --car-model /R/Cars/evo/evo.sgc"), None) {
            Err(e) => assert_eq!(e, "--car-model, --car-bank and --car-key need --car-pack"),
            Ok(_) => panic!("a model without --car-pack parses"),
        }
    }

    /// A stage folder on disk: `--stage-dir` reads it, and an empty one and a missing one fail as before.
    #[test]
    fn stage_dir_reads_a_folder() {
        let dir = std::env::temp_dir().join("rerbr-args-test-stage");
        let empty = std::env::temp_dir().join("rerbr-args-test-empty");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::write(dir.join("Tracks101.ini"), "[Map101]\nTrackName=track-101\n").unwrap();
        for ext in ["lbs", "col", "trk", "dls", "mat", "ini"] {
            std::fs::write(dir.join(format!("track-101_O.{ext}")), [0u8; 4]).unwrap();
        }
        let dir = dir.to_string_lossy().into_owned();
        let boot = ["--boot", "--stage-dir", &dir, "--data", "D", "--ngp-physics", "N", "--autopilot", "OUT"];
        let args: Vec<&str> = boot.iter().map(|s| s.as_ref()).collect();
        let parsed = run(&args, None, None).expect("a stage folder with --boot parses");
        assert!(parsed.rsf.is_some() && parsed.stage.is_empty());
        let no_boot = ["--recording", "R", "--snapshot", "S", "--stage-dir", &dir, "--data", "D"];
        let args: Vec<&str> = no_boot.iter().map(|s| s.as_ref()).collect();
        match run(&args, None, None) {
            Err(e) => assert_eq!(e, "--stage-dir and --rsf-stage need --boot (the weather picks the variant)"),
            Ok(_) => panic!("a stage folder without --boot parses"),
        }
        let empty = empty.to_string_lossy().into_owned();
        let missing_ini = ["--recording", "R", "--snapshot", "S", "--stage-dir", &empty, "--data", "D"];
        let args: Vec<&str> = missing_ini.iter().map(|s| s.as_ref()).collect();
        match run(&args, None, None) {
            Err(e) => assert_eq!(e, format!("{empty}: no Tracks<id>.ini")),
            Ok(_) => panic!("an empty folder parses"),
        }
        std::fs::remove_dir_all(dir).ok();
    }
}
