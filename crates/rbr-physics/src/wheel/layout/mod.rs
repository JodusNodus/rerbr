//! The game memory of a wheel (`this`) and of its tyre tail, as the wheel's functions read it. The offsets are the only
//! raw ones in the wheel module: the ported functions take the typed values these loaders return.

mod copy_and_emit;
mod ctors;
mod fields;
mod host;
mod probe;
mod refresh_pose;
mod step;
mod store;
mod wheel_calls;

pub use copy_and_emit::{
    apply_setup, blend_tags, pick_sector, refresh_solid, surface_classes, table_lerp_pair, tire_emit, DefCopyCalls,
    Ported,
};
pub use ctors::{
    clear_scratch, construct_options, damage_zero, def_ctor, definition_dictionary, definition_name, finish, new_def,
    new_wheel, refresh_travel, scratch_ctor, select_top_mount, setup_ctor, setup_dictionary, setup_options_dictionary,
    state_ctor, tyre_tail_setup, wheel_ctor, FinishCalls, WHEEL_SIZE,
};
pub use fields::{
    AXLE, BLEND_OPS, BLOCK_BLEND, BLOCK_CLAMP_A, BLOCK_CLAMP_B, BLOCK_DIFF, BLOCK_INPUT, BLOCK_PHASE, BLOCK_TAG_1,
    BLOCK_TAG_2, BLOCK_TOTAL, BLOCK_WEIGHT, CAR, CARCASS_EDGE_MINUS, CARCASS_EDGE_MINUS_COPY, CARCASS_EDGE_PLUS,
    CARCASS_EDGE_PLUS_COPY, CAR_FLAGS, CAR_FLAG_XFORM, CAR_FORCE, CAR_SCRATCH, CAR_SCRATCH_WORD, CBASE_NAME, DAMAGE,
    DAMAGE_CHANGED, DAMAGE_FACTOR, DAMAGE_HUB, DAMAGE_SECTORS_A, DAMAGE_SECTORS_B, DAMAGE_STEERING,
    DAMAGE_TOP_MOUNT_SIDE, DAMAGE_TOP_MOUNT_UP, DAMPER_FORCE, DEF, DEFORMED_TOP_MOUNT, DEF_ANTI_ROLL_BAR_ID,
    DEF_ANTI_ROLL_BAR_SLOT, DEF_AXIS_INCLINATION, DEF_FLAGS, DEF_HUB_DAMAGE_MAX_TORQUE, DEF_HUB_DAMAGE_THRESHOLD,
    DEF_INCLINATION_DAMAGE_ANGLE, DEF_INCLINED_AXLE, DEF_INVERSE_MASS, DEF_MATERIALS, DEF_MAX_DEFORM_SIDE,
    DEF_MAX_DEFORM_UP, DEF_PAIR_TAG, DEF_PLATFORM_HEIGHT, DEF_PLATFORM_HEIGHT_HEADER, DEF_STEERING_ROD_LENGTH_HEADER,
    DEF_STRUT_DIM_Z_KEY, DEF_STRUT_DIM_Z_TWEAK, DEF_TOP_MOUNT, DEF_TOP_MOUNT_DAMAGE_MAX_FORCE,
    DEF_TOP_MOUNT_DAMAGE_THRESHOLD, DEF_TOP_MOUNT_POSITION, DEF_TOP_MOUNT_SIDE_AXIS, DEF_TOP_MOUNT_UP_AXIS,
    EDGE_ACROSS, END_SPIN_RATE, FLAG_PAIR_TAG, FRAME_AXIS, GROUND_BODY, HUB_POSITION, HUB_VELOCITY, INCLINED_AXLE,
    INCLINED_SIDE, INV_ARM, INV_RACK, LAST_TAG_1, LAST_TAG_2, LAST_WEIGHT, MATRIX_ROW_1, MATRIX_ROW_2, MAT_GAIN,
    MAT_LERP_A, MAT_LERP_B, MAT_PERIOD, ON_GROUND, ORIENTATION, PROBE_DIRECTION, RAY_CENTRE, RAY_DISTANCE,
    RAY_EDGE_MINUS, RAY_EDGE_PLUS, RAY_HIT, RAY_HIT_POINT, RAY_NORMAL, RAY_OFFSET, RAY_VALID, ROLL_BAR_FORCE,
    SECTOR_VALUE_A, SECTOR_VALUE_B, SETUP_AXIS_INCLINATION, SETUP_PLATFORM_HEIGHT, SETUP_PLATFORM_HEIGHT_HEADER,
    SETUP_STEERING_ROD_LENGTH, SETUP_STEERING_ROD_LENGTH_HEADER, SETUP_TOP_MOUNT, SETUP_TOP_MOUNT_POSITION,
    SPEED_SCALE, SPIN_RATE, SPRING_FORCE, STATE, STRUT, STRUT_DIR, STRUT_FORCE, STRUT_INNER_BUMP_POS, STRUT_LENGTH,
    STRUT_OUTER_BUMP_POS, STRUT_TWEAK, STRUT_VEL, STRUT_VEL_TWEAK, SWEEP_RESIDUE, TYRE_DEF, TYRE_DEF_CARCASS_WIDTH,
    TYRE_DEF_HUB_OFFSET, TYRE_SCRATCH, WALK_OUT, WHEEL_ANGLE, WHEEL_ANGLE_TWEAK, WHEEL_INDEX, WOBBLED_AXLE, WORLD_AXLE,
    WORLD_DEF, WORLD_HUB, XFORM_CAR, XFORM_PAIRS,
};
pub use host::{
    blend_ops, construct, forward_integrate, hook, transform_call, BlendCalls, CarSlotCalls, CarXform, ConstructCalls,
    EmbeddedCalls, EmitCalls, HookCalls, SlotCalls, SolidCalls, StrutCalls, SubBlockInit, SubCalls, WalkCalls,
};
pub use probe::{
    ground_probe, GroundCalls, ProbeTuning, WheelTuning, PROBE_FV, PROBE_HIT, PROBE_HIT_DIST, PROBE_PROLOGUE,
    PROBE_RAYS, PROBE_RAY_BLOCKS, PROBE_RAY_SIZE, PROBE_SCRATCH, PROBE_SWEEP_DIST, PROBE_SWEEP_OUT, PROBE_V1, PROBE_V2,
    PROBE_V3,
};
pub use refresh_pose::{
    refresh, RefreshCalls, CAR_GLOBAL, DEBUG_GLOBAL, DEBUG_ROWS, DEF_DRIVE_AXLE_JOINT, DEF_DRIVE_AXLE_MOUNT_OFFSET,
    DEF_IS_STEERED, DEF_JOINT_AXIS, DEF_JOINT_MOUNT, DEF_STEERING_AXIS, DEF_STEERING_MOUNT, DEF_STEERING_ROD_LENGTH,
    DEF_STEERING_SIGN, DEF_STRUT_DIM_X, DEF_STRUT_DIM_Z, DEF_STRUT_HUB_POINT, DEF_STRUT_ROD_MOUNT, DEF_WISHBONE_HEADER,
    DEF_WISHBONE_LENGTH, DEF_WISHBONE_SIGN, DRIVE_AXLE_DIR, NEG_KNUCKLE_Z, ROD_MOUNT, STEERING_ROD,
    STEERING_ROD_LENGTH, STEERING_ROD_TWEAK, TIP, WISHBONE_COPY,
};
pub use step::{wheel_step, StepCalls};
pub use store::WheelStore;
pub use wheel_calls::{
    apply_hub_torque, car_frames, carcass_edges, clear_and_forward, deform_top_mount, edge_force, edge_forces,
    forward_to_car_and_clear, pick_contact, spin_cross, steering_walk, transform_and_copy, update_ray_block,
    update_strut, wobble_axle,
};
