//! The stage's typed state (`notes/physics.md` "Store seam"): what of the stage a step reads, the tables the load
//! builds and the step never writes, held once per loaded stage and never lowered. The drive line and the material
//! table with its sheets, each with its address (the handle its readers pass). Their image side (lift, the owned
//! bytes, the store over them) is `layout::typed`.

use crate::engine_driveline::drive_line::DriveLinePoint;
use crate::entities::material_table::MaterialSlot;
use crate::ngp::material::SIDE;

/// A drive line entry as the table holds it: the point, then two words no reader names.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineEntry {
    pub point: DriveLinePoint,
    pub spare: [u32; 2],
}

/// The drive line table: all its entries (past the count, as `SetDriveLine` leaves them) and the count.
#[derive(Clone, Debug, PartialEq)]
pub struct DriveLine {
    pub entries: Box<[LineEntry]>,
    pub count: u32,
}

impl DriveLine {
    /// Entry `k`'s point, `None` past the table.
    pub fn point(&self, k: u32) -> Option<&DriveLinePoint> {
        self.entries.get(k as usize).map(|e| &e.point)
    }
}

/// The material table: its slots (the width byte, the sheet's address), the slots' address and count words, and each
/// slot's sheet (16 by 16 bytes), held where the slot names one.
#[derive(Clone, Debug, PartialEq)]
pub struct Materials {
    pub slots: Box<[MaterialSlot]>,
    pub slots_at: u32,
    pub count: u32,
    pub sheets: Box<[Option<Box<[u8]>>]>,
}

impl Materials {
    /// Slot `k`'s byte at `column`, `row`: 0 without a sheet address, `None` when its sheet is not held.
    pub fn texel(&self, k: usize, column: u32, row: u32) -> Option<u32> {
        if self.slots[k].high == 0 {
            return Some(0);
        }
        let sheet = self.sheets[k].as_ref()?;
        Some(u32::from(sheet[(row * SIDE + column) as usize]))
    }
}

/// The stage a step reads: the drive line and the material table, each with its address, where the image has them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stage {
    pub line: Option<(u32, DriveLine)>,
    pub materials: Option<(u32, Materials)>,
}

impl Stage {
    /// The drive line at `line`, if it is the one held.
    pub fn line_at(&self, line: u32) -> Option<&DriveLine> {
        self.line.as_ref().filter(|(at, _)| *at == line).map(|(_, l)| l)
    }

    /// The material table at `table`, if it is the one held.
    pub fn materials_at(&self, table: u32) -> Option<&Materials> {
        self.materials.as_ref().filter(|(at, _)| *at == table).map(|(_, m)| m)
    }
}
