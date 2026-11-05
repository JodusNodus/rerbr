//! The engine's reflections (rbr-recomp `notes/sound.md` 5.4): the `Reflection L` and `R` streams (`CCarSound +0x1090`,
//! `+0x1094`), fed in `CCarSound::Vf08` (0x4b0b50, 0x4b11cd-0x4b14fe) from the engine's block after `MixScale`
//! (`CDXEngine +0x20df8`, the same block the engine stream plays):
//!
//! - `E25Engine` (`CCarSound +0x111c`) low-passes it (`Filter80` 0x4b8780, a one-pole bilinear filter; `Mix60`
//!   0x4b8860, `RunD0` 0x4b87d0) into a 32768-sample ring, and `Blend90` (0x4b8e90) reads the ring back a delay behind
//!   for each side, times the side's gain and a volume, as two 16-bit blocks (`+0x1138`, `+0x11138`);
//! - in the car with the side's door off the car (`+0x41a9c` left, `+0x41aa0` right) a side's stream gets the delayed
//!   block plus the engine's block times `trunc(engine volume * 512) / 256`, clamped (0x4b13dd); otherwise the
//!   delayed block alone (0x4b144e).
//!
//! The delays and gains are the master's `+0x106cc..+0x106d8`. In the car `Vf08` sets them to 0.4 m and 1.4 m and gains
//! 1, and `Mcb80` (0x4acb80) then sets the left one to 1.6 m: delays of `trunc(m / 340 * 44100)` samples, 207 and 181.
//! Outside nothing sets them, so they keep the car's values once the camera has been in the car (before that 0: the
//! master's heap block, read as zero, **inferred**). The stage's `Real` animation channels of tags 0x15-0x18 could
//! set them (`E12_AnimSet::CbD80` 0x477d80 on `CSigAnimReal`, emitted by `Tic140` 0x478140), but no retail stage has
//! one: five `.dls` files have a single `Real` channel, of tag 11. The low-pass cutoff is 750 Hz in the car, else
//! `min(max((left + right) * 31.25, 100), min(16000 / distance, 2000))` Hz (0x4b1232-0x4b12e6). The volume is 256
//! from 4500 rpm (0x1194), below it `128 + trunc(rpm * 0.028444)` (0x4b1344). The read cursor starts 128 samples
//! behind the write cursor (`InitD0` 0x4b86d0: 0x7f80), which every delay adds.
//!
//! The voices are 3D, head-relative, at 1 m left and right (`CCarSound::Vf04`, the DX mixer's `+0x70` is 0), at the
//! `EnviormentalReflections` amp times the engine volume (`CCarSound +0x1d8`). Ours: at the output rate (delays and
//! filter for it, not 44.1 kHz), the left voice in the left channel and the right in the right, the delay change at
//! the first frame in the car taken at once (`StereoD0` 0x4b88d0 crossfades 256 samples), the in-car volume
//! option taken as 1, and the low-pass after the ring (one per side; the same output for a fixed cutoff).
//!
//! Ours (realism, game feel; the tuning constants below are ours): with a [`Roadside`] (the presentation's probe of
//! the stage beside the car, `rbr_render::roadside`) the sides follow the real roadside in place of the fixed
//! delays: each side's delay is the round trip to its nearest reflecting surface, its gain falls with distance and
//! rises with the surface's size and hardness ([`side_gain`]), its low-pass closes with distance and softness
//! ([`side_cutoff`], under the in-car 750 Hz in the car and the outside cap outside), and a hard ceiling (a bridge)
//! adds a short tap to both sides. Open ground on both sides fades them to nothing. Delay, gain and cutoff glide
//! ([`DELAY_GLIDE`], [`GAIN_GLIDE`]) so a change does not zipper while trees and posts going by still flutter; the
//! delay glides no faster than a passing reflector's (a Doppler bend, [`DELAY_SLEW`]), and a jump to a new surface
//! crossfades ([`JUMP_METRES`]). The door-open term is as
//! above. Without a `Roadside` the 1.02 behaviour above applies.

use crate::mixer::voices::OnePole;

/// The ring's length (`+0x205bc`, 0x8000 words).
const RING: usize = 0x8000;
/// The read cursor's start behind the write cursor (`InitD0`: 0x7f80 against 0).
const READ_LAG: usize = 0x80;
/// Samples per metre (0x7319d8 `1 / 340` times 0x7319d4 44100), at 44.1 kHz; the delay cap (0x60c0).
const PER_METRE: f32 = 0.002_941_176_5;
const MAX_DELAY: usize = 0x60c0;
/// The master's delays (m) and gains in the car.
const INSIDE_METRES: [f32; 2] = [1.6, 1.4];
const INSIDE_GAINS: [f32; 2] = [1.0, 1.0];
/// The in-car cutoff (0x443b8000), the outside floor and cap, the distance term (0x72bf14 320 times 0x7304a0 50) and
/// the factor on the distance sum (0x731d04 times 100).
const INSIDE_CUTOFF: f32 = 750.0;
const OUTSIDE_FLOOR: f32 = 100.0;
const OUTSIDE_CAP: f32 = 2000.0;
const PER_DISTANCE: f32 = 16000.0;
const PER_METRE_SUM: f32 = 31.25;
/// The volume's full-scale rpm and its slope below it (0x731cfc times 0x731cf8 -128).
const FULL_RPM: u32 = 0x1194;
const RPM_SLOPE: f32 = 0.000_222_222_22;
/// The direct block's clamp (0x5fffff and -0x600000, in the 16-bit scale).
const DIRECT_CLAMP: f32 = 24576.0;

/// Ours (tuning): the gain of a full-size surface of hardness 1 at [`NEAR`] m or nearer, against the 1.02 in-car
/// gain of 1, and the most a side takes. Set so a forest stage's in-car average sits at about the 1.02 level
/// (`notes/sound.md`).
pub const ECHO_GAIN: f32 = 4.5;
pub const GAIN_CAP: f32 = 2.0;
/// Ours (tuning): the distance, m, inside which a side's echo is full; beyond it the gain falls as `NEAR / d`.
pub const NEAR: f32 = 5.0;
/// Ours (tuning): the cutoff, Hz, of a soft surface (a bank, a hedge of trunks) and a hard one (rock, a wall, metal),
/// near; it halves by [`ROLL_OFF`] m.
pub const SOFT_CUTOFF: f32 = 1200.0;
pub const HARD_CUTOFF: f32 = 6000.0;
pub const ROLL_OFF: f32 = 40.0;
/// Ours (tuning): the overhead tap's share of a side's gain law.
pub const OVERHEAD_SHARE: f32 = 0.7;
/// Ours (tuning): the time constants, s, the delay and the gain and cutoff glide toward the probe's with.
pub const DELAY_GLIDE: f32 = 0.03;
pub const GAIN_GLIDE: f32 = 0.025;
/// Ours: the fastest the delay glides, samples a sample: a passing reflector's own rate at 100 km/h
/// (`2 v / 340`), so the glide bends the pitch no more than a real one does.
pub const DELAY_SLEW: f32 = 0.16;
/// Ours (tuning): a delay change longer than this, m of path, is a new surface (the nearest tree changing): the tap
/// crossfades to it over [`JUMP_FADE`] s in place of gliding.
pub const JUMP_METRES: f32 = 1.5;
pub const JUMP_FADE: f32 = 0.02;

/// One side's nearest reflecting surface, as the probe sees it: its distance, m, from the car, how much of the side
/// it fills (0 none, 1 a wall), and how hard it is (0 soft, 1 rock or metal).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Side {
    pub distance: f32,
    pub size: f32,
    pub hardness: f32,
}

/// The roadside about the car: its left and right as the listener hears them (the presentation swaps them for a
/// camera facing the car), and a ceiling over it (`distance` its height).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Roadside {
    pub sides: [Side; 2],
    pub overhead: Side,
}

/// A side's gain (ours): [`ECHO_GAIN`] times size and hardness, falling as `NEAR / distance` beyond [`NEAR`], at
/// most [`GAIN_CAP`].
pub fn side_gain(side: &Side) -> f32 {
    let gain = ECHO_GAIN * side.size.clamp(0.0, 1.0) * side.hardness.clamp(0.0, 1.0) * (NEAR / side.distance.max(NEAR));
    gain.min(GAIN_CAP)
}

/// A side's cutoff, Hz (ours): from [`SOFT_CUTOFF`] to [`HARD_CUTOFF`] with hardness, halved by [`ROLL_OFF`] m;
/// under the in-car cutoff in the car, and outside under 1.02's cap for a camera `distance` m from the car.
pub fn side_cutoff(side: &Side, inside: bool, distance: f32) -> f32 {
    let surface = (SOFT_CUTOFF + (HARD_CUTOFF - SOFT_CUTOFF) * side.hardness.clamp(0.0, 1.0))
        / (1.0 + side.distance.max(0.0) / ROLL_OFF);
    let cap = if inside { INSIDE_CUTOFF } else { (PER_DISTANCE / distance).min(OUTSIDE_CAP) };
    surface.min(cap).max(OUTSIDE_FLOOR.min(cap))
}

/// A side's delay, samples at `rate`: the round trip to its surface.
pub fn side_delay(side: &Side, rate: u32) -> f32 {
    (2.0 * side.distance.max(0.0) * PER_METRE * rate as f32).min(MAX_DELAY as f32)
}

/// The two reflections' state.
#[derive(Debug, Clone)]
pub struct Reflections {
    ring: Vec<f32>,
    at: usize,
    /// The left, right and overhead taps' filters.
    low_pass: [OnePole; 3],
    /// The camera has been in the car: the master holds the car's delays and gains.
    pub set: bool,
    /// The taps' current delay (samples), scale and cutoff; `None` until the first block.
    taps: Option<[Tap; 3]>,
}

/// One tap of the ring: its delay, samples, the scale on the ring's sample, and its cutoff, Hz; while it crossfades
/// to a new delay, the delay it leaves and the fade's progress (1 done).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tap {
    delay: f32,
    scale: f32,
    cutoff: f32,
    from: f32,
    fade: f32,
}

impl Tap {
    fn new(delay: f32, scale: f32, cutoff: f32) -> Tap {
        Tap { delay, scale, cutoff, from: delay, fade: 1.0 }
    }
}

impl Default for Reflections {
    fn default() -> Self {
        Reflections { ring: vec![0.0; RING], at: 0, low_pass: Default::default(), set: false, taps: None }
    }
}

/// `Blend90`'s volume at `rpm` (the engine's after the slew).
pub fn volume(rpm: u32) -> i32 {
    if rpm >= FULL_RPM {
        0x100
    } else {
        0x80 - (rpm as f32 * RPM_SLOPE * -128.0) as i32
    }
}

/// The low-pass cutoff, Hz: in the car, or outside at `distance` m from the car with the master's delays `metres`.
pub fn cutoff(inside: bool, metres: [f32; 2], distance: f32) -> f32 {
    if inside {
        return INSIDE_CUTOFF;
    }
    let far = (PER_DISTANCE / distance).min(OUTSIDE_CAP);
    ((metres[0] + metres[1]) * PER_METRE_SUM).max(OUTSIDE_FLOOR).min(far)
}

impl Reflections {
    /// The master's delays (m) and gains.
    pub fn params(&self) -> ([f32; 2], [f32; 2]) {
        if self.set {
            (INSIDE_METRES, INSIDE_GAINS)
        } else {
            ([0.0; 2], [0.0; 2])
        }
    }

    /// The taps 1.02 sets: the master's delays and gains, no overhead tap.
    fn fixed(&self, rate: u32, inside: bool, distance: f32, vol: i32) -> [Tap; 3] {
        let (metres, gains) = self.params();
        let cut = cutoff(inside, metres, distance);
        // `Blend90`: `ring * ((trunc(gain * 128) * vol) >> 8) >> 16` on the ring's `y * 256`.
        let tap = |side: usize| {
            Tap::new(
                (((metres[side] * PER_METRE * rate as f32) as usize).min(MAX_DELAY) + READ_LAG) as f32,
                (((gains[side] * 128.0) as i32 * vol) >> 8) as f32 / 256.0,
                cut,
            )
        };
        [tap(0), tap(1), Tap::new(0.0, 0.0, cut)]
    }

    /// The taps the roadside asks for (ours); a side with nothing keeps its delay and fades.
    fn probed(&self, roadside: &Roadside, rate: u32, inside: bool, distance: f32, vol: i32) -> [Tap; 3] {
        let surfaces = [roadside.sides[0], roadside.sides[1], roadside.overhead];
        std::array::from_fn(|i| {
            let side = &surfaces[i];
            let share = if i == 2 { OVERHEAD_SHARE } else { 1.0 };
            let gain = side_gain(side) * share;
            let delay = match self.taps {
                Some(taps) if gain <= 0.0 => taps[i].delay,
                _ => side_delay(side, rate),
            };
            // The continuous form of `Blend90`'s scale (its truncation would step the glide).
            Tap::new(delay, gain * vol as f32 / 512.0, side_cutoff(side, inside, distance))
        })
    }

    /// Adds both streams of the engine's block `raw` (after `MixScale`, 16-bit scale) into `out` at `gain` (each
    /// sample's, from the voices' gain move): in the car or not, `distance` m from the car, at `rpm`, with each
    /// side's door off the car or on it, and the probed `roadside` (ours) or the 1.02 delays.
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &mut self,
        raw: &[f32],
        rate: u32,
        inside: bool,
        distance: f32,
        rpm: u32,
        doors: [bool; 2],
        roadside: Option<&Roadside>,
        out: &mut [[f32; 2]],
        mut gain: impl FnMut() -> f32,
    ) {
        self.set |= inside;
        let vol = volume(rpm);
        let target = match roadside {
            Some(r) => self.probed(r, rate, inside, distance, vol),
            None => self.fixed(rate, inside, distance, vol),
        };
        // 1.02's taps are taken at once; the probe's glide from where they are.
        let glide = roadside.is_some();
        let mut taps = match self.taps {
            Some(taps) if glide => taps,
            _ => target,
        };
        let per = |tau: f32| 1.0 - (-1.0 / (tau * rate as f32)).exp();
        let (kd, kg) = (per(DELAY_GLIDE), per(GAIN_GLIDE));
        let (jump, step) = (JUMP_METRES * PER_METRE * rate as f32, 1.0 / (JUMP_FADE * rate as f32));
        // The cutoff glides once a block, by the block's share of the gain's time constant.
        let kc = 1.0 - (1.0 - kg).powi(raw.len().min(out.len()) as i32);
        for (tap, (filter, want)) in taps.iter_mut().zip(self.low_pass.iter_mut().zip(&target)) {
            if glide {
                tap.cutoff += (want.cutoff - tap.cutoff) * kc;
            }
            filter.retune(rate, tap.cutoff);
        }
        for (frame, &x) in out.iter_mut().zip(raw) {
            self.ring[self.at] = x;
            if glide {
                for (tap, want) in taps.iter_mut().zip(&target) {
                    let to = want.delay - tap.delay;
                    if tap.fade < 1.0 {
                        tap.fade = (tap.fade + step).min(1.0);
                    } else if to.abs() > jump {
                        (tap.from, tap.delay, tap.fade) = (tap.delay, want.delay, 0.0);
                    } else {
                        tap.delay += (to * kd).clamp(-DELAY_SLEW, DELAY_SLEW);
                    }
                    tap.scale += (want.scale - tap.scale) * kg;
                }
            }
            let ring = &self.ring;
            let at = self.at;
            let read = |delay: f32| {
                let whole = delay as usize;
                let a = ring[(at + RING - whole) % RING];
                let frac = delay - whole as f32;
                if frac == 0.0 {
                    a
                } else {
                    a + (ring[(at + 2 * RING - whole - 1) % RING] - a) * frac
                }
            };
            let filters = &mut self.low_pass;
            let delayed: [f32; 3] = std::array::from_fn(|i| {
                // A silent tap skips its filter (the overhead tap without a ceiling, 1.02's always).
                if taps[i].scale == 0.0 {
                    0.0
                } else {
                    let tap = &taps[i];
                    let y = if tap.fade < 1.0 {
                        read(tap.from) + (read(tap.delay) - read(tap.from)) * tap.fade
                    } else {
                        read(tap.delay)
                    };
                    filters[i].run(y) * tap.scale
                }
            });
            let direct = (x * 2.0).clamp(-DIRECT_CLAMP, DIRECT_CLAMP);
            let g = gain();
            for side in 0..2 {
                let d = delayed[side] + delayed[2];
                let s = if inside && doors[side] { d + direct } else { d };
                frame[side] += s * g;
            }
            self.at = (self.at + 1) % RING;
        }
        self.taps = Some(taps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_volume_rises_with_rpm_to_full_at_4500() {
        assert_eq!(volume(0), 0x80);
        assert_eq!(volume(1000), 0x80 + 28);
        assert_eq!(volume(4499), 0xff);
        assert_eq!(volume(4500), 0x100);
    }

    #[test]
    fn the_cutoff_is_750_in_the_car_and_100_outside_until_far() {
        assert_eq!(cutoff(true, [0.0; 2], 5.0), 750.0);
        assert_eq!(cutoff(false, INSIDE_METRES, 5.0), 100.0);
        assert_eq!(cutoff(false, INSIDE_METRES, 320.0), 50.0);
    }

    #[test]
    fn an_impulse_comes_back_a_delay_later_on_each_side() {
        let mut r = Reflections::default();
        let mut raw = vec![0.0; 600];
        raw[0] = 10000.0;
        let mut out = vec![[0.0; 2]; 600];
        r.add(&raw, 44100, true, 1.0, 5000, [false; 2], None, &mut out, || 1.0);
        let onset = |side: usize| out.iter().position(|f| f[side] != 0.0).unwrap();
        // 1.6 m: 207 samples, 1.4 m: 181, each 128 more.
        assert_eq!(onset(0), 207 + 128);
        assert_eq!(onset(1), 181 + 128);
    }

    #[test]
    fn a_missing_door_lets_the_engine_through_in_the_car_only() {
        let raw = vec![1000.0; 4];
        for (inside, doors, direct) in [(true, [true, false], [2000.0, 0.0]), (false, [true, true], [0.0, 0.0])] {
            let mut r = Reflections { set: true, ..Reflections::default() };
            let mut out = vec![[0.0; 2]; 4];
            r.add(&raw, 44100, inside, 1.0, 5000, doors, None, &mut out, || 1.0);
            assert_eq!(out[3], direct);
        }
    }

    #[test]
    fn outside_before_the_car_the_streams_are_silent() {
        let mut r = Reflections::default();
        let raw = vec![1000.0; 1000];
        let mut out = vec![[0.0; 2]; 1000];
        r.add(&raw, 44100, false, 3.0, 3000, [true; 2], None, &mut out, || 1.0);
        assert!(out.iter().all(|f| *f == [0.0; 2]));
    }

    fn wall(distance: f32, hardness: f32) -> Side {
        Side { distance, size: 1.0, hardness }
    }

    #[test]
    fn the_delay_is_the_round_trip_to_the_surface() {
        // 10 m out and back at 340 m/s: 58.8 ms.
        assert!((side_delay(&wall(10.0, 1.0), 44100) - 20.0 / 340.0 * 44100.0).abs() < 0.01);
        assert_eq!(side_delay(&wall(0.0, 1.0), 48000), 0.0);
        assert_eq!(side_delay(&wall(1000.0, 1.0), 48000), MAX_DELAY as f32);
    }

    #[test]
    fn the_gain_falls_with_distance_and_softness_and_is_zero_with_nothing() {
        let soft = |d: f32| side_gain(&wall(d, 0.5));
        assert_eq!(soft(1.0), ECHO_GAIN / 2.0);
        assert_eq!(soft(NEAR), ECHO_GAIN / 2.0);
        assert!((soft(4.0 * NEAR) - ECHO_GAIN / 8.0).abs() < 1e-6);
        assert!((side_gain(&wall(NEAR, 0.25)) - ECHO_GAIN / 4.0).abs() < 1e-6);
        // A hard wall at arm's length is held at the cap.
        assert_eq!(side_gain(&wall(1.0, 1.0)), GAIN_CAP);
        assert_eq!(side_gain(&Side::default()), 0.0);
    }

    #[test]
    fn the_cutoff_closes_with_distance_and_softness_under_the_cabin_and_the_far_cap() {
        let outside = |s: &Side| side_cutoff(s, false, 1.0);
        assert_eq!(outside(&wall(0.0, 1.0)), OUTSIDE_CAP);
        assert_eq!(outside(&wall(0.0, 0.0)), SOFT_CUTOFF);
        assert_eq!(outside(&wall(ROLL_OFF, 0.0)), SOFT_CUTOFF / 2.0);
        assert!(outside(&wall(5.0, 0.2)) > outside(&wall(30.0, 0.2)));
        assert_eq!(side_cutoff(&wall(0.0, 1.0), true, 1.0), INSIDE_CUTOFF);
        // A soft bank far off stays under the cabin's cutoff in the car, and outside the camera's distance caps it.
        assert!(side_cutoff(&wall(35.0, 0.0), true, 1.0) < INSIDE_CUTOFF);
        assert_eq!(side_cutoff(&wall(0.0, 1.0), false, 160.0), 100.0);
    }

    /// The first sample each side of `out` that is not silent.
    fn onsets(out: &[[f32; 2]]) -> [usize; 2] {
        std::array::from_fn(|side| out.iter().position(|f| f[side].abs() > 1e-3).unwrap())
    }

    #[test]
    fn a_probed_side_comes_back_after_its_round_trip_and_open_ground_is_silent() {
        let roadside = Roadside { sides: [wall(3.4, 1.0), wall(6.8, 1.0)], overhead: Side::default() };
        let mut r = Reflections::default();
        let mut raw = vec![0.0; 2000];
        raw[0] = 10000.0;
        let mut out = vec![[0.0; 2]; 2000];
        r.add(&raw, 44100, true, 1.0, 5000, [false; 2], Some(&roadside), &mut out, || 1.0);
        // 6.8 m and 13.6 m of path: 882 and 1764 samples (the fractional read may lead by one).
        let [left, right] = onsets(&out);
        assert!((881..=882).contains(&left) && (1763..=1764).contains(&right), "{left} {right}");
        let mut r = Reflections::default();
        let raw = vec![1000.0; 2000];
        let mut out = vec![[0.0; 2]; 2000];
        r.add(&raw, 44100, true, 1.0, 5000, [false; 2], Some(&Roadside::default()), &mut out, || 1.0);
        assert!(out.iter().all(|f| *f == [0.0; 2]));
    }

    #[test]
    fn a_change_of_roadside_glides_without_a_step() {
        let mut r = Reflections::default();
        let raw: Vec<f32> = (0..4410).map(|i| (i as f32 * 0.05).sin() * 8000.0).collect();
        let near = Roadside { sides: [wall(2.0, 1.0); 2], overhead: Side::default() };
        let far = Roadside { sides: [wall(20.0, 0.3); 2], overhead: Side::default() };
        let mut out = vec![[0.0; 2]; 4410];
        r.add(&raw, 44100, false, 3.0, 5000, [false; 2], Some(&near), &mut out, || 1.0);
        let mut next = vec![[0.0; 2]; 4410];
        r.add(&raw, 44100, false, 3.0, 5000, [false; 2], Some(&far), &mut next, || 1.0);
        // No sample-to-sample jump over the change much above the signal's own slope.
        let slope = |o: &[[f32; 2]]| o.windows(2).map(|w| (w[1][0] - w[0][0]).abs()).fold(0.0f32, f32::max);
        let joined: Vec<[f32; 2]> = out[4000..].iter().chain(&next[..400]).copied().collect();
        assert!(slope(&joined) <= slope(&out[2000..]) * 1.5, "{} {}", slope(&joined), slope(&out[2000..]));
        // And it has moved most of the way by the end of the second block (0.1 s, past 2.5 gain glides).
        let tail = next[4000..].iter().map(|f| f[0].abs()).fold(0.0f32, f32::max);
        let head = out[4000..].iter().map(|f| f[0].abs()).fold(0.0f32, f32::max);
        assert!(tail < head * 0.2, "{tail} {head}");
    }
}
