//! The wheel's member offsets (`CWheel`, its CDef, its tyre block and tail) and the small loaders and stores over
//! them.

use super::refresh_pose::{
    DEBUG_GLOBAL, DEF_DRIVE_AXLE_JOINT, DEF_DRIVE_AXLE_MOUNT_OFFSET, DEF_IS_STEERED, DEF_JOINT_AXIS, DEF_JOINT_MOUNT,
    DEF_STEERING_AXIS, DEF_STEERING_MOUNT, DEF_STEERING_ROD_LENGTH, DEF_STEERING_SIGN, DEF_STRUT_DIM_X,
    DEF_STRUT_DIM_Z, DEF_STRUT_HUB_POINT, DEF_STRUT_ROD_MOUNT, DEF_WISHBONE_HEADER, DEF_WISHBONE_LENGTH,
    DEF_WISHBONE_SIGN,
};
use super::store::WheelStore;
use crate::math::vec::{Vec3, Vec4};
use crate::mem::Mem;
use crate::wheel::axes::{AxleWobbleIn, HubDamageIn, HubDamageUpdate, Inclination, InclinedAxle, TopMountDeformIn};
use crate::wheel::contact::RayBlock;
use crate::wheel::refresh::SuspensionDef;

/// `CWheel::m_car` (`+0x7f4`): the car this wheel belongs to (the receiver of `Xform6870`).
pub const CAR: u32 = 0x7F4;
/// `+0xf10`: the tyre's setup, the `m_def` of the tyre scratch at `TYRE_SCRATCH` (`tyre::layout::DEF`), read by
/// `TyreTail_Get10` (`*(container + 0x10)`, 0x5326b0).
pub const TYRE_DEF: u32 = 0xF10;
/// Vector `this+0x140`: the hub position (`M3B900` stores it; `Copy100` moves it along the axle by the tyre's hub offset).
pub const HUB_POSITION: u32 = 0x140;
/// Vector `this+0x100`: the wheel's rotation as a quaternion (`M3B900` stores it; `Copy100` copies it to its output).
pub const ORIENTATION: u32 = 0x100;
/// `this+0x11c` and `+0x128`: the second and third rows of the body's matrix (three words each, the matrix from
/// `+0x110`, `rigid_body::layout`). The step takes them through the car into the world: the third, negated, is
/// `PROBE_DIRECTION`.
pub const MATRIX_ROW_1: u32 = 0x11C;
pub const MATRIX_ROW_2: u32 = 0x128;
/// Vector `this+0xa90`: `WORLD_AXLE x PROBE_DIRECTION` (lanes 0..2; the ground probe stores it, lane 3 keeps its old
/// value), the axis `Unk9290` dots the ray normals with and scales into its output.
pub const FRAME_AXIS: u32 = 0xA90;
/// Vector `this+0xa70`: the wheel axle (`AXLE`) through the car's transform, in the world (`M3DD30`). `Blend4` moves along
/// it by half the carcass width; `Unk9290` adds it scaled by its `second` term.
pub const WORLD_AXLE: u32 = 0xA70;
/// Vector `this+0xa80`: the ground probe's direction, the matrix's third row through the car, negated (`M3DD30`). Each
/// ray is one unit along it from its start, and the contact point is the hub plus it times the contact distance.
/// `Unk9290` adds it scaled by its `first` term.
pub const PROBE_DIRECTION: u32 = 0xA80;
/// Vector `this+0xa30`: the hub in the world (`M3DD30` copies the car's transform of `HUB_POSITION`), the centre ray's
/// start.
pub const WORLD_HUB: u32 = 0xA30;
/// Vectors `this+0xa20` and `+0xa40`: the carcass edges `Blend4` writes, the world hub moved by minus and plus half the
/// tyre's `CarcassWidth` along `WORLD_AXLE` (its `d` and `c`), the starts of the two edge rays. `+0xe00` and `+0xde0` take
/// the same two vectors.
pub const CARCASS_EDGE_MINUS: u32 = 0xA20;
pub const CARCASS_EDGE_PLUS: u32 = 0xA40;
pub const CARCASS_EDGE_PLUS_COPY: u32 = 0xDE0;
pub const CARCASS_EDGE_MINUS_COPY: u32 = 0xE00;

/// The tyre's `HubOffset` (`tyre_def+0x6c`), which `Copy100` moves the hub along the axle by.
pub const TYRE_DEF_HUB_OFFSET: u32 = 0x6C;
/// The tyre's `CarcassWidth` (`tyre_def+0xcc`), which `Blend4` halves.
pub const TYRE_DEF_CARCASS_WIDTH: u32 = 0xCC;

/// `this+0x34`: the CDef, the wheel's setup (`CWheel::m_def`, `RBR_E51_WheelA.cpp:41`).
pub const DEF: u32 = 0x34;
/// `this+0x44`: the car `M3A8A0` makes its transforms on (`RBR_E51_WheelA.cpp:41`), kept when its flags carry bit 3.
pub const XFORM_CAR: u32 = 0x44;
/// `car+0x60`: the car's flags byte; bit 3 (`CAR_FLAG_XFORM`) lets `M3A8A0` use the car (`RBR_E51_WheelA.cpp:43`).
pub const CAR_FLAGS: u32 = 0x60;
/// The bit of `car+0x60` that `M3A8A0` tests (`testb $0x8`).
pub const CAR_FLAG_XFORM: u8 = 0x8;
/// The car transforms `M3A8A0` makes: for each pair, the guard (`def + guard` is null when zero), the source and the output
/// offsets from the CDef, in the order of the calls (`RBR_E51_WheelA.cpp:50-67`). The transform kinds are in `xform.rs`.
pub const XFORM_PAIRS: [(u32, u32, u32); 8] = [
    (0x310, 0x320, 0x8E0),
    (0x330, 0x340, 0x8F0),
    (0x250, 0x260, 0x880),
    (0x270, 0x280, 0x890),
    (0x290, 0x2A0, 0x8A0),
    (0x2B0, 0x2C0, 0x8B0),
    (0x2D0, 0x2E0, 0x8C0),
    (0x2F0, 0x300, 0x8D0),
];

// The wheel's damage record (`this+0x7f0`, a float block: `RBR_E51_WheelA.cpp:74`, `RBR_E51_WheelB.cpp:73`,
// `RBR_E50_WheelSim.cpp:570`; `E51WheelDamage::Zero` clears its first thirteen words). The step raises `+0` with the
// force on the top mount and `+8` with the force along the axle, and `M3AC50` moves `+0x10` with the hub torque, clamped
// to [-1, 1]; the other words are named by use.
/// `this+0x7f0`: the damage record pointer.
pub const DAMAGE: u32 = 0x7F0;
/// `damage+0x0`: the top mount's damage along the up axis, the factor of the `+0x570` lerp (`RBR_E51_WheelA.cpp:128`).
pub const DAMAGE_TOP_MOUNT_UP: u32 = 0x0;
/// `damage+0x4`: the top mount's damage along the side axis, the factor of the second `+0x570` lerp
/// (`RBR_E51_WheelA.cpp:138`).
pub const DAMAGE_TOP_MOUNT_SIDE: u32 = 0x4;
/// `damage+0x8`: the steering damage (0.015 of it shortens the steering rod in `M3B900`; the step raises it with the force
/// along the axle).
pub const DAMAGE_STEERING: u32 = 0x8;
/// `damage+0x10`: the hub damage the axis angle and the setter use (`RBR_E51_WheelA.cpp:74`).
pub const DAMAGE_HUB: u32 = 0x10;
/// `damage+0x14` and `+0x24`: the two 4-entry tables `Unk96e0` interpolates between the wheel's angular sectors (named by
/// use).
pub const DAMAGE_SECTORS_A: u32 = 0x14;
pub const DAMAGE_SECTORS_B: u32 = 0x24;
/// `damage+0x34`: the factor the step takes the strut's force and the force along the axle by before their damage
/// thresholds, and the hub torque before `M3AC50` (named by use; `Zero` leaves it).
pub const DAMAGE_FACTOR: u32 = 0x34;

/// `this+0x570`: the vector `M3AB90` lerps from the CDef's `+0x8e0`, `+0x8a0` and `+0x8f0` (`RBR_E51_WheelA.cpp:124`).
pub const DEFORMED_TOP_MOUNT: u32 = 0x570;
/// `this+0x5a0`: the real of the wheel's angle (its value is `WHEEL_ANGLE`), made dirty before each store.
pub const WHEEL_ANGLE_TWEAK: u32 = 0x5A0;
/// `this+0x5ac`: the angle `M3A9E0` takes the sine and cosine of (`RBR_E51_WheelA.cpp:77`).
pub const WHEEL_ANGLE: u32 = 0x5AC;
/// `this+0x5f0`: the vector `M3A9E0` writes and normalises (`RBR_E51_WheelA.cpp:78-89`); lane 3 is kept from the old value.
pub const WOBBLED_AXLE: u32 = 0x5F0;
/// `this+0x600`: the axis base `M3A9E0` adds to its scaled turn vector, and `M3AB00` writes (cos, 0, sin).
pub const INCLINED_AXLE: u32 = 0x600;
/// `this+0x610`: the turn vector `M3A9E0` scales by the cosine term (`RBR_E51_WheelA.cpp:82`); `M3AB00` writes (-sin, 0, cos).
pub const INCLINED_SIDE: u32 = 0x610;
/// `this+0x9c0`: the inverse of the steering rod's moment arm about the strut (`M3B900`), a factor of the walk's gain
/// (`M3AEF0`, `RBR_E51_WheelA.cpp:229-236`).
pub const INV_ARM: u32 = 0x9C0;
/// `this+0x9c4`: the inverse of the rod's direction along the steering axis (`M3B900`), the magnitude `M3AEF0` tests
/// against 0.001.
pub const INV_RACK: u32 = 0x9C4;
/// `this+0x9a0`: the vector `M3AEF0` adds, scaled by its gain, to `this+0xc10` (`RBR_E51_WheelA.cpp:241-244`).
pub const STRUT_DIR: u32 = 0x9A0;
/// `this+0xc10`: the vector `M3AEF0` walks into (`MB560`'s `c`) and then adds to; `M3AFB0` reads it. Working name.
pub const WALK_OUT: u32 = 0xC10;
/// `this+0xa50`: the spin rate `M3AFB0` scales the vectors by (`RBR_E51_WheelA.cpp:247-250`).
pub const SPIN_RATE: u32 = 0xA50;
/// `this+0xaa0`: the contact distance the ground probe stores (the contact point is the world hub plus `PROBE_DIRECTION`
/// times it), the scale the FMOD block multiplies the spin rate by for the wheel's rim speed (|ωR|, m/s).
pub const SPEED_SCALE: u32 = 0xAA0;
/// `this+0x9e0`: the wheel axle, the first row of the wheel's rotation (`M3AFB0` scales it by the spin, `Copy100` by the hub offset).
pub const AXLE: u32 = 0x9E0;
/// `this+0xea0`: the spin rate read again at the end of the step (named by use).
pub const END_SPIN_RATE: u32 = 0xEA0;

/// CDef `+0x8e0`, `+0x8a0`, `+0x8f0`: the lerp inputs of `M3AB90` (`RBR_E51_WheelA.cpp:126-137`).
pub const DEF_TOP_MOUNT_UP_AXIS: u32 = 0x8E0;
pub const DEF_TOP_MOUNT: u32 = 0x8A0;
pub const DEF_TOP_MOUNT_SIDE_AXIS: u32 = 0x8F0;
/// CDef `+0x920` and `+0x910`: the scales of the two `M3AB90` lerps (`RBR_E51_WheelA.cpp:128`, `:138`).
pub const DEF_MAX_DEFORM_UP: u32 = 0x920;
pub const DEF_MAX_DEFORM_SIDE: u32 = 0x910;
/// CDef `+0x930` and `+0x940`: `TopMountDamageForceThreshold` and `TopMountDamageMaxForce` (NGP's keys of the reals at
/// `+0x924` and `+0x934`), the step's bounds of the top mount's damage.
pub const DEF_TOP_MOUNT_DAMAGE_THRESHOLD: u32 = 0x930;
pub const DEF_TOP_MOUNT_DAMAGE_MAX_FORCE: u32 = 0x940;
/// CDef `+0x950` and `+0x900`: the scale and offset of the axis angle in `M3AB00` (`RBR_E51_WheelA.cpp:106-109`).
pub const DEF_INCLINATION_DAMAGE_ANGLE: u32 = 0x950;
pub const DEF_AXIS_INCLINATION: u32 = 0x900;
/// CDef `+0x960` and `+0x970`: the setter's lower bound and range end in `M3AC50` (`RBR_E51_WheelA.cpp:147-151`).
pub const DEF_HUB_DAMAGE_THRESHOLD: u32 = 0x960;
pub const DEF_HUB_DAMAGE_MAX_TORQUE: u32 = 0x970;
/// CDef `+0x43c` and `+0x44c`: the values of `AntiRollBarId` and `AntiRollBarSlot`, the car's roll-bar cell of the wheel.
pub const DEF_ANTI_ROLL_BAR_ID: u32 = 0x43C;
pub const DEF_ANTI_ROLL_BAR_SLOT: u32 = 0x44C;

// The words `CopyCF30` (0x53cf30) copies from the wheel's `CWheel::CSetup` into the CDef. A real is a tweak whose two
// header words (`+4`, `+8`) come before its value (`+0xc`); a vector's value is at `+0x10`.
/// The setup's `vecTopMountPosition` (the vector tweak at `+0x20`) and its value (`+0x30`).
pub const SETUP_TOP_MOUNT: u32 = 0x20;
pub const SETUP_TOP_MOUNT_POSITION: u32 = 0x30;
/// The setup's `SteeringRodLength`: its header words (`+0x44`) and value (`+0x4c`).
pub const SETUP_STEERING_ROD_LENGTH_HEADER: u32 = 0x44;
pub const SETUP_STEERING_ROD_LENGTH: u32 = 0x4C;
/// The setup's `StrutPlatformHeight`: its header words (`+0x54`) and value (`+0x5c`).
pub const SETUP_PLATFORM_HEIGHT_HEADER: u32 = 0x54;
pub const SETUP_PLATFORM_HEIGHT: u32 = 0x5C;
/// The setup's `WheelAxisInclination` (`+0x6c`, rad).
pub const SETUP_AXIS_INCLINATION: u32 = 0x6C;
/// CDef `+0x234`: the header words of `SteeringRodLength` (its value is `DEF_STEERING_ROD_LENGTH`).
pub const DEF_STEERING_ROD_LENGTH_HEADER: u32 = 0x234;
/// CDef `+0x2a0`: the value of `vecTopMount`.
pub const DEF_TOP_MOUNT_POSITION: u32 = 0x2A0;
/// CDef `+0x414` and `+0x41c`: the header words and value of the real at `+0x410`, the setup's `StrutPlatformHeight`
/// (named by use: the step adds it to the travel for the strut's coordinate).
pub const DEF_PLATFORM_HEIGHT_HEADER: u32 = 0x414;
pub const DEF_PLATFORM_HEIGHT: u32 = 0x41C;
/// CDef `+0x40c`: the value of `StrutDimZ`, which `CopyCF30` adds the platform height to for `DEF_STRUT_DIM_Z`.
pub const DEF_STRUT_DIM_Z_KEY: u32 = 0x40C;
/// CDef `+0x3f0`: the real whose value is `DEF_STRUT_DIM_Z` (made dirty before the store).
pub const DEF_STRUT_DIM_Z_TWEAK: u32 = 0x3F0;
/// CDef `+0x3d0`: the setup's axle direction, `(cos, 0, sin)` of `WheelAxisInclination` (three lanes).
pub const DEF_INCLINED_AXLE: u32 = 0x3D0;

/// The tyre scratch (`CScratch`, `this+0xf00`) and the blend ops (`E50BlendOps`, `this+0xaa4`) that `Unk9900` uses.
pub const TYRE_SCRATCH: u32 = 0xF00;
pub const BLEND_OPS: u32 = 0xAA4;
/// `this+0xc04`, `+0xc08`, `+0xc0c`: the two tags and the weight of the last `Unk9900` blend (`m_unkC04`, `m_unkC08`,
/// `m_unkC0C`), which the step's tyre-particle payload carries.
pub const LAST_TAG_1: u32 = 0xC04;
pub const LAST_TAG_2: u32 = 0xC08;
pub const LAST_WEIGHT: u32 = 0xC0C;
/// `this+0xeb4`: the car's index of this wheel (`m_wheelIndex`, inferred). The car's calls take it, and the spring
/// damper's signal owner id is the same value.
pub const WHEEL_INDEX: u32 = 0xEB4;
/// `this+0xebc`: 1 when the ground probe found a contact, else 0 (`m_onGround`, inferred).
pub const ON_GROUND: u32 = 0xEBC;
/// `this+0xec0`: an int set to 1 when the damage record changes (the hub torque in `M3AC50`, `RBR_E51_WheelA.cpp:159`,
/// and the step's top mount and steering damage), cleared through `M3B880` (`RBR_E51_WheelB.cpp:74`). Named by use.
pub const DAMAGE_CHANGED: u32 = 0xEC0;
/// `this+0x9f0` and `+0xa00`: the sums the step adds to and `E43Vf5C` passes to the car (`CAR_FORCE` to its force
/// accumulator, `CAR_SCRATCH` to its scratch accumulator) before it clears them, and the word after them it zeroes
/// (0x539a90).
pub const CAR_FORCE: u32 = 0x9F0;
pub const CAR_SCRATCH: u32 = 0xA00;
pub const CAR_SCRATCH_WORD: u32 = 0xA10;

// The ground probe's three ray blocks (`M3D520`, 0x70 bytes each): the centre ray's from the world hub, and the two edge
// rays' from the carcass edges. A block's `+0x10` takes the 16 hit words of a cast (`physics_core::layout`'s edge result:
// the point, the normal, 1.0, 0.0, the hit's two words and its distance).
/// The centre ray's block (`+0xc20`).
pub const RAY_CENTRE: u32 = 0xC20;
/// The block of the ray from `CARCASS_EDGE_MINUS` (`+0xc90`).
pub const RAY_EDGE_MINUS: u32 = 0xC90;
/// The block of the ray from `CARCASS_EDGE_PLUS` (`+0xd00`).
pub const RAY_EDGE_PLUS: u32 = 0xD00;
/// A block's hit word (`+0`): the cast's mesh-query result, then 1 on a hit.
pub const RAY_HIT: u32 = 0x0;
/// A block's second word (`+4`): cleared on a miss, and read by the blend as the block's "valid" flag.
pub const RAY_VALID: u32 = 0x4;
/// A block's hit point (`+0x10`, the first of the hit words).
pub const RAY_HIT_POINT: u32 = 0x10;
/// A block's hit normal (`+0x20`): `Unk9290` dots it with the frame axis, `Unk94d0` splits along it, and the probe loads
/// it as its contact normal.
pub const RAY_NORMAL: u32 = 0x20;
/// The x and y of the hit point as the tag blend reads them (NGP's `tag_blend`: its sine inputs).
pub const BLOCK_INPUT: u32 = 0x10;
pub const BLOCK_PHASE: u32 = 0x14;
/// A block's tag weight and two material tags (`+0x34`, `+0x38`, `+0x3c`; the hit words).
pub const BLOCK_WEIGHT: u32 = 0x34;
pub const BLOCK_TAG_1: u32 = 0x38;
pub const BLOCK_TAG_2: u32 = 0x3C;
/// A block's hit distance (`+0x40`).
pub const RAY_DISTANCE: u32 = 0x40;
/// A block's offset across the tyre (`+0x50`; named by use: the probe sets 0 for the centre ray, 0.1 for the plus edge
/// and the word at 0x739594 for the minus edge), the first coordinate of `Unk9290`'s normal.
pub const RAY_OFFSET: u32 = 0x50;
/// The tag blend's outputs (`+0x54`, `+0x58` the total), its two limit words (`+0x5c`, `+0x60`) and the limited
/// difference (`+0x64`, `ray::limit_ray`).
pub const BLOCK_BLEND: u32 = 0x54;
pub const BLOCK_TOTAL: u32 = 0x58;
pub const BLOCK_CLAMP_A: u32 = 0x5C;
pub const BLOCK_CLAMP_B: u32 = 0x60;
pub const BLOCK_DIFF: u32 = 0x64;
/// `this+0x3d0`: the wheel's state (`CWheel::CState`), whose slot 17 `M3B5B0` tail-calls.
pub const STATE: u32 = 0x3D0;
/// The two outputs of `Unk96e0`, in the wheel: the sector tables' values at the wheel's angle (named by use). The second is
/// the floor of the edge rays' limited difference.
pub const SECTOR_VALUE_A: u32 = 0xD70;
pub const SECTOR_VALUE_B: u32 = 0xD74;
/// `this+0xd78`: the tyre state's effective radius less the sweep's distance, when the sweep hits nearer (else 0); the
/// tyre step takes it (named by use).
pub const SWEEP_RESIDUE: u32 = 0xD78;
/// `this+0xd7c`: the object of the nearest hit with bit 4 of its flags (0 for none); the step takes its point velocity.
pub const GROUND_BODY: u32 = 0xD7C;
/// `this+0xa60`: the hub's velocity in the world: the car's velocity at the hub plus the strut's velocity along the
/// knuckle's axis (`M3DD30`). `Unk97a0` adds its scaled cross product to it.
pub const HUB_VELOCITY: u32 = 0xA60;
/// `this+0xe40`: the across-normal part of the velocity of the last `Unk94d0` (name ours).
pub const EDGE_ACROSS: u32 = 0xE40;
/// `this+0xe70`, `+0xe74`, `+0xe78`, `+0xe7c`: the step's anti-roll bar, spring, damper and strut forces.
pub const ROLL_BAR_FORCE: u32 = 0xE70;
pub const SPRING_FORCE: u32 = 0xE74;
pub const DAMPER_FORCE: u32 = 0xE78;
pub const STRUT_FORCE: u32 = 0xE7C;

/// The CDef's inverse mass (`+0x16c`, `CSolid::CDef`'s InverseMass, `m_inverseMass`): the strut step's `m`.
pub const DEF_INVERSE_MASS: u32 = 0x16C;

/// The CPhysics global (`g_1659b70`, `car_body::layout::WORLD`) points at the CPhysics; its `+0x24` is the CDef
/// (`RBR_E51_WheelB.cpp:151`, `:203`).
pub const WORLD_DEF: u32 = 0x24;
/// The CDef's material manager (`+0x18`, `RBR_E51_WheelB.cpp:152`).
pub const DEF_MATERIALS: u32 = 0x18;
/// The CDef's flags word (`+0x160cc`) and the tag it gives the block's pair (`+0x1614c`, `RBR_E51_WheelB.cpp:203`).
pub const DEF_FLAGS: u32 = 0x160CC;
pub const DEF_PAIR_TAG: u32 = 0x1614C;
/// The flag bit that makes `M3D460` take the pair tag.
pub const FLAG_PAIR_TAG: u32 = 0x40000;
/// A material's fields `M3D230` reads: the divisor (`+0xd8`), the gain (`+0xc8`) and the two lerped values (`+0x18`,
/// `+0xb8`). The tag entry is `physics_core::layout::material::tag_entry`.
pub const MAT_PERIOD: u32 = 0xD8;
pub const MAT_GAIN: u32 = 0xC8;
pub const MAT_LERP_A: u32 = 0x18;
pub const MAT_LERP_B: u32 = 0xB8;

/// The wheel's strut, a `CSpringDamper` at `this+0xec4` (`RBR_E51_WheelB.cpp:131`, `M3D030`); its name object is read
/// through `CBase`'s `m_name` (+4, `RBR_E48_NTweak.h:183`).
pub const STRUT: u32 = 0xEC4;
pub const CBASE_NAME: u32 = 0x4;
/// The `CBase` whose dirty flag `M3D030` sets (`this+0x5b0`).
pub const STRUT_TWEAK: u32 = 0x5B0;
/// `this+0x5bc`: the strut's length: `M3D030` stores the blend of the name's floats here, and `M3DD30` and `M3B900`
/// read it as the strut's travel (`m_strutLength`, rbr-recomp `notes/physics/renames/wheel.csv`).
pub const STRUT_LENGTH: u32 = 0x5BC;
/// `this+0x5c0`: the real of the strut's velocity (its value is `STRUT_VEL`), made dirty before the step's store.
pub const STRUT_VEL_TWEAK: u32 = 0x5C0;
/// `this+0x5cc`: the strut's velocity, clamped to +-100 (`m_strutVel`, rbr-recomp `notes/physics/wheel.md` 2a).
pub const STRUT_VEL: u32 = 0x5CC;
/// The strut definition's `+0x28` (`InnerBumpRubberPos`, the bump stop) and `+0x38` (`OuterBumpRubberPos`, the droop stop).
pub const STRUT_INNER_BUMP_POS: u32 = 0x28;
pub const STRUT_OUTER_BUMP_POS: u32 = 0x38;
// The typed inputs and outputs of the ports. Each `load_*` reads the game memory a port takes, and each `store_*` writes
// what the port returns; the ports never touch `Mem`.

/// The Mem store: the bodies are the layout's reads and writes.
impl WheelStore for Mem {
    /// Loads the ray block at `at` (the hit word is read first, as the original does).
    fn load_ray_block(&self, at: u32) -> RayBlock {
        RayBlock {
            normal: (self.u32(at + RAY_HIT) != 0).then(|| Vec4::load(self, at + RAY_NORMAL)),
            offset: self.f32(at + RAY_OFFSET),
            difference: self.f32(at + BLOCK_DIFF),
        }
    }

    /// The tyre's setup: the pointer at `this+0xf10`.
    fn tyre_def(&self, this: u32) -> u32 {
        self.u32(this + TYRE_DEF)
    }

    /// The tyre's `HubOffset` (`tyre_def+0x6c`).
    fn load_hub_offset(&self, this: u32) -> f32 {
        self.f32(self.tyre_def(this) + TYRE_DEF_HUB_OFFSET)
    }

    /// The tyre's `CarcassWidth` (`tyre_def+0xcc`).
    fn load_carcass_width(&self, this: u32) -> f32 {
        self.f32(self.tyre_def(this) + TYRE_DEF_CARCASS_WIDTH)
    }

    /// The car pointer (`this+0x7f4`).
    fn load_car(&self, this: u32) -> u32 {
        self.u32(this + CAR)
    }

    /// A wheel vector at `this + offset`.
    fn load_at(&self, this: u32, offset: u32) -> Vec4 {
        Vec4::load(self, this + offset)
    }

    /// Stores a wheel vector at `this + offset`.
    fn store_at(&mut self, this: u32, offset: u32, v: Vec4) {
        v.store(self, this + offset);
    }

    /// The CDef pointer (`this+0x34`).
    fn load_def(&self, this: u32) -> u32 {
        self.u32(this + DEF)
    }

    /// The car `M3A8A0` reads (`this+0x44`), before its flag test.
    fn load_xform_car(&self, this: u32) -> u32 {
        self.u32(this + XFORM_CAR)
    }

    /// The flags byte of a car (`car+0x60`).
    fn car_flags(&self, car: u32) -> u8 {
        self.u8(car + CAR_FLAGS)
    }

    /// A float of the CDef at `off`.
    fn load_def_f32(&self, this: u32, off: u32) -> f32 {
        self.f32(self.load_def(this) + off)
    }

    /// The damage record pointer (`this+0x7f0`).
    fn load_damage(&self, this: u32) -> u32 {
        self.u32(this + DAMAGE)
    }

    /// A float of the damage record at `off` (`DAMAGE_*`).
    fn load_damage_f32(&self, this: u32, off: u32) -> f32 {
        self.f32(self.load_damage(this) + off)
    }

    /// The angle `M3A9E0` reads (`this+0x5ac`).
    fn load_angle(&self, this: u32) -> f32 {
        self.f32(this + WHEEL_ANGLE)
    }

    /// A float of this at `at` (the wheel's scalars).
    fn load_f32(&self, this: u32, off: u32) -> f32 {
        self.f32(this + off)
    }

    /// Stores a float of this at `off`.
    fn store_f32(&mut self, this: u32, off: u32, value: f32) {
        self.set_f32(this + off, value);
    }

    /// Stores a float of the damage record at `off` (`DAMAGE_*`).
    fn store_damage_f32(&mut self, this: u32, off: u32, value: f32) {
        self.set_f32(self.load_damage(this) + off, value);
    }

    /// Stores an int at `this + off`.
    fn store_u32(&mut self, this: u32, off: u32, value: u32) {
        self.set_u32(this + off, value);
    }

    /// A vector of the CDef at `off` (`DEF_*`).
    fn load_def_vec(&self, this: u32, off: u32) -> Vec4 {
        Vec4::load(self, self.load_def(this) + off)
    }

    /// The CDef the CPhysics global points at.
    fn world_def(&self) -> u32 {
        self.u32(self.u32(crate::car_body::layout::WORLD) + WORLD_DEF)
    }

    fn load_axle_wobble(&self, this: u32) -> AxleWobbleIn {
        AxleWobbleIn {
            hub_damage: self.load_damage_f32(this, DAMAGE_HUB),
            axle: self.load_at(this, INCLINED_AXLE),
            side: self.load_at(this, INCLINED_SIDE),
            wheel_angle: self.load_angle(this),
            previous_w: self.f32(this + WOBBLED_AXLE + 12),
        }
    }

    fn store_wobbled_axle(&mut self, this: u32, v: Vec4) {
        self.store_at(this, WOBBLED_AXLE, v);
    }

    fn load_inclination(&self, this: u32) -> Inclination {
        Inclination {
            hub_damage: self.load_damage_f32(this, DAMAGE_HUB),
            damage_angle: self.load_def_f32(this, DEF_INCLINATION_DAMAGE_ANGLE),
            axis_inclination: self.load_def_f32(this, DEF_AXIS_INCLINATION),
        }
    }

    fn store_inclined_axle(&mut self, this: u32, axle: &InclinedAxle) {
        axle.axle.store(self, this + INCLINED_AXLE);
        axle.side.store(self, this + INCLINED_SIDE);
    }

    fn load_top_mount_deform(&self, this: u32) -> TopMountDeformIn {
        TopMountDeformIn {
            up_damage: self.load_damage_f32(this, DAMAGE_TOP_MOUNT_UP),
            side_damage: self.load_damage_f32(this, DAMAGE_TOP_MOUNT_SIDE),
            max_up: self.load_def_f32(this, DEF_MAX_DEFORM_UP),
            max_side: self.load_def_f32(this, DEF_MAX_DEFORM_SIDE),
            up_axis: self.load_def_vec(this, DEF_TOP_MOUNT_UP_AXIS),
            mount: self.load_def_vec(this, DEF_TOP_MOUNT),
            side_axis: self.load_def_vec(this, DEF_TOP_MOUNT_SIDE_AXIS),
        }
    }

    fn store_deformed_top_mount(&mut self, this: u32, v: Vec4) {
        self.store_at(this, DEFORMED_TOP_MOUNT, v);
    }

    fn load_hub_damage(&self, this: u32, torque: f32) -> HubDamageIn {
        HubDamageIn {
            torque,
            threshold: self.load_def_f32(this, DEF_HUB_DAMAGE_THRESHOLD),
            max_torque: self.load_def_f32(this, DEF_HUB_DAMAGE_MAX_TORQUE),
            hub_damage: self.load_damage_f32(this, DAMAGE_HUB),
            inclination: self.load_inclination(this),
        }
    }

    /// Stores the damage update and sets the changed flag. Nothing is written when `update` is `None`.
    fn store_hub_damage(&mut self, this: u32, update: Option<&HubDamageUpdate>) {
        if let Some(update) = update {
            self.store_damage_f32(this, DAMAGE_HUB, update.hub_damage);
            self.store_inclined_axle(this, &update.axle);
            self.store_u32(this, DAMAGE_CHANGED, 1);
        }
    }

    /// The CDef words `M3B900` reads (`this+0x34`).
    fn load_suspension_def(&self, this: u32) -> SuspensionDef {
        let def = self.load_def(this);
        SuspensionDef {
            steering_rod_length: self.f32(def + DEF_STEERING_ROD_LENGTH),
            wishbone_header: [self.u32(def + DEF_WISHBONE_HEADER), self.u32(def + DEF_WISHBONE_HEADER + 4)],
            wishbone_length: self.f32(def + DEF_WISHBONE_LENGTH),
            joint_mount: Vec3::load(self, def + DEF_JOINT_MOUNT),
            joint_axis: Vec4::load(self, def + DEF_JOINT_AXIS),
            wishbone_sign: self.f32(def + DEF_WISHBONE_SIGN),
            strut_dim_x: self.f32(def + DEF_STRUT_DIM_X),
            strut_dim_z: self.f32(def + DEF_STRUT_DIM_Z),
            steering_sign: self.f32(def + DEF_STEERING_SIGN),
            strut_rod_mount: Vec3::load(self, def + DEF_STRUT_ROD_MOUNT),
            strut_hub_point: Vec3::load(self, def + DEF_STRUT_HUB_POINT),
            is_steered: self.u32(def + DEF_IS_STEERED) != 0,
            drive_axle_mount_offset: self.f32(def + DEF_DRIVE_AXLE_MOUNT_OFFSET),
            steering_mount: Vec4::load(self, def + DEF_STEERING_MOUNT),
            steering_axis: Vec4::load(self, def + DEF_STEERING_AXIS),
            drive_axle_joint: Vec3::load(self, def + DEF_DRIVE_AXLE_JOINT),
        }
    }

    /// The debug flag word, `[[0x1659b70] + 0x24] + 0x160e0`: bit 2 is the debug flag `M3B900` tests, bit 0x400 gates the
    /// travel in `M3B880`.
    fn flag_word(&self) -> u32 {
        let global = self.u32(DEBUG_GLOBAL);
        self.u32(self.u32(global + 0x24) + 0x160E0)
    }

    fn debug_flag(&self) -> bool {
        self.flag_word() & 4 != 0
    }
}
