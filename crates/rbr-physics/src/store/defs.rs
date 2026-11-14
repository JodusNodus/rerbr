//! The definitions a step reads, held read only (`notes/physics.md` "Store seam"): [`Defs`], the typed definitions
//! the car's typed subsystems read (the wheels', the struts' and the tyres' setups), lifted from the image before each
//! step with the car's state and never lowered. A step does not write them; setup adoption, a respawn and a restart
//! write them between steps, so each step lifts them anew. Each is keyed by its address (the handle its readers hold),
//! and a definition or a word the step does not hold reads the image. The load builds the definitions on the image.
//!
//! The words a step writes stay out, on the image: the wheel definitions' `+0x880..+0x900` (the frames `M3A8A0`
//! writes) and the tyre setups' by-load tables `+0x204..+0x284` (NGP's step wrapper scales them around `Step`). With the `poison` feature each typed answer is checked against the image's ([`checked`]) and the pure step
//! watches the held bytes ([`Defs::watched`], `Mem::watch_writes`): a write into them during a step fails the run.

use std::ops::Range;

use crate::car_body::store::CarStore;
use crate::mem::Mem;
use crate::spring_damper::layout::{self as strut, ControllerObj, DEFINITION_BYTES};
use crate::spring_damper::Definition;
use crate::store::Object;
use crate::tyre::layout::{self as tyre, TyreDef};
use crate::wheel::layout::{self as wheel, WheelDef};

/// A held definition and its address.
#[derive(Clone, Debug, PartialEq)]
pub struct Held<T> {
    pub at: u32,
    pub def: T,
}

/// Up to one definition of a kind per wheel slot: the slot's, unless an earlier slot holds it.
pub type PerWheel<T> = [Option<Held<T>>; 4];

/// The definitions of a car's wheels, struts and tyres, each once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Defs {
    pub wheels: PerWheel<WheelDef>,
    pub struts: PerWheel<Definition>,
    pub tyres: PerWheel<TyreDef>,
}

fn find<T>(list: &PerWheel<T>, at: u32) -> Option<&T> {
    list.iter().flatten().find(|held| held.at == at).map(|held| &held.def)
}

/// Lifts the definition at `at` into slot `k` of `list` unless an earlier slot holds it (or `at` is 0).
fn add<T>(list: &mut PerWheel<T>, k: usize, at: u32, lift: impl FnOnce(u32) -> T) {
    if at != 0 && find(list, at).is_none() {
        list[k] = Some(Held { at, def: lift(at) });
    }
}

impl Defs {
    /// The definitions of the car at `car`'s wheels, their struts and their tyres, from the image.
    pub fn lift(mem: &Mem, car: u32) -> Self {
        let mut defs = Defs::default();
        for (k, at) in mem.wheel_pointers(car).into_iter().enumerate().filter(|&(_, at)| at != 0) {
            add(&mut defs.wheels, k, mem.u32(at + wheel::DEF), |def| WheelDef::read(mem, def));
            let strut_def = ControllerObj::definition.load(mem, at + wheel::STRUT);
            add(&mut defs.struts, k, strut_def, |def| strut::lift_definition(mem, def));
            add(&mut defs.tyres, k, mem.u32(at + wheel::TYRE_DEF), |def| TyreDef::lift(mem, def));
        }
        defs
    }

    /// The wheel definition at `at`, if held.
    #[inline]
    pub fn wheel(&self, at: u32) -> Option<&WheelDef> {
        find(&self.wheels, at)
    }

    /// The strut definition at `at`, if held.
    #[inline]
    pub fn strut(&self, at: u32) -> Option<&Definition> {
        find(&self.struts, at)
    }

    /// The tyre setup at `at`, if held.
    #[inline]
    pub fn tyre(&self, at: u32) -> Option<&TyreDef> {
        find(&self.tyres, at)
    }

    /// The byte ranges the held definitions stand for, named: what a step must not write.
    pub fn watched(&self) -> Vec<(Range<u32>, String)> {
        let at = |base: u32, r: &Range<u32>| base + r.start..base + r.end;
        let mut out = Vec::new();
        for held in self.wheels.iter().flatten() {
            out.extend(
                WheelDef::FIELDS
                    .iter()
                    .map(|f| (held.at + f.at..held.at + f.at + f.len, format!("wheel def {:#x} {}", held.at, f.name))),
            );
        }
        out.extend(
            self.struts.iter().flatten().map(|h| (at(h.at, &DEFINITION_BYTES), format!("strut def {:#x}", h.at))),
        );
        for h in self.tyres.iter().flatten() {
            out.extend(tyre::DEF_BYTES.iter().map(|r| (at(h.at, r), format!("tyre setup {:#x}", h.at))));
        }
        out
    }
}

/// `typed`, a held definition's answer; with the `poison` feature checked against the image's (`image`).
#[inline(always)]
pub fn checked<T: PartialEq + std::fmt::Debug>(typed: T, _image: impl FnOnce() -> T) -> T {
    #[cfg(feature = "poison")]
    {
        let image = _image();
        assert_eq!(typed, image, "a typed definition's answer differs from the image's");
    }
    typed
}

/// The definitions of the car at `car` lifted from `mem`, each answer against the image's: the wheel definitions' words,
/// the struts' definitions and the tyres' setups with their tables. The bytes compared, or the first that differs.
pub fn differential(mem: &Mem, car: u32) -> Result<usize, String> {
    let defs = Defs::lift(mem, car);
    let mut bytes = 0;
    for held in defs.wheels.iter().flatten() {
        bytes += held.def.differential(mem, held.at)?;
    }
    for held in defs.struts.iter().flatten() {
        if held.def != strut::definition(mem, held.at) {
            return Err(format!("strut def at {:#010x}: the typed definition differs from the image's", held.at));
        }
        bytes += (DEFINITION_BYTES.end - DEFINITION_BYTES.start) as usize;
    }
    for held in defs.tyres.iter().flatten() {
        bytes += held.def.differential(mem, held.at)?;
    }
    Ok(bytes)
}
