//! The physics core's store: `CoreStore` over `CoreFields` (`typed.rs`: the physics object's, its state's and the step
//! globals' typed words, `Mem`'s per call or `CoreView`'s held), with the offsets of the rest: the pointers, the body
//! list's nodes, the collision events, the passenger and the mounted body, on the image.

use super::globals::{physics, physics_global, SIGNAL_MANAGER};
use super::prof_calls::{LOG_FLAGS_AT, LOG_HOOK_FLAG, PHYSICS_FLAGS_AT};
use super::typed::{slot, CoreFields};
use super::PHYSICS_GLOBAL;
use crate::car_body::layout::CURRENT_CAR;
use crate::collision::layout::sort::event_time;
use crate::physics_core::contact::advanced_by_contact_step;
use crate::physics_core::layout::TELEMETRY_GLOBAL;
use crate::physics_core::math::{BodyMotion, Vec4};
use crate::physics_core::store::CoreStore;

// The physics object.
const PHYSICS_DEF_AT: u32 = 0x24;
const PHYSICS_STATE_AT: u32 = 0x20;
const PHYSICS_SCRATCH_AT: u32 = 0x28;
// The definition (`CPhysics::CDef`): the values of `Flags`, `DbgFlags` and `MaxTimeStep`.
const STEP_FLAGS_AT: u32 = 0x160cc;
const DEBUG_FLAGS_AT: u32 = 0x160e0;
const STEP_INTERVAL_AT: u32 = 0x1608c;
/// The definition's `vecGravity` key (an NTweak vector: its header, then the gravity at `+0x10`), and the gravity.
const GRAVITY_KEY_AT: u32 = 0x160a0;
const GRAVITY_KEY_VALUE_AT: u32 = 0x10;
const GRAVITY_AT: u32 = GRAVITY_KEY_AT + GRAVITY_KEY_VALUE_AT;
/// The sentinel -0x160a0 in the definition's place makes the original read the key at address 0.
const NO_GRAVITY: u32 = GRAVITY_KEY_AT.wrapping_neg();
// The state and its nodes.
const STATE_DIRTY_AT: u32 = 0x0c;
const STATE_BODIES_AT: u32 = 0x450;
const STATE_SECOND_AT: u32 = 0x480;
const NODE_NEXT_AT: u32 = 0x10;
// The collision events.
const EVENT_NEXT_AT: u32 = 0xc;
const SOLID_FLAGS_AT: u32 = 0x60;
// Globals.
const CAR_DRIVE_AT: u32 = 0x4b0;
// The passenger and its host.
const PASSENGER_HOST_AT: u32 = 0x14;
const STEP_HOST_AT: u32 = 0x40;
const VELOCITY_AT: u32 = 0x30;
const HOST_PULL_RATE_AT: u32 = 0x40;
const HOST_PULL_OFFSET_AT: u32 = 0x90;
const HOST_OFFSET_AT: u32 = 0x230;
// The mounted body.
const BODY_EVENT_SIGNAL: u32 = 0xec98;
const BODY_STATE_SIGNAL: u32 = 0xe438;
const MOUNTED_POSITION_AT: u32 = 0x140;
const MOUNTED_BASE_AT: u32 = 0x1d0;
const MOUNTED_COPY_AT: u32 = 0x330;
const CAR_BLOCK_AT: u32 = 0x200;
const CAR_POSE_AT: u32 = 0x220;
const MOUNTED_LINEAR_AT: u32 = 0x1c0;
const MOUNT_VELOCITY_AT: u32 = 0x230;
const MOUNT_POINT_AT: u32 = 0x220;
const MOUNTED_KEY_AT: u32 = 0x30;
const MOUNTED_EVENT_SENT_AT: u32 = 0x584;
const MOUNTED_ENABLE_AT: u32 = 0x580;
const MOUNTED_DIRTY_AT: u32 = 0x1dc;
const MOUNTED_CAR_AT: u32 = 0x34;
const MOUNTED_OWNER_AT: u32 = 0x3d0;
const POSE_SOURCE_AT: u32 = 0x100;
const POSE_TARGET_AT: u32 = 0x2a0;

impl<S: CoreFields> CoreStore for S {
    fn step_flags(&self) -> u32 {
        let def = self.u32(physics_global(self.mem()) + PHYSICS_DEF_AT);
        self.u32(def + STEP_FLAGS_AT)
    }

    fn debug_flags(&self) -> u32 {
        self.u32(self.u32(physics(self.mem()) + PHYSICS_DEF_AT) + DEBUG_FLAGS_AT)
    }

    fn gravity_point(&self) -> Option<[f32; 3]> {
        let def = self.u32(physics_global(self.mem()) + PHYSICS_DEF_AT);
        if def == NO_GRAVITY {
            return None;
        }
        let at = def + GRAVITY_AT;
        Some([0, 4, 8].map(|k| self.f32(at + k)))
    }

    fn step_interval(&self, this: u32) -> f32 {
        self.f32(self.u32(this + PHYSICS_DEF_AT) + STEP_INTERVAL_AT)
    }

    fn single_step_pending(&self, this: u32) -> u32 {
        self.core_field(this, slot::single_step_pending)
    }

    fn single_step_delay(&self, this: u32) -> u32 {
        self.core_field(this, slot::single_step_delay)
    }

    fn set_single_step_delay(&mut self, this: u32, value: u32) {
        self.set_core_field(this, slot::single_step_delay, value);
    }

    fn physics_state(&self, this: u32) -> u32 {
        self.u32(this + PHYSICS_STATE_AT)
    }

    fn set_contact_hit(&mut self, this: u32, hits: u32) {
        self.set_core_field(this, slot::contact_hit, hits);
    }

    fn solver(&self, this: u32) -> u32 {
        self.u32(this + PHYSICS_SCRATCH_AT) + 0x10
    }

    fn scratch_events(&self, this: u32) -> u32 {
        self.u32(self.u32(this + PHYSICS_SCRATCH_AT) + 0x14)
    }

    fn pass_count(&self, this: u32) -> u32 {
        self.core_field(this, slot::pass_count)
    }

    fn set_pass_count(&mut self, this: u32, value: u32) {
        self.set_core_field(this, slot::pass_count, value);
    }

    fn pass_maximum(&self, this: u32) -> u32 {
        self.core_field(this, slot::pass_maximum)
    }

    fn set_pass_maximum(&mut self, this: u32, value: u32) {
        self.set_core_field(this, slot::pass_maximum, value);
    }

    fn set_hits(&mut self, this: u32, value: u32) {
        self.set_core_field(this, slot::hits, value);
    }

    fn set_hits_last(&mut self, this: u32, value: u32) {
        self.set_core_field(this, slot::hits_last, value);
    }

    fn hits_maximum(&self, this: u32) -> u32 {
        self.core_field(this, slot::hits_maximum)
    }

    fn set_hits_maximum(&mut self, this: u32, value: u32) {
        self.set_core_field(this, slot::hits_maximum, value);
    }

    fn step_counter(&self, state: u32) -> u32 {
        self.core_field(state, slot::step_counter)
    }

    fn set_step_counter(&mut self, state: u32, value: u32) {
        self.set_core_field(state, slot::step_counter, value);
    }

    fn mark_state_dirty(&mut self, state: u32) {
        crate::ntweak::layout::set_dirty(self.mem_mut(), state + STATE_DIRTY_AT);
    }

    fn bodies_head(&self, state: u32) -> u32 {
        self.u32(state + STATE_BODIES_AT)
    }

    fn second_head(&self, state: u32) -> u32 {
        self.u32(state + STATE_SECOND_AT)
    }

    fn node_next(&self, node: u32) -> u32 {
        self.u32(node + NODE_NEXT_AT)
    }

    fn node_slot(&self, node: u32, offset: u32) -> u32 {
        self.u32(self.u32(node) + offset)
    }

    fn elapsed(&self, state: u32) -> f64 {
        self.core_field(state, slot::elapsed)
    }

    fn set_elapsed(&mut self, state: u32, value: f64) {
        self.set_core_field(state, slot::elapsed, value);
    }

    fn event_next(&self, event: u32) -> u32 {
        self.u32(event + EVENT_NEXT_AT)
    }

    fn event_solid(&self, event: u32, at: u32) -> u32 {
        self.u32(event + at)
    }

    fn event_penetration(&self, event: u32) -> f32 {
        event_time(self.mem(), event)
    }

    fn solid_advanced(&self, solid: u32) -> bool {
        advanced_by_contact_step(self.u8(solid + SOLID_FLAGS_AT))
    }

    fn telemetry(&self) -> u32 {
        self.u32(TELEMETRY_GLOBAL)
    }

    fn set_step_index(&mut self, step: u32) {
        self.set_core_field(PHYSICS_GLOBAL, slot::step_index, step);
    }

    fn count_step_call(&mut self) {
        let calls = self.core_field(PHYSICS_GLOBAL, slot::call_count).wrapping_add(1);
        self.set_core_field(PHYSICS_GLOBAL, slot::call_count, calls);
    }

    fn player_car(&self) -> u32 {
        self.u32(CURRENT_CAR)
    }

    fn player_drive(&self, car: u32) -> u32 {
        car + CAR_DRIVE_AT
    }

    fn passenger_host(&self, passenger: u32) -> u32 {
        self.u32(passenger + PASSENGER_HOST_AT)
    }

    fn passenger_step_host(&self, passenger: u32) -> u32 {
        self.u32(passenger + STEP_HOST_AT)
    }

    /// The host's words (the car's acceleration estimate) are its solid's (`SolidWords`).
    fn host_offset(&self, host: u32) -> [f32; 3] {
        [0, 4, 8].map(|k| f32::from_bits(self.word(host + HOST_OFFSET_AT + k)))
    }

    fn passenger_velocity(&self, passenger: u32) -> Vec4 {
        Vec4::load(self, passenger + VELOCITY_AT)
    }

    fn passenger_velocity_at(&self, passenger: u32) -> u32 {
        passenger + VELOCITY_AT
    }

    fn host_pull_at(&self, host: u32) -> u32 {
        host + HOST_PULL_OFFSET_AT
    }

    fn host_pull_vectors(&self, host: u32) -> (Vec4, Vec4) {
        (Vec4::load(self, host + HOST_PULL_RATE_AT), Vec4::load(self, host + HOST_PULL_OFFSET_AT))
    }

    fn store_host_pull_vectors(&mut self, host: u32, rate: Vec4, offset: Vec4) {
        rate.store(self, host + HOST_PULL_RATE_AT);
        offset.store(self, host + HOST_PULL_OFFSET_AT);
    }

    fn mounted_enable(&self, body: u32) -> u32 {
        self.u32(self.u32(body + MOUNTED_ENABLE_AT))
    }

    fn mounted_car(&self, body: u32) -> u32 {
        self.u32(body + MOUNTED_CAR_AT)
    }

    fn mounted_base(&self, body: u32) -> u32 {
        body + MOUNTED_BASE_AT
    }

    fn mounted_pose_at(&self, body: u32) -> u32 {
        body + MOUNTED_POSITION_AT
    }

    fn mounted_pose_copy_at(&self, body: u32) -> u32 {
        body + MOUNTED_COPY_AT
    }

    fn car_block(&self, car: u32) -> u32 {
        match car.wrapping_add(CAR_BLOCK_AT) {
            0 => 0,
            block => block + 0x10,
        }
    }

    fn car_pose(&self, car: u32) -> u32 {
        car.wrapping_add(CAR_POSE_AT)
    }

    fn mounted_owner(&self, body: u32) -> u32 {
        self.u32(body + MOUNTED_OWNER_AT)
    }

    fn set_mounted_dirty(&mut self, body: u32, value: u32) {
        self.set_u32(body + MOUNTED_DIRTY_AT, value);
    }

    fn copy_pose_source(&mut self, body: u32) {
        let words = [0, 4, 8, 12].map(|k| self.u32(body + POSE_SOURCE_AT + k));
        for (k, value) in [0, 4, 8, 12].into_iter().zip(words) {
            self.set_u32(body + POSE_TARGET_AT + k, value);
        }
    }

    fn signal_count_at(&self, signal: u32) -> u32 {
        self.u32(signal + 4)
    }

    fn signal_guard(&self, signal: u32) -> u32 {
        self.u32(signal)
    }

    fn set_signal_guard(&mut self, signal: u32, value: u32) {
        self.set_u32(signal, value);
    }

    fn list_head(&self, list: u32) -> u32 {
        self.u32(list)
    }

    fn link_next(&self, conn: u32) -> u32 {
        self.u32(conn + 4)
    }

    fn conn_owner(&self, conn: u32) -> u32 {
        self.u32(conn + 0x10)
    }

    fn signal_count(&self, count_at: u32) -> u32 {
        self.u32(count_at)
    }

    fn body_event_signal(&self) -> u32 {
        self.u32(SIGNAL_MANAGER) + BODY_EVENT_SIGNAL
    }

    fn body_state_signal(&self) -> u32 {
        self.u32(SIGNAL_MANAGER) + BODY_STATE_SIGNAL
    }

    fn mounted_position(&self, body: u32) -> Vec4 {
        Vec4::load(self, body + MOUNTED_POSITION_AT)
    }

    fn set_mounted_position(&mut self, body: u32, value: Vec4) {
        value.store(self, body + MOUNTED_POSITION_AT);
    }

    fn mounted_linear(&self, body: u32) -> Vec4 {
        Vec4::load(self, body + MOUNTED_LINEAR_AT)
    }

    fn set_mounted_linear(&mut self, body: u32, value: Vec4) {
        value.store(self, body + MOUNTED_LINEAR_AT);
    }

    fn mount_velocity(&self, body: u32) -> Vec4 {
        Vec4::load(self, body + MOUNT_VELOCITY_AT)
    }

    fn set_mount_velocity(&mut self, body: u32, value: Vec4) {
        value.store(self, body + MOUNT_VELOCITY_AT);
    }

    fn mount_velocity_at(&self, body: u32) -> u32 {
        body + MOUNT_VELOCITY_AT
    }

    fn mount_point(&self, car: u32) -> Vec4 {
        Vec4::load(self, car + MOUNT_POINT_AT)
    }

    fn mount_point_at(&self, car: u32) -> u32 {
        car + MOUNT_POINT_AT
    }

    fn copy_key_gravity(&mut self, key: u32, to: u32) {
        CoreStore::copy_vec4(self, key + GRAVITY_KEY_VALUE_AT, to);
    }

    fn gravity_key(&self) -> u32 {
        self.u32(physics_global(self.mem()) + PHYSICS_DEF_AT).wrapping_add(GRAVITY_KEY_AT)
    }

    fn mounted_key(&self, body: u32) -> u32 {
        self.u32(body + MOUNTED_KEY_AT)
    }

    fn mounted_event_sent(&self, body: u32) -> u32 {
        self.u32(body + MOUNTED_EVENT_SENT_AT)
    }

    fn set_mounted_event_sent(&mut self, body: u32, value: u32) {
        self.set_u32(body + MOUNTED_EVENT_SENT_AT, value);
    }

    fn copy_mounted_rotation(&mut self, body: u32, to: u32) {
        CoreStore::copy_vec4(self, body + POSE_SOURCE_AT, to);
    }

    fn copy_mounted_position(&mut self, body: u32, to: u32) {
        CoreStore::copy_vec4(self, body + MOUNTED_POSITION_AT, to);
    }

    fn body_motion(&self, body: u32) -> BodyMotion {
        BodyMotion {
            position: Vec4::load(self, body + MOUNTED_POSITION_AT),
            angular: Vec4::load(self, body + 0x190),
            lever: Vec4::load(self, body + 0x260),
            base: Vec4::load(self, body + 0x250),
        }
    }

    fn accumulate_hook(&self) -> bool {
        let physics = physics_global(self.mem());
        let log_flags = self.u32(self.u32(physics + PHYSICS_DEF_AT) + LOG_FLAGS_AT);
        (log_flags | self.u32(physics + PHYSICS_FLAGS_AT)) & LOG_HOOK_FLAG != 0
    }

    fn accumulate_out(&self, temp: u32) -> u32 {
        temp + 0x10
    }

    fn inverse_mass(&self, body: u32) -> f32 {
        self.f32(self.u32(body + 0x34) + 0x16c)
    }

    fn copy_gravity(&mut self, to: u32) {
        let def = self.u32(physics_global(self.mem()) + PHYSICS_DEF_AT);
        CoreStore::copy_vec4(self, def + GRAVITY_AT, to);
    }
}
