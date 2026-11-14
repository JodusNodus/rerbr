//! `E51Wheel::M3B900` (0x53b900), the pose refresh, and the CDef words it reads.

use super::fields::{AXLE, DAMAGE_STEERING, HUB_POSITION, ORIENTATION, TYRE_DEF_HUB_OFFSET, TYRE_SCRATCH};
use super::store::WheelStore;
use super::typed::slot;
pub use crate::car_body::layout::CURRENT_CAR as CAR_GLOBAL;
use crate::car_body::store::CarStore;
use crate::math::quat;
use crate::math::vec::{Vec3, Vec4};
pub use crate::physics_core::layout::PHYSICS_GLOBAL as DEBUG_GLOBAL;
use crate::wheel::refresh::{self, PoseIn};

// The refresh (`M3B900`, `refresh.rs`): the CDef words it reads, and its inputs from the wheel and the tyre block.
/// The CDef words of the refresh (`+0x23c` `SteeringRodLength`, `+0x244`, `+0x248` the header words and `+0x24c` the value of
/// `WishboneLength`, `+0x880` the joint mount, `+0x890` the joint axis).
pub const DEF_STEERING_ROD_LENGTH: u32 = 0x23C;
pub const DEF_WISHBONE_HEADER: u32 = 0x244;
pub const DEF_WISHBONE_LENGTH: u32 = 0x24C;
pub const DEF_JOINT_MOUNT: u32 = 0x880;
pub const DEF_JOINT_AXIS: u32 = 0x890;
/// `+0x36c` (`WishboneSign`), `+0x3fc` (the real after `StrutDimX`: `StrutDimZ` plus the setup's `StrutPlatformHeight`,
/// as `CopyCF30` stores it), `+0x3ec` (`StrutDimX`) and `+0x35c` (`SteeringSign`).
pub const DEF_WISHBONE_SIGN: u32 = 0x36C;
pub const DEF_STRUT_DIM_Z: u32 = 0x3FC;
pub const DEF_STRUT_DIM_X: u32 = 0x3EC;
pub const DEF_STEERING_SIGN: u32 = 0x35C;
/// `+0x380` and `+0x3a0`: `vecStrutRodMount` and `vecStrutHubPoint` (three lanes each).
pub const DEF_STRUT_ROD_MOUNT: u32 = 0x380;
pub const DEF_STRUT_HUB_POINT: u32 = 0x3A0;
/// `+0x21c` (`IsSteered`), `+0x42c` (`DriveAxleMountOffset`), `+0x8b0` and `+0x8c0` (the steering mount and axis; the
/// axis' y and z are the words `+0x8c4` and `+0x8c8`) and `+0x8d0` (the drive axle's joint).
pub const DEF_IS_STEERED: u32 = 0x21C;
pub const DEF_DRIVE_AXLE_MOUNT_OFFSET: u32 = 0x42C;
pub const DEF_STEERING_MOUNT: u32 = 0x8B0;
pub const DEF_STEERING_AXIS: u32 = 0x8C0;
pub const DEF_DRIVE_AXLE_JOINT: u32 = 0x8D0;

/// `this+0x5d0`: the object `SetDirty` runs on (the live copy of `SteeringRodLength`).
pub const STEERING_ROD_TWEAK: u32 = 0x5D0;
/// `this+0x5dc`: the steering rod's length after damage, stored after `SetDirty`.
pub const STEERING_ROD_LENGTH: u32 = 0x5DC;
/// `this+0x5e4`: the three words of `WishboneLength` the refresh copies (`+0x244`, `+0x248`, `+0x24c`).
pub const WISHBONE_COPY: u32 = 0x5E4;
/// `this+0x990`: the knuckle's third axis, negated.
pub const NEG_KNUCKLE_Z: u32 = 0x990;
/// `this+0x9b0`: the wishbone's tip.
pub const TIP: u32 = 0x9B0;
/// `this+0x9d0`: the drive axle's direction.
pub const DRIVE_AXLE_DIR: u32 = 0x9D0;
/// `this+0xd80`, `+0xd90`, `+0xda0`: the knuckle's three axes, copied under the debug flag.
pub const DEBUG_ROWS: u32 = 0xD80;
/// `this+0xe50`: the rod mount in the world.
pub const ROD_MOUNT: u32 = 0xE50;
/// `this+0xe60`: the steering rod.
pub const STEERING_ROD: u32 = 0xE60;

/// The original calls `E51Wheel::M3B900` makes: `SetDirty` (0x52f450), `TyreTail_Get10` (0x5326b0) and the wheel's
/// vtable slot 16 (`Vf40`, thiscall, no arguments).
pub trait RefreshCalls {
    /// `NTweak::CBase::SetDirty` (0x52f450) on `object` (no arguments).
    fn set_dirty(&mut self, object: u32);
    /// `TyreTail_Get10` (0x5326b0) on the tyre scratch: its setup.
    fn tyre_def(&mut self, scratch: u32) -> u32;
    /// The wheel's vtable slot 16 (+0x40, `E51Wheel::M3B5B0`, [`refresh_solid`](super::refresh_solid)), called on
    /// `wheel`; returns eax.
    fn refresh_solid(&mut self, wheel: u32) -> u32;
}

/// `E51Wheel::M3B900` (0x53b900) after its prologue calls: the deformed top mount (`M3AB90`) is stored at `+0x570`, the
/// wobbled axle (`M3A9E0`) at `+0x5f0`, and both are read back here. The sequence is the original's: the stores and host
/// calls in order. Returns the eax of the final virtual call.
pub fn refresh<S: WheelStore>(mem: &mut S, calls: &mut impl RefreshCalls, this: u32, travel: f32) -> u32 {
    let input = PoseIn {
        travel,
        def: mem.load_suspension_def(this),
        steering_damage: mem.load_damage_f32(this, DAMAGE_STEERING),
        top_mount: mem.get(this, slot::deformed_top_mount),
        axle: mem.get(this, slot::wobbled_axle),
    };
    let def = &input.def;
    let strut = refresh::strut_geometry(&input);
    calls.set_dirty(this + STEERING_ROD_TWEAK);
    mem.set(this, slot::steering_rod_length, strut.steering_rod_length);
    let wishbone = [def.wishbone_header[0], def.wishbone_header[1], def.wishbone_length.to_bits()];
    mem.set(this, slot::wishbone_length, wishbone);
    let linkage = refresh::linkage(&input, &strut);
    let steer_target = if def.is_steered { mem.mem().steer_target(mem.u32(CAR_GLOBAL)) } else { 0.0 };
    let frame = refresh::hub_frame(&input, &strut, &linkage, steer_target, strut.steering_rod_length);
    frame.hub_position.store(mem, this + HUB_POSITION);
    (Vec3 { x: frame.rotation[0][0], y: frame.rotation[0][1], z: frame.rotation[0][2] }).store(mem, this + AXLE);
    let tyre_def = calls.tyre_def(this + TYRE_SCRATCH);
    let (hub_position, drive_axle_dir) = refresh::place_hub(
        Vec4::load(mem, this + AXLE),
        mem.f32(tyre_def + TYRE_DEF_HUB_OFFSET),
        frame.hub_position,
        def.drive_axle_mount_offset,
        def.drive_axle_joint,
    );
    hub_position.store(mem, this + HUB_POSITION);
    frame.neg_knuckle_z.store(mem, this + NEG_KNUCKLE_Z);
    mem.set(this, slot::strut_dir, frame.strut_dir);
    mem.set(this, slot::tip, frame.tip);
    mem.set(this, slot::rod_mount, frame.rod_mount);
    mem.set(this, slot::steering_rod, frame.steering_rod);
    mem.set(this, slot::inv_arm, frame.inv_arm);
    mem.set(this, slot::inv_rack, frame.inv_rack);
    mem.set(this, slot::drive_axle_dir, drive_axle_dir);
    if mem.debug_flag() {
        mem.set(this, slot::debug_rows, frame.knuckle_axes);
    }
    let q = quat::to_quat(&frame.rotation);
    Vec4 { x: q.x, y: q.y, z: q.z, w: q.w }.store(mem, this + ORIENTATION);
    calls.refresh_solid(this)
}
