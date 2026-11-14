//! `E51Wheel::CopyCF30` (the setup copy), `CWheel::Unk9900`/`Unk9660`/`Unk96e0`, the solid refresh and
//! `E51MultiTire::Emit`.

use super::fields::{
    BLEND_OPS, DAMAGE_SECTORS_A, DAMAGE_SECTORS_B, DEF, DEF_AXIS_INCLINATION, DEF_INCLINED_AXLE, DEF_MATERIALS,
    DEF_PLATFORM_HEIGHT, DEF_PLATFORM_HEIGHT_HEADER, DEF_STEERING_ROD_LENGTH_HEADER, DEF_STRUT_DIM_Z_KEY,
    DEF_STRUT_DIM_Z_TWEAK, DEF_TOP_MOUNT_POSITION, LAST_TAG_1, LAST_TAG_2, LAST_WEIGHT, SECTOR_VALUE_A, SECTOR_VALUE_B,
    SETUP_AXIS_INCLINATION, SETUP_PLATFORM_HEIGHT, SETUP_PLATFORM_HEIGHT_HEADER, SETUP_STEERING_ROD_LENGTH,
    SETUP_STEERING_ROD_LENGTH_HEADER, SETUP_TOP_MOUNT, SETUP_TOP_MOUNT_POSITION, STATE, STRUT, TYRE_SCRATCH,
};
use super::host::{BlendCalls, EmbeddedCalls, EmitCalls, SubBlockInit};
use super::probe::WheelTuning;
use super::refresh_pose::{DEF_STEERING_ROD_LENGTH, DEF_STRUT_DIM_Z};
use super::store::WheelStore;
use crate::math::vec::Vec4;
use crate::mem::Mem;
use crate::physics_core::layout::material;
use crate::rigid_body::host::HostUnavailable;
use crate::wheel::axes::Trig;
use crate::wheel::sector;
use crate::{ntweak, spring_damper, tyre};

/// A material entry's surface class (`+0x148`) and `WaterFactor` (`+0xe8`, 0 dry to 1 wet).
const MAT_CLASS: u32 = 0x148;
const MAT_WATER_FACTOR: u32 = 0xE8;

/// Copies the dword at `from` to `to`.
fn copy_word(mem: &mut Mem, from: u32, to: u32) {
    let v = mem.u32(from);
    mem.set_u32(to, v);
}

/// The calls `CopyCF30` makes outside the wheel, one method each (the sine and cosine are `axes::Trig`). The
/// objects are addresses, `this`-relative.
pub trait DefCopyCalls {
    /// `CSpringDamper::CopyStateToDef(setup)` (0x52dbd0, thiscall, one stack word) on the strut object `strut`
    /// (`this+0xec4`).
    fn copy_state_to_def(&mut self, mem: &mut Mem, strut: u32, setup: u32);
    /// `CTyre::CScratch::CopyDefFrom(setup)` (0x534760, thiscall, one stack word) on the tyre scratch `scratch`
    /// (`this+0xf00`).
    fn copy_def_from(&mut self, mem: &mut Mem, scratch: u32, setup: u32);
    /// `NTweak::CBase::SetDirty` (0x52f450, thiscall, no arguments) on the CDef's strut z-extent real `base` (`def+0x3f0`).
    fn set_dirty(&mut self, mem: &mut Mem, base: u32);
}

/// The three calls as the ports make them: `spring_damper::layout::apply_setup`, `tyre::layout::copy_def_from` and
/// `ntweak::set_dirty`.
pub struct Ported;

impl DefCopyCalls for Ported {
    fn copy_state_to_def(&mut self, mem: &mut Mem, strut: u32, setup: u32) {
        spring_damper::layout::apply_setup(mem, strut, setup);
    }

    fn copy_def_from(&mut self, mem: &mut Mem, scratch: u32, setup: u32) {
        tyre::layout::copy_def_from(mem, scratch, setup);
    }

    fn set_dirty(&mut self, mem: &mut Mem, base: u32) {
        ntweak::set_dirty(mem, base);
    }
}

/// `E51Wheel::CopyCF30` (0x53cf30, thiscall, stack `setup`, `damper_setup`, `tyre_setup`): the wheel's `CSetup` into its
/// CDef. The strut copies its state from `damper_setup` and the tyre scratch copies its CDef from `tyre_setup` (host
/// calls); then the setup's `SteeringRodLength` real (header and value) goes to the CDef's, its `vecTopMountPosition`
/// to `vecTopMount` (null when `setup` is -0x20, as the asm tests the tweak's address), its `StrutPlatformHeight` real
/// to the CDef's real at `+0x410`, `SetDirty` on the strut's z-extent real (a host call), `StrutDimZ` plus the platform
/// height to that real's value, and the sine and cosine of `WheelAxisInclination` to the CDef's inclined axle (its y
/// zeroed). The angle itself is copied to `DEF_AXIS_INCLINATION`.
pub fn apply_setup(
    host: &mut impl DefCopyCalls,
    trig: &mut impl Trig,
    mem: &mut Mem,
    this: u32,
    setup: u32,
    damper_setup: u32,
    tyre_setup: u32,
) {
    host.copy_state_to_def(mem, this + STRUT, damper_setup);
    host.copy_def_from(mem, this + TYRE_SCRATCH, tyre_setup);
    let def = mem.u32(this + DEF);
    copy_word(mem, setup + SETUP_STEERING_ROD_LENGTH_HEADER, def + DEF_STEERING_ROD_LENGTH_HEADER);
    copy_word(mem, setup + SETUP_STEERING_ROD_LENGTH_HEADER + 4, def + DEF_STEERING_ROD_LENGTH_HEADER + 4);
    copy_word(mem, setup + SETUP_STEERING_ROD_LENGTH, def + DEF_STEERING_ROD_LENGTH);
    let source = if setup.wrapping_add(SETUP_TOP_MOUNT) == 0 { 0 } else { setup + SETUP_TOP_MOUNT_POSITION };
    Vec4::load(mem, source).store(mem, def + DEF_TOP_MOUNT_POSITION);
    copy_word(mem, setup + SETUP_PLATFORM_HEIGHT_HEADER, def + DEF_PLATFORM_HEIGHT_HEADER);
    copy_word(mem, setup + SETUP_PLATFORM_HEIGHT_HEADER + 4, def + DEF_PLATFORM_HEIGHT_HEADER + 4);
    copy_word(mem, setup + SETUP_PLATFORM_HEIGHT, def + DEF_PLATFORM_HEIGHT);
    let sum = mem.f32(def + DEF_STRUT_DIM_Z_KEY) + mem.f32(setup + SETUP_PLATFORM_HEIGHT);
    host.set_dirty(mem, def + DEF_STRUT_DIM_Z_TWEAK);
    mem.set_f32(def + DEF_STRUT_DIM_Z, sum);
    let angle = f32::from_bits(mem.u32(setup + SETUP_AXIS_INCLINATION));
    let sine = trig.sin(angle) as f32;
    let cosine = trig.cos(angle) as f32;
    mem.set_f32(def + DEF_INCLINED_AXLE, cosine);
    mem.set_f32(def + DEF_INCLINED_AXLE + 4, 0.0);
    mem.set_f32(def + DEF_INCLINED_AXLE + 8, sine);
    copy_word(mem, setup + SETUP_AXIS_INCLINATION, def + DEF_AXIS_INCLINATION);
}

/// `CWheel::Unk9900` (0x539900, `blend_tags`): the material entries of the two tags `x` and `y` (from the CDef's manager),
/// the scratch's coefficients `BlendTable` returns, the blend of the two entries on the blend ops, then the stores of `x`,
/// `y` and `z`. The material entries are read before the host calls, as the asm reads them.
pub fn blend_tags<S: WheelStore>(host: &mut impl BlendCalls, mem: &mut S, this: u32, x: u32, y: u32, z: f32) {
    let manager = mem.world_def() + DEF_MATERIALS;
    let s1 = material::tag_entry(mem.mem(), manager, x);
    let s2 = material::tag_entry(mem.mem(), manager, y);
    let (a, b) = host.blend_coefficients(
        this + TYRE_SCRATCH,
        mem.u32(s1 + MAT_CLASS),
        mem.u32(s2 + MAT_CLASS),
        mem.f32(s1 + MAT_WATER_FACTOR),
        mem.f32(s2 + MAT_WATER_FACTOR),
    );
    host.blend_materials(this + BLEND_OPS, s1, s2, a, b, z);
    mem.set_u32(this + LAST_TAG_1, x);
    mem.set_u32(this + LAST_TAG_2, y);
    mem.set_f32(this + LAST_WEIGHT, z);
}

/// The surface classes (`+0x148`) of the two materials the last `blend_tags` blended on the wheel whose tyre scratch is
/// at `scratch`, and the second one's weight: what the tyre type's surface friction took. Read only (ours, for a tyre
/// model: `rbr_physics::tyre::model::ForceInput::surface_classes`).
pub fn surface_classes(mem: &Mem, scratch: u32) -> ([u32; 2], f32) {
    let this = scratch.wrapping_sub(TYRE_SCRATCH);
    let manager = mem.world_def() + DEF_MATERIALS;
    let class = |tag: u32| mem.u32(material::tag_entry(mem, manager, tag) + MAT_CLASS);
    ([class(mem.u32(this + LAST_TAG_1)), class(mem.u32(this + LAST_TAG_2))], mem.f32(this + LAST_WEIGHT))
}

/// `CWheel::Unk9660` (0x539660, thiscall with `p` and `n`): `sector::pick_sector` of `this+0x5ac`, written to `p` in the
/// asm's order (`p[0]`, the float fraction at `p[2]`, then `p[1]`). Returns the wrap count.
pub fn pick_sector<S: WheelStore>(mem: &mut S, this: u32, p: u32, n: u32) -> u32 {
    let pick = sector::pick_sector(mem.load_angle(this), n);
    mem.set_u32(p, pick.first);
    mem.set_f32(p + 8, pick.first_weight);
    mem.set_u32(p + 4, pick.next);
    pick.next_wraps
}

/// `CWheel::Unk96e0` (0x5396e0, thiscall, plain `ret`): `Init` on the local `li` (a host call), `pick_sector` with `n = 4`
/// into `li`, then the two lerps of the damage record's sector tables. Table A goes to `this+0xd70` (weight 0.15), table
/// B to `this+0xd74` (weight 0); both from the base 0.2. NGP makes the base and the first weight per-car (`tuning`).
/// Returns the damage record pointer, the asm's last `eax`.
pub fn table_lerp_pair<S: WheelStore>(
    init: &mut impl SubBlockInit,
    mem: &mut S,
    this: u32,
    li: u32,
    tuning: WheelTuning,
) -> u32 {
    init.init_sub_block(li);
    pick_sector(mem, this, li, 4);
    let damage = mem.load_damage(this);
    let first = mem.u32(li);
    let next = mem.u32(li + 4);
    let first_weight = mem.f32(li + 8);
    let a = sector::interpolate(
        mem.f32(damage + DAMAGE_SECTORS_A + 4 * first),
        mem.f32(damage + DAMAGE_SECTORS_A + 4 * next),
        first_weight,
    );
    mem.set_f32(this + SECTOR_VALUE_A, sector::mix_toward(a, tuning.pose, tuning.weight));
    let b = sector::interpolate(
        mem.f32(damage + DAMAGE_SECTORS_B + 4 * first),
        mem.f32(damage + DAMAGE_SECTORS_B + 4 * next),
        first_weight,
    );
    mem.set_f32(this + SECTOR_VALUE_B, sector::mix_toward(b, tuning.pose, 0.0));
    damage
}

/// `E51Wheel::M3B5B0` (0x53b5b0, the wheel's slot 16, thiscall, no stack words): the solid's own slot 16 (`CSolid::Vf40`,
/// its refresh) on the wheel, then a tail jump to slot 17 of the wheel's state at `this+0x3d0` (its rotation refresh).
/// Returns that tail call's eax, which the wheel returns.
pub fn refresh_solid(host: &mut impl EmbeddedCalls, this: u32) -> Result<u32, HostUnavailable> {
    host.base_refresh(this)?;
    host.state_rotation(this + STATE)
}

/// `CMultiSignal1<CSigTireParticles>` of `E51MultiTire::Emit` (0x53d0d0): the guard word, the count pointer, the slots
/// (16 bytes each, from +8, one per count) and the overflow list head at +0x208. The template layout is the physics
/// core's signal layout (`physics_core::layout::signal_emit`).
const TIRE_GUARD: u32 = 0x0;
const TIRE_COUNT: u32 = 0x4;
const TIRE_SLOTS: u32 = 0x8;
const TIRE_OVERFLOW: u32 = 0x208;
/// A signal has 32 slots; the count's limit.
const TIRE_SLOT_LIMIT: u32 = 0x20;
/// A connection's `Invoke` is vtable slot 1 (+4); its list link is at +4.
const INVOKE_SLOT: u32 = 4;
const CONN_NEXT: u32 = 4;

/// `E51MultiTire::Emit` (0x53d0d0, thiscall, `ret 4`): `sig` goes to each connection of the overflow list, then of the
/// slot the count selects. A null count pointer returns silently. A set guard logs and returns. An out-of-range count logs
/// and returns with the guard still set. The original's log calls leave their five arguments on the stack, so they do not
/// return into the emit; the port stops after each log for the same reason.
pub fn tire_emit<S: WheelStore>(mem: &mut S, host: &mut impl EmitCalls, this: u32, sig: u32) {
    let count_at = mem.u32(this + TIRE_COUNT);
    if count_at == 0 {
        return;
    }
    if mem.u32(this + TIRE_GUARD) != 0 {
        host.log_recursive();
        return;
    }
    mem.set_u32(this + TIRE_GUARD, 1);
    let count = mem.u32(count_at);
    if count >= TIRE_SLOT_LIMIT {
        host.log_range(count);
        return;
    }
    let slot = this + TIRE_SLOTS + 16 * count;
    let overflow = this + TIRE_OVERFLOW;
    if mem.u32(slot) != 0 || mem.u32(overflow) != 0 {
        for head in [overflow, slot] {
            let mut conn = mem.u32(head);
            while conn != 0 {
                let target = mem.u32(mem.u32(conn) + INVOKE_SLOT);
                host.invoke(target, conn, sig);
                conn = mem.u32(conn + CONN_NEXT);
            }
        }
    }
    mem.set_u32(this + TIRE_GUARD, 0);
}
