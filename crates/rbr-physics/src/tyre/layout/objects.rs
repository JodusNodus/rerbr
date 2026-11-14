//! The tyre's field tables (`crate::object!`). The names are the step footprint's (`frame_types.rs`, whose field docs
//! give the evidence); `load_frame` and `store_frame` still read by their own offsets.

use crate::math::vec::Vec4;

crate::object! {
    /// `CTyre::CScratch` (`this`, the wheel's `+0xf00`): the words the tyre step reads and writes, and its pointers. The
    /// size runs to the wheel's end; the bytes no field names are not owned.
    pub struct ScratchObj, size 0x520 {
        vtable @ 0x000: u32,
        def @ 0x010: u32,
        state @ 0x014: u32,
        contact_point @ 0x160: Vec4,
        contact_frame @ 0x170: [Vec4; 3],
        /// The `CTyreMatData` copy (`+0x1a0..+0x29c`, `material::WORDS`) and the words the step keeps between them
        /// (`+0x1e4`, `+0x1e8`, `+0x1f4`, `+0x1f8`, `+0x208`, `+0x218`, `+0x228`, `+0x258`, `+0x288`, `+0x298`).
        material @ 0x1a0: [u32; 64],
        lateral_deflection_ratio @ 0x300: f32,
        camber_radius @ 0x304: f32,
        tread_contact_area @ 0x308: f32,
        patch_length @ 0x30c: f32,
        contact_pressure @ 0x310: f32,
        normal_force @ 0x314: f32,
        carcass_force @ 0x318: f32,
        tread_force @ 0x31c: f32,
        total_force_ratio @ 0x324: f32,
        tread_force_share @ 0x328: f32,
        indentation @ 0x32c: [f32; 7],
        soil_drag @ 0x348: f32,
        lateral_soil_force @ 0x34c: f32,
        longitudinal_soil_force @ 0x350: f32,
        lateral_soil_flow @ 0x354: f32,
        soil_flow_aligned @ 0x358: f32,
        soil_flow_opposed @ 0x35c: f32,
        speed @ 0x360: f32,
        speed_sq @ 0x364: f32,
        contact_velocity @ 0x368: [f32; 3],
        long_slip_velocity @ 0x374: f32,
        /// Float bits.
        spin @ 0x378: u32,
        spin_slip @ 0x37c: f32,
        low_speed_a @ 0x380: f32,
        low_speed_b @ 0x384: f32,
        slip_phase @ 0x388: f32,
        cornering_stiffness @ 0x38c: f32,
        traction_stiffness @ 0x390: f32,
        lateral_stiffness @ 0x394: f32,
        torsional_stiffness @ 0x398: f32,
        lateral_force @ 0x39c: f32,
        longitudinal_force @ 0x3a0: f32,
        torsion_torque @ 0x3a4: f32,
        /// Float bits.
        hub_height @ 0x3a8: u32,
        inclination_sin @ 0x3ac: f32,
        inclination_cos @ 0x3b0: f32,
        stiffness_factor @ 0x3b4: f32,
        pneumatic_trail @ 0x3b8: f32,
        tread_remaining @ 0x3bc: f32,
        sector @ 0x3c0: u32,
        wet_term @ 0x3c4: f32,
        rolling_torque @ 0x3c8: f32,
        extra_force @ 0x3cc: f32,
        extra_axle_torque @ 0x3d0: f32,
        extra_term @ 0x3d4: f32,
        /// Float bits.
        slip_area_scale @ 0x3d8: u32,
        sliding_weight @ 0x3dc: f32,
        lateral_phase @ 0x3e0: f32,
        longitudinal_phase @ 0x3e4: f32,
        sliding_deflection @ 0x3e8: f32,
        sliding_torsion @ 0x3ec: f32,
        sticking_weight @ 0x3f0: f32,
        stick_phase_a @ 0x3f4: f32,
        stick_phase_b @ 0x3f8: f32,
        sticking_deflection @ 0x3fc: f32,
        sticking_torsion @ 0x400: f32,
        abrasion_rate @ 0x404: f32,
        /// The contact record (its `+0x5ac` is the contact angle) and the wear table (`m_wear`).
        contact_record @ 0x410: u32,
        wear_table @ 0x414: u32,
        signal @ 0x420: [u32; 24],
        in_contact @ 0x480: u32,
        sector_copy @ 0x484: u32,
        contact_point_copy @ 0x490: Vec4,
        motion_copy @ 0x4a0: Vec4,
        soil_flow_copy @ 0x4b0: u32,
        long_slip_velocity_copy @ 0x4b4: u32,
        longitudinal_velocity_copy @ 0x4b8: u32,
        temperature_grip @ 0x4bc: f32,
        lateral_friction_force @ 0x4c0: f32,
        longitudinal_friction_force @ 0x4c4: f32,
        debug_words @ 0x4c8: [u32; 3],
        surface_heat_loss @ 0x4e0: f32,
        frictional_power @ 0x4e4: f32,
        ngp_heat_power @ 0x4e8: f32,
        wet_damping @ 0x4ec: f32,
        smoothed_soil_drag @ 0x504: f32,
        smoothed_drag_weight @ 0x508: f32,
        smoothed_furrow_ratio @ 0x50c: f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{check_table, Object};

    #[test]
    fn table_fits() {
        check_table::<ScratchObj>();
        assert_eq!(ScratchObj::def.at, super::super::DEF);
        assert_eq!(ScratchObj::state.at, super::super::STATE);
        assert_eq!(ScratchObj::speed.at, super::super::SPEED);
        assert_eq!(ScratchObj::spin.at, super::super::SPIN);
        assert_eq!(ScratchObj::FIELDS.len(), 84);
    }
}
