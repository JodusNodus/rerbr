//! The flag classes and their accessors (`RBR_E45_PhysA.cpp`, `RBR_E45_Material.cpp`). A flag class has a table of name
//! words and a table of detail words, and a fixed bit count. `GetBitName(i)` is the i-th name word, `GetBitDetail(i)`
//! the i-th detail word, and `GetBitCount()` the constant. The tables are the exe's data.

use crate::mem::Mem;
use crate::physics_core::layout;

/// The physics definition's flags (`CPhysics::CDef::CFlags`): 0x14 bits (`GetBitName` 0x523b90, `GetBitDetail`
/// 0x523ba0, `GetBitCount` 0x523bb0).
pub const DEF_FLAG_NAMES: u32 = 0x007c_bc10;
pub const DEF_FLAG_BITS: u32 = 0x14;

/// `CPhysics::CDef::CDbgFlags`: 0x1b bits (0x523bc0, 0x523bd0, 0x523be0).
pub const DBG_NAMES: u32 = 0x007c_bcb0;
pub const DBG_BITS: u32 = 0x1b;

/// `CPhysics::CDef::CLogFlags`: 0x14 bits (0x523bf0, 0x523c00, 0x523c10).
pub const LOG_NAMES: u32 = 0x007c_bd90;
pub const LOG_BITS: u32 = 0x14;

/// `CMaterial::CFlags`: 4 bits (`GetBitName` 0x520840, `GetBitCount` 0x520860). Its `GetBitDetail` (0x520850) returns one
/// address whatever the index, and is not reached by the drive.
pub const MATERIAL_NAMES: u32 = 0x007c_af18;
pub const MATERIAL_BITS: u32 = 4;

/// `GetBitName(index)` of the flag class whose names are at `names`.
pub fn bit_name(mem: &Mem, names: u32, index: u32) -> u32 {
    layout::table_word(mem, names, index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_words_are_read_at_the_index() {
        let mut mem = Mem::new();
        let table: Vec<u8> = (0..4u32).flat_map(|i| (0x7300 + 4 * i).to_le_bytes()).collect();
        mem.map(0x007c_b000, &table);
        assert_eq!(bit_name(&mem, 0x007c_b000, 2), 0x7308);
    }
}
