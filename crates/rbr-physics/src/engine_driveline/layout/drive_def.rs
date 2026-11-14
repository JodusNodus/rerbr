//! The drive's definition (`CDrive::CDef`, the drive's `+4`) as a step reads it, held read only with the step's
//! definitions (`store::defs`): [`DriveDefinition`], the accessors' fields ([`DriveDef`]: the brake scale and the gear
//! count the VCU reads too), the gear ratios, the locks' maximum torques, the workers' fields ([`WorkerDef`], with the
//! physics definition's gear guard bit) and the gear bands. The image readers are here too: the per-call path reads
//! them.

use std::ops::Range;

use super::drive_objects::DriveDef;
use super::worker::{GearBand, WorkerDef};
use crate::mem::Mem;
use crate::vcu::layout::{GearRatios, GEARS};

const DEF_LOCK_MAX_TORQUE: u32 = 0x58;
const DEF_BRAKE_SCALE: u32 = 0x620;
const DEF_LOWER_RATIO: u32 = 0x644;
const DEF_GEAR_COUNT: u32 = 0x654;
const DEF_RATIO: u32 = 0x694;
/// The gears whose band the typed definition holds (a gear past them reads the image, as the original's index does).
const BANDS: usize = GEARS;
/// The bytes of the definition the readers read, from the first axle's direction to the gear trim's reach.
pub const DRIVE_DEF_BYTES: Range<u32> = 0x18..0xA88;

/// The gear count at `def` (the VCU's gear setter clamps to it).
pub fn gear_count_at(mem: &Mem, def: u32) -> u32 {
    mem.u32(def + DEF_GEAR_COUNT)
}

/// The per-gear ratios at `def`.
pub fn gear_ratios_at(mem: &Mem, def: u32) -> GearRatios {
    GearRatios {
        lower_ratio: std::array::from_fn(|g| mem.f32(def + DEF_LOWER_RATIO + 0x50 * g as u32)),
        ratio: std::array::from_fn(|g| mem.f32(def + DEF_RATIO + 0x50 * g as u32)),
    }
}

/// The three locks' maximum torques at `def`.
pub fn lock_max_torque_at(mem: &Mem, def: u32) -> [f32; 3] {
    std::array::from_fn(|ch| mem.f32(def + DEF_LOCK_MAX_TORQUE + 0x60 * ch as u32))
}

/// The brake scale at `def`.
pub fn brake_scale_at(mem: &Mem, def: u32) -> f32 {
    mem.f32(def + DEF_BRAKE_SCALE)
}

/// A drive's definition as a step reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriveDefinition {
    pub accessors: DriveDef,
    pub ratios: GearRatios,
    pub lock_max_torque: [f32; 3],
    pub worker: WorkerDef,
    pub bands: [GearBand; BANDS],
}

impl DriveDefinition {
    /// The definition at `def`, from the image.
    pub fn lift(mem: &Mem, def: u32) -> Self {
        DriveDefinition {
            accessors: DriveDef::load(mem, def),
            ratios: gear_ratios_at(mem, def),
            lock_max_torque: lock_max_torque_at(mem, def),
            worker: WorkerDef::load(mem, def),
            bands: std::array::from_fn(|g| GearBand::load(mem, def, g as u32)),
        }
    }

    /// The band of `gear`, if held.
    pub fn band(&self, gear: u32) -> Option<GearBand> {
        self.bands.get(gear as usize).copied()
    }

    /// Every answer against the image's at `def`: the bytes compared, or the first that differs.
    pub fn differential(&self, mem: &Mem, def: u32) -> Result<usize, String> {
        if *self != Self::lift(mem, def) {
            return Err(format!("drive def at {def:#010x}: the typed definition differs from the image's"));
        }
        Ok(std::mem::size_of::<Self>())
    }
}
