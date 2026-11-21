//! The definitions a step reads, held read only (`notes/physics.md` "Store seam"): [`Defs`], the typed definitions
//! the car's typed subsystems read (the wheels', the struts' and the tyres' setups; the car's, the VCU's, the drive's
//! and the engine's), lifted from the image before each step with the car's state and never lowered. A step does not write
//! them; setup adoption, a respawn and a restart write them between steps, so each step lifts them anew. Each is keyed
//! by its address (the handle its readers hold), and a definition or a word the step does not hold reads the image.
//! The load builds the definitions on the image.
//!
//! The words a step writes stay out, on the image: the wheel definitions' `+0x880..+0x900` (the frames `M3A8A0`
//! writes) and the tyre setups' by-load tables `+0x204..+0x284` (NGP's step wrapper scales them around `Step`). With
//! the `poison` feature each typed answer is checked against the image's ([`checked`]) and the step watches the
//! held bytes ([`Defs::watched`], `Mem::watch_writes`): a write into them during a step fails the run.

use std::ops::Range;

use crate::car_body::layout::CarDef;
use crate::car_body::store::CarStore;
use crate::engine_driveline::layout::{
    DriveDefinition, DriveObjects, EngineDefinition, EngineObjects, DRIVE_DEF_BYTES, ENGINE_DEF_BYTES,
};
use crate::mem::Mem;
use crate::physics_core::layout::physics_global;
use crate::spring_damper::layout::{self as strut, ControllerObj, DEFINITION_BYTES};
use crate::spring_damper::Definition;
use crate::store::Object;
use crate::tyre::layout::{self as tyre, TyreDef};
use crate::vcu::layout as vcu;
use crate::wheel::layout::{self as wheel, WheelDef};

/// The physics object's definition pointer (`+0x24`) and the definition's flags word (`+0x160cc`): the words
/// `physics_core::layout::def_flags` reads.
const PHYSICS_DEF: u32 = 0x24;
const DEF_FLAGS: u32 = 0x160CC;

/// A held definition and its address.
#[derive(Clone, Debug, PartialEq)]
pub struct Held<T> {
    pub at: u32,
    pub def: T,
}

/// Up to one definition of a kind per wheel slot: the slot's, unless an earlier slot holds it.
pub type PerWheel<T> = [Option<Held<T>>; 4];

/// The definitions of a car's wheels, struts and tyres, each once, and its own, its VCU's, its drive's and its engine's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Defs {
    pub wheels: PerWheel<WheelDef>,
    pub struts: PerWheel<Definition>,
    pub tyres: PerWheel<TyreDef>,
    pub car: Option<Held<CarDef>>,
    pub vcu: Option<Held<vcu::Def>>,
    pub drive: Option<Held<DriveDefinition>>,
    /// The engine's definition (the words its ports read, served at their addresses by the engine's view).
    pub engine: Option<Held<EngineDefinition>>,
    /// The physics definition's flags word, which the drive's workers read their gear guard bit from (held in
    /// [`DriveDefinition::worker`]): its address, watched with the definitions.
    pub flags_at: Option<u32>,
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
    /// The definitions of the car at `car` (its wheels', their struts' and tyres', its own, its VCU's, its drive's and its
    /// engine's), from the image.
    pub fn lift(mem: &Mem, car: u32) -> Self {
        let mut defs = Defs::default();
        for (k, at) in mem.wheel_pointers(car).into_iter().enumerate().filter(|&(_, at)| at != 0) {
            add(&mut defs.wheels, k, mem.u32(at + wheel::DEF), |def| WheelDef::read(mem, def));
            let strut_def = ControllerObj::definition.load(mem, at + wheel::STRUT);
            add(&mut defs.struts, k, strut_def, |def| strut::lift_definition(mem, def));
            add(&mut defs.tyres, k, mem.u32(at + wheel::TYRE_DEF), |def| TyreDef::lift(mem, def));
        }
        let def = mem.def_address(car);
        defs.car = (def != 0).then(|| Held { at: def, def: CarDef::lift(mem, def) });
        let unit = vcu::ScratchObj::def.load(mem, crate::car_body::layout::scratch_at(car));
        defs.vcu = (unit != 0).then(|| Held { at: unit, def: vcu::def(mem, unit) });
        let drive = DriveObjects::at(mem, crate::car_body::layout::drive_at(car)).def;
        if drive != 0 {
            defs.drive = Some(Held { at: drive, def: DriveDefinition::lift(mem, drive) });
            defs.flags_at = Some(mem.u32(physics_global(mem) + PHYSICS_DEF) + DEF_FLAGS);
        }
        let engine = EngineObjects::at(mem, crate::car_body::layout::engine_at(car)).def;
        defs.engine = (engine != 0).then(|| Held { at: engine, def: EngineDefinition::lift(mem, engine) });
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

    /// The car's definition at `at`, if held.
    #[inline]
    pub fn car(&self, at: u32) -> Option<&CarDef> {
        self.car.as_ref().filter(|held| held.at == at).map(|held| &held.def)
    }

    /// The VCU's definition at `at`, if held.
    #[inline]
    pub fn vcu(&self, at: u32) -> Option<&vcu::Def> {
        self.vcu.as_ref().filter(|held| held.at == at).map(|held| &held.def)
    }

    /// The drive's definition at `at`, if held.
    #[inline]
    pub fn drive(&self, at: u32) -> Option<&DriveDefinition> {
        self.drive.as_ref().filter(|held| held.at == at).map(|held| &held.def)
    }

    /// The engine's definition at `at`, if held.
    #[inline]
    pub fn engine(&self, at: u32) -> Option<&EngineDefinition> {
        self.engine.as_ref().filter(|held| held.at == at).map(|held| &held.def)
    }

    /// The byte ranges the held definitions stand for, named: what a step must not write.
    pub fn watched(&self) -> Vec<(Range<u32>, String)> {
        let at = |base: u32, r: &Range<u32>| base + r.start..base + r.end;
        /// A tabled definition's fields.
        fn fields<'a, T: Object>(held: &'a Held<T>, kind: &'a str) -> impl Iterator<Item = (Range<u32>, String)> + 'a {
            let named = move |f: &crate::store::FieldInfo| format!("{kind} {:#x} {}", held.at, f.name);
            T::FIELDS.iter().map(move |f| (held.at + f.at..held.at + f.at + f.len, named(f)))
        }
        let mut out = Vec::new();
        for held in self.wheels.iter().flatten() {
            out.extend(fields(held, "wheel def"));
        }
        let struts = self.struts.iter().flatten();
        out.extend(struts.map(|h| (at(h.at, &DEFINITION_BYTES), format!("strut def {:#x}", h.at))));
        for h in self.tyres.iter().flatten() {
            out.extend(tyre::DEF_BYTES.iter().map(|r| (at(h.at, r), format!("tyre setup {:#x}", h.at))));
        }
        if let Some(held) = &self.car {
            out.extend(fields(held, "car def"));
        }
        out.extend(self.vcu.iter().map(|h| (at(h.at, &vcu::DEF_BYTES), format!("vcu def {:#x}", h.at))));
        out.extend(self.drive.iter().map(|h| (at(h.at, &DRIVE_DEF_BYTES), format!("drive def {:#x}", h.at))));
        out.extend(self.engine.iter().map(|h| (at(h.at, &ENGINE_DEF_BYTES), format!("engine def {:#x}", h.at))));
        out.extend(self.flags_at.map(|f| (f..f + 4, "physics def flags".to_string())));
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
