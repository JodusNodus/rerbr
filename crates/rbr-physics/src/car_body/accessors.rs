//! The car's value cells and their dirty flag, and the car's ground direction. `layout` reads and writes the words;
//! these are the values.

use super::layout::Real;

/// An anti-roll bar's force as the wheel's strut takes it: negated for the bar's second side (exact).
pub fn signed_force(value: f32, negate: bool) -> f32 {
    if negate {
        -value
    } else {
        value
    }
}

/// Marks the cell changed: its dirty flag becomes 1.
pub fn set_dirty(cell: Real) -> Real {
    Real { dirty: 1, ..cell }
}

/// Adds `v` to the steering torque demand: the cell is dirtied, then `value += v` (single precision, the old value
/// first).
pub fn add_steer_torque(demand: Real, v: f32) -> Real {
    Real { value: demand.value + v, ..set_dirty(demand) }
}

/// Dirties the cell, then stores `v` in it.
pub fn set_cell(cell: Real, v: f32) -> Real {
    Real { value: v, ..set_dirty(cell) }
}
