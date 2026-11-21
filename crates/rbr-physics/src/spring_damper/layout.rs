//! Spring-damper game-memory boundary. This file alone knows object offsets.

use super::store::{DamperBlocks, StrutStore};
use super::strut::{Strut, StrutId, Struts};
use super::suspension::Damping;
use super::{
    BlendTerms, ClampParameters, Controller, DamperSignal, Definition, FadeState, ForceTerms, HeatAccumulator,
    HeatCapacity, RealValue, Relaxation, SetupValues, UpdateTerms, WeightedValue,
};
use crate::mem::{Block, Mem};
use crate::ntweak::layout::{REAL_NAME, REAL_VTABLE, UNSIGNED_NAME, UNSIGNED_VTABLE};
use crate::store::defs::{self, Defs};
use crate::store::{Raw, Real};

const DEFINITION_VTABLE: u32 = 0x0074_0124; // 0x52d750
const STATE_VTABLE: u32 = 0x0074_018c; // 0x52da50
const CONTROLLER_VTABLE: u32 = 0x0074_01e8; // 0x52df20
const SCRATCH_VTABLE: u32 = 0x0074_01dc; // 0x52df20
const BASE_SECOND_VTABLE: u32 = 0x0074_0174;

const SETUP_OPTIONS_DICTIONARY: u32 = 0x0165_f16c; // 0x52d710
const DEFINITION_DICTIONARY: u32 = 0x0165_f180; // 0x52d730
const SETUP_DICTIONARY: u32 = 0x0165_f158; // 0x52d9e0

crate::object! {
    /// `CSpringDamper` (the controller; the wheel's strut at `wheel+0xec4`). The size is the end of the last known
    /// field.
    pub struct ControllerObj, size 0x30 {
        vtable @ 0x00: u32,
        definition @ 0x04: u32,
        state @ 0x08: u32,
        scratch_vtable @ 0x0c: u32,
        fade @ 0x10: u32,
        mode @ 0x14: i32,
        high_speed_hold @ 0x18: i32,
        signal_id @ 0x1c: u32,
        signal_peak @ 0x20: f32,
        signal_bump @ 0x24: f32,
        signal_droop @ 0x28: f32,
        signal_valid @ 0x2c: i32,
    }
}

crate::object! {
    /// `CSpringDamper::CState` (rbr-recomp `notes/physics/renames/spring-damper.csv`; the strut's at `wheel+0x670`).
    pub struct StateObj, size 0x3c {
        vtable @ 0x00: u32,
        second_vtable @ 0x04: u32,
        dirty @ 0x08: u32,
        damp_bump_eff @ 0x0c: f32,
        damp_rebound_eff @ 0x10: f32,
        hs_damp_bump_eff @ 0x14: f32,
        owner @ 0x18: u32,
        last_deflection @ 0x1c: Real,
        unsigned_header @ 0x2c: [u32; 3],
        unsigned_tweak @ 0x38: u32,
    }
}

crate::object! {
    /// The strut's fade pair (the controller's `+0x10` points at it): the damage handler's damper pair of the wheel
    /// (`+0x3ce4 + 8k`), which `ResetDamage` clears the amount of (`ntweak::layout::clear_unsigned_value`).
    pub struct FadeObj, size 0x08 {
        test_multiplier @ 0x00: f32,
        amount @ 0x04: f32,
    }
}

const DEFINITION_POINTER: u32 = ControllerObj::definition.at;
const STATE_POINTER: u32 = ControllerObj::state.at;
const SCRATCH_VTABLE_WORD: u32 = ControllerObj::scratch_vtable.at;
const FADE_POINTER: u32 = ControllerObj::fade.at;
const MODE: u32 = ControllerObj::mode.at;
const HIGH_SPEED_HOLD: u32 = ControllerObj::high_speed_hold.at;

/// `CSpringDamper::CDef` members (rbr-recomp `notes/physics/renames/spring-damper.csv`). The reals are CReal headers;
/// each value is at +0xc.
const DEFINITION_DEBUG_ID: u32 = 0x18;
const DEFINITION_BUMP_STOP_POS: u32 = 0x1c;
const DEFINITION_DROOP_STOP_POS: u32 = 0x2c;
const DEFINITION_SPRING_FREE_LENGTH: u32 = 0x3c;
const DEFINITION_SPRING_RATE: u32 = 0x4c;
const DEFINITION_HELPER_MIN_LENGTH: u32 = 0x5c;
const DEFINITION_HELPER_LENGTH: u32 = 0x6c;
const DEFINITION_HELPER_RATE: u32 = 0x7c;
const DEFINITION_BUMP_STOP_RATE: u32 = 0x8c;
const DEFINITION_DROOP_STOP_RATE: u32 = 0x9c;
const DEFINITION_DAMP_BUMP: u32 = 0xac;
const DEFINITION_DAMP_REBOUND: u32 = 0xbc;
const DEFINITION_STOP_DAMP: u32 = 0xcc;
const DEFINITION_HS_BREAK_BUMP: u32 = 0xdc;
const DEFINITION_HS_BREAK_REBOUND: u32 = 0xec;
const DEFINITION_HS_DAMP_BUMP: u32 = 0xfc;
const DEFINITION_HS_DAMP_REBOUND: u32 = 0x10c;

/// `CSpringDamper::CSetup` members (rbr-recomp `notes/physics/renames/spring-damper.csv`), CReal headers as in the
/// definition.
const SETUP_SPRING_FREE_LENGTH: u32 = 0x0c;
const SETUP_SPRING_RATE: u32 = 0x1c;
const SETUP_HELPER_MIN_LENGTH: u32 = 0x2c;
const SETUP_HELPER_LENGTH: u32 = 0x3c;
const SETUP_HELPER_RATE: u32 = 0x4c;
const SETUP_DAMP_BUMP: u32 = 0x5c;
const SETUP_DAMP_REBOUND: u32 = 0x6c;
const SETUP_HS_BREAK_BUMP: u32 = 0x7c;
const SETUP_HS_DAMP_BUMP: u32 = 0x8c;

/// `CSpringDamper::CState` members.
const STATE_DAMP_BUMP_EFF: u32 = StateObj::damp_bump_eff.at;
const STATE_DAMP_REBOUND_EFF: u32 = StateObj::damp_rebound_eff.at;
const STATE_HS_DAMP_BUMP_EFF: u32 = StateObj::hs_damp_bump_eff.at;
const STATE_OWNER: u32 = StateObj::owner.at;
const STATE_LAST_DEFLECTION: u32 = StateObj::last_deflection.at;
const STATE_UNSIGNED_HEADER: u32 = StateObj::unsigned_header.at;
/// The state's unsigned tweak value (`CState`'s CUnsigned at +0x2c, its value at +0xc; key not found).
const STATE_UNSIGNED_TWEAK: u32 = StateObj::unsigned_tweak.at;

/// The owner wheel's index (`wheel+0xeb4`, `wheel::layout::WHEEL_INDEX`), the damper signal's owner id.
const OWNER_WHEEL_INDEX: u32 = 0xeb4;

/// Reads a CReal body: its vtable (+0), dirty (+8) and value (+0xc) words. The name word (+4) is left to the
/// constructor and is not copied by setup apply.
fn real_value(mem: &Mem, at: u32) -> RealValue {
    RealValue { label: mem.u32(at), dirty: mem.u32(at + 8), value: mem.f32(at + 12) }
}

/// CSetupOptions::Unk1C returns the setup-options dictionary. 0x52d710.
pub fn setup_options_dictionary() -> u32 {
    SETUP_OPTIONS_DICTIONARY
}

/// CDef::Unk20 returns the definition dictionary. 0x52d730.
pub fn definition_dictionary() -> u32 {
    DEFINITION_DICTIONARY
}

/// CSetup::Unk20 returns the setup dictionary. 0x52d9e0.
pub fn setup_dictionary() -> u32 {
    SETUP_DICTIONARY
}

/// Initializes the complete CDef object, including NTweak headers. 0x52d750.
pub fn construct_definition(mem: &mut Mem, at: u32) -> u32 {
    let value = Definition::new();
    mem.set_u32(at, DEFINITION_VTABLE);
    mem.set_u32(at + 4, BASE_SECOND_VTABLE);
    mem.set_u32(at + 8, 0);
    mem.set_u32(at + 0x0c, UNSIGNED_VTABLE);
    mem.set_u32(at + 0x10, UNSIGNED_NAME);
    mem.set_u32(at + 0x14, 0);
    mem.set_u32(at + DEFINITION_DEBUG_ID, value.debug_id);
    write_real(mem, at + DEFINITION_BUMP_STOP_POS, value.bump_stop_pos, 0);
    write_real(mem, at + DEFINITION_DROOP_STOP_POS, value.droop_stop_pos, 0);
    write_real(mem, at + DEFINITION_SPRING_FREE_LENGTH, value.spring_free_length, 0);
    write_real(mem, at + DEFINITION_SPRING_RATE, value.spring_rate, 0);
    write_real(mem, at + DEFINITION_HELPER_MIN_LENGTH, value.helper_min_length, 0);
    write_real(mem, at + DEFINITION_HELPER_LENGTH, value.helper_length, 0);
    write_real(mem, at + DEFINITION_HELPER_RATE, value.helper_rate, 0);
    write_real(mem, at + DEFINITION_BUMP_STOP_RATE, value.bump_stop_rate, 0);
    write_real(mem, at + DEFINITION_DROOP_STOP_RATE, value.droop_stop_rate, 1);
    write_real(mem, at + DEFINITION_DAMP_BUMP, value.damp_bump, 0);
    write_real(mem, at + DEFINITION_DAMP_REBOUND, value.damp_rebound, 0);
    write_real(mem, at + DEFINITION_STOP_DAMP, value.stop_damp, 0);
    write_real(mem, at + DEFINITION_HS_BREAK_BUMP, value.hs_break_bump, 0);
    write_real(mem, at + DEFINITION_HS_BREAK_REBOUND, value.hs_break_rebound, 0);
    write_real(mem, at + DEFINITION_HS_DAMP_BUMP, value.hs_damp_bump, 0);
    write_real(mem, at + DEFINITION_HS_DAMP_REBOUND, value.hs_damp_rebound, 0);
    at
}

fn write_real(mem: &mut Mem, at: u32, value: RealValue, dirty: u32) {
    mem.set_u32(at, REAL_VTABLE);
    mem.set_u32(at + 4, REAL_NAME);
    mem.set_u32(at + 8, dirty);
    mem.set_f32(at + 12, value.value);
}

/// `CSpringDamper::CSetup`'s vtable (0x72e5d4).
const SETUP_VTABLE: u32 = 0x0072_e5d4;

/// `CSpringDamper::CSetup::CSetup` (0x47bc50) on the setup at `at`: an unnamed tweak, its vtable, and the nine reals
/// `CopyStateToDef` (0x52dbd0) copies, from +0x0c, 0x10 apart.
pub fn construct_setup(mem: &mut Mem, at: u32) -> u32 {
    crate::ntweak::construct_base(mem, at, 0);
    mem.set_u32(at, SETUP_VTABLE);
    for real in (0x0c..=0x8c).step_by(0x10) {
        crate::ntweak::construct_real(mem, at + real);
    }
    at
}

/// `CSpringDamper::CSetupOptions`'s vtable (0x72e864).
const OPTIONS_VTABLE: u32 = 0x0072_e864;

/// `CSpringDamper::CSetupOptions::CSetupOptions` (0x47d870) on the options at `at`: an unnamed tweak, its vtable, and
/// eight ranges (`E13RealTriple`s: `Min`, `Max`, `Step`) from +0xc, every real 0x10 apart.
pub fn construct_options(mem: &mut Mem, at: u32) {
    crate::ntweak::construct_base(mem, at, 0);
    mem.set_u32(at, OPTIONS_VTABLE);
    for real in (0x0c..0x18c).step_by(0x10) {
        crate::ntweak::construct_real(mem, at + real);
    }
}

/// Initializes the CState base and its tweak subobjects. 0x52da50.
pub fn construct_state(mem: &mut Mem, at: u32, owner: u32) -> u32 {
    mem.set_u32(at, STATE_VTABLE);
    mem.set_u32(at + 4, BASE_SECOND_VTABLE);
    mem.set_u32(at + 8, 0);
    mem.set_f32(at + STATE_DAMP_BUMP_EFF, 0.0);
    mem.set_f32(at + STATE_DAMP_REBOUND_EFF, 0.0);
    mem.set_f32(at + STATE_HS_DAMP_BUMP_EFF, 0.0);
    mem.set_u32(at + STATE_OWNER, owner);
    write_real(mem, at + STATE_LAST_DEFLECTION, RealValue::default(), 0);
    mem.set_u32(at + STATE_UNSIGNED_HEADER, UNSIGNED_VTABLE);
    mem.set_u32(at + STATE_UNSIGNED_HEADER + 4, UNSIGNED_NAME);
    mem.set_u32(at + STATE_UNSIGNED_HEADER + 8, 0);
    mem.set_u32(at + STATE_UNSIGNED_TWEAK, 0);
    at
}

/// `CState::SetUnk10` (0x52da40) writes its argument at `this+0x10`. Its one caller (`E51Wheel::UnkA860`) calls it on
/// the strut, whose `+0x10` is the fade pointer: this is the fade pointer's writer, the damage handler's damper pair of
/// the wheel (the wheel's `+0x3ce4 + 8k`).
pub fn set_fade_pointer(mem: &mut Mem, controller_at: u32, fade_at: u32) {
    ControllerObj::fade.store(mem, controller_at, fade_at);
}

/// Copies a CSetup's nine CReal bodies into the matching CDef fields. 0x52dbd0.
/// A setup copy on the image: the definition stays there; the owned adoption runs on `Suspension`.
pub fn apply_setup(mem: &mut Mem, controller_at: u32, setup_at: u32) {
    let definition_at = definition_at(mem, controller_at);
    let mut suspension =
        super::suspension::Suspension { definition: definition(mem, definition_at), ..Default::default() };
    suspension.adopt_setup(setup_values(mem, setup_at));
    store_setup_values(mem, definition_at, &suspension.definition);
}

fn write_real_body(mem: &mut Mem, at: u32, value: RealValue) {
    mem.set_u32(at, value.label);
    mem.set_u32(at + 8, value.dirty);
    mem.set_f32(at + 12, value.value);
}

/// CSpringDamper::CSpringDamper initializes its pointers, scratch header, and counters. 0x52df20.
pub fn construct_controller(mem: &mut Mem, at: u32, definition: u32, state: u32) -> u32 {
    mem.set_u32(at, CONTROLLER_VTABLE);
    mem.set_u32(at + DEFINITION_POINTER, definition);
    mem.set_u32(at + STATE_POINTER, state);
    mem.set_u32(at + SCRATCH_VTABLE_WORD, SCRATCH_VTABLE);
    mem.set_u32(at + FADE_POINTER, 0);
    mem.set_u32(at + MODE, 0);
    mem.set_u32(at + HIGH_SPEED_HOLD, 0);
    at
}

/// CDamperSignal::Emit is an external signal-manager call with no spring-owned memory effects.
pub fn emit_signal() {}

/// Runs DamperAccumA (heat accumulation) on its capacity pair and running temperature. 0x52e3a0.
pub fn accumulate_heat<S: DamperBlocks>(mem: &mut S, capacity_at: u32, heat: f32, temperature_at: u32) {
    let capacity = mem.load_heat_capacity(capacity_at);
    let mut temperature = mem.f32(temperature_at);
    super::accumulate_heat(capacity, heat, &mut temperature);
    mem.set_f32(temperature_at, temperature);
}

/// Runs DamperAccumB (heat accumulation through the shared scale word) on its scale word
/// and per-block accumulator. 0x52e3d0.
pub fn accumulate_heat_scaled<S: DamperBlocks>(mem: &mut S, scale_at: u32, heat: f32, cell_at: u32) {
    let mut cell = mem.load_heat_accumulator(cell_at);
    super::accumulate_heat_scaled(mem.f32(scale_at), heat, &mut cell);
    mem.store_heat_accumulator(cell_at, cell);
}

/// Runs DamperClamp on typed transfer values. 0x52e400.
pub fn clamp_value<S: DamperBlocks>(mem: &mut S, parameters_at: u32, target_at: u32) -> f32 {
    let parameters = mem.load_clamp_parameters(parameters_at);
    let mut target = mem.load_weighted(target_at);
    let result = super::clamp_value(parameters, &mut target);
    mem.store_weighted(target_at, target);
    result
}

/// Runs DamperMerge on typed weighted values. 0x52e460.
pub fn merge_value<S: DamperBlocks>(mem: &mut S, source_at: u32, target_at: u32) {
    let source = mem.load_weighted(source_at);
    let mut target = mem.load_weighted(target_at);
    super::merge_value(source, &mut target);
    mem.store_weighted(target_at, target);
}

/// Runs DamperUpdateA (solves the response term) on its four-value terms. 0x52e4c0.
pub fn solve_response<S: DamperBlocks>(mem: &mut S, terms_at: u32, gas_constant: f32) {
    let mut terms = mem.load_update_terms(terms_at);
    super::solve_response(&mut terms, gas_constant);
    mem.store_update_terms(terms_at, terms);
}

/// Runs DamperUpdateB (solves the scale term) on its four-value terms. 0x52e4f0.
pub fn solve_scale<S: DamperBlocks>(mem: &mut S, terms_at: u32, gas_constant: f32) {
    let mut terms = mem.load_update_terms(terms_at);
    super::solve_scale(&mut terms, gas_constant);
    mem.store_update_terms(terms_at, terms);
}

/// Runs DamperUpdateC (solves the numerator term) on its four-value terms. 0x52e530.
pub fn solve_numerator<S: DamperBlocks>(mem: &mut S, terms_at: u32, gas_constant: f32) {
    let mut terms = mem.load_update_terms(terms_at);
    super::solve_numerator(&mut terms, gas_constant);
    mem.store_update_terms(terms_at, terms);
}

/// Runs DamperCombine on its target and source blocks. 0x52e630.
pub fn combine<S: DamperBlocks>(mem: &mut S, target_at: u32, source_at: u32, input: f32) {
    let mut target = mem.load_blend_terms(target_at);
    let source = mem.load_blend_terms(source_at);
    super::combine(&mut target, source, input);
    mem.store_blend_terms(target_at, target);
}

/// Runs DamperRelax on its four-value state. 0x52e6d0.
pub fn relax<S: DamperBlocks>(mem: &mut S, state_at: u32, amount: f32, divisor: f32) {
    let mut value = mem.load_relaxation(state_at);
    super::relax(&mut value, amount, divisor);
    mem.store_relaxation(state_at, value);
}

/// Runs DamperSolve on its four-value state. 0x52e760.
pub fn solve<S: DamperBlocks>(mem: &mut S, state_at: u32, input: f32, limit: f32, divisor: f32) -> f32 {
    let mut value = mem.load_relaxation(state_at);
    let result = super::solve(&mut value, input, limit, divisor);
    mem.store_relaxation(state_at, value);
    result
}

/// Runs DamperForce on its four-value accumulator. 0x52e8a0.
pub fn force<S: DamperBlocks>(mem: &mut S, state_at: u32, input: f32, divisor: f32, velocity: f32) {
    let mut value = mem.load_force_terms(state_at);
    super::force(&mut value, input, divisor, velocity);
    mem.store_force_terms(state_at, value);
}

/// The controller's definition pointer.
fn definition_at(mem: &Mem, controller: u32) -> u32 {
    mem.u32(controller + DEFINITION_POINTER)
}

/// The bytes of a definition [`definition`] reads (from the debug id to the last real), the ones the typed definitions
/// hold (`store::defs`).
pub const DEFINITION_BYTES: std::ops::Range<u32> = DEFINITION_DEBUG_ID..DEFINITION_HS_DAMP_REBOUND + 0x10;

/// Reads the fields needed by the reached spring-damper functions.
pub fn definition(mem: &Mem, at: u32) -> Definition {
    definition_from(|a| mem.u32(a), at)
}

/// [`definition`] from one read of its bytes ([`DEFINITION_BYTES`]): the typed definitions' lift (`store::defs`).
pub fn lift_definition(mem: &Mem, at: u32) -> Definition {
    const WORDS: usize = (DEFINITION_BYTES.end - DEFINITION_BYTES.start) as usize / 4;
    let block = Block::<WORDS>::read(mem, at + DEFINITION_BYTES.start);
    definition_from(|a| block.u32(a), at)
}

/// The definition at `at`, its words read by `word`.
#[inline(always)]
fn definition_from(word: impl Fn(u32) -> u32, at: u32) -> Definition {
    let real_value = |at: u32| RealValue { label: word(at), dirty: word(at + 8), value: f32::from_bits(word(at + 12)) };
    Definition {
        debug_id: word(at + DEFINITION_DEBUG_ID),
        bump_stop_pos: real_value(at + DEFINITION_BUMP_STOP_POS),
        droop_stop_pos: real_value(at + DEFINITION_DROOP_STOP_POS),
        spring_free_length: real_value(at + DEFINITION_SPRING_FREE_LENGTH),
        spring_rate: real_value(at + DEFINITION_SPRING_RATE),
        helper_min_length: real_value(at + DEFINITION_HELPER_MIN_LENGTH),
        helper_length: real_value(at + DEFINITION_HELPER_LENGTH),
        helper_rate: real_value(at + DEFINITION_HELPER_RATE),
        bump_stop_rate: real_value(at + DEFINITION_BUMP_STOP_RATE),
        droop_stop_rate: real_value(at + DEFINITION_DROOP_STOP_RATE),
        damp_bump: real_value(at + DEFINITION_DAMP_BUMP),
        damp_rebound: real_value(at + DEFINITION_DAMP_REBOUND),
        stop_damp: real_value(at + DEFINITION_STOP_DAMP),
        hs_break_bump: real_value(at + DEFINITION_HS_BREAK_BUMP),
        hs_break_rebound: real_value(at + DEFINITION_HS_BREAK_REBOUND),
        hs_damp_bump: real_value(at + DEFINITION_HS_DAMP_BUMP),
        hs_damp_rebound: real_value(at + DEFINITION_HS_DAMP_REBOUND),
    }
}

/// Reads the nine setup values that CopyStateToDef adopts.
fn setup_values(mem: &Mem, at: u32) -> SetupValues {
    SetupValues {
        spring_free_length: real_value(mem, at + SETUP_SPRING_FREE_LENGTH),
        spring_rate: real_value(mem, at + SETUP_SPRING_RATE),
        helper_min_length: real_value(mem, at + SETUP_HELPER_MIN_LENGTH),
        helper_length: real_value(mem, at + SETUP_HELPER_LENGTH),
        helper_rate: real_value(mem, at + SETUP_HELPER_RATE),
        damp_bump: real_value(mem, at + SETUP_DAMP_BUMP),
        damp_rebound: real_value(mem, at + SETUP_DAMP_REBOUND),
        hs_break_bump: real_value(mem, at + SETUP_HS_BREAK_BUMP),
        hs_damp_bump: real_value(mem, at + SETUP_HS_DAMP_BUMP),
    }
}

fn store_setup_values(mem: &mut Mem, definition_at: u32, definition: &Definition) {
    // CopyStateToDef copies each CReal's label, dirty word, and value, in this order.
    write_real_body(mem, definition_at + DEFINITION_SPRING_FREE_LENGTH, definition.spring_free_length);
    write_real_body(mem, definition_at + DEFINITION_SPRING_RATE, definition.spring_rate);
    write_real_body(mem, definition_at + DEFINITION_HELPER_MIN_LENGTH, definition.helper_min_length);
    write_real_body(mem, definition_at + DEFINITION_HELPER_LENGTH, definition.helper_length);
    write_real_body(mem, definition_at + DEFINITION_HELPER_RATE, definition.helper_rate);
    write_real_body(mem, definition_at + DEFINITION_DAMP_BUMP, definition.damp_bump);
    write_real_body(mem, definition_at + DEFINITION_DAMP_REBOUND, definition.damp_rebound);
    write_real_body(mem, definition_at + DEFINITION_HS_BREAK_BUMP, definition.hs_break_bump);
    write_real_body(mem, definition_at + DEFINITION_HS_DAMP_BUMP, definition.hs_damp_bump);
}

/// Where a strut's typed state lives in the image: its controller, the definition and state it points at, and its fade
/// pair (0 before `E51Wheel::UnkA860` sets it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrutImage {
    pub controller: u32,
    pub definition: u32,
    pub state: u32,
    pub fade: u32,
}

/// The words a strut's typed state owns: the controller's from the mode to the signal's last word, the state's three
/// effective dampings, the fade pair.
const OWNED_CONTROLLER: std::ops::Range<u32> = ControllerObj::mode.at..ControllerObj::signal_valid.at + 4;
const OWNED_STATE: std::ops::Range<u32> = StateObj::damp_bump_eff.at..StateObj::hs_damp_bump_eff.at + 4;
const OWNED_FADE: std::ops::Range<u32> = FadeObj::test_multiplier.at..FadeObj::amount.at + 4;

impl StrutImage {
    /// The strut whose controller is at `controller`, its objects as its pointers give them.
    pub fn of(mem: &Mem, controller: u32) -> Self {
        StrutImage {
            controller,
            definition: ControllerObj::definition.load(mem, controller),
            state: ControllerObj::state.load(mem, controller),
            fade: ControllerObj::fade.load(mem, controller),
        }
    }

    /// The strut's typed state, read from the image. Without a fade pair the fade is zero (nothing reads it: the
    /// view panics where the original would read address 0).
    pub fn lift(&self, mem: &Mem) -> Strut {
        let (c, state, fade) = (self.controller, self.state, self.fade);
        Strut {
            controller: Controller {
                mode: ControllerObj::mode.load(mem, c),
                high_speed_hold: ControllerObj::high_speed_hold.load(mem, c),
                signal: DamperSignal {
                    owner_id: ControllerObj::signal_id.load(mem, c),
                    peak_velocity: ControllerObj::signal_peak.load(mem, c),
                    bump_velocity: ControllerObj::signal_bump.load(mem, c),
                    droop_velocity: ControllerObj::signal_droop.load(mem, c),
                    valid: ControllerObj::signal_valid.load(mem, c),
                },
            },
            damping: Damping {
                bump: StateObj::damp_bump_eff.load(mem, state),
                rebound: StateObj::damp_rebound_eff.load(mem, state),
                high_speed_bump: StateObj::hs_damp_bump_eff.load(mem, state),
            },
            fade: match fade {
                0 => FadeState::default(),
                _ => FadeState {
                    test_multiplier: FadeObj::test_multiplier.load(mem, fade),
                    amount: FadeObj::amount.load(mem, fade),
                },
            },
        }
    }

    /// Writes the strut's typed state back: every owned word, nothing else.
    pub fn lower(&self, strut: &Strut, mem: &mut Mem) {
        let (c, state, fade) = (self.controller, self.state, self.fade);
        let controller = &strut.controller;
        ControllerObj::mode.store(mem, c, controller.mode);
        ControllerObj::high_speed_hold.store(mem, c, controller.high_speed_hold);
        ControllerObj::signal_id.store(mem, c, controller.signal.owner_id);
        ControllerObj::signal_peak.store(mem, c, controller.signal.peak_velocity);
        ControllerObj::signal_bump.store(mem, c, controller.signal.bump_velocity);
        ControllerObj::signal_droop.store(mem, c, controller.signal.droop_velocity);
        ControllerObj::signal_valid.store(mem, c, controller.signal.valid);
        StateObj::damp_bump_eff.store(mem, state, strut.damping.bump);
        StateObj::damp_rebound_eff.store(mem, state, strut.damping.rebound);
        StateObj::hs_damp_bump_eff.store(mem, state, strut.damping.high_speed_bump);
        if fade != 0 {
            FadeObj::test_multiplier.store(mem, fade, strut.fade.test_multiplier);
            FadeObj::amount.store(mem, fade, strut.fade.amount);
        }
    }

    /// The owned byte ranges with their names (the fade pair's only when the strut has one).
    pub fn owned(&self) -> impl Iterator<Item = (std::ops::Range<u32>, &'static str)> {
        let at = |base: u32, r: &std::ops::Range<u32>| base + r.start..base + r.end;
        let parts = [
            (at(self.controller, &OWNED_CONTROLLER), "strut controller"),
            (at(self.state, &OWNED_STATE), "strut damping"),
            (at(self.fade, &OWNED_FADE), "strut fade"),
        ];
        parts.into_iter().take(if self.fade == 0 { 2 } else { 3 })
    }
}

/// Struts in typed storage: where each lives in the image and its state, by slot ([`StrutId`]). A car's are by wheel
/// ([`CarStruts::lift`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarStruts {
    pub images: [Option<StrutImage>; 4],
    pub struts: Struts,
}

impl CarStruts {
    /// The struts of the car at `car`, wheel by wheel, lifted from the image.
    pub fn lift(mem: &Mem, car: u32) -> Self {
        let wheels = crate::car_body::store::CarStore::wheel_pointers(mem, car);
        Self::lift_controllers(mem, wheels.map(|wheel| (wheel != 0).then_some(wheel + crate::wheel::layout::STRUT)))
    }

    /// The struts whose controllers are at `controllers`, slot by slot, lifted from the image.
    pub fn lift_controllers(mem: &Mem, controllers: [Option<u32>; 4]) -> Self {
        let images = controllers.map(|c| c.map(|c| StrutImage::of(mem, c)));
        CarStruts { images, struts: Struts(images.map(|image| image.map(|image| image.lift(mem)))) }
    }

    /// Writes every strut's state back to the image.
    pub fn lower(&self, mem: &mut Mem) {
        for (image, strut) in self.images.iter().zip(&self.struts.0) {
            if let (Some(image), Some(strut)) = (image, strut) {
                image.lower(strut, mem);
            }
        }
    }

    /// The slot of the strut whose controller is at `controller`, if it is one of these.
    pub fn id_of(&self, controller: u32) -> Option<StrutId> {
        self.images.iter().position(|image| image.is_some_and(|image| image.controller == controller)).map(StrutId)
    }

    /// The struts as a store: their state here, their definitions and owners in `mem`.
    pub fn view<'a>(&'a mut self, mem: &'a Mem) -> StrutView<'a> {
        StrutView { mem, images: &self.images, struts: &mut self.struts, defs: None }
    }

    /// Every owned byte range, named by its wheel slot and part.
    pub fn owned(&self) -> Vec<(std::ops::Range<u32>, String)> {
        let slots = self.images.iter().enumerate().filter_map(|(k, image)| image.map(|image| (k, image)));
        slots.flat_map(|(k, image)| image.owned().map(move |(r, name)| (r, format!("wheel{k} {name}")))).collect()
    }
}

/// Struts in typed storage as a [`StrutStore`]: the state in [`CarStruts`], the definitions the step's typed
/// definitions hold ([`StrutView::with_defs`]), the other definitions and the owners' ids read from the image.
pub struct StrutView<'a> {
    mem: &'a Mem,
    images: &'a [Option<StrutImage>; 4],
    struts: &'a mut Struts,
    defs: Option<&'a Defs>,
}

impl<'a> StrutView<'a> {
    /// The view with the step's typed definitions (`None`: every definition on the image).
    pub fn with_defs(self, defs: Option<&'a Defs>) -> Self {
        StrutView { defs, ..self }
    }
}

impl StrutView<'_> {
    fn image(&self, strut: StrutId) -> &StrutImage {
        self.images[strut.0].as_ref().expect("a strut in its slot")
    }

    /// Panics for a strut without its fade pair, where the original reads address 0.
    fn has_fade(&self, strut: StrutId) {
        assert!(self.image(strut).fade != 0, "the strut at {:#010x} has no fade pair", self.image(strut).controller);
    }
}

impl StrutStore for StrutView<'_> {
    type Strut = StrutId;

    fn definition(&self, strut: StrutId) -> Definition {
        let at = self.image(strut).definition;
        match self.defs.and_then(|defs| defs.strut(at)) {
            Some(def) => defs::checked(*def, || definition(self.mem, at)),
            None => definition(self.mem, at),
        }
    }

    fn damping(&self, strut: StrutId) -> Damping {
        self.struts.get(strut).damping
    }

    fn store_damping(&mut self, strut: StrutId, value: Damping) {
        self.struts.get_mut(strut).damping = value;
    }

    fn owner_id(&self, strut: StrutId) -> u32 {
        let owner = StateObj::owner.load(self.mem, self.image(strut).state);
        self.mem.u32(owner + OWNER_WHEEL_INDEX)
    }

    fn controller(&self, strut: StrutId) -> Controller {
        self.struts.get(strut).controller
    }

    fn store_step_state(&mut self, strut: StrutId, value: Controller) {
        let controller = &mut self.struts.get_mut(strut).controller;
        controller.mode = value.mode;
        controller.high_speed_hold = value.high_speed_hold;
        controller.signal.peak_velocity = value.signal.peak_velocity;
        controller.signal.bump_velocity = value.signal.bump_velocity;
        controller.signal.droop_velocity = value.signal.droop_velocity;
    }

    fn store_signal(&mut self, strut: StrutId, value: DamperSignal) {
        self.struts.get_mut(strut).controller.signal = value;
    }

    fn fade(&self, strut: StrutId) -> FadeState {
        self.has_fade(strut);
        self.struts.get(strut).fade
    }

    fn store_fade(&mut self, strut: StrutId, value: FadeState) {
        self.has_fade(strut);
        self.struts.get_mut(strut).fade = value;
    }
}

/// The damper kernels' blocks on any word store, at their addresses.
impl<R: Raw> DamperBlocks for R {
    fn load_update_terms(&self, at: u32) -> UpdateTerms {
        UpdateTerms {
            scale: self.f32(at),
            numerator: self.f32(at + 4),
            denominator: self.f32(at + 8),
            response: self.f32(at + 12),
        }
    }

    fn store_update_terms(&mut self, at: u32, terms: UpdateTerms) {
        self.set_f32(at, terms.scale);
        self.set_f32(at + 4, terms.numerator);
        self.set_f32(at + 8, terms.denominator);
        self.set_f32(at + 12, terms.response);
    }

    fn load_blend_terms(&self, at: u32) -> BlendTerms {
        BlendTerms {
            weight: self.f32(at),
            value: self.f32(at + 4),
            response: self.f32(at + 8),
            scale: self.f32(at + 12),
        }
    }

    fn store_blend_terms(&mut self, at: u32, value: BlendTerms) {
        self.set_f32(at, value.weight);
        self.set_f32(at + 4, value.value);
        self.set_f32(at + 8, value.response);
        self.set_f32(at + 12, value.scale);
    }

    fn load_relaxation(&self, at: u32) -> Relaxation {
        Relaxation {
            remaining: self.f32(at),
            accumulated: self.f32(at + 4),
            response: self.f32(at + 8),
            scale: self.f32(at + 12),
        }
    }

    fn store_relaxation(&mut self, at: u32, value: Relaxation) {
        self.set_f32(at, value.remaining);
        self.set_f32(at + 4, value.accumulated);
        self.set_f32(at + 8, value.response);
        self.set_f32(at + 12, value.scale);
    }

    fn load_force_terms(&self, at: u32) -> ForceTerms {
        ForceTerms {
            multiplier: self.f32(at),
            accumulated_force: self.f32(at + 4),
            response: self.f32(at + 8),
            divisor: self.f32(at + 12),
        }
    }

    fn store_force_terms(&mut self, at: u32, value: ForceTerms) {
        self.set_f32(at, value.multiplier);
        self.set_f32(at + 4, value.accumulated_force);
        self.set_f32(at + 8, value.response);
        self.set_f32(at + 12, value.divisor);
    }

    fn load_heat_capacity(&self, at: u32) -> HeatCapacity {
        HeatCapacity { mass: self.f32(at), specific_heat: self.f32(at + 4) }
    }

    fn load_heat_accumulator(&self, at: u32) -> HeatAccumulator {
        HeatAccumulator { divisor: self.f32(at), temperature: self.f32(at + 4) }
    }

    fn store_heat_accumulator(&mut self, at: u32, value: HeatAccumulator) {
        self.set_f32(at, value.divisor);
        self.set_f32(at + 4, value.temperature);
    }

    fn load_clamp_parameters(&self, at: u32) -> ClampParameters {
        let values: [f32; 4] = self.f32s(at);
        ClampParameters {
            transfer_scale: values[0],
            transfer_divisor: values[1],
            reserved: values[2],
            maximum_value: values[3],
        }
    }

    fn load_weighted(&self, at: u32) -> WeightedValue {
        WeightedValue { weight: self.f32(at), value: self.f32(at + 4) }
    }

    fn store_weighted(&mut self, at: u32, value: WeightedValue) {
        self.set_f32(at, value.weight);
        self.set_f32(at + 4, value.value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{check_table, Object};

    const CONTROLLER: u32 = 0x1000;
    const STATE: u32 = 0x2000;
    const FADE: u32 = 0x3000;
    const DEFINITION: u32 = 0x4000;

    #[test]
    fn tables_are_ordered_and_fit() {
        check_table::<ControllerObj>();
        check_table::<StateObj>();
        check_table::<FadeObj>();
    }

    /// The strut's three objects mapped with `pattern`-derived bytes, the controller's pointers set.
    fn image(pattern: u8) -> Mem {
        let mut mem = Mem::new();
        let bytes = |n: u32, salt: u8| (0..n).map(|k| (k as u8).wrapping_mul(31) ^ salt ^ pattern).collect::<Vec<_>>();
        mem.map(CONTROLLER, &bytes(ControllerObj::SIZE + 0x10, 1));
        mem.map(STATE, &bytes(StateObj::SIZE, 2));
        mem.map(FADE - 8, &bytes(0x18, 3));
        mem.map(DEFINITION, &bytes(0x120, 4));
        ControllerObj::definition.store(&mut mem, CONTROLLER, DEFINITION);
        ControllerObj::state.store(&mut mem, CONTROLLER, STATE);
        ControllerObj::fade.store(&mut mem, CONTROLLER, FADE);
        mem
    }

    #[test]
    fn lower_writes_the_lifted_owned_words_and_nothing_else() {
        let a = image(0);
        let image_a = StrutImage::of(&a, CONTROLLER);
        assert_eq!(image_a, StrutImage { controller: CONTROLLER, definition: DEFINITION, state: STATE, fade: FADE });
        let strut = image_a.lift(&a);
        let mut b = image(0x5a);
        // Same pointers, so the same image.
        image_a.lower(&strut, &mut b);
        let owned: Vec<_> = image_a.owned().map(|(r, _)| r).collect();
        assert_eq!(owned.iter().map(|r| r.end - r.start).sum::<u32>(), 12 * 4);
        for (base, len) in [(CONTROLLER, ControllerObj::SIZE + 0x10), (STATE, StateObj::SIZE), (FADE - 8, 0x18)] {
            for at in base..base + len {
                let want = if owned.iter().any(|r| r.contains(&at)) { &a } else { &image(0x5a) };
                assert_eq!(b.u8(at), want.u8(at), "{at:#x}");
            }
        }
        assert_eq!(StrutImage::of(&b, CONTROLLER).lift(&b), strut);
    }

    /// `run` on the strut at `controller` lifted into a store of its own, lowered after it.
    fn with_strut<T>(mem: &mut Mem, controller: u32, run: impl FnOnce(&mut StrutView, StrutId) -> T) -> T {
        let mut one = CarStruts::lift_controllers(mem, [Some(controller), None, None, None]);
        let out = run(&mut one.view(mem), StrutId(0));
        one.lower(mem);
        out
    }

    #[test]
    fn the_reset_emits_the_held_signal_and_takes_its_owner_from_the_image() {
        let mut mem = image(0);
        let owner = 0x5000;
        StateObj::owner.store(&mut mem, STATE, owner);
        mem.map(owner + OWNER_WHEEL_INDEX, &2u32.to_le_bytes());
        let lifted = |mem: &Mem| StrutImage::of(mem, CONTROLLER).lift(mem);
        let held = lifted(&mem);
        assert_eq!(
            with_strut(&mut mem, CONTROLLER, |v, id| crate::spring_damper::strut::reset_outputs(v, id)),
            held.controller.signal
        );
        let after = lifted(&mem);
        let reset = DamperSignal { owner_id: 2, peak_velocity: 0.0, bump_velocity: 0.0, droop_velocity: 0.0, valid: 1 };
        assert_eq!(after.controller.signal, reset);
        assert_eq!((after.damping, after.fade), (held.damping, held.fade));
    }

    #[test]
    #[should_panic(expected = "has no fade pair")]
    fn the_view_panics_on_a_strut_without_its_fade_pair() {
        let mut mem = image(0);
        ControllerObj::fade.store(&mut mem, CONTROLLER, 0);
        assert_eq!(StrutImage::of(&mem, CONTROLLER).owned().count(), 2);
        with_strut(&mut mem, CONTROLLER, |v, id| crate::spring_damper::strut::update_forces(v, id));
    }
}
