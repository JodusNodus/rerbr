//! Typed views of the memory the engine and drive ports read and write. `load` and `store` are the only code that
//! knows the original's offsets; the ports take and return the typed values.

mod construct;
mod curve;
mod drive_calls;
mod drive_line;
mod drive_objects;
mod engine_calls;
mod engine_objects;
mod engine_sim;
mod mem_store;
mod setup;
mod step;
mod tables;
mod worker;

pub use construct::{
    construct_drive, construct_drive_def, construct_drive_options, construct_drive_part, construct_drive_setup,
    construct_drive_state, construct_engine, construct_engine_def, construct_engine_options, construct_engine_scratch,
    construct_engine_setup, construct_engine_state, drive_reset_part, reset_drive_damage, reset_engine_damage,
};
pub use curve::{
    brake_block_at, curve_input_at, curve_row_at, curve_state_at, ratio_ring_at, with_gear, with_worker, BrakeBlock,
    CurveInput, CurveRow, CurveState, CurveTables,
};
pub use drive_calls::{
    clear_lanes, copy_row, distance_at, drive_gate, mat4_inverse_transposed, mat4_mul_vec4, project_point,
    set_diff_lock, set_wheel_offset_and_bias, table_entry_field_at, transpose_into, wheel_prev_force, wheel_slip,
};
pub use drive_line::{
    entry_at, forward_distance_at, last_distance, point_at, sample, set_drive_line, spline_at, COUNT_AT, MAX_ENTRIES,
    STRIDE,
};
pub use drive_objects::{Drive, DriveDef, DriveObjects, DriveState, IntLane, Lane};
pub use engine_calls::{
    abs_rpm_term, boost_pressure, compliance, coolant_temperature, discarded_temperatures, engine_running,
    engine_state_speed, exhaust_blend_at, init_real_lerp, options_first_gate, pedal_cosine, rev_gate, set_cut_flags,
    set_options, slot_roll, torque_curve, ROLL_TABLE,
};
pub use engine_objects::{EngineDef, EngineObjects, EngineOptions, EngineOwn, EngineState, EngineTuning, LoadDef};
pub use engine_sim::{
    crank_tick, engine_step, engine_torque, fuel_air_terms, heat_step, intake_entry_update, intake_state_reset,
    misfire_roll, overheat_step, signal_update, smooth_damper, solve_damper, turbo_energy_step,
};
pub use setup::{drive_setup_into_def, link_worker, normalize_gear_rows, select_gear_rows};
pub use step::step_drive;
pub use tables::{BlendKnot, CurveKnot, UnitSplit};
pub use worker::{
    Axle, DriveDamage, DriveOwn, DriveWorker, GearBand, GearWork, LaneScale, RatioRing, Wheel, WheelBlock, WheelTerm,
    WorkerDef, WorkerState, CLUTCH_MATRIX_AT, SOLUTION_AT, STEP_MATRIX_AT, STEP_VEC_AT,
};
