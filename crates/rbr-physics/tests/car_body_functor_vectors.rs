//! The car-body damage functors against the original's direct calls (`harness/vector_specs/car_body_functors.py`, recorded
//! with `harness/direct_calls.py --set car_body_functors`). Needs record files: `RBR_CAR_BODY_VECTORS=<file>[:<file>...]`.
//! Without the variable the tests are skipped.

mod common;

use common::{check_windows, load, map_windows, Call};
use rbr_physics::car_body::layout::{self, ImpactCalls};
use rbr_physics::mem::Mem;

const VAR: &str = "RBR_CAR_BODY_VECTORS";

/// Replays the records of `addr` through `run` (a layout sequence fn: `this`, the four stack words, returns eax).
fn check_functor(addr: u32, name: &str, run: fn(&mut Mem, u32, [u32; 4]) -> u32) {
    let Some(calls) = load(VAR, addr) else {
        eprintln!("skipping {name}: {VAR} not set");
        return;
    };
    assert!(!calls.is_empty(), "{name}: no calls in the vector files");
    for call in &calls {
        check_call(call, name, run);
    }
    eprintln!("{name}: {} calls", calls.len());
}

fn check_call(call: &Call, name: &str, run: fn(&mut Mem, u32, [u32; 4]) -> u32) {
    let mut mem = map_windows(call);
    let args = [call.args[1], call.args[2], call.args[3], call.args[4]];
    let eax = run(&mut mem, call.args[0], args);
    assert_eq!(eax, call.eax, "{name} call {}: eax", call.index);
    check_windows(call, &mem, name);
}

#[test]
fn dmg_660_matches_original_calls() {
    check_functor(0x4EC660, "CCarDamageHandler::Dmg660", layout::damage_radiator);
}

#[test]
fn dmg_a00_matches_original_calls() {
    check_functor(0x4ECA00, "CCarDamageHandler::DmgA00", layout::bend_top_mount);
}

#[test]
fn dmg_cf20_matches_original_calls() {
    check_functor(0x4ECF20, "CCarDamageHandler::DmgCF20", layout::fail_driveshaft);
}

#[test]
fn dmg_cfa0_matches_original_calls() {
    check_functor(0x4ECFA0, "CCarDamageHandler::DmgCFA0", layout::crack_windshield);
}

#[test]
fn dmg_cfe0_matches_original_calls() {
    check_functor(0x4ECFE0, "CCarDamageHandler::DmgCFE0", layout::chip_windshield);
}

#[test]
fn dmg_9a0_matches_original_calls() {
    check_functor(0x4EC9A0, "CCarDamageHandler::Dmg9A0", layout::fail_brake_circuit);
}

#[test]
fn dmg_7b0_matches_original_calls() {
    check_functor(0x4EC7B0, "CCarDamageHandler::Dmg7B0", layout::fail_differential);
}

#[test]
fn dmg_aa0_matches_original_calls() {
    check_functor(0x4ECAA0, "CCarDamageHandler::DmgAA0", layout::break_hinged_body);
}

#[test]
fn dmg_b20_matches_original_calls() {
    check_functor(0x4ECB20, "CCarDamageHandler::DmgB20", layout::break_hinged_lock);
}

#[test]
fn dmg_890_matches_original_calls() {
    check_functor(0x4EC890, "CCarDamageHandler::Dmg890", layout::drain_oil_pressure);
}

#[test]
fn dmg_930_matches_original_calls() {
    check_functor(0x4EC930, "CCarDamageHandler::Dmg930", layout::fail_water_pump);
}

#[test]
fn dmg_820_matches_original_calls() {
    check_functor(0x4EC820, "CCarDamageHandler::Dmg820", layout::fail_starter);
}

#[test]
fn dmg_ceb0_matches_original_calls() {
    check_functor(0x4ECEB0, "CCarDamageHandler::DmgCEB0", layout::cap_clutch);
}

#[test]
fn dmg_bb0_matches_original_calls() {
    check_functor(0x4ECBB0, "CCarDamageHandler::DmgBB0", layout::break_lamps_kind_2);
}

#[test]
fn dmg_cc30_matches_original_calls() {
    check_functor(0x4ECC30, "CCarDamageHandler::DmgCC30", layout::break_lamps_kind_1);
}

#[test]
fn dmg_d020_matches_original_calls() {
    check_functor(0x4ED020, "CCarDamageHandler::DmgD020", layout::wear_gearbox);
}

#[test]
fn dmg_d200_matches_original_calls() {
    check_functor(0x4ED200, "CCarDamageHandler::DmgD200", layout::set_body_state);
}

#[test]
fn dmg_d300_matches_original_calls() {
    check_functor(0x4ED300, "CCarDamageHandler::DmgD300", layout::always_zero);
}

#[test]
fn dmg_720_matches_original_calls() {
    check_functor(0x4EC720, "CCarDamageHandler::Dmg720", layout::grow_cooling_wear);
}

#[test]
fn dmg_ccb0_matches_original_calls() {
    check_functor(0x4ECCB0, "CCarDamageHandler::DmgCCB0", layout::break_lamps_kind_4);
}

#[test]
fn dmg_cd30_matches_original_calls() {
    check_functor(0x4ECD30, "CCarDamageHandler::DmgCD30", layout::break_lamps_kind_3);
}

#[test]
fn dmg_cdb0_matches_original_calls() {
    check_functor(0x4ECDB0, "CCarDamageHandler::DmgCDB0", layout::break_lamps_kind_5);
}

#[test]
fn dmg_ce30_matches_original_calls() {
    check_functor(0x4ECE30, "CCarDamageHandler::DmgCE30", layout::break_lamps_kind_6);
}

#[test]
fn dmg_d0f0_matches_original_calls() {
    check_functor(0x4ED0F0, "CCarDamageHandler::DmgD0F0", layout::fail_hydraulics);
}

#[test]
fn dmg_d160_matches_original_calls() {
    check_functor(0x4ED160, "CCarDamageHandler::DmgD160", layout::leak_oil_cooler);
}

#[test]
fn zero2f0_matches_original_calls() {
    check_functor(0x4ED2F0, "NPhysics::E35_Zero2F0", layout::second_zero);
}

const SLOT1: u32 = 0x4F12A0;

/// Answers `ImpactScale`'s call on slot 1 from its record: the target, `this` and the eight words (the length scaled).
struct RecordedSlot1<'a> {
    call: &'a Call,
}

impl ImpactCalls for RecordedSlot1<'_> {
    fn call(&mut self, target: u32, this: u32, args: [u32; 8]) -> u32 {
        let inner = self
            .call
            .inner
            .iter()
            .find(|c| c.addr == SLOT1)
            .unwrap_or_else(|| panic!("ImpactScale call {}: no Slot1 record", self.call.index));
        assert_eq!(target, SLOT1, "ImpactScale call {}: target", self.call.index);
        assert_eq!(
            inner.args[..],
            [&[this][..], &args[..]].concat(),
            "ImpactScale call {}: Slot1's arguments",
            self.call.index
        );
        inner.eax
    }
}

/// `CCarDamageHandler::ImpactScale(a1..a8)` (the slot 2 thunk) against its records: slot 1 gets the words with the length
/// scaled by 0.08, and the thunk returns slot 1's eax.
#[test]
fn impact_scale_matches_original_calls() {
    let Some(calls) = load(VAR, 0x4EC140) else {
        eprintln!("skipping ImpactScale: {VAR} not set");
        return;
    };
    assert!(!calls.is_empty(), "ImpactScale: no calls in the vector files");
    for call in &calls {
        let mut mem = map_windows(call);
        // The handler's vtable (its pointer is the window's word) has Slot1 at slot 1; the thunk reads that entry.
        let vtbl = mem.u32(call.args[0]);
        mem.map(vtbl + 4, &SLOT1.to_le_bytes());
        let args: [u32; 8] = call.args[1..9].try_into().unwrap();
        let mut slot1 = RecordedSlot1 { call };
        let eax = layout::impact_scale(&mut slot1, &mem, call.args[0], args);
        assert_eq!(eax, call.eax, "ImpactScale call {}: eax", call.index);
        check_windows(call, &mem, "CCarDamageHandler::ImpactScale");
    }
    eprintln!("CCarDamageHandler::ImpactScale: {} calls", calls.len());
}
