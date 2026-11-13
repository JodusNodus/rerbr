//! The tyre sounds from each wheel's `CSigTyreNoise` block (`rbr_recording::sound`, `tyres`): the surface loops of
//! `CTireSnd::M5fd0` (0x4d5fd0, read in the disassembly) and the off-road rattles of `CCarSound::Mce10` (0x4ace10).
//! Pure: gains, pitches and the samples to play per step. rbr-recomp `notes/sound.md` section 6.
//!
//! Each wheel has two surface voices playing the `TIRE::SURFACES` loops of the surface's two materials (gravel,
//! tarmac or snow; the skid loop of each above a grip maximum of 1.25), at one pitch from the surface table. A level
//! from the slip, the tyre factor and the speed sets their volume, split between the two by the table's share. Ours:
//! the skid loops fade in instead ([`SKID_FADE`]), the tarmac one (the squeal) over the rolling loop by the sliding
//! power ([`SQUEAL_POWER`]), the loose ones in place of theirs at 1.02's level.
//! The third and fourth voices of a wheel are the rumble under the car (`M6750` 0x4d6750, in the car only, off a
//! surface whose first material is tarmac) and the wetness (`TIRE::SURFACES SkidWater` at the `CSigTireParticles`
//! splash that `M6560` 0x4d6560 keeps). Per frame, `M7330` (0x4d7330) plays the gravel hits (`IMPACT::GRUS`) off
//! tarmac at a rate from the tyre's force. `M7ea0` (0x4d7ea0, with fewer than 0x21 free hardware channels: every
//! system without DirectSound hardware mixing, which is any since Windows Vista) is not a mix of the four wheels: it
//! renders each wheel's two surface voices through the software 3D path (`E30MixerBase::Mce250`), so each wheel plays
//! at its full level times that path's distance gain (the mixer's `Place::Tire`), both voices at the first's volume. The original's frame-rate updates run here once per
//! physics step (ours).

use rbr_formats::exe_data;

use crate::voice::{Command, Material, Shot, WheelLoop};

/// The 1.02 exe's surface table (`E31Tire::M5f80`, `exe_data`), one row of six words per surface id: the gain bits
/// (f32), the first and second material (0 gravel, 1 tarmac, 2 snow), the pitch and the second material's share
/// (16.16), and a variant word (0, 0x400 or 0x800) the handler reads and then discards.
const SURFACE_TABLE: u32 = 0x0073_5C80;

/// Surface `id`'s row of [`SURFACE_TABLE`].
fn surface_row(id: u32) -> [u32; 6] {
    std::array::from_fn(|k| exe_data::u32(SURFACE_TABLE + 0x18 * (id & 0xff) + 4 * k as u32).unwrap_or_default())
}

/// The words of the block the handler reads (word 0, the wheel, is the slot the block comes in).
const MAX: usize = 1;
const GRIP_MAX: usize = 4;
const FACTOR: usize = 5;
const FORCE_SUM: usize = 2;
const SLIP_RATIO: usize = 8;
const SPEED: usize = 9;
const SURFACE: usize = 0x14;
/// The second surface of the contact's material pair and its share (the `pair` dwords' second and third).
const SECOND_SURFACE: usize = 0x15;
const SECOND_SHARE: usize = 0x16;
const CONTACT: usize = 0x17;

/// A grip maximum above this plays the skid loops in 1.02 (ours: [`SKID_FADE`]); at 2 (0x72ad94) its share of the
/// level is full, and from here it rises by 4/3 (0x737488) per unit. Below it the share is 0.5 on a surface whose first material is tarmac (0x795e18)
/// and 0 on the others. NGP's patch of `M5fd0` (0x4d6197, 0x4d62a2) reads the threshold from NGP.dll (`10173478` on a
/// tarmac-first surface, `1017347c` on the others) instead of the exe's 1.25 (0x7344d8): NGP's settings
/// `tarmacSkidSoundThreshold` and `skidSoundThreshold` in percent (10 to 200), whose default 100 is 1 (both 1 in every
/// NGP snapshot).
const SKID: f32 = 1.0;
const SKID_FULL: f32 = 2.0;
const SKID_RISE: f32 = 1.333_333_4;
const TARMAC_REST: f32 = 0.5;
/// Ours (`notes/sound.md` "Tyres and surfaces"): the skid loops fade in with the grip word, `0.75 * phase + 0.5`
/// (`rbr_physics::tyre::noise_signal`; NGP's slip phase, 1 at the friction peak, 2 sliding), from half way to the
/// peak to the peak, instead of 1.02's switch at [`SKID`] (two thirds of the way), as more of the contact patch slides.
const SKID_FADE: [f32; 2] = [0.875, 1.25];
/// Ours: the squeal's volume is the power the sliding rubber dissipates, the normal force (word 5, N) times a slide
/// speed: the contact's speed (word 9; near the peak the lateral slide is a share of it, folded in here) plus the spin
/// slip (word 8, rad/s) times a nominal radius. Full at [`SQUEAL_POWER`] W, the 90th percentile while squealing on an
/// NGP tarmac drive (RSF 1654149, median a third of it). The mixer takes the square root, so the squeal's level in dB
/// follows the power's. The rolling loop plays on under it.
const SQUEAL_POWER: f32 = 150_000.0;
const SQUEAL_RADIUS: f32 = 0.315;
/// Ours: past the peak the squeal's pitch falls to this factor at a full slide, the rougher scrub of the whole patch.
const SQUEAL_SLIDE_PITCH: f32 = 0.9;
/// The tyre factor's share: full above 5000 (0x731054), else times 0.0002 (0x73748c).
const FACTOR_FULL: f32 = 5000.0;
const FACTOR_SCALE: f32 = 0.0002;
/// The slip ratio's share: full above 40 (0x73089c), else times 0.025 (0x737484).
const SLIP_FULL: f32 = 40.0;
const SLIP_SCALE: f32 = 0.025;
/// The speed term: speed times 0.05 (0x72dd78) plus slip ratio times 0.1 (0x72b068), at most 1.
const SPEED_SCALE: f32 = 0.05;
const SLIP_SPEED_SCALE: f32 = 0.1;
/// The level is `(slip + skid + factor) * 2000 (0x731d00) * speed term`, truncated and capped at 4000, and the
/// volumes take it times 1/4000 (0x737480).
const LEVEL_SCALE: f32 = 2000.0;
const LEVEL_CAP: u32 = 4000;
const LEVEL_TO_VOLUME: f32 = 0.000_25;
/// 16.16 to a float (0x7319f4).
const FIXED: f32 = 1.0 / 65536.0;

/// The rumble: `x = min(speed * 0.1 (0x72b068), 1)`, the level `min((word 1 + 1 + factor * 1.5625e-5 (0x737490))
/// * x, 1)`, the voice's volume moving a quarter (0x730888) of the way there each frame and set at half (0x795e18) of
/// it times the tyre volume; pitch `x * 0.4 + 0.8` (0x72f664, 0x72f610).
const RUMBLE_SPEED: f32 = 0.1;
const RUMBLE_FACTOR: f32 = 1.5625e-5;
const RUMBLE_SMOOTH: f32 = 0.25;
const RUMBLE_GAIN: f32 = 0.5;
const RUMBLE_PITCH: (f32, f32) = (0.4, 0.8);
/// Ours (game feel): in the car a wheel's rolling loops (not the skid loops) rise in pitch above the speed where the
/// level saturates ([`SPEED_SCALE`]: 20 m/s, 72 km/h), linearly to [`ROLL_PITCH_RISE`] more at [`ROLL_PITCH_TOP`]
/// m/s (180 km/h), held above; the level is left as it is. 1.02 plays them at the surface table's pitch only.
const ROLL_PITCH_FROM: f32 = 1.0 / SPEED_SCALE;
const ROLL_PITCH_TOP: f32 = 50.0;
const ROLL_PITCH_RISE: f32 = 0.2;

/// The in-car rolling loops' pitch factor at wheel speed `speed` m/s (ours, [`ROLL_PITCH_RISE`]).
fn roll_pitch(speed: f32) -> f32 {
    1.0 + ROLL_PITCH_RISE * ((speed - ROLL_PITCH_FROM) / (ROLL_PITCH_TOP - ROLL_PITCH_FROM)).clamp(0.0, 1.0)
}

/// The wetness voice decays by 0.95 (0x7355d4) a frame, to 0 below 0.001 (0x795e94); the splash sets it, at most 1.
const WET_DECAY: f32 = 0.95;
const QUIET: f32 = 0.001;

/// Ours: each wheel's water as the renderer sees it ([`Tyres::set_water`]; its wheel spray's), in place of 1.02's
/// splash, 0 in every drive:
/// - running into a puddle: a splash (`Shot::Splash`, the embedded recordings of a car hitting water, its impact in
///   them) once the depth stops rising, if it rose by at least [`SPLASH_MIN`] times [`SPLASH_SPEED`], at
///   [`SPLASH_GAIN`] times the rise, lower the deeper ([`SPLASH_DROP`]); the rise also sets the gush, which decays as
///   1.02's wetness ([`WET_DECAY`] a frame) and ducks the rest while the splash plays;
/// - wet tarmac's film (`Water::film`) never splashes or gushes: it drives the swish and hiss alone;
/// - in a puddle: a swish on the wetness voice (`TIRE::SURFACES SkidWater`, the retail `water.wav`) by its depth and
///   the speed ([`SWISH_GAIN`]);
/// - on a wet road: the wet-road loop (`WheelLoop::Hiss`) by the ground's wetness squared ([`HISS_GAIN`]) over
///   [`HISS_SPEED`], darker and quieter on loose ground ([`LOOSE_PITCH`], [`LOOSE_HISS`]), brighter with speed
///   ([`WATER_PITCH`]).
///
/// The gush overrides the swish and the swish the hiss: their shares of 1 add up to at most 1. Both loops play at the
/// tyres' distance and, in the car, through the cabin low-pass (`crate::mixer`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Water {
    /// How much of a puddle the standing water is, 0 to 1 (the spray's plume depth: 0 under a wet rut's film).
    pub puddle: f32,
    /// The ground's wetness: 0 dry, 0.5 damp, 1 wet.
    pub wet: f32,
    /// Gravel, dirt or grass rather than tarmac.
    pub loose: bool,
    /// The standing water is wet tarmac's even film, not a puddle: it swishes but never splashes, and counts as no
    /// depth for the next puddle's rise.
    pub film: bool,
}

/// The wheel speeds, m/s, over which a puddle splashes from nothing to full (the spray's).
const SPLASH_SPEED: [f32; 2] = [2.0, 15.0];
const SPLASH_MIN: f32 = 0.15;
const SPLASH_GAIN: f32 = 1.6;
const SPLASH_DROP: f32 = 0.15;
const SWISH_GAIN: f32 = 0.35;
/// The wheel speeds, m/s, over which a wet road's hiss rises from nothing (about 7 km/h) to full (90 km/h).
const HISS_SPEED: [f32; 2] = [2.0, 25.0];
const HISS_GAIN: f32 = 0.5;
const LOOSE_HISS: f32 = 0.6;
const LOOSE_PITCH: f32 = 0.78;
/// The water's pitch from standstill to [`WATER_PITCH_TOP`] m/s; the swish's times 0.85 at full puddle depth.
const WATER_PITCH: [f32; 2] = [0.85, 1.1];
const WATER_PITCH_TOP: f32 = 40.0;
const DEEP_PITCH: f32 = 0.85;

fn smoothstep([from, to]: [f32; 2], x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A wheel's swish and hiss shares of 1 at wheel speed `speed` m/s in `water` under a gush of `gush` (ours, [`Water`]):
/// the gush overrides the swish, the swish the hiss.
fn water_shares(water: &Water, speed: f32, gush: f32) -> (f32, f32) {
    let swish = water.puddle * smoothstep(SPLASH_SPEED, speed) * (1.0 - gush);
    let hiss = water.wet * water.wet * smoothstep(HISS_SPEED, speed) * if water.loose { LOOSE_HISS } else { 1.0 };
    (swish, hiss * (1.0 - gush - swish).max(0.0))
}

/// The water loops' pitch in `water` at wheel speed `speed`, a puddle's swish lower the deeper (ours, [`Water`]).
fn water_pitch(water: &Water, speed: f32, swish: bool) -> f32 {
    let base = if water.loose { LOOSE_PITCH } else { 1.0 };
    let pace = WATER_PITCH[0] + (WATER_PITCH[1] - WATER_PITCH[0]) * (speed / WATER_PITCH_TOP).clamp(0.0, 1.0);
    let deep = if swish { 1.0 - (1.0 - DEEP_PITCH) * water.puddle } else { 1.0 };
    base * pace * deep
}

/// The gravel hits: off a tarmac-first surface a hit is due every `trunc(50 / (force sum + 0.01))` frames (0x7304a0,
/// 0x72f440), from 1 to 999, while the wheel touches the ground and the car is within 20 m (0x72f614) of the
/// listener. The pitch is the surface table's word 0 plus `(rand() % 10) * 0.02 - 0.1` (0x72fcb4, 0x72b068).
const GRUS_RATE: f32 = 50.0;
const GRUS_FORCE: f32 = 0.01;
const GRUS_FRAMES: u32 = 1000;
const GRUS_NEAR: f32 = 20.0;
const GRUS_PITCH_STEP: f32 = 0.02;
const GRUS_PITCH_OFFSET: f32 = 0.1;
/// Ours: a wheel's hits come at most every [`GRUS_MIN_FRAMES`] frames, and those due more often than every
/// [`GRUS_DENSE`] frames play at `sqrt(frames / GRUS_DENSE)`, so that dense gravel keeps the loudness of 10 hits a
/// second a wheel instead of a constant crackle (1.02: up to 60 at full volume).
const GRUS_MIN_FRAMES: u32 = 3;
const GRUS_DENSE: u32 = 6;
/// The game's frames, which the gravel hits' counter, the rumble and the wetness decay run on (`Vf0c` 0x4d8290 from the
/// frame update, no time step): 60 a second of the drive's time here, as 1.02 runs with `WaitRetrace = true` (the
/// retail `RichardBurnsRally.ini`) on a 60 Hz display (ours: the original's rate follows its frame rate).
pub(crate) const FRAME_RATE: f32 = 60.0;

/// The off-road rattles: at or above 5 km/h (0x72ae44) with all four wheels off the road, one plays each
/// `10 / speed` seconds (0x72b154).
const OFFROAD_SPEED: f32 = 5.0;
const OFFROAD_PERIOD: f32 = 10.0;
/// The off-road samples (`CAR::OTHER Offroad1-7`).
pub const OFFROAD_SAMPLES: usize = 7;

/// How far the wheel of tyre-noise block `sig` (its position at `+0xc`, the slot's `+0x7c`) is from `at`.
pub(crate) fn distance(sig: &[u32; 24], at: [f32; 3]) -> f32 {
    (0..3).map(|i| (f32::from_bits(sig[0xc + i]) - at[i]).powi(2)).sum::<f32>().sqrt()
}

/// One wheel's surface voices for a step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelSound {
    /// The two materials and their rolling loops' volumes (before the tyre gain): 1.02's level below the skid
    /// threshold, whatever the grip word.
    pub materials: [(Material, f32); 2],
    /// The skid loops' share, 0 to 1 ([`SKID_FADE`]; 1.02 switches to them above [`SKID`]).
    pub skid: f32,
    /// A loose material's skid loop at full share: 1.02's level with its skid share, at the first voice's volume. The
    /// loose rolling loops give way to it.
    pub skid_level: f32,
    /// The tarmac skid loop's (the squeal's) at full share, over the tarmac rolling loop ([`SQUEAL_POWER`]).
    pub squeal: f32,
    pub pitch: f32,
    /// The squeal's pitch factor, lower as the patch slides past the peak ([`SQUEAL_SLIDE_PITCH`]).
    pub squeal_pitch: f32,
    /// The surface is not one of the road surfaces (`E31_5fa0` gives 1).
    pub off_road: bool,
    /// The surface's first material is tarmac (`+0x278`: no rumble, no gravel hits).
    pub tarmac: bool,
    /// The gravel hits' period in frames (`+0x280`; 0 on tarmac) and their base pitch (the table's word 0, `+0x2a0`).
    pub grus_frames: u32,
    pub grus_pitch: f32,
}

fn material(word: u32) -> Material {
    match word {
        1 => Material::Tarmac,
        2 => Material::Snow,
        _ => Material::Gravel,
    }
}

/// `M5fd0` on one block: `None` when the wheel has no contact (its voices go silent). NGP's patch
/// (0x4d60ab) takes the pair's second surface when its share is above 0.5 (0x795e18), its own road list
/// (`10171280`, called at 0x4d60df) and its skid threshold ([`NGP_SKID`]).
pub fn wheel(sig: &[u32; 24], road_surfaces: &[u32]) -> Option<WheelSound> {
    if sig[CONTACT] == 0 {
        return None;
    }
    let f = |i: usize| f32::from_bits(sig[i]);
    let surface = if f(SECOND_SHARE) > 0.5 { sig[SECOND_SURFACE] } else { sig[SURFACE] };
    let row = surface_row(surface);
    let first = material(row[1]);
    let grip = f(GRIP_MAX);
    let factor = if f(FACTOR) > FACTOR_FULL { 1.0 } else { f(FACTOR) * FACTOR_SCALE };
    let rest = if first == Material::Tarmac { TARMAC_REST } else { 0.0 };
    let skid_share = if grip >= SKID_FULL {
        1.0
    } else if grip >= SKID {
        (grip - SKID) * SKID_RISE
    } else {
        rest
    };
    let slip = f(SLIP_RATIO);
    let slip_share = if slip > SLIP_FULL { 1.0 } else { slip * SLIP_SCALE };
    let speed = (f(SPEED) * SPEED_SCALE + slip * SLIP_SPEED_SCALE).min(1.0);
    let volume = |skid_share: f32| {
        let level = ((slip_share + skid_share + factor) * LEVEL_SCALE * speed) as i32 as u32;
        level.min(LEVEL_CAP) as f32 * LEVEL_TO_VOLUME
    };
    let x = volume(rest);
    let share = row[4] as f32 * FIXED;
    let power = f(FACTOR) * (f(SPEED) + SQUEAL_RADIUS * slip);
    Some(WheelSound {
        materials: [(first, (1.0 - share) * x), (material(row[2]), share * x)],
        skid: if grip.is_nan() { 0.0 } else { smoothstep(SKID_FADE, grip) },
        skid_level: (1.0 - share) * volume(skid_share),
        squeal: (1.0 - share) * (power / SQUEAL_POWER).clamp(0.0, 1.0),
        pitch: row[3] as f32 * FIXED,
        squeal_pitch: 1.0 - (1.0 - SQUEAL_SLIDE_PITCH) * smoothstep([SKID_FADE[1], SKID_FULL], grip),
        off_road: !road_surfaces.contains(&surface),
        tarmac: first == Material::Tarmac,
        grus_frames: if first == Material::Tarmac {
            0
        } else {
            (GRUS_RATE / (f(FORCE_SUM) + GRUS_FORCE)) as i32 as u32
        },
        grus_pitch: f32::from_bits(row[0]),
    })
}

/// The tyres' state between steps.
#[derive(Debug, Clone, Default)]
pub struct Tyres {
    road_surfaces: Vec<u32>,
    /// `GravelParticles` amp times `SOUNDDEF::TIRE` volume.
    pub gain: f32,
    /// `Offtrack` amp times the effects volume.
    pub offroad_gain: f32,
    /// The off-road timer (`+0x41944`).
    timer: f32,
    /// Per wheel: the contact flag, the tarmac flag and the hit period as the last block left them (`+0x40`,
    /// `+0x278`, `+0x280`; a block without contact leaves the last two), the hits' base pitch, the frame counter
    /// (`+0xb74`), the rumble voice's volume and the wetness level.
    contact: [bool; 4],
    tarmac: [bool; 4],
    grus_frames: [u32; 4],
    grus_pitch: [f32; 4],
    grus_count: [u32; 4],
    /// The frames of [`FRAME_RATE`] not yet counted.
    frames: f32,
    rumble: [f32; 4],
    wet: [f32; 4],
    /// The last splash each wheel's `CSigTireParticles` carried (`+0xac`).
    splash: [f32; 4],
    /// `IMPACT::GRUS` samples.
    pub grus_count_samples: usize,
    /// Ours ([`Water`]): each wheel's water from the renderer, whether it is new since the last step, the puddle
    /// depth it last gave, the rise since the depth last stopped growing, the gush, and the splash samples
    /// (`data::CarBank::splashes`) and the splashes made, which pick them in turn (not from the random numbers, so
    /// the other sounds stay as without water).
    water: Option<[Water; 4]>,
    fresh: bool,
    depth: [f32; 4],
    rise: [f32; 4],
    gush: [f32; 4],
    pub splash_samples: usize,
    splashes: usize,
}

impl Tyres {
    /// Ours (`crate::impact`'s slide): `None` with a wheel on the ground, else whether the ground the wheels last ran
    /// on is loose (fewer than two on a tarmac-first surface).
    pub fn slide(&self) -> Option<bool> {
        (!self.contact.contains(&true)).then(|| self.tarmac.iter().filter(|&&t| t).count() < 2)
    }

    pub fn new(gain: f32, offroad_gain: f32, grus_samples: usize, road_surfaces: Vec<u32>) -> Tyres {
        Tyres { gain, offroad_gain, grus_count_samples: grus_samples, road_surfaces, ..Tyres::default() }
    }

    /// Each wheel's water as the renderer saw it at its last frame (ours, [`Water`]); `None` leaves 1.02's splash
    /// alone on the wetness voice.
    pub fn set_water(&mut self, water: Option<[Water; 4]>) {
        if water.is_none() {
            (self.depth, self.rise, self.gush) = ([0.0; 4], [0.0; 4], [0.0; 4]);
        }
        self.water = water;
        self.fresh = true;
    }

    /// The commands for one step of `dt` seconds: the eight surface voices, the rumble and wetness loops, the gravel
    /// hits, and an off-road rattle when it is due. `splash` is each wheel's `CSigTireParticles` splash of the step,
    /// `inside` the in-car camera, `car_distance` the car's distance from the listener and `listener` where it is (a
    /// gravel hit plays at its wheel, the block's `+0xc`). `pick` gives the random
    /// numbers (the original's `rand()`).
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        blocks: &[[u32; 24]; 4],
        splash: &[Option<f32>; 4],
        speed: f32,
        dt: f32,
        inside: bool,
        (car_distance, listener): (f32, [f32; 3]),
        mut pick: impl FnMut() -> u32,
    ) -> Vec<Command> {
        let mut out = Vec::with_capacity(20);
        let mut all_off = true;
        // The frame clock: the rumble, the wetness decay and the gravel hits run on the game's frames (`Vf0c`), taken
        // as [`FRAME_RATE`] of the drive's time, so the viewer's frame rate does not change them.
        self.frames += dt * FRAME_RATE;
        let frames = self.frames.floor();
        self.frames -= frames;
        let frames = frames as u32;
        for (k, sig) in blocks.iter().enumerate() {
            if let Some(s) = splash[k] {
                self.splash[k] = s;
            }
            // The frames' decay of the wetness voice (`M7ba0`/`M7ea0`), before the block sets it again.
            for _ in 0..frames {
                self.wet[k] = if self.wet[k] * WET_DECAY < QUIET { 0.0 } else { self.wet[k] * WET_DECAY };
            }
            let sound = wheel(sig, &self.road_surfaces);
            all_off &= sound.is_some_and(|s| s.off_road);
            self.contact[k] = sound.is_some();
            if let Some(s) = sound {
                self.tarmac[k] = s.tarmac;
                self.grus_frames[k] = s.grus_frames;
                self.grus_pitch[k] = s.grus_pitch;
                // M5fd0: the splash level, at most 1 (a splash of 0 or less silences it).
                self.wet[k] = if self.splash[k] > 0.0 { self.splash[k].min(1.0) } else { 0.0 };
            }
            // M7ea0 mixes both voices at the first's volume (0x4d80e8, 0x4d81b1): on a surface of one material the
            // second voice doubles the first, the same loop from the same place. Ours: each voice's rolling loop
            // and, sharing it out, its skid loop (slots 2 and 3, only while they sound).
            let wheel = k as u8;
            match sound {
                None => out.extend((0..2).map(|slot| Command::Tyre {
                    wheel,
                    slot,
                    material: Material::Gravel,
                    skid: false,
                    gain: 0.0,
                    pitch: 1.0,
                })),
                Some(s) => {
                    let rolling = if inside { roll_pitch(f32::from_bits(sig[SPEED])) } else { 1.0 };
                    // Ours: the squeal dies on a wet road, as water stops the rubber's stick-slip.
                    let dry = self.water.map_or(1.0, |w| (1.0 - w[k].wet.clamp(0.0, 1.0)).powi(2));
                    for (slot, (material, _)) in s.materials.into_iter().enumerate() {
                        let (roll, skid, pitch) = if material == Material::Tarmac {
                            (s.materials[0].1, s.skid * s.squeal * dry, s.pitch * s.squeal_pitch)
                        } else {
                            (s.materials[0].1 * (1.0 - s.skid), s.skid * s.skid_level, s.pitch)
                        };
                        let (slot, gain) = (slot as u8, roll * self.gain);
                        out.push(Command::Tyre { wheel, slot, material, skid: false, gain, pitch: s.pitch * rolling });
                        if skid > 0.0 {
                            let (slot, gain) = (slot + 2, skid * self.gain);
                            out.push(Command::Tyre { wheel, slot, material, skid: true, gain, pitch });
                        }
                    }
                }
            }
            let [wetness, hiss] = self.water(k, sig, sound.is_some(), frames, listener, &mut out);
            for (voice, (gain, pitch)) in [(WheelLoop::Wetness, wetness), (WheelLoop::Hiss, hiss)] {
                out.push(Command::WheelLoop { wheel: k as u8, voice, gain, pitch });
            }
            out.push(self.rumble(k, sig, inside, frames));
        }
        self.fresh = false;
        for _ in 0..frames {
            if car_distance > GRUS_NEAR {
                break;
            }
            for (k, block) in blocks.iter().enumerate() {
                let n = self.grus_frames[k];
                if !(1..GRUS_FRAMES).contains(&n) || !self.contact[k] {
                    continue;
                }
                self.grus_count[k] += 1;
                let n = n.max(GRUS_MIN_FRAMES);
                if n <= self.grus_count[k] && self.grus_count_samples > 0 {
                    let pitch = self.grus_pitch[k] + (pick() % 10) as f32 * GRUS_PITCH_STEP - GRUS_PITCH_OFFSET;
                    self.grus_count[k] = 0;
                    let shot = Shot::Grus(pick() as usize % self.grus_count_samples);
                    let distance = distance(block, listener);
                    let gain = self.gain * (n.min(GRUS_DENSE) as f32 / GRUS_DENSE as f32).sqrt();
                    out.push(Command::Hit { shot, gain, pitch, distance });
                }
            }
        }
        // Mce10: the speed is CarSensorData's |p+8| * 3.6; the car's speed is taken for it (inferred).
        let kmh = speed.abs() * 3.6;
        if kmh >= OFFROAD_SPEED && all_off {
            self.timer -= dt;
            if self.timer <= 0.0 {
                self.timer = OFFROAD_PERIOD / kmh;
                out.push(Command::Shot {
                    shot: Shot::Offroad(pick() as usize % OFFROAD_SAMPLES),
                    gain: self.offroad_gain,
                });
            }
        } else if !all_off {
            self.timer = 0.0;
        }
        out
    }

    /// The wetness and hiss voices' gains and pitches for wheel `k` of block `sig` (touching the ground or not) over
    /// `frames` game frames: 1.02's splash level on the wetness, or with the renderer's water the louder of it and
    /// ours ([`Water`]); a splash into `out` when a puddle's rise ends.
    #[allow(clippy::too_many_arguments)]
    fn water(
        &mut self,
        k: usize,
        sig: &[u32; 24],
        contact: bool,
        frames: u32,
        listener: [f32; 3],
        out: &mut Vec<Command>,
    ) -> [(f32, f32); 2] {
        let silent = [(self.wet[k], 1.0), (0.0, 1.0)];
        let Some(waters) = self.water else { return silent };
        let water = if contact { waters[k] } else { Water::default() };
        let speed = if contact { f32::from_bits(sig[SPEED]).abs() } else { 0.0 };
        for _ in 0..frames {
            self.gush[k] = if self.gush[k] * WET_DECAY < QUIET { 0.0 } else { self.gush[k] * WET_DECAY };
        }
        if self.fresh {
            let depth = if water.film { 0.0 } else { water.puddle };
            let deeper = (depth - self.depth[k]).max(0.0) * smoothstep(SPLASH_SPEED, speed);
            self.depth[k] = depth;
            self.gush[k] = (self.gush[k] + deeper).min(1.0);
            if deeper > 0.0 {
                self.rise[k] += deeper;
            } else {
                let rise = std::mem::take(&mut self.rise[k]).min(1.0);
                if rise >= SPLASH_MIN && self.splash_samples > 0 {
                    self.splashes += 1;
                    let shot = Shot::Splash(self.splashes % self.splash_samples);
                    let (gain, pitch) = (SPLASH_GAIN * rise, 1.0 - SPLASH_DROP * rise);
                    out.push(Command::Hit { shot, gain, pitch, distance: distance(sig, listener) });
                }
            }
        }
        let (swish, hiss) = water_shares(&water, speed, self.gush[k]);
        if swish <= 0.0 && hiss <= 0.0 {
            return silent;
        }
        let wetness = if swish > 0.0 {
            ((SWISH_GAIN * swish).max(self.wet[k]), water_pitch(&water, speed, true))
        } else {
            silent[0]
        };
        let hiss = if hiss > 0.0 { (HISS_GAIN * hiss, water_pitch(&water, speed, false)) } else { silent[1] };
        [wetness, hiss]
    }

    /// `M6750` for wheel `k`: the rumble plays in the car on a wheel touching a surface whose first material is not
    /// tarmac, and stops otherwise (its volume kept). Word 1 of the block is the frame's largest `FillOutputs` term,
    /// which the wheel's frame finish clears (`rbr_recording::sound::TyreMax`; the standalone writer folds it).
    /// The voice's volume moves once per game frame, `frames` of them in this step.
    fn rumble(&mut self, k: usize, sig: &[u32; 24], inside: bool, frames: u32) -> Command {
        if !self.contact[k] || self.tarmac[k] || !inside {
            return Command::WheelLoop { wheel: k as u8, voice: WheelLoop::Rumble, gain: 0.0, pitch: 1.0 };
        }
        let f = |i: usize| f32::from_bits(sig[i]);
        let x = (f(SPEED) * RUMBLE_SPEED).min(1.0);
        let level = ((f(MAX) + 1.0 + f(FACTOR) * RUMBLE_FACTOR) * x).min(1.0);
        for _ in 0..frames {
            self.rumble[k] = ((level - self.rumble[k]) * RUMBLE_SMOOTH + self.rumble[k]) * RUMBLE_GAIN;
        }
        let pitch = x * RUMBLE_PITCH.0 + RUMBLE_PITCH.1;
        Command::WheelLoop { wheel: k as u8, voice: WheelLoop::Rumble, gain: self.rumble[k], pitch }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic road membership for these model tests. The complete DLL table is checked in rbr-formats.
    fn wheel(sig: &[u32; 24]) -> Option<WheelSound> {
        super::wheel(sig, &[1, 33, 101])
    }

    fn block(surface: u32, grip: f32, factor: f32, slip: f32, speed: f32) -> [u32; 24] {
        let mut sig = [0; 24];
        sig[GRIP_MAX] = grip.to_bits();
        sig[FACTOR] = factor.to_bits();
        sig[SLIP_RATIO] = slip.to_bits();
        sig[SPEED] = speed.to_bits();
        sig[SURFACE] = surface;
        sig[CONTACT] = 1;
        sig
    }

    #[test]
    fn rolling_on_gravel_is_the_level_from_slip_factor_and_speed() {
        // Surface 1: gravel, pitch 73728 / 65536, a road surface.
        let s = wheel(&block(1, 0.8, 2500.0, 4.0, 30.0)).unwrap();
        // slip 4 * 0.025 = 0.1, skid 0, factor 0.5; speed term min(1.5 + 0.4, 1) = 1; level 1200 -> 0.3.
        assert_eq!(s.materials[0].0, Material::Gravel);
        assert!((s.materials[0].1 - 0.3).abs() < 1e-6);
        assert_eq!(s.materials[1].1, 0.0);
        assert_eq!((s.skid, s.skid_level), (0.0, s.materials[0].1));
        assert_eq!(s.pitch, 1.125);
        assert!(!s.off_road);
    }

    #[test]
    fn the_skid_loops_fade_in_up_to_the_peak_at_a_capped_level() {
        let s = wheel(&block(1, 3.0, 9000.0, 80.0, 50.0)).unwrap();
        assert_eq!(s.skid, 1.0);
        // (1 + 1 + 1) * 2000 = 6000, capped at 4000; the rolling level without the skid share (1 + 0 + 1) * 2000.
        assert_eq!((s.skid_level, s.materials[0].1), (1.0, 1.0));
        // Between 1 and 2 the skid share rises by 4/3 per unit.
        let s = wheel(&block(1, 1.375, 0.0, 0.0, 20.0)).unwrap();
        assert!((s.skid_level - 0.5 * 2000.0 * 1.0 / 4000.0).abs() < 1e-3);
        assert_eq!(s.materials[0].1, 0.0);
        // From half way to the peak (grip word 0.875) to the peak (1.25), smoothly; a NaN does not skid.
        let fade = |grip: f32| wheel(&block(1, grip, 0.0, 0.0, 20.0)).unwrap().skid;
        assert_eq!([0.5, 0.875, 1.0625, 1.25, 2.0, f32::NAN].map(fade), [0.0, 0.0, 0.5, 1.0, 1.0, 0.0]);
    }

    /// The first tarmac surface of one material.
    fn tarmac() -> u32 {
        (0..256).find(|&id| surface_row(id)[1] == 1 && surface_row(id)[2] == 1 && surface_row(id)[4] == 0).unwrap()
    }

    /// Wheel 0's tyre commands as (slot, material, skid, gain, pitch).
    fn wheel_tyres(out: &[Command]) -> Vec<(u8, Material, bool, f32, f32)> {
        out.iter()
            .filter_map(|c| match *c {
                Command::Tyre { wheel: 0, slot, material, skid, gain, pitch } => {
                    Some((slot, material, skid, gain, pitch))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_squeal_follows_the_sliding_power_over_the_rolling_loop_and_dies_on_a_wet_road() {
        let pitch = surface_row(tarmac())[3] as f32 * FIXED;
        // 2500 N at 30 m/s and 0.315 m times 20 rad/s of spin slip: 91 kW of 150.
        let s = wheel(&block(tarmac(), 2.0, 2500.0, 20.0, 30.0)).unwrap();
        assert!((s.squeal - 2500.0 * (30.0 + 0.315 * 20.0) / 150_000.0).abs() < 1e-6);
        assert!((s.squeal_pitch - 0.9).abs() < 1e-6);
        // Full at 150 kW; at the peak the pitch is the table's.
        let s = wheel(&block(tarmac(), 1.25, 6000.0, 0.0, 30.0)).unwrap();
        assert_eq!((s.skid, s.squeal, s.squeal_pitch), (1.0, 1.0, 1.0));
        // The tarmac rolling loops play on at their level below the threshold.
        assert_eq!(s.materials, wheel(&block(tarmac(), 0.8, 6000.0, 0.0, 30.0)).unwrap().materials);
        let step = |water: Option<f32>| {
            let mut tyres = Tyres::new(1.0, 1.0, 0, vec![]);
            tyres.set_water(water.map(|wet| [Water { wet, ..Water::default() }; 4]));
            wheel_tyres(&tyres.step(
                &[block(tarmac(), 1.25, 6000.0, 0.0, 30.0); 4],
                &[None; 4],
                30.0,
                0.01,
                false,
                (0.0, [0.0; 3]),
                || 0,
            ))
        };
        let roll = s.materials[0].1;
        let (t, r) = (Material::Tarmac, |slot| (slot, Material::Tarmac, false, roll, pitch));
        assert_eq!(step(None), [r(0), (2, t, true, 1.0, pitch), r(1), (3, t, true, 1.0, pitch)]);
        assert_eq!(step(Some(0.5)), [r(0), (2, t, true, 0.25, pitch), r(1), (3, t, true, 0.25, pitch)]);
        assert_eq!(step(Some(1.0)), [r(0), r(1)]);
    }

    #[test]
    fn loose_rolling_loops_give_way_to_the_skid_loops() {
        let s = wheel(&block(1, 1.0625, 2500.0, 4.0, 30.0)).unwrap();
        let mut tyres = Tyres::new(1.0, 1.0, 0, vec![]);
        let out =
            tyres.step(&[block(1, 1.0625, 2500.0, 4.0, 30.0); 4], &[None; 4], 30.0, 0.01, false, (0.0, [0.0; 3]), || 0);
        let (g, half) = (Material::Gravel, |x: f32| 0.5 * x);
        assert_eq!(
            wheel_tyres(&out),
            [
                (0, g, false, half(s.materials[0].1), 1.125),
                (2, g, true, half(s.skid_level), 1.125),
                (1, g, false, half(s.materials[0].1), 1.125),
                (3, g, true, half(s.skid_level), 1.125),
            ]
        );
    }

    /// The table's rows as the hand copy that preceded `exe_data` held them (spot checks).
    #[test]
    fn surface_rows_are_the_exe_table() {
        assert_eq!(surface_row(0), [0x3f19999a, 0, 0, 65536, 0, 0]);
        assert_eq!(surface_row(2), [0x3f19999a, 0, 0, 73728, 0, 1024]);
        assert_eq!(surface_row(0x100 + 2), surface_row(2));
    }

    #[test]
    fn a_mixed_surface_splits_the_level_and_tarmac_rests_at_half() {
        // Surface 101: tarmac first, gravel second, share 57344 / 65536.
        let s = wheel(&block(101, 1.0, 0.0, 0.0, 20.0)).unwrap();
        assert_eq!((s.materials[0].0, s.materials[1].0), (Material::Snow, Material::Gravel));
        let tarmac = (0..256).find(|&id| surface_row(id)[1] == 1 && surface_row(id)[4] == 0).unwrap();
        let t = wheel(&block(tarmac, 0.8, 0.0, 0.0, 20.0)).unwrap();
        assert_eq!(t.materials[0].0, Material::Tarmac);
        // skid share 0.5 at rest on tarmac: 0.5 * 2000 * 1 / 4000.
        assert!((t.materials[0].1 + t.materials[1].1 - 0.25).abs() < 1e-6);
    }

    #[test]
    fn in_the_car_the_rolling_loops_rise_in_pitch_past_72_kmh_at_the_same_level() {
        assert_eq!([0.0, 10.0, 20.0].map(roll_pitch), [1.0; 3]);
        assert!((roll_pitch(35.0) - 1.1).abs() < 1e-6);
        assert_eq!([50.0, 80.0].map(roll_pitch), [1.2; 2]);
        let tyre = |speed: f32, grip: f32, inside: bool, slot: u8| {
            let mut tyres = Tyres::new(1.0, 1.0, 0, vec![1, 33, 101]);
            let out = tyres.step(
                &[block(1, grip, 2500.0, 4.0, speed); 4],
                &[None; 4],
                speed,
                0.01,
                inside,
                (0.0, [0.0; 3]),
                || 0,
            );
            out.into_iter()
                .find_map(|c| match c {
                    Command::Tyre { wheel: 0, slot: s, gain, pitch, .. } if s == slot => Some((gain, pitch)),
                    _ => None,
                })
                .unwrap()
        };
        let (slow, fast) = (tyre(20.0, 0.8, true, 0), tyre(50.0, 0.8, true, 0));
        assert_eq!((slow.0, slow.1), (fast.0, 1.125));
        assert!((fast.1 - 1.125 * 1.2).abs() < 1e-6);
        // Outside the car and on the skid loops: the table's pitch; the rolling loops rise while skidding.
        assert_eq!(tyre(50.0, 0.8, false, 0).1, 1.125);
        assert_eq!(tyre(50.0, 3.0, true, 2).1, 1.125);
        assert!((tyre(50.0, 1.0, true, 0).1 - 1.125 * 1.2).abs() < 1e-6);
    }

    #[test]
    fn both_surface_voices_play_at_the_first_voices_volume() {
        let mut tyres = Tyres::new(1.0, 1.0, 0, vec![1, 33, 101]);
        let sig = block(101, 0.8, 0.0, 0.0, 20.0);
        let first = wheel(&sig).unwrap().materials[0].1;
        let out = tyres.step(&[sig; 4], &[None; 4], 20.0, 1.0 / 60.0, false, (0.0, [0.0; 3]), || 0);
        let gains: Vec<(u8, Material, f32)> = out
            .iter()
            .filter_map(|c| match *c {
                Command::Tyre { wheel: 0, slot, material, gain, .. } => Some((slot, material, gain)),
                _ => None,
            })
            .collect();
        assert_eq!(gains, [(0, Material::Snow, first), (1, Material::Gravel, first)]);
    }

    #[test]
    fn the_pairs_second_surface_its_road_list_and_skid_threshold() {
        // Surface 1 (gravel) with 101 (snow first) at a share of 0.6 plays 101.
        let mut sig = block(1, 1.1, 0.0, 0.0, 20.0);
        (sig[SECOND_SURFACE], sig[SECOND_SHARE]) = (101, 0.6f32.to_bits());
        let s = wheel(&sig).unwrap();
        assert_eq!(s.materials[0].0, Material::Snow);
        // A grip maximum of 1.1 is past the fade's start.
        assert!(s.skid > 0.0);
        sig[SECOND_SHARE] = 0.5f32.to_bits();
        assert_eq!(wheel(&sig).unwrap().materials[0].0, Material::Gravel);
        // Surface 33 is a road surface, 72 is not.
        assert!(!wheel(&block(33, 1.0, 0.0, 0.0, 20.0)).unwrap().off_road);
        assert!(wheel(&block(72, 1.0, 0.0, 0.0, 20.0)).unwrap().off_road);
    }

    #[test]
    fn no_contact_is_silent() {
        let mut sig = block(1, 3.0, 9000.0, 80.0, 50.0);
        sig[CONTACT] = 0;
        assert_eq!(wheel(&sig), None);
    }

    #[test]
    fn gravel_hits_come_at_the_force_rate_off_tarmac() {
        let mut tyres = Tyres::new(1.0, 1.0, 21, vec![1, 33, 101]);
        // Surface 1 is gravel first: a force sum of 4.99 gives a hit every trunc(50 / 5) = 10 frames.
        let mut sig = block(1, 1.0, 0.0, 0.0, 10.0);
        sig[FORCE_SUM] = 4.99f32.to_bits();
        let hits = |c: &[Command]| c.iter().filter(|c| matches!(c, Command::Hit { shot: Shot::Grus(_), .. })).count();
        let count: usize =
            (0..100).map(|_| hits(&tyres.step(&[sig; 4], &[None; 4], 10.0, 0.01, false, (5.0, [0.0; 3]), || 4))).sum();
        assert_eq!(count, 20); // frames 10 to 50 of the first 59
                               // Beyond 20 m from the listener there are none.
        let far: usize =
            (0..100).map(|_| hits(&tyres.step(&[sig; 4], &[None; 4], 10.0, 0.01, false, (30.0, [0.0; 3]), || 4))).sum();
        assert_eq!(far, 0);
        let pitch = (0..20)
            .flat_map(|_| tyres.step(&[sig; 4], &[None; 4], 10.0, 0.01, false, (5.0, [0.0; 3]), || 4))
            .find_map(|c| match c {
                Command::Hit { pitch, .. } => Some(pitch),
                _ => None,
            });
        // The table's word 0 plus 4 * 0.02 - 0.1.
        assert!(pitch.is_some_and(|p| (p - (f32::from_bits(surface_row(1)[0]) - 0.02)).abs() < 1e-6));
    }

    #[test]
    fn dense_gravel_hits_are_capped_and_quieter() {
        let mut tyres = Tyres::new(1.0, 1.0, 21, vec![1, 33, 101]);
        // A force sum of 49.99 is due every frame: one every 3 frames instead, at sqrt(3 / 6).
        let mut sig = block(1, 1.0, 0.0, 0.0, 10.0);
        sig[FORCE_SUM] = 49.99f32.to_bits();
        let gains: Vec<f32> = (0..100)
            .flat_map(|_| tyres.step(&[sig; 4], &[None; 4], 10.0, 0.01, false, (5.0, [0.0; 3]), || 4))
            .filter_map(|c| match c {
                Command::Hit { shot: Shot::Grus(_), gain, .. } => Some(gain),
                _ => None,
            })
            .collect();
        assert_eq!(gains.len(), 76); // frames 3 to 57 of the first 59, each wheel
        assert!(gains.iter().all(|g| (g - 0.5f32.sqrt()).abs() < 1e-6));
    }

    #[test]
    fn the_rumble_plays_in_the_car_off_tarmac_and_the_wetness_follows_the_splash() {
        let mut tyres = Tyres::new(1.0, 1.0, 0, vec![1, 33, 101]);
        let sig = block(1, 1.0, 0.0, 0.0, 20.0);
        let loops = |c: &[Command], want: WheelLoop| -> Vec<(f32, f32)> {
            c.iter()
                .filter_map(|c| match *c {
                    Command::WheelLoop { voice, gain, pitch, .. } if voice == want => Some((gain, pitch)),
                    _ => None,
                })
                .collect()
        };
        let out = tyres.step(&[sig; 4], &[Some(0.4); 4], 20.0, 1.0 / 60.0, true, (0.0, [0.0; 3]), || 0);
        // One game frame a step. Level 1 at full speed: the volume moves a quarter there and is set at half: 0.125;
        // pitch 1.2.
        assert_eq!(loops(&out, WheelLoop::Rumble)[0], (0.125, 1.2));
        assert_eq!(loops(&out, WheelLoop::Wetness)[0].0, 0.4);
        // Settles at a fifth of the level (the set volume is the next frame's start).
        for _ in 0..100 {
            tyres.step(&[sig; 4], &[None; 4], 20.0, 1.0 / 60.0, true, (0.0, [0.0; 3]), || 0);
        }
        let out = tyres.step(&[sig; 4], &[None; 4], 20.0, 1.0 / 60.0, false, (0.0, [0.0; 3]), || 0);
        assert!((tyres.rumble[0] - 0.2).abs() < 1e-4);
        assert_eq!(loops(&out, WheelLoop::Rumble)[0].0, 0.0);
    }

    #[test]
    fn word_1_raises_the_rumble_below_full_speed() {
        let rumble = |max: f32| {
            let mut tyres = Tyres::new(1.0, 1.0, 0, vec![1, 33, 101]);
            let mut sig = block(1, 1.0, 0.0, 0.0, 2.0);
            sig[MAX] = max.to_bits();
            let out = tyres.step(&[sig; 4], &[None; 4], 2.0, 1.0 / 60.0, true, (0.0, [0.0; 3]), || 0);
            out.iter()
                .find_map(|c| match *c {
                    Command::WheelLoop { voice: WheelLoop::Rumble, gain, .. } => Some(gain),
                    _ => None,
                })
                .unwrap()
        };
        // x = 0.2: level (1 + word 1) * 0.2, a quarter of it set at half.
        assert!((rumble(0.0) - 0.025).abs() < 1e-6);
        assert!((rumble(1.5) - 0.0625).abs() < 1e-6);
    }

    #[test]
    fn off_road_rattles_follow_the_speed() {
        let mut tyres = Tyres::new(1.0, 1.0, 0, vec![1, 33, 101]);
        let grass = [block(0, 1.0, 0.0, 0.0, 10.0); 4];
        let shots = |c: &[Command]| c.iter().filter(|c| matches!(c, Command::Shot { .. })).count();
        // 36 km/h: one rattle at once, then one each 10 / 36 s.
        let mut count = 0;
        for _ in 0..100 {
            count += shots(&tyres.step(&grass, &[None; 4], 10.0, 0.01, false, (5.0, [0.0; 3]), || 3));
        }
        assert_eq!(count, 4);
        let road = [block(1, 1.0, 0.0, 0.0, 10.0); 4];
        assert_eq!(shots(&tyres.step(&road, &[None; 4], 10.0, 0.01, false, (5.0, [0.0; 3]), || 3)), 0);
        assert_eq!(shots(&tyres.step(&grass, &[None; 4], 1.0, 0.01, false, (5.0, [0.0; 3]), || 3)), 0);
    }

    /// One step of `tyres` on gravel (surface 1) at wheel speed `speed` in `water`, a new renderer frame: wheel 0's
    /// wetness (the swish) and hiss voices' gains and pitches, and the splashes.
    type WetStep = ([(f32, f32); 2], Vec<(f32, f32)>);

    fn wet_step(tyres: &mut Tyres, water: Water, speed: f32) -> WetStep {
        tyres.set_water(Some([water; 4]));
        let block = [block(1, 1.0, 0.0, 0.0, speed); 4];
        let out = tyres.step(&block, &[None; 4], speed, 1.0 / 60.0, false, (0.0, [0.0; 3]), || 0);
        let voice = |want: WheelLoop| {
            out.iter()
                .find_map(|c| match *c {
                    Command::WheelLoop { wheel: 0, voice, gain, pitch } if voice == want => Some((gain, pitch)),
                    _ => None,
                })
                .unwrap()
        };
        let splashes = out
            .iter()
            .filter_map(|c| match *c {
                Command::Hit { shot: Shot::Splash(_), gain, pitch, .. } => Some((gain, pitch)),
                _ => None,
            })
            .collect();
        ([voice(WheelLoop::Wetness), voice(WheelLoop::Hiss)], splashes)
    }

    fn wet_tyres() -> Tyres {
        let mut tyres = Tyres::new(1.0, 1.0, 0, vec![1, 33, 101]);
        tyres.splash_samples = 3;
        tyres
    }

    #[test]
    fn running_into_a_puddle_splashes_once_the_rise_ends_and_the_gush_ducks_the_swish_and_hiss() {
        let mut tyres = wet_tyres();
        let dry = Water::default();
        let puddle = Water { puddle: 1.0, wet: 1.0, loose: true, film: false };
        assert_eq!(wet_step(&mut tyres, dry, 20.0), ([(0.0, 1.0), (0.0, 1.0)], vec![]));
        let ([hit, hit_hiss], splashes) = wet_step(&mut tyres, puddle, 20.0);
        assert!(splashes.is_empty(), "the splash waits for the rise to end");
        // The gush is full: no swish, no hiss under the splash.
        assert_eq!((hit.0, hit_hiss.0), (0.0, 0.0));
        let (_, splashes) = wet_step(&mut tyres, puddle, 20.0);
        // Four wheels ran in at once: four splashes, at the deep puddle's pitch.
        assert_eq!(splashes, vec![(SPLASH_GAIN, 1.0 - SPLASH_DROP); 4]);
        // In it, once the gush has died away, the swish alone; out of it the hiss alone.
        let ([held, held_hiss], _) = (0..200).map(|_| wet_step(&mut tyres, puddle, 20.0)).last().unwrap();
        assert_eq!((held.0, held_hiss.0), (SWISH_GAIN, 0.0));
        let wet = Water { puddle: 0.0, ..puddle };
        let ([after, after_hiss], splashes) = wet_step(&mut tyres, wet, 20.0);
        assert!(splashes.is_empty() && after.0 == 0.0 && after_hiss.0 > 0.0, "{after:?} {after_hiss:?}");
    }

    #[test]
    fn onto_wet_tarmacs_film_from_gravel_no_splash_or_gush_only_the_swish() {
        let mut tyres = wet_tyres();
        let gravel = Water { puddle: 0.0, wet: 1.0, loose: true, film: false };
        let film = Water { puddle: 0.84, wet: 1.0, loose: false, film: true };
        wet_step(&mut tyres, gravel, 20.0);
        for _ in 0..3 {
            let ([swish, hiss], splashes) = wet_step(&mut tyres, film, 20.0);
            assert!(splashes.is_empty(), "no splash");
            // No gush: the swish is at once its full share, the hiss under it.
            let (share, rest) = water_shares(&film, 20.0, 0.0);
            assert_eq!((swish.0, hiss.0), (SWISH_GAIN * share, HISS_GAIN * rest));
        }
        assert_eq!(tyres.gush[0], 0.0);
        // From the film into a puddle is a puddle's rise from nothing.
        let puddle = Water { puddle: 1.0, film: false, ..gravel };
        wet_step(&mut tyres, puddle, 20.0);
        assert_eq!(wet_step(&mut tyres, puddle, 20.0).1.len(), 4);
    }

    #[test]
    fn the_shares_never_stack_past_one() {
        let water = Water { puddle: 0.5, wet: 1.0, loose: false, film: false };
        for gush in [0.0, 0.3, 0.6, 1.0] {
            let (swish, hiss) = water_shares(&water, 30.0, gush);
            assert!(gush + swish + hiss <= 1.0 + 1e-6, "{gush} {swish} {hiss}");
        }
    }

    #[test]
    fn a_shallow_or_slow_puddle_does_not_splash() {
        // A rut's film is no puddle (the spray's depth is 0 under 0.15 standing water): the hiss alone.
        let mut tyres = wet_tyres();
        let rut = Water { puddle: 0.0, wet: 1.0, loose: true, film: false };
        for _ in 0..5 {
            let ([swish, hiss], splashes) = wet_step(&mut tyres, rut, 20.0);
            assert!(splashes.is_empty() && swish.0 == 0.0 && hiss.0 > 0.0);
        }
        let shallow = Water { puddle: 0.12, ..rut };
        let mut tyres = wet_tyres();
        assert!((0..3).all(|_| wet_step(&mut tyres, shallow, 20.0).1.is_empty()), "a rise under 0.15");
        let mut tyres = wet_tyres();
        let deep = Water { puddle: 1.0, ..rut };
        assert!((0..3).all(|_| wet_step(&mut tyres, deep, 2.0).1.is_empty()), "at walking pace");
    }

    #[test]
    fn the_wet_road_hiss_rises_with_speed_darker_off_tarmac_and_is_silent_dry() {
        let hiss = |water: Water, speed| wet_step(&mut wet_tyres(), water, speed).0[1];
        let tarmac = Water { puddle: 0.0, wet: 1.0, loose: false, film: false };
        let gains: Vec<f32> = [2.0, 8.0, 15.0, 20.0, 25.0].iter().map(|&v| hiss(tarmac, v).0).collect();
        assert_eq!(gains[0], 0.0);
        assert!(gains.windows(2).all(|w| w[1] > w[0]), "{gains:?}");
        assert_eq!(gains[4], HISS_GAIN);
        let gravel = Water { loose: true, ..tarmac };
        let (t, g) = (hiss(tarmac, 25.0), hiss(gravel, 25.0));
        assert!(g.0 < t.0 && g.1 < t.1, "{t:?} {g:?}");
        assert!(hiss(tarmac, 35.0).1 > t.1, "brighter with speed");
        assert!(hiss(Water { wet: 0.5, ..tarmac }, 25.0).0 < t.0 * 0.5, "a damp road hisses less");
        assert_eq!(hiss(Water::default(), 35.0), (0.0, 1.0), "dry");
    }

    #[test]
    fn without_the_renderers_water_the_wetness_follows_the_splash_alone() {
        let run = |water: Option<[Water; 4]>| {
            let mut tyres = wet_tyres();
            tyres.set_water(water);
            tyres.step(
                &[block(1, 1.0, 0.0, 0.0, 20.0); 4],
                &[Some(0.3); 4],
                20.0,
                1.0 / 60.0,
                true,
                (0.0, [0.0; 3]),
                || 0,
            )
        };
        assert_eq!(run(None), run(Some([Water::default(); 4])));
    }
}
