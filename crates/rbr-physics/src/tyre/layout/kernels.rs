//! The blocks the tyre functions read and write through their callers' pointers (`Locate`'s table and segment,
//! `CTyreMatData::CopyFrom`'s material, the contact solve's blocks, the output pointers of `Step`, `StepInner`,
//! `ComputeOutputs` and `BlendTable`, `Step`'s stack words), the scratch's pointer words, and the small tyre calls
//! (the tail getters, the noise signal). All on the image: the tyre's own state is the typed state's (`typed.rs`).

use crate::math::vec::Vec4;
use crate::mem::Mem;

use super::frame_types::{Indentation, Segment};
use super::{Table, DEF, STATE, WORDS};
use crate::physics_core;
use crate::physics_core::layout::SignalHost;
use crate::tyre::friction_limits::FrictionLimits;
use crate::tyre::material::Material;
use crate::tyre::output_block::OutputBlock;
use crate::tyre::patch_forces::PatchForces;
use crate::tyre::solve::{Coefficients, Contact};
use crate::tyre::surface::SurfaceFriction;

/// `this+0x410`: the contact record (the wheel), whose `+0x5ac` is the contact angle.
pub(super) const RECORD: u32 = 0x410;
/// `this+0x414`: the wear table (`m_wear`, eight floats in the damage handler).
pub(super) const WEAR_TABLE: u32 = 0x414;
/// The contact record's contact angle.
pub(super) const RECORD_CONTACT_ANGLE: u32 = 0x5AC;
/// `this+0x420`: the tyre-noise signal block (`m_sig`, 24 words); its second word is cleared after the emit.
const SIGNAL_BLOCK: u32 = 0x420;

/// `v * s + base` over the four lanes (the asm's `mulps` by `s`, then `addps` to `base`).
pub fn scaled_sum(v: Vec4, s: f32, base: Vec4) -> Vec4 {
    Vec4 { x: v.x * s + base.x, y: v.y * s + base.y, z: v.z * s + base.z, w: v.w * s + base.w }
}

/// The address of word `index` of the table at `base`. The indices are Locate's `lo`, which is -1 (0xffffffff) when a NaN
/// input walks below the table, so the address wraps as the original's 32-bit one does.
pub fn word_at(base: u32, index: u32) -> u32 {
    base.wrapping_add(index.wrapping_mul(4))
}

/// The solve coefficients from their eight words.
pub fn coefficients_from_words(w: [f32; 8]) -> Coefficients {
    Coefficients {
        quadratic: w[0],
        linear: w[1],
        tread_stiffness: w[2],
        sink_factor: w[3],
        tread_area: w[4],
        groove_area: w[5],
        groove_soil_stiffness: w[6],
        tread_soil_stiffness: w[7],
    }
}

/// The stack words of the tyre step as typed values, loaded once at entry. `torque_in` is the caller's torque vector,
/// which the forces add to.
#[derive(Clone, Copy, Debug)]
pub struct StepArgs {
    /// The wheel's spin, rad/s, and the time step, s.
    pub spin: f32,
    pub dt: f32,
    /// The height of the hub above the surface, m, and the scale of the slipped area.
    pub hub_height: f32,
    pub slip_area_scale: f32,
    pub material: Material,
    pub debug_words: [u32; 3],
    /// The game's debug flag that copies the debug words into the scratch.
    pub copy_debug_words: bool,
    /// The surface normal and the wheel's axle direction.
    pub normal: Vec4,
    pub axle: Vec4,
    /// The velocity and the position of the contact point.
    pub velocity: Vec4,
    pub contact_point: Vec4,
    pub torque_in: Vec4,
}

/// Where the tyre step stores its outputs.
#[derive(Clone, Copy, Debug)]
pub struct StepPtrs {
    pub force: u32,
    pub torque: u32,
    pub reaction_torque: u32,
    pub sticking_load: u32,
    pub output_block: u32,
}

/// The outputs of the tyre step. The no-contact path sets the force, the torque and the reaction torque to zero and
/// leaves the sticking load and the output block as they were, so those are `None` there.
#[derive(Clone, Copy, Debug)]
pub struct StepOut {
    /// The force and the torque on the wheel.
    pub force: Vec4,
    pub torque: Vec4,
    /// The rolling resistance torque plus the torsion torque, N·m.
    pub reaction_torque: f32,
    /// The normal force scaled by 0.025 and the sticking weight (named by use).
    pub sticking_load: Option<f32>,
    pub block: Option<OutputBlock>,
}

/// `CTyre::CScratch::EmitNoise` (0x535890, thiscall, no arguments): emits the tyre-noise signal with the scratch's signal
/// block (`m_sig`, +0x420), then clears the block's second word (+0x424). On the image: the emit's connections read the
/// block at its address.
pub fn emit_noise(mem: &mut Mem, host: &mut impl SignalHost, this: u32) {
    let signal = noise_signal(mem);
    let sig = this + SIGNAL_BLOCK;
    physics_core::layout::emit(mem, host, signal, sig);
    mem.set_u32(sig + 4, 0);
}

/// `CU32Value8::Clear` (0x5317f0, thiscall): the eight floats at `this` become 0.0.
pub fn clear_eight_floats(mem: &mut Mem, this: u32) {
    for k in 0..8 {
        mem.set_f32(this + 4 * k, 0.0);
    }
}

/// Loads a [`Segment`] at `at`: `lo`, `hi` and `frac` in three words.
pub fn load_segment(mem: &Mem, at: u32) -> Segment {
    Segment { lo: mem.u32(at), hi: mem.u32(at + 4), frac: mem.f32(at + 8) }
}

/// Stores a [`Segment`] at `at`.
pub fn store_segment(mem: &mut Mem, at: u32, s: &Segment) {
    mem.set_u32(at, s.lo);
    mem.set_u32(at + 4, s.hi);
    mem.set_f32(at + 8, s.frac);
}

/// The table of `len` entries at `at`, with the word below it.
pub fn read_table(mem: &Mem, at: u32, len: usize) -> Table {
    Table {
        below: mem.f32(at - 4),
        len,
        entries: std::array::from_fn(|k| if k < len { mem.f32(at + 4 * k as u32) } else { 0.0 }),
    }
}

/// Loads the copied words of the material block at `at`.
pub fn load_material(mem: &Mem, at: u32) -> Material {
    std::array::from_fn(|i| mem.u32(at + 4 * WORDS[i]))
}

/// `CTyreMatData::CopyFrom` (0x535040)'s store: the copied words into the block at `at`; the other words are left as
/// they are.
pub fn store_material(mem: &mut Mem, at: u32, material: &Material) {
    for (i, word) in material.iter().enumerate() {
        mem.set_u32(at + 4 * WORDS[i], *word);
    }
}

/// `CScratch::Set414` (0x532490): the wear table pointer at `this+0x414`, the eight sector wear words the heat step reads
/// and writes. A pointer: the image's.
pub fn store_wear_table(mem: &mut Mem, this: u32, table: u32) {
    mem.set_u32(this + WEAR_TABLE, table);
}

/// The contact solve's blocks: the indentation (7 words) at `indentation`, the forces (3 words) at `forces` and the
/// coefficients (8 words) at `coefficients`.
pub fn load_contact_blocks(
    mem: &Mem,
    indentation: u32,
    forces: u32,
    coefficients: u32,
) -> (Indentation, [f32; 3], [f32; 8]) {
    (Indentation::from_words(mem.f32s(indentation)), mem.f32s(forces), mem.f32s(coefficients))
}

/// Stores the solved contact's indentation and forces into their blocks.
pub fn store_contact_blocks(mem: &mut Mem, indentation: u32, forces: u32, c: &Contact) {
    for (k, v) in c.indentation.to_words().into_iter().enumerate() {
        mem.set_f32(indentation + 4 * k as u32, v);
    }
    for (k, v) in [c.normal_force, c.carcass_force, c.tread_force].into_iter().enumerate() {
        mem.set_f32(forces + 4 * k as u32, v);
    }
}

/// Stores the patch forces: `ptrs[0]` the two forces (three floats), `ptrs[1]` zeroed, then the pneumatic trail and the
/// two slip phases.
pub fn store_patch_forces(mem: &mut Mem, ptrs: [u32; 5], out: &PatchForces) {
    for (i, value) in [out.lateral_force, out.longitudinal_force, 0.0].iter().enumerate() {
        mem.set_f32(ptrs[0] + 4 * i as u32, *value);
    }
    Vec4::default().store(mem, ptrs[1]);
    mem.set_f32(ptrs[2], out.pneumatic_trail);
    mem.set_f32(ptrs[3], out.lateral_phase);
    mem.set_f32(ptrs[4], out.longitudinal_phase);
}

/// The stiffnesses `ComputeOutputs` passes through, at `src+0x36c` and `+0x370`.
pub fn load_limit_stiffnesses(mem: &Mem, src: u32) -> [f32; 2] {
    [mem.f32(src + 0x36C), mem.f32(src + 0x370)]
}

/// Stores the friction limits through the ten output pointers (the stiffnesses first, then the limits in the struct's
/// order).
pub fn store_friction_limits(mem: &mut Mem, ptrs: [u32; 10], out: &FrictionLimits) {
    mem.set_f32(ptrs[0], out.cornering_stiffness);
    mem.set_f32(ptrs[1], out.traction_stiffness);
    mem.set_f32(ptrs[2], out.lateral_peak);
    mem.set_f32(ptrs[3], out.lateral_kinetic);
    mem.set_f32(ptrs[4], out.longitudinal_peak);
    mem.set_f32(ptrs[5], out.longitudinal_kinetic);
    mem.set_f32(ptrs[6], out.lateral_peak_slip);
    mem.set_f32(ptrs[7], out.lateral_end_slip);
    mem.set_f32(ptrs[8], out.longitudinal_peak_slip);
    mem.set_f32(ptrs[9], out.longitudinal_end_slip);
}

/// The game's debug flags for `Step`: the word at `g_1659b70 -> +0x24 -> +0x160e0` (`& 0x20`). When it is set, `Step`
/// copies its `f` debug_words to `this+0x4c8`. The three words are read here, and the test maps them as node windows.
pub fn load_step_flag(mem: &Mem) -> bool {
    let physics = mem.u32(0x0165_9B70);
    let def = mem.u32(physics + 0x24);
    mem.u32(def + 0x160E0) & 0x20 != 0
}

/// Stores the output block at `out`.
pub fn store_output_block(mem: &mut Mem, out: u32, b: &OutputBlock) {
    b.edge_a.store(mem, out + 0x10);
    b.edge_b.store(mem, out + 0x20);
    b.motion.store(mem, out + 0x50);
    mem.set_f32(out + 0x80, b.mark_a);
    mem.set_f32(out + 0x84, b.mark_b);
    mem.set_f32(out + 0x88, b.soil_flow);
    mem.set_u32(out + 0x90, b.sink_depth_bits);
    mem.set_u32(out + 0x94, b.slip_phase_bits);
    mem.set_f32(out + 0x98, b.slip_area);
    mem.set_u32(out + 0xAC, b.valid);
}

/// Loads the sixteen stack words of the tyre step.
pub fn load_step(mem: &Mem, w: &[u32]) -> (StepArgs, StepPtrs) {
    let args = StepArgs {
        spin: f32::from_bits(w[0]),
        dt: f32::from_bits(w[1]),
        hub_height: f32::from_bits(w[2]),
        slip_area_scale: f32::from_bits(w[3]),
        material: load_material(mem, w[4]),
        debug_words: std::array::from_fn(|k| mem.u32(w[5] + 4 * k as u32)),
        copy_debug_words: load_step_flag(mem),
        normal: Vec4::load(mem, w[6]),
        axle: Vec4::load(mem, w[8]),
        velocity: Vec4::load(mem, w[9]),
        contact_point: Vec4::load(mem, w[10]),
        torque_in: Vec4::load(mem, w[12]),
    };
    let ptrs =
        StepPtrs { force: w[11], torque: w[12], reaction_torque: w[13], sticking_load: w[14], output_block: w[15] };
    (args, ptrs)
}

/// Stores the outputs of the tyre step (the `None` fields are not written).
pub fn store_step(mem: &mut Mem, p: &StepPtrs, out: &StepOut) {
    out.force.store(mem, p.force);
    out.torque.store(mem, p.torque);
    mem.set_f32(p.reaction_torque, out.reaction_torque);
    if let Some(load) = out.sticking_load {
        mem.set_f32(p.sticking_load, load);
    }
    if let Some(block) = &out.block {
        store_output_block(mem, p.output_block, block);
    }
}

/// Stores the surface friction through the caller's pointers `first` and `second`, in that order.
pub fn store_surface_friction(mem: &mut Mem, first: u32, second: u32, out: SurfaceFriction) {
    mem.set_f32(first, out.first);
    mem.set_f32(second, out.second);
}

/// `TyreTail_Get10` (0x5326b0, thiscall): the scratch's setup pointer (`this+0x10`, `m_def`).
pub fn load_def_pointer(mem: &Mem, this: u32) -> u32 {
    mem.u32(this + DEF)
}

/// `TyreTail_Get14` (0x5326c0, thiscall): the scratch's state pointer (`this+0x14`, `m_state`).
pub fn load_state_pointer(mem: &Mem, this: u32) -> u32 {
    mem.u32(this + STATE)
}

/// `NTweak::CBase::GetName` (0x52db10, thiscall): the name object at `this+4`.
pub fn load_name_object(mem: &Mem, this: u32) -> u32 {
    mem.u32(this + 4)
}

/// The tyre-noise signal, at `g_signalManager + 0x6268` (`CSignalManager::CMultiSignal1<CSigTyreNoise>`).
pub fn noise_signal(mem: &Mem) -> u32 {
    physics_core::layout::signal_manager(mem) + 0x6268
}
