//! The car's smaller effects in `CCarSound` (rbr-recomp `notes/sound.md` 5.5), read in the disassembly. Pure.
//!
//! - The ignition: the `DrivetrainState` handler (0x4ae510, 0x4ae73b-0x4ae7a6) keeps the starter word `p[9]`
//!   (`CDrive +0x1c8`) while the engine is stopped (`+0x41924`, which `M6a0` sets below the low rpm and clears above
//!   it), and when it rises from 0 calls `Vf54` (0x4ad090): `CAR::IGNITION Ignition1` at the car, at the `Ignition`
//!   amp, min distance 10 m, unless the last one still plays.
//! - The dog rings: the handler keeps `f = p[4] / 2 pi * 0.5` (`p[4]` is `|CDrive::M5A10|` while a gear engages in
//!   `CDrive::M8570` 0x508570, else 0) and `p[5]` (the engagement's level, `dri +0x560`). Each frame with `f > 0`
//!   (`CCarSound::Vf08` 0x4b1085-0x4b111d) a clock gains the frame's time and, once it reaches `f / 2 pi`, loses `1 /
//!   f` and plays a random `CAR::DOGRINGS DR1-8` (`E31Transmission::M3510` 0x4d3510) in the car, 2D, at `p[5] * 0.7`.
//!   The clock is never reset but by the car's reset.
//! - The broken cooler: `M6a0` keeps `min(p[5], 1) * 0.25` of the `EngineNoise` payload (`+0x41abc`), and `Md2b0`
//!   (0x4ad2b0) loops `Audio\Cars\Effects\cooler.wav` at the car at the `BrokenCooler` amp times it while it is at
//!   least 0.001 (min distance 5 m), and stops it below. What sets the engine state's `p[5]` is not traced.
//! - The wipers: `Md490` (0x4ad490), in the car, with the glass on (`0x167e834`, the in-car camera modes) and
//!   precipitation, plays `CAR::WIPER WiperEngine` (`WiperEngine` amp) and `WiperUp` or `WiperDown` (`Wiper` amp),
//!   2D, when `E88Wiper`'s direction (`0x167e830`) differs from `CCarSound +0x41928`. 1.02 never writes `+0x41928`
//!   after setting it to 1, so it starts both again every frame of each return stroke. Ours: both play once at each
//!   change of direction, `WiperUp` when the blades turn back (the direction 0, which 1.02 plays) and `WiperDown`
//!   when they set out again. The direction changes every 0.4 s while the precipitation is above 0
//!   (`E88Wiper::Tick` 0x5f8720, `rbr_render::glass::model::Wiping`).
//!
//! Ours: the frame clock of the dog rings runs on the physics step, and the effects volume option is 1.
//!
//! Ours (realism): with the renderer's glass model ([`Glass`], `rbr_render::glass::model`) the wipers play as its
//! blades move instead of on 1.02's timer and samples, in the car only: the motor loop while they are off the park,
//! a stroke at each stroke the glass begins (its `stroke_count`), as recorded on wet glass by the water under the
//! blades and on dry glass for the rest ([`stroke_gains`]), and water slapping the windscreen at each puddle splat
//! the glass lands, louder for a bigger one. The recordings are embedded (`data::GlassBank`).

use rbr_recording::sound::Sound;

use crate::voice::{Command, Loop, Repeat, Shot};

/// 1 / 2 pi (0x731b1c) and the half (0x795e18) of the dog rings' rate; their volume factor (0x72f674).
const INV_TAU: f32 = 0.159_154_94;
const DOG_RATE: f32 = 0.5;
const DOG_GAIN: f32 = 0.7;
/// The cooler's factor (0x730888) and its threshold (0x795e94).
const COOLER_SCALE: f32 = 0.25;
const COOLER_QUIET: f32 = 0.001;
/// Seconds per wiper stroke (`E88Wiper`: 1.3823 rad at 3.4558 rad/s).
pub const WIPER_STROKE: f32 = 0.4;

/// The wiper motor's gain over the `WiperEngine` amp (ours; the recording is at -24 dBFS RMS).
const MOTOR_GAIN: f32 = 0.5;
/// A stroke's gain over the `Wiper` amp (ours; the recordings' loudest 50 ms at -15 dBFS).
const STROKE_GAIN: f32 = 1.6;
/// The water under the blades (the glass film's mean over their sweep plus the share the drops cover) at which a
/// stroke sounds fully wet; below it the dry stroke takes over.
const WET_FULL: f32 = 0.08;
/// A splat's gain (ours, the effects' level; the recordings' loudest 0.1 s at -16 dBFS) for the smallest and the
/// biggest splat ([`Glass::splats`] 0 to 1).
const SPLAT_GAIN: [f32; 2] = [0.25, 0.5];
/// Seconds between splats landed in one frame (ours: the glass lands a frame's at once), and the most kept waiting.
const SPLAT_SPACING: f32 = 0.03;
const SPLATS_WAITING: usize = 6;

/// The windscreen as the renderer's glass model shows it after a frame (ours): the blades and the water under them,
/// and the puddle splats it landed since the last frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Glass {
    /// The blades are off the park.
    pub moving: bool,
    /// The strokes begun so far, and whether the last sets out from the park (else it turns back).
    pub strokes: u32,
    pub out: bool,
    /// The water under the blades' sweep: the film's mean plus the share the drops cover (0 dry).
    pub wet: f32,
    /// Each splat's size, 0 to 1.
    pub splats: Vec<f32>,
}

/// A stroke's wet and dry gains (before the `Wiper` amp) with `wet` water under the blades ([`WET_FULL`]).
pub fn stroke_gains(wet: f32) -> [f32; 2] {
    let w = (wet / WET_FULL).clamp(0.0, 1.0);
    [w, 1.0 - w].map(|g| g.sqrt() * STROKE_GAIN)
}

/// The effects' amps (`audio.ini`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectGains {
    pub ignition: f32,
    pub cooler: f32,
    pub wiper_engine: f32,
    pub wiper: f32,
}

impl Default for EffectGains {
    /// The retail values.
    fn default() -> Self {
        EffectGains { ignition: 1.0, cooler: 1.0, wiper_engine: 0.25, wiper: 0.25 }
    }
}

/// What the effects keep between steps.
#[derive(Debug, Clone, Default)]
pub struct Effects {
    pub gains: EffectGains,
    /// The banks' sizes: the dog rings.
    dog_rings: usize,
    /// The starter word while stopped (`+0x41858`), the dog rings' clock (`+0x41b14`).
    starter: f32,
    dog_clock: f32,
    /// The wipers' time running and the direction last played.
    wiper_time: f32,
    wiper_played: Option<bool>,
    /// The glass model's windscreen (ours; `None`: 1.02's timer; its splats moved to `splats`), the strokes last
    /// heard, the splats not yet played and the next splat recording (the mixer takes it modulo its bank).
    glass: Option<Glass>,
    strokes: Option<u32>,
    splats: Vec<f32>,
    next_splat: usize,
    splat_wait: f32,
    /// The motor loop runs.
    motor: bool,
}

impl Effects {
    pub fn new(gains: EffectGains, dog_rings: usize) -> Effects {
        Effects { gains, dog_rings, ..Effects::default() }
    }

    /// The windscreen as the glass model shows it after a frame (ours); `None` plays 1.02's wipers. Its splats wait
    /// for the next step.
    pub fn set_glass(&mut self, mut glass: Option<Glass>) {
        if let Some(g) = &mut glass {
            self.splats.append(&mut g.splats);
            let over = self.splats.len().saturating_sub(SPLATS_WAITING);
            self.splats.drain(..over);
        } else {
            self.strokes = None;
        }
        self.glass = glass;
    }

    /// The commands for one step of `dt` seconds: the engine is stopped or not, the camera in the car or not, under
    /// `precipitation` (`M9b80`, rain or snow).
    pub fn step(
        &mut self,
        sound: &Sound,
        stopped: bool,
        inside: bool,
        precipitation: f32,
        dt: f32,
        mut rand: impl FnMut() -> u32,
    ) -> Vec<Command> {
        let mut out = Vec::new();
        let d = |i: usize| sound.drivetrain_f32(i);
        if stopped {
            let rose = self.starter == 0.0 && d(9) != 0.0;
            self.starter = d(9);
            if rose {
                out.push(Command::Effect {
                    shot: Shot::Ignition,
                    gain: self.gains.ignition,
                    pitch: 1.0,
                    repeat: Repeat::IfIdle,
                });
            }
        }

        let f = d(4) * INV_TAU * DOG_RATE;
        if f > 0.0 {
            self.dog_clock += dt;
            if self.dog_clock >= f * INV_TAU {
                self.dog_clock -= 1.0 / f;
                let pick = rand() as usize;
                if inside && self.dog_rings > 0 {
                    out.push(Command::Shot { shot: Shot::DogRing(pick % self.dog_rings), gain: d(5) * DOG_GAIN });
                }
            }
        }

        let cooler = sound.engine_f32(5).min(1.0) * COOLER_SCALE;
        let cooler = if cooler >= COOLER_QUIET { cooler * self.gains.cooler } else { 0.0 };
        out.push(Command::Loop { voice: Loop::Cooler, gain: cooler, pitch: 1.0 });

        // Its splats are taken: the clone allocates nothing.
        if let Some(glass) = self.glass.clone() {
            self.glass_step(&glass, inside, dt, &mut out);
            return out;
        }
        if precipitation > 0.0 {
            self.wiper_time += dt;
        }
        // `E88Wiper +0x30` starts at 1 and flips at each stop.
        let direction = ((self.wiper_time / WIPER_STROKE) as u32).is_multiple_of(2);
        if inside && precipitation > 0.0 && self.wiper_played != Some(direction) {
            self.wiper_played = Some(direction);
            let stroke = if direction { Shot::WiperDown } else { Shot::WiperUp };
            out.push(Command::Shot { shot: Shot::WiperEngine, gain: self.gains.wiper_engine });
            out.push(Command::Shot { shot: stroke, gain: self.gains.wiper });
        }
        out
    }

    /// The wipers and splats as the glass shows them (ours).
    fn glass_step(&mut self, glass: &Glass, inside: bool, dt: f32, out: &mut Vec<Command>) {
        let motor = inside && glass.moving;
        if motor || self.motor {
            let gain = if motor { self.gains.wiper_engine * MOTOR_GAIN } else { 0.0 };
            out.push(Command::Loop { voice: Loop::WiperMotor, gain, pitch: 1.0 });
        }
        self.motor = motor;
        // One stroke a step at most: a jump of several (a replay seek) plays once; fewer (a new glass) plays none.
        let new_stroke = self.strokes.is_some_and(|last| glass.strokes > last);
        self.strokes = Some(glass.strokes);
        if inside && new_stroke {
            let k = usize::from(glass.out);
            let [wet, dry] = stroke_gains(glass.wet);
            for (shot, gain) in [(Shot::WiperWet(k), wet), (Shot::WiperDry(k), dry)] {
                if gain > 0.0 {
                    out.push(Command::Shot { shot, gain: gain * self.gains.wiper });
                }
            }
        }
        // A splat at a time, [`SPLAT_SPACING`] apart; outside the car they are dropped.
        self.splat_wait -= dt;
        if !inside {
            self.splats.clear();
        } else if self.splat_wait <= 0.0 && !self.splats.is_empty() {
            let size = self.splats.remove(0);
            let gain = SPLAT_GAIN[0] + (SPLAT_GAIN[1] - SPLAT_GAIN[0]) * size.clamp(0.0, 1.0);
            out.push(Command::Shot { shot: Shot::GlassSplat(self.next_splat), gain });
            self.next_splat = self.next_splat.wrapping_add(1);
            self.splat_wait = SPLAT_SPACING;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sound(drivetrain: &[(usize, f32)], engine5: f32) -> Sound {
        let mut s = Sound::default();
        for &(i, v) in drivetrain {
            s.drivetrain[i] = v.to_bits();
        }
        s.engine[5] = engine5.to_bits();
        s
    }

    fn shots(commands: &[Command]) -> Vec<Shot> {
        commands
            .iter()
            .filter_map(|c| match *c {
                Command::Shot { shot, .. } | Command::Effect { shot, .. } => Some(shot),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_starter_cranks_the_ignition_once_while_stopped() {
        let mut fx = Effects::new(EffectGains::default(), 8);
        let crank = sound(&[(9, 0.5)], 0.0);
        assert!(shots(&fx.step(&crank, false, true, 0.0, 0.01, || 0)).is_empty(), "running");
        let mut fx = Effects::new(EffectGains::default(), 8);
        assert_eq!(shots(&fx.step(&sound(&[], 0.0), true, true, 0.0, 0.01, || 0)), []);
        assert_eq!(shots(&fx.step(&crank, true, true, 0.0, 0.01, || 0)), [Shot::Ignition]);
        assert_eq!(shots(&fx.step(&crank, true, true, 0.0, 0.01, || 0)), []);
    }

    #[test]
    fn the_dog_rings_rattle_at_the_engagement_rate_in_the_car() {
        // p[4] = 4 pi * 10: f = 10, the first after 10 / 2 pi s, then every 0.1 s.
        let engaging = sound(&[(4, 40.0 * std::f32::consts::PI), (5, 0.5)], 0.0);
        let mut fx = Effects::new(EffectGains::default(), 8);
        let steps: Vec<usize> =
            (0..200).filter(|_| !shots(&fx.step(&engaging, false, true, 0.0, 0.01, || 3)).is_empty()).collect();
        assert_eq!(steps.first(), Some(&159));
        assert!(steps.windows(2).all(|w| w[1] - w[0] == 10), "{steps:?}");
        let mut fx = Effects::new(EffectGains::default(), 8);
        assert!((0..200).all(|_| shots(&fx.step(&engaging, false, false, 0.0, 0.01, || 3)).is_empty()));
    }

    #[test]
    fn the_cooler_loops_above_its_threshold() {
        let mut fx = Effects::new(EffectGains::default(), 8);
        let gain = |c: &[Command]| {
            c.iter().find_map(|c| match *c {
                Command::Loop { voice: Loop::Cooler, gain, .. } => Some(gain),
                _ => None,
            })
        };
        assert_eq!(gain(&fx.step(&sound(&[], 2.0), false, false, 0.0, 0.01, || 0)), Some(0.25));
        assert_eq!(gain(&fx.step(&sound(&[], 0.002), false, false, 0.0, 0.01, || 0)), Some(0.0));
    }

    #[test]
    fn the_wipers_sound_at_each_turn_in_the_car_in_the_wet() {
        let mut fx = Effects::new(EffectGains::default(), 8);
        let dry = sound(&[], 0.0);
        let all: Vec<(usize, Vec<Shot>)> = (0..100)
            .map(|k| (k, shots(&fx.step(&dry, false, true, 50.0, 0.01, || 0))))
            .filter(|(_, s)| !s.is_empty())
            .collect();
        let at: Vec<usize> = all.iter().map(|(k, _)| *k).collect();
        assert_eq!(at, [0, 40, 80]);
        assert_eq!(all[0].1, [Shot::WiperEngine, Shot::WiperDown]);
        assert_eq!(all[1].1, [Shot::WiperEngine, Shot::WiperUp]);
        let mut fx = Effects::new(EffectGains::default(), 8);
        assert!((0..100).all(|_| shots(&fx.step(&dry, false, true, 0.0, 0.01, || 0)).is_empty()));
    }

    fn motor(commands: &[Command]) -> Option<f32> {
        commands.iter().find_map(|c| match *c {
            Command::Loop { voice: Loop::WiperMotor, gain, .. } => Some(gain),
            _ => None,
        })
    }

    /// The glass's blades over `steps` steps of 0.01 s: parked, then out and back at 0.4 s a stroke.
    fn blades(k: usize, wet: f32) -> Glass {
        let t = k as f32 * 0.01;
        let strokes = (t / WIPER_STROKE).ceil() as u32;
        Glass { moving: t > 0.0 && t < 1.6, strokes: strokes.min(4), out: strokes % 2 == 1, wet, splats: Vec::new() }
    }

    #[test]
    fn the_wipers_follow_the_glass_blades() {
        let dry = sound(&[], 0.0);
        let mut fx = Effects::new(EffectGains::default(), 8);
        // Parked: nothing, rain or not.
        fx.set_glass(Some(Glass::default()));
        for _ in 0..50 {
            let out = fx.step(&dry, false, true, 100.0, 0.01, || 0);
            assert!(shots(&out).is_empty() && motor(&out).is_none(), "{out:?}");
        }
        // Moving on wet glass: the motor every step and one wet stroke a stroke, by direction.
        let mut fx = Effects::new(EffectGains::default(), 8);
        let mut played = Vec::new();
        for k in 0..200 {
            fx.set_glass(Some(blades(k, 0.5)));
            let out = fx.step(&dry, false, true, 100.0, 0.01, || 0);
            let moving = blades(k, 0.5).moving;
            assert_eq!(motor(&out).is_some_and(|g| g > 0.0), moving, "step {k}");
            played.extend(shots(&out).into_iter().map(|s| (k, s)));
        }
        assert_eq!(
            played,
            [(1, Shot::WiperWet(1)), (41, Shot::WiperWet(0)), (81, Shot::WiperWet(1)), (121, Shot::WiperWet(0))]
        );
        // Parked again: the motor stops once, then nothing.
        let mut fx = Effects::new(EffectGains::default(), 8);
        let gains: Vec<Option<f32>> = (0..200)
            .map(|k| {
                fx.set_glass(Some(blades(k, 0.5)));
                motor(&fx.step(&dry, false, true, 100.0, 0.01, || 0))
            })
            .collect();
        let stop = gains.iter().position(|g| *g == Some(0.0)).unwrap();
        assert!((160..=161).contains(&stop) && gains[stop - 1].is_some_and(|g| g > 0.0), "{stop}");
        assert!(gains[stop + 1..].iter().all(Option::is_none));
    }

    #[test]
    fn dry_glass_squeaks_and_the_wipers_stay_quiet_outside() {
        let dry = sound(&[], 0.0);
        let strokes = |wet: f32, inside: bool| {
            let mut fx = Effects::new(EffectGains::default(), 8);
            (0..100)
                .flat_map(|k| {
                    fx.set_glass(Some(blades(k, wet)));
                    shots(&fx.step(&dry, false, inside, 0.0, 0.01, || 0))
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(strokes(0.0, true), [Shot::WiperDry(1), Shot::WiperDry(0), Shot::WiperDry(1)]);
        assert_eq!(strokes(0.02, true).len(), 6, "damp: both");
        assert!(strokes(0.0, false).is_empty());
        let [wet, dry] = stroke_gains(0.02);
        assert!(wet > 0.0 && dry > wet);
        assert_eq!(stroke_gains(1.0)[1], 0.0);
    }

    #[test]
    fn each_splat_slaps_once_in_the_car() {
        let dry = sound(&[], 0.0);
        let mut fx = Effects::new(EffectGains::default(), 8);
        let gain = |c: &[Command]| -> Vec<(Shot, f32)> {
            c.iter()
                .filter_map(|c| match *c {
                    Command::Shot { shot: shot @ Shot::GlassSplat(_), gain } => Some((shot, gain)),
                    _ => None,
                })
                .collect()
        };
        // Two in a frame: one each, 30 ms apart; then none until the next frame's.
        fx.set_glass(Some(Glass { splats: vec![0.0, 1.0], ..Glass::default() }));
        let played: Vec<(usize, Vec<(Shot, f32)>)> = (0..10)
            .map(|k| (k, gain(&fx.step(&dry, false, true, 0.0, 0.01, || 0))))
            .filter(|(_, s)| !s.is_empty())
            .collect();
        assert_eq!(played, [(0, vec![(Shot::GlassSplat(0), 0.25)]), (3, vec![(Shot::GlassSplat(1), 0.5)])]);
        fx.set_glass(Some(Glass { splats: vec![0.5], ..Glass::default() }));
        assert_eq!(gain(&fx.step(&dry, false, true, 0.0, 0.01, || 0)), [(Shot::GlassSplat(2), 0.375)]);
        // Outside the car they are dropped.
        fx.set_glass(Some(Glass { splats: vec![0.5], ..Glass::default() }));
        assert!(gain(&fx.step(&dry, false, false, 0.0, 0.01, || 0)).is_empty());
        assert!(gain(&fx.step(&dry, false, true, 0.0, 0.01, || 0)).is_empty());
    }
}
