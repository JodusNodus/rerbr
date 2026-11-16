//! Bindings of the engine and driveline ports (`rbr_physics::engine_driveline`, exe 0x504450-0x50ed70). The ABIs
//! are the original's: the `CDrive`,
//! `CEngine` and `CDriveLine` methods are thiscall, the `E41_*` helpers stdcall. Their vector, CRT and `SetDirty`
//! callees are computed by the ports; `CCar::Get30`, the gear-change signal and `E12_SampleX` are host calls. eax is
//! given where the original returns a value; elsewhere it is a scratch value no caller reads, given as 0.

use rbr_physics::engine_driveline::layout::{DriveView, EngineView};
use rbr_physics::mem::Mem;

use crate::{Binding, Ctx, Host};

mod curves;
mod engine_sim;
mod inputs;
mod workers;

use crate::rigid_body::WALKER;
pub(crate) use curves::row_value_of;
use curves::{e12_sample_x, thunk_4f0};
use engine_sim::{
    drive_setup, intake_state_reset, link_worker, sample_big, set_cut_flags, set_options, set_scratch08, sim8cf0,
    sim_e9a0, stall_guard, vec_lerp,
};
pub(crate) use engine_sim::{engine_pedal, engine_running, engine_step};
pub(crate) use inputs::{clear_lanes, clutch_slip, m6db0, m6dd0, set_two};
use inputs::{
    drive_gate, drive_reset_part, drive_speed_key, emit_gear_change, emit_noise, get_engine_speed, mat4_mul_vec4,
    mgr_b_entry, mgr_b_field_at, mgr_b_word8, nop_ret4, set_clutch_slip, set_diff_lock,
};
pub(crate) use workers::clutch_torque;
use workers::{set_lane_scale, step1e0, wheel_worker};

/// `CCar::Get30` (thiscall): the car's word at `+0x30`.
pub const CAR_GET30: u32 = 0x4e67f0;
/// `CMultiSignal1<const CSigMrRCarGearChange&>::Emit` (thiscall, the payload's address).
pub const EMIT_GEAR_CHANGE: u32 = 0x506f20;
/// The signal manager (`[0x893600]`) and its gear-change slot.
pub const SIGNALS: u32 = 0x893600;
pub const GEAR_CHANGE_SLOT: u32 = 0xea80;
/// `E12_SampleX` (thiscall, one float, st0), which `CDriveLine::Thunk4F0` tail-jumps to.
pub const E12_SAMPLE_X: u32 = 0x475ea0;
/// `E39_MatVec` (thiscall, the vector and the out pointer on the stack): the wheel worker's callee.
const MATVEC: u32 = 0x502400;
/// The car forwarder `CCar::Fwd18E20` (0x4e6650, thiscall: `dt` and the shaft speed on the stack, st0).
const FWD18E20: u32 = 0x4e6650;
const GRAPH_PUSH: u32 = 0x494cf0; // CGraph1D::Push
pub(crate) const E01LOG: u32 = 0x43c8e0; // E01Log (cdecl)
const FLOOR_S32_FORMAT: u32 = 0x72dcb0; // "FloorS32: Invalid argument %f\n"

const fn bind(addr: u32, name: &'static str, run: crate::BindingFn) -> Binding {
    crate::bind("engine_driveline", addr, name, run)
}

pub static BINDINGS: [Binding; 32] = [
    bind(0x501fa0, "E39_MgrB_Word8", mgr_b_word8),
    bind(0x501fb0, "E39_MgrB_Entry", mgr_b_entry),
    bind(0x502060, "E39_MgrB_FieldAt", mgr_b_field_at),
    bind(0x502400, "E39_MgrD_Mat4Vec4", mat4_mul_vec4),
    bind(0x502c00, "E39_ClearLanes", clear_lanes),
    bind(0x503860, "E39_Nop3860", nop_ret4),
    bind(0x5043b0, "CDrive::Nop3B0", nop_ret4),
    bind(0x504510, "CDrive::Set4510", drive_reset_part),
    bind(0x504520, "CDrive::Get4520", get_engine_speed),
    bind(0x504540, "CDrive::Get4540", drive_speed_key),
    bind(0x504570, "CDrive::Set4570", set_diff_lock),
    bind(0x504820, "CDrive::Set4820", set_clutch_slip),
    bind(0x504840, "CDrive::Test4840", drive_gate),
    bind(0x505970, "CDrive::M5970", set_lane_scale),
    bind(0x506f20, "CSigMrRCarGearChange emit", emit_gear_change),
    bind(0x509bc0, "CDrive::M9BC0", wheel_worker),
    bind(0x50a1e0, "CDrive::Step1E0", step1e0),
    bind(0x50a4f0, "CDriveLine::Thunk4F0", thunk_4f0),
    bind(E12_SAMPLE_X, "E12_SampleX", e12_sample_x),
    bind(0x50d310, "CEngine::Set48", set_options),
    bind(0x50d340, "CEngine::SetState4", set_cut_flags),
    bind(0x50e9a0, "CEngine::SimE9A0", sim_e9a0),
    bind(0x50eb90, "CEngine::SetScratch08", set_scratch08),
    bind(0x50ecf0, "CEngine::SimECF0", stall_guard),
    bind(0x50ef50, "E41EngineNoiseSignal::Emit", emit_noise),
    bind(0x5180d0, "E43EngineSim::Sim80D0", intake_state_reset),
    bind(0x518cf0, "E43EngineSim::Sim8CF0", sim8cf0),
    bind(0x473c90, "E11_VecLerp", vec_lerp),
    bind(0x50b300, "CDriveLine::SampleBig", sample_big),
    bind(0x508770, "CDrive::M8770", drive_setup),
    bind(0x503ab0, "E39_Link3AB0", link_worker),
    bind(0x50d300, "E41Plain::NopD300", nop_ret4),
];

fn f(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// Runs `run` on the engine at `engine`, in the typed storage the host holds ([`crate::Host::typed`]) with the held
/// definitions. The engine's
/// readers outside its bindings (NGP's engine wrapper, the drive's gate) and any reader of its words during a step go
/// through it.
pub(crate) fn on_engine_in<T>(
    host: &dyn Host,
    mem: &mut Mem,
    engine: u32,
    run: impl FnOnce(&mut EngineView) -> T,
) -> T {
    let typed = host.typed();
    assert!(typed.car.engine.is(engine), "the engine at {engine:#x} is not the car's");
    run(&mut typed.car.engine.view(mem).with_defs(Some(&typed.defs)))
}

/// [`on_engine_in`] on a binding's memory and host.
pub(crate) fn on_engine<T>(ctx: &mut Ctx, engine: u32, run: impl FnOnce(&mut EngineView) -> T) -> T {
    on_engine_in(ctx.host, &mut ctx.mem, engine, run)
}

/// Runs `run` on the drive at `drive`, in the typed storage the host holds ([`crate::Host::typed`]).
pub(crate) fn on_drive_in<T>(host: &dyn Host, mem: &mut Mem, drive: u32, run: impl FnOnce(&mut DriveView) -> T) -> T {
    let typed = host.typed();
    assert!(typed.car.drive.is(drive), "the drive at {drive:#x} is not the car's");
    run(&mut typed.car.drive.view(mem).with_defs(Some(&typed.defs)))
}

/// [`on_drive_in`] on a binding's memory and host.
pub(crate) fn on_drive<T>(ctx: &mut Ctx, drive: u32, run: impl FnOnce(&mut DriveView) -> T) -> T {
    on_drive_in(ctx.host, &mut ctx.mem, drive, run)
}

/// Whether `drive` is the drive a step holds.
pub(crate) fn is_held(host: &dyn Host, drive: u32) -> bool {
    host.typed().car.drive.is(drive)
}
