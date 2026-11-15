//! `--twin mem` (Phase B): a second playback from the same image, stepped with the
//! same payloads, and compared with the first after every step: the car and wheel blocks (FRM1's bytes) and every
//! owned byte of the car's and the physics core's tabled objects (`rbr_physics::store::car_objects`,
//! `rbr_physics::physics_core::layout::core_objects`). Both images' tables are checked with
//! `lower(lift(image)) == image` first.
//!
//! `mem` runs the second side as the first, the Mem path against itself; `mem-direct` runs it with B1's Mem-direct
//! dispatch ([`rbr_hybrid::pure::Pure::direct`]); `typed` runs it with the typed subsystems (the struts, the VCU, the
//! drive, the engine, the tyres, the wheels, the car's own, the physics core) on the image between calls, against the first side's typed storage over each step
//! ([`rbr_hybrid::pure::Pure::typed`], the default). Both images' typed state is checked with `lower(lift(image)) == image` at the start too, and the typed
//! stage's queries against the image's (`stage::layout::differential`). Every solid the car has held so far is compared
//! too (`rigid_body::layout::Solid`'s owned bytes): a mounted body leaves the car's slots when it detaches.
//! `across` runs the second side with its typed state lifted and lowered around each step, against the first side's
//! held across steps (`Pure::typed_across_steps`, the default), the proof that holding it across steps changes nothing.
//!
//! Both sides' images are read through [`Pure::image`](rbr_hybrid::pure::Pure::image) (the typed state's words
//! lowered into them first).

use rbr_hybrid::pure::{frame_bytes, word_name, Playback, CAR_PTR};
use rbr_physics::mem::Mem;
use rbr_physics::rigid_body::layout::{CarSolids, Solid};
use rbr_physics::store::{self, FieldInfo, Object};
use rbr_recording::CONTROL_BYTES;

/// What the second side runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The Mem storage on the pure host, as the first side.
    Mem,
    /// The Mem storage on the pure host with the leaf calls of the entity walk, wheel, collision and rigid body run in
    /// place.
    MemDirect,
    /// The typed subsystems on the image between calls (each call lifts and lowers its own), against the first side's
    /// typed storage (held over each step or across steps).
    Typed,
    /// The typed storage held over each step (lifted before it, lowered after), against the first side's held across
    /// steps.
    Across,
}

impl Kind {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "mem" => Ok(Kind::Mem),
            "mem-direct" => Ok(Kind::MemDirect),
            "typed" => Ok(Kind::Typed),
            "across" => Ok(Kind::Across),
            _ => Err(format!("--twin {value}: mem, mem-direct, typed or across")),
        }
    }
}

pub struct Twin {
    kind: Kind,
    pub playback: Playback,
    /// Steps compared, and the first one that differed with what differed.
    steps: usize,
    first: Option<(usize, String)>,
    /// The address and name of every solid the first side's car has had, in the order they appeared.
    solids: Vec<(u32, &'static str, &'static [FieldInfo])>,
    /// The owned bytes of each side's tables, from the round trip at the start (or its failure).
    round_trip: [Result<usize, String>; 2],
}

impl Twin {
    /// The twin of `a` on `b`, a second playback of the same image; [`Kind::Across`] has `b` lift and lower around each
    /// step, against `a` holding its typed state across steps (the default).
    pub fn new(kind: Kind, a: &Playback, mut b: Playback) -> Result<Self, String> {
        if kind == Kind::Across && !(a.pure.typed && a.pure.typed_across_steps) {
            return Err("--twin across needs the first side's typed state held across steps".into());
        }
        b.pure.typed_across_steps = a.pure.typed_across_steps && kind != Kind::Across;
        b.pure.direct = kind == Kind::MemDirect;
        b.pure.typed = a.pure.typed && kind != Kind::Typed;
        let round_trip = [a.pure.image(), b.pure.image()].map(|mem| {
            let bytes = store::round_trip(mem, &objects(mem))?;
            Ok(bytes + store::typed::round_trip(mem, mem.u32(CAR_PTR))?)
        });
        for (side, r) in ["first", "second"].iter().zip(&round_trip) {
            eprintln!("twin: lower(lift) of the {side} side's image: {}", Self::round_trip_text(r));
        }
        match rbr_physics::stage::layout::differential(a.pure.image()) {
            Ok(bytes) => eprintln!("twin: the typed stage's queries equal the image's, {bytes} bytes"),
            Err(e) => eprintln!("twin: the typed stage's queries differ from the image's: {e}"),
        }
        let mut twin = Twin { kind, playback: b, steps: 0, first: None, solids: Vec::new(), round_trip };
        twin.compare(a, "the start");
        Ok(twin)
    }

    /// Steps the second side as the first was stepped, then compares them.
    pub fn step(&mut self, a: &Playback, payload: Option<&[u8; CONTROL_BYTES]>, substeps: u32) {
        let b = &self.playback;
        b.step(payload, substeps);
        // The host's queues the first side's caller drains.
        b.pure.take_tyre_terms();
        b.pure.take_outside_assistance();
        b.pure.take_retire();
        b.pure.take_wall_emits();
        self.steps += 1;
        let at = format!("step {}", self.steps);
        self.compare(a, &at);
    }

    /// Notes the first difference of the two images, at `when`.
    fn compare(&mut self, a: &Playback, when: &str) {
        if self.first.is_some() {
            return;
        }
        let (mem, other) = (a.pure.image(), self.playback.pure.image());
        for solid in CarSolids::lift(mem, mem.u32(CAR_PTR)).solids {
            if self.solids.iter().all(|(at, ..)| *at != solid.at) {
                self.solids.push((solid.at, solid.name, solid.own_fields()));
            }
        }
        let what = difference(mem, other).or_else(|| solid_difference(mem, other, &self.solids));
        if let Some(what) = what {
            self.first = Some((self.steps, format!("{when}: {what}")));
        }
    }

    fn round_trip_text(r: &Result<usize, String>) -> String {
        match r {
            Ok(bytes) => format!("{bytes} owned bytes equal"),
            Err(e) => e.clone(),
        }
    }

    /// The report's line.
    pub fn summary(&self) -> String {
        let round_trip = Self::round_trip_text;
        let compare = match &self.first {
            None => format!(
                "{} steps, car, wheels, solids ({}) and owned bytes equal after each",
                self.steps,
                self.solids.len()
            ),
            Some((_, what)) => format!("{} steps, first difference at {what}", self.steps),
        };
        format!(
            "twin {:?}: {compare}; lower(lift) first side {}, second side {}; {} calls run in place",
            self.kind,
            round_trip(&self.round_trip[0]),
            round_trip(&self.round_trip[1]),
            self.playback.pure.direct_runs()
        )
    }

    /// Whether every compare and both round trips held.
    pub fn equal(&self) -> bool {
        self.first.is_none() && self.round_trip.iter().all(Result::is_ok)
    }
}

/// The car's tabled objects and the physics core's.
fn objects(mem: &Mem) -> Vec<store::Tabled> {
    let mut objects = store::car_objects(mem, mem.u32(CAR_PTR));
    objects.extend(rbr_physics::physics_core::layout::core_objects(mem));
    objects
}

/// The first difference between `a` and `b`: a word of the car and wheel blocks, else an owned byte of a tabled object.
fn difference(a: &Mem, b: &Mem) -> Option<String> {
    let (x, y) = (frame_bytes(a), frame_bytes(b));
    if let Some(i) = x.chunks(4).zip(y.chunks(4)).position(|(p, q)| p != q) {
        let word = |bytes: &[u8]| u32::from_le_bytes(bytes[4 * i..4 * i + 4].try_into().unwrap());
        return Some(format!("{} {:#010x} vs {:#010x}", word_name(i), word(&x), word(&y)));
    }
    objects(a).iter().find_map(|o| {
        o.first_difference(a, b).map(|(field, offset)| format!("{} at {:#010x}: {field} (+{offset:#x})", o.name, o.at))
    })
}

/// The first owned byte of a solid in `solids` (its own words too: a hinged or mounted body's) that differs between
/// `a` and `b`.
fn solid_difference(a: &Mem, b: &Mem, solids: &[(u32, &str, &[FieldInfo])]) -> Option<String> {
    solids.iter().find_map(|&(at, name, own)| {
        Solid::FIELDS.iter().chain(own).find_map(|f| {
            let (x, y) = (a.read(at + f.at, f.len as usize), b.read(at + f.at, f.len as usize));
            let offset = x.iter().zip(&y).position(|(p, q)| p != q)?;
            Some(format!("{name} at {at:#010x}: {} (+{offset:#x})", f.name))
        })
    })
}
