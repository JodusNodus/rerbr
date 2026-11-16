//! Bindings of the collision ports (`rbr_physics::collision`, exe 0x4e0000-0x4fe000), with the original's
//! ABIs.

use rbr_physics::collision::layout::{CollisionImage, CollisionView};
use rbr_physics::mem::Mem;
use rbr_physics::rigid_body::contact::Contact;

use crate::{Binding, Host};

mod bodies;
mod brake_wall;
mod contacts;
mod edge;
mod queries;
mod solver;
mod vectors;

use crate::entities::{ENTITY_VF118, NODE_OF, SAMPLE_INDEXED};
pub(crate) use bodies::reset_state_at;
pub(crate) use bodies::{
    body_box_direct, body_vf_cc_direct, box_cells_direct, cell_word, held_cell_link, on_cell_links,
    register_cell_direct, BodyCallees, VfCcCorners, BODY_VF110, BODY_VF11C,
};
use bodies::{
    body_grid, body_grid_active, body_has_grid, body_link_unlink, body_vf11c, body_vf40, body_vf_cc, lookup_twin,
    move_to_cell, notify_filtered, state_unka44,
};
use brake_wall::{
    brake_wall_emit, brake_wall_is_active, brake_wall_vf_e8, brake_wall_vf_f0, raycast_entry, wall_vf_cc,
};
use contacts::{body_vf_d0, body_vf_dc};
pub(crate) use contacts::{body_vf_d8, body_vf_e8, body_vf_f4};
use edge::edge_edge;
use queries::{
    collect, face_query, grid_add_edge, grid_ops_m6610, grid_ray, grid_sample_a, grid_sample_b, grid_sync,
    mesh_copy_bounds, mesh_is_active, ray_clear, ray_seeds, seg_test, segment_sweep, segment_test, system_vf00,
};
pub(crate) use queries::{ray_clear_at, ray_seeds_at};
use solver::{
    alloc_event, apply_event_impulse, apply_offsets, clear_contact_rows, contact_rows, correct_penetrations,
    drain_events, event_count, event_query_bodies, init_full, min_contact_time, plane_diff, refresh_events,
    solve_due_events, solve_event, solver_add_event, E37_PLANE_DIFF,
};
pub(crate) use vectors::{copy_vec4, length};
use vectors::{float3_length, float3_scale, min_max_a, vec4_clear, vec4_copy_from, vec4_normalize, vec_cross};

pub(crate) const RAY_CLEAR: u32 = 0x4f6740;
pub(crate) const RAY_SEEDS: u32 = 0x4f6550;
pub(crate) const E36_ALLOC_EVENT: u32 = 0x4f4f10;
pub(crate) const COLL_EVENT_INIT_FULL: u32 = 0x4f50e0;

const fn bind(addr: u32, name: &'static str, run: crate::BindingFn) -> Binding {
    crate::bind("collision", addr, name, run)
}

pub static BINDINGS: [Binding; 63] = [
    bind(0x4f3950, "E36Collect", collect),
    bind(0x4faeb0, "CBodyLink::MoveToCell (ours)", move_to_cell),
    bind(0x4f6550, "E38RaySeeds", ray_seeds),
    bind(0x4f6740, "E38RayClear", ray_clear),
    bind(0x4f7090, "CColGrid::SyncGrid", grid_sync),
    bind(0x4fd760, "M_D760X", grid_ray),
    bind(0x4fe810, "CCollisionSystem::Vf00", system_vf00),
    bind(0x4ff170, "E39_SegTest170", seg_test),
    bind(0x4ff070, "E39_SegTest70", segment_test),
    bind(0x4febc0, "E38Ops::M_EBC0", face_query),
    bind(0x4e4960, "CBrakeWallManager::RaycastEntry", raycast_entry),
    bind(0x4e4490, "CMultiSignal1<CSigCarBrakeWallPenetration const &>::Emit", brake_wall_emit),
    bind(0x4fad90, "E38SegmentSweep", segment_sweep),
    bind(0x4e03b0, "CCellGrid::LookupTwin", lookup_twin),
    bind(0x4e1120, "CBody::Vf40", body_vf40),
    bind(0x4e11f0, "CBody::Vf118", body_grid),
    bind(0x4e1230, "CBody::VfCC", body_vf_cc),
    bind(0x4e3070, "CBody::VfD8", body_vf_d8),
    bind(0x4e2f50, "CBody::VfE8", body_vf_e8),
    bind(0x4e2e90, "CBody::VfF4", body_vf_f4),
    bind(0x4e2850, "CBody::VfDC", body_vf_dc),
    bind(0x4fb7f0, "E38_004FB7F0", clear_contact_rows),
    bind(0x4fbc20, "E38_004FBC20", refresh_events),
    bind(0x4fbca0, "E38_004FBCA0", event_count),
    bind(0x4fc0b0, "CCollisionSolver::Vf08", drain_events),
    bind(0x4fc590, "E38_004FC590X", correct_penetrations),
    bind(0x4fca20, "E38_004FCA20", solve_due_events),
    bind(0x4fb890, "E38_004FB890", apply_offsets),
    bind(0x4fbd30, "E38Ops::M_BD30", contact_rows),
    bind(0x4fc1a0, "E38Ops::M_C1A0", solve_event),
    bind(0x47d160, "CLink<CConnection<VCRegistrationZone>>::Unlink", body_link_unlink),
    bind(0x43c5b0, "E01Vec4::CopyFrom", vec4_copy_from),
    bind(0x43c630, "E01VecSetA::Cross", vec_cross),
    bind(0x465d80, "E09Vec4::Clear", vec4_clear),
    bind(0x473cd0, "CFloat3::Scale", float3_scale),
    bind(FLOAT3_LENGTH, "CFloat3::Length", float3_length),
    bind(0x4e33e0, "CBody::CState::Unk44", state_unka44),
    bind(0x4e0080, "CBodyLink::NotifyFiltered", notify_filtered),
    bind(0x4e0ae0, "E33Vec4::MinMaxA", min_max_a),
    bind(0x4e11b0, "CBody::Vf10C", body_has_grid),
    bind(0x4e11e0, "CBody::Vf114", body_has_grid),
    bind(BODY_VF110, "CBody::Vf110", body_grid_active),
    bind(0x4e3860, "CBrakeWall::Vf110", brake_wall_is_active),
    bind(0x4fc500, "E38_004FC500", min_contact_time),
    bind(0x4f6610, "E37GridOps::M6610", grid_ops_m6610),
    bind(E37_EDGE_EDGE, "E37GridOps::EdgeEdge", edge_edge),
    bind(0x4f65b0, "E37GridOps::AddEdge", grid_add_edge),
    bind(0x4e42f0, "E34WallVf::VfCC", wall_vf_cc),
    bind(BODY_VF11C, "CBody::Vf11C", body_vf11c),
    bind(0x4e1eb0, "CBody::VfD0", body_vf_d0),
    bind(0x4e3970, "CBrakeWall::VfF0", brake_wall_vf_f0),
    bind(0x4e3a10, "CBrakeWall::VfE8", brake_wall_vf_e8),
    bind(E37_PLANE_DIFF, "E37EvOps::PlaneDiff", plane_diff),
    bind(0x4fb070, "CCollisionMeshOpen::CopyBounds", mesh_copy_bounds),
    bind(0x4fb0b0, "CCollisionMeshOpen::IsActive", mesh_is_active),
    bind(0x4f4390, "E37EvOps::Query4390", event_query_bodies),
    bind(0x4f4dc0, "E37EvOps::Emit4DC0", apply_event_impulse),
    bind(E01_VEC4_NORMALIZE, "E01Vec4::Normalize", vec4_normalize),
    bind(COLGRID_SAMPLE_A, "CColGrid::SampleA", grid_sample_a),
    bind(COLGRID_SAMPLE_B, "CColGrid::SampleB", grid_sample_b),
    bind(E36_ALLOC_EVENT, "E36AllocEvent", alloc_event),
    bind(COLL_EVENT_INIT_FULL, "CCollisionEvent::InitFull", init_full),
    bind(0x4fc9a0, "CCollisionSolver::Vf04", solver_add_event),
];

/// Runs `run` on the collision store: the solver's rows and the car's grids' bytes in the typed storage the host holds
/// over a step ([`Host::typed`]), and the bodies' words in the car's solids when it holds them; else (per call) the
/// image's.
pub(crate) fn on_collision<T>(host: &dyn Host, mem: &mut Mem, run: impl FnOnce(&mut CollisionView) -> T) -> T {
    let typed = host.typed();
    run(&mut typed.car.collision.view(mem).with_solids(Some(&typed.solids)))
}

/// [`on_collision`] for a call on the grid at `grid` (and `src`), whose held words are only a car grid's: the image's
/// store for any other grid, without the host's lookup (the grid calls are among the step's most frequent).
fn on_grids<T>(host: &dyn Host, mem: &mut Mem, grids: &[u32], run: impl FnOnce(&mut CollisionView) -> T) -> T {
    if grids.iter().any(|&g| CollisionImage::is_car_grid(g)) {
        on_collision(host, mem, run)
    } else {
        run(&mut CollisionView::image(mem))
    }
}

/// `host.call_at(sp, target, this, args)` of a binding on the cell grid or a collision grid (`E45_M40C0`, `E45_M46A0`,
/// `M_D760X`, `CColGrid::SyncGrid`, `E37GridOps::M6610`, `CLink::Unlink`, `CBodyLink::NotifyFiltered`) from a caller on held state: run in place
/// while the pure host holds the state ([`Host::call_in_place`]: the cell table and the grids' hit bytes are the held
/// ones through their views, the rest is the image's either way, and none of the call's words is read after it:
/// residue audit, notes/physics.md "Store seam"), else the host call.
pub(crate) fn call_grid(host: &dyn Host, sp: u32, target: u32, this: u32, args: &[u32]) -> crate::Called {
    host.call_in_place(sp, target, this, args).unwrap_or_else(|| host.call_at(sp, target, this, args))
}

/// The contact record at `at` as a step leaves it at this point: a held solver row's, else the image's. For the
/// rigid body's contact (`CSolid::BigA`), which reads the solver's rows.
pub(crate) fn read_contact(host: &dyn Host, mem: &Mem, at: u32) -> Contact {
    (host.typed().car.collision.contact(mem, at))
        .unwrap_or_else(|| rbr_physics::rigid_body::layout::load_contact(mem, at))
}

/// The count of the body's contact list (`None` without one), as a step leaves it at this point: a held solver row's,
/// else the image's; the list word is the solid's (`rigid_body::solid_word`). For the rigid body's vote.
pub(crate) fn contact_count(host: &dyn Host, mem: &Mem, body: u32) -> Option<u32> {
    let list = crate::rigid_body::solid_word(host, mem, body + rbr_physics::rigid_body::layout::CONTACT_LIST);
    (list != 0).then(|| host.typed().car.collision.row_count(mem, list).unwrap_or_else(|| mem.u32(list + 4)))
}

/// Clears the body word of the contact rows at `rows` (the car's respawn): a held solver row's, else on the image.
pub(crate) fn clear_row_body(host: &dyn Host, mem: &mut Mem, rows: u32) {
    if !host.typed().car.collision.clear_row_body(mem, rows) {
        mem.set_u32(rows, 0);
    }
}

const E37_WALL_SYNC: u32 = 0x4f7090;
const E09_VEC4_CLEAR: u32 = 0x465d80;
pub(crate) const E01_VEC4_COPY_FROM: u32 = 0x43c5b0;
const SAMPLE_TABLE: u32 = 0x16592b8;

pub(crate) const E45_CELL_QUERY: u32 = 0x5240c0;
pub(crate) const FLOAT3_LENGTH: u32 = 0x473b30;
pub(crate) const BODY_GRID: u32 = 0x4e11f0;

/// The original callees that E36Collect calls, run through the host. The virtual slots are thiscall on their owner, the
/// E45 query is cdecl, and CFloat3::Length returns its result in st0.
struct Callees<'a>(&'a dyn Host);

pub(crate) const E01_VEC4_NORMALIZE: u32 = 0x43c6a0;
pub(crate) const COLGRID_SAMPLE_A: u32 = 0x4e0840;
pub(crate) const COLGRID_SAMPLE_B: u32 = 0x4e0950;
pub(crate) const E37_GRID_COPY: u32 = 0x4f6610;
pub(crate) const E37_EDGE_EDGE: u32 = 0x4f8280;
