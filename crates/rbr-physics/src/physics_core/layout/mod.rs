//! The game memory the physics core reads and writes, and its calls into the original's code: the typed views of the
//! vectors, matrices and row arrays, the object images and constructors, the detector and contact walks and their
//! hosts. The modules outside `layout` never touch memory; this directory is the only edge.

pub mod collide_work;
pub mod construct;
pub mod flags;
pub mod material;
pub mod mem_store;
pub mod mounted;
pub mod mounted_body;
pub mod mounted_ctor;
pub mod mounted_pose;
pub mod setup;

pub mod contact_walk;
pub mod detect_calls;
pub mod detector_hosts;
pub mod passenger_ctor;
pub mod physics_object;
pub mod signal_emit;

mod detector_calls;
mod globals;
mod lists;
pub mod mounted_calls;
mod notify;
mod passenger_step;
mod physics_ctor;
pub mod prof_calls;
mod step_forward;
mod vectors;

pub use collide_work::{advance_contact_bodies, advance_contact_bodies_remainder, collide_window};
pub use detector_calls::{
    body_pair_detect, edge_pairs, giant_penetration_detect, kernel_collect, kernel_entity, l1_collision_detect,
    l1_penetration_detect, l2_collision_detect, l2_penetration_detect, penetration_check, run_collision_detectors,
    set_cells, set_edge_result, visit_walls, xy, KernelArgs, KernelEntity,
};
pub use globals::{
    cell_grid, collision_global, def_debug_flags, def_flags, entity_list, link_registrar, physics, physics_global,
    relay_link, relay_target, release_target, set_app_callbacks, set_cell_grid, signal_manager, state_of, wall_manager,
    water_table, Registrar, CELL_GRID_GLOBAL, PHYSICS_GLOBAL, REGISTRARS, TELEMETRY_GLOBAL, WALL_MANAGER_GLOBAL,
    ZERO_VEC4,
};
pub(crate) use globals::{COLLISION_GLOBAL, SIGNAL_MANAGER};
pub use lists::{
    conn_owner, copy_vec4, emit_traps, f32, link_next, list_head, mounted_state, push_item, register_entity, set_f32,
    set_mounted_state, set_signal_guard, set_word, signal_count_at, signal_guard, signal_overflow, signal_slot,
    table_word, unlink_item, unlink_with_eax, vtable_target, word,
};
pub use mounted_calls::{mounted_position_step, mounted_pull, MountedPullHost, MountedStepHost};
pub use notify::{
    create_entity, entity_name, notify_placement, refile_entity, refile_through_parent, Filer, Filing, FilingHost,
    NotifyHost, RefileHost, REGISTRY,
};
pub use passenger_step::{passenger_pull, passenger_step};
pub use physics_ctor::{
    byte, clear_scratch_block, construct_material_manager, def_global, set_create_result, set_def_global,
    set_physics_global, set_physics_hook, set_physics_pointers, set_physics_tail, set_scratch_vtable, store_material,
    store_physics_def, store_physics_front, store_solver_def, store_state, Ported, CONN_AT,
};
pub use prof_calls::{prof_chan_enter, prof_enter, prof_exit, store_frequency};
pub use signal_emit::{emit, notify_owner, OwnerHost, SignalHost};
pub use step_forward::{
    seed_body_pass_edi, seed_penetration_entry, seed_penetration_exit, seed_step_forward_entry, step_forward,
    step_scratch_events, virtual_target, StepStack, END_STEP_SLOT, INIT_PASS_SLOT, POST_BODIES_SLOT, POST_CONTACT_SLOT,
    PRE_STEP_SLOT, SOLVER_POST_SLOT,
};
pub use vectors::{
    accumulate_forces, end_step_word, floats, force_accum, force_scale, inverse_mass, mat, mat_mul, mat_scale_rows,
    point_velocity_at, set_force_accum, set_table_index, set_vec3s, set_vec4s, table_index_hint, takes_part_word,
    vec3s, vec4s, vec_add, AccumHost,
};
