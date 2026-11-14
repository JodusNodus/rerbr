//! Replays the original's VCU calls (harness/vcu_vectors.py) through the ports.
//!
//! Needs a vector file: `.venv/bin/python -I harness/vcu_vectors.py <snapshot> <out>`, then
//! `RBR_VCU_VECTORS=<out>` (several files separated by `:`). Without the variable the tests are skipped. Each record is one call of
//! the original with its arguments, the windows before and after, and the x87 return value. The unit's state is read
//! and written through the per-call store (`layout::with_vcu`), the path the bindings take on the image.

mod common;

use common::{Call, Inner, Window};
use rbr_physics::engine_driveline::layout::{CarDrive, DriveImage, DriveObj, DriveRef, DriveStateObj};
use rbr_physics::fpu::Precision;
use rbr_physics::mem::Mem;
use rbr_physics::store::Object;
use rbr_physics::vcu::layout::{ScratchObj, VcuView};
use rbr_physics::vcu::store::{DriveBlocks, VcuStore};
use rbr_physics::vcu::{self, layout};

const TABLE_BLEND: u32 = 0x536950;
const LOOKUP_PAIR_TABLE: u32 = 0x536880;
const LOCK_MAX: u32 = 0x5376e0;
const DIFF_LOCKS: u32 = 0x537b70;
const GET_4540: u32 = 0x504540;
const SET_4570: u32 = 0x504570;
const GET_4520: u32 = 0x504520;
const SET_STATE4: u32 = 0x50d340;
const REV_LIMIT_CUT: u32 = 0x536ee0;
const GEARBOX_UPDATE: u32 = 0x537160;
const GET_15F8: u32 = 0x4e6770;
const CHECK_SLIP: u32 = 0x537110;
const LAUNCH_CLUTCH: u32 = 0x5375b0;
const FLAG_DISPATCH: u32 = 0x537a80;
const QUERY_SLIP: u32 = 0x536fe0;
const SET_4820: u32 = 0x504820;
const COPY_DEF: u32 = 0x537c90;
const STEP: u32 = 0x537f50;
const STEP_BASIC: u32 = 0x5380a0;
const SET_SCRATCH08: u32 = 0x50eb90;
const SET_44A0: u32 = 0x5044a0;
const SET_30: u32 = 0x504450;
const SET_1E40: u32 = 0x4f1e40;
const SET_1ED0: u32 = 0x4f1ed0;

/// The recorded calls of `addr` in the files named by `RBR_VCU_VECTORS` (separated by `:`), or None to
/// skip (no variable, or no call of `addr` in the set).
fn load(addr: u32) -> Option<Vec<Call>> {
    let calls = common::load("RBR_VCU_VECTORS", addr)?;
    if calls.is_empty() {
        eprintln!("no calls at {addr:#x} in the vector set; skipping");
        return None;
    }
    Some(calls)
}

/// Maps zeros at the bytes of the object at `at` (`len` bytes) that no window holds.
fn fill(mem: &mut Mem, at: u32, len: u32) {
    for a in at..at + len {
        if mem.try_read(a, 1).is_none() {
            mem.map(a, &[0]);
        }
    }
}

/// Runs `run` on the unit whose scratch is at `scratch`, on the per-call store. The per-call path lifts the whole unit
/// (its scratch and its state) and, for the drive's blocks, the whole drive (the drive and its state); a record's
/// windows hold what the call reads, so the rest of their bytes are mapped as zeros first, which no window compare
/// reads.
fn on_unit<T>(mem: &mut Mem, scratch: u32, run: impl FnOnce(&mut VcuView) -> T) -> T {
    fill_unit(mem, scratch);
    layout::with_vcu(mem, scratch, run)
}

/// The zeros [`on_unit`] maps: the unit's objects and its drive's.
fn fill_unit(mem: &mut Mem, scratch: u32) {
    fill(mem, scratch, ScratchObj::SIZE);
    let state = mem.u32(scratch + ScratchObj::state.at);
    fill(mem, state, layout::StateObj::SIZE);
    let drive = ScratchObj::drive.load(mem, scratch);
    fill(mem, drive, DriveObj::SIZE);
    let drive_state = DriveImage::of(mem, drive).state;
    fill(mem, drive_state, DriveStateObj::SIZE);
}

/// Windows mapped from `call`'s before bytes.
fn windows(call: &Call) -> Mem {
    let mut mem = Mem::new();
    for Window { index: _, at, len: _, before: pre, after: _ } in &call.windows {
        mem.map(*at, pre);
    }
    mem
}

/// Every window of `call` as the original left it.
fn check_windows(mem: &Mem, call: &Call) {
    for Window { index: k, at, len, before: _, after: post } in &call.windows {
        assert_eq!(mem.read(*at, *len), *post, "call {} ({}) window {k}", call.index, call.branch);
    }
}

/// Replays every recorded call of `addr` through `port`, which gets the call's windows mapped and its
/// arguments, and returns st0. Checks st0 bit for bit and every window after the call.
fn replay(addr: u32, name: &str, port: impl Fn(&Mem, &[u32]) -> f64) {
    common::replay_branches("RBR_VCU_VECTORS", addr, name, |call, mem| {
        let st0 = port(mem, &call.args);
        assert_eq!(
            st0.to_bits(),
            f64::from_bits(call.st0_bits).to_bits(),
            "{name} call {} ({}): st0 {st0} vs {}",
            call.index,
            call.branch,
            f64::from_bits(call.st0_bits)
        );
    });
}

#[test]
fn table_blend_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay(TABLE_BLEND, "table_blend", |mem, a| {
        let [table, x, lo, hi, n] = a[..] else {
            panic!("{} args", a.len());
        };
        let knots = layout::pedal_map(mem, table, n as i32 as usize);
        vcu::tables::table_blend(&knots, f32::from_bits(x), f32::from_bits(lo), f32::from_bits(hi))
    });
}

#[test]
fn lookup_pair_table_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay(LOOKUP_PAIR_TABLE, "lookup_pair_table", |mem, a| {
        let [table, x, n] = a[..] else {
            panic!("{} args", a.len());
        };
        let knots = layout::speed_map(mem, table, n as usize);
        vcu::tables::lookup_pair_table(&knots, f32::from_bits(x))
    });
}

#[test]
fn lock_max_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    common::replay_branches("RBR_VCU_VECTORS", LOCK_MAX, "lock_max", |call, mem| {
        let [this, t1, t2, t3, f] = call.args[..] else {
            panic!("{} args", call.args.len());
        };
        let pedals = on_unit(mem, this, |vcu| vcu.scratch().pedals);
        let throttle = layout::pedal_map(mem, t1, layout::KNOTS);
        let brake = layout::pedal_map(mem, t2, layout::KNOTS);
        let speed = layout::speed_map(mem, t3, layout::KNOTS);
        let st0 = vcu::locks::lock_max(pedals, &throttle, &brake, &speed, f32::from_bits(f)) as f64;
        assert_eq!(st0.to_bits(), call.st0_bits, "lock_max call {} ({}): st0 {st0}", call.index, call.branch);
    });
}

/// CScratchSim::DiffLocks: the three locks must be the values the original passed to CDrive::Set4570
/// (channel, value bits), in the speed key the original's CDrive::Get4540 returned.
#[test]
fn diff_locks_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    common::replay_branches("RBR_VCU_VECTORS", DIFF_LOCKS, "diff_locks", |call, mem| {
        let speeds: Vec<&Inner> = call.inner.iter().filter(|f| f.addr == GET_4540).collect();
        let [speed] = speeds[..] else {
            panic!("call {}: {} CDrive::Get4540 calls", call.index, speeds.len());
        };
        let drive_speed = f64::from_bits(speed.st0_bits) as f32;
        let locks =
            on_unit(mem, call.args[0], |vcu| vcu::locks::diff_locks(vcu.scratch().pedals, &vcu.def(), drive_speed));
        let sets: Vec<(u32, u32)> =
            call.inner.iter().filter(|f| f.addr == SET_4570).map(|f| (f.args[1], f.args[2])).collect();
        assert_eq!(sets.len(), 3, "call {}: Set4570 calls", call.index);
        for (channel, (index, bits)) in sets.iter().enumerate() {
            assert_eq!(*index, channel as u32, "call {}: Set4570 order", call.index);
            assert_eq!(locks[channel].to_bits(), *bits, "call {} ({}) channel {channel}", call.index, call.branch);
        }
    });
}

/// The one inner call of `addr` made by `call` (the original calls each accessor once here).
fn inner_one(call: &Call, addr: u32) -> &Inner {
    let found: Vec<&Inner> = call.inner.iter().filter(|f| f.addr == addr).collect();
    let [one] = found[..] else {
        panic!("call {}: {} calls to {addr:#x}", call.index, found.len());
    };
    one
}

/// CScratchSim::RevLimitCut: the four SetState4 arguments must be the flags of the port, with the
/// engine speed from the recorded CDrive::Get4520.
#[test]
fn rev_limit_cut_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    let Some(calls) = load(REV_LIMIT_CUT) else {
        eprintln!("skipping rev_limit_cut: RBR_VCU_VECTORS not set");
        return;
    };
    let mut flags_seen = std::collections::BTreeMap::<[u32; 4], usize>::new();
    for call in &calls {
        let mut mem = windows(call);
        let engine_speed = f64::from_bits(inner_one(call, GET_4520).st0_bits) as f32;
        let (flags, engine) = on_unit(&mut mem, call.args[0], |vcu| {
            (vcu::cut::rev_limit_cut(engine_speed, &vcu.def(), &vcu.state(), 300.0), vcu.parts().engine)
        });
        let set = inner_one(call, SET_STATE4);
        assert_eq!(set.args[0], engine, "call {}: engine object", call.index);
        assert_eq!(set.args[1..], flags, "call {}: SetState4 flags", call.index);
        check_windows(&mem, call);
        *flags_seen.entry(flags).or_default() += 1;
    }
    eprintln!("rev_limit_cut: {} calls match, by flags: {flags_seen:?}", calls.len());
}

/// The st0 of the one call of `addr` made by `call`, or 0 when the call made none (the gate returned
/// before the accessor ran, and then the value does not matter).
fn inner_value(call: &Call, addr: u32) -> f32 {
    call.inner.iter().find(|f| f.addr == addr).map_or(0.0, |f| f64::from_bits(f.st0_bits) as f32)
}

/// CScratchSim::GearboxUpdate: the drive's gear fields, the state and the scratch's flags must end as
/// the original left them, with the engine speed and car speed from its recorded accessors.
#[test]
fn gearbox_update_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    let Some(calls) = load(GEARBOX_UPDATE) else {
        eprintln!("skipping gearbox_update: RBR_VCU_VECTORS not set");
        return;
    };
    let mut seen = std::collections::BTreeMap::<(u32, bool), usize>::new();
    for call in &calls {
        let mut mem = windows(call);
        let (requested_before, drive) = on_unit(&mut mem, call.args[0], |vcu| {
            let scratch = vcu.scratch();
            let drive_at = vcu.parts().drive;
            let def = vcu.def();
            let mut state = vcu.state();
            let mut drive = vcu.gearbox(drive_at);
            let input = vcu::gear::Input {
                launch_enabled: scratch.launch_enabled,
                vcu_enabled: scratch.vcu_enabled,
                pedals: scratch.pedals,
                engine: inner_value(call, GET_4520),
                speed: inner_value(call, GET_15F8),
                dt: f32::from_bits(call.args[1]),
                gear_count: vcu.gear_count(drive_at),
                tuning: layout::Tuning::STOCK,
            };
            let requested_before = drive.commanded;
            vcu::gear::update(input, &def, &mut state, &mut drive);
            vcu.store_state(&state);
            vcu.store_gearbox(drive_at, &drive);
            (requested_before, drive)
        });
        check_windows(&mem, call);
        *seen.entry((drive.gear, drive.commanded != requested_before)).or_default() += 1;
    }
    eprintln!("gearbox_update: {} calls match, by (gear, requested): {seen:?}", calls.len());
}

/// CScratchSim::CheckSlip returns the flag `cut::check_slip` computes from the scratch's VCU flag, the
/// drive's slip flag and the speed argument.
#[test]
fn check_slip_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    let Some(calls) = load(CHECK_SLIP) else {
        eprintln!("skipping check_slip: RBR_VCU_VECTORS not set");
        return;
    };
    let mut results = std::collections::BTreeMap::<u32, usize>::new();
    for call in &calls {
        let mut mem = windows(call);
        // window 1 is the drive's sub-object (CDrive+8), the pointer the slip flag lives behind; the record does not
        // hold the pointer's word, so it is mapped to the window
        let drive_sub = call.windows.iter().find(|w| w.index == 1).expect("window 1").at;
        let drive = mem.u32(call.args[0] + ScratchObj::drive.at);
        if mem.try_read(drive + 8, 4).is_none() {
            mem.map(drive + 8, &drive_sub.to_le_bytes());
        }
        let got = on_unit(&mut mem, call.args[0], |vcu| {
            let slip = vcu.gearbox(drive).slip;
            vcu::cut::check_slip(vcu.scratch().vcu_enabled, slip, f32::from_bits(call.args[1])) as u32
        });
        assert_eq!(got, call.eax, "call {}: CheckSlip", call.index);
        check_windows(&mem, call);
        *results.entry(got).or_default() += 1;
    }
    eprintln!("check_slip: {} calls match, by result: {results:?}", calls.len());
}

/// CScratchSim::LaunchClutch: the state must end as the original left it, and the value the port returns
/// must be the one the original sent to CDrive::Set4820 (nothing is sent with the launch mode off).
#[test]
fn launch_clutch_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    let Some(calls) = load(LAUNCH_CLUTCH) else {
        eprintln!("skipping launch_clutch: RBR_VCU_VECTORS not set");
        return;
    };
    let mut sent_counts = std::collections::BTreeMap::<bool, usize>::new();
    for call in &calls {
        let mut mem = windows(call);
        let sent = on_unit(&mut mem, call.args[0], |vcu| {
            let scratch = vcu.scratch();
            let drive = vcu.gearbox(vcu.parts().drive);
            let def = vcu.def();
            let mut state = vcu.state();
            let input = vcu::clutch::Input {
                launch_mode: scratch.launch_mode,
                engine: inner_value(call, GET_4520),
                dt: f32::from_bits(call.args[1]),
                tuning: vcu::layout::Tuning::STOCK,
            };
            let sent = vcu::clutch::launch_clutch(input, &drive, &def, &mut state);
            vcu.store_state(&state);
            sent
        });
        let set: Vec<&Inner> = call.inner.iter().filter(|f| f.addr == SET_4820).collect();
        match (sent, &set[..]) {
            (None, []) => {}
            (Some(value), [one]) => {
                assert_eq!(one.args[1], value.to_bits(), "call {} ({}): Set4820", call.index, call.branch)
            }
            _ => panic!("call {}: {} Set4820 calls, port sent {sent:?}", call.index, set.len()),
        }
        check_windows(&mem, call);
        *sent_counts.entry(sent.is_some()).or_default() += 1;
    }
    eprintln!("launch_clutch: {} calls match, by sent: {sent_counts:?}", calls.len());
}

/// CScratchSim::FlagDispatch (UpdateD): the request packet, the scratch, the drive and the state must end as
/// the original left them, and the return value must be the original's. `E50_QuerySlip` is the one the
/// original returned (it is read only with the downshift flag). The original reaches FlagDispatch only through
/// ReadPacket, so the vector set needs the `--packets` dumps; a set without them has no calls and fails the guard.
#[test]
fn flag_dispatch_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    let Some(calls) = load(FLAG_DISPATCH) else {
        eprintln!("skipping flag_dispatch: RBR_VCU_VECTORS not set");
        return;
    };
    let mut seen = std::collections::BTreeMap::<(u8, bool), usize>::new();
    for call in &calls {
        let mut mem = windows(call);
        let packet = layout::packet(&mem, call.args[1]);
        let (engine, car_speed) = (inner_value(call, GET_4520), inner_value(call, GET_15F8));
        let on = on_unit(&mut mem, call.args[0], |vcu| {
            let scratch = vcu.scratch();
            let drive_at = vcu.parts().drive;
            let def = vcu.def();
            let mut state = vcu.state();
            let mut drive = vcu.gearbox(drive_at);
            let gear_count = vcu.gear_count(drive_at);
            let gears = vcu.gear_ratios(drive_at);
            let gear = drive.gear;
            let on = vcu::dispatch::flag_dispatch(
                packet,
                || {
                    vcu::slip::query_slip(
                        scratch.slip_gate_rpm,
                        scratch.slip_gate_speed,
                        gear,
                        &gears,
                        def.rpm_limit,
                        engine,
                        car_speed,
                    )
                },
                scratch.vcu_enabled,
                gear_count,
                &def,
                &vcu::layout::Tuning::STOCK,
                &mut state,
                &mut drive,
            );
            vcu.store_state(&state);
            vcu.store_gearbox(drive_at, &drive);
            on
        });
        assert_eq!(on as u32, call.eax, "call {}: FlagDispatch return", call.index);
        check_windows(&mem, call);
        *seen.entry((packet.flags, on)).or_default() += 1;
    }
    eprintln!("flag_dispatch: {} calls match, by (flags, on): {seen:?}", calls.len());
}

/// CScratchSim::CopyDef (CopySetup): the scratch's CDef, the setup and the scratch must end as the original
/// left them. The windows are the scratch (0x60), the setup (0xb70) and the CDef (0xcac, through this+0).
#[test]
fn copy_def_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    common::replay("RBR_VCU_VECTORS", COPY_DEF, "copy_def", |call, mem| {
        let flags = layout::copy_def(mem, call.args[0], call.args[1]);
        on_unit(mem, call.args[0], |vcu| layout::copy_flags(vcu, flags));
    });
}

/// CScratchSim::ReadPacket (CopyFrom): its own record never comes out, since it tail-jumps into FlagDispatch.
/// Its scratch writes are checked on FlagDispatch's entry records, where the scratch is the one ReadPacket
/// left. Zeroing the five fields it writes and running the port must give back that scratch exactly.
#[test]
fn read_packet_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    let Some(calls) = load(FLAG_DISPATCH) else {
        eprintln!("skipping read_packet: RBR_VCU_VECTORS not set");
        return;
    };
    for call in &calls {
        let mut mem = windows(call);
        let (scratch, packet) = (call.args[0], call.args[1]);
        let Window { index: _, at: _, len: _, before: entry, after: _ } =
            call.windows.iter().find(|w| w.at == scratch && w.len == 0x60).expect("the scratch window");
        for at in [0x0c, 0x10, 0x14, 0x18, 0x1c] {
            mem.set_u32(scratch + at, 0);
        }
        let packet = layout::packet(&mem, packet);
        on_unit(&mut mem, scratch, |vcu| layout::read_packet(vcu, packet));
        assert_eq!(mem.read(scratch, 0x60), *entry, "call {}: scratch", call.index);
    }
    eprintln!("read_packet: {} calls match", calls.len());
}

/// The setter calls of Step and StepBasic (their own, and their callees' in call order), as (address, args
/// without the object).
const SENDS: [u32; 8] = [SET_SCRATCH08, SET_44A0, SET_30, SET_4820, SET_4570, SET_STATE4, SET_1E40, SET_1ED0];

/// A port's send as the (address, args) the original's call records.
fn send_words(send: vcu::step::Send) -> (u32, Vec<u32>) {
    use vcu::step::Send;
    match send {
        Send::EnginePedal(v) => (SET_SCRATCH08, vec![v.to_bits()]),
        Send::DriveThrottle(v) => (SET_44A0, vec![v.to_bits()]),
        Send::DriveBrake(v) => (SET_30, vec![v.to_bits()]),
        Send::DriveClutch(v) => (SET_4820, vec![v.to_bits()]),
        Send::DiffLock { channel, value } => (SET_4570, vec![channel, value.to_bits()]),
        Send::CutFlags(flags) => (SET_STATE4, flags.to_vec()),
        Send::BrakeLight(on) => (SET_1E40, vec![on as u32]),
        Send::NeutralLight(on) => (SET_1ED0, vec![on as u32]),
    }
}

/// Step or StepBasic: the port under test.
type StepPort = fn(
    vcu::step::Input,
    &layout::Scratch,
    &layout::Def,
    &mut layout::State,
    &mut layout::Gearbox,
    &mut layout::Outputs,
    &mut dyn FnMut(vcu::step::Send),
);

/// Replays Step or StepBasic (`addr`) through `port`: the sends must be the original's setter calls in order,
/// and the windows (scratch, state, drive, its D8 and definition, CDef) must end as the original left them.
/// The engine, car and drive speeds are the values the original's accessors returned (its inner records).
fn replay_steps(addr: u32, name: &str, port: StepPort) {
    let Some(calls) = load(addr) else {
        eprintln!("skipping {name}: RBR_VCU_VECTORS not set");
        return;
    };
    let mut stale = 0;
    for call in &calls {
        // records from before the gear-count and lock-gate windows (5, 6) were dumped cannot be replayed
        if call.windows.len() < 7 {
            stale += 1;
            continue;
        }
        let mut mem = windows(call);
        let sent = on_unit(&mut mem, call.args[0], |vcu| {
            let scratch = vcu.scratch();
            let drive_at = vcu.parts().drive;
            let input = vcu::step::Input {
                dt: f32::from_bits(call.args[1]),
                engine: inner_value(call, GET_4520),
                car_speed: inner_value(call, GET_15F8),
                drive_speed: inner_value(call, GET_4540),
                gear_count: vcu.gear_count(drive_at),
                tuning: vcu::layout::Tuning::STOCK,
            };
            let def = vcu.def();
            let mut state = vcu.state();
            let mut drive = vcu.gearbox(drive_at);
            let mut out = vcu.outputs(drive_at);
            let mut sent = Vec::new();
            port(input, &scratch, &def, &mut state, &mut drive, &mut out, &mut |s| sent.push(send_words(s)));
            vcu.store_state(&state);
            vcu.store_gearbox(drive_at, &drive);
            vcu.store_outputs(drive_at, &out);
            sent
        });
        let expected: Vec<(u32, Vec<u32>)> =
            call.inner.iter().filter(|f| SENDS.contains(&f.addr)).map(|f| (f.addr, f.args[1..].to_vec())).collect();
        assert_eq!(sent, expected, "call {} ({name}): setter calls", call.index);
        check_windows(&mem, call);
    }
    eprintln!("{name}: {} calls match, {stale} older records skipped", calls.len() - stale);
}

/// CScratchSim::Step, with the gear logic.
#[test]
fn step_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_steps(STEP, "step", vcu::step::step);
}

/// CScratchSim::StepBasic, without the gear logic.
#[test]
fn step_basic_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_steps(STEP_BASIC, "step_basic", vcu::step::step_basic);
}

/// CScratchSim::QuerySlip: the answer must be the original's, and the query writes nothing.
#[test]
fn query_slip_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    let Some(calls) = load(QUERY_SLIP) else {
        eprintln!("skipping query_slip: RBR_VCU_VECTORS not set");
        return;
    };
    let mut answers = std::collections::BTreeMap::<bool, usize>::new();
    for call in &calls {
        let mut mem = windows(call);
        let on = on_unit(&mut mem, call.args[0], |vcu| {
            let scratch = vcu.scratch();
            let drive = vcu.parts().drive;
            vcu::slip::query_slip(
                scratch.slip_gate_rpm,
                scratch.slip_gate_speed,
                vcu.gearbox(drive).gear,
                &vcu.gear_ratios(drive),
                vcu.def().rpm_limit,
                inner_value(call, GET_4520),
                inner_value(call, GET_15F8),
            )
        });
        assert_eq!(on as u32, call.eax, "call {}: QuerySlip", call.index);
        check_windows(&mem, call);
        *answers.entry(on).or_default() += 1;
    }
    eprintln!("query_slip: {} calls match, by answer: {answers:?}", calls.len());
}

/// The callees of the entries' sequences: the getters return what the record's did, the setters do nothing (their
/// writes are not the VCU's).
struct Recorded<'a>(&'a Call);

impl layout::VcuCalls for Recorded<'_> {
    fn engine_speed(&mut self, _: u32) -> f32 {
        inner_value(self.0, GET_4520)
    }
    fn lock_speed(&mut self, _: u32) -> f32 {
        inner_value(self.0, GET_4540)
    }
    fn car_speed(&mut self, _: u32) -> f32 {
        inner_value(self.0, GET_15F8)
    }
    fn set_diff_lock(&mut self, _: u32, _: u32, _: f32) -> u32 {
        0
    }
    fn set_clutch_slip(&mut self, _: u32, _: f32) -> u32 {
        0
    }
    fn set_cut_flags(&mut self, _: u32, _: [u32; 4]) -> u32 {
        0
    }
    fn set_engine_pedal(&mut self, _: u32, _: f32) {}
    fn set_brake_light(&mut self, _: u32, _: bool) {}
    fn set_neutral_light(&mut self, _: u32, _: bool) {}
    fn query_slip(&mut self) -> bool {
        false
    }
}

/// The chain on the image between calls (each entry lifts and lowers the unit) and on the typed storage over the whole
/// chain (the unit and its drive lifted before it, lowered after) leave every byte of the record's windows, the unit's
/// objects and the drive's equal, on
/// every Step record.
#[test]
fn the_typed_storage_over_a_chain_keeps_the_per_call_bytes() {
    let _pc24 = Precision::Pc24.scope();
    let Some(calls) = load(STEP) else {
        eprintln!("skipping the typed storage's chain: RBR_VCU_VECTORS not set");
        return;
    };
    let mut chained = 0;
    for call in calls.iter().filter(|call| call.windows.len() >= 7) {
        let scratch = call.args[0];
        let mut base = windows(call);
        on_unit(&mut base, scratch, |_| ());
        // A packet that sets the gear (flag 4) to the one in use, so the dispatch runs and changes no request.
        let drive = ScratchObj::drive.load(&base, scratch);
        let gear = DriveRef::of(&base, None, drive).gearbox(&base).gear;
        let packet = layout::Packet {
            pedals: layout::Pedals { throttle: 0.75, brake: 0.25, handbrake: 0.5 },
            fixed_clutch: 0.5,
            carried_word: 3,
            flags: 4,
            value: gear,
        };
        // The entries in a row, as a step makes its VCU calls: the packet, the gear logic, the launch clutch, both
        // steps, the cut and the locks.
        let (dt, tuning) = (f32::from_bits(call.args[1]), layout::Tuning::STOCK);
        let entries: [&dyn Fn(&mut VcuView); 8] = [
            &|v| layout::read_packet(v, packet),
            &|v| _ = layout::apply_packet(v, &mut Recorded(call), packet, &tuning),
            &|v| layout::gearbox_update(v, &mut Recorded(call), dt, tuning),
            &|v| _ = layout::launch_clutch(v, &mut Recorded(call), dt, tuning),
            &|v| layout::step(v, &mut Recorded(call), dt, true, tuning),
            &|v| layout::step(v, &mut Recorded(call), dt, false, tuning),
            &|v| _ = layout::rev_limit_cut(v, &mut Recorded(call), tuning.cut_margin),
            &|v| _ = layout::diff_locks(v, &mut Recorded(call)),
        ];
        let mut per_call = base.clone();
        for entry in entries {
            layout::with_vcu(&mut per_call, scratch, entry);
        }
        let mut typed = base.clone();
        let held = layout::CarVcu::of(&typed, scratch);
        let held_drive = CarDrive::of(&typed, drive);
        let mut view = held.view(&mut typed, Some(&held_drive));
        for entry in entries {
            entry(&mut view);
        }
        held.lower(&mut typed);
        held_drive.lower(&mut typed);
        let state = base.u32(scratch + ScratchObj::state.at);
        let drive_state = DriveImage::of(&base, drive).state;
        let objects = [
            (scratch, ScratchObj::SIZE as usize),
            (state, layout::StateObj::SIZE as usize),
            (drive, DriveObj::SIZE as usize),
            (drive_state, DriveStateObj::SIZE as usize),
        ];
        for (at, len) in call.windows.iter().map(|w| (w.at, w.len)).chain(objects) {
            let (a, b) = (per_call.read(at, len), typed.read(at, len));
            let k = a.iter().zip(&b).position(|(x, y)| x != y);
            assert!(k.is_none(), "call {}: {at:#x}+{:#x} differs between the storages", call.index, k.unwrap_or(0));
        }
        assert_ne!(per_call.read(scratch, 0x20), base.read(scratch, 0x20), "call {}: the packet", call.index);
        chained += 1;
    }
    assert!(chained > 0, "no Step record with every window");
    eprintln!("the typed storage's chain: {chained} Step records, every byte equal to the per-call path's");
}
