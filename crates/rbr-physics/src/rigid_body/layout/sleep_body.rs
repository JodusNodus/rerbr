//! The body and definition fields the sleep step reads and writes, and the parent frame.

use super::contacts::{HOOK, RING_TAIL};
use super::mem_store::{quat, set_vec4, vec4, SolidWords};
use super::rest::{ACTIVE, VERY_SOFT};
use super::Solid;
use super::{
    ACCEL_ANGULAR_EST, ACCEL_ANGULAR_FILT, ACCEL_LINEAR_EST, ACCEL_LINEAR_FILT, ANGULAR_ACCEL, ANGULAR_VELOCITY,
    DEF_ANGULAR_SLEEP_VELOCITY, DEF_INVERSE_MASS, DEF_LINEAR_SLEEP_VELOCITY, DEF_LOCAL_INVERSE_INERTIA, DEF_SLEEPABLE,
    DEF_SLEEP_DELAY, ENTITY_FLAGS, FORCE_ACCUM, LINEAR_ACCEL, LINEAR_VELOCITY, ORIENTATION, PARENT, POSITION,
    PREV_ANGULAR_VELOCITY, PREV_LINEAR_VELOCITY, SCRATCH_ACCUM, SCRATCH_SCALE, SLEEP_TIMER, START_ANGULAR_VELOCITY,
    START_LINEAR_VELOCITY, START_ORIENTATION, START_POSITION, STEP_FLAGS, WINDOW_PROGRESS,
};
use crate::math::quat::Quat;
use crate::math::vec::Vec4;
use crate::mem::Mem;

/// The parent frame a body is attached to (`CSolid+0x44` non-null). Only the flag the pose step reads is here: bit
/// 0x4 of the parent's flags byte at +0x60 makes the parent transform the child's position. The asm tests `0x60(%ecx)`
/// (`testb $0x4`); the C++ transcription reads `pi + 6` (+0x18), which the asm does not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParentFrame {
    pub updates_child: bool,
}

impl ParentFrame {
    /// Reads the parent at `parent` (the non-null pointer at `CSolid+0x44`).
    pub fn load(mem: &Mem, parent: u32) -> Self {
        ParentFrame { updates_child: mem.u8(parent + ENTITY_FLAGS) & 0x4 != 0 }
    }
}

/// The definition's sleep settings (the keys `Sleepable`, `SleepDelay`, `LinearSleepVelocity`,
/// `AngularSleepVelocity`), and the mass terms the force pass reads (`InverseMass`, the inverse inertia diagonal).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Def {
    /// Whether the body may sleep (`Sleepable`, a flag word). 0 = never.
    pub sleepable: u32,
    /// Seconds below both thresholds before the body sleeps (`SleepDelay`).
    pub sleep_delay: f32,
    /// Linear speed threshold, m/s (`LinearSleepVelocity`). Compared squared.
    pub linear_sleep_velocity: f32,
    /// Angular speed threshold, rad/s (`AngularSleepVelocity`). Compared squared.
    pub angular_sleep_velocity: f32,
    /// Inverse mass (`InverseMass`, `def+0x16c`): the force pass scales the force by it.
    pub inverse_mass: f32,
    /// Local inverse inertia diagonal (`vecLocalInverseInertiaDiagonal`, `def+0x130`): the torque is scaled by it.
    pub local_inverse_inertia: Vec4,
}

impl Def {
    /// Reads the sleep settings of the definition at `at`.
    pub fn load(mem: &Mem, at: u32) -> Self {
        Def {
            sleepable: mem.u32(at + DEF_SLEEPABLE),
            sleep_delay: mem.f32(at + DEF_SLEEP_DELAY),
            linear_sleep_velocity: mem.f32(at + DEF_LINEAR_SLEEP_VELOCITY),
            angular_sleep_velocity: mem.f32(at + DEF_ANGULAR_SLEEP_VELOCITY),
            inverse_mass: mem.f32(at + DEF_INVERSE_MASS),
            local_inverse_inertia: Vec4::load(mem, at + DEF_LOCAL_INVERSE_INERTIA),
        }
    }
}

/// The body's fields the sleep step reads and writes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Body {
    /// Body linear velocity, m/s (`CSolid+0x1c0`).
    pub linear_velocity: Vec4,
    /// Body angular velocity, rad/s (`CSolid+0x190`).
    pub angular_velocity: Vec4,
    /// Time below the sleep thresholds, s (`CSolid+0x270`).
    pub sleep_timer: f32,
    /// Non-zero when the body may sleep at its delay (`CSolid+0x274`).
    pub sleep_armed: u32,
    /// The body is in the active set (`IsActive`, `CSolid+0x1dc`). Cleared when it sleeps.
    pub is_active: u32,
    /// Orientation quaternion, unit length after each pose step (`CSolid+0x100`).
    pub orientation: Quat,
    /// World position (`CSolid+0x140`).
    pub position: Vec4,
    /// Fraction of the current window already taken (`CSolid+0x290`). The window steps move the body to it.
    pub progress: f32,
    /// Orientation at the start of the window (`CSolid+0x2a0`, the back step blends with it; name is ours).
    pub start_orientation: Quat,
    /// Position at the start of the window (`CSolid+0x330`, the back step blends with it; name is ours).
    pub start_position: Vec4,
    /// Linear and angular velocity at the start of the window (`CSolid+0x340` and `+0x2e0`; `CSolid::S18` copies them
    /// with the pose; names are ours).
    pub start_linear_velocity: Vec4,
    pub start_angular_velocity: Vec4,
    /// Body flags byte (`CSolid+0x60`): [`CONTACT_LIST_FLAG`](super::CONTACT_LIST_FLAG) (0x8) takes the drive-line
    /// sample in the pose step.
    pub body_flags: u8,
    /// Parent frame pointer (`CSolid+0x44`), 0 when the body is free.
    pub parent: u32,
    /// Step flags (`CSolid+0x3b4`): bit 0 is set when the pose step gets a negative time.
    pub step_flags: u32,
    /// The contact sample the pose step stores (`CSolid+0x17c`). Its meaning is unknown: it is the value
    /// `CDriveLine::SampleBig` returns at the body's position.
    pub sampled: f32,
    /// Dirty flag of the tweak at `CSolid+0x170` (`CBase::m_dirty`, `+0x178`). The pose step sets it with the sample.
    pub sampled_dirty: u32,
    /// Scratch accumulator `E43Vf74` adds to (`CSolid+0x310`). The C++ comment calls it `m_v90`; the offset is the
    /// evidence.
    pub scratch: Vec4,
    /// The scale `E43Vf74` multiplies its vector by (`CSolid+0xd0`, `m_fD0` in the C++).
    pub scratch_scale: f32,
    /// Angular acceleration (`CSolid+0x320`). `Vf25x` writes it from the torque; the integrator adds it to the angular
    /// velocity times `dt`.
    pub angular_accel: Vec4,
    /// Linear force input (`CSolid+0x380`). `Vf25x` scales it by the inverse mass; the integrator adds it to the linear
    /// velocity times `dt`.
    pub linear_accel: Vec4,
    /// Angular velocity at the last integrator pass (`CSolid+0x200`, the data of a `CVector` object at `+0x1f0`).
    pub prev_angular_velocity: Vec4,
    /// Linear velocity at the last integrator pass (`CSolid+0x220`, the data of a `CVector` object at `+0x210`).
    pub prev_linear_velocity: Vec4,
    /// Linear acceleration estimate, `(v - prev_linear_velocity) / dt` (`CSolid+0x230`).
    pub accel_linear_est: Vec4,
    /// Angular acceleration estimate, `(w - prev_angular_velocity) / dt` (`CSolid+0x240`).
    pub accel_angular_est: Vec4,
    /// Filtered linear acceleration, `0.2 * old + 0.8 * estimate` (`CSolid+0x250`).
    pub accel_linear_filt: Vec4,
    /// Filtered angular acceleration, `0.2 * old + 0.8 * estimate` (`CSolid+0x260`).
    pub accel_angular_filt: Vec4,
    /// The body's pointer at `CSolid+0x30`, the first word of the state signal the integrator emits. Its meaning is
    /// open.
    pub state_owner: u32,
    /// Force accumulator (`CSolid+0x370`): `E46_Accum340` (slot 28) adds `v * scratch_scale` to it. The force pass
    /// scales it by the inverse mass into `linear_accel` and clears it.
    pub force: Vec4,
    /// Dirty word of the tweak at `CSolid+0x1d0` (`+0x1d8`, `CBase::m_dirty`). `E43Vf90` sets it with `SetDirty`.
    pub tweak_a_dirty: u32,
    /// Dirty word of the tweak at `CSolid+0x1e0` (`+0x1e8`). `E43Vf90` sets it with `SetDirty`.
    pub tweak_b_dirty: u32,
    /// The value of the tweak at `CSolid+0x1e0` (`+0x1ec`), the key `VerySoft`. No reader in the E47 code; `E43Vf90`
    /// clears it.
    pub very_soft: u32,
    /// The hook object at `CSolid+0x3cc` (`E47S01_8` for `E43Vf128`, the sample sink for `BigA`). Zero when absent.
    pub hook: u32,
    /// The ring tail at `CSolid+0x3c8`, which `E43Vf128` appends to when the global's bit is set. Zero when absent.
    pub ring_tail: u32,
}

impl Body {
    /// Reads the body at `this`. The memory must cover the object (0x3d0 bytes).
    pub fn load(mem: &Mem, this: u32) -> Self {
        Self::read(mem, this)
    }

    /// Writes the fields back. Only the fields the ports change need to be stored; this writes all of them.
    pub fn store(&self, mem: &mut Mem, this: u32) {
        self.write(mem, this)
    }

    /// [`Body::load`] on a rigid store's words (a held solid's typed state where it holds the word).
    pub fn read<W: SolidWords + ?Sized>(w: &W, this: u32) -> Self {
        if let Some(mem) = w.image_ref() {
            return Self::read_words(mem, this);
        }
        match w.held(this) {
            Some(solid) => Self::from_held(&solid.borrow(), w.mem(), this),
            None => Self::read_words(w, this),
        }
    }

    /// [`Body::read_words`] with the solid held: its typed state's words at once, the rest the image's.
    fn from_held(s: &Solid, mem: &Mem, this: u32) -> Self {
        let quat = |[x, y, z, w]: [f32; 4]| Quat { x, y, z, w };
        Body {
            linear_velocity: s.linear_velocity.into(),
            angular_velocity: s.angular_velocity.into(),
            sleep_timer: s.sleep_timer,
            sleep_armed: s.sleep_armed,
            is_active: s.is_active,
            orientation: quat(s.orientation),
            position: s.position.into(),
            progress: s.progress,
            start_orientation: quat(s.start_orientation),
            start_position: s.start_position.into(),
            start_linear_velocity: s.start_linear_velocity.into(),
            start_angular_velocity: s.start_angular_velocity.into(),
            body_flags: mem.u8(this + ENTITY_FLAGS),
            parent: mem.u32(this + PARENT),
            step_flags: s.step_flags,
            sampled: s.sampled,
            sampled_dirty: mem.u32(this + 0x178),
            scratch: s.scratch.into(),
            scratch_scale: mem.f32(this + SCRATCH_SCALE),
            angular_accel: s.angular_accel.into(),
            linear_accel: s.linear_accel.into(),
            prev_angular_velocity: s.prev_angular_velocity.into(),
            prev_linear_velocity: s.prev_linear_velocity.into(),
            accel_linear_est: s.accel_linear_est.into(),
            accel_angular_est: s.accel_angular_est.into(),
            accel_linear_filt: s.accel_linear_filt.into(),
            accel_angular_filt: s.accel_angular_filt.into(),
            state_owner: mem.u32(this + 0x30),
            force: s.force.into(),
            tweak_a_dirty: mem.u32(this + 0x1D8),
            tweak_b_dirty: mem.u32(this + 0x1E8),
            very_soft: s.very_soft,
            hook: mem.u32(this + HOOK),
            ring_tail: mem.u32(this + RING_TAIL),
        }
    }

    fn read_words<W: SolidWords + ?Sized>(w: &W, this: u32) -> Self {
        Body {
            linear_velocity: vec4(w, this + LINEAR_VELOCITY),
            angular_velocity: vec4(w, this + ANGULAR_VELOCITY),
            sleep_timer: f32::from_bits(w.word(this + SLEEP_TIMER)),
            sleep_armed: w.word(this + 0x274),
            is_active: w.word(this + ACTIVE),
            orientation: quat(w, this + ORIENTATION),
            position: vec4(w, this + POSITION),
            progress: f32::from_bits(w.word(this + WINDOW_PROGRESS)),
            start_orientation: quat(w, this + START_ORIENTATION),
            start_position: vec4(w, this + START_POSITION),
            start_linear_velocity: vec4(w, this + START_LINEAR_VELOCITY),
            start_angular_velocity: vec4(w, this + START_ANGULAR_VELOCITY),
            body_flags: w.u8(this + ENTITY_FLAGS),
            parent: w.u32(this + PARENT),
            step_flags: w.word(this + STEP_FLAGS),
            sampled: f32::from_bits(w.word(this + 0x17C)),
            sampled_dirty: w.u32(this + 0x178),
            scratch: vec4(w, this + SCRATCH_ACCUM),
            scratch_scale: f32::from_bits(w.word(this + SCRATCH_SCALE)),
            angular_accel: vec4(w, this + ANGULAR_ACCEL),
            linear_accel: vec4(w, this + LINEAR_ACCEL),
            prev_angular_velocity: vec4(w, this + PREV_ANGULAR_VELOCITY),
            prev_linear_velocity: vec4(w, this + PREV_LINEAR_VELOCITY),
            accel_linear_est: vec4(w, this + ACCEL_LINEAR_EST),
            accel_angular_est: vec4(w, this + ACCEL_ANGULAR_EST),
            accel_linear_filt: vec4(w, this + ACCEL_LINEAR_FILT),
            accel_angular_filt: vec4(w, this + ACCEL_ANGULAR_FILT),
            state_owner: w.u32(this + 0x30),
            force: vec4(w, this + FORCE_ACCUM),
            tweak_a_dirty: w.u32(this + 0x1D8),
            tweak_b_dirty: w.u32(this + 0x1E8),
            very_soft: w.word(this + VERY_SOFT),
            hook: w.u32(this + HOOK),
            ring_tail: w.u32(this + RING_TAIL),
        }
    }

    /// [`Body::store`] on a rigid store's words, in the same order.
    pub fn write<W: SolidWords + ?Sized>(&self, w: &mut W, this: u32) {
        if let Some(mem) = w.image() {
            return self.write_words(mem, this);
        }
        match w.held(this).map(|solid| self.store_held(&mut solid.borrow_mut())) {
            Some(()) => self.store_unheld(w.mem_mut(), this),
            None => self.write_words(w, this),
        }
    }

    /// [`Body::write_words`]' words a held solid's typed state holds.
    fn store_held(&self, s: &mut Solid) {
        let vec = |v: Vec4| [v.x, v.y, v.z, v.w];
        let quat = |q: Quat| [q.x, q.y, q.z, q.w];
        s.linear_velocity = vec(self.linear_velocity);
        s.angular_velocity = vec(self.angular_velocity);
        s.sleep_timer = self.sleep_timer;
        s.sleep_armed = self.sleep_armed;
        s.is_active = self.is_active;
        s.orientation = quat(self.orientation);
        s.position = vec(self.position);
        s.progress = self.progress;
        s.start_orientation = quat(self.start_orientation);
        s.start_position = vec(self.start_position);
        s.start_linear_velocity = vec(self.start_linear_velocity);
        s.start_angular_velocity = vec(self.start_angular_velocity);
        s.step_flags = self.step_flags;
        s.sampled = self.sampled;
        s.scratch = vec(self.scratch);
        s.angular_accel = vec(self.angular_accel);
        s.linear_accel = vec(self.linear_accel);
        s.prev_angular_velocity = vec(self.prev_angular_velocity);
        s.prev_linear_velocity = vec(self.prev_linear_velocity);
        s.accel_linear_est = vec(self.accel_linear_est);
        s.accel_angular_est = vec(self.accel_angular_est);
        s.accel_linear_filt = vec(self.accel_linear_filt);
        s.accel_angular_filt = vec(self.accel_angular_filt);
        s.force = vec(self.force);
        s.very_soft = self.very_soft;
    }

    /// [`Body::write_words`]' other words, on the image.
    fn store_unheld(&self, mem: &mut Mem, this: u32) {
        mem.map(this + ENTITY_FLAGS, &[self.body_flags]);
        mem.set_u32(this + 0x178, self.sampled_dirty);
        mem.set_u32(this + 0x30, self.state_owner);
        mem.set_u32(this + 0x1D8, self.tweak_a_dirty);
        mem.set_u32(this + 0x1E8, self.tweak_b_dirty);
        mem.set_u32(this + HOOK, self.hook);
        mem.set_u32(this + RING_TAIL, self.ring_tail);
    }

    fn write_words<W: SolidWords + ?Sized>(&self, w: &mut W, this: u32) {
        set_vec4(w, this + LINEAR_VELOCITY, self.linear_velocity);
        set_vec4(w, this + ANGULAR_VELOCITY, self.angular_velocity);
        w.set_word(this + SLEEP_TIMER, self.sleep_timer.to_bits());
        w.set_word(this + 0x274, self.sleep_armed);
        w.set_word(this + ACTIVE, self.is_active);
        set_quat(w, this + ORIENTATION, self.orientation);
        set_vec4(w, this + POSITION, self.position);
        w.set_word(this + WINDOW_PROGRESS, self.progress.to_bits());
        set_quat(w, this + START_ORIENTATION, self.start_orientation);
        set_vec4(w, this + START_POSITION, self.start_position);
        set_vec4(w, this + START_LINEAR_VELOCITY, self.start_linear_velocity);
        set_vec4(w, this + START_ANGULAR_VELOCITY, self.start_angular_velocity);
        w.mem_mut().map(this + ENTITY_FLAGS, &[self.body_flags]);
        w.set_word(this + STEP_FLAGS, self.step_flags);
        w.set_word(this + 0x17C, self.sampled.to_bits());
        w.set_u32(this + 0x178, self.sampled_dirty);
        set_vec4(w, this + SCRATCH_ACCUM, self.scratch);
        set_vec4(w, this + ANGULAR_ACCEL, self.angular_accel);
        set_vec4(w, this + LINEAR_ACCEL, self.linear_accel);
        set_vec4(w, this + PREV_ANGULAR_VELOCITY, self.prev_angular_velocity);
        set_vec4(w, this + PREV_LINEAR_VELOCITY, self.prev_linear_velocity);
        set_vec4(w, this + ACCEL_LINEAR_EST, self.accel_linear_est);
        set_vec4(w, this + ACCEL_ANGULAR_EST, self.accel_angular_est);
        set_vec4(w, this + ACCEL_LINEAR_FILT, self.accel_linear_filt);
        set_vec4(w, this + ACCEL_ANGULAR_FILT, self.accel_angular_filt);
        w.set_u32(this + 0x30, self.state_owner);
        set_vec4(w, this + FORCE_ACCUM, self.force);
        w.set_u32(this + 0x1D8, self.tweak_a_dirty);
        w.set_u32(this + 0x1E8, self.tweak_b_dirty);
        w.set_word(this + VERY_SOFT, self.very_soft);
        w.set_u32(this + HOOK, self.hook);
        w.set_u32(this + RING_TAIL, self.ring_tail);
    }
}

fn set_quat<W: SolidWords + ?Sized>(w: &mut W, at: u32, q: Quat) {
    set_vec4(w, at, Vec4 { x: q.x, y: q.y, z: q.z, w: q.w });
}
