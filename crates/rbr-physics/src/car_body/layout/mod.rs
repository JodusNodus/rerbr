//! Typed views of the car fields the batches read and write. Offsets are the original's (`notes/physics.md` "Car body",
//! rbr-recomp `notes/physics/renames/car-body.csv`). The `load` and `store` functions below are the only code that
//! knows them; the ports work on the values. Each reads only its own words, so a window that maps part of the car still
//! works.

mod body_points;
mod callees;
mod car;
mod car_calls;
mod car_parts;
mod construct;
mod damage_ctor;
mod damage_handler;
pub mod damage_handler_at;
pub mod damage_tables;
pub mod lights;
mod offsets;
pub mod wall_hit;

pub use body_points::{reset_body_grid, scatter_points};
pub use callees::{
    publish, setup, virtual_target, wiring, DAMAGE_PUBLISH_SLOT, IDENTITY_SLOT, VEC4_CLEAR, VEC4_COPY_FROM,
};
pub use car::{
    attach_lights, roll_bar_cell_at, roll_bar_force_at, solid_scratch_at, state_address, steer_slew_at,
    steer_target_at, steer_target_rate_at, steer_torque_sum_at, Real, ANGULAR_VELOCITY, DAMAGE_HANDLER, DIR_BACK,
    GRID_PUSHED, OVERLOAD_FLAG, OVERRIDE_TIMER, PARTIAL_CONTACT, SNAPSHOT_ON, SPEED, SPEED_PRE, STATE, STEER_INPUT,
    STEER_TARGET_RATE, STEP_COUNT, VELOCITY, WALL_CONTACT, WALL_PENETRATION, WORLD,
};
pub use car_calls::{
    add_steer_torque_sum, car_impulse, car_soften, drive_line_sample, edge_probe, lane_test, outside_assistance,
    push_grid_edge, refresh_edge, reset_scratch_and_dispatch, respawn_car, respawn_pose, retire_announce,
    run_countdown, set_brake_light, set_neutral_light, set_roll_bar_cell, snapshot, speed_sum_at, update_ffb_torque,
    vote_probe, CarImpulseCalls, CarSoftenCalls, CountdownCalls, EdgeProbeCalls, RespawnCalls, RespawnPoseCalls,
    SnapshotCalls, VoteCalls, DRIVE_LINE,
};
pub use car_parts::{
    drive_at, engine_at, hinged_slot, mounted_slot, passenger_at, scratch_at, wheel_list_at, Setup, TimeKeeperDiff,
    FLOW_OBJECT, SETUP_COPY,
};
pub use construct::{
    body_def_after_parse, construct_body, construct_body_def, construct_car, construct_def, construct_light_def,
    construct_light_state, construct_lights, construct_options, construct_setup, construct_state, copy_setup, new_body,
    new_body_def, new_car, new_def, push_back_connection, BODY_DEF_AFTER_PARSE, BODY_DEF_SIZE, BODY_SIZE, CAR_SIZE,
    CURRENT_CAR, DEF_DANGEROUS_ACCELERATION, DEF_DRAG_COEFFICIENT, DEF_DRIVE, DEF_ENGINE, DEF_FRONTAL_AREA, DEF_SIZE,
    DEF_VCU,
};
pub use damage_ctor::{
    construct_damage_host, construct_damage_ops, damage_level_fractions, init_damage_handler_tables,
    init_damage_ops_tables, DamageOpsCalls, ResetPorts, DAMAGE_HOST_SIZE, DAMAGE_OPS_SIZE,
};
pub use damage_handler::{
    always_zero, apply_functors, apply_impact, bend_top_mount, break_hinged_body, break_hinged_lock,
    break_lamps_kind_1, break_lamps_kind_2, break_lamps_kind_3, break_lamps_kind_4, break_lamps_kind_5,
    break_lamps_kind_6, cap_clutch, chip_windshield, crack_windshield, damage_radiator, damage_rand,
    drain_oil_pressure, engine_gate, fail_brake_circuit, fail_differential, fail_driveshaft, fail_hydraulics,
    fail_starter, fail_water_pump, grow_cooling_wear, impact_scale, leak_oil_cooler, ring_store, second_zero,
    set_body_state, wear_gearbox, zone_scaled, DamageHitCalls, ImpactCalls,
};
pub use offsets::{
    all_receivers_at, control, damage_source_at, drive_line_position_at, emitter_busy_at, emitter_source_at, handler,
    index_receivers_at, light_system_at, next_receiver_at, position_at, signal_at, step_mark_steering_at,
    step_mark_timer_at, tweak_word_at, walk_source_at, BRAKE_AT, CAR_INIT_EMITTER_AT, PARTIAL_CONTACT_FLAG,
    PROBLEM_EMITTER_AT, RETIRE_EMITTER_AT, STAND_STILL_EMITTER_AT, THROTTLE_AT, TWEAK_BLOCK_WORDS,
};
