//! The impact sounds from the damage handler's `CSigCarDamage` payload (`rbr_recording::sound`, `hit`), as
//! `CCollisionSnd::OnDamage` (0x4b27c0) and `Vf64` (0x4b2ed0) play them. Pure. rbr-recomp `notes/sound.md` section 7.
//!
//! The handler keeps its last payload until the next impact overwrites it, so a new impact is a payload that differs
//! from the last step's (ours: the original runs inside the emit). Words 0 and 2 (the functor's result and tag) are
//! left out: `E36DamageOps::Slot1` rewrites them on calls that emit nothing. Only the ranged set sounds in retail
//! (`SOUNDDEF::SHATTER` and `GLASS` are muted), and the scream in the car.
//!
//! Ours: a contact that lasts while the car moves is a scrape, a loop (`data::CarBank::scrape`) in place of 1.02's
//! impact for every step of it ([`Impacts::step`]).

use crate::voice::{Command, Loop, Shot};

/// The payload's state word and impact power.
const STATE: usize = 1;
const POWER: usize = 4;
/// The impact sounds from a power of 100 (0x72bf10); the ranged set takes the power plus 500 (0x731cd4) and plays
/// from 200 (0x730890), its index rising by 0.005 per unit above 200. Above 8000 (0x731e08) in the car, the speech
/// voice stops and the scream plays.
const MIN_POWER: f32 = 100.0;
const RANGED_ADD: f32 = 500.0;
const RANGED_FROM: f32 = 200.0;
const RANGED_STEP: f32 = 0.005;
const SCREAM_POWER: f32 = 8000.0;
/// Ours: after a ranged impact the next plays only after this many steps (0.15 s) and once the contact has
/// ended, or sooner when its power is more than [`HARDER`] times the last one's. Scraping along a bank changes the
/// payload every step, and 1.02 plays each, up to 140 impacts a second (the Lancer drive), a rattle the mixer's cap
/// of 3 ranged impacts only thins; the scrape loop carries the contact instead.
const GAP_STEPS: u32 = 21;
pub(crate) const HARDER: f32 = 1.5;
/// Ours: a ranged impact's level is its power's share of this (amplitude by the hit's strength); 1.02 plays a
/// touch of 100 as loud as a crash. The sample still follows the power.
const FULL_POWER: f32 = 1000.0;
/// Ours, the scrape: a contact lasts this many steps past its last new payload (a live frame of several periods
/// repeats its record) and scrapes from its second one; the loop's level is [`SCRAPE_GAIN`] times the power's
/// share of [`FULL_POWER`] times the car's speed over [`SCRAPE_SPEED`] (m/s, silent to full), its pitch
/// [`SCRAPE_PITCH`] over the same, rising in about 30 ms and falling in 150 (a share of the way each step). At
/// gain 1 the loop (-14 dBFS RMS) sat under the other loops in the car.
const HOLD_STEPS: u32 = 4;
const SCRAPE_GAIN: f32 = 2.0;
const SCRAPE_SPEED: [f32; 2] = [1.0, 15.0];
const SCRAPE_PITCH: [f32; 2] = [0.8, 1.2];
const SCRAPE_RISE: f32 = 0.25;
const SCRAPE_FALL: f32 = 0.05;
/// Ours, the slide: with no wheel on the ground the body itself is sliding (on its roof or side). The contact then
/// holds [`SLIDE_HOLD`] steps (the payload pauses for up to 0.2 s in a slide; the scrape fell by half in each), the
/// scrape is [`SLIDE_GAIN`] times as loud and [`SLIDE_PITCH`] times its pitch (more of the body on the ground), and
/// on loose ground the ground's own loop (`Loop::Slide`) plays at [`SLIDE_GROUND`] by the speed, pitched
/// [`SLIDE_GROUND_PITCH`]. Measured on a roof slide from 110 to 70 km/h (Fiat 131, RSF 1075, in the car): the mix
/// stayed at its driving level, the engine leading.
const SLIDE_HOLD: u32 = 30;
const SLIDE_GAIN: f32 = 2.0;
const SLIDE_PITCH: f32 = 0.7;
const SLIDE_GROUND: f32 = 2.0;
const SLIDE_GROUND_PITCH: [f32; 2] = [0.7, 1.0];
/// Below this the falling loop is silent.
const SCRAPE_FLOOR: f32 = 0.001;

/// The impact state between steps.
#[derive(Debug, Clone, Default)]
pub struct Impacts {
    /// `SOUNDDEF::RANGED` volume times the `Impacts` amp.
    pub ranged_gain: f32,
    /// The `ImpactScreams` amp.
    pub scream_gain: f32,
    pub ranged_count: usize,
    pub scream_count: usize,
    last: Option<[u32; 18]>,
    /// Steps until the next ranged impact may play, and the last one's power ([`GAP_STEPS`]).
    wait: u32,
    last_power: f32,
    /// Steps left of the contact, its new payloads so far, and the scrape loop's level ([`HOLD_STEPS`]).
    hold: u32,
    run: u32,
    scrape: f32,
    /// The ground loop's level in a slide ([`SLIDE_GROUND`]).
    ground: f32,
}

impl Impacts {
    pub fn new(ranged_gain: f32, scream_gain: f32, ranged_count: usize, scream_count: usize) -> Impacts {
        Impacts { ranged_gain, scream_gain, ranged_count, scream_count, ..Impacts::default() }
    }

    /// The commands for a step's payload with the car at `speed` m/s; `rand` stands for the original's `rand()`
    /// calls. The first payload seen is taken as the state before the drive. `slide` is set while no wheel is on the
    /// ground, to whether the ground the wheels last ran on is loose.
    pub fn step(
        &mut self,
        hit: &[u32; 20],
        speed: f32,
        inside: bool,
        slide: Option<bool>,
        mut rand: impl FnMut() -> u32,
    ) -> Vec<Command> {
        let emitted: [u32; 18] = std::array::from_fn(|i| hit[if i == 0 { 1 } else { i + 2 }]);
        let new = self.last.is_some_and(|last| last != emitted);
        self.last = Some(emitted);
        self.wait = self.wait.saturating_sub(1);
        let power = f32::from_bits(hit[POWER]);
        let ranged = (1..4).contains(&(hit[STATE] as i32));
        let new = new && power >= MIN_POWER;
        let touching = self.hold > 0;
        let mut out = Vec::new();
        if new && power > SCREAM_POWER && inside && self.scream_count > 0 {
            out.push(Command::StopSpeech);
            out.push(Command::Shot { shot: Shot::Scream(rand() as usize % self.scream_count), gain: self.scream_gain });
        }
        if new && ranged && ((self.wait == 0 && !touching) || power > self.last_power * HARDER) {
            if let Some(index) = ranged_index(power + RANGED_ADD, self.ranged_count, &mut rand) {
                let gain = self.ranged_gain * (power / FULL_POWER).min(1.0);
                out.push(Command::Shot { shot: Shot::Ranged(index), gain });
                (self.wait, self.last_power) = (GAP_STEPS, power);
            }
        }
        if new && ranged {
            (self.hold, self.run) = (if slide.is_some() { SLIDE_HOLD } else { HOLD_STEPS }, self.run + 1);
        } else {
            self.hold = self.hold.saturating_sub(1);
            if self.hold == 0 {
                self.run = 0;
            }
        }
        let pace = ((speed.abs() - SCRAPE_SPEED[0]) / (SCRAPE_SPEED[1] - SCRAPE_SPEED[0])).clamp(0.0, 1.0);
        let scraping = if self.run > 1 { self.ranged_gain * pace } else { 0.0 };
        let (heavier, lower) = if slide.is_some() { (SLIDE_GAIN, SLIDE_PITCH) } else { (1.0, 1.0) };
        let lerp = |[from, to]: [f32; 2]| from + (to - from) * pace;
        let loops = [
            (
                Loop::Scrape,
                &mut self.scrape,
                SCRAPE_GAIN * heavier * (power / FULL_POWER).min(1.0),
                lerp(SCRAPE_PITCH) * lower,
            ),
            (
                Loop::Slide,
                &mut self.ground,
                if slide == Some(true) { SLIDE_GROUND } else { 0.0 },
                lerp(SLIDE_GROUND_PITCH),
            ),
        ];
        for (voice, level, strength, pitch) in loops {
            let (was, target) = (*level, scraping * strength);
            *level += (target - was) * if target > was { SCRAPE_RISE } else { SCRAPE_FALL };
            if *level < SCRAPE_FLOOR && target == 0.0 {
                *level = 0.0;
            }
            if *level > 0.0 || was > 0.0 {
                out.push(Command::Loop { voice, gain: *level, pitch });
            }
        }
        out
    }
}

/// `Vf64`'s pick: `rand() % 3` plus the steps above 200, and past the end `count - 1 - rand() % 3`.
pub fn ranged_index(v: f32, count: usize, rand: &mut impl FnMut() -> u32) -> Option<usize> {
    if v < RANGED_FROM || count == 0 {
        return None;
    }
    let index = (rand() % 3) as usize + ((v - RANGED_FROM) * RANGED_STEP) as usize;
    Some(if index >= count { (count - 1).saturating_sub((rand() % 3) as usize) } else { index })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(power: f32) -> [u32; 20] {
        let mut hit = [0; 20];
        hit[STATE] = 2;
        hit[POWER] = power.to_bits();
        hit
    }

    #[test]
    fn the_ranged_index_follows_the_power() {
        let mut zero = || 0;
        assert_eq!(ranged_index(150.0, 20, &mut zero), None);
        assert_eq!(ranged_index(1500.0, 20, &mut zero), Some(6));
        assert_eq!(ranged_index(9000.0, 20, &mut || 2), Some(17));
    }

    #[test]
    fn a_new_payload_plays_once_and_a_hard_one_screams_in_the_car() {
        let mut impacts = Impacts::new(1.0, 0.5, 20, 2);
        assert!(impacts.step(&hit(900.0), 0.0, true, None, || 0).is_empty());
        assert!(impacts.step(&hit(900.0), 0.0, true, None, || 0).is_empty());
        let mut functor_only = hit(900.0);
        functor_only[0] = 7;
        functor_only[2] = 3;
        assert!(impacts.step(&functor_only, 0.0, true, None, || 0).is_empty());
        let out = impacts.step(&hit(1000.0), 0.0, true, None, || 0);
        assert_eq!(out, [Command::Shot { shot: Shot::Ranged(6), gain: 1.0 }]);
        // A light touch plays at its power's share of 1000, once the gap is over.
        (0..GAP_STEPS).for_each(|_| assert!(impacts.step(&hit(1000.0), 0.0, true, None, || 0).is_empty()));
        let out = impacts.step(&hit(250.0), 0.0, true, None, || 0);
        assert_eq!(out, [Command::Shot { shot: Shot::Ranged(2), gain: 0.25 }]);
        let out = impacts.step(&hit(9000.0), 0.0, true, None, || 1);
        assert_eq!(out[..2], [Command::StopSpeech, Command::Shot { shot: Shot::Scream(1), gain: 0.5 }]);
        // Outside the car no scream; within the gap and not 1.5 times harder, no ranged impact either.
        assert!(impacts.step(&hit(9500.0), 0.0, false, None, || 0).is_empty());
    }

    #[test]
    fn a_scrape_plays_its_first_impact_the_harder_ones_and_the_loop() {
        let mut impacts = Impacts::new(1.0, 0.5, 20, 2);
        impacts.step(&hit(0.0), 20.0, true, None, || 0);
        // A new payload every step for a second at 20 m/s: the first impact and the harder one, no others.
        let steps: Vec<Vec<Command>> = (0..143)
            .map(|k| impacts.step(&hit(if k == 30 { 3000.0 } else { 1000.0 + k as f32 }), 20.0, true, None, || 0))
            .collect();
        let shot = |out: &Vec<Command>| out.iter().any(|c| matches!(c, Command::Shot { .. }));
        assert_eq!((0..143).filter(|&k| shot(&steps[k])).collect::<Vec<_>>(), [0, 30]);
        // The loop opens with the second payload and reaches the full level and pitch.
        let level = |out: &Vec<Command>| {
            out.iter().find_map(|c| match *c {
                Command::Loop { voice: Loop::Scrape, gain, pitch } => Some((gain, pitch)),
                _ => None,
            })
        };
        assert_eq!((level(&steps[0]), level(&steps[1])), (None, Some((0.5, 1.2))));
        assert!(level(&steps[142]).unwrap().0 > 1.99);
        // The contact over: the loop falls to nothing and the next touch is an impact again.
        let after: Vec<Vec<Command>> = (0..200).map(|_| impacts.step(&hit(1142.0), 20.0, true, None, || 0)).collect();
        assert_eq!((level(&after[0]).unwrap().0 > 1.8, level(&after[199])), (true, None));
        assert!(after.iter().all(|out| !shot(out)));
        assert!(shot(&impacts.step(&hit(500.0), 20.0, true, None, || 0)));
        // A car at rest against a wall does not scrape.
        let mut impacts = Impacts::new(1.0, 0.5, 20, 2);
        impacts.step(&hit(0.0), 0.0, true, None, || 0);
        assert!((0..50).all(|k| level(&impacts.step(&hit(900.0 + k as f32), 0.5, true, None, || 0)).is_none()));
    }

    #[test]
    fn a_body_sliding_with_no_wheel_down_scrapes_heavier_and_on_loose_ground_plays_it() {
        let level = |slide: Option<bool>, voice: Loop| {
            let mut impacts = Impacts::new(1.0, 0.5, 20, 2);
            impacts.step(&hit(0.0), 20.0, true, slide, || 0);
            let out: Vec<Command> =
                (0..143).flat_map(|k| impacts.step(&hit(700.0 + (k % 2) as f32), 20.0, true, slide, || 0)).collect();
            let last = out.iter().rev().find_map(|c| match c {
                Command::Loop { voice: v, gain, pitch } if *v == voice => Some((*gain, *pitch)),
                _ => None,
            });
            // The payload pauses for 20 steps: on its wheels the contact ends, in a slide it holds.
            let held = (0..20).all(|_| {
                impacts.step(&hit(700.0), 20.0, true, slide, || 0);
                impacts.run > 0
            });
            (last, held)
        };
        let (Some((wheels, pitch)), false) = level(None, Loop::Scrape) else { panic!() };
        let (Some((roof, lower)), true) = level(Some(false), Loop::Scrape) else { panic!() };
        assert!((roof / wheels - SLIDE_GAIN).abs() < 1e-3 && (lower / pitch - SLIDE_PITCH).abs() < 1e-6);
        // The ground's loop only on loose ground.
        assert_eq!(level(None, Loop::Slide).0, None);
        assert_eq!(level(Some(false), Loop::Slide).0, None);
        assert!(level(Some(true), Loop::Slide).0.is_some_and(|(gain, _)| (gain - SLIDE_GROUND).abs() < 1e-3));
    }
}
