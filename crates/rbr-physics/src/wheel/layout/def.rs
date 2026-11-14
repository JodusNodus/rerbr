//! The wheel's definition (`E51Wheel::CDef`, `this+0x34`) as a step reads it, held read only with the step's
//! definitions (`store::defs`): [`WheelDef`], every word the wheel's functions read but the eight frames `M3A8A0`
//! writes during a step (`+0x880..+0x900`, the car's transforms of the definition's points: they stay on the image).

use crate::mem::Mem;
use crate::store::Object;

crate::object! {
    /// The words of a wheel's definition a step reads, at their offsets. The points `M3A8A0` transforms are in the
    /// car body's frame (its sources, `XFORM_PAIRS`); the vectors are tabled as words, so the word table serves them.
    pub struct WheelDef, size 0x974 {
        inverse_mass @ 0x16C: f32,
        is_steered @ 0x21C: u32,
        steering_rod_length @ 0x23C: f32,
        wishbone_header @ 0x244: [u32; 2],
        wishbone_length @ 0x24C: f32,
        joint_mount_body @ 0x260: [f32; 4],
        joint_axis_body @ 0x280: [f32; 4],
        top_mount_body @ 0x2A0: [f32; 4],
        steering_mount_body @ 0x2C0: [f32; 4],
        steering_axis_body @ 0x2E0: [f32; 4],
        drive_axle_joint_body @ 0x300: [f32; 4],
        top_mount_up_axis_body @ 0x320: [f32; 4],
        top_mount_side_axis_body @ 0x340: [f32; 4],
        steering_sign @ 0x35C: f32,
        wishbone_sign @ 0x36C: f32,
        strut_rod_mount @ 0x380: [f32; 3],
        strut_hub_point @ 0x3A0: [f32; 3],
        strut_dim_x @ 0x3EC: f32,
        strut_dim_z @ 0x3FC: f32,
        platform_height @ 0x41C: f32,
        drive_axle_mount_offset @ 0x42C: f32,
        anti_roll_bar_id @ 0x43C: u32,
        anti_roll_bar_slot @ 0x44C: u32,
        axis_inclination @ 0x900: f32,
        max_deform_side @ 0x910: f32,
        max_deform_up @ 0x920: f32,
        top_mount_damage_threshold @ 0x930: f32,
        top_mount_damage_max_force @ 0x940: f32,
        inclination_damage_angle @ 0x950: f32,
        hub_damage_threshold @ 0x960: f32,
        hub_damage_max_torque @ 0x970: f32,
    }
}

impl WheelDef {
    /// Every word the table holds against the image's at `def`: the bytes compared, or the first word that differs.
    pub fn differential(&self, mem: &Mem, def: u32) -> Result<usize, String> {
        let mut bytes = 0;
        for range in Self::owned(0) {
            for off in range.step_by(4) {
                let typed = self.word_of(off).expect("a field of words");
                let image = mem.u32(def + off);
                if typed != image {
                    return Err(format!("wheel def at {def:#010x} +{off:#x}: typed {typed:#x}, image {image:#x}"));
                }
                bytes += 4;
            }
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::super::fields::{
        DEF_ANTI_ROLL_BAR_ID, DEF_ANTI_ROLL_BAR_SLOT, DEF_AXIS_INCLINATION, DEF_HUB_DAMAGE_MAX_TORQUE,
        DEF_HUB_DAMAGE_THRESHOLD, DEF_INCLINATION_DAMAGE_ANGLE, DEF_INVERSE_MASS, DEF_MAX_DEFORM_SIDE,
        DEF_MAX_DEFORM_UP, DEF_PLATFORM_HEIGHT, DEF_TOP_MOUNT_DAMAGE_MAX_FORCE, DEF_TOP_MOUNT_DAMAGE_THRESHOLD,
        XFORM_PAIRS,
    };
    use super::super::refresh_pose::{
        DEF_DRIVE_AXLE_MOUNT_OFFSET, DEF_IS_STEERED, DEF_STEERING_ROD_LENGTH, DEF_STEERING_SIGN, DEF_STRUT_DIM_X,
        DEF_STRUT_DIM_Z, DEF_STRUT_HUB_POINT, DEF_STRUT_ROD_MOUNT, DEF_WISHBONE_HEADER, DEF_WISHBONE_LENGTH,
        DEF_WISHBONE_SIGN,
    };
    use super::*;
    use crate::store::check_table;

    /// The offsets the wheel's code names, against the table's (a test: a renamed or moved constant fails it).
    const NAMED: [(u32, u32); 23] = [
        (DEF_INVERSE_MASS, WheelDef::inverse_mass.at),
        (DEF_IS_STEERED, WheelDef::is_steered.at),
        (DEF_STEERING_ROD_LENGTH, WheelDef::steering_rod_length.at),
        (DEF_WISHBONE_HEADER, WheelDef::wishbone_header.at),
        (DEF_WISHBONE_LENGTH, WheelDef::wishbone_length.at),
        (DEF_STEERING_SIGN, WheelDef::steering_sign.at),
        (DEF_WISHBONE_SIGN, WheelDef::wishbone_sign.at),
        (DEF_STRUT_ROD_MOUNT, WheelDef::strut_rod_mount.at),
        (DEF_STRUT_HUB_POINT, WheelDef::strut_hub_point.at),
        (DEF_STRUT_DIM_X, WheelDef::strut_dim_x.at),
        (DEF_STRUT_DIM_Z, WheelDef::strut_dim_z.at),
        (DEF_PLATFORM_HEIGHT, WheelDef::platform_height.at),
        (DEF_DRIVE_AXLE_MOUNT_OFFSET, WheelDef::drive_axle_mount_offset.at),
        (DEF_ANTI_ROLL_BAR_ID, WheelDef::anti_roll_bar_id.at),
        (DEF_ANTI_ROLL_BAR_SLOT, WheelDef::anti_roll_bar_slot.at),
        (DEF_AXIS_INCLINATION, WheelDef::axis_inclination.at),
        (DEF_MAX_DEFORM_SIDE, WheelDef::max_deform_side.at),
        (DEF_MAX_DEFORM_UP, WheelDef::max_deform_up.at),
        (DEF_TOP_MOUNT_DAMAGE_THRESHOLD, WheelDef::top_mount_damage_threshold.at),
        (DEF_TOP_MOUNT_DAMAGE_MAX_FORCE, WheelDef::top_mount_damage_max_force.at),
        (DEF_INCLINATION_DAMAGE_ANGLE, WheelDef::inclination_damage_angle.at),
        (DEF_HUB_DAMAGE_THRESHOLD, WheelDef::hub_damage_threshold.at),
        (DEF_HUB_DAMAGE_MAX_TORQUE, WheelDef::hub_damage_max_torque.at),
    ];

    #[test]
    fn the_table_holds_every_named_word_and_no_frame_m3a8a0_writes() {
        check_table::<WheelDef>();
        for (named, tabled) in NAMED {
            assert_eq!(named, tabled);
        }
        for (_, source, out) in XFORM_PAIRS {
            assert!(WheelDef::field_at(source).is_some(), "{source:#x}");
            assert!(WheelDef::field_at(out).is_none(), "{out:#x}");
        }
        assert!(WheelDef::owned(0).all(|r| r.end <= 0x880 || r.start >= 0x900));
    }
}
