//! Ours: the car meeting the verge, from what the renderer sees after each frame's step ([`Verge`]; 1.02 has no such
//! sounds): the bushes and bendable trees brushing and whipping the body and their branches slapping it. Pure: gains, pitches and
//! the hits to play per step, on the embedded recordings (`sounds/CREDITS.md`).
//!
//! - Patter (`Loop::Grass`): the stalks whipping the body. Grass under the wheels plays nothing (user: short grass
//!   does not whip a car, and the stages mark grass by firmness, not height).
//! - Brush (`Loop::Brush`): the rustle of the leaves, a steady hiss, at half the patter's gain ([`BRUSH_GAIN`]).
//! - Both loops follow the body's contacts with bushes and bendable trees, by how deep it is in each
//!   ([`BRUSH_DEPTH`]) times its density, and the bushes under the wheels ([`Verge::brush`], [`BRUSH_WHEELS`]), at
//!   most 1; the speed ([`LOOP_SPEED`]: a slow push through a bush is quiet); and each new contact's burst, which
//!   falls to the drag along the body ([`DRAG`], [`BURST`]), so a hedge is heard bush by bush.
//! - Branch hits (`Shot::Branch`): the body meeting a bendable tree, or (softer, [`BUSH_HIT`]) a bush, it was not
//!   touching at the last frame, by the speed ([`HIT_SPEED`]); the samples are taken in turn, not from the random
//!   numbers, so the other sounds stay as without the verge.
//!
//! All three play with the tyres' distance gain on the side the vegetation stands on from the camera ([`Touch::at`];
//! bushes under the wheels alone at the car). In the car the loops come through a low-pass of their own and the hits
//! ring the shell as the stones do (`crate::mixer`). Without a [`Verge`] nothing is commanded, so the sound is as
//! without the verge, sample for sample.

use crate::voice::{Command, Loop, Shot};

/// The most contacts a frame carries (the deepest).
pub const TOUCHES: usize = 8;

/// A scenery placement the body is in: its index in the stage's roadside (`rbr_render::roadside`), where it stands
/// (physics frame, at the body's height), how far the body reaches into it, m, how dense it is (0 to 1), and whether
/// it is a bendable tree's branches rather than a bush.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Touch {
    pub id: u32,
    pub at: [f32; 3],
    pub depth: f32,
    pub density: f32,
    pub branches: bool,
}

/// The car at the verge for a frame: the vegetation under the wheels and the vegetation the body is in.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Verge {
    /// Each wheel's share in bushes a stage marks in the ground under it, times their density (0 to 1).
    pub brush: [f32; 4],
    pub touches: [Option<Touch>; TOUCHES],
}

/// The loops: at [`PATTER_GAIN`] and [`BRUSH_GAIN`] with the contacts full in a new contact's burst, rising with the
/// speed over [`LOOP_SPEED`] m/s (7 to 108 km/h); their pitches from the first of [`PATTER_PITCH`] and
/// [`BRUSH_PITCH`] at standstill to the second at [`PITCH_TOP`] m/s.
const PATTER_GAIN: f32 = 2.0;
const BRUSH_GAIN: f32 = 1.0;
const LOOP_SPEED: [f32; 2] = [2.0, 30.0];
const PATTER_PITCH: [f32; 2] = [0.8, 1.2];
const BRUSH_PITCH: [f32; 2] = [0.85, 1.15];
/// A placement adds its density to the contacts, full once the body is [`BRUSH_DEPTH`] m into it; at most 1.
const BRUSH_DEPTH: f32 = 0.5;
/// The wheels in bushes ([`Verge::brush`]) that make the contacts full.
const BRUSH_WHEELS: f32 = 2.0;
/// The contacts follow a rise at once and fall with this time constant, s, so the body clearing a bush for a frame
/// does not chop the loops.
const RELEASE: f32 = 0.15;
/// The loops' level while the body drags along what it is in, of a new contact's burst (its density, 1 for a
/// tree), which falls to it with [`BURST`] s.
const DRAG: f32 = 0.4;
const BURST: f32 = 0.1;
const QUIET: f32 = 1e-4;
/// The top of the loops' pitch rise, m/s (108 km/h).
const PITCH_TOP: f32 = 30.0;
/// A new contact's hit: [`HIT_GAIN`] by the speed over [`HIT_SPEED`] m/s (7 to 72 km/h), a bush's times its density
/// and [`BUSH_HIT`]; none below the first speed; at most [`HITS`] a frame.
const HIT_GAIN: f32 = 0.5;
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
    /// The contacts' level ([`RELEASE`]) and the last new contact's burst ([`BURST`]).
    contact: f32,
    burst: f32,
    /// The side the loops play on: the body's contacts', kept while they fade; `None` at the car.
    side: Option<f32>,
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
            (self.contact, self.burst, self.side) = (0.0, 0.0, None);
        }
        self.verge = verge;
        self.fresh = true;
    }

    /// The commands for one step of `dt` seconds at car speed `speed` m/s, the car `distance` m from the listener, who
    /// hears a point on side `side` (-1 left to 1 right, `crate::world::pan`).
    pub fn step(&mut self, speed: f32, distance: f32, dt: f32, side: impl Fn([f32; 3]) -> f32) -> Vec<Command> {
        let Some(verge) = self.verge else { return Vec::new() };
        let speed = speed.abs();
        let touches = verge.touches.iter().flatten();
        let weight = |t: &Touch| t.density * (t.depth / BRUSH_DEPTH).clamp(0.0, 1.0);
        let body = touches.clone().map(weight).sum::<f32>();
        let contact = (body + verge.brush.iter().map(|b| b.clamp(0.0, 1.0)).sum::<f32>() / BRUSH_WHEELS).min(1.0);
        if body > 0.0 {
            self.side = Some(touches.clone().map(|t| weight(t) * side(t.at)).sum::<f32>() / body);
        } else if contact > 0.0 || self.contact == 0.0 {
            self.side = None;
        }
        self.contact =
            if contact >= self.contact { contact } else { contact + (self.contact - contact) * (-dt / RELEASE).exp() };
        // Silent once it is inaudible, so the mixer stops the loops.
        if self.contact < QUIET {
            self.contact = 0.0;
        }
        self.burst *= (-dt / BURST).exp();
        let mut hits = Vec::new();
        if std::mem::take(&mut self.fresh) {
            let pace = smoothstep(HIT_SPEED, speed);
            for t in touches.clone().filter(|t| !self.last.contains(&t.id)) {
                let heavy = if t.branches { 1.0 } else { t.density };
                self.burst = self.burst.max(heavy);
                if hits.len() < HITS && pace > 0.0 && self.samples > 0 {
                    let gain = HIT_GAIN * pace * if t.branches { 1.0 } else { BUSH_HIT * t.density };
                    let pitch = pitch(HIT_PITCH, speed) * (1.0 - 0.1 * heavy);
                    let shot = Shot::Branch(self.hits % self.samples);
                    hits.push(Command::HitAt { shot, gain, pitch, distance, pan: side(t.at) });
                    self.hits += 1;
                }
            }
            self.last = touches.map(|t| t.id).collect();
        }
        let level = self.contact * (DRAG + (1.0 - DRAG) * self.burst) * smoothstep(LOOP_SPEED, speed);
        let mut out = vec![
            Command::Loop { voice: Loop::Grass, gain: PATTER_GAIN * level, pitch: pitch(PATTER_PITCH, speed) },
            Command::Loop { voice: Loop::Brush, gain: BRUSH_GAIN * level, pitch: pitch(BRUSH_PITCH, speed) },
        ];
        let loops = [Loop::Grass, Loop::Brush];
        out.extend(self.side.into_iter().flat_map(|pan| loops.map(|voice| Command::Pan { voice, pan })));
        out.extend(hits);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(id: u32, depth: f32, branches: bool) -> Option<Touch> {
        Some(Touch { id, at: [0.0; 3], depth, density: 1.0, branches })
    }

    /// The listener's side of a point: its x.
    fn side(at: [f32; 3]) -> f32 {
        at[0]
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
                Command::HitAt { shot: Shot::Branch(i), gain, .. } => Some((i, gain)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn without_a_verge_nothing_plays() {
        let mut verges = Verges::new(3);
        assert!(verges.step(20.0, 0.0, 10.0, side).is_empty());
        verges.set(None);
        assert!(verges.step(20.0, 0.0, 10.0, side).is_empty());
    }

    #[test]
    fn the_patter_follows_the_brush_contacts_and_the_speed() {
        let mut verges = Verges::new(3);
        let mut patter = |brush: [f32; 4], speed: f32| {
            verges.set(Some(Verge { brush, ..Verge::default() }));
            loops(&verges.step(speed, 0.0, 10.0, side))[0]
        };
        assert_eq!(patter([0.0; 4], 30.0), 0.0);
        // Bushes under the wheels drag: no new contact, no burst.
        let one = patter([1.0, 0.0, 0.0, 0.0], 30.0);
        let two = patter([1.0, 0.0, 1.0, 0.0], 30.0);
        assert!(
            (one - PATTER_GAIN * DRAG * 0.5).abs() < 1e-6 && (two - PATTER_GAIN * DRAG).abs() < 1e-6,
            "{one} {two}"
        );
        // Silent at a standstill, rising with speed.
        assert_eq!(patter([1.0; 4], 0.0), 0.0);
        assert!(patter([1.0; 4], 8.0) < two);
    }

    #[test]
    fn a_bush_brushes_a_branch_hits_and_open_road_gives_nothing() {
        let mut verges = Verges::new(3);
        // Open road: silent loops, no hits.
        verges.set(Some(Verge::default()));
        let out = verges.step(20.0, 0.0, 10.0, side);
        assert_eq!(loops(&out), [0.0, 0.0]);
        assert!(hits(&out).is_empty());
        // Into a bush: the brush by depth at the burst, and a soft hit on entering it.
        let bush =
            Verge { touches: [touch(7, 0.25, false), None, None, None, None, None, None, None], ..Verge::default() };
        verges.set(Some(bush));
        let out = verges.step(30.0, 0.0, 10.0, side);
        assert!((loops(&out)[1] - BRUSH_GAIN * 0.5).abs() < 1e-6, "{out:?}");
        assert_eq!(hits(&out), vec![(0, HIT_GAIN * BUSH_HIT)]);
        // Still in it: the burst falls toward the drag within a frame, no new hit (steps without a new frame neither).
        verges.set(Some(bush));
        let out = verges.step(30.0, 0.0, 0.007, side);
        assert!(loops(&out)[1] < BRUSH_GAIN * 0.5 && loops(&out)[1] > BRUSH_GAIN * 0.5 * DRAG, "{out:?}");
        assert!(hits(&out).is_empty());
        let out = verges.step(30.0, 0.0, 10.0, side);
        assert!((loops(&out)[1] - BRUSH_GAIN * 0.5 * DRAG).abs() < 1e-6, "{out:?}");
        assert!(hits(&out).is_empty());
        // A bendable tree's branches: a full hit, the next sample in turn.
        let mut tree = bush;
        tree.touches[1] = touch(9, 0.1, true);
        verges.set(Some(tree));
        let out = verges.step(30.0, 0.0, 10.0, side);
        assert_eq!(hits(&out), vec![(1, HIT_GAIN)]);
        // Bushes under the wheels brush too: two wheels in dense ones are full, no hit; quieter when slower.
        verges.set(Some(Verge { brush: [1.0, 0.5, 0.0, 0.0], ..Verge::default() }));
        let under = verges.step(30.0, 0.0, 10.0, side);
        assert!((loops(&under)[1] - BRUSH_GAIN * 0.75 * DRAG).abs() < 1e-6, "{under:?}");
        assert!(!under.iter().any(|c| matches!(c, Command::Shot { .. })));
        let slow = loops(&verges.step(11.0, 0.0, 10.0, side))[1];
        assert!(slow > 0.0 && slow < 0.3 * loops(&under)[1], "{slow}");
        // The brush is capped at 1.
        let deep = Verge { touches: [touch(1, 2.0, false); TOUCHES], ..Verge::default() };
        verges.set(Some(deep));
        assert!((loops(&verges.step(30.0, 0.0, 10.0, side))[1] - BRUSH_GAIN).abs() < 1e-6);
        // Back on the road, then a slow touch: no hit.
        verges.set(Some(Verge::default()));
        verges.step(1.0, 0.0, 10.0, side);
        verges.set(Some(tree));
        assert!(hits(&verges.step(1.0, 0.0, 10.0, side)).is_empty());
    }

    #[test]
    fn the_verge_plays_on_the_side_the_vegetation_stands_on() {
        let mut verges = Verges::new(3);
        let tree = |id, x, depth| Some(Touch { at: [x, 0.0, 0.0], ..touch(id, depth, true).unwrap() });
        let pans = |out: &[Command]| -> Vec<f32> {
            out.iter()
                .filter_map(|c| match *c {
                    Command::Pan { pan, .. } | Command::HitAt { pan, .. } => Some(pan),
                    _ => None,
                })
                .collect()
        };
        // A tree on the right: both loops and its hit there.
        let mut verge = Verge::default();
        verge.touches[0] = tree(1, 0.8, 0.1);
        verges.set(Some(verge));
        let right = pans(&verges.step(30.0, 0.0, 0.007, side));
        assert!(right.len() == 3 && right.iter().all(|p| (p - 0.8).abs() < 1e-6), "{right:?}");
        // One on the left as well, twice as deep: the loops between them by the depths, its hit on the left.
        verge.touches[1] = tree(2, -0.5, 0.2);
        verges.set(Some(verge));
        let both = pans(&verges.step(30.0, 0.0, 0.007, side));
        let mean = (0.2 * 0.8 + 0.4 * -0.5) / 0.6;
        assert!((both[0] - mean).abs() < 1e-6 && both[0] == both[1] && both[2] == -0.5, "{both:?}");
        // Bushes under the wheels alone play at the car.
        verges.set(Some(Verge { brush: [1.0; 4], ..Verge::default() }));
        assert!(pans(&verges.step(30.0, 0.0, 0.007, side)).is_empty());
        // Back on the road the loops fade where they were, then play at the car again.
        verges.set(Some(verge));
        verges.step(30.0, 0.0, 0.007, side);
        verges.set(Some(Verge::default()));
        assert_eq!(pans(&verges.step(30.0, 0.0, 0.007, side)), [both[0]; 2]);
        verges.step(30.0, 0.0, 10.0, side);
        assert!(pans(&verges.step(30.0, 0.0, 0.007, side)).is_empty());
    }
}
