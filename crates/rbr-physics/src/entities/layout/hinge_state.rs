//! `CHingedRigidBody`'s words: the limit, slip, pivot, signal, solve, `SimDB40`/`SimD860`/`SimEFE0`/`SimD160`
//! inputs and stores, the limit update, `SimF950` and the scratch of the hinge step in the original's frames.

use super::frames::ROOT_IDENTITY;
use crate::car_body;
use crate::entities::hinge_axes::{self, AxesHandles};
use crate::entities::hinge_frame_walks::FrameWalkInput;
use crate::entities::hinge_limits::{HingeState, LimitSpring, RotationBlock};
use crate::entities::hinge_offset::OffsetInput;
use crate::entities::hinge_pivot::{Pivot, PivotHandles, PivotInput};
use crate::entities::hinge_pose::{PoseBody, PoseDefinition, PoseHandles, PoseState};
use crate::entities::hinge_refresh::{RefreshHandles, RefreshState};
use crate::entities::hinge_signals::SignalInput;
use crate::entities::hinge_slip::{SlipHandles, SlipInput};
use crate::entities::hinge_solve::{ContactHandles, EntryTerms, SolveTerms};
use crate::entities::hinge_stops::Limits;
use crate::entities::store::{AdvanceInput, HingeStore, StatePayload};
use crate::math::vec::Vec4;
use crate::math::Mat3;
use crate::mem::Mem;
use crate::physics_core;

// The hinged body (`CHingedRigidBody`, at `this`). The names follow the model's fields ([`HingeStore`]'s inputs).
/// Its key (+0x30), definition (+0x34), contact grid (+0x64), orientation quaternion (+0x100), position (+0x140) and
/// linear and angular velocities (+0x1c0, +0x190).
const HINGE_KEY: u32 = 0x30;
const HINGE_DEF: u32 = 0x34;
const HINGE_GRID: u32 = 0x64;
const HINGE_ORIENTATION: u32 = 0x100;
const HINGE_POSITION: u32 = 0x140;
const HINGE_ANGULAR_VELOCITY: u32 = 0x190;
const HINGE_LINEAR_VELOCITY: u32 = 0x1C0;
/// The tweak object whose dirty marker (+8 of it) the refresh sets, and the counter beside it.
const HINGE_DIRTY_TWEAK: u32 = 0x1D0;
const HINGE_DIRTY_COUNTER: u32 = 0x1DC;
/// The start pose's copies of the orientation and the position.
const HINGE_ORIENTATION_COPY: u32 = 0x2A0;
const HINGE_POSITION_COPY: u32 = 0x330;
/// The owner car (`CCar*`, +0x3d0), read by `SimCFF0` and the walks it makes.
pub(super) const HINGE_OWNER: u32 = 0x3D0;
/// The two limit springs' positions (+0x3e8, +0x3ec).
const HINGE_SPRING_POSITIONS: u32 = 0x3E8;
/// The angle's tweak object (+0x3f0, `NTweak::CBase`; its value at +0xc is the angle) and the angle block from its +4
/// (the window the end stops take), the angle (+0x3fc), the rate's tweak object (+0x400, also the limit object the
/// flag call is made on) and the rate (+0x40c).
const ANGLE_TWEAK: u32 = 0x3F0;
const HINGE_ANGLE_BLOCK: u32 = 0x3F4;
const HINGE_ANGLE: u32 = 0x3FC;
const RATE_TWEAK: u32 = 0x400;
const HINGE_RATE: u32 = 0x40C;
/// The pose's orientation matrix (+0x550) and frame matrix (+0x574), and the hinge rotation (+0x598..+0x5b8, nine words,
/// row by row).
const HINGE_ORIENTATION_MATRIX: u32 = 0x550;
const HINGE_FRAME_MATRIX: u32 = 0x574;
const HINGE_ROTATION: u32 = 0x598;
/// The body's offset vector (+0x5c0, the meaning is unknown: `SimD860`'s offset, also the transformed hinge point of the
/// pose), its copy, and the copy of the offset result.
const HINGE_OFFSET: u32 = 0x5C0;
const HINGE_OFFSET_COPY: u32 = 0x5D0;
const HINGE_OFFSET_RESULT_COPY: u32 = 0x5E0;
/// The hinge axis carried into the car's frame (`SimD160`'s second walk output, `MB560`'s).
const HINGE_CARRIED_AXIS: u32 = 0x5F0;
/// The pivot's outputs: the hinge point's acceleration (+0x600..+0x608), the car's angular acceleration in the body
/// (+0x610, the second walk's output) and the frame's angular acceleration along the axis (+0x620).
const HINGE_POINT_ACCELERATION: u32 = 0x600;
const HINGE_ANGULAR_ACCELERATION: u32 = 0x610;
const HINGE_FRAME_ANGULAR_ACCELERATION: u32 = 0x620;
/// The solve's value (`SimEAD0` writes it).
const HINGE_SOLVED: u32 = 0x624;
/// The hinge link pointer (+0x650); its meaning is unknown.
const HINGE_LINK: u32 = 0x650;

// The hinged definition (`CHingedRigidBody::CDef`, at `def`): the value words of its tweaks (a tweak's value is at +0xc
// of a real, +0x10 of a vector; `layout::construct_hinged_def`).
/// The inverse mass (+0x16c).
const DEF_INVERSE_MASS: u32 = 0x16C;
/// The limit springs (`+0x1f0 + 0x60 i`) and the values of their members (kind, stiffness, damping, maximum displacement,
/// span).
const LIMIT_BLOCK: u32 = 0x1F0;
const LIMIT_STRIDE: u32 = 0x60;
const LIMIT_KIND: u32 = 0x0C;
const LIMIT_STIFFNESS: u32 = 0x2C;
const LIMIT_DAMPING: u32 = 0x3C;
const LIMIT_MAX_DISPLACEMENT: u32 = 0x4C;
const LIMIT_SPAN: u32 = 0x5C;
/// The springs' second members (the position the refresh copies to the body), in the first and second block.
const DEF_SPRING_POSITION_A: u32 = 0x20C;
const DEF_SPRING_POSITION_B: u32 = 0x26C;
/// The reference-frame vectors: the hinge axis (+0x2c0, whose tweak object at +0x2b0 is the gate the settle axis tests
/// the address of), the zero axis (+0x2e0) and the hinge point (+0x300, whose tweak object at +0x2f0 is the gate the
/// pivot tests the address of), the first frame build's inverse origin (+0x310) and the hinge point composed with the
/// car (+0x320).
const DEF_HINGE_AXIS_RF_TWEAK: u32 = 0x2B0;
const DEF_HINGE_AXIS_RF: u32 = 0x2C0;
const DEF_ZERO_AXIS_RF: u32 = 0x2E0;
const DEF_HINGE_POINT_RF_TWEAK: u32 = 0x2F0;
const DEF_HINGE_POINT_RF: u32 = 0x300;
const DEF_INVERSE_ORIGIN_RF: u32 = 0x310;
const DEF_COMPOSED_HINGE_POINT: u32 = 0x320;
/// `FaceArea` (+0x34c) and the body-frame vectors: the face normal (+0x360), the hinge axis (+0x380), the zero axis
/// (+0x3a0), the hinge point (+0x3c0) and the second frame build's inverse origin (+0x3d0).
const DEF_FACE_AREA: u32 = 0x34C;
const DEF_FACE_NORMAL_BF: u32 = 0x360;
const DEF_HINGE_AXIS_BF: u32 = 0x380;
const DEF_ZERO_AXIS_BF: u32 = 0x3A0;
const DEF_HINGE_POINT_BF: u32 = 0x3C0;
const DEF_INVERSE_ORIGIN_BF: u32 = 0x3D0;
/// The frame builds' matrices: the reference-frame axes (+0x3e0) and inverse axes (+0x404), the body-frame inverse axes
/// (+0x428) and axes (+0x44c), 3x3 row-major.
const DEF_AXES_RF: u32 = 0x3E0;
const DEF_INVERSE_AXES_RF: u32 = 0x404;
const DEF_INVERSE_AXES_BF: u32 = 0x428;
const DEF_AXES_BF: u32 = 0x44C;
/// `HingeMaxAngle` and `HingeMinAngle`: the records the end stops take (three words from +4 of the tweak, the bound
/// last) and the bounds.
const DEF_MAX_ANGLE_RECORD: u32 = 0x474;
const DEF_MAX_ANGLE: u32 = 0x47C;
const DEF_MIN_ANGLE_RECORD: u32 = 0x484;
const DEF_MIN_ANGLE: u32 = 0x48C;
/// The angle block (+0x494, `HingeDefaultAngle`'s tweak from +4; its last word is the default angle, +0x49c).
const DEF_ANGLE_BLOCK: u32 = 0x494;
const DEF_DEFAULT_ANGLE: u32 = 0x49C;
/// The end stops' restitution (+0x4ac), `InertiaHingeAxis` (+0x4bc), `SpringRate` (+0x4cc), `Damping` (+0x4dc),
/// `Unlocked` (+0x4ec) and `BodyId` (+0x4fc).
const DEF_RESTITUTION: u32 = 0x4AC;
const DEF_INERTIA: u32 = 0x4BC;
const DEF_SPRING_RATE: u32 = 0x4CC;
const DEF_DAMPING: u32 = 0x4DC;
const DEF_UNLOCKED: u32 = 0x4EC;
const DEF_BODY_ID: u32 = 0x4FC;

// The owner car (`CCar`, at `car`).
/// The car's angular velocity (+0x190; its tweak object at +0x180 is the gate the pivot tests the address of), its
/// acceleration (+0x250, the z word at +0x258) and its angular acceleration (+0x260).
const CAR_ANGULAR_VELOCITY_TWEAK: u32 = 0x180;
const CAR_ANGULAR_VELOCITY: u32 = 0x190;
const CAR_ACCELERATION: u32 = 0x250;
const CAR_ACCELERATION_Z: u32 = 0x258;
const CAR_ANGULAR_ACCELERATION: u32 = 0x260;
/// The car's state object is at car+0xa00 (`car_body::layout::state_address`); its vector the slip copies is at +0xc10
/// and its speed word (named by use: the slip squares it) at +0xc00.
const STATE_VECTOR: u32 = 0xC10;
const STATE_SPEED: u32 = 0xC00;
/// The CDef pointer in the CPhysics (`g_1659b70`), the CDef's debug flags, and the flag the frame walks test (its
/// meaning is inferred from the guard).
const PHYSICS_DEF: u32 = 0x24;
const DBG_FLAGS: u32 = 0x160E0;
const DBG_HINGE_FLAG: u32 = 0x40_0000;
/// The state signal's offset from the signal manager.
const STATE_SIGNAL: u32 = 0xE438;

/// `CHingedRigidBody::Set650` (0x51da50): stores `link` at `at + 0x650`.
pub fn set_hinge_link(mem: &mut Mem, at: u32, link: u32) {
    mem.set_u32(at + HINGE_LINK, link);
}

pub fn vec3(mem: &Mem, at: u32) -> [f32; 3] {
    [mem.f32(at), mem.f32(at + 4), mem.f32(at + 8)]
}

fn mat3(mem: &Mem, at: u32) -> [[f32; 3]; 3] {
    std::array::from_fn(|row| vec3(mem, at + 12 * row as u32))
}

pub(super) fn words<const N: usize>(mem: &Mem, at: u32) -> [u32; N] {
    std::array::from_fn(|i| mem.u32(at + 4 * i as u32))
}

fn set_words(mem: &mut Mem, at: u32, values: &[u32]) {
    for (i, value) in values.iter().enumerate() {
        mem.set_u32(at + 4 * i as u32, *value);
    }
}

fn set_mat3(mem: &mut Mem, at: u32, m: &Mat3) {
    for (i, value) in m.iter().flatten().enumerate() {
        mem.set_f32(at + 4 * i as u32, *value);
    }
}

/// A frame build of the hinged definition ([`hinge_axes`]): `CHingedRigidBody::VfE070` (0x51e070) on the windows of
/// `frame`. It stores `e`, then reads `f[9]` (the word after `f`: the
/// second call's is `e[0]`, which that store has just written), and stores `f` and `d`. The asm stores `f` before it reads
/// `f[9]` and reads `a` after that; `a` is read here before `f` is stored, which changes nothing: the windows don't overlap.
pub fn build_hinge_axes(mem: &mut Mem, frame: &AxesHandles) {
    let axes = hinge_axes::axes(mem.f32s(frame.direction), vec3(mem, frame.reference));
    set_mat3(mem, frame.axes, &axes);
    let tail = mem.f32(frame.inverse_axes + 36);
    let (inverse_origin, inverse_axes) = hinge_axes::inverse(vec3(mem, frame.origin), &axes, tail);
    set_f32s(mem, frame.inverse_axes, &inverse_axes);
    set_f32s(mem, frame.inverse_origin, &inverse_origin);
}

fn set_f32s(mem: &mut Mem, at: u32, values: &[f32]) {
    for (i, value) in values.iter().enumerate() {
        mem.set_f32(at + 4 * i as u32, *value);
    }
}

fn load_window(mem: &Mem, at: u32) -> [u32; 3] {
    std::array::from_fn(|i| mem.u32(at + 4 * i as u32))
}

/// The limit object the flag call is made on.
pub fn limit_object(this: u32) -> u32 {
    this + RATE_TWEAK
}

/// Stores the walk vector `SimE1D0` builds in the frame (`PivotCalls::store_vector`): three words.
pub fn store_walk_vector(mem: &mut Mem, at: u32, words: [u32; 3]) {
    for (i, word) in words.into_iter().enumerate() {
        mem.set_u32(at + 4 * i as u32, word);
    }
}

/// `SimCFF0`'s scratch in its frame (`base`, the frame's 16-aligned base): the copy of the car's vector and the two
/// walks' outputs.
pub fn slip_frame(base: u32) -> SlipHandles {
    SlipHandles { vector_copy: base - 0x20, face_normal: base - 0x50, velocity: base - 0x60 }
}

/// `SimE1D0`'s scratch in its frame: the walk vector, the first walk's output, the compose's scratch (the third walk's
/// output, with the compose's output 0x10 on).
pub fn pivot_frame(base: u32, this: u32) -> PivotHandles {
    PivotHandles {
        vector: base - 0x50,
        acceleration: base - 0x40,
        angular_acceleration: this + HINGE_ANGULAR_ACCELERATION,
        angular_velocity: base - 0x30,
        lever: base - 0x20,
    }
}

/// `SimEAD0`'s vote vectors in its frame (the two words the vote writes, 16 bytes each).
pub fn solve_frame(base: u32) -> ContactHandles {
    ContactHandles { force: base - 0x20, torque: base - 0x80 }
}

/// The tweak objects of the hinge rate and the angle (`NTweak::CBase`), which `SimF950` marks dirty.
pub fn rate_tweak(this: u32) -> u32 {
    this + RATE_TWEAK
}

pub fn angle_tweak(this: u32) -> u32 {
    this + ANGLE_TWEAK
}

/// The angular velocity vector `SimF950` scales into. The asm tests the address (`this + 0x180`), so the vector is absent
/// only when `this` is -0x180.
pub fn scaled_vector(this: u32) -> u32 {
    if this.wrapping_add(0x180) != 0 {
        this + HINGE_ANGULAR_VELOCITY
    } else {
        0
    }
}

/// The hinged body's position (`this + 0x140`), which `E01Vec4::CopyFrom` returns.
pub fn position_at(this: u32) -> u32 {
    this + HINGE_POSITION
}

/// The definition's limit block `idx` (any index: the asm's `idx * 0x60` wraps).
fn limit_block(mem: &Mem, def: u32, idx: u32) -> LimitSpring {
    let block = def.wrapping_add(LIMIT_BLOCK).wrapping_add(LIMIT_STRIDE.wrapping_mul(idx));
    LimitSpring {
        kind: mem.u32(block + LIMIT_KIND),
        stiffness: mem.f32(block + LIMIT_STIFFNESS),
        damping: mem.f32(block + LIMIT_DAMPING),
        max_displacement: mem.f32(block + LIMIT_MAX_DISPLACEMENT),
        span: mem.f32(block + LIMIT_SPAN),
    }
}

/// The body's limit state word `idx` (any index: the address the asm forms).
fn limit_state_at(this: u32, idx: u32) -> u32 {
    this.wrapping_add(HINGE_SPRING_POSITIONS).wrapping_add(idx.wrapping_mul(4))
}

impl HingeStore for Mem {
    fn hinge_link(&self, this: u32) -> u32 {
        self.u32(this + HINGE_LINK)
    }

    fn hinge_definition(&self, this: u32) -> u32 {
        self.u32(this + HINGE_DEF)
    }

    /// `*(this+0x650) != 0` makes `SimEAD0` return at once.
    fn hinge_linked(&self, this: u32) -> bool {
        self.u32(self.u32(this + HINGE_LINK)) != 0
    }

    /// `SimEAD0` solves only for a body with an owner car (`this+0x3d0`).
    fn hinge_owned(&self, this: u32) -> bool {
        self.u32(this + HINGE_OWNER) != 0
    }

    fn load_hinge_limit(&self, def: u32, idx: u32) -> LimitSpring {
        limit_block(self, def, idx)
    }

    fn load_hinge_limits(&self, def: u32) -> [LimitSpring; 2] {
        std::array::from_fn(|i| limit_block(self, def, i as u32))
    }

    /// The hinge state of the body at `this`: its limit states, angle and rate.
    fn load_hinge_state(&self, this: u32) -> HingeState {
        HingeState {
            spring_position: self.f32s(this + HINGE_SPRING_POSITIONS),
            angle: self.f32(this + HINGE_ANGLE),
            rate: self.f32(this + HINGE_RATE),
        }
    }

    fn body_limit(&self, this: u32, idx: u32) -> LimitSpring {
        limit_block(self, self.u32(this + HINGE_DEF), idx)
    }

    fn body_limits(&self, this: u32) -> [LimitSpring; 2] {
        self.load_hinge_limits(self.u32(this + HINGE_DEF))
    }

    fn limit_position(&self, this: u32, idx: u32) -> f32 {
        self.f32(limit_state_at(this, idx))
    }

    fn store_limit_position(&mut self, this: u32, idx: u32, value: f32) {
        self.set_f32(limit_state_at(this, idx), value);
    }

    fn store_limit_positions(&mut self, this: u32, values: [f32; 2]) {
        for (i, value) in values.into_iter().enumerate() {
            self.set_f32(this + HINGE_SPRING_POSITIONS + 4 * i as u32, value);
        }
    }

    fn angle_rate(&self, this: u32) -> (f32, f32) {
        (self.f32(this + HINGE_ANGLE), self.f32(this + HINGE_RATE))
    }

    fn hinge_angle(&self, this: u32) -> f32 {
        self.f32(this + HINGE_ANGLE)
    }

    /// The rotation block of the hinge angle, nine words row by row.
    fn store_rotation_block(&mut self, this: u32, block: &RotationBlock) {
        for (i, value) in block.rows.iter().flatten().enumerate() {
            self.set_f32(this + HINGE_ROTATION + 4 * i as u32, *value);
        }
    }

    /// The inputs of `CHingedRigidBody::SimCFF0` (0x51cff0) at `this`.
    fn slip_input(&self, this: u32) -> SlipInput {
        let def = self.u32(this + HINGE_DEF);
        let car = self.u32(this + HINGE_OWNER);
        let state = car_body::layout::state_address(car);
        SlipInput {
            body: this,
            face_normal_bf: def + DEF_FACE_NORMAL_BF,
            car,
            state,
            identity: self.u32(ROOT_IDENTITY),
            car_vector: state + STATE_VECTOR,
            position: vec3(self, this + HINGE_POSITION),
            composed_hinge_point: vec3(self, def + DEF_COMPOSED_HINGE_POINT),
            face_area: self.f32(def + DEF_FACE_AREA),
            hinge_axis_rf: vec3(self, def + DEF_HINGE_AXIS_RF),
        }
    }

    fn car_slip_word(&self, state: u32) -> f32 {
        self.f32(state + STATE_SPEED)
    }

    /// The inputs `SimE1D0` reads (`PivotInput`). The probe and the compose's `a` are taken only when their gate word's
    /// address is nonzero, as the asm tests the address (`leal`/`testl`), not the memory.
    fn pivot_input(&self, this: u32) -> PivotInput {
        let def = self.u32(this + HINGE_DEF);
        let car = self.u32(this + HINGE_OWNER);
        PivotInput {
            identity: self.u32(ROOT_IDENTITY),
            car,
            car_acceleration_xy: [self.u32(car + CAR_ACCELERATION), self.u32(car + CAR_ACCELERATION + 4)],
            car_acceleration_z: self.f32(car + CAR_ACCELERATION_Z),
            car_angular_acceleration: car + CAR_ANGULAR_ACCELERATION,
            car_angular_velocity: if car + CAR_ANGULAR_VELOCITY_TWEAK != 0 { car + CAR_ANGULAR_VELOCITY } else { 0 },
            hinge_point_rf: if def + DEF_HINGE_POINT_RF_TWEAK != 0 { def + DEF_HINGE_POINT_RF } else { 0 },
            hinge_axis_rf: vec3(self, def + DEF_HINGE_AXIS_RF),
        }
    }

    /// Stores `SimE1D0`'s pivot (+0x600..+0x608) and settle value (+0x620) in the hinged body.
    fn store_pivot(&mut self, this: u32, pivot: &Pivot) {
        for (i, &v) in pivot.hinge_point_acceleration.iter().enumerate() {
            self.set_f32(this + HINGE_POINT_ACCELERATION + 4 * i as u32, v);
        }
        self.set_f32(this + HINGE_FRAME_ANGULAR_ACCELERATION, pivot.frame_angular_acceleration);
    }

    fn settle_axis(&self, this: u32) -> u32 {
        let def = self.u32(this + HINGE_DEF);
        if def.wrapping_add(DEF_HINGE_AXIS_RF_TWEAK) == 0 {
            0
        } else {
            def.wrapping_add(DEF_HINGE_AXIS_RF)
        }
    }

    /// The inputs `SimFB40` reads (`SignalInput`). Its branches test the key and the owner; the addresses it forms are not
    /// tested in memory, so every field is read here.
    fn signal_input(&self, this: u32) -> SignalInput {
        let definition = self.u32(this + HINGE_DEF);
        let link = self.u32(this + HINGE_LINK);
        SignalInput {
            body: this,
            orientation_matrix: this + HINGE_ORIENTATION_MATRIX,
            identity: self.u32(ROOT_IDENTITY),
            hinge_point_bf: definition + DEF_HINGE_POINT_BF,
            key: self.u32(this + HINGE_KEY),
            owner: self.u32(this + HINGE_OWNER),
            orientation: self.f32s(this + HINGE_ORIENTATION),
            linear_velocity: self.f32s(this + HINGE_LINEAR_VELOCITY),
            angular_velocity: self.f32s(this + HINGE_ANGULAR_VELOCITY),
            hinge_point_rf: self.f32s(definition + DEF_HINGE_POINT_RF),
            body_id: self.u32(definition + DEF_BODY_ID),
            angle: self.f32(this + HINGE_ANGLE),
            default_angle: self.f32(definition + DEF_DEFAULT_ANGLE),
            link_flag: self.u32(link),
        }
    }

    /// The definition's `+0x4bc` (`b0`) and `+0x16c` (`mass`), read before `SimE1D0`.
    fn solve_entry_terms(&self, this: u32) -> EntryTerms {
        let def = self.u32(this + HINGE_DEF);
        EntryTerms { inertia: self.f32(def + DEF_INERTIA), inverse_mass: self.f32(def + DEF_INVERSE_MASS) }
    }

    /// The solve's inputs, read after `SimE1D0`.
    fn solve_terms(&self, this: u32) -> SolveTerms {
        let def = self.u32(this + HINGE_DEF);
        SolveTerms {
            axes_rf: mat3(self, def + DEF_AXES_RF),
            hinge_rotation: mat3(self, this + HINGE_ROTATION),
            hinge_point_acceleration: vec3(self, this + HINGE_POINT_ACCELERATION),
            inverse_origin_bf: vec3(self, def + DEF_INVERSE_ORIGIN_BF),
            hinge_axis_rf: vec3(self, def + DEF_HINGE_AXIS_RF),
            hinge_axis_bf: vec3(self, def + DEF_HINGE_AXIS_BF),
            hinge_point_bf: vec3(self, def + DEF_HINGE_POINT_BF),
            default_angle: self.f32(def + DEF_DEFAULT_ANGLE),
            spring_rate: self.f32(def + DEF_SPRING_RATE),
            damping: self.f32(def + DEF_DAMPING),
            angle: self.f32(this + HINGE_ANGLE),
            rate: self.f32(this + HINGE_RATE),
            frame_angular_acceleration: self.f32(this + HINGE_FRAME_ANGULAR_ACCELERATION),
        }
    }

    /// The solve's value, which `SimEAD0` writes at +0x624.
    fn store_solved(&mut self, this: u32, value: f32) {
        self.set_f32(this + HINGE_SOLVED, value);
    }

    /// The inputs of `CHingedRigidBody::SimDB40` (0x51db40) at `this`, or `None` when its guard fails: the debug flag is
    /// clear, or there is no owner car. The asm tests the flag first, then the car.
    fn frame_walk_input(&self, this: u32) -> Option<FrameWalkInput> {
        let physics = self.u32(car_body::layout::WORLD);
        let flags = self.u32(self.u32(physics + PHYSICS_DEF) + DBG_FLAGS);
        let car = self.u32(this + HINGE_OWNER);
        if flags & DBG_HINGE_FLAG == 0 || car == 0 {
            return None;
        }
        let def = self.u32(this + HINGE_DEF);
        Some(FrameWalkInput {
            identity: self.u32(ROOT_IDENTITY),
            body: this,
            car,
            hinge_rotation: mat3(self, this + HINGE_ROTATION),
            frame_matrix: mat3(self, this + HINGE_FRAME_MATRIX),
            axes_rf: mat3(self, def + DEF_AXES_RF),
            inverse_origin_bf: vec3(self, def + DEF_INVERSE_ORIGIN_BF),
            hinge_point_rf: vec3(self, def + DEF_HINGE_POINT_RF),
            face_normal_bf: vec3(self, def + DEF_FACE_NORMAL_BF),
        })
    }

    /// The inputs of `CHingedRigidBody::SimD860` (0x51d860) at `this`.
    fn offset_input(&self, this: u32) -> OffsetInput {
        let def = self.u32(this + HINGE_DEF);
        OffsetInput {
            identity: self.u32(ROOT_IDENTITY),
            car: self.u32(this + HINGE_OWNER),
            rotation_row: vec3(self, this + HINGE_ROTATION + 12),
            axes_rf: mat3(self, def + DEF_AXES_RF),
            offset: vec3(self, this + HINGE_OFFSET),
        }
    }

    /// The body and definition words `SimEFE0` reads or writes (`RefreshState`), as the body is in memory now.
    fn hinge_refresh_state(&self, this: u32) -> RefreshState {
        let def = self.u32(this + HINGE_DEF);
        let link_address = self.u32(this + HINGE_LINK);
        RefreshState {
            unlocked: self.u32(def + DEF_UNLOCKED),
            definition_angle_block: words(self, def + DEF_ANGLE_BLOCK),
            max_angle: self.u32(def + DEF_MAX_ANGLE),
            min_angle: self.u32(def + DEF_MIN_ANGLE),
            definition_spring_position: [self.u32(def + DEF_SPRING_POSITION_A), self.u32(def + DEF_SPRING_POSITION_B)],
            hinge_point_bf: words(self, def + DEF_HINGE_POINT_BF),
            link_address,
            link_flag: self.u32(link_address),
            grid: self.u32(this + HINGE_GRID),
            dirty: self.u32(this + HINGE_DIRTY_TWEAK + 8),
            dirty_counter: self.u32(this + HINGE_DIRTY_COUNTER),
            angle_block: words(self, this + HINGE_ANGLE_BLOCK),
            spring_position: words(self, this + HINGE_SPRING_POSITIONS),
            position: words(self, this + HINGE_POSITION),
            position_copy: words(self, this + HINGE_POSITION_COPY),
            orientation: words(self, this + HINGE_ORIENTATION),
            orientation_copy: words(self, this + HINGE_ORIENTATION_COPY),
            offset: words(self, this + HINGE_OFFSET),
            offset_copy: words(self, this + HINGE_OFFSET_COPY),
            offset_result_copy: words(self, this + HINGE_OFFSET_RESULT_COPY),
        }
    }

    /// Writes the words `SimEFE0` changes: the flag, the CBase and its counter, the angle block, the limit words and the
    /// copies.
    fn store_hinge_refresh_state(&mut self, this: u32, state: &RefreshState) {
        self.set_u32(state.link_address, state.link_flag);
        set_words(self, this + HINGE_DIRTY_TWEAK + 8, &[state.dirty]);
        set_words(self, this + HINGE_DIRTY_COUNTER, &[state.dirty_counter]);
        set_words(self, this + HINGE_ANGLE_BLOCK, &state.angle_block);
        set_words(self, this + HINGE_SPRING_POSITIONS, &state.spring_position);
        set_words(self, this + HINGE_POSITION_COPY, &state.position_copy);
        set_words(self, this + HINGE_ORIENTATION_COPY, &state.orientation_copy);
        set_words(self, this + HINGE_OFFSET_COPY, &state.offset_copy);
        set_words(self, this + HINGE_OFFSET_RESULT_COPY, &state.offset_result_copy);
    }

    /// The addresses `SimEFE0` passes to its calls: the definition's blocks (the `VfE070` pointers, `BodyCompose`'s `a` and
    /// output) and the body's car. `offset_result` is the caller's scratch vector for `SimD860`.
    fn refresh_handles(&self, this: u32, offset_result: u32) -> RefreshHandles {
        let def = self.u32(this + HINGE_DEF);
        RefreshHandles {
            body: this,
            car: self.u32(this + HINGE_OWNER),
            first_axes: [
                def + DEF_HINGE_POINT_RF,
                def + DEF_HINGE_AXIS_RF,
                def + DEF_ZERO_AXIS_RF,
                def + DEF_INVERSE_ORIGIN_RF,
                def + DEF_AXES_RF,
                def + DEF_INVERSE_AXES_RF,
            ],
            second_axes: [
                def + DEF_HINGE_POINT_BF,
                def + DEF_HINGE_AXIS_BF,
                def + DEF_ZERO_AXIS_BF,
                def + DEF_INVERSE_ORIGIN_BF,
                def + DEF_AXES_BF,
                def + DEF_INVERSE_AXES_BF,
            ],
            compose_input: def + DEF_HINGE_POINT_RF,
            compose_output: def + DEF_COMPOSED_HINGE_POINT,
            offset_out: offset_result,
        }
    }

    /// The addresses `SimD160` passes to its calls.
    fn pose_handles(&self, this: u32) -> PoseHandles {
        let def = self.u32(this + HINGE_DEF);
        PoseHandles {
            car: self.u32(this + HINGE_OWNER),
            hinge_point_rf: def + DEF_HINGE_POINT_RF,
            hinge_axis_rf: def + DEF_HINGE_AXIS_RF,
            identity: self.u32(ROOT_IDENTITY),
            transformed_hinge_point: this + HINGE_OFFSET,
            carried_hinge_axis: this + HINGE_CARRIED_AXIS,
            body: this,
            frame_matrix: this + HINGE_FRAME_MATRIX,
            quaternion_out: this + HINGE_ORIENTATION,
        }
    }

    /// The definition's frame words, and the body's block and position, as they are in memory now (`SimD160`'s reads).
    fn pose_state(&self, this: u32) -> PoseState {
        let def = self.u32(this + HINGE_DEF);
        PoseState {
            definition: PoseDefinition {
                composed_hinge_point: vec3(self, def + DEF_COMPOSED_HINGE_POINT),
                inverse_origin_bf: vec3(self, def + DEF_INVERSE_ORIGIN_BF),
                axes_rf: mat3(self, def + DEF_AXES_RF),
                inverse_axes_bf: mat3(self, def + DEF_INVERSE_AXES_BF),
            },
            body: PoseBody {
                orientation_matrix: mat3(self, this + HINGE_ORIENTATION_MATRIX),
                frame_matrix: mat3(self, this + HINGE_FRAME_MATRIX),
                hinge_rotation: mat3(self, this + HINGE_ROTATION),
            },
            position: self.f32s(this + HINGE_POSITION),
        }
    }

    /// Writes the body's products and its position (`SimD160`'s stores). `hinge_rotation` is not written: the port does not
    /// change it.
    fn store_pose_state(&mut self, this: u32, state: &PoseState) {
        set_mat3(self, this + HINGE_ORIENTATION_MATRIX, &state.body.orientation_matrix);
        set_mat3(self, this + HINGE_FRAME_MATRIX, &state.body.frame_matrix);
        for (i, value) in state.position.iter().enumerate() {
            self.set_f32(this + HINGE_POSITION + 4 * i as u32, *value);
        }
    }

    fn limit_inputs(&self, this: u32) -> Limits {
        let tweak = self.u32(this + HINGE_DEF);
        Limits {
            angle: self.f32(this + HINGE_ANGLE),
            rate: self.f32(this + HINGE_RATE),
            restitution: self.f32(tweak + DEF_RESTITUTION),
            max_angle: load_window(self, tweak + DEF_MAX_ANGLE_RECORD),
            min_angle: load_window(self, tweak + DEF_MIN_ANGLE_RECORD),
        }
    }

    fn store_limit_window(&mut self, this: u32, window: [u32; 3]) {
        for (i, word) in window.into_iter().enumerate() {
            self.set_u32(this + HINGE_ANGLE_BLOCK + 4 * i as u32, word);
        }
    }

    fn store_limit_rate(&mut self, this: u32, rate: f32) {
        self.set_f32(this + HINGE_RATE, rate);
    }

    fn advance_input(&self, this: u32) -> AdvanceInput {
        AdvanceInput {
            solved: self.f32(this + HINGE_SOLVED),
            rate: self.f32(this + HINGE_RATE),
            angle: self.f32(this + HINGE_ANGLE),
        }
    }

    fn store_rate(&mut self, this: u32, rate: f32) {
        self.set_f32(this + HINGE_RATE, rate);
    }

    fn store_angle(&mut self, this: u32, angle: f32) {
        self.set_f32(this + HINGE_ANGLE, angle);
    }

    fn rate_axis(&self, this: u32) -> u32 {
        self.u32(this + HINGE_DEF) + DEF_HINGE_AXIS_RF
    }

    fn clear_linear_velocity(&mut self, this: u32) {
        Vec4::from([0.0; 4]).store(self, this + HINGE_LINEAR_VELOCITY);
    }

    /// The payload's reads: the key, the rotation block's vector (+0x100, or 0 when `this` is -0xf0) and the position
    /// (+0x140).
    fn state_payload(&self, this: u32) -> StatePayload {
        let orientation_at = if this.wrapping_add(0xF0) != 0 { this + HINGE_ORIENTATION } else { 0 };
        StatePayload {
            key: self.u32(this + HINGE_KEY),
            orientation: self.f32s(orientation_at),
            position: self.f32s(this + HINGE_POSITION),
        }
    }

    /// The key, the orientation at +0x20, then the position (`E01Vec4::CopyFrom`) at +0x14, which overwrites the
    /// orientation's first lane (+0x20). The padding words are not written, as the original does not write them.
    fn store_state_payload(&mut self, sig: u32, payload: &StatePayload) {
        self.set_u32(sig, payload.key);
        Vec4::from(payload.orientation).store(self, sig + 0x20);
        Vec4::from(payload.position).store(self, sig + 0x14);
    }

    fn state_signal(&self) -> u32 {
        physics_core::layout::signal_manager(self) + STATE_SIGNAL
    }
}
