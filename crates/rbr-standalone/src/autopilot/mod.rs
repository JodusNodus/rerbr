//! Autopilot (`rbr-standalone --autopilot OUT`): a driveline-following controller
//! that drives the pure step from a snapshot and writes the controls as a playback (`harness/play.sh`'s
//! `RBRRecompPlayback.bin`), so the original can replay them open-loop.
//!
//! The playback is the drive's own recorded frames before the snapshot's step, then one frame per live step
//! (`live/`): the snapshot's own step with its recorded control, and from the next step on the controller's. Each
//! frame holds the pure car and wheels after its step, so the playback is also the Rust run's recording.
//! From `--boot` the live steps start after the boot's load settle and take a neutral
//! control first; the playback then has no settle frames (the plugin plays single steps only). The race clock's 7 s
//! start countdown runs over those steps, so the controller holds the car on the handbrake until it is out: a
//! playback that left before it would take the jump start again wherever it is replayed (`--live --pilot`, the game).
//!
//! The controller is not an original function: it steers to a look-ahead point on the `.trk` driveline (a Hermite
//! spline), holds a target speed from the driveline's curvature ahead, and shifts by engine speed. It stops a few
//! hundred steps after the bound time keeper records the finish (with `--saves`, at the race's end, which writes the
//! records as a live drive does), or when the car is stuck or off the road.
//!
//! Stuck recovery (ours): when the car, having moved, stays under `STUCK_SPEED` (or under `BACKWARDS_SPEED` facing
//! away from the driveline) for `RECOVER_AFTER` steps, the controller brakes, shifts to reverse and backs up with the
//! wheels turned to swing the nose towards the driveline, then shifts back to first and drives on. It tries
//! `ATTEMPTS` times; the count starts again once the car is `PROGRESS` m further along the driveline. After the last
//! attempt, or at once when the car is on its side or roof, the run ends as stuck. Upshifts wait while the driven
//! wheels spin (engine speed over `SLIP` times the lowest engine speed per m/s seen in the gear): shifting up on
//! wheelspin left cars in a high gear in slides and on climbs, where they stalled or rolled (track-25_O, track-35_N).
//!
//! Limit runs (ours, `--plan LAT,BRAKE`, test tooling for `harness/limit_pace.py`): the speed plan takes the given
//! lateral and braking acceleration (m/s²) in place of `LATERAL` and `BRAKING` (the wetness's grip still scales them;
//! `--top-speed` sets the top), and the race pedal stays under threshold braking's at `PLAN_SLIP`: at the car's limits
//! the plain pedal locked the fronts entering corners. The kink scan keeps `LATERAL` (geometry), and the look-ahead,
//! `FULL_LOCK_TOP`, the steering gain and the anti-stall logic are unchanged. Without the flag the drive is the
//! default's, byte for byte.
//!
//! On RSF stages (ours): the target swings around heavy props near the driveline (`HEAVY`),
//! a driveline segment that loops is flattened (`LOOP`, `BACK`), and a car leaning over steers towards its low side
//! (`TIP`).
//!
//! Under NGP (ours): upshifts come before the car's rev limit, a standing start revs in
//! neutral before it selects first, a car bogged in first goes back to neutral and starts again, the throttle
//! eases while the wheels spin, and a recovery backs up no faster than `REVERSE_TOP`. NGP's clutch help closes the clutch from idle, where the Escort Mk II and the 208 R2
//! have too little torque to pull away, and they sat in first at the clutch help's speed.
//! In a race under NGP, an engine the braked wheels drag under the clutch help's speed takes neutral until the brake
//! comes off, and a stalled engine is restarted on the ignition (`DECLUTCH`, `STALLED`): the Golf II on RSF 104 under
//! `experimental-brush` stalled at 4450 m, where loose-v1's braked fronts creep, and never restarted.

use rbr_formats::trk::{DriveLineRecord, Vec3};
use rbr_gameplay::control::QuantizedControl;
use rbr_hybrid::pure;
use rbr_recording::hud::HUD2_BYTES;
use rbr_recording::pose::{self, CarPose};
use rbr_recording::sound::FmodBlock;
use rbr_recording::{read_f32, read_u32, CONTROL_BYTES, HEADER_BYTES, WHEELS, WHEEL_BYTES};

mod drive;
mod limit;
pub mod pad;
pub mod race;
pub mod start;

pub use drive::{drive, from_snapshot, Pilot};
pub use pad::Manoeuvre;

use crate::live::Live;
use crate::physics::DRIVELINE_OFFSET;

/// Look-ahead of the steering target: `LOOK_BASE + LOOK_TIME * speed` metres. A base of 6 m cut the inside of slow
/// hairpins into barriers and banks (RSF 125, 143, 310, 338, 428, 445 rolled or stuck); 3 m drives 465 of the 470 plain
/// RSF variants to the step limit.
const LOOK_BASE: f32 = 3.0;
const LOOK_TIME: f32 = 0.5;
/// Steering per radian of heading error to the target (full lock at about 20 degrees).
const STEER_GAIN: f32 = 3.0;
/// The speed plan: lateral and braking acceleration (m/s², `--plan` in their place), the default top speed (m/s,
/// `--top-speed`), and how far ahead it looks (m).
const LATERAL: f32 = 5.0;
const BRAKING: f32 = 4.0;
const TOP_SPEED: f32 = 30.0;
/// The target speed while the steering is past full lock (m/s): the curve plan missed hairpins at full lock on RSF
/// stages 122 and 302, and the map 87 kink at 800 m slid 2.4 m off at 6.5 m/s and retired on the wall.
const FULL_LOCK_TOP: f32 = 5.0;
/// A very slow corner looms (ours): the slowest curve limit within `KINK_AHEAD` m under this (m/s), and the hard
/// braking for it. The gentle pedal below tracks fast corners loosely (lesson 2 overshoots its 16 m/s corner by 8
/// and still passes), but the map 87 kink (limit 6) needs full braking from 30 m out to arrive at 5 m/s on line;
/// arriving hot slides it into the wall. Only the kink brakes hard; lesson 2's corners (limits 16 and up) keep the
/// gentle pedal.
const KINK_SLOW: f32 = 7.0;
const KINK_AHEAD: f32 = 40.0;
const PLAN_AHEAD: f32 = 150.0;
/// The plan's sample spacing (m): 1 m resolves the driveline's kinks (the map 87 kink turns 28 degrees over 5 m
/// between 2.5 m-spaced records, which 3 m samples aliased into a fast read), while smooth corners read the same.
const PLAN_STEP: f32 = 1.0;
/// Shift points (engine speed, rad/s), the steps between shifts, and the gear index of first and the top gear (neutral is below first).
const SHIFT_UP: f32 = 680.0;
const SHIFT_DOWN: f32 = 380.0;
const SHIFT_GAP: u32 = 150;
/// Upshifts come at `UP_LIMIT` of the car's rev limit when that is below `SHIFT_UP` (NGP cars: the Evo IX's limiter is
/// at 6200 rpm; 1.02's 7800 and 8500 rpm leave `SHIFT_UP`), and always there flat out, where `SHIFT_UP` would shift
/// a high-revving engine short of its power (the M3's 300 bhp is at 8200 rpm).
const UP_LIMIT: f32 = 0.93;
/// Under NGP a standing start revs to `LAUNCH` of the rev limit in neutral before it selects first, and a car bogged in
/// first (engine under `BOG` of the limit below `BOG_SPEED` m/s for `BOG_STEPS` steps) goes back to neutral to do it
/// again. NGP's clutch help closes the clutch from about idle on (`notes/ngp.md`, "Controls under NGP"), and an engine
/// with little torque there (Escort Mk II, 208 R2) cannot pull the car from it.
const LAUNCH: f32 = 0.6;
const BOG: f32 = 0.2;
const BOG_SPEED: f32 = 3.0;
const BOG_STEPS: u32 = 100;
/// Under NGP the throttle while the driven wheels spin (`SLIP`) above `BOG_SPEED`: full throttle spun the rear-driven
/// Escort Mk II off the road on gravel.
const TRACTION: f32 = 0.5;
/// Race driving under NGP (ours, not the measurement modes or lessons): braking in gear, with a wheel past
/// `LOCK_SLIP` and the engine dragged by the wheels (within `DRAGGED` rad/s of the speed the gear gives the drive)
/// under `DECLUTCH` rad/s (where the Celica's clutch help is fully open, `notes/ngp.md`, "Controls under NGP") for
/// `DRAG_STEPS` 1.02 steps in a row (a downshift into first on track-10_O dips the Golf to 81 rad/s for 6 NGP steps
/// under ngp, then the wheels pull it up again), the controller selects neutral until the brake comes off, then the gear that keeps the engine over `SHIFT_DOWN`. Braked
/// fronts that creep instead of locking (loose-v1's flat friction, the Golf II on RSF 104 at 4450 m) keep NGP's
/// driveline clutch-locked and drag the engine to a stall despite the clutch help; the clutch pedal does nothing while
/// the clutch help sets the clutch (`rbr_physics::vcu::clutch`), so neutral stands in for the driver's declutch.
const DECLUTCH: f32 = 89.0;
const DRAGGED: f32 = 10.0;
const DRAG_STEPS: u32 = 4;
const LOCK_SLIP: f32 = 0.5;
/// A stalled engine (under `STALLED` rad/s) is restarted: brake, neutral, and the ignition held for `CRANK_HOLD` 1.02
/// steps of every `CRANK_CYCLE` (NGP's starter cranks from 0.5 s to 2.5 s of a hold, then the ignition must clear),
/// until the engine runs over `STARTED` rad/s; the standing start (or the gear for the speed) then pulls away.
const STALLED: f32 = 30.0;
const STARTED: f32 = 60.0;
const CRANK_HOLD: u32 = 370;
const CRANK_CYCLE: u32 = 400;
/// The payload's gear select (flag 0x04 with the gear byte, `rbr_physics::vcu::dispatch`), and neutral's gear.
const SELECT: u8 = 0x04;
const NEUTRAL: u32 = 1;
/// Full throttle in too tall a gear with the engine below torque and the car barely moving: loose ground lugs it
/// down (gravel circles stall in second at 500 rad/s), so downshift to keep it pulling. Only in the measurement
/// modes (`free`), so race and lesson driving keep the cruising `SHIFT_DOWN` floor; the tarmac pace and circle
/// runs never drop here (their second-gear lugging stays above `LUG_SPEED`).
const LUG_ENGINE: f32 = 600.0;
const LUG_SPEED: f32 = 12.0;
const FIRST: u32 = 2;
const TOP_GEAR: u32 = 7;
/// Steps driven on (braking) after the finish.
const AFTER_FINISH: u32 = 300;
/// The car counts as stuck, once it has moved faster than `MOVING` (m/s), while under `STUCK_SPEED` before the finish
/// (recovery: module comment). The run ends as off the road after `OFF_ROAD_STEPS` steps (2 s) over `OFF_ROAD` m from
/// the driveline. Neither with `--aim`, which leaves the road on purpose.
const MOVING: f32 = 5.0;
const STUCK_SPEED: f32 = 1.0;
const OFF_ROAD: f32 = 15.0;
const OFF_ROAD_STEPS: u32 = 300;
/// Stuck recovery (module comment). Step counts here are 1.02's steps of 7 ms; a run of shorter steps (NGP's 1/720 s)
/// scales them (`Driver::with_step`).
const RECOVER_AFTER: u32 = 280;
const BACKWARDS: f32 = 2.0;
const BACKWARDS_SPEED: f32 = 3.0;
const ATTEMPTS: u32 = 4;
const PROGRESS: f32 = 50.0;
/// Steps between gear pulses while recovering, the reverse's throttle, its shortest and longest time (from the start
/// of the recovery), and the heading error (rad) to the steering target that ends it early.
const RECOVER_GAP: u32 = 20;
const REVERSE_THROTTLE: f32 = 0.5;
/// Under NGP the reverse's throttle is off above this speed (m/s): at `REVERSE_THROTTLE` an NGP Evo IX backed up at
/// 11.7 m/s and off the road (RSF stage 122).
const REVERSE_TOP: f32 = 3.0;
const REVERSE_MIN: u32 = 300;
const REVERSE_MAX: u32 = 700;
const AIMED: f32 = 0.3;
const REVERSE: u32 = 0;
/// An outside assistance holds the car until its spectators are done (`race_clock`'s timer at 4.5 s, and a step or two:
/// a pulse at 4.48 s did nothing, one at 4.6 s took): gear pulses in that time do nothing (RSF stages 122 and 302 sat
/// in neutral after one, then backed up without reverse). The controller waits on the brake until then and starts
/// again from neutral.
const HELD: f32 = 4.6;
/// A stuck car whose up axis has a world z under this is on its side or roof: backing up cannot help.
const ROLLED: f32 = 0.5;
/// While the car leans sideways by more than this (the up axis's component along the car's horizontal right, about 15
/// degrees), it steers towards the low side: on RSF 396's 90-degree left at 636 m the driveline runs along the foot
/// of a steep bank, and at full lock the left wheels climbed it until the car fell on its side at 6 m/s.
const TIP: f32 = 0.26;
/// An upshift waits while the engine speed is over `SLIP` times the gear's lowest engine speed per m/s (learnt above
/// `RATIO_SPEED` m/s under throttle without brake, `RATIO_SETTLE` steps after a shift: the clutch is in, and the wheels
/// can only turn faster than the road).
const SLIP: f32 = 1.4;
const RATIO_SPEED: f32 = 3.0;
const RATIO_SETTLE: u32 = 70;
/// `--circle`'s target speed (m/s): it starts here and rises at `CIRCLE_RAMP` (m/s per s), so the car settles onto
/// the circle before the ramp leaves city speeds.
const CIRCLE_START: f32 = 8.0;
const CIRCLE_RAMP: f32 = 0.5;
/// `CCar` +0x4b0: the drive (`CDrive`), whose gearbox gives the engine speed of the drive's speed in gear.
const CAR_DRIVE: u32 = 0x4b0;
/// `CCar` +0x4b0 +8: the drive state; its +0x45c the engine speed the VCU reads (`CDrive::Get4520`, 0x504520).
const DRIVE_STATE: u32 = 0x4b8;
const DRIVE_ENGINE_SPEED: u32 = 0x45c;
/// `CCar` +0x850: the VCU's definition; its +0x188 the rev limit (`RPMLimit`, rpm).
const VCU_DEF: u32 = 0x850;
const VCU_RPM_LIMIT: u32 = 0x188;
/// An answer's frame (`live/`); the HUD2 record follows it, with the gear as its word 1.
const FRAME_BYTES: usize = HEADER_BYTES + CONTROL_BYTES + pure::CAR_BYTES + WHEELS * WHEEL_BYTES;
/// `--aim`: the controller steers at the point instead of the driveline while it is closer than `AIM_RANGE` metres and
/// ahead; the drive ends `AIM_AFTER` steps after the car first comes within `AIM_HIT` metres of it.
const AIM_RANGE: f32 = 60.0;
const AIM_HIT: f32 = 4.0;
const AIM_AFTER: u32 = 600;

/// `--aim X,Y[,COAST]` (scenery drives): a stage point to drive into, and how many steps to
/// keep the last control after the pure step stops (the original replays them; the playback's car state then repeats
/// the last good step's, which the plugin does not read).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aim {
    pub at: [f32; 2],
    pub coast: u32,
}

impl std::str::FromStr for Aim {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let v: Vec<f32> =
            s.split(',').map(|x| x.parse().map_err(|_| format!("--aim {s}"))).collect::<Result<_, _>>()?;
        match v[..] {
            [x, y] => Ok(Aim { at: [x, y], coast: 0 }),
            [x, y, c] => Ok(Aim { at: [x, y], coast: c as u32 }),
            _ => Err(format!("--aim {s}: want X,Y[,COAST]")),
        }
    }
}

/// Heavy props (ours): the collision meshes of kinds 2 (concrete block, `PIG`, 4000 kg), 8 (`LOG_POLE`) and 10
/// (`WOODEN_FENCE_POLE`, both 1000 kg; rbr-recomp `notes/interactive-scenery.md` 1c, 3.2), and the static (`static_flag
/// == 1`, one collision id shared across placements) meshes of kinds 0 and 3, the baked solid stage geometry. The
/// controller steers around those near the driveline, at most `DODGE_TOP` m/s: RSF 468's driveline runs through a
/// chicane of 1000 kg boxes at 711-738 m, and the car hit the first at 22 m/s and rolled; the school playground's
/// fence (static kind-0 2 m panels) crosses the map 87 driveline at 18-76 m and the map 76 one at 22-40 m, and the car
/// drove into it at 33 and 48 m. Lighter and breakable props (cones, signs, banners, logs, fence bars, the dynamic
/// kind-0 trees and start fences) are driven through: no static kind-0 or kind-3 mesh covers the driveline of the
/// retail road stages checked (track-11_O, track-22_O), so the road drives keep their line.
const HEAVY: [u32; 3] = [2, 8, 10];
/// A static kind-0 or kind-3 mesh counts as a heavy prop too.
const STATIC_HEAVY: [u32; 2] = [0, 3];
/// Lateral clearance from a prop's footprint to the car's centre line (m; the car is about 1.75 m wide); how far the
/// target may swing off the driveline (m: the road's edges are not known, so a chicane is passed prop by prop, and a
/// fence is ridden along its edge; a centred 2 m panel needs 2.6 m of offset with `CLEAR`); how
/// long a prop stays in view after the car's centre passes it (m); how far before a prop the target swings past the
/// look-ahead point, and the speed is held at `DODGE_TOP` m/s for one on the line (m).
const CLEAR: f32 = 1.6;
const SWING: f32 = 3.0;
/// The tracking guard (ours): while driving, the target speed is capped where the car is further from its steered
/// line than this (m), proportionally down to `TRACK_MIN` m/s. The curve plan reads the driveline, but a sliding car
/// leaves it: the wet track-22_O hairpin at 3796 m held 7.2 m/s dry and slid wide at 8.6 m/s wet, down the bank past
/// recovery; the map 87 kink at 800 m slid 2.2 m off at 6.5 m/s and ground the wall into a school retire. Slowing
/// until the car answers the steering again holds both, on any surface, with no grip data.
const TRACK_ERR: f32 = 1.2;
const TRACK_MIN: f32 = 3.0;
/// Adaptive look-ahead (ours): the steering target stays within the driveline's near part, pulling nearer while the
/// line ahead winds past this (rad, cumulative). A fixed look aims across kinks and cuts them: the map 87 kink at
/// 800 m turns 28 degrees over 5 m, and the 6-9 m look put the target across it, so the car left the road before it
/// could slow. Tracking the entry instead threads it. Straights and sweepers keep the full look.
const TURN_IN: f32 = 0.3;
const PASSED: f32 = 2.2;
const LEAD: f32 = 10.0;
/// The crawl past a prop on the line (m/s): above the school's lowest judged speed band (15 km/h, lesson 1's
/// 69.7-707.5 m check trips below it), slow enough for the RSF 468 chicane it was built for.
const DODGE_TOP: f32 = 4.5;

/// A heavy prop's footprint: its collision mesh's vertices placed in the stage (x, y).
#[derive(Clone, Debug, PartialEq)]
pub struct Obstacle {
    pub footprint: Vec<[f32; 2]>,
}

/// The heavy props of a stage's `.trk` collision meshes (`HEAVY`, and the static kind-0 and kind-3 meshes).
pub fn obstacles(trk: &rbr_formats::trk::TrkFile) -> Vec<Obstacle> {
    use rbr_formats::trk::ChunkPayload;
    let meshes = trk.chunks.iter().filter_map(|c| match &c.payload {
        ChunkPayload::CollisionMeshes(m) => Some(&m.meshes),
        _ => None,
    });
    let heavy = meshes.flatten().filter(|m| {
        !m.vertices.is_empty() && (HEAVY.contains(&m.kind) || (STATIC_HEAVY.contains(&m.kind) && m.static_flag == 1))
    });
    let placed = heavy.flat_map(|mesh| {
        mesh.objects.iter().map(move |o| {
            let at = CarPose {
                position: [o.position.x, o.position.y, o.position.z],
                orientation: o.values,
                velocity: [0.0; 3],
            };
            let footprint = mesh.vertices.iter().map(|v| {
                let w = at.to_world([v.x * o.extent.x, v.y * o.extent.y, v.z * o.extent.z]);
                [w[0], w[1]]
            });
            Obstacle { footprint: footprint.collect() }
        })
    });
    placed.collect()
}

/// A prop along the driveline: its footprint's extent along it (m) and to its left (m, negative right).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placed {
    from: f32,
    to: f32,
    left: [f32; 2],
}

/// A driveline tangent longer than `LOOP` times its segment's chord makes the segment loop: RSF stage 302's driveline
/// starts with a 10.7 m segment that ends on a 164 m tangent, and the car steered off the road at the start. So does a
/// segment that runs back along its chord by more than `BACK` m: RSF 370's 13 m segment at 454 m ends on an 86 m
/// tangent (6.6 times the chord) and bulges 5 m to the side, and the car followed it into the bank. The controller
/// shortens such a segment's tangents to the chord (ours; the game's driveline parameter still follows the loop). The
/// retail drivelines' largest ratio is 5.9 and their furthest run back 1.0 m (both track-51_N), so they are unchanged.
const LOOP: f32 = 8.0;
const BACK: f32 = 1.5;

/// How far the Hermite segment `p0`..`p1` with tangents `m0`, `m1` runs back along its chord (m), sampled.
fn runs_back(p0: [f32; 3], m0: [f32; 3], p1: [f32; 3], m1: [f32; 3]) -> f32 {
    let chord = sub(p1, p0);
    let length = dot(chord, chord).sqrt();
    let (mut ahead, mut back) = (0.0f32, 0.0f32);
    for k in 1..=16 {
        let along = dot(sub(hermite(p0, m0, p1, m1, k as f32 / 16.0), p0), chord) / length;
        ahead = ahead.max(along);
        back = back.max(ahead - along);
    }
    back
}

fn hermite(p0: [f32; 3], m0: [f32; 3], p1: [f32; 3], m1: [f32; 3], t: f32) -> [f32; 3] {
    let (t2, t3) = (t * t, t * t * t);
    let h = [2.0 * t3 - 3.0 * t2 + 1.0, t3 - 2.0 * t2 + t, -2.0 * t3 + 3.0 * t2, t3 - t2];
    std::array::from_fn(|k| h[0] * p0[k] + h[1] * m0[k] + h[2] * p1[k] + h[3] * m1[k])
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Hermite position on the driveline at parameter `u` (record index plus fraction; `CCar` +0x17c).
fn point(track: &[DriveLineRecord], u: f32) -> [f32; 3] {
    let last = track.len() - 1;
    let u = u.clamp(0.0, last as f32);
    let i = (u as usize).min(last.saturating_sub(1));
    let t = u - i as f32;
    let v = |v: Vec3| [v.x, v.y, v.z];
    let length = |v: [f32; 3]| dot(v, v).sqrt();
    let (p0, p1) = (v(track[i].vectors[0]), v(track[i + 1].vectors[0]));
    let (m0, m1) = (v(track[i].vectors[1]), v(track[i + 1].vectors[1]));
    let chord = length(sub(p1, p0));
    let loops = runs_back(p0, m0, p1, m1) > BACK;
    let fit = |m: [f32; 3]| {
        let long = length(m) > chord && (loops || length(m) > LOOP * chord);
        let scale = if long { chord / length(m) } else { 1.0 };
        m.map(|x| x * scale)
    };
    hermite(p0, fit(m0), p1, fit(m1), t)
}

/// The driveline parameter at distance `d` along the stage (the inverse of `driveline::sample_x`).
fn parameter(values: &[f32], d: f32) -> f32 {
    let i = values.partition_point(|&v| v <= d).clamp(1, values.len() - 1) - 1;
    let span = values[i + 1] - values[i];
    i as f32 + ((d - values[i]) / span).clamp(0.0, 1.0)
}

fn distance(values: &[f32], u: f32) -> f32 {
    rbr_gameplay::driveline::sample_x(values, u.max(0.0)).unwrap_or(*values.last().unwrap())
}

/// Signed horizontal angle from `from` to `to` (z-up), positive counter-clockwise.
fn angle(from: [f32; 3], to: [f32; 3]) -> f32 {
    (from[0] * to[1] - from[1] * to[0]).atan2(from[0] * to[0] + from[1] * to[1])
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|k| a[k] - b[k])
}

/// How far the car leans to its right: its up axis's component along its horizontal right.
fn lean(pose: &CarPose) -> f32 {
    let (forward, up) = (pose.forward(), pose.rotate([0.0, 0.0, 1.0]));
    let length = forward[0].hypot(forward[1]);
    (up[0] * forward[1] - up[1] * forward[0]) / length
}

/// What the controller reads after a step.
pub struct Sense {
    pub pose: CarPose,
    /// `CCar` +0x17c.
    pub parameter: f32,
    pub gear: u32,
    /// rad/s.
    pub engine: f32,
    /// An outside assistance holds the car (`HELD`).
    pub held: bool,
    /// The wheels' rim speeds |ωR| (m/s, FL FR RL RR), as the FMOD block reads them after the step.
    pub wheels: [f32; 4],
    /// The engine speed the gear gives the drive's speed (rad/s, 0 out of a forward gear): above it the clutch slips.
    pub geared: f32,
}

/// The car's sense from an answer's frame and HUD record, and the drive's engine speed as the VCU reads it (the drive
/// state's +0x45c, which the shift points were tuned on; the HUD's is the engine's own).
pub(crate) fn sense(answer: &[u8], live: &Live) -> Sense {
    let mem = live.mem();
    let assist = live.game().race_clock().assist_timer;
    let car = &answer[HEADER_BYTES + CONTROL_BYTES..];
    let hud = &answer[FRAME_BYTES..FRAME_BYTES + HUD2_BYTES];
    let gear = read_u32(hud, 4).expect("HUD gear");
    let gearbox = crate::live::gearbox(mem, mem.u32(pure::CAR_PTR) + CAR_DRIVE);
    Sense {
        pose: pose::car_pose(car).expect("car block"),
        parameter: read_f32(car, DRIVELINE_OFFSET).expect("car +0x17c"),
        gear,
        engine: mem.f32(mem.u32(mem.u32(pure::CAR_PTR) + DRIVE_STATE) + DRIVE_ENGINE_SPEED),
        held: (0.0..HELD).contains(&assist),
        wheels: FmodBlock::read_from(mem.u32(pure::CAR_PTR), |a| mem.u32(a)).wheel_speeds.map(f32::abs),
        geared: u8::try_from(gear).ok().filter(|&g| g >= 2).and_then(|g| gearbox.engine_speed(g)).unwrap_or(0.0),
    }
}

/// What the controller is doing: following the driveline, backing up (since the step), or shifting back to first.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Drive,
    Reverse(u32),
    Forward,
}

/// The controller's state.
pub struct Driver<'a> {
    track: &'a [DriveLineRecord],
    values: Vec<f32>,
    /// The step of the last shift.
    shifted: u32,
    /// m/s.
    top: f32,
    aim: Option<Aim>,
    mode: Mode,
    /// The car has moved faster than `MOVING`.
    moved: bool,
    /// Steps stuck in a row while driving.
    slow: u32,
    /// Recoveries since the car last made `PROGRESS` m, and the distance at the last one.
    attempts: u32,
    stuck_at: f32,
    /// The run is stuck for good: why.
    pub gave_up: Option<String>,
    /// Per gear: the lowest engine speed per m/s seen.
    ratio: [f32; TOP_GEAR as usize + 1],
    /// The last control had throttle and no brake.
    pulling: bool,
    /// Physics steps per 1.02 step: the step counts here are 1.02's (7 ms), and NGP steps 1/720 s (5 per 7 ms).
    per: u32,
    /// The car's rev limit, rad/s.
    limit: f32,
    /// Steps bogged in first in a row (`BOG`).
    bogged: u32,
    /// The heavy props near the driveline, by `from`.
    props: Vec<Placed>,
    /// A rally school lesson's stop: the driveline distance the brakes stay off before and the one to stop at, m.
    stop: Option<(f32, f32)>,
    /// The drive runs down the driveline, to a stop behind the start.
    against: bool,
    /// `--flat-out` (pace measurement): hold full throttle without braking; the steering, the traction easing and
    /// the gear shifts stay the controller's (`target_speed`, `control`).
    flat_out: bool,
    /// `--flat-out` on an NGP car: the torque curve and gear ratios the thrust-crossover shifts read (`start`), if
    /// the live car gave them.
    start: Option<start::Curve>,
    /// `--drive-slip K` (with `--flat-out`): the drive-slip controller's target and integral (`start::Drive`), none
    /// without the flag.
    drive: Option<start::Drive>,
    /// The clutch launch (`start::Launch`): `--drive-slip` where the driver has the clutch (`--clutch-help off`).
    launch: Option<start::Launch>,
    /// The flat-out shift points are logged once per run.
    start_logged: bool,
    /// `--circle` (pace measurement): the steady-state circle to steer, if any.
    circle: Option<Circle>,
    /// `--circle-ramp` (pace measurement): the circle target's rise (m/s per s), `CIRCLE_RAMP` unless given.
    circle_ramp: f32,
    /// The circle run's ramp clock (s): the target speed rises with it, from the first driven step on.
    circle_t: f32,
    /// `--circle-past T` (limit tests, `PAST_WIDE`): the throttle past the limit, and the seconds since the car first
    /// ran wide (negative before).
    past: Option<(f32, f32)>,
    /// `--circle` without `--circle-past`: the hold at the limit (`limit`), and the inward steering the last control
    /// held (of full lock).
    hold: limit::LimitHold,
    inward: f32,
    /// `--brake-from` (pace measurement): brake once the speed reaches this (m/s), if any.
    brake_at: Option<f32>,
    /// The brake test's brake is on (latched when the speed reaches `brake_at`).
    braking: bool,
    /// The brake test's pedal: held (`--brake-pedal`, 1 unless given), threshold braking (`--brake-slip`) or the
    /// per-wheel ABS (`--brake-abs`).
    brake: Brake,
    /// `--brake-declutch`: the brake test holds the clutch pedal in and shifts no gear (the engine off the wheels).
    declutch: bool,
    /// A rally school lesson's judged speed bands: the driveline window and the top speed in it, m/s.
    bands: Vec<SpeedBand>,
    /// The grip factor for the race's wetness (1 dry): the plan's lateral and braking rates scale with it.
    grip: f32,
    /// The speed plan's dry lateral and braking acceleration (m/s²): `LATERAL` and `BRAKING`, or `--plan`'s.
    plan: Plan,
    /// `--plan`: the race pedal's cap, the threshold controller's integral (`PLAN_SLIP`); none without the flag.
    antilock: Option<f32>,
    /// A rally school lesson drives (`in_lesson`): no declutch under braking or restart (`DECLUTCH`, `STALLED`).
    lesson: bool,
    /// Neutral is held under braking or for a restart (`DECLUTCH`, `STALLED`).
    neutral: bool,
    /// Steps in a row the wheels have dragged the engine under braking (`DECLUTCH`).
    dragging: u32,
    /// Steps since the restart of a stalled engine began (`STALLED`), while it lasts.
    crank: Option<u32>,
    /// `--race`: the line and its plan, and the steps each guard held the speed under the plan (full lock, tracking).
    race: Option<race::Line>,
    pub guarded: [u32; 2],
    /// `--manoeuvre` (`pad`): an open-loop handling manoeuvre on the flat pad.
    pad: Option<pad::Pad>,
}

/// The pace-measurement modes of an autopilot drive (`--flat-out`, `--brake-from` and its `--brake-pedal`,
/// `--brake-slip`, `--brake-abs`, `--brake-hold`, `--brake-declutch`, `--circle`, `--circle-ramp`, `--trace-speed`); the default drives as a race.
#[derive(Clone, Debug, Default)]
pub struct Measure {
    pub flat_out: bool,
    /// `--drive-slip K` (with `--flat-out`): while the car accelerates, the throttle is modulated so the
    /// most-slipping wheel's drive slip ratio holds near K (a driver's throttle foot, not NGP's clutch help).
    pub drive_slip: Option<f32>,
    /// The driver has the clutch pedal (`--clutch-help off`).
    pub clutch: bool,
    pub brake_from: Option<f32>,
    /// `--brake-pedal P`, `--brake-slip K` (`Brake`) and `--brake-hold S` (s on the brake after the stop).
    pub brake_pedal: Option<f32>,
    pub brake_slip: Option<f32>,
    /// `--brake-abs K` (`Brake::Abs`).
    pub brake_abs: Option<f32>,
    pub brake_hold: Option<f32>,
    /// `--brake-declutch`.
    pub brake_declutch: bool,
    pub circle: Option<Circle>,
    pub circle_ramp: Option<f32>,
    /// `--circle-past T` (`PAST_WIDE`).
    pub circle_past: Option<f32>,
    pub trace_speed: Option<std::path::PathBuf>,
    /// `--tyre-probe FILE`: read-only dumps of each wheel's tyre frame (research tooling).
    pub tyre_probe: Option<std::path::PathBuf>,
    /// `--tyre-probe-every N`: the probe's interval in steps.
    pub tyre_probe_every: Option<u32>,
    /// `--tyre-image FILE [--tyre-image-step N]`: a memory image of the wheels' tyre footprints (research tooling).
    pub tyre_image: Option<std::path::PathBuf>,
    pub tyre_image_step: Option<u32>,
    /// `--plan LAT,BRAKE`: the speed plan's lateral and braking acceleration (`Plan`).
    pub plan: Option<Plan>,
    /// `--race LAT,BRAKE,SLIP`: race driving on a line across the road at the car's limits (`race`).
    pub race: Option<race::Race>,
    /// `--manoeuvre KIND,KMH,AMP[,EXTRA]` (`pad`, on a pad stage).
    pub manoeuvre: Option<Manoeuvre>,
}

/// `--circle X,Y,R` (pace measurement): a steady-state circle to drive, counter-clockwise, its centre (m) and
/// radius (m). The controller steers a pure pursuit on the circle while the target speed ramps from
/// `CIRCLE_START` at `CIRCLE_RAMP` (m/s per s) to the top, so the car holds ever faster steady states until it can
/// no longer hold the radius. Once more steering stops buying lateral acceleration the steering is held there
/// (`limit`, not under `--circle-past`), so the car runs wide at its limit instead of sliding round on more lock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle {
    pub center: [f32; 2],
    pub radius: f32,
}

impl std::str::FromStr for Circle {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let v: Vec<f32> =
            s.split(',').map(|x| x.parse().map_err(|_| format!("--circle {s}"))).collect::<Result<_, _>>()?;
        match v[..] {
            [x, y, r] if r > 0.0 => Ok(Circle { center: [x, y], radius: r }),
            _ => Err(format!("--circle {s}: want X,Y,R with R > 0")),
        }
    }
}

/// `--plan LAT,BRAKE` (limit runs, `harness/limit_pace.py`): the speed plan's dry lateral and braking acceleration
/// (m/s², both over 0) in place of `LATERAL` and `BRAKING`; the wetness's grip still scales them. Nothing else of the
/// controller changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    pub lateral: f32,
    pub braking: f32,
}

impl Plan {
    pub const DEFAULT: Plan = Plan { lateral: LATERAL, braking: BRAKING };
}

impl std::str::FromStr for Plan {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let v: Vec<f32> =
            s.split(',').map(|x| x.parse().map_err(|_| format!("--plan {s}"))).collect::<Result<_, _>>()?;
        match v[..] {
            [lateral, braking] if lateral > 0.0 && braking > 0.0 => Ok(Plan { lateral, braking }),
            _ => Err(format!("--plan {s}: want LAT,BRAKE in m/s², both over 0")),
        }
    }
}

/// `--circle-past T` (with `--circle`, limit tests): once the car first runs `PAST_WIDE` m outside the circle (past the
/// limit, the ramp still rising), the throttle is held at T (0-1; 0 lifts) with no brake and no gear change for the
/// rest of the run, while the pure pursuit keeps steering for the circle (it countersteers a car whose nose points
/// inside the target, as a driver would). Whether and how fast the car regains the circle, and the grip it keeps
/// past the limit, are the measure (`harness/limit_recovery.py`).
const PAST_WIDE: f32 = 1.0;
/// `--circle-past` looks for the limit, and `--circle`'s hold (`limit`) for steering that no longer buys lateral
/// acceleration, only after this many seconds of the ramp (the car settles onto the circle from the start first).
const PAST_SETTLE: f32 = 10.0;

/// The brake test's pedal (`--brake-from`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Brake {
    /// `--brake-pedal P`: this pedal (0-1) held to the stop (1, full, unless given).
    Pedal(f32),
    /// `--brake-slip K`: threshold braking, the pedal modulated so the most-slipping wheel's slip ratio holds near
    /// `target` (a driver's threshold braking: one pedal for the four wheels, not a per-wheel ABS). `held` is the
    /// controller's integral, the pedal it settles on.
    Slip { target: f32, held: f32 },
    /// `--brake-abs K`: a per-wheel ABS, the full pedal with each wheel's pressure scaled (`scale`, FL FR RL RR, 0-1)
    /// so its slip ratio from free rolling holds near `target` (`ABS_LIFT`, `ABS_BAND`, `ABS_DUMP`, `ABS_APPLY`).
    /// `free` sums each wheel's free-rolling |ωR| / v over the lift, `lift` counts its steps. The scales go to the
    /// brake step through `rbr_hybrid::ngp::set_brake_scale` (NGP cars only).
    Abs { target: f32, scale: [f32; 4], free: [f32; 4], lift: u32 },
}

impl Brake {
    /// Threshold braking at slip ratio `target`, from a released pedal.
    pub fn slip(target: f32) -> Self {
        Brake::Slip { target, held: 0.0 }
    }

    /// The per-wheel ABS at slip ratio `target`, from the full pressure on every wheel.
    pub fn abs(target: f32) -> Self {
        Brake::Abs { target, scale: [1.0; 4], free: [0.0; 4], lift: 0 }
    }
}

/// The per-wheel ABS's reference (s): before braking it lifts off for this long, declutched, and takes each wheel's
/// free-rolling |ωR| / v over the second half (the wheels settle in ~50 ms), as an ABS learns its wheels' rolling
/// radii. The rim speed |ωR| is not the ground speed of a free wheel: declutched coasts read 1 - |ωR| / v 0.054-0.061
/// on RSF 345 gravel and 0.022-0.028 on RSF 568 tarmac at every speed over 4 m/s, NGP and the brush alike.
const ABS_LIFT: f32 = 0.2;

/// The per-wheel ABS's law, a two-threshold pressure modulator as a simple ABS valve block runs it: each step, a
/// wheel whose slip ratio is over `target + ABS_BAND` has its pressure dumped at `ABS_DUMP` (scale per s), one under
/// `target - ABS_BAND` has it reapplied at `ABS_APPLY`, and in between it holds. Dumping faster than reapplying, as
/// real ABS does, cycles each wheel about the target (measured cycle rates in `notes/tyre-model.md`, "A per-wheel ABS
/// in the brake test"; a real ABS cycles at ~5-15 Hz). Each step reads the slip after the step before.
const ABS_BAND: f32 = 0.02;
const ABS_DUMP: f32 = 20.0;
const ABS_APPLY: f32 = 5.0;

/// The wheels' slip ratios from free rolling at speed `v`: each rim speed against `v` times its free-rolling ratio,
/// the lift's `sum` over `n` steps. A wheel whose rim never turned in the lift (sum 0) keeps the raw ratio 1.
fn abs_slips(wheels: [f32; 4], v: f32, sum: [f32; 4], n: f32) -> [f32; 4] {
    std::array::from_fn(|k| {
        let free = if sum[k] > 0.0 { sum[k] / n } else { 1.0 };
        1.0 - wheels[k] / (v * free)
    })
}

/// The per-wheel ABS's step: the scales after `dt` s from `scale` at the wheels' `slips` from free rolling.
fn abs_step(target: f32, scale: [f32; 4], slips: [f32; 4], dt: f32) -> [f32; 4] {
    std::array::from_fn(|k| {
        let rate = if slips[k] > target + ABS_BAND {
            -ABS_DUMP
        } else if slips[k] < target - ABS_BAND {
            ABS_APPLY
        } else {
            0.0
        };
        (scale[k] + rate * dt).clamp(0.0, 1.0)
    })
}

/// Threshold braking's PI controller on the slip error (target minus the most-slipping wheel's slip): the pedal is
/// the integral, `SLIP_I` pedal per s per unit of error, plus `SLIP_P` times the error. Each step reads the slip
/// after the step before (one step late, 1/720 s under NGP); the u8 pedal quantizes the sum, not the integral.
/// Fiesta Rally2 from 100 and 50 km/h, declutched (RSF 568 tarmac, 345 gravel; NGP and the brush): the most-slipping
/// wheel's slip above 3 m/s, past the onset, averages within 0.008 of K for K 0.1-0.7, spread 0.004-0.024 up to K 0.2,
/// 0.02-0.04 at 0.3 and 0.02-0.09 at 0.5-0.7. In gear with clutch help on, the idling engine turning the wheels
/// widens it (0.025 off K at 0.3, 0.43-0.50 at 0.5).
const SLIP_I: f32 = 60.0;
const SLIP_P: f32 = 1.0;
/// `--plan`'s race braking: the pedal stays under threshold braking's at this slip ratio (most-slipping wheel). At
/// the plan's limit the plain pedal (up to 0.8) locked the fronts entering corners under load transfer (Fiesta Rally2
/// on RSF 524 at 3980 m, ngp, plan 5.6/8.1: fronts 1.3 and 4.1 m/s at 15.6 m/s, then off the line at full lock).
const PLAN_SLIP: f32 = 0.2;
/// The slip ratio's floor on the speed (m/s): under it the ratio is (v - ωR) / `SLIP_SPEED`, so the stop's last
/// metre neither divides by zero nor takes the wheel speeds' noise as slip.
const SLIP_SPEED: f32 = 1.0;

/// The most-slipping wheel's slip ratio under braking, 1 - |ωR| / v, the car's speed for each wheel's ground speed
/// (a straight stop).
fn slip(sense: &Sense) -> f32 {
    let v = sense.pose.speed();
    sense.wheels.iter().map(|&w| (v - w) / v.max(SLIP_SPEED)).fold(f32::NEG_INFINITY, f32::max)
}

/// A judged speed band of a rally school lesson's filter bank (a check with both speed bounds, km/h): the car must
/// stay under the top in its driveline window (lesson 1 cruises at 108 km/h through its 90 km/h 69.7-707.5 m band),
/// with 2 km/h of margin for the tracking overshoot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeedBand {
    pub from: f32,
    pub to: f32,
    /// m/s.
    pub hi: f32,
}

impl<'a> Driver<'a> {
    pub fn new(track: &'a [DriveLineRecord], values: Vec<f32>, top: f32, aim: Option<Aim>) -> Self {
        Self {
            track,
            values,
            shifted: 0,
            top,
            aim,
            mode: Mode::Drive,
            moved: false,
            slow: 0,
            attempts: 0,
            stuck_at: 0.0,
            gave_up: None,
            ratio: [f32::INFINITY; TOP_GEAR as usize + 1],
            pulling: false,
            per: 1,
            limit: f32::INFINITY,
            bogged: 0,
            props: Vec::new(),
            stop: None,
            against: false,
            flat_out: false,
            start: None,
            drive: None,
            launch: None,
            start_logged: false,
            circle: None,
            circle_ramp: CIRCLE_RAMP,
            circle_t: 0.0,
            past: None,
            hold: limit::LimitHold::default(),
            inward: 0.0,
            brake_at: None,
            braking: false,
            brake: Brake::Pedal(1.0),
            declutch: false,
            bands: Vec::new(),
            grip: 1.0,
            plan: Plan::DEFAULT,
            antilock: None,
            lesson: false,
            neutral: false,
            dragging: 0,
            crank: None,
            race: None,
            guarded: [0; 2],
            pad: None,
        }
    }

    /// The driver racing on `line` (`--race`): its steering target and speed plan replace the driveline's, and the
    /// brake stays under threshold braking's at the line's slip, as `--plan`'s does at `PLAN_SLIP`.
    pub fn with_race(self, line: race::Line) -> Self {
        Self { race: Some(line), antilock: Some(1.0), ..self }
    }

    /// The heavy props by the driveline (`with_obstacles`).
    fn props(&self) -> &[Placed] {
        &self.props
    }

    /// The driver on a surface with `grip` of the dry grip (1 dry, less on a damp or wet race: `with_lesson`).
    pub fn with_grip(self, grip: f32) -> Self {
        Self { grip, ..self }
    }

    /// The driver planning its speed with `plan`'s lateral and braking acceleration (`--plan`) in place of `LATERAL`
    /// and `BRAKING`.
    pub fn with_plan(self, plan: Plan) -> Self {
        Self { plan, antilock: Some(1.0), ..self }
    }

    /// The driver holding the lesson's judged speed bands (`bands`): the target speed stays under each band's top in
    /// its window, braking for it from ahead.
    pub fn with_bands(self, bands: Vec<SpeedBand>) -> Self {
        Self { bands, ..self }
    }

    /// The driver in a rally school lesson.
    pub fn in_lesson(self) -> Self {
        Self { lesson: true, ..self }
    }

    /// The driver keeping off the brakes before the driveline distance `brake_from`, then stopping at `at` and
    /// standing there.
    pub fn with_stop(self, brake_from: f32, at: f32) -> Self {
        Self { stop: Some((brake_from, at)), ..self }
    }

    /// The driver following the driveline down to the stop `at` behind the start and standing there: the free
    /// lessons 0 and 10 start facing the yellow box that ends them, which lies before the start on the driveline.
    pub fn with_stop_behind(self, at: f32) -> Self {
        Self { stop: Some((0.0, at)), against: true, ..self }
    }

    /// The driver steering around `obstacles`: each is placed by its centre's nearest driveline point (sampled).
    pub fn with_obstacles(self, obstacles: &[Obstacle]) -> Self {
        let samples: Vec<(f32, [f32; 3])> =
            (0..(self.track.len() - 1) * 8).map(|k| k as f32 / 8.0).map(|u| (u, point(self.track, u))).collect();
        let mut props: Vec<Placed> = obstacles
            .iter()
            .filter_map(|o| {
                let n = o.footprint.len() as f32;
                let c = o.footprint.iter().fold([0.0; 2], |c, v| [c[0] + v[0] / n, c[1] + v[1] / n]);
                let off = |p: [f32; 3]| (p[0] - c[0]).hypot(p[1] - c[1]);
                let &(u, p) = samples.iter().min_by(|a, b| off(a.1).total_cmp(&off(b.1)))?;
                let ahead = sub(point(self.track, u + 0.01), p);
                let length = ahead[0].hypot(ahead[1]);
                let (t, at) = ([ahead[0] / length, ahead[1] / length], distance(&self.values, u));
                let span = |f: &dyn Fn([f32; 2]) -> f32| {
                    let values = o.footprint.iter().map(|v| f([v[0] - p[0], v[1] - p[1]]));
                    values.fold([f32::INFINITY, f32::NEG_INFINITY], |r, x| [r[0].min(x), r[1].max(x)])
                };
                let along = span(&|d| at + d[0] * t[0] + d[1] * t[1]);
                let left = span(&|d| d[1] * t[0] - d[0] * t[1]);
                (left[0] < SWING + CLEAR && left[1] > -SWING - CLEAR).then_some(Placed {
                    from: along[0],
                    to: along[1],
                    left,
                })
            })
            .collect();
        props.sort_by(|a, b| a.from.total_cmp(&b.from));
        Self { props, ..self }
    }

    /// How far left of the driveline (m) the car passes the props between `at` and `ahead`: the free offset nearest the
    /// driveline, within `SWING`, from as many of the props as allow one, nearest first.
    fn dodge(&self, at: f32, ahead: f32) -> f32 {
        let near: Vec<[f32; 2]> = self
            .props
            .iter()
            .filter(|p| p.to + PASSED >= at && p.from <= ahead)
            .map(|p| [p.left[0] - CLEAR, p.left[1] + CLEAR])
            .collect();
        (0..=near.len())
            .rev()
            .find_map(|n| {
                let blocked = |s: f32| near[..n].iter().any(|b| s > b[0] && s < b[1]);
                let edges = near[..n].iter().flatten().copied();
                let free = std::iter::once(0.0).chain(edges).filter(|s| s.abs() <= SWING && !blocked(*s));
                free.min_by(|a, b| a.abs().total_cmp(&b.abs()))
            })
            .unwrap_or(0.0)
    }

    /// The aim point when it is in range and ahead of the car.
    fn aim_target(&self, pose: &CarPose) -> Option<[f32; 3]> {
        let at = self.aim?.at;
        let to = sub([at[0], at[1], pose.position[2]], pose.position);
        let ahead = angle(pose.forward(), to).abs() < std::f32::consts::FRAC_PI_2;
        (to[0].hypot(to[1]) < AIM_RANGE && ahead).then_some([at[0], at[1], pose.position[2]])
    }

    /// The curve limit `s` metres ahead of `at`: the speed that holds `lateral` (m/s²) there, uncapped by braking.
    /// The plan passes its own lateral at the wetness's grip; the kink scan passes the dry `LATERAL` whatever the
    /// grip or `--plan`: a kink is sharp geometry, and `KINK_SLOW` is set on that scale.
    fn curve_limit(&self, at: f32, s: f32, lateral: f32) -> f32 {
        let u = |d: f32| parameter(&self.values, at + d);
        let (a, b, c) =
            (point(self.track, u(s)), point(self.track, u(s + PLAN_STEP)), point(self.track, u(s + 2.0 * PLAN_STEP)));
        let turn = angle(sub(b, a), sub(c, b)).abs();
        let curvature = turn / PLAN_STEP;
        (lateral / curvature.max(1e-4)).sqrt().min(self.top)
    }

    /// The target speed: the slowest of the curve limits ahead, each reachable by braking; the top under
    /// `--flat-out`, with no curve, kink, tracking or prop-dodge slowdown; the circle's ramp under `--circle`.
    pub fn target_speed(&self, at: f32) -> f32 {
        if self.circle.is_some() {
            return self.top.min(CIRCLE_START + self.circle_ramp * self.circle_t);
        }
        if self.flat_out {
            return self.top;
        }
        if let Some(line) = &self.race {
            return line.speed(at).min(self.top);
        }
        let braking = self.plan.braking * self.grip;
        let lateral = self.plan.lateral * self.grip;
        if self.against {
            let stop = self.stop.map_or(0.0, |(_, s)| s);
            return self.top.min((2.0 * braking * (at - stop).max(0.0)).sqrt());
        }
        let mut target = self.top;
        let mut s = 0.0;
        while s < PLAN_AHEAD {
            let limit = self.curve_limit(at, s, lateral);
            target = target.min((limit * limit + 2.0 * braking * s).sqrt());
            s += PLAN_STEP;
        }
        let on_line = self.props.iter().filter(|p| p.left[0] - CLEAR < 0.0 && p.left[1] + CLEAR > 0.0);
        for p in on_line.filter(|p| p.to + PASSED >= at && p.from < at + PLAN_AHEAD) {
            let s = (p.from - LEAD - at).max(0.0);
            target = target.min((DODGE_TOP * DODGE_TOP + 2.0 * braking * s).sqrt());
        }
        for b in self.bands.iter().filter(|b| b.to >= at && b.from < at + PLAN_AHEAD) {
            if at >= b.from {
                target = target.min(b.hi);
            } else {
                target = target.min((b.hi * b.hi + 2.0 * braking * (b.from - at)).sqrt());
            }
        }
        if let Some((brake_from, stop)) = self.stop {
            if at < brake_from {
                return self.top;
            }
            target = target.min((2.0 * braking * (stop - at).max(0.0)).sqrt());
        }
        target
    }

    /// The driver for physics steps of `seconds` (the step counts scale to 1.02's 7 ms steps).
    pub fn with_step(self, seconds: f32) -> Self {
        Self { per: (rbr_recording::STEP_SECONDS / seconds).round().max(1.0) as u32, ..self }
    }

    /// The driver holding full throttle without braking (`--flat-out`, pace measurement) or not.
    pub fn with_flat_out(self, flat_out: bool) -> Self {
        Self { flat_out, ..self }
    }

    /// The flat-out start's torque curve and gear ratios (`start::Curve`), read from the live car.
    pub fn with_start_curve(self, curve: start::Curve) -> Self {
        Self { start: Some(curve), ..self }
    }

    /// The flat-out start's drive-slip target K (`--drive-slip`, `start::Drive`).
    pub fn with_drive_slip(self, target: f32) -> Self {
        Self { drive: Some(start::Drive::new(target)), ..self }
    }

    /// The driver has the clutch pedal (`--clutch-help off`): a start with a slip target and the torque curve slips
    /// the clutch (`start::Launch`).
    pub fn with_clutch(self) -> Self {
        Self { launch: (self.drive.is_some() && self.start.is_some()).then(start::Launch::new), ..self }
    }

    /// The launch's engine speed (rad/s), on a clutch launch still to close.
    fn launch_revs(&self) -> Option<f32> {
        let curve = self.start.as_ref()?;
        self.launch.filter(|l| !l.locked).map(|_| start::power_peak(&curve.knots) * std::f32::consts::PI / 30.0)
    }

    /// The start countdown's control on a clutch launch: on the handbrake in first with the clutch in, the engine
    /// at the launch revs. `None` otherwise (the caller revs in neutral).
    pub fn countdown(&mut self, step: u32, sense: &Sense) -> Option<QuantizedControl> {
        let revs = self.launch_revs()?;
        let flags = if sense.gear < FIRST { self.shift(step, SHIFT_GAP, 0x01) } else { 0 };
        Some(QuantizedControl {
            flags,
            handbrake: 255,
            clutch: 255,
            throttle: (start::rev_hold(sense.engine, revs) * 255.0).round() as u8,
            ..QuantizedControl::default()
        })
    }

    /// NGP's steps (not 1.02's).
    pub(crate) fn is_ngp(&self) -> bool {
        self.per > 1
    }

    /// The driver steering the steady-state `circle` (`--circle`, pace measurement) or none.
    pub fn with_circle(self, circle: Circle) -> Self {
        Self { circle: Some(circle), ..self }
    }

    /// The driver ramping the circle target at `ramp` m/s per s (`--circle-ramp`, repeat runs).
    pub fn with_circle_ramp(self, ramp: f32) -> Self {
        Self { circle_ramp: ramp, ..self }
    }

    /// The driver holding `throttle` once past the circle's limit (`--circle-past`, limit tests).
    pub fn with_circle_past(self, throttle: f32) -> Self {
        Self { past: Some((throttle, -1.0)), ..self }
    }

    /// `--circle-past`: the seconds since the car first ran wide, else `None`.
    pub fn past_since(&self) -> Option<f32> {
        self.past.map(|(_, t)| t).filter(|&t| t >= 0.0).or_else(|| self.pad.as_ref().and_then(|p| p.since()))
    }

    /// The driver braking fully from `v` m/s on (`--brake-from`, pace measurement) or never.
    pub fn with_brake_from(self, v: f32) -> Self {
        Self { brake_at: Some(v), ..self }
    }

    /// The brake test's pedal (`--brake-pedal`, `--brake-slip`).
    pub fn with_brake(self, brake: Brake) -> Self {
        Self { brake, ..self }
    }

    /// The per-wheel ABS's pressure scales for the next step (`Brake::Abs` while the brake test brakes), else `None`.
    pub fn brake_scale(&self) -> Option<[f32; 4]> {
        match self.brake {
            Brake::Abs { scale, .. } if self.braking => Some(scale),
            _ => None,
        }
    }

    /// The brake test with the clutch pedal in and no gear changes from its start (`--brake-declutch`), or not.
    pub fn with_declutch(self, declutch: bool) -> Self {
        Self { declutch, ..self }
    }

    /// The driver holds no slowdown: `--flat-out` or `--circle`, where the curve, kink, tracking and prop-dodge
    /// slowdowns stay off and only the steering, the traction easing and the gear shifts drive.
    fn free(&self) -> bool {
        self.flat_out || self.circle.is_some()
    }

    /// The steering target on `circle`, counter-clockwise: the circle point a lookahead arc ahead of the car's
    /// nearest circle point (pure pursuit), at the car's height.
    fn circle_point(&self, pose: &CarPose, speed: f32, circle: Circle) -> [f32; 3] {
        let dx = pose.position[0] - circle.center[0];
        let dy = pose.position[1] - circle.center[1];
        let ahead = (LOOK_BASE + LOOK_TIME * speed) / circle.radius;
        let (s, c) = (dy.atan2(dx) + ahead).sin_cos();
        [circle.center[0] + circle.radius * c, circle.center[1] + circle.radius * s, pose.position[2]]
    }

    /// The driver for a car whose rev limit is `rpm`.
    pub fn with_limit(self, rpm: f32) -> Self {
        Self { limit: rpm * std::f32::consts::PI / 30.0, ..self }
    }

    /// The highest gear learnt to keep the engine over `SHIFT_DOWN` at `speed` (m/s), else first.
    /// Whether an upshift is due: flat out on an NGP car with the torque curve, the next gear's thrust at the ground
    /// speed wins or the engine is at the rev limit less `start::LIMIT_MARGIN` (`start::shift_up`);
    /// else past the rev point `up` without wheelspin.
    fn wants_up(&self, sense: &Sense, speed: f32, up: f32, start: bool, spinning: bool) -> bool {
        if sense.gear < FIRST || sense.gear >= TOP_GEAR {
            return false;
        }
        if !start {
            return sense.engine > up && !spinning;
        }
        let Some(curve) = &self.start else { return sense.engine > up && !spinning };
        let g = sense.gear as usize;
        match (
            curve.ratios.get(g).copied(),
            curve.ratios.get(g + 1).copied(),
            start::gain(&self.ratio, &curve.ratios, sense.gear + 1),
        ) {
            (Some(now), Some(next), Some(k)) if now > 0.0 && next > 0.0 => {
                start::shift_up(&curve.knots, now, next, sense.engine, speed * k, self.limit)
            }
            _ => sense.engine > up && !spinning,
        }
    }

    /// The flat-out start's torque curve and gear ratios from `live`'s car (`start::Curve`), if they read.
    pub(crate) fn start_curve(live: &Live) -> Option<start::Curve> {
        let mem = live.mem();
        let car = mem.u32(pure::CAR_PTR);
        let knots = crate::physics::torque_knots(mem, live.ngp_base(), car)?;
        if knots.len() < 2 {
            return None;
        }
        let ratios = crate::live::gearbox(mem, car + CAR_DRIVE).ratios.to_vec();
        let torque_peak = start::torque_peak(&knots);
        Some(start::Curve { knots, ratios, torque_peak })
    }

    fn gear_for(&self, speed: f32) -> u32 {
        (FIRST..=TOP_GEAR)
            .rev()
            .find(|&g| self.ratio[g as usize].is_finite() && self.ratio[g as usize] * speed > SHIFT_DOWN)
            .unwrap_or(FIRST)
    }

    /// `n` 1.02 steps in this run's steps.
    fn steps(&self, n: u32) -> u32 {
        n * self.per
    }

    /// A gear pulse (`flags`) when `gap` 1.02 steps have passed since the last one.
    fn shift(&mut self, step: u32, gap: u32, flags: u8) -> u8 {
        if step < self.shifted + self.steps(gap) {
            return 0;
        }
        self.shifted = step;
        flags
    }

    /// Counts the steps stuck while driving and starts a recovery, or gives up, after `RECOVER_AFTER` of them.
    fn watch(&mut self, step: u32, at: f32, sense: &Sense, error: f32) {
        if self.mode != Mode::Drive {
            return;
        }
        let speed = sense.pose.speed();
        self.moved |= speed > MOVING;
        let stuck = speed < STUCK_SPEED || (speed < BACKWARDS_SPEED && error.abs() > BACKWARDS);
        self.slow = if self.moved && stuck { self.slow + 1 } else { 0 };
        if at > self.stuck_at + PROGRESS {
            self.attempts = 0;
        }
        if self.slow < self.steps(RECOVER_AFTER) {
            return;
        }
        self.slow = 0;
        if sense.pose.rotate([0.0, 0.0, 1.0])[2] < ROLLED {
            self.gave_up = Some(format!("stuck at step {step} (on its side or roof)"));
            return;
        }
        if self.attempts == ATTEMPTS {
            self.gave_up =
                Some(format!("stuck at step {step} (for {RECOVER_AFTER} steps after {ATTEMPTS} recoveries)"));
            return;
        }
        self.attempts += 1;
        self.stuck_at = at;
        self.mode = Mode::Reverse(step);
        eprintln!(
            "autopilot: step {step}: stuck at {at:.1} m in gear {}, backing up (attempt {})",
            sense.gear, self.attempts
        );
    }

    /// The control for `step` (emitted before it), from the state after the step before.
    pub fn control(&mut self, step: u32, sense: &Sense, finished: bool) -> QuantizedControl {
        if sense.held {
            (self.mode, self.slow, self.bogged) = (Mode::Drive, 0, 0);
            return QuantizedControl { brake: 255, ..QuantizedControl::default() };
        }
        if self.pad.is_some() {
            return self.pad_control(step, sense);
        }
        let speed = sense.pose.speed();
        let mut cap = None;
        if self.circle.is_some() {
            // The ramp runs on the driven steps only (this function is not called while held or pending).
            let dt = rbr_recording::STEP_SECONDS / self.per.max(1) as f32;
            self.circle_t += dt;
            // Armed once settled on the circle and above the ramp's start (a spun car's crawl is no limit).
            let armed = self.circle_t > PAST_SETTLE && speed > CIRCLE_START;
            let set;
            (cap, set) = self.hold.observe(sense.pose.velocity, self.inward, dt, armed);
            if set && self.past.is_none() {
                eprintln!(
                    "autopilot: step {step}: at the circle's limit (steering {:.3} no longer buys lateral \
                     acceleration, {:.3} g at {speed:.2} m/s): steering held there",
                    cap.unwrap_or(0.0),
                    self.hold.lateral().unwrap_or(0.0) / 9.81,
                );
            }
        }
        if self.brake_at.is_some_and(|v| speed >= v) {
            self.braking = true;
        }
        let at = distance(&self.values, sense.parameter);
        let mut look = LOOK_BASE + LOOK_TIME * speed;
        if self.aim.is_none() && !self.against && self.race.is_none() {
            // Pull the target nearer through windings: while the driveline ahead turns away, aim at its near part.
            let mut turn = 0.0;
            let mut s = LOOK_BASE;
            let mut prev = point(self.track, parameter(&self.values, at));
            while s < look {
                s += 2.0;
                let p = point(self.track, parameter(&self.values, at + s));
                let q = point(self.track, parameter(&self.values, at + s + 2.0));
                turn += angle(sub(p, prev), sub(q, p)).abs();
                prev = p;
                if turn > TURN_IN {
                    look = s;
                    break;
                }
            }
        }
        let aimed = self.aim_target(&sense.pose);
        let swerve = if aimed.is_some() || self.against || self.free() || self.race.is_some() {
            0.0
        } else {
            self.dodge(at, at + look + LEAD)
        };
        let target = if let Some(circle) = self.circle {
            self.circle_point(&sense.pose, speed, circle)
        } else if let Some(line) = &self.race {
            line.point(at + look)
        } else {
            aimed.unwrap_or_else(|| {
                if self.against {
                    return point(self.track, parameter(&self.values, at - look));
                }
                let u = parameter(&self.values, at + look);
                let p = point(self.track, u);
                if swerve == 0.0 {
                    return p;
                }
                let ahead = sub(point(self.track, u + 0.01), p);
                let length = ahead[0].hypot(ahead[1]);
                [p[0] - swerve * ahead[1] / length, p[1] + swerve * ahead[0] / length, p[2]]
            })
        };
        let error = angle(sense.pose.forward(), sub(target, sense.pose.position));
        if finished {
            self.mode = Mode::Drive;
        } else if self.aim.is_none() && self.stop.is_none() && !self.braking && self.circle.is_none() {
            self.watch(step, at, sense, error);
        }
        // Counter-clockwise (left) error wants left (negative) steering; backing up, right swings the nose left.
        let (mut steer, mut throttle, mut brake, mut flags) = ((-error * STEER_GAIN).clamp(-1.0, 1.0), 0.0, 1.0, 0);
        if let Some(line) = &mut self.race {
            if line.loose() && self.mode == Mode::Drive && speed > 3.0 {
                steer = line.steer(at, &sense.pose, rbr_recording::STEP_SECONDS / self.per.max(1) as f32);
            }
        }
        match self.mode {
            Mode::Reverse(_) if sense.gear > REVERSE => {
                steer = 0.0;
                flags = self.shift(step, RECOVER_GAP, 0x02);
            }
            Mode::Reverse(since) => {
                let fast = self.per > 1 && speed > REVERSE_TOP;
                (steer, throttle, brake) = (-steer, if fast { 0.0 } else { REVERSE_THROTTLE }, 0.0);
                if step >= since + self.steps(REVERSE_MAX)
                    || (step >= since + self.steps(REVERSE_MIN) && error.abs() < AIMED)
                {
                    self.mode = Mode::Forward;
                }
            }
            Mode::Forward if sense.gear < FIRST => flags = self.shift(step, RECOVER_GAP, 0x01),
            Mode::Forward | Mode::Drive => {
                self.mode = Mode::Drive;
                let off = sub(sense.pose.position, point(self.track, sense.parameter));
                let err = match &self.race {
                    Some(line) => race::off_line(line, at, sense.pose.position),
                    None => (off[0].hypot(off[1]) - swerve.abs()).abs(),
                };
                // Race: the plan's speed `ANTICIPATE` s ahead; full lock is the race steering's.
                let (ahead, lock, track_err) = match &self.race {
                    Some(_) => (speed * race::ANTICIPATE, steer.abs() >= 1.0, race::TRACK_ERR),
                    None => (0.0, (error * STEER_GAIN).abs() > 1.0, TRACK_ERR),
                };
                let mut want = if finished { 0.0 } else { self.target_speed(at + ahead) };
                let planned = want;
                if !self.free() && lock {
                    want = want.min(FULL_LOCK_TOP * self.grip);
                    if self.race.is_some() && want < planned {
                        self.guarded[0] += 1;
                    }
                }
                if !self.free() && !finished && err > track_err {
                    // The tracking guard: the steered line is the driveline swung by the dodge offset; further from
                    // it than `TRACK_ERR`, the car is sliding, so slow down proportionally until it answers again.
                    want = want.min((speed * track_err / err).max(TRACK_MIN));
                    if self.race.is_some() && want < planned {
                        self.guarded[1] += 1;
                    }
                }
                if let (Some(cap), None) = (cap, self.past) {
                    // `--circle`: steering no longer buys lateral acceleration (`limit`); the speed ramp runs wide.
                    steer = steer.max(-cap);
                }
                let lean = lean(&sense.pose);
                if lean.abs() > TIP {
                    steer = lean.signum();
                }
                let gap = want - speed;
                // A very slow corner looms within `KINK_AHEAD` m: brake hard now, while straight, to arrive on
                // line (the gentle pedal below arrives hot and slides wide). Only while tracking well; a sliding
                // car takes the tracking guard's proportional slow instead of piling on brake in the turn.
                let mut slow = f32::INFINITY;
                if !self.free() && self.race.is_none() {
                    let mut s = 0.0;
                    while s < KINK_AHEAD {
                        slow = slow.min(self.curve_limit(at, s, LATERAL));
                        s += PLAN_STEP;
                    }
                }
                let kink = !self.free() && !finished && slow < KINK_SLOW && err < TRACK_ERR && gap < -1.0;
                (throttle, brake) = if let (Some(line), false) = (&self.race, finished) {
                    race::pedals(gap, race::sideslip(&sense.pose), line.loose())
                } else if finished || want <= 0.0 {
                    (0.0, 1.0)
                } else if gap > 0.0 {
                    ((0.4 + 0.2 * gap).min(1.0), 0.0)
                } else if gap > -1.5 && !kink {
                    (0.15, 0.0)
                } else if kink {
                    (0.0, 0.8)
                } else {
                    (0.0, (-0.15 * gap).min(0.8))
                };
                if let Some(held) = &mut self.antilock {
                    // `--plan`, `--race`: the pedal held under the threshold controller's at `PLAN_SLIP` or the line's
                    // slip (`Brake::Slip`'s law).
                    if brake > 0.0 && !finished {
                        let target = self.race.as_ref().map_or(PLAN_SLIP, |line| line.slip);
                        let error = target - slip(sense);
                        let dt = rbr_recording::STEP_SECONDS / self.per.max(1) as f32;
                        *held = (*held + SLIP_I * error * dt).clamp(0.0, 1.0);
                        brake = brake.min((*held + SLIP_P * error).clamp(0.0, 1.0));
                    } else {
                        *held = 1.0;
                    }
                }
                let g = sense.gear.min(TOP_GEAR) as usize;
                // NGP's clutch help opens the clutch below about 100 rad/s even under a light throttle (`notes/ngp.md`,
                // "Controls under NGP"), so a run of NGP's steps learns only above the downshift speed.
                let engaged = self.per == 1 || sense.engine > SHIFT_DOWN;
                if self.pulling
                    && engaged
                    && speed > RATIO_SPEED
                    && sense.gear >= FIRST
                    && step >= self.shifted + self.steps(RATIO_SETTLE)
                {
                    self.ratio[g] = self.ratio[g].min(sense.engine / speed);
                }
                let spinning = sense.engine > SLIP * self.ratio[g] * speed;
                // NGP's steps (the clutch help above).
                let ngp = self.per > 1;
                let bogged = ngp && sense.gear == FIRST && speed < BOG_SPEED && sense.engine < BOG * self.limit;
                self.bogged = if bogged && step > self.shifted { self.bogged + 1 } else { 0 };
                let launch = !ngp || speed > BOG_SPEED || sense.engine > LAUNCH * self.limit;
                let up = if self.flat_out { UP_LIMIT * self.limit } else { SHIFT_UP.min(UP_LIMIT * self.limit) };
                // Flat out on an NGP car with the torque curve: the slip controller replaces the traction cap below,
                // and the thrust crossover the rev-point shift (`start`).
                let start = self.flat_out && ngp && self.start.is_some();
                if start && !self.start_logged {
                    self.start_logged = true;
                    let curve = self.start.as_ref().expect("logged with the curve");
                    let points = start::shift_points(&curve.knots, &curve.ratios, FIRST, TOP_GEAR, self.limit);
                    let points = points
                        .iter()
                        .map(|&(g, rpm)| format!("{}>{g} {rpm:.0} rpm", g - 1))
                        .collect::<Vec<_>>()
                        .join(", ");
                    eprintln!("autopilot: flat-out shift points {points}");
                    eprintln!(
                        "autopilot: flat-out power peak {:.0} rpm, torque peak {:.0} rpm",
                        start::power_peak(&curve.knots),
                        curve.torque_peak * 30.0 / std::f32::consts::PI
                    );
                }
                if ngp && spinning && speed > BOG_SPEED && !(start && self.drive.is_some()) {
                    throttle = throttle.min(TRACTION);
                }
                if let Some(drive) = &mut self.drive {
                    if start && !finished && throttle > 0.0 && self.launch.is_none_or(|l| l.locked) {
                        // At or below the torque peak the engine has no revs to spare, so the throttle stays full
                        // and the integral parks (`start::drive_pedal`); above it the slip target holds as before.
                        let peak = self.start.as_ref().map_or(f32::INFINITY, |c| c.torque_peak);
                        let slip = start::drive_slip(sense.wheels, speed);
                        let dt = rbr_recording::STEP_SECONDS / self.per.max(1) as f32;
                        let (pedal, held) = start::drive_pedal(drive.target, drive.held, slip, dt, sense.engine, peak);
                        drive.held = held;
                        throttle = throttle.min(pedal);
                    }
                }
                if !finished {
                    if self.bogged > self.steps(BOG_STEPS) {
                        flags = self.shift(step, SHIFT_GAP, 0x02);
                    } else if (sense.gear < FIRST && launch) || self.wants_up(sense, speed, up, start, spinning) {
                        flags = self.shift(step, SHIFT_GAP, 0x01);
                    } else if sense.gear > FIRST
                        && (sense.engine < SHIFT_DOWN
                            || (self.free() && self.pulling && sense.engine < LUG_ENGINE && speed < LUG_SPEED))
                    {
                        flags = self.shift(step, SHIFT_GAP, 0x02);
                    }
                }
            }
        }
        let mut clutch = 0;
        if self.braking {
            // The brake test: its pedal with the computed steering (`--brake-from`); declutched, the idling engine
            // cannot turn the wheels against the brake.
            throttle = 0.0;
            if self.declutch {
                (flags, clutch) = (0, 255);
            }
            brake = match &mut self.brake {
                Brake::Pedal(pedal) => *pedal,
                Brake::Slip { target, held } => {
                    let error = *target - slip(sense);
                    let dt = rbr_recording::STEP_SECONDS / self.per.max(1) as f32;
                    *held = (*held + SLIP_I * error * dt).clamp(0.0, 1.0);
                    (*held + SLIP_P * error).clamp(0.0, 1.0)
                }
                Brake::Abs { target, scale, free, lift } => {
                    let dt = rbr_recording::STEP_SECONDS / self.per.max(1) as f32;
                    let steps = (ABS_LIFT / dt).round() as u32;
                    let v = sense.pose.speed().max(SLIP_SPEED);
                    if *lift < steps {
                        // The lift: no brake, the free-rolling reference summed over its second half.
                        *lift += 1;
                        if *lift > steps / 2 {
                            *free = std::array::from_fn(|k| free[k] + sense.wheels[k] / v);
                        }
                        0.0
                    } else {
                        let n = (steps - steps / 2).max(1) as f32;
                        let slips = abs_slips(sense.wheels, v, *free, n);
                        *scale = abs_step(*target, *scale, slips, dt);
                        1.0
                    }
                }
            };
        }
        if let (Some(revs), Some(drive)) = (self.launch_revs(), self.drive) {
            // The clutch launch: the clutch holds the slip target, the throttle the launch revs, in first until the
            // clutch has closed.
            let launch = self.launch.as_mut().expect("its revs");
            if self.mode == Mode::Drive && !finished && sense.gear >= FIRST {
                let dt = rbr_recording::STEP_SECONDS / self.per.max(1) as f32;
                let slip = start::launch_slip(sense.wheels, sense.pose.speed());
                let floor = self.start.as_ref().map_or(0.0, |c| c.torque_peak);
                let (held, pedal) = start::clutch_step(launch.clutch, slip, drive.target, sense.engine, floor, dt);
                launch.clutch = held;
                launch.locked = pedal == 0.0 && (sense.engine - sense.geared).abs() < start::LOCKED * sense.engine;
                (throttle, brake, flags) = (start::rev_hold(sense.engine, revs), 0.0, 0);
                clutch = (pedal * 255.0).round() as u8;
            }
        }
        let (mut aux, mut select) = (0, 0);
        if self.per > 1 && !self.lesson && !self.free() && self.brake_at.is_none() {
            // Race driving under NGP: restart a stalled engine, and hold neutral while locked wheels drag the engine
            // under braking (`DECLUTCH`, `STALLED`).
            self.crank = match self.crank {
                None if sense.engine.abs() < STALLED && !finished => Some(0),
                Some(n) if sense.engine < STARTED => Some(n + 1),
                _ => None,
            };
            let braking = self.mode == Mode::Drive && brake > 0.0;
            let dragged = braking
                && sense.gear >= FIRST
                && sense.engine < DECLUTCH
                && (sense.engine - sense.geared).abs() < DRAGGED
                && slip(sense) > LOCK_SLIP;
            self.dragging = if dragged { self.dragging + 1 } else { 0 };
            let hold = self.crank.is_some() || (braking && (self.neutral || self.dragging >= self.steps(DRAG_STEPS)));
            if hold {
                flags = 0;
                if sense.gear != NEUTRAL && (!self.neutral || step >= self.shifted + self.steps(RECOVER_GAP)) {
                    (flags, select, self.shifted) = (SELECT, NEUTRAL, step);
                }
            } else if self.neutral && self.mode == Mode::Drive && sense.gear < FIRST && speed > BOG_SPEED {
                (flags, select, self.shifted) = (SELECT, self.gear_for(speed), step);
            }
            self.neutral = hold;
            if let Some(n) = self.crank {
                (throttle, brake, self.slow) = (0.0, 1.0, 0);
                aux = u8::from(n % self.steps(CRANK_CYCLE) < self.steps(CRANK_HOLD));
            }
        }
        if let (Some((held, t)), Some(circle)) = (&mut self.past, self.circle) {
            // `--circle-past`: from the first time the car runs `PAST_WIDE` m wide, its throttle with no brake and no
            // gear change; the steering stays the pursuit's.
            let wide = (sense.pose.position[0] - circle.center[0]).hypot(sense.pose.position[1] - circle.center[1])
                - circle.radius;
            if *t >= 0.0 {
                *t += rbr_recording::STEP_SECONDS / self.per.max(1) as f32;
            } else if wide > PAST_WIDE && self.circle_t > PAST_SETTLE {
                *t = 0.0;
            }
            if *t >= 0.0 {
                (throttle, brake, flags, select) = (*held, 0.0, 0, 0);
            }
        }
        self.pulling = self.mode == Mode::Drive && throttle > 0.0 && brake == 0.0;
        self.inward = -steer;
        let pedal = |x: f32| (x * 255.0).round() as u8;
        QuantizedControl {
            flags,
            steering: (steer * 127.5).round().clamp(-127.0, 127.0) as i8,
            throttle: pedal(throttle),
            brake: pedal(brake),
            clutch,
            aux,
            gear: select as u8,
            ..QuantizedControl::default()
        }
    }
}

/// The car's rev limit (rpm).
fn rev_limit(live: &Live) -> f32 {
    let mem = live.mem();
    mem.f32(mem.u32(mem.u32(pure::CAR_PTR) + VCU_DEF) + VCU_RPM_LIMIT)
}

#[cfg(test)]
mod tests;
