//! The stage's reads on the image: the collision-system selection, and `Mem`'s [`StageStore`].

use super::DRIVE_LINE;
use crate::engine_driveline::layout::{entry_at, COUNT_AT};
use crate::entities::layout::{SLOT_SIZE, TABLE_COUNT, TABLE_SELF};
use crate::entities::store::SlotLookup;
use crate::math::vec::Vec4;
use crate::mem::Mem;
use crate::ngp::layout::material::SLOT_DATA;
use crate::ngp::material::SIDE;
use crate::stage::store::StageStore;

/// Readers of NGP's first collision-system copy (in the original, the operands NGP patches at 0x51bda4, 0x51c726,
/// 0x4e54e8, 0x529d5a and 0x53d5e0).
#[derive(Clone, Copy)]
pub enum CollisionQuery {
    EntityCollision,
    GroundProbe,
    CarProbe,
    BodyProbe,
    WheelProbe,
}

impl CollisionQuery {
    /// The collision system the query's reader uses: NGP's first copy, which the boot's runtime holds, else 1.02's.
    pub fn system(self, mem: &Mem) -> u32 {
        match mem.ngp_runtime() {
            Some(runtime) => runtime.collision_systems[0],
            None => mem.u32(super::COLLISION_SYSTEM),
        }
    }
}

impl StageStore for Mem {
    fn drive_line(&self) -> u32 {
        self.u32(DRIVE_LINE)
    }

    fn line_count(&self, line: u32) -> u32 {
        self.u32(line + COUNT_AT)
    }

    fn line_position(&self, line: u32, k: u32) -> Vec4 {
        Vec4::load(self, entry_at(line, k))
    }

    fn line_tangent(&self, line: u32, k: u32) -> Vec4 {
        Vec4::load(self, entry_at(line, k).wrapping_add(0x10))
    }

    fn line_distance(&self, line: u32, k: u32) -> f32 {
        self.f32(entry_at(line, k).wrapping_add(0x20))
    }

    fn line_tag(&self, line: u32, k: u32) -> u32 {
        self.u32(entry_at(line, k).wrapping_add(0x24))
    }

    fn material_slot(&self, table: u32, index: u8) -> SlotLookup {
        let slots = self.u32(table + TABLE_SELF);
        if slots == 0 {
            return SlotLookup::NoTable;
        }
        if index as u32 >= self.u32(table + TABLE_COUNT) {
            return SlotLookup::OutOfRange;
        }
        SlotLookup::At(slots + SLOT_SIZE * index as u32)
    }

    fn material_texel(&self, slot: u32, column: u32, row: u32) -> u32 {
        let data = self.u32(slot + SLOT_DATA);
        if data == 0 {
            return 0;
        }
        self.u8(data + row * SIDE + column) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ngp::layout::runtime::Runtime;

    #[test]
    fn boot_queries_use_the_same_copy_without_instruction_bytes() {
        let mut mem = Mem::new();
        mem.set_ngp_runtime(Runtime::new([0x1000, 0x2000, 0x3000], 1));
        for query in [
            CollisionQuery::EntityCollision,
            CollisionQuery::GroundProbe,
            CollisionQuery::CarProbe,
            CollisionQuery::BodyProbe,
            CollisionQuery::WheelProbe,
        ] {
            assert_eq!(query.system(&mem), 0x1000);
        }
        assert!(mem.try_read(0x53d5e0, 4).is_none());
    }
}
