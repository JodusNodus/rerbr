//! Ours: the car meeting the verge, from what the renderer sees after each frame's step ([`Verge`]; 1.02 has no such
//! sounds): the tyres on grass, the bushes brushing the body and the branches slapping it. Pure: gains, pitches and
//! the hits to play per step, on the embedded recordings (`sounds/CREDITS.md`).
//!
//! - Grass (`Loop::Grass`): a swish by how many wheels are on grass ([`Verge::grass`], the square root of their share
//!   of four) and the car's speed ([`GRASS_SPEED`]), brighter with speed ([`GRASS_PITCH`]).
//! - Brush (`Loop::Brush`): the rustle of the bushes and bendable trees the body is in, by how deep it is in each
//!   ([`BRUSH_DEPTH`]) times its density, at most 1, and the speed ([`BRUSH_SPEED`]).
//! - Branch hits (`Shot::Branch`): the body meeting a bendable tree, or (softer, [`BUSH_HIT`]) a bush, it was not
//!   touching at the last frame, by the speed ([`HIT_SPEED`]); the samples are taken in turn, not from the random
//!   numbers, so the other sounds stay as without the verge.
//!
//! All three play at the car with the tyres' distance gain; in the car the loops come through the cabin low-pass and
//! the hits ring the shell as the stones do (`crate::mixer`). Without a [`Verge`] nothing is commanded, so the sound is
//! as without the verge, sample for sample.

use crate::voice::{Command, Loop, Shot};

/// The most contacts a frame carries (the deepest).
pub const TOUCHES: usize = 8;

/// A scenery placement the body is in: its index in the stage's roadside (`rbr_render::roadside`), how far the body
/// reaches into it, m, how dense it is (0 to 1), and whether it is a bendable tree's branches rather than a bush.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Touch {
    pub id: u32,
    pub depth: f32,
    pub density: f32,
    pub branches: bool,
}

/// The car at the verge for a frame: each wheel's share on grass (0 to 1) and the vegetation the body is in.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Verge {
    pub grass: [f32; 4],
    pub touches: [Option<Touch>; TOUCHES],
}

/// The grass loop: full at [`GRASS_GAIN`] with four wheels on grass, rising with the speed over [`GRASS_SPEED`] m/s
/// (5 to 70 km/h); its pitch from [`GRASS_PITCH`]'s first at standstill to its second at [`PITCH_TOP`] m/s.
const GRASS_GAIN: f32 = 2.0;
const GRASS_SPEED: [f32; 2] = [1.5, 20.0];
const GRASS_PITCH: [f32; 2] = [0.8, 1.2];
/// The brush loop: a placement adds its density, full once the body is [`BRUSH_DEPTH`] m into it; at most 1, at
/// [`BRUSH_GAIN`], rising with the speed over [`BRUSH_SPEED`] m/s.
const BRUSH_DEPTH: f32 = 0.5;
const BRUSH_GAIN: f32 = 1.4;
const BRUSH_SPEED: [f32; 2] = [0.5, 12.0];
const BRUSH_PITCH: [f32; 2] = [0.85, 1.15];
/// The loops' levels follow a rise at once and fall with this time constant, s, so a wheel skipping over the grass
/// or the body clearing a bush for a frame does not chop them.
const RELEASE: f32 = 0.15;
const QUIET: f32 = 1e-4;
/// The top of the loops' pitch rise, m/s (108 km/h).
const PITCH_TOP: f32 = 30.0;
/// A new contact's hit: [`HIT_GAIN`] by the speed over [`HIT_SPEED`] m/s (7 to 72 km/h), a bush's times its density
/// and [`BUSH_HIT`]; none below the first speed; at most [`HITS`] a frame.
const HIT_GAIN: f32 = 1.0;
const HIT_SPEED: [f32; 2] = [2.0, 20.0];
const BUSH_HIT: f32 = 0.5;
const HITS: usize = 2;
/// A hit's pitch: lower for a denser bush or a tree's branches, higher with speed.
const HIT_PITCH: [f32; 2] = [0.9, 1.1];

fn smoothstep([from, to]: [f32; 2], x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn pitch([low, high]: [f32; 2], speed: f32) -> f32 {
    low + (high - low) * (speed / PITCH_TOP).clamp(0.0, 1.0)
}

/// The verge's state between steps.
#[derive(Debug, Clone, Default)]
pub struct Verges {
    verge: Option<Verge>,
    /// The verge is new since the last step: its contacts are compared once with the last frame's.
    fresh: bool,
    /// The placements the body touched at the last frame.
    last: Vec<u32>,
    /// The grass and brush loops' levels ([`RELEASE`]).
    levels: [f32; 2],
    /// The branch samples (`data::CarBank::branches`) and the hits made, which pick them in turn.
    pub samples: usize,
    hits: usize,
}

impl Verges {
    pub fn new(samples: usize) -> Verges {
        Verges { samples, ..Verges::default() }
    }

    /// The car at the verge at the renderer's last frame; `None` plays nothing of it.
    pub fn set(&mut self, verge: Option<Verge>) {
        if verge.is_none() {
            self.last.clear();
            self.levels = [0.0; 2];
        }
        self.verge = verge;
        self.fresh = true;
    }

    /// The commands for one step of `dt` seconds at car speed `speed` m/s, the car `distance` m from the listener.
    pub fn step(&mut self, speed: f32, distance: f32, dt: f32) -> Vec<Command> {
        let Some(verge) = self.verge else { return Vec::new() };
        let speed = speed.abs();
        let on_grass = verge.grass.iter().map(|g| g.clamp(0.0, 1.0)).sum::<f32>() / 4.0;
        let grass = GRASS_GAIN * on_grass.sqrt() * smoothstep(GRASS_SPEED, speed);
        let touches = verge.touches.iter().flatten();
        let level = touches.clone().map(|t| t.density * (t.depth / BRUSH_DEPTH).clamp(0.0, 1.0)).sum::<f32>();
        let brush = BRUSH_GAIN * level.min(1.0) * smoothstep(BRUSH_SPEED, speed);
        let fall = (-dt / RELEASE).exp();
        for (now, target) in self.levels.iter_mut().zip([grass, brush]) {
            *now = if target >= *now { target } else { target + (*now - target) * fall };
            // Silent once it is inaudible, so the mixer stops the loop.
            if *now < QUIET {
                *now = 0.0;
            }
        }
        let [grass, brush] = self.levels;
        let mut out = vec![
            Command::Loop { voice: Loop::Grass, gain: grass, pitch: pitch(GRASS_PITCH, speed) },
            Command::Loop { voice: Loop::Brush, gain: brush, pitch: pitch(BRUSH_PITCH, speed) },
        ];
        if std::mem::take(&mut self.fresh) {
            let pace = smoothstep(HIT_SPEED, speed);
            let new = touches.clone().filter(|t| !self.last.contains(&t.id));
            for t in new.take(HITS).filter(|_| pace > 0.0 && self.samples > 0) {
                let gain = HIT_GAIN * pace * if t.branches { 1.0 } else { BUSH_HIT * t.density };
                let heavy = if t.branches { 1.0 } else { t.density };
                let pitch = pitch(HIT_PITCH, speed) * (1.0 - 0.1 * heavy);
                out.push(Command::Hit { shot: Shot::Branch(self.hits % self.samples), gain, pitch, distance });
                self.hits += 1;
            }
            self.last = touches.map(|t| t.id).collect();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(id: u32, depth: f32, branches: bool) -> Option<Touch> {
        Some(Touch { id, depth, density: 1.0, branches })
    }

    fn loops(out: &[Command]) -> [f32; 2] {
        let gain = |voice| {
            out.iter()
                .find_map(|c| match *c {
                    Command::Loop { voice: v, gain, .. } if v == voice => Some(gain),
                    _ => None,
                })
                .unwrap()
        };
        [gain(Loop::Grass), gain(Loop::Brush)]
    }

    fn hits(out: &[Command]) -> Vec<(usize, f32)> {
        out.iter()
            .filter_map(|c| match *c {
                Command::Hit { shot: Shot::Branch(i), gain, .. } => Some((i, gain)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn without_a_verge_nothing_plays() {
        let mut verges = Verges::new(3);
        assert!(verges.step(20.0, 0.0, 10.0).is_empty());
        verges.set(None);
        assert!(verges.step(20.0, 0.0, 10.0).is_empty());
    }

    #[test]
    fn the_grass_swish_follows_the_wheels_on_grass_and_the_speed() {
        let mut verges = Verges::new(3);
        let grass = |verges: &mut Verges, grass: [f32; 4], speed: f32| {
            verges.set(Some(Verge { grass, ..Verge::default() }));
            loops(&verges.step(speed, 0.0, 10.0))[0]
        };
        assert_eq!(grass(&mut verges, [0.0; 4], 20.0), 0.0);
        let one = grass(&mut verges, [1.0, 0.0, 0.0, 0.0], 20.0);
        let two = grass(&mut verges, [1.0, 0.0, 1.0, 0.0], 20.0);
        let four = grass(&mut verges, [1.0; 4], 20.0);
        assert!((one - GRASS_GAIN * 0.5).abs() < 1e-6 && (four - GRASS_GAIN).abs() < 1e-6, "{one} {four}");
        assert!(one < two && two < four);
        // A wheel straddling the edge counts its share.
        assert!((grass(&mut verges, [0.5, 0.5, 0.0, 0.0], 20.0) - one).abs() < 1e-6);
        // A wheel skipping off the grass for a step fades instead of cutting.
        verges.set(Some(Verge::default()));
        assert!(loops(&verges.step(20.0, 0.0, 0.007))[0] > 0.9 * one);
        // Silent at a standstill, rising with speed.
        assert_eq!(grass(&mut verges, [1.0; 4], 0.0), 0.0);
        assert!(grass(&mut verges, [1.0; 4], 8.0) < four);
    }

    #[test]
    fn a_bush_brushes_a_branch_hits_and_open_road_gives_nothing() {
        let mut verges = Verges::new(3);
        // Open road: silent loops, no hits.
        verges.set(Some(Verge::default()));
        let out = verges.step(20.0, 0.0, 10.0);
        assert_eq!(loops(&out), [0.0, 0.0]);
        assert!(hits(&out).is_empty());
        // Into a bush: the brush by depth, and a soft hit on entering it.
        let bush =
            Verge { touches: [touch(7, 0.25, false), None, None, None, None, None, None, None], ..Verge::default() };
        verges.set(Some(bush));
        let out = verges.step(20.0, 0.0, 10.0);
        assert!((loops(&out)[1] - BRUSH_GAIN * 0.5).abs() < 1e-6, "{out:?}");
        assert_eq!(hits(&out), vec![(0, HIT_GAIN * BUSH_HIT)]);
        // Still in it: the brush goes on, no new hit (steps without a new frame neither).
        verges.set(Some(bush));
        assert!(hits(&verges.step(20.0, 0.0, 10.0)).is_empty());
        assert!(hits(&verges.step(20.0, 0.0, 10.0)).is_empty());
        // A bendable tree's branches: a full hit, the next sample in turn.
        let mut tree = bush;
        tree.touches[1] = touch(9, 0.1, true);
        verges.set(Some(tree));
        let out = verges.step(20.0, 0.0, 10.0);
        assert_eq!(hits(&out), vec![(1, HIT_GAIN)]);
        // The brush is capped at 1.
        let deep = Verge { touches: [touch(1, 2.0, false); TOUCHES], ..Verge::default() };
        verges.set(Some(deep));
        assert!((loops(&verges.step(20.0, 0.0, 10.0))[1] - BRUSH_GAIN).abs() < 1e-6);
        // Back on the road, then a slow touch: no hit.
        verges.set(Some(Verge::default()));
        verges.step(1.0, 0.0, 10.0);
        verges.set(Some(tree));
        assert!(hits(&verges.step(1.0, 0.0, 10.0)).is_empty());
    }
}
