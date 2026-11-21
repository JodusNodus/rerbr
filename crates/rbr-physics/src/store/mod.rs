//! The storage seam of the physics (Phase B, `notes/physics.md` "Store seam").
//!
//! Each subsystem declares a store trait (`TyreStore`, `StrutStore`, `VcuStore`, ...) whose methods are its layout's
//! `load_*`/`store_*` groups. [`Mem`] implements them with the layout bodies (the Mem store), but for the typed
//! subsystems (the struts, the VCU, the tyres), whose stores are views over their typed state ([`typed::CarTyped`]); the sequence
//! functions are generic over the trait.
//!
//! [`Raw`] is the word access every store has: on [`Mem`] it is the inherent accessors, elsewhere the inner image's.
//! A sequence function reaches what its subsystem's trait does not name yet (the caller's out pointers, another
//! subsystem's words) through it.
//!
//! [`object!`](crate::object) tables an object's fields once (`layout.rs`): it generates the typed struct, a [`Field`]
//! per field (the Mem accessor), and the [`Object`] impl with `lift`, `lower` and the owned byte ranges the round-trip
//! test, the twin run and the poison mode use.

use crate::mem::Mem;

pub mod defs;
mod layout;
pub mod typed;
pub use layout::{
    car_objects, check_table, first_difference, word_fields, Field, FieldInfo, Object, Real, Tabled, Value,
};

/// Raw word access on a store's image.
pub trait Raw {
    fn mem(&self) -> &Mem;
    fn mem_mut(&mut self) -> &mut Mem;

    #[inline]
    fn u8(&self, at: u32) -> u8 {
        self.mem().u8(at)
    }
    #[inline]
    fn u32(&self, at: u32) -> u32 {
        self.mem().u32(at)
    }
    #[inline]
    fn f32(&self, at: u32) -> f32 {
        self.mem().f32(at)
    }
    #[inline]
    fn u32s<const N: usize>(&self, at: u32) -> [u32; N] {
        self.mem().u32s(at)
    }
    #[inline]
    fn f32s<const N: usize>(&self, at: u32) -> [f32; N] {
        self.mem().f32s(at)
    }
    #[inline]
    fn set_u8(&mut self, at: u32, value: u8) {
        self.mem_mut().set_u8(at, value)
    }
    #[inline]
    fn set_u32(&mut self, at: u32, value: u32) {
        self.mem_mut().set_u32(at, value)
    }
    #[inline]
    fn set_f32(&mut self, at: u32, value: f32) {
        self.mem_mut().set_f32(at, value)
    }
}

impl Raw for Mem {
    #[inline]
    fn mem(&self) -> &Mem {
        self
    }
    #[inline]
    fn mem_mut(&mut self) -> &mut Mem {
        self
    }
}
