//! Typed views of the game memory the VCU reads and writes: the setup (`CVehicleControlUnit::CDef`,
//! 0xcac bytes), the unit's state (`CState`, 0x68 bytes) and the per-step scratch (`CScratchSim`).
//! Offsets are the original's (rbr-recomp `notes/physics/renames/vcu.csv`, `notes/physics.md` "VCU"); this file alone
//! knows them. The unit's typed state ([`Unit`]: the scratch's and the state's changing words) lifts from its image
//! ([`VcuImage`]) and lowers back; [`VcuView`] is the store over it ([`with_vcu`] per call), with the definition and
//! the pointers read on the image and the drive's blocks on the drive's typed state (`engine_driveline::layout`).

use super::store::{DriveBlocks, VcuStore};
use crate::engine_driveline::layout::{CarDrive, DriveRef};
use crate::mem::Mem;
use crate::store::defs::{self, Defs};
use crate::store::Real;

/// Knots in a pedal map and in a speed map (the VCU always passes 11).
pub const KNOTS: usize = 11;
/// Forward gears with a shift threshold in the setup (`Gear0`..`Gear9`).
pub const GEARS: usize = 10;

/// `CVehicleControlUnit::CDef` members (rbr-recomp `notes/physics/renames/vcu.csv`). The per-gear rpm rows are 0x20
/// bytes apart.
const GEAR_UPSHIFT_RPM: u32 = 0x18;
const GEAR_DOWNSHIFT_RPM: u32 = 0x28;
const SHIFT_LOCKOUT_UP_S: u32 = 0x158;
const SHIFT_LOCKOUT_DOWN_S: u32 = 0x168;
const CLUTCH_RAMP_RATE: u32 = 0x178;
const RPM_LIMIT: u32 = 0x188;
const CENTRE_DIFF_HANDBRAKE_RELEASE: u32 = 0x198;
const LEFT_FOOT_BRAKE_THRESHOLD: u32 = 0x1a8;
/// CReal headers of the knot maps (the value is at +0xc; pedal knots are 0x10 bytes apart, speed records 0x20).
const CENTRE_THROTTLE_LOCK: u32 = 0x1ac;
const CENTRE_BRAKE_LOCK: u32 = 0x25c;
const CENTRE_SPEED_LOCK: u32 = 0x30c;
const LF_CENTRE_THROTTLE_LOCK: u32 = 0x46c;
const LF_CENTRE_BRAKE_LOCK: u32 = 0x51c;
const LF_CENTRE_SPEED_LOCK: u32 = 0x5cc;
const FRONT_THROTTLE_LOCK: u32 = 0x72c;
const FRONT_BRAKE_LOCK: u32 = 0x7dc;
const FRONT_SPEED_LOCK: u32 = 0x88c;
const REAR_THROTTLE_LOCK: u32 = 0x9ec;
const REAR_BRAKE_LOCK: u32 = 0xa9c;
const REAR_SPEED_LOCK: u32 = 0xb4c;

crate::object! {
    /// `CVehicleControlUnit::CState` (rbr-recomp `notes/physics/renames/vcu.csv`; the scratch's `m_state`). The words
    /// the constructor clears and the state's car; the size is the end of the last known field.
    pub struct StateObj, size 0x6c {
        vtable @ 0x00: u32,
        shift_lockout @ 0x2c: f32,
        /// The launch clutch's CReal: its dirty word (`+0x48`) and value (`+0x4c`).
        launch_clutch @ 0x40: Real,
        /// `+0x50`: a word the constructor clears; named by use, no reader known.
        cleared_a @ 0x50: u32,
        /// `+0x54`: a word the constructor clears; named by use, no reader known.
        cleared_b @ 0x54: u32,
        pedal_swap @ 0x58: u32,
        /// `+0x5c`: a word the constructor clears; named by use, no reader known.
        cleared_c @ 0x5c: u32,
        gear_timer @ 0x60: f32,
        shift_cooldown @ 0x64: f32,
        owner @ 0x68: u32,
    }
}

crate::object! {
    /// `CScratchSim` (rbr-recomp `notes/physics/renames/vcu.csv`; the car's `+0x850`). The size is the end of the last
    /// known field.
    pub struct ScratchObj, size 0x60 {
        def @ 0x00: u32,
        state @ 0x04: u32,
        vtable @ 0x08: u32,
        throttle @ 0x0c: f32,
        brake @ 0x10: f32,
        handbrake @ 0x14: f32,
        /// `+0x18` (`m_unk18`): the packet word ReadPacket carries; no reader known.
        carried_word @ 0x18: u32,
        fixed_clutch @ 0x1c: f32,
        launch_enabled @ 0x40: u32,
        slip_gate_rpm @ 0x44: u32,
        slip_gate_speed @ 0x48: u32,
        launch_mode @ 0x4c: u32,
        vcu_enabled @ 0x50: u32,
        engine @ 0x54: u32,
        drive @ 0x58: u32,
        lights @ 0x5c: u32,
    }
}

const SHIFT_LOCKOUT: u32 = StateObj::shift_lockout.at;
const LAUNCH_CLUTCH_DIRTY: u32 = StateObj::launch_clutch.at + 8;
const LAUNCH_CLUTCH: u32 = StateObj::launch_clutch.at + 12;
const PEDAL_SWAP: u32 = StateObj::pedal_swap.at;
const GEAR_TIMER: u32 = StateObj::gear_timer.at;
const SHIFT_COOLDOWN: u32 = StateObj::shift_cooldown.at;

/// One knot of a pedal map. Records are 0x10 bytes apart; the value is at +0x0c.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Knot {
    pub value: f32,
}

/// One record of a speed map. Records are 0x20 bytes apart; the speed key is at +0x0c and the
/// value at +0x1c.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpeedKnot {
    pub key: f32,
    pub value: f32,
}

/// The constants NGP repoints to its per-car `.data` (rbr-recomp `notes/ngp.md`, "VCU"): the shift cooldown (0.05 at 0x72dd78 in
/// GearboxUpdate and FlagDispatch), the launch rpm (5000, 0x731054), the launch clutch's band (200, 275 and
/// -1/75 at 0x730890, 0x741380, 0x74137c) and the rev cut's margin over the limit (300 at 0x72f5e0, RevLimitCut).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    pub cooldown: f32,
    pub launch_rpm: f32,
    pub clutch_from: f32,
    pub clutch_to: f32,
    pub clutch_slope: f32,
    pub cut_margin: f32,
}

impl Tuning {
    pub const STOCK: Tuning = Tuning {
        cooldown: 0.05,
        launch_rpm: 5000.0,
        clutch_from: 200.0,
        clutch_to: 275.0,
        clutch_slope: -0.013333334,
        cut_margin: 300.0,
    };
}

/// The setup object (`CDef`). Only the fields the ported functions read are here.
#[derive(Clone, Debug, PartialEq)]
pub struct Def {
    /// Upshift when the engine rpm is above this (0 = off), per gear.
    pub gear_upshift_rpm: [f32; GEARS],
    /// Downshift when the engine rpm is below this (0 = off), per gear.
    pub gear_downshift_rpm: [f32; GEARS],
    /// Shift lockout after an up request, seconds.
    pub shift_lockout_up_s: f32,
    /// Shift lockout after a down request, seconds.
    pub shift_lockout_down_s: f32,
    /// Launch clutch fall rate, per second.
    pub clutch_ramp_rate: f32,
    /// Rev limit, rpm.
    pub rpm_limit: f32,
    /// Centre lock is 0 while the handbrake is at or above this.
    pub centre_diff_handbrake_release: f32,
    /// Left-foot maps are used above this pedal value (0 = off).
    pub left_foot_brake_threshold: f32,
    /// Number of gears (`NumberOfGears`).
    pub gear_count: u32,
    pub centre_throttle_lock: [Knot; KNOTS],
    pub centre_brake_lock: [Knot; KNOTS],
    pub centre_speed_lock: [SpeedKnot; KNOTS],
    pub lf_centre_throttle_lock: [Knot; KNOTS],
    pub lf_centre_brake_lock: [Knot; KNOTS],
    pub lf_centre_speed_lock: [SpeedKnot; KNOTS],
    pub front_throttle_lock: [Knot; KNOTS],
    pub front_brake_lock: [Knot; KNOTS],
    pub front_speed_lock: [SpeedKnot; KNOTS],
    pub rear_throttle_lock: [Knot; KNOTS],
    pub rear_brake_lock: [Knot; KNOTS],
    pub rear_speed_lock: [SpeedKnot; KNOTS],
}

/// Three consecutive words of a knot record, the ones CopyDef moves. In a pedal map's record they sit at
/// +0x4, +0x8 and +0xc, and the knot value is `last`. A speed map's record holds two such triples: the key is
/// the last word of the first (+0xc) and the value the last of the second (+0x1c). `first` and `second` have
/// no known meaning.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KnotWords {
    pub first: u32,
    pub second: u32,
    pub last: u32,
}

impl KnotWords {
    fn load(mem: &Mem, at: u32) -> Self {
        KnotWords { first: mem.u32(at), second: mem.u32(at + 0x04), last: mem.u32(at + 0x08) }
    }

    fn store(&self, mem: &mut Mem, at: u32) {
        mem.set_u32(at, self.first);
        mem.set_u32(at + 0x04, self.second);
        mem.set_u32(at + 0x08, self.last);
    }
}

/// A pedal map's triples, one per knot.
pub type PedalWords = [KnotWords; KNOTS];

/// A speed map's triples: the keys (the first triple of each record) and the values (the second).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeedWords {
    pub keys: [KnotWords; KNOTS],
    pub values: [KnotWords; KNOTS],
}

fn load_triples(mem: &Mem, at: u32, stride: u32) -> [KnotWords; KNOTS] {
    std::array::from_fn(|k| KnotWords::load(mem, at + stride * k as u32))
}

fn store_triples(mem: &mut Mem, at: u32, stride: u32, triples: &[KnotWords; KNOTS]) {
    for (k, triple) in triples.iter().enumerate() {
        triple.store(mem, at + stride * k as u32);
    }
}

fn store_speed(mem: &mut Mem, keys: u32, values: u32, speed: &SpeedWords) {
    store_triples(mem, keys, 0x20, &speed.keys);
    store_triples(mem, values, 0x20, &speed.values);
}

/// The knot maps CopyDef copies from the setup into the CDef. In the setup each triple of a map is 0x10 bytes
/// from the next knot's; in the CDef a pedal triple is 0x10 and a speed triple 0x20 bytes from the next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KnotMaps {
    pub centre_throttle: PedalWords,
    pub centre_brake: PedalWords,
    pub lf_centre_throttle: PedalWords,
    pub lf_centre_brake: PedalWords,
    pub front_throttle: PedalWords,
    pub front_brake: PedalWords,
    pub rear_throttle: PedalWords,
    pub rear_brake: PedalWords,
    pub centre_speed: SpeedWords,
    pub lf_centre_speed: SpeedWords,
    pub front_speed: SpeedWords,
    pub rear_speed: SpeedWords,
}

/// The setup's words that CopyDef reads outside the knot maps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SetupFlags {
    /// `+0x18`: the launch flag. The scratch's launch and VCU flags take it.
    pub launch: u32,
    /// `+0x28`: the rpm slip gate.
    pub slip_gate_rpm: u32,
    /// `+0x38`: the speed slip gate.
    pub slip_gate_speed: u32,
    /// `+0x48`: the launch mode when the launch flag is off.
    pub launch_mode_when_off: u32,
}

impl SetupFlags {
    /// The scratch's launch mode (`+0x4c`): 1 when the launch flag is set, else `launch_mode_when_off`.
    pub fn launch_mode(&self) -> u32 {
        if self.launch != 0 {
            1
        } else {
            self.launch_mode_when_off
        }
    }
}

/// The unit's state (`CState`, the scratch's `m_state`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    /// Shift requests are blocked while this is nonzero (seconds).
    pub shift_lockout: f32,
    /// Launch clutch value sent to the drive (0..1).
    pub launch_clutch: f32,
    /// Dirty flag of the launch clutch's value object (`+0x48`), set on every clutch write.
    pub launch_clutch_dirty: u32,
    /// Pedal routing flag: 1 swaps the pedals sent to the engine and the drive (the asm stores an integer).
    pub pedal_swap: u32,
    /// Counts for the neutral and second-gear low-speed rules (seconds).
    pub gear_timer: f32,
    /// Set to 0.05 s on each request; read by the rev-limit cut (seconds).
    pub shift_cooldown: f32,
}

/// Engine speed (rad/s) to rpm, the asm's `9.5492964f`.
pub const RPM_PER_RAD_S: f32 = 9.549296;

/// The drive's gear fields (the `CDrive + 8` sub-object): the gear in use, the gear request (its
/// `+0x564` block) and the slip flag that `CheckSlip` reads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gearbox {
    /// Gear in use (`+0x54c`).
    pub gear: u32,
    /// Commanded gear (`+0x570`, the request's gear).
    pub commanded: u32,
    /// Set by every request (`+0x56c`).
    pub changed: u32,
    /// Slip flag (`+0x55c`).
    pub slip: u32,
}

/// The drive's CReal outputs the step sets. `CDrive::Set44A0` (+0x20) holds the throttle and `Set30` (+0x30)
/// the brake; `Set30` also writes the brake times `brake_scale` (the definition's `+0x620`) to +0x64, and
/// `Set4820` (+0x54) the clutch slip. A write sets the object's dirty flag (+8, to 1) and its value (+0xc).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reals {
    pub throttle: f32,
    pub brake: f32,
    pub brake_scaled: f32,
    pub clutch_slip: f32,
    /// Read by `Set30`, not stored here.
    pub brake_scale: f32,
}

/// What the step writes into the drive: the diff locks (`Set4570`) and the CReals.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Outputs {
    pub locks: Locks,
    pub reals: Reals,
}

/// The drive definition's per-gear ratios that `E50_QuerySlip` reads, 0x50 bytes per gear.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GearRatios {
    /// `+0x644` (rbr-recomp's `lim`): the word 0x50 before `ratio`, so gear `g - 1`'s ratio for `g >= 1`.
    pub lower_ratio: [f32; GEARS],
    /// `+0x694` (rbr-recomp's `div`): the gear's ratio (its gear band's, `engine_driveline::layout::GearBand::ratio`).
    pub ratio: [f32; GEARS],
}

/// The drive's diff-lock channels, as `CDrive::Set4570` reads and writes them: each channel's record in the drive state
/// (`[drive + 8]`, 0x14 bytes: `ratio` at +0x0c, `torque` at +0x18), the gate at `drive + 0x170` (its `+0x90`), and each
/// channel's `max_torque` in the drive's definition (`drive + 4`, `+0x58` with 0x60 per channel).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Locks {
    pub ratio: [f32; 3],
    pub enabled: bool,
    pub max_torque: [f32; 3],
    pub torque: [f32; 3],
}

/// The driver's pedals as the scratch holds them (0..1; the handbrake too).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pedals {
    pub throttle: f32,
    pub brake: f32,
    pub handbrake: f32,
}

/// The request packet the car fills in each step, as `CScratchSim::ReadPacket` reads it. The pedals and the
/// fixed clutch go to the scratch; the flags and the gear value go to FlagDispatch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Packet {
    pub pedals: Pedals,
    pub fixed_clutch: f32,
    /// The word at +0x10, copied to the scratch's `carried_word`. Named by use; meaning unknown.
    pub carried_word: u32,
    /// The request flags (+0x14): 1 upshift, 2 downshift, 4 set the gear to `value`.
    pub flags: u8,
    /// The gear for the set flag (+0x18).
    pub value: u32,
}

/// The per-step scratch's changing words (`CScratchSim`, `car + 0x850`); its pointers stay the image's
/// ([`VcuImage`], [`VcuStore::parts`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scratch {
    pub pedals: Pedals,
    /// Fixed clutch sent when the launch mode is off (copied from the packet by ReadPacket).
    pub fixed_clutch: f32,
    /// `+0x18`: copied from the packet's `carried_word` by ReadPacket. Named by use; meaning
    /// unknown.
    pub carried_word: u32,
    /// The gear logic and the launch step run only when this is nonzero.
    pub launch_enabled: u32,
    /// Selects the launch clutch logic (`UpdateC`): set from the setup's `+0x18`, or else its `+0x48`.
    pub launch_mode: u32,
    /// The gear logic runs only when this is nonzero.
    pub vcu_enabled: u32,
    /// The rpm slip gate (`+0x44`, copied from the setup by CopyDef): the slip query checks it.
    pub slip_gate_rpm: u32,
    /// The speed slip gate (`+0x48`, copied from the setup by CopyDef): the slip query checks it.
    pub slip_gate_speed: u32,
}

/// The unit's typed state: its scratch's and its state's changing words.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Unit {
    pub scratch: Scratch,
    pub state: State,
}

/// `CVehicleControlUnit::CDef`'s vtable (0x7412dc) and the name its base gets (`"ControlUnit"`, 0x7380c8).
const DEF_VTABLE: u32 = 0x0074_12DC;
const DEF_NAME: u32 = 0x0073_80C8;
/// The definition's size: its tweaks are all reals, 0x10 apart from +0xc (`CReal2A`, `CReal2B` and the arrays are
/// pairs and runs of `NTweak::CReal`).
pub const DEF_SIZE: u32 = 0xCAC;
/// The reals the constructor sets (dirty, then the value): +0x14c..+0x17c, from the float words at 0x72f674, 0x72f428,
/// 0x72d150 and 0x731050.
const DEF_DEFAULTS: [(u32, f32); 4] = [(0x14C, 0.7), (0x15C, 0.2), (0x16C, 4.0), (0x17C, 7_800.0)];

/// `CVehicleControlUnit::CDef::CDef` (0x5377a0) on the definition at `this`: the base, the vtable, every real, then
/// the four defaults.
pub fn construct_def(mem: &mut Mem, this: u32) {
    crate::ntweak::construct_base(mem, this, DEF_NAME);
    mem.set_u32(this, DEF_VTABLE);
    for at in (0xC..DEF_SIZE).step_by(0x10) {
        crate::ntweak::construct_real(mem, this + at);
    }
    for (at, value) in DEF_DEFAULTS {
        crate::ntweak::set_dirty(mem, this + at);
        mem.set_f32(this + at + 0xC, value);
    }
}

/// `CVehicleControlUnit::CState`'s vtable (0x74132c) and `CScratchSim`'s (0x7412d0, at its +8; slot 1 returns).
const STATE_VTABLE: u32 = 0x0074_132C;
const SCRATCH_SIM_VTABLE: u32 = 0x0074_12D0;

/// `CVehicleControlUnit::CState::CState` (0x536d50, thiscall, three stack arguments it does not read) on the state at
/// `this`: the base named like the definition, the vtable, its tweaks (a real, an unsigned, two reals) and the cleared
/// words.
pub fn construct_state(mem: &mut Mem, this: u32) {
    crate::ntweak::construct_base(mem, this, DEF_NAME);
    mem.set_u32(this, STATE_VTABLE);
    crate::ntweak::construct_real(mem, this + 0xC);
    crate::ntweak::construct_unsigned(mem, this + 0x1C);
    crate::ntweak::construct_real(mem, this + 0x30);
    crate::ntweak::construct_real(mem, this + 0x40);
    for at in [0x50, 0x54, 0x5C, 0x60, 0x58, 0x2C, 0x64] {
        mem.set_u32(this + at, 0);
    }
}

/// The objects `CScratchSim` keeps (its `+0x54..+0x5c`): the car's engine, drive and light system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimParts {
    pub engine: u32,
    pub drive: u32,
    pub lights: u32,
}

/// `CVehicleControlUnit::CScratchSim::CScratchSim(def, state, owner, engine, drive, lights)` (0x536e50, thiscall) on
/// the scratch at `this`: the vtable at +8, the cleared words, the parts, the definition and state, the owner into the
/// state (`+0x68`) when there is a definition, then the counters (+0x40 is 1).
pub fn construct_scratch_sim(mem: &mut Mem, this: u32, def: u32, state: u32, owner: u32, parts: SimParts) {
    mem.set_u32(this + 8, SCRATCH_SIM_VTABLE);
    for at in [0x10, 0x14, 0x18, 0xC] {
        mem.set_u32(this + at, 0);
    }
    mem.set_u32(this + 0x54, parts.engine);
    mem.set_u32(this + 0x58, parts.drive);
    mem.set_u32(this + 0x5C, parts.lights);
    mem.set_u32(this, def);
    mem.set_u32(this + 4, state);
    if def != 0 {
        mem.set_u32(state + 0x68, owner);
    }
    for at in [0x4C, 0x44, 0x48] {
        mem.set_u32(this + at, 0);
    }
    mem.set_u32(this + 0x40, 1);
}

/// `CVehicleControlUnit::CSetup`'s vtable (0x72e534) and its size (its last real ends at +0xb6c).
const SETUP_VTABLE: u32 = 0x0072_E534;
const SETUP_SIZE: u32 = 0xB6C;

/// `CVehicleControlUnit::CSetup::CSetup` (0x47b720) on the setup at `this`: an unnamed tweak, its vtable, unsigneds at
/// +0xc..+0x3c (`AutoGears`, `GearGuard`, `NeutralLock`, `ClutchHelp`), then reals from +0x4c on (the two thresholds
/// and the maps).
pub fn construct_setup(mem: &mut Mem, this: u32) {
    crate::ntweak::construct_base(mem, this, 0);
    mem.set_u32(this, SETUP_VTABLE);
    for at in (0xC..0x4C).step_by(0x10) {
        crate::ntweak::construct_unsigned(mem, this + at);
    }
    for at in (0x4C..SETUP_SIZE).step_by(0x10) {
        crate::ntweak::construct_real(mem, this + at);
    }
}

/// `CVehicleControlUnit::CSetupOptions`'s vtable (0x72e7c4).
const OPTIONS_VTABLE: u32 = 0x0072_E7C4;

/// `CVehicleControlUnit::CSetupOptions::CSetupOptions` (0x47d710) on the options at `this`: an unnamed tweak, its
/// vtable, and four ranges (`E13RealTriple`s: the diff map's lock and speed, the handbrake release and the left-foot
/// threshold) from +0xc, every real 0x10 apart.
pub fn construct_options(mem: &mut Mem, this: u32) {
    crate::ntweak::construct_base(mem, this, 0);
    mem.set_u32(this, OPTIONS_VTABLE);
    for at in (0xC..0xCC).step_by(0x10) {
        crate::ntweak::construct_real(mem, this + at);
    }
}

/// The setup's unsigneds from +0xc (`AutoGears`, `GearGuard`, `NeutralLock`, `ClutchHelp`) as the settings code stores
/// the player's driver aids there, each marked dirty first (`CCarSettingsManager::LoadGlobal` 0x547080 and `CommitGlobal`
/// 0x5471a0 the first three, the gears page's `vf48` 0x5486c0 the last).
pub fn set_driver_aids(mem: &mut Mem, this: u32, aids: [u32; 4]) {
    for (k, value) in aids.into_iter().enumerate() {
        let at = this + 0xC + 0x10 * k as u32;
        crate::ntweak::set_dirty(mem, at);
        mem.set_u32(at + 0xC, value);
    }
}

/// The definition's part of `CScratchSim::CopyDef` (0x537c90): the setup's knot maps into the definition at `def`
/// (`copy_def` does it through a scratch).
pub fn copy_maps(mem: &mut Mem, def: u32, setup: u32) {
    let maps = knot_maps(mem, setup);
    store_knot_maps(mem, def, &maps);
}

/// `CScratchSim::CopyDef` (`CopySetup`, 0x537c90) up to the scratch's fields: the setup at `setup`'s knot maps into
/// the CDef of the scratch at `scratch`, and the setup's flags, which [`copy_flags`] sets on the scratch after them. The
/// copies are dword moves, so the bytes are the original's whatever the values. The asm's order decides the writes
/// (`notes/physics.md` "VCU"); the C++ in `src/rbr` has them in a different order, which the asm decides.
pub fn copy_def(mem: &mut Mem, scratch: u32, setup: u32) -> SetupFlags {
    copy_maps(mem, ScratchObj::def.load(mem, scratch), setup);
    setup_flags(mem, setup)
}

/// CopyDef's last writes: the scratch's launch and slip fields from the setup's flags.
pub fn copy_flags<S: VcuStore>(store: &mut S, flags: SetupFlags) {
    let value = Scratch {
        launch_enabled: flags.launch,
        vcu_enabled: flags.launch,
        launch_mode: flags.launch_mode(),
        slip_gate_rpm: flags.slip_gate_rpm,
        slip_gate_speed: flags.slip_gate_speed,
        ..store.scratch()
    };
    store.store_scratch(&value);
}

/// `CScratchSim::ReadPacket` (0x537f20): copies the packet's pedals, fixed clutch and `carried_word` into the
/// scratch. The asm tail-jumps into FlagDispatch, so the caller applies the packet with [`apply_packet`].
pub fn read_packet<S: VcuStore>(store: &mut S, packet: Packet) {
    let value = Scratch {
        pedals: packet.pedals,
        fixed_clutch: packet.fixed_clutch,
        carried_word: packet.carried_word,
        ..store.scratch()
    };
    store.store_scratch(&value);
}

/// The original functions the VCU's entries call (thiscall each): the drive's, the engine's, the car's and the lights'
/// accessors and setters, and the slip query (the VCU's own). The bindings run them on the step's held state, else
/// through the host as the original calls them (`rbr-hybrid`'s `VcuCallees`).
pub trait VcuCalls {
    /// `CDrive::Get4520`: the engine speed (rad/s).
    fn engine_speed(&mut self, drive: u32) -> f32;
    /// `CDrive::Get4540`: the diff-lock speed key.
    fn lock_speed(&mut self, drive: u32) -> f32;
    /// `CCar::Get15F8`: the car speed.
    fn car_speed(&mut self, car: u32) -> f32;
    /// `CDrive::Set4570 (channel, value)`, eax.
    fn set_diff_lock(&mut self, drive: u32, channel: u32, value: f32) -> u32;
    /// `CDrive::Set4820 (value)`: the clutch slip, eax.
    fn set_clutch_slip(&mut self, drive: u32, value: f32) -> u32;
    /// `CEngine::SetState4`: the four cut flags, eax.
    fn set_cut_flags(&mut self, engine: u32, flags: [u32; 4]) -> u32;
    /// `CEngine::SetScratch08`: the pedal.
    fn set_engine_pedal(&mut self, engine: u32, value: f32);
    /// `Lights::Set1e40`: the brake light.
    fn set_brake_light(&mut self, lights: u32, on: bool);
    /// `Lights::Set1ed0`: the neutral light.
    fn set_neutral_light(&mut self, lights: u32, on: bool);
    /// `E50_QuerySlip` (fastcall on the unit's scratch), as FlagDispatch calls it.
    fn query_slip(&mut self) -> bool;
}

// The VCU functions' sequences, one per original entry, generic over the store (moved from the bindings in
// `rbr-hybrid/src/vcu.rs`, which decode the registers and call these).

/// `E49Proxy::VcuTableBlend (table, x, lo, hi, n)`, st0, for `n` from 2 (the VCU always passes 11): a map of the
/// definition, read where it is.
pub fn table_blend(mem: &Mem, table: u32, x: f32, lo: f32, hi: f32, n: u32) -> f64 {
    super::tables::table_blend(&pedal_map(mem, table, n as usize), x, lo, hi)
}

/// `NPhysics::LookupPairTable (table, x, n)`, st0, for `n` from 1.
pub fn lookup_pair_table(mem: &Mem, table: u32, x: f32, n: u32) -> f64 {
    super::tables::lookup_pair_table(&speed_map(mem, table, n as usize), x)
}

/// `CScratchSim::LockMax (t1, t2, t3, f)`, st0: the largest of the throttle, brake and speed maps (the definition's,
/// at `t`) at the scratch's `pedals`.
pub fn lock_max(mem: &Mem, pedals: Pedals, t: [u32; 3], f: f32) -> f32 {
    let [throttle, brake, speed] = t;
    super::locks::lock_max(pedals, &pedal_knots(mem, throttle), &pedal_knots(mem, brake), &speed_knots(mem, speed), f)
}

/// `CScratchSim::DiffLocks`: the three diff locks, sent to the drive's `Set4570`; eax is the last send's.
pub fn diff_locks<S: VcuStore>(store: &S, calls: &mut impl VcuCalls) -> u32 {
    let drive = store.parts().drive;
    let speed = calls.lock_speed(drive);
    let values = super::locks::diff_locks(store.scratch().pedals, &store.def(), speed);
    let mut eax = 0;
    for (channel, value) in values.into_iter().enumerate() {
        eax = calls.set_diff_lock(drive, channel as u32, value);
    }
    eax
}

/// `E50_QuerySlip`: the slip query before a downshift. The engine speed and the car speed come from their getters,
/// each called only where its gate reads it, as the original does.
pub fn query_slip<S: VcuStore + DriveBlocks>(store: &S, calls: &mut impl VcuCalls) -> bool {
    let scratch = store.scratch();
    let drive = store.parts().drive;
    let gear = store.gearbox(drive).gear;
    let gears = store.gear_ratios(drive);
    let rpm_limit = store.def().rpm_limit;
    let engine = if scratch.slip_gate_rpm != 0 && gear as i32 > 2 { calls.engine_speed(drive) } else { 0.0 };
    let car_speed = if scratch.slip_gate_speed != 0 && gear == 2 { calls.car_speed(store.car()) } else { 0.0 };
    super::slip::query_slip(scratch.slip_gate_rpm, scratch.slip_gate_speed, gear, &gears, rpm_limit, engine, car_speed)
}

/// `CScratchSim::RevLimitCut`: the rev-limit cut flags, sent to the engine's `SetState4`; eax is its.
pub fn rev_limit_cut<S: VcuStore>(store: &S, calls: &mut impl VcuCalls, margin: f32) -> u32 {
    let parts = store.parts();
    let engine_speed = calls.engine_speed(parts.drive);
    let flags = super::cut::rev_limit_cut(engine_speed, &store.def(), &store.state(), margin);
    calls.set_cut_flags(parts.engine, flags)
}

/// `CScratchSim::CheckSlip (x)`: the slip gate.
pub fn check_slip<S: VcuStore + DriveBlocks>(store: &S, x: f32) -> bool {
    let slip = store.gearbox(store.parts().drive).slip;
    super::cut::check_slip(store.scratch().vcu_enabled, slip, x)
}

/// The engine speed and the car speed the gear logic reads, in its order (`CDrive::Get4520`, then `CCar::Get15F8` on
/// the state's car), or None where it returns before reading them: the launch flag off, or a shift under way.
fn gear_speeds<S: VcuStore>(
    store: &S,
    calls: &mut impl VcuCalls,
    scratch: &Scratch,
    gears: &Gearbox,
) -> Option<(f32, f32)> {
    if scratch.launch_enabled == 0 || gears.gear != gears.commanded {
        return None;
    }
    let engine = calls.engine_speed(store.parts().drive);
    Some((engine, calls.car_speed(store.car())))
}

/// `CScratchSim::GearboxUpdate (dt)`: the gear logic. With the launch flag off, or a shift under way, it returns before
/// any call. The requests' `CDrive::Set47B0` writes (`SetDirty` and the commanded gear) are the gear block it stores.
pub fn gearbox_update<S: VcuStore + DriveBlocks>(store: &mut S, calls: &mut impl VcuCalls, dt: f32, tuning: Tuning) {
    let scratch = store.scratch();
    let drive = store.parts().drive;
    let mut gears = store.gearbox(drive);
    let (engine, speed) = gear_speeds(store, calls, &scratch, &gears).unwrap_or((0.0, 0.0));
    let input = super::gear::Input {
        launch_enabled: scratch.launch_enabled,
        vcu_enabled: scratch.vcu_enabled,
        pedals: scratch.pedals,
        engine,
        speed,
        dt,
        gear_count: store.gear_count(drive),
        tuning,
    };
    let def = store.def();
    let mut state = store.state();
    super::gear::update(input, &def, &mut state, &mut gears);
    store.store_state(&state);
    store.store_gearbox(drive, &gears);
}

/// `CScratchSim::LaunchClutch (dt)`: the launch clutch, sent to the drive's `Set4820` (nothing with the launch mode
/// off); eax is the send's, else 0.
pub fn launch_clutch<S: VcuStore + DriveBlocks>(
    store: &mut S,
    calls: &mut impl VcuCalls,
    dt: f32,
    tuning: Tuning,
) -> u32 {
    let scratch = store.scratch();
    if scratch.launch_mode == 0 {
        return 0;
    }
    let drive = store.parts().drive;
    let input =
        super::clutch::Input { launch_mode: scratch.launch_mode, engine: calls.engine_speed(drive), dt, tuning };
    let gears = store.gearbox(drive);
    let def = store.def();
    let mut state = store.state();
    let sent = super::clutch::launch_clutch(input, &gears, &def, &mut state);
    store.store_state(&state);
    match sent {
        Some(value) => calls.set_clutch_slip(drive, value),
        None => 0,
    }
}

/// FlagDispatch's body: the packet's flags as gear requests; whether anything was requested. The drive setters it
/// calls (`Set47B0`, `M6840`) write the gear block it stores; the slip query reads the gear in use, the tables and the
/// speeds, none of which an upshift request changes, so the stores can follow it.
pub fn apply_packet<S: VcuStore + DriveBlocks>(
    store: &mut S,
    calls: &mut impl VcuCalls,
    packet: Packet,
    tuning: &Tuning,
) -> bool {
    let scratch = store.scratch();
    let drive = store.parts().drive;
    let mut gears = store.gearbox(drive);
    let def = store.def();
    let mut state = store.state();
    let gear_count = store.gear_count(drive);
    let on = super::dispatch::flag_dispatch(
        packet,
        || calls.query_slip(),
        scratch.vcu_enabled,
        gear_count,
        &def,
        tuning,
        &mut state,
        &mut gears,
    );
    if on {
        store.store_state(&state);
        store.store_gearbox(drive, &gears);
    }
    on
}

/// `CDrive::M6840 (value)`, the drive's own: `SetDirty` on the gear request (the drive state's `+0x564`, whose dirty
/// word is [`Gearbox::changed`]), then the commanded gear at `+0x570`. The caller makes the `SetDirty` call before
/// this; `dirty` sets the flag here instead, where the caller skipped it (`SetDirty`'s one write is the flag, to 1).
pub fn set_commanded_gear<S: DriveBlocks>(store: &mut S, drive: u32, value: u32, dirty: bool) {
    let count = store.gear_count(drive);
    let mut gears = store.gearbox(drive);
    if dirty {
        gears.changed = 1;
    }
    gears.commanded = super::gear::commanded_gear(count, value);
    store.store_gearbox(drive, &gears);
}

/// `CScratchSim::Step (dt)` (`gear`, with the gear logic) or `StepBasic`. It writes the state, the drive's gear block
/// and its outputs (the drive setters' writes); the engine and light setters are called in the port's order, after
/// the stores. The original's eax at its ret is the state pointer ([`state_of`]).
pub fn step<S: VcuStore + DriveBlocks>(store: &mut S, calls: &mut impl VcuCalls, dt: f32, gear: bool, tuning: Tuning) {
    use super::step::Send;
    let scratch = store.scratch();
    let parts = store.parts();
    let drive = parts.drive;
    let mut gears = store.gearbox(drive);
    // Read only by the gear logic, which skips it without the launch flag or during a shift.
    let car_speed =
        if gear { gear_speeds(store, calls, &scratch, &gears).map_or(0.0, |(_, speed)| speed) } else { 0.0 };
    let input = super::step::Input {
        dt,
        engine: calls.engine_speed(drive),
        car_speed,
        drive_speed: calls.lock_speed(drive),
        gear_count: store.gear_count(drive),
        tuning,
    };
    let def = store.def();
    let mut state = store.state();
    let mut out = store.outputs(drive);
    // The step sends at most ten: seven, and a lock on each of the three channels.
    let mut sends = [None; 10];
    let mut sent = 0;
    let port = if gear { super::step::step } else { super::step::step_basic };
    port(input, &scratch, &def, &mut state, &mut gears, &mut out, &mut |s| {
        sends[sent] = Some(s);
        sent += 1;
    });
    store.store_state(&state);
    store.store_gearbox(drive, &gears);
    store.store_outputs(drive, &out);
    for send in sends.into_iter().flatten() {
        match send {
            Send::EnginePedal(v) => calls.set_engine_pedal(parts.engine, v),
            Send::CutFlags(flags) => {
                calls.set_cut_flags(parts.engine, flags);
            }
            Send::BrakeLight(on) => calls.set_brake_light(parts.lights, on),
            Send::NeutralLight(on) => calls.set_neutral_light(parts.lights, on),
            // the drive setters' writes are the outputs stored above
            Send::DriveThrottle(_) | Send::DriveBrake(_) | Send::DriveClutch(_) | Send::DiffLock { .. } => {}
        }
    }
}

/// The state of the scratch at `scratch` (its `m_state`, an image pointer).
pub fn state_of(mem: &Mem, scratch: u32) -> u32 {
    ScratchObj::state.load(mem, scratch)
}

/// Knot `k` of the speed map at `at`.
fn speed_knot(mem: &Mem, at: u32, k: u32) -> SpeedKnot {
    SpeedKnot { key: mem.f32(at + 0x20 * k + 0x0c), value: mem.f32(at + 0x20 * k + 0x1c) }
}

/// Knot `k` of the pedal map at `at`.
fn pedal_knot(mem: &Mem, at: u32, k: u32) -> Knot {
    Knot { value: mem.f32(at + 0x10 * k + 0x0c) }
}

/// The [`KNOTS`] records of the speed map at `at`.
fn speed_knots(mem: &Mem, at: u32) -> [SpeedKnot; KNOTS] {
    std::array::from_fn(|k| speed_knot(mem, at, k as u32))
}

/// The [`KNOTS`] knots of the pedal map at `at`.
fn pedal_knots(mem: &Mem, at: u32) -> [Knot; KNOTS] {
    std::array::from_fn(|k| pedal_knot(mem, at, k as u32))
}

/// The speed map of `n` records at `at`.
pub fn speed_map(mem: &Mem, at: u32, n: usize) -> Vec<SpeedKnot> {
    (0..n as u32).map(|k| speed_knot(mem, at, k)).collect()
}

/// The pedal map of `n` knots at `at`.
pub fn pedal_map(mem: &Mem, at: u32, n: usize) -> Vec<Knot> {
    (0..n as u32).map(|k| pedal_knot(mem, at, k)).collect()
}

/// The bytes of the definition [`def`] reads, from the first gear's rpm to the last speed knot (the object's end):
/// what the step's typed definition stands for (`store::defs`).
pub const DEF_BYTES: std::ops::Range<u32> = GEAR_UPSHIFT_RPM..REAR_SPEED_LOCK + 0x20 * (KNOTS as u32 - 1) + 0x20;
const _: () = assert!(DEF_BYTES.end == DEF_SIZE);

/// The definition at `at`. The memory must cover the whole object (0xcac bytes).
pub fn def(mem: &Mem, at: u32) -> Def {
    let f = |off: u32| mem.f32(at + off);
    Def {
        gear_upshift_rpm: std::array::from_fn(|k| f(GEAR_UPSHIFT_RPM + 0x20 * k as u32)),
        gear_downshift_rpm: std::array::from_fn(|k| f(GEAR_DOWNSHIFT_RPM + 0x20 * k as u32)),
        shift_lockout_up_s: f(SHIFT_LOCKOUT_UP_S),
        shift_lockout_down_s: f(SHIFT_LOCKOUT_DOWN_S),
        clutch_ramp_rate: f(CLUTCH_RAMP_RATE),
        rpm_limit: f(RPM_LIMIT),
        centre_diff_handbrake_release: f(CENTRE_DIFF_HANDBRAKE_RELEASE),
        left_foot_brake_threshold: f(LEFT_FOOT_BRAKE_THRESHOLD),
        gear_count: mem.u32(at + 0x654),
        centre_throttle_lock: pedal_knots(mem, at + CENTRE_THROTTLE_LOCK),
        centre_brake_lock: pedal_knots(mem, at + CENTRE_BRAKE_LOCK),
        centre_speed_lock: speed_knots(mem, at + CENTRE_SPEED_LOCK),
        lf_centre_throttle_lock: pedal_knots(mem, at + LF_CENTRE_THROTTLE_LOCK),
        lf_centre_brake_lock: pedal_knots(mem, at + LF_CENTRE_BRAKE_LOCK),
        lf_centre_speed_lock: speed_knots(mem, at + LF_CENTRE_SPEED_LOCK),
        front_throttle_lock: pedal_knots(mem, at + FRONT_THROTTLE_LOCK),
        front_brake_lock: pedal_knots(mem, at + FRONT_BRAKE_LOCK),
        front_speed_lock: speed_knots(mem, at + FRONT_SPEED_LOCK),
        rear_throttle_lock: pedal_knots(mem, at + REAR_THROTTLE_LOCK),
        rear_brake_lock: pedal_knots(mem, at + REAR_BRAKE_LOCK),
        rear_speed_lock: speed_knots(mem, at + REAR_SPEED_LOCK),
    }
}

/// The maps of the setup at `setup`.
fn knot_maps(mem: &Mem, setup: u32) -> KnotMaps {
    let pedal = |at: u32| load_triples(mem, setup + at, 0x10);
    let speed = |keys: u32, values: u32| SpeedWords {
        keys: load_triples(mem, setup + keys, 0x10),
        values: load_triples(mem, setup + values, 0x10),
    };
    KnotMaps {
        centre_throttle: pedal(0x070),
        centre_brake: pedal(0x120),
        lf_centre_throttle: pedal(0x330),
        lf_centre_brake: pedal(0x3e0),
        front_throttle: pedal(0x5f0),
        front_brake: pedal(0x6a0),
        rear_throttle: pedal(0x8b0),
        rear_brake: pedal(0x960),
        centre_speed: speed(0x1d0, 0x280),
        lf_centre_speed: speed(0x490, 0x540),
        front_speed: speed(0x750, 0x800),
        rear_speed: speed(0xa10, 0xac0),
    }
}

/// Writes the maps into the CDef at `def`.
fn store_knot_maps(mem: &mut Mem, def: u32, value: &KnotMaps) {
    store_triples(mem, def + 0x1b0, 0x10, &value.centre_throttle);
    store_triples(mem, def + 0x260, 0x10, &value.centre_brake);
    store_triples(mem, def + 0x470, 0x10, &value.lf_centre_throttle);
    store_triples(mem, def + 0x520, 0x10, &value.lf_centre_brake);
    store_triples(mem, def + 0x730, 0x10, &value.front_throttle);
    store_triples(mem, def + 0x7e0, 0x10, &value.front_brake);
    store_triples(mem, def + 0x9f0, 0x10, &value.rear_throttle);
    store_triples(mem, def + 0xaa0, 0x10, &value.rear_brake);
    store_speed(mem, def + 0x310, def + 0x320, &value.centre_speed);
    store_speed(mem, def + 0x5d0, def + 0x5e0, &value.lf_centre_speed);
    store_speed(mem, def + 0x890, def + 0x8a0, &value.front_speed);
    store_speed(mem, def + 0xb50, def + 0xb60, &value.rear_speed);
}

fn setup_flags(mem: &Mem, setup: u32) -> SetupFlags {
    SetupFlags {
        launch: mem.u32(setup + 0x18),
        slip_gate_rpm: mem.u32(setup + 0x28),
        slip_gate_speed: mem.u32(setup + 0x38),
        launch_mode_when_off: mem.u32(setup + 0x48),
    }
}

/// The request packet at `at`.
pub fn packet(mem: &Mem, at: u32) -> Packet {
    Packet {
        pedals: Pedals { throttle: mem.f32(at), brake: mem.f32(at + 0x04), handbrake: mem.f32(at + 0x08) },
        fixed_clutch: mem.f32(at + 0x0c),
        carried_word: mem.u32(at + 0x10),
        flags: mem.u8(at + 0x14),
        value: mem.u32(at + 0x18),
    }
}

/// Where a unit's typed state lives in the image: its scratch, and the definition and state the scratch points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VcuImage {
    pub scratch: u32,
    pub def: u32,
    pub state: u32,
}

/// The words a unit's typed state owns: the scratch's pedals to its fixed clutch and its launch and slip fields, the
/// state's shift lockout, launch clutch (its dirty word and value), pedal swap, gear timer and shift cooldown.
const OWNED_SCRATCH: [(std::ops::Range<u32>, &str); 2] = [
    (ScratchObj::throttle.at..ScratchObj::fixed_clutch.at + 4, "vcu pedals"),
    (ScratchObj::launch_enabled.at..ScratchObj::vcu_enabled.at + 4, "vcu flags"),
];
const OWNED_STATE: [(std::ops::Range<u32>, &str); 4] = [
    (SHIFT_LOCKOUT..SHIFT_LOCKOUT + 4, "vcu shift lockout"),
    (LAUNCH_CLUTCH_DIRTY..LAUNCH_CLUTCH + 4, "vcu launch clutch"),
    (PEDAL_SWAP..PEDAL_SWAP + 4, "vcu pedal swap"),
    (GEAR_TIMER..SHIFT_COOLDOWN + 4, "vcu gear timer"),
];

impl VcuImage {
    /// The unit whose scratch is at `scratch`, its objects as its pointers give them.
    pub fn of(mem: &Mem, scratch: u32) -> Self {
        VcuImage { scratch, def: ScratchObj::def.load(mem, scratch), state: ScratchObj::state.load(mem, scratch) }
    }

    /// The unit's typed state, read from the image.
    pub fn lift(&self, mem: &Mem) -> Unit {
        let (s, st) = (self.scratch, self.state);
        Unit {
            scratch: Scratch {
                pedals: Pedals {
                    throttle: ScratchObj::throttle.load(mem, s),
                    brake: ScratchObj::brake.load(mem, s),
                    handbrake: ScratchObj::handbrake.load(mem, s),
                },
                fixed_clutch: ScratchObj::fixed_clutch.load(mem, s),
                carried_word: ScratchObj::carried_word.load(mem, s),
                launch_enabled: ScratchObj::launch_enabled.load(mem, s),
                launch_mode: ScratchObj::launch_mode.load(mem, s),
                vcu_enabled: ScratchObj::vcu_enabled.load(mem, s),
                slip_gate_rpm: ScratchObj::slip_gate_rpm.load(mem, s),
                slip_gate_speed: ScratchObj::slip_gate_speed.load(mem, s),
            },
            state: State {
                shift_lockout: mem.f32(st + SHIFT_LOCKOUT),
                launch_clutch: mem.f32(st + LAUNCH_CLUTCH),
                launch_clutch_dirty: mem.u32(st + LAUNCH_CLUTCH_DIRTY),
                pedal_swap: mem.u32(st + PEDAL_SWAP),
                gear_timer: mem.f32(st + GEAR_TIMER),
                shift_cooldown: mem.f32(st + SHIFT_COOLDOWN),
            },
        }
    }

    /// Writes the unit's typed state back: every owned word, nothing else.
    pub fn lower(&self, unit: &Unit, mem: &mut Mem) {
        let (s, st) = (self.scratch, self.state);
        let (scratch, state) = (&unit.scratch, &unit.state);
        ScratchObj::throttle.store(mem, s, scratch.pedals.throttle);
        ScratchObj::brake.store(mem, s, scratch.pedals.brake);
        ScratchObj::handbrake.store(mem, s, scratch.pedals.handbrake);
        ScratchObj::carried_word.store(mem, s, scratch.carried_word);
        ScratchObj::fixed_clutch.store(mem, s, scratch.fixed_clutch);
        ScratchObj::launch_enabled.store(mem, s, scratch.launch_enabled);
        ScratchObj::slip_gate_rpm.store(mem, s, scratch.slip_gate_rpm);
        ScratchObj::slip_gate_speed.store(mem, s, scratch.slip_gate_speed);
        ScratchObj::launch_mode.store(mem, s, scratch.launch_mode);
        ScratchObj::vcu_enabled.store(mem, s, scratch.vcu_enabled);
        mem.set_f32(st + SHIFT_LOCKOUT, state.shift_lockout);
        mem.set_u32(st + LAUNCH_CLUTCH_DIRTY, state.launch_clutch_dirty);
        mem.set_f32(st + LAUNCH_CLUTCH, state.launch_clutch);
        mem.set_u32(st + PEDAL_SWAP, state.pedal_swap);
        mem.set_f32(st + GEAR_TIMER, state.gear_timer);
        mem.set_f32(st + SHIFT_COOLDOWN, state.shift_cooldown);
    }

    /// The owned byte ranges with their names.
    pub fn owned(&self) -> impl Iterator<Item = (std::ops::Range<u32>, &'static str)> {
        let at =
            |base: u32| move |(r, name): (std::ops::Range<u32>, &'static str)| (base + r.start..base + r.end, name);
        OWNED_SCRATCH.into_iter().map(at(self.scratch)).chain(OWNED_STATE.into_iter().map(at(self.state)))
    }
}

/// A unit in typed storage: where it lives in the image and its state. The state is in a cell: the unit's entries call
/// each other through the host (FlagDispatch the slip query), so the views of one unit share it.
#[derive(Clone, Debug, PartialEq)]
pub struct CarVcu {
    pub image: VcuImage,
    pub unit: std::cell::Cell<Unit>,
}

impl CarVcu {
    /// The VCU of the car at `car`, lifted from the image.
    pub fn lift(mem: &Mem, car: u32) -> Self {
        Self::of(mem, crate::car_body::layout::scratch_at(car))
    }

    /// The unit whose scratch is at `scratch`, lifted from the image.
    pub fn of(mem: &Mem, scratch: u32) -> Self {
        let image = VcuImage::of(mem, scratch);
        CarVcu { image, unit: std::cell::Cell::new(image.lift(mem)) }
    }

    /// Writes the unit's state back to the image.
    pub fn lower(&self, mem: &mut Mem) {
        self.image.lower(&self.unit.get(), mem);
    }

    /// Whether this is the unit whose scratch is at `scratch`.
    pub fn is(&self, scratch: u32) -> bool {
        self.image.scratch == scratch
    }

    /// The unit as a store: its state here, its definition and pointers in `mem`; the drive's blocks `drive`'s where it
    /// is the unit's drive, else the image's.
    pub fn view<'a>(&'a self, mem: &'a mut Mem, drive: Option<&'a CarDrive>) -> VcuView<'a> {
        VcuView { mem, image: &self.image, unit: &self.unit, drive, defs: None }
    }

    /// Every owned byte range, named.
    pub fn owned(&self) -> Vec<(std::ops::Range<u32>, String)> {
        self.image.owned().map(|(r, name)| (r, name.to_string())).collect()
    }
}

/// Runs `run` on the unit whose scratch is at `scratch`, lifted into a store of its own and lowered after it: the
/// per-call path, on which the image holds the unit between calls.
pub fn with_vcu<T>(mem: &mut Mem, scratch: u32, run: impl FnOnce(&mut VcuView) -> T) -> T {
    let one = CarVcu::of(mem, scratch);
    let out = run(&mut one.view(mem, None));
    one.lower(mem);
    out
}

/// `lower(lift(image))` of the car's VCU into an empty image, compared with `mem` on every owned byte: their count, or
/// the first byte that differs.
pub fn round_trip(mem: &Mem, car: u32) -> Result<usize, String> {
    let vcu = CarVcu::lift(mem, car);
    let mut out = Mem::new();
    vcu.lower(&mut out);
    let mut bytes = 0;
    for (range, name) in vcu.owned() {
        let len = (range.end - range.start) as usize;
        let (want, got) = (mem.read(range.start, len), out.read(range.start, len));
        if let Some(k) = want.iter().zip(&got).position(|(a, b)| a != b) {
            return Err(format!("{name} at {:#010x}: +{k:#x} differs after lower(lift)", range.start));
        }
        bytes += len;
    }
    Ok(bytes)
}

/// A unit in typed storage as a [`VcuStore`]: the state in [`CarVcu`], the definition the step's typed definitions
/// hold ([`VcuView::with_defs`], else the image's), the pointers read from the image; and the drive's blocks
/// ([`DriveBlocks`]), the drive's typed state (held in `drive`, else per access on the image).
pub struct VcuView<'a> {
    mem: &'a mut Mem,
    image: &'a VcuImage,
    unit: &'a std::cell::Cell<Unit>,
    drive: Option<&'a CarDrive>,
    defs: Option<&'a Defs>,
}

impl<'a> VcuView<'a> {
    /// The view with the step's typed definitions (`None`: the definition on the image).
    pub fn with_defs(self, defs: Option<&'a Defs>) -> Self {
        VcuView { defs, ..self }
    }
}

impl VcuStore for VcuView<'_> {
    fn def(&self) -> Def {
        let at = self.image.def;
        match self.defs.and_then(|d| d.vcu(at)) {
            Some(held) => defs::checked(held.clone(), || def(self.mem, at)),
            None => def(self.mem, at),
        }
    }

    fn scratch(&self) -> Scratch {
        self.unit.get().scratch
    }

    fn store_scratch(&mut self, value: &Scratch) {
        self.unit.set(Unit { scratch: *value, ..self.unit.get() });
    }

    fn state(&self) -> State {
        self.unit.get().state
    }

    fn store_state(&mut self, value: &State) {
        self.unit.set(Unit { state: *value, ..self.unit.get() });
    }

    fn parts(&self) -> SimParts {
        let s = self.image.scratch;
        SimParts {
            engine: ScratchObj::engine.load(self.mem, s),
            drive: ScratchObj::drive.load(self.mem, s),
            lights: ScratchObj::lights.load(self.mem, s),
        }
    }

    fn car(&self) -> u32 {
        StateObj::owner.load(self.mem, self.image.state)
    }
}

impl VcuView<'_> {
    fn drive(&self, drive: u32) -> DriveRef<'_> {
        DriveRef::of(self.mem, self.drive, drive).with_defs(self.defs)
    }
}

impl DriveBlocks for VcuView<'_> {
    fn gearbox(&self, drive: u32) -> Gearbox {
        self.drive(drive).gearbox(self.mem)
    }

    fn store_gearbox(&mut self, drive: u32, value: &Gearbox) {
        DriveRef::of(self.mem, self.drive, drive).store_gearbox(self.mem, value)
    }

    fn gear_count(&self, drive: u32) -> u32 {
        self.drive(drive).gear_count(self.mem)
    }

    fn gear_ratios(&self, drive: u32) -> GearRatios {
        self.drive(drive).gear_ratios(self.mem)
    }

    fn outputs(&self, drive: u32) -> Outputs {
        self.drive(drive).outputs(self.mem)
    }

    fn store_outputs(&mut self, drive: u32, value: &Outputs) {
        DriveRef::of(self.mem, self.drive, drive).store_outputs(self.mem, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::store::{check_table, Object};

    const SCRATCH: u32 = 0x1000;
    const STATE: u32 = 0x2000;

    #[test]
    fn constants_are_the_originals() {
        assert_eq!(DEF_DEFAULTS.map(|(_, v)| v.to_bits()), [0x3F33_3333, 0x3E4C_CCCD, 0x4080_0000, 0x45F3_C000]);
    }

    #[test]
    fn tables_are_ordered_and_fit() {
        check_table::<ScratchObj>();
        check_table::<StateObj>();
    }

    /// The scratch and the state mapped with `pattern`-derived bytes, the scratch's pointers set.
    fn image(pattern: u8) -> Mem {
        let mut mem = Mem::new();
        let bytes = |n: u32, salt: u8| (0..n).map(|k| (k as u8).wrapping_mul(31) ^ salt ^ pattern).collect::<Vec<_>>();
        mem.map(SCRATCH, &bytes(ScratchObj::SIZE, 1));
        mem.map(STATE, &bytes(StateObj::SIZE, 2));
        ScratchObj::def.store(&mut mem, SCRATCH, 0x3000);
        ScratchObj::state.store(&mut mem, SCRATCH, STATE);
        mem
    }

    /// The unit's owned bytes as `lower` writes them (a NaN payload compares as its bits).
    fn bytes(unit: &Unit) -> Vec<u8> {
        let (image, mut mem) = (VcuImage { scratch: SCRATCH, def: 0, state: STATE }, Mem::new());
        image.lower(unit, &mut mem);
        image.owned().flat_map(|(r, _)| mem.read(r.start, (r.end - r.start) as usize)).collect()
    }

    #[test]
    fn lower_writes_the_lifted_owned_words_and_nothing_else() {
        let a = image(0);
        let image_a = VcuImage::of(&a, SCRATCH);
        assert_eq!(image_a, VcuImage { scratch: SCRATCH, def: 0x3000, state: STATE });
        let unit = image_a.lift(&a);
        let mut b = image(0x5a);
        image_a.lower(&unit, &mut b);
        let owned: Vec<_> = image_a.owned().map(|(r, _)| r).collect();
        assert_eq!(owned.iter().map(|r| r.end - r.start).sum::<u32>(), 16 * 4);
        for (base, len) in [(SCRATCH, ScratchObj::SIZE), (STATE, StateObj::SIZE)] {
            for at in base..base + len {
                let want = if owned.iter().any(|r| r.contains(&at)) { &a } else { &image(0x5a) };
                assert_eq!(b.u8(at), want.u8(at), "{at:#x}");
            }
        }
        assert_eq!(bytes(&VcuImage::of(&b, SCRATCH).lift(&b)), bytes(&unit));
    }

    #[test]
    fn the_per_call_store_lowers_what_a_sequence_stores() {
        let mut mem = image(0);
        let packet = Packet {
            pedals: Pedals { throttle: 0.25, brake: 0.5, handbrake: 1.0 },
            fixed_clutch: 0.75,
            carried_word: 7,
            flags: 0,
            value: 0,
        };
        let before = VcuImage::of(&mem, SCRATCH).lift(&mem);
        with_vcu(&mut mem, SCRATCH, |vcu| read_packet(vcu, packet));
        let after = VcuImage::of(&mem, SCRATCH).lift(&mem);
        let want = Scratch { pedals: packet.pedals, fixed_clutch: 0.75, carried_word: 7, ..before.scratch };
        assert_eq!(bytes(&after), bytes(&Unit { scratch: want, ..before }));
    }
}
