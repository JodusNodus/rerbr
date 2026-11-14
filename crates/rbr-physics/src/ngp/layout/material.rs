//! NGP's material sheet lookup (`0x1004df40`), which the exe's indexed lookup (`0x520240`, its call at `0x520265`)
//! calls in place of 1.02's slot lookup (`0x520010`, `entities::layout::materials::sample_slot` (git e5824ccc), removed
//! with 1.02's physics).

use crate::ngp::material::cell;
use crate::stage::store::StageStore;

/// The slot's data pointer.
pub(crate) const SLOT_DATA: u32 = 4;

/// `0x1004df40` (thiscall `(a, b)` on the slot): the byte at `(a, b)`, 0 when the slot has no data.
pub fn sample_slot<S: StageStore + ?Sized>(store: &S, slot: u32, a: f32, b: f32) -> u32 {
    store.material_texel(slot, cell(a), cell(b))
}
