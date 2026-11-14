//! Typed views of the rigid body's memory: the body (`CSolid`, 0x3d0 bytes) and its definition (`CSolid::CDef`,
//! 0x1f0 bytes, at the pointer stored at `CSolid+0x34`). Offsets are the original's
//! (rbr-recomp `notes/physics/rigid-body.md` 2a, 2b). `load` and `store` are the only code that knows them.

mod contacts;
mod mem_store;
mod rest;
mod scratch_state;
mod sleep_body;
mod solid_ctor;

pub use crate::physics_core::layout::vtable_target;
pub use crate::rigid_body::solid_contacts::{
    contact_vote, contact_worker, impulse_tail, ring_append, ContactOut, ImpulseTailCalls,
};
pub use crate::rigid_body::solid_rows::{
    body_fold_rows_by_columns, body_fold_rows_by_rows, body_rotate_rows, clamp_velocity_along, def_flag,
    fold_body_rows, fold_body_rows_reordered, point_velocity, quat_diff_body_rows, quat_plus_body_rows,
    quat_transform_body_rows, response_combine_at, rows_to_body, scale_add_rows, solid_inverse_mass, solid_response,
    transform_body_rows, transform_body_rows3,
};
pub use crate::rigid_body::solid_window::{
    refresh_and_reset, refresh_state_rotation, scratch_accumulate, solid_advance_rest, solid_back_step,
    solid_window_start,
};
pub(crate) use contacts::HALF;
pub use contacts::{
    contact_record, contact_records, impulse_tail_frame, load_contact, store_event, ImpulseTailFrame, CONTACT_LIST,
};
pub use rest::{solid_filing, solid_sleep, solid_soften, solid_wake, store_body_event, RestHost, VerySoftHost};
pub use scratch_state::{
    clear_vec, scratch_reset_after_entity, store_scratch_base_init, store_scratch_reset, walk_children, CELL_LINK,
    SCRATCH_VTABLE,
};
pub use sleep_body::{Body, Def, ParentFrame};
pub use solid_ctor::{
    construct_def, construct_solid, construct_state, new_def, new_solid, place_solid, PlaceHost, SolidCtorHost,
    DEF_SIZE, SOLID_SIZE,
};

/// The `CSolid` members the ports read and write (`notes/physics.md` "Rigid body", which marks the inferred ones).
pub const ORIENTATION: u32 = 0x100;
pub const POSITION: u32 = 0x140;
pub const ANGULAR_VELOCITY: u32 = 0x190;
pub const LINEAR_VELOCITY: u32 = 0x1c0;
pub const PREV_ANGULAR_VELOCITY: u32 = 0x200;
pub const PREV_LINEAR_VELOCITY: u32 = 0x220;
/// The pose at the window's start, which `VfDx` blends from (the notes' `pose_end`, their open question 8; the port
/// copies the current pose here when a window begins, `emit.rs`).
pub const START_ORIENTATION: u32 = 0x2a0;
pub const START_POSITION: u32 = 0x330;
pub const ANGULAR_ACCEL: u32 = 0x320;
pub const FORCE_ACCUM: u32 = 0x370;
/// The force input `Vf25x` scales by the inverse mass into a linear acceleration.
pub const LINEAR_ACCEL: u32 = 0x380;

/// `CEntity` members the body inherits (rbr-recomp `notes/physics/entities.md` 2b): the flags byte and the parent frame
/// pointer.
pub const ENTITY_FLAGS: u32 = 0x60;
pub const PARENT: u32 = 0x44;

/// More `CSolid` members the body's fields read (rbr-recomp `notes/physics/rigid-body.md` 2a, 2b).
pub const DEFINITION: u32 = 0x34;
pub const SLEEP_TIMER: u32 = 0x270;
pub const WINDOW_PROGRESS: u32 = 0x290;
/// The angular velocity at the window's start, `+0x2e0` (name ours).
pub const START_ANGULAR_VELOCITY: u32 = 0x2e0;
/// The linear velocity at the window's start, `+0x340` (name ours).
pub const START_LINEAR_VELOCITY: u32 = 0x340;
pub const SCRATCH_ACCUM: u32 = 0x310;
pub const SCRATCH_SCALE: u32 = 0xd0;
pub const ACCEL_LINEAR_EST: u32 = 0x230;
pub const ACCEL_ANGULAR_EST: u32 = 0x240;
pub const ACCEL_LINEAR_FILT: u32 = 0x250;
pub const ACCEL_ANGULAR_FILT: u32 = 0x260;
pub const STEP_FLAGS: u32 = 0x3b4;

/// `CSolid::CDef` members the ports read (rbr-recomp `notes/physics/rigid-body.md` 2a).
pub const DEF_LOCAL_INVERSE_INERTIA: u32 = 0x130;
pub const DEF_INVERSE_MASS: u32 = 0x16c;
pub const DEF_SLEEPABLE: u32 = 0x1ac;
pub const DEF_SLEEP_DELAY: u32 = 0x1bc;
pub const DEF_LINEAR_SLEEP_VELOCITY: u32 = 0x1cc;
pub const DEF_ANGULAR_SLEEP_VELOCITY: u32 = 0x1dc;
pub const DEF_VERY_SOFT: u32 = 0x1ec;

/// Bit 0x8 of the body flags (`+0x60`, bit 3; named by use, the meaning beyond these uses is open): the force pass runs
/// the contact list step (`BigB`), which retires kind-200 records on such a body; the integrator clears the slow
/// velocities; the pose step takes the drive-line sample; the contact worker's sample hook runs (with a hook); and the
/// placement wants a hook (`place_solid`).
pub const CONTACT_LIST_FLAG: u8 = 0x8;

/// A `E09Vec4`: four floats. The body's vectors keep the fourth word (`w`) in the object. The type is the shared
/// `crate::math::vec::Vec4`, re-exported so the body's views keep their paths.
pub use crate::math::vec::Vec4;
