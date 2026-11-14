//! The wheel's store (`crate::store`): the wheel's typed state by field ([`Slot`], `typed.rs`), and the image's words
//! the wheel reads besides (its definition, the tyre's setup, the damage record, the pointers, the world's flags).
//! `Mem` keeps every field on the image (`fields.rs`); [`WheelView`](super::WheelView) holds the step's wheels.

use super::fields::{
    BLOCK_DIFF, CAR, CAR_FLAGS, DAMAGE, DAMAGE_HUB, DAMAGE_TOP_MOUNT_SIDE, DAMAGE_TOP_MOUNT_UP, DEF,
    DEF_AXIS_INCLINATION, DEF_HUB_DAMAGE_MAX_TORQUE, DEF_HUB_DAMAGE_THRESHOLD, DEF_INCLINATION_DAMAGE_ANGLE,
    DEF_MAX_DEFORM_SIDE, DEF_MAX_DEFORM_UP, DEF_TOP_MOUNT, DEF_TOP_MOUNT_SIDE_AXIS, DEF_TOP_MOUNT_UP_AXIS, RAY_HIT,
    RAY_NORMAL, RAY_OFFSET, TYRE_DEF, TYRE_DEF_CARCASS_WIDTH, TYRE_DEF_HUB_OFFSET, WHEEL_ANGLE, WORLD_DEF, XFORM_CAR,
};
use super::refresh_pose::{
    DEBUG_GLOBAL, DEF_DRIVE_AXLE_JOINT, DEF_DRIVE_AXLE_MOUNT_OFFSET, DEF_IS_STEERED, DEF_JOINT_AXIS, DEF_JOINT_MOUNT,
    DEF_STEERING_AXIS, DEF_STEERING_MOUNT, DEF_STEERING_ROD_LENGTH, DEF_STEERING_SIGN, DEF_STRUT_DIM_X,
    DEF_STRUT_DIM_Z, DEF_STRUT_HUB_POINT, DEF_STRUT_ROD_MOUNT, DEF_WISHBONE_HEADER, DEF_WISHBONE_LENGTH,
    DEF_WISHBONE_SIGN,
};
use super::typed::{slot, Slot};
use crate::math::vec::{Vec3, Vec4};
use crate::store::{Raw, Value};
use crate::wheel::axes::{AxleWobbleIn, HubDamageIn, HubDamageUpdate, Inclination, InclinedAxle, TopMountDeformIn};
use crate::wheel::contact::RayBlock;
use crate::wheel::refresh::SuspensionDef;

pub trait WheelStore: Raw {
    /// The field `slot` of the state of the wheel at `this`.
    fn get<T: Value>(&self, this: u32, slot: Slot<T>) -> T;
    /// Stores the field `slot` of the state of the wheel at `this`.
    fn set<T: Value>(&mut self, this: u32, slot: Slot<T>, value: T);

    // The solids' words: the wheel's and the car's `CSolid` words (orientation, matrix, position, velocities; the
    // rigid body's `Solid`) the image's, a typed store's held solid's where it holds the word (`WheelView`).

    fn solid_u32(&self, at: u32) -> u32 {
        self.u32(at)
    }
    fn solid_f32(&self, at: u32) -> f32 {
        f32::from_bits(self.solid_u32(at))
    }
    fn solid_vec4(&self, at: u32) -> Vec4 {
        Vec4::load(self, at)
    }
    fn set_solid_vec4(&mut self, at: u32, v: Vec4) {
        v.store(self, at)
    }

    // The image's words.

    /// Loads the ray block at `at` (the hit word is read first, as the original does).
    fn load_ray_block(&self, at: u32) -> RayBlock {
        RayBlock {
            normal: (self.u32(at + RAY_HIT) != 0).then(|| Vec4::load(self, at + RAY_NORMAL)),
            offset: self.f32(at + RAY_OFFSET),
            difference: self.f32(at + BLOCK_DIFF),
        }
    }
    /// The tyre's setup: the pointer at `this+0xf10`.
    fn tyre_def(&self, this: u32) -> u32 {
        self.u32(this + TYRE_DEF)
    }
    /// The tyre's `HubOffset` (`tyre_def+0x6c`).
    fn load_hub_offset(&self, this: u32) -> f32 {
        self.hub_offset_of(self.tyre_def(this))
    }
    /// The `HubOffset` of the tyre setup at `tyre_def`.
    fn hub_offset_of(&self, tyre_def: u32) -> f32 {
        self.f32(tyre_def + TYRE_DEF_HUB_OFFSET)
    }
    /// The tyre's `CarcassWidth` (`tyre_def+0xcc`).
    fn load_carcass_width(&self, this: u32) -> f32 {
        self.f32(self.tyre_def(this) + TYRE_DEF_CARCASS_WIDTH)
    }
    /// The word at `off` of the wheel definition at `def` (the image's; a typed store's held definition's where it
    /// holds the word, `WheelView`).
    fn def_u32(&self, def: u32, off: u32) -> u32 {
        self.u32(def + off)
    }
    fn def_f32(&self, def: u32, off: u32) -> f32 {
        f32::from_bits(self.def_u32(def, off))
    }
    /// The vector at `off` of the wheel definition at `def`, its lanes in order.
    fn def_vec4(&self, def: u32, off: u32) -> Vec4 {
        let [x, y, z, w] = [0, 4, 8, 12].map(|k| self.def_f32(def, off + k));
        Vec4 { x, y, z, w }
    }
    fn def_vec3(&self, def: u32, off: u32) -> Vec3 {
        let [x, y, z] = [0, 4, 8].map(|k| self.def_f32(def, off + k));
        Vec3 { x, y, z }
    }
    /// The car pointer (`this+0x7f4`).
    fn load_car(&self, this: u32) -> u32 {
        self.u32(this + CAR)
    }
    /// The CDef pointer (`this+0x34`).
    fn load_def(&self, this: u32) -> u32 {
        self.u32(this + DEF)
    }
    /// The car `M3A8A0` reads (`this+0x44`), before its flag test.
    fn load_xform_car(&self, this: u32) -> u32 {
        self.u32(this + XFORM_CAR)
    }
    /// The flags byte of a car (`car+0x60`).
    fn car_flags(&self, car: u32) -> u8 {
        self.u8(car + CAR_FLAGS)
    }
    /// A float of the CDef at `off`.
    fn load_def_f32(&self, this: u32, off: u32) -> f32 {
        self.def_f32(self.load_def(this), off)
    }
    /// A vector of the CDef at `off` (`DEF_*`).
    fn load_def_vec(&self, this: u32, off: u32) -> Vec4 {
        self.def_vec4(self.load_def(this), off)
    }
    /// The damage record pointer (`this+0x7f0`).
    fn load_damage(&self, this: u32) -> u32 {
        self.u32(this + DAMAGE)
    }
    /// A float of the damage record at `off` (`DAMAGE_*`).
    fn load_damage_f32(&self, this: u32, off: u32) -> f32 {
        self.f32(self.load_damage(this) + off)
    }
    /// Stores a float of the damage record at `off` (`DAMAGE_*`).
    fn store_damage_f32(&mut self, this: u32, off: u32, value: f32) {
        let at = self.load_damage(this) + off;
        self.set_f32(at, value);
    }
    /// The field `slot` of the car at `car` (the car's typed state: the steering target and its rate).
    fn car_field<T: Value>(&self, car: u32, slot: crate::car_body::layout::Slot<T>) -> T {
        T::load(self.mem(), car + slot.at)
    }
    /// The wheel's angle of rotation (`this+0x5ac`).
    fn load_angle(&self, this: u32) -> f32 {
        self.f32(this + WHEEL_ANGLE)
    }
    /// The CDef the CPhysics global points at.
    fn world_def(&self) -> u32 {
        self.u32(self.u32(crate::car_body::layout::WORLD) + WORLD_DEF)
    }
    /// The debug flag word, `[[0x1659b70] + 0x24] + 0x160e0`: bit 2 is the debug flag `M3B900` tests, bit 0x400 gates the
    /// travel in `M3B880`.
    fn flag_word(&self) -> u32 {
        let global = self.u32(DEBUG_GLOBAL);
        self.u32(self.u32(global + 0x24) + 0x160E0)
    }
    fn debug_flag(&self) -> bool {
        self.flag_word() & 4 != 0
    }
    /// The CDef words `M3B900` reads (`this+0x34`).
    fn load_suspension_def(&self, this: u32) -> SuspensionDef {
        let def = self.load_def(this);
        SuspensionDef {
            steering_rod_length: self.def_f32(def, DEF_STEERING_ROD_LENGTH),
            wishbone_header: [self.def_u32(def, DEF_WISHBONE_HEADER), self.def_u32(def, DEF_WISHBONE_HEADER + 4)],
            wishbone_length: self.def_f32(def, DEF_WISHBONE_LENGTH),
            joint_mount: self.def_vec3(def, DEF_JOINT_MOUNT),
            joint_axis: self.def_vec4(def, DEF_JOINT_AXIS),
            wishbone_sign: self.def_f32(def, DEF_WISHBONE_SIGN),
            strut_dim_x: self.def_f32(def, DEF_STRUT_DIM_X),
            strut_dim_z: self.def_f32(def, DEF_STRUT_DIM_Z),
            steering_sign: self.def_f32(def, DEF_STEERING_SIGN),
            strut_rod_mount: self.def_vec3(def, DEF_STRUT_ROD_MOUNT),
            strut_hub_point: self.def_vec3(def, DEF_STRUT_HUB_POINT),
            is_steered: self.def_u32(def, DEF_IS_STEERED) != 0,
            drive_axle_mount_offset: self.def_f32(def, DEF_DRIVE_AXLE_MOUNT_OFFSET),
            steering_mount: self.def_vec4(def, DEF_STEERING_MOUNT),
            steering_axis: self.def_vec4(def, DEF_STEERING_AXIS),
            drive_axle_joint: self.def_vec3(def, DEF_DRIVE_AXLE_JOINT),
        }
    }

    // The axes' inputs and outputs: the state's words with the damage record's and the CDef's.

    fn load_axle_wobble(&self, this: u32) -> AxleWobbleIn {
        AxleWobbleIn {
            hub_damage: self.load_damage_f32(this, DAMAGE_HUB),
            axle: self.get(this, slot::inclined_axle),
            side: self.get(this, slot::inclined_side),
            wheel_angle: self.load_angle(this),
            previous_w: self.get(this, slot::wobbled_axle).w,
        }
    }
    fn store_wobbled_axle(&mut self, this: u32, v: Vec4) {
        self.set(this, slot::wobbled_axle, v);
    }
    fn load_inclination(&self, this: u32) -> Inclination {
        Inclination {
            hub_damage: self.load_damage_f32(this, DAMAGE_HUB),
            damage_angle: self.load_def_f32(this, DEF_INCLINATION_DAMAGE_ANGLE),
            axis_inclination: self.load_def_f32(this, DEF_AXIS_INCLINATION),
        }
    }
    /// Stores the axle and its side vector, three lanes each (lane 3 keeps its value).
    fn store_inclined_axle(&mut self, this: u32, axle: &InclinedAxle) {
        for (slot, v) in [(slot::inclined_axle, axle.axle), (slot::inclined_side, axle.side)] {
            let w = self.get(this, slot).w;
            self.set(this, slot, Vec4 { x: v.x, y: v.y, z: v.z, w });
        }
    }
    fn load_top_mount_deform(&self, this: u32) -> TopMountDeformIn {
        TopMountDeformIn {
            up_damage: self.load_damage_f32(this, DAMAGE_TOP_MOUNT_UP),
            side_damage: self.load_damage_f32(this, DAMAGE_TOP_MOUNT_SIDE),
            max_up: self.load_def_f32(this, DEF_MAX_DEFORM_UP),
            max_side: self.load_def_f32(this, DEF_MAX_DEFORM_SIDE),
            up_axis: self.load_def_vec(this, DEF_TOP_MOUNT_UP_AXIS),
            mount: self.load_def_vec(this, DEF_TOP_MOUNT),
            side_axis: self.load_def_vec(this, DEF_TOP_MOUNT_SIDE_AXIS),
        }
    }
    fn store_deformed_top_mount(&mut self, this: u32, v: Vec4) {
        self.set(this, slot::deformed_top_mount, v);
    }
    fn load_hub_damage(&self, this: u32, torque: f32) -> HubDamageIn {
        HubDamageIn {
            torque,
            threshold: self.load_def_f32(this, DEF_HUB_DAMAGE_THRESHOLD),
            max_torque: self.load_def_f32(this, DEF_HUB_DAMAGE_MAX_TORQUE),
            hub_damage: self.load_damage_f32(this, DAMAGE_HUB),
            inclination: self.load_inclination(this),
        }
    }
    /// Stores the damage update and sets the changed flag. Nothing is written when `update` is `None`.
    fn store_hub_damage(&mut self, this: u32, update: Option<&HubDamageUpdate>) {
        if let Some(update) = update {
            self.store_damage_f32(this, DAMAGE_HUB, update.hub_damage);
            self.store_inclined_axle(this, &update.axle);
            self.set(this, slot::damage_changed, 1);
        }
    }
}
