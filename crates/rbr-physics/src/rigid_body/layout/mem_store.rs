//! The rigid body's store: the layout's reads and writes behind `RigidStore`, on every [`SolidWords`] (`Mem`'s on the
//! image, per call; `SolidView`'s on the held solids, `typed.rs`). The solids' words go through `SolidWords`; the
//! settings, the rings and the pair are the image's.

use super::contacts::{self, HALF, HOOK, RING_TAIL};

use super::solid_ctor::SOLID_STATE;
use super::{
    Body, Def, ParentFrame, Vec4, ANGULAR_VELOCITY, CONTACT_LIST_FLAG, DEFINITION, DEF_INVERSE_MASS, DEF_VERY_SOFT,
    ENTITY_FLAGS, LINEAR_VELOCITY, POSITION, SCRATCH_ACCUM, SCRATCH_SCALE, START_ORIENTATION, START_POSITION,
};
use crate::math::quat::Quat;
use crate::math::vec::Vec3;
use crate::mem::Mem;
use crate::physics_core::math::Mat;
use crate::rigid_body::contact::{Contact, Input};
use crate::rigid_body::rows::RowFrame;
use crate::rigid_body::store::{HookPair, Motion, RigidStore, RingHeader, StateRotation};
use crate::store::Raw;

/// The physics global (`g_1659b70`), whose `+0x24` is the settings object.
const PHYSICS_GLOBAL: u32 = 0x1659b70;
/// The settings' `MaxTimeStep`, and the two limits the velocity clamp reads.
const MAX_TIME_STEP: u32 = 0x1608C;
const CLAMP_LIMIT: u32 = 0x1610c;
const CLAMP_SCALE: u32 = 0x1611c;

/// The body's matrix, ten words from `+0x110` (the state's matrix at `+0x30`).
const MATRIX: u32 = 0x110;

/// The state's orientation, scale diagonal and gate, and where its refreshed matrix goes.
const STATE_ORIENTATION: u32 = 0x20;
const STATE_DIAGONAL: u32 = 0x70;
const STATE_GATE: u32 = 0x7c;
const STATE_MATRIX: u32 = 0x30;

/// The solids' words as a rigid store finds them: the image's (`Mem`, per call) or, for a solid held in typed storage,
/// its typed state's (`SolidView`). Every other word (pointers, flags, settings, rings) is read on the image.
pub trait SolidWords: Raw {
    #[inline]
    fn word(&self, at: u32) -> u32 {
        self.mem().u32(at)
    }
    #[inline]
    fn set_word(&mut self, at: u32, value: u32) {
        self.mem_mut().set_u32(at, value)
    }
}

impl SolidWords for Mem {}

#[inline]
fn f<W: SolidWords + ?Sized>(w: &W, at: u32) -> f32 {
    f32::from_bits(w.word(at))
}

#[inline]
fn set_f<W: SolidWords + ?Sized>(w: &mut W, at: u32, value: f32) {
    w.set_word(at, value.to_bits())
}

#[inline]
pub(super) fn floats<const N: usize, W: SolidWords + ?Sized>(w: &W, at: u32) -> [f32; N] {
    std::array::from_fn(|k| f(w, at + 4 * k as u32))
}

#[inline]
pub(super) fn vec4<W: SolidWords + ?Sized>(w: &W, at: u32) -> Vec4 {
    Vec4::from(floats::<4, W>(w, at))
}

#[inline]
pub(super) fn set_vec4<W: SolidWords + ?Sized>(w: &mut W, at: u32, v: Vec4) {
    for (k, x) in [v.x, v.y, v.z, v.w].into_iter().enumerate() {
        set_f(w, at + 4 * k as u32, x);
    }
}

#[inline]
fn vec3<W: SolidWords + ?Sized>(w: &W, at: u32) -> Vec3 {
    let [x, y, z] = floats(w, at);
    Vec3 { x, y, z }
}

#[inline]
pub(super) fn quat<W: SolidWords + ?Sized>(w: &W, at: u32) -> Quat {
    let [x, y, z, w] = floats(w, at);
    Quat { x, y, z, w }
}

fn settings(mem: &Mem) -> u32 {
    mem.u32(mem.u32(PHYSICS_GLOBAL) + 0x24)
}

impl<W: SolidWords> RigidStore for W {
    fn body(&self, this: u32) -> Body {
        Body::read(self, this)
    }

    fn store_body(&mut self, this: u32, body: &Body) {
        body.write(self, this);
    }

    fn definition(&self, this: u32) -> Def {
        Def::load(self.mem(), self.u32(this + DEFINITION))
    }

    fn parent_frame(&self, parent: u32) -> ParentFrame {
        ParentFrame::load(self.mem(), parent)
    }

    fn def_very_soft(&self, this: u32) -> bool {
        self.u32(self.u32(this + DEFINITION) + DEF_VERY_SOFT) != 0
    }

    fn inverse_mass(&self, this: u32) -> f32 {
        let definition = self.u32(this + DEFINITION);
        self.f32(definition + DEF_INVERSE_MASS)
    }

    fn row(&self, at: u32) -> Vec4 {
        vec4(self, at)
    }

    fn store_row(&mut self, at: u32, row: Vec4) {
        set_vec4(self, at, row);
    }

    fn store_row3(&mut self, at: u32, row: Vec4) {
        set_f(self, at, row.x);
        set_f(self, at + 4, row.y);
        set_f(self, at + 8, row.z);
    }

    fn vec3(&self, at: u32) -> Vec3 {
        vec3(self, at)
    }

    fn store_vec3(&mut self, at: u32, value: Vec3) {
        set_f(self, at, value.x);
        set_f(self, at + 4, value.y);
        set_f(self, at + 8, value.z);
    }

    fn row_frame(&self, this: u32) -> RowFrame {
        RowFrame { m: floats(self, this + MATRIX), origin: vec3(self, this + POSITION).extend(0.0) }
    }

    fn matrix_words(&self, this: u32) -> [f32; 9] {
        floats(self, this + MATRIX)
    }

    fn matrix(&self, this: u32) -> Mat {
        let at = this + MATRIX;
        Mat { rows: std::array::from_fn(|r| floats(self, at + 12 * r as u32)), extra: f(self, at + 36) }
    }

    fn position(&self, this: u32) -> Vec4 {
        vec4(self, this + POSITION)
    }

    fn motion(&self, this: u32) -> Motion {
        Motion {
            position: vec3(self, this + POSITION),
            angular_velocity: vec3(self, this + ANGULAR_VELOCITY),
            linear_velocity: vec3(self, this + LINEAR_VELOCITY),
        }
    }

    fn linear_velocity(&self, this: u32) -> Vec4 {
        vec4(self, this + LINEAR_VELOCITY)
    }

    fn store_linear_velocity(&mut self, this: u32, value: Vec4) {
        set_vec4(self, this + LINEAR_VELOCITY, value);
    }

    fn start_orientation(&self, this: u32) -> Quat {
        quat(self, this + START_ORIENTATION)
    }

    fn start_offset(&self, this: u32) -> Vec4 {
        vec3(self, this + START_POSITION).extend(0.0)
    }

    fn impulse_limits(&self) -> (f32, f32) {
        let settings = settings(self.mem());
        (self.f32(settings + CLAMP_LIMIT), self.f32(settings + CLAMP_SCALE))
    }

    fn contact_input(&self, this: u32, contact: Contact, range: f32) -> Input {
        Input {
            contact,
            inverse_mass: RigidStore::inverse_mass(self, this),
            max_step: self.f32(settings(self.mem()) + MAX_TIME_STEP),
            range,
            sample: self.u32(this + HOOK) != 0 && self.u8(this + ENTITY_FLAGS) & CONTACT_LIST_FLAG != 0,
        }
    }

    fn linear_velocity_at(&self, this: u32) -> u32 {
        this + LINEAR_VELOCITY
    }

    fn state_at(&self, this: u32) -> u32 {
        this + SOLID_STATE
    }

    fn hook_pair(&self, pair: u32) -> HookPair {
        HookPair { third: self.f32(pair + 8), half: self.f32(HALF), first: self.u32(pair), second: self.u32(pair + 4) }
    }

    fn hook(&self, this: u32) -> u32 {
        self.u32(this + HOOK)
    }

    fn ring_tail(&self, this: u32) -> u32 {
        self.u32(this + RING_TAIL)
    }

    fn ring_header(&self, ring: u32) -> RingHeader {
        contacts::ring_header(self.mem(), ring)
    }

    fn store_ring_entry(&mut self, ring: u32, index: u32, words: [u32; 3], src: u32) {
        let vector: [u32; 4] = std::array::from_fn(|k| self.word(src + 4 * k as u32));
        contacts::store_ring_entry(self.mem_mut(), ring, index, words, vector);
    }

    fn store_ring_cursor(&mut self, ring: u32, index: u32, count: u32) {
        contacts::store_ring_cursor(self.mem_mut(), ring, index, count);
    }

    fn scratch_scale(&self, this: u32) -> f32 {
        f(self, this + SCRATCH_SCALE)
    }

    fn scratch_accum(&self, this: u32) -> Vec4 {
        vec4(self, this + SCRATCH_ACCUM)
    }

    fn store_scratch_accum(&mut self, this: u32, value: Vec4) {
        set_vec4(self, this + SCRATCH_ACCUM, value);
    }

    fn state_rotation(&self, state: u32) -> StateRotation {
        StateRotation {
            orientation: quat(self, state + STATE_ORIENTATION),
            gate: f(self, state + STATE_GATE),
            diagonal: floats(self, state + STATE_DIAGONAL),
        }
    }

    fn store_state_matrix(&mut self, state: u32, matrix: [f32; 9]) {
        for (k, v) in matrix.into_iter().enumerate() {
            set_f(self, state + STATE_MATRIX + 4 * k as u32, v);
        }
    }
}
