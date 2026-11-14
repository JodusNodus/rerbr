//! The load/store layer of the entity ports: the only code that addresses the game's memory (the
//! offsets, the vtable word and the stores), as `vcu::layout` is for the VCU. Each function reads the
//! bytes its original reads, calls the port with typed values, and stores what the original stores,
//! in the original's order.

mod classes;
mod collide_init;
mod construct;
mod detach;
mod entity_slots;
mod fence;
mod frames;
mod functor;
mod hinge_state;
mod materials;
mod store_mem;
mod walk_nodes;

pub use crate::entities::dummy::{dummy_box, dummy_span_test, DummyCalls};
pub use crate::entities::exclusion::{
    add_exclusion_at as add_exclusion, alloc_exclusion_at as alloc_exclusion, remove_exclusion_at as remove_exclusion,
};
pub use crate::entities::frame::{entity_hook_at, old_hook_at};
pub use crate::entities::ground::box_probe;
pub use crate::entities::hinge_step::{
    hinge_advance_at, hinge_limit_residual, hinge_limit_sum, hinge_rotation, hinge_solve_at, limit_update_at, pivot_at,
    pose_at, slip_term_at,
};
pub use crate::entities::material_table::Indexed;
pub use crate::entities::slots::{clear_car_bodies, componentwise_mul, init_walker_marks, ClearBodiesCalls};
pub use classes::{
    TweakKind, BASE_VTABLE, ENTITY_DEF_VTABLE, GROUND_DEF_NAME, GROUND_DEF_SIZE, GROUND_DEF_VTABLE, GROUND_OPTIONS,
    GROUND_SINGLETON, GROUND_SIZE, GROUND_TWEAK_DICT, GROUND_TWEAK_MEMBERS, GROUND_VTABLE, HINGED_DEF_NAME,
    HINGED_TWEAK_DICT, MATERIAL_TWEAK_DICT,
};
pub use collide_init::{box_probe_frame, collide_init, CollideHost, ProbeFrame};
pub use construct::{
    append_child, child_head, construct_base, construct_entity, construct_entity_def, construct_ground_def,
    construct_hinged, construct_hinged_def, ground_def_new, ground_make, hinged_def_new, new_hinged,
    release_entity_def, sibling, EntityArgs, HINGED_DEF_SIZE, HINGED_SIZE,
};
pub use detach::{
    apply_impulse_owned, detach_from_car, mounted_detach, pose_to_world, DetachCalls, ImpulseCalls, MountedDetachCalls,
    PoseToWorldCalls,
};
pub use entity_slots::{
    clear_bodies_detach_sp, dummy_span_frame, load_excluder, overlaps_box, penetration_response, zero_hook,
    DummySpanFrame,
};
pub use fence::{
    add_bend, add_fixed, add_particle, add_pin, add_spring, build_fence_group, build_fence_post, build_fence_span,
    construct_segment, d3dx_vec3_normalize, destroy_segment, init_bend_pool, init_particle_pool, init_pin_pool,
    init_segment, init_segment_pool, init_spring_table, ClothPoolCalls, FenceGroupCalls, FenceSegmentCalls,
    SegmentInitCalls, SegmentPoolCalls,
};
pub use frames::{construct_entity_frame, construct_frame, ROOT_IDENTITY};
pub use functor::{call_stored, Calls as FunctorCalls, SLOT16_FORWARD};
pub use hinge_state::{
    angle_tweak, build_hinge_axes, limit_object, pivot_frame, position_at, rate_tweak, scaled_vector, set_hinge_link,
    slip_frame, solve_frame, store_walk_vector, vec3,
};
pub use materials::{build_material_table, fill_ones, sample_indexed, store_material_table};
pub use walk_nodes::{
    append_name_index, copy_block16, copy_grid, empty_hook, node_of, old_frame_of, vtable_target, walker_address,
    GRID_FIRST_BOXES, GRID_SECOND_BOXES, GRID_STRIDE,
};
