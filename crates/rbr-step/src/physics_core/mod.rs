//! Bindings of the physics core (`rbr_physics::physics_core`, exe 0x5224f0-0x5293d0 mostly): the passenger's step,
//! `CPhysics::StepForward` and its passes, the detectors, the mounted body and the small forwarding slots.

use crate::entities::{empty_hook_run, EMPTY_HOOK, STATE_EMIT};
use crate::Binding;
use rbr_physics::mem::Mem;
use rbr_physics::physics_core::layout::typed::Slot;
use rbr_physics::physics_core::layout::CoreView;
use rbr_physics::store::Value;

mod detection;
mod mounted_body;
mod passenger_step;
mod slots;
mod step_forward;

use crate::collision::E37_EDGE_EDGE;
use crate::rigid_body::{BODY_EVENT_EMIT, E43VF64, E43VF6C, LOOKUP_TWIN, WALKER};
pub(crate) use detection::detector_vf04;
use detection::{
    collide_window, e46_l2_vf04, giant_vf04, kernel_5bd0, l1_collision_vf04, l1_pen_vf04, penetration_check,
    penetration_ctor, penetration_vf04, run_collision_detectors, visit_list470, PENETRATION_CTOR, VISIT_470,
};
use mounted_body::{mounted_pose_update, mounted_set_state, mounted_vf64, mounted_vf6c};
use passenger_step::{pull_run, spring_run, step_run};
pub(crate) use slots::dispatch_vf5c;
use slots::{
    accumulate_forces, entity_vf00, entity_vf158, entity_vf88, flag_is_one, flag_is_two, m40c0, m46a0, mat_mul,
    reg_wall, unlink_a, vf80, vf8c,
};
pub(crate) use step_forward::scope;
use step_forward::{advance_contact_bodies, advance_contact_bodies_remainder, chan_enter, prof_exit, step_forward_run};

pub(crate) const PASSENGER_STEP: u32 = 0x523160;
const SPRING: u32 = 0x5229e0;
const PULL: u32 = 0x522bc0;
const MOUNTED_VF64: u32 = 0x521740;
pub(crate) const MOUNTED_VF6C: u32 = 0x521d90;
const WORLD_COMPOSE: u32 = 0x4e6950; // CCar::WorldCompose (thiscall: the car; point, out)
const UNLINK: u32 = 0x47d160;
/// The passenger's host pointer (+0x14), and the spring's last vector store (the host's +0x70), the eax it leaves.
const HOST_AT: u32 = 0x14;
const SPRING_EAX_AT: u32 = 0x70;
/// The temporaries the walk's point takes, below the entry's return slot (as `rigid_body`'s).
const TEMPS: u32 = 0x100;

/// Runs `run` on the physics core's store: the typed storage's core and the car's solids while the host holds them
/// over a step, else every word on the image (the per-call path).
pub(crate) fn on_core<T>(ctx: &mut crate::Ctx, run: impl FnOnce(&mut CoreView) -> T) -> T {
    let host = ctx.host;
    let t = host.typed();
    run(&mut CoreView::new(&mut ctx.mem, Some(&t.core)).with_solids(Some(&t.solids)))
}

/// The core's field `slot` of the object at `base` as a step leaves it at this point: the typed storage's while the
/// host holds it, else the image's. For the readers outside the core's bindings (the time keeper's update).
pub(crate) fn read_core<T: Value>(host: &dyn crate::Host, mem: &Mem, base: u32, slot: Slot<T>) -> T {
    host.typed().core.read(base, slot).unwrap_or_else(|| T::load(mem, base + slot.at))
}

const fn bind(addr: u32, name: &'static str, run: crate::BindingFn) -> Binding {
    crate::bind("physics_core", addr, name, run)
}

pub static BINDINGS: [Binding; 38] = [
    bind(SPRING, "E45PSmalls::M29E0", spring_run),
    bind(PULL, "E45PSmalls::M2BC0", pull_run),
    bind(PASSENGER_STEP, "CPassenger::Step", step_run),
    bind(0x5224f0, "E45_P24F0", empty_hook_run),
    bind(0x528690, "ProfExit", prof_exit),
    bind(0x529240, "CSolid::E43Vf5C", dispatch_vf5c),
    bind(0x5293d0, "CSolid::E43Vf80", vf80),
    bind(0x5293e0, "CSolid::E43Vf8C", vf8c),
    bind(0x519a40, "CEntity::E43Vf88", entity_vf88),
    bind(0x519950, "CEntity::Vf00", entity_vf00),
    bind(0x519aa0, "CEntity::E43Vf158", entity_vf158),
    bind(0x4e41a0, "E34Link10::UnlinkA", unlink_a),
    bind(0x5263d0, "CPhysics::CState::RegWall", reg_wall),
    bind(0x521920, "E45_M1920", flag_is_one),
    bind(0x521940, "E45_M1940", flag_is_two),
    bind(MOUNTED_VF64, "CMountedRigidBody::Vf64", mounted_vf64),
    bind(MOUNTED_VF6C, "CMountedRigidBody::Vf6c", mounted_vf6c),
    bind(0x5240c0, "E45_M40C0", m40c0),
    bind(0x5242c0, "CL2GiganticPenetrationDetector::Vf04", giant_vf04),
    bind(0x524480, "CL2PenetrationDetector::Vf04", penetration_vf04),
    bind(0x5246a0, "E45_M46A0", m46a0),
    bind(0x525a20, "E46L1PenDetector::Vf04", l1_pen_vf04),
    bind(0x525bd0, "E46_Kernel5bd0", kernel_5bd0),
    bind(0x5257e0, "E46L2Detector::Vf04", e46_l2_vf04),
    bind(0x526ad0, "CL1CollisionDetector::Vf04", l1_collision_vf04),
    bind(0x526cc0, "CPhysics::PenCheck", penetration_check),
    bind(0x526d20, "CPhysics::CollideWork", collide_window),
    bind(0x5273a0, "CPhysics::StepForward", step_forward_run),
    bind(0x528880, "E46ProfChan::Enter", chan_enter),
    bind(0x528c20, "E46Mat::Mul", mat_mul),
    bind(0x529250, "CSolid::E43Vf60", accumulate_forces),
    bind(PENETRATION_CTOR, "CL2PenetrationDetector::Ctor", penetration_ctor),
    bind(VISIT_470, "CPhysics::Visit470", visit_list470),
    bind(0x526a70, "CPhysics::Work6a70", run_collision_detectors),
    bind(0x525640, "CPhysics::Work5640", advance_contact_bodies),
    bind(0x525760, "CPhysics::Work5760", advance_contact_bodies_remainder),
    bind(0x521910, "E45Smalls::M1910", mounted_set_state),
    bind(0x521980, "E45_M1980", mounted_pose_update),
];
