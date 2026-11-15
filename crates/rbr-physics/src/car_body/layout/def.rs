//! The car's definition (`CCar::CDef`, the car's `+0x34`) as a step reads it, held read only with the step's
//! definitions (`store::defs`): [`CarDef`], the words `CarStore`'s getters read (`car.rs`).

use super::construct::{DEF_DANGEROUS_ACCELERATION, DEF_DRAG_COEFFICIENT, DEF_FRONTAL_AREA};
use crate::mem::Mem;
use crate::store::Object;

crate::object! {
    /// The words of a car's definition a step reads, at their offsets (names as `CarStore`'s getters).
    pub struct CarDef, size 0x44B4 {
        local_inverse_inertia @ 0x130: [f32; 3],
        inverse_mass @ 0x16C: f32,
        max_steering_lock @ 0x22C: f32,
        steering_rack_ratio @ 0x23C: f32,
        eff_wheel_radius @ 0x24C: f32,
        roll_bar_stiffness_0 @ 0x4460: f32,
        roll_bar_stiffness_1 @ 0x4470: f32,
        frontal_area @ 0x4480: f32,
        drag_coefficient @ 0x4490: f32,
        max_ff_torque @ 0x44A0: f32,
        dangerous_acceleration @ 0x44B0: f32,
    }
}

impl CarDef {
    /// Every word the table holds against the image's at `def`: the bytes compared, or the first word that differs.
    pub fn differential(&self, mem: &Mem, def: u32) -> Result<usize, String> {
        let mut bytes = 0;
        for off in Self::owned(0).flat_map(|r| r.step_by(4)) {
            let (typed, image) = (self.word_of(off).expect("a field of words"), mem.u32(def + off));
            if typed != image {
                return Err(format!("car def at {def:#010x} +{off:#x}: typed {typed:#x}, image {image:#x}"));
            }
            bytes += 4;
        }
        Ok(bytes)
    }
}

/// The offsets `car.rs` names, against the table's.
const _: () = assert!(
    CarDef::drag_coefficient.at == DEF_DRAG_COEFFICIENT
        && CarDef::frontal_area.at == DEF_FRONTAL_AREA
        && CarDef::dangerous_acceleration.at == DEF_DANGEROUS_ACCELERATION
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_ordered_and_fits() {
        crate::store::check_table::<CarDef>();
    }
}
