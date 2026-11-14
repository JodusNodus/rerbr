//! The entities' small slots: the `CEntity` hooks and flags, `CDummyObject`'s box and span test, the exclusion set
//! and `CCar::Clear480`.

use crate::entities::exclusion::{Excluder, Members};
use crate::mem::Mem;

/// A hook that answers 0 (`CEntity` 0x519ac0: `xor eax, eax; ret`).
pub fn zero_hook() -> u32 {
    0
}

/// The entity's box overlap test (slot 51 at +0xcc, named by use: the detectors' overlap test, see
/// `physics_core::layout::detector_hosts`): `CEntity::VfCC` (0x519e90, ret 8) answers 1 whatever its two arguments are.
pub fn overlaps_box() -> u32 {
    1
}

/// The entity's penetration response (slot 52 at +0xd0, named by use: the detectors' response call):
/// `CEntity::VfD0` (0x519ea0, ret 12) answers 0 whatever its three arguments are.
pub fn penetration_response() -> u32 {
    0
}

/// The exclusion set (`+0x2c`, `[count, members...]`) and the parent (`+0x44`) of a CEntity (`ExclusionTest`).
pub(super) const EXCL_SET: u32 = 0x2c;
pub(super) const PARENT: u32 = 0x44;

/// The entity at `at` as the exclusion test sees it.
pub fn load_excluder(mem: &Mem, at: u32) -> Excluder<SetInMem<'_>> {
    let set = mem.u32(at + EXCL_SET);
    let members = (set != 0).then(|| SetInMem { mem, set, count: mem.u32(set) });
    Excluder { handle: at, parent: mem.u32(at + PARENT), members }
}

/// An exclusion set read where it is (`[count, members...]` at `set`) instead of copied: the step tests dozens.
pub struct SetInMem<'a> {
    mem: &'a Mem,
    set: u32,
    count: u32,
}

impl Members for SetInMem<'_> {
    fn count(&self) -> u32 {
        self.count
    }

    fn holds(&self, entity: u32) -> bool {
        (1..=self.count).any(|i| self.mem.u32(self.set + 4 * i) == entity)
    }
}

/// `CDummyObject::VfCC`'s words at its frame base `frame`: the two fetched origins, and the entry stack pointers of the
/// fetches and the group's hit handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DummySpanFrame {
    pub origin_a: u32,
    pub origin_b: u32,
    pub fetch_sp: u32,
    pub hit_sp: u32,
}

pub fn dummy_span_frame(frame: u32) -> DummySpanFrame {
    DummySpanFrame { origin_a: frame + 0x1c, origin_b: frame + 0xc, fetch_sp: frame - 0x10, hit_sp: frame - 0x8 }
}

/// The entry stack pointer of `CCar::Clear480`'s detach calls (`clear_car_bodies`, entered at `frame`).
pub fn clear_bodies_detach_sp(frame: u32) -> u32 {
    frame - 0x10
}
