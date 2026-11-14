//! The drive state, definition and worker object the wheel and gear workers read and write (`E39`).

use super::drive_objects::{DriveState, IntLane, Lane};
use super::step::{DAMAGE_TORQUE_GAIN, DEF_MODE, DEF_RATIO_GAIN};
use crate::mem::Mem;
use crate::physics_core::layout::def_flags;
use crate::store::Value;

/// One axle record of the definition (`CDef`, 0x60 bytes each, four from `+0x0`): the direction word (`+0x18`), the two
/// block indices (`+0x28`, `+0x38`), the scale (`+0x48`) and the active index (`+0x68`). The names are ours.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Axle {
    pub direction: i32,
    pub block_pair: [u32; 2],
    pub scale: f32,
    pub active_index: u32,
}

/// One wheel record of the definition (0xf0 bytes each, four from `+0x148`): the weight (`+0x148`) and the pack kind
/// (`+0x158`, an index into the axles).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wheel {
    pub weight: f32,
    pub pack: u32,
}

/// The gear band of one gear (`CDef`, 0x50 bytes each from `+0x688`): the ratio (`+0x694`), and the lower and upper
/// edges (`+0x6c4`, `+0x6d4`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GearBand {
    pub ratio: f32,
    pub lower_edge: f32,
    pub upper_edge: f32,
}

impl GearBand {
    /// The band of `gear` in the definition at `def` (any value, read at the band's offset as the original does).
    pub fn load(mem: &Mem, def: u32, gear: u32) -> Self {
        let band = def.wrapping_add(0x688).wrapping_add(gear.wrapping_mul(0x50));
        GearBand { ratio: mem.f32(band + 0xC), lower_edge: mem.f32(band + 0x3C), upper_edge: mem.f32(band + 0x4C) }
    }
}

/// The definition's fields the group 6 workers read (`CDrive::CDef`, `this+4`). Only the names of the axle and wheel
/// records are ours; `torque_scale` is the axle 0 scale (`+0x48`) and is kept apart where the original reads the word
/// as a scalar.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WorkerDef {
    /// The mode (`+0x640`, the tweak `DriveType`): `Step1E0` runs `M96C0` when it is 3, else the wheel worker.
    pub mode: u32,
    /// The pack term (`+0x684`; names ours), the base of the pack gain.
    pub pack_term: f32,
    pub axles: [Axle; 4],
    pub wheels: [Wheel; 4],
    /// Set when the CDef flags have `0x80000`: the gear guard runs.
    pub gear_guard: bool,
    /// The scalar at `+0x48` (the axle 0 scale).
    pub torque_scale: f32,
    /// `+0x664` and `+0x674`: the two terms of the clutch torque's total (`ClutchTorque`).
    pub slip_gain_a: f32,
    pub slip_gain_b: f32,
    /// `+0xa54`: the scale of the drive speed in `GearSpeedError`.
    pub ratio_gain: f32,
    /// `+0xa74` and `+0xa84`: the slope and the reach of the gear trim's limit.
    pub slope_limit: f32,
    pub reach_limit: f32,
    /// `+0x630`: the gain `M5970` scales the lane by (names ours).
    pub lane_scale_gain: f32,
}

impl WorkerDef {
    /// The definition at `def` (the image's: a step does not write it).
    pub fn load(mem: &Mem, def: u32) -> Self {
        let f = |off: u32| mem.f32(def + off);
        WorkerDef {
            axles: std::array::from_fn(|k| {
                let at = def + 0x60 * k as u32;
                Axle {
                    direction: mem.u32(at + 0x18) as i32,
                    block_pair: [mem.u32(at + 0x28), mem.u32(at + 0x38)],
                    scale: mem.f32(at + 0x48),
                    active_index: mem.u32(at + 0x68),
                }
            }),
            wheels: std::array::from_fn(|k| {
                let at = def + 0xF0 * k as u32;
                Wheel { weight: mem.f32(at + 0x148), pack: mem.u32(at + 0x158) }
            }),
            gear_guard: def_flags(mem) & 0x80000 != 0,
            torque_scale: f(0x48),
            slip_gain_a: f(0x664),
            slip_gain_b: f(0x674),
            ratio_gain: f(DEF_RATIO_GAIN),
            slope_limit: f(0xA74),
            reach_limit: f(0xA84),
            lane_scale_gain: f(0x630),
            mode: mem.u32(def + DEF_MODE),
            pack_term: f(0x684),
        }
    }
}

/// One wheel term of the state (`0xc + 0x14 i`, `i < 4`): the value (`+0`), the term (`+4`), the sign (`+8`), the scale
/// (`+0xc`) and the speed (`+0x10`). Term 3's words are the first words of block 0's slip lane (`+0x48` to `+0x58`):
/// they are wheel 0's words in the typed state (`DriveTyped`), whose store writes term 3's sign and scale only where
/// they changed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelTerm {
    pub value: f32,
    pub term: f32,
    pub sign: i32,
    pub scale: f32,
    pub speed: f32,
}

impl Value for WheelTerm {
    const LEN: u32 = 0x14;
    fn load(mem: &Mem, at: u32) -> Self {
        WheelTerm {
            value: mem.f32(at),
            term: mem.f32(at + 4),
            sign: mem.u32(at + 8) as i32,
            scale: mem.f32(at + 0xC),
            speed: mem.f32(at + 0x10),
        }
    }

    fn store(self, mem: &mut Mem, at: u32) {
        mem.set_f32(at, self.value);
        mem.set_f32(at + 4, self.term);
        mem.set_u32(at + 8, self.sign as u32);
        mem.set_f32(at + 0xC, self.scale);
        mem.set_f32(at + 0x10, self.speed);
    }
}

/// One wheel block of the state (`200 i`, `i < 4`). `force` (`+0x88`) and `gate` (`+0x98`) are the lanes the wheels
/// work on; `slip` (`+0x48`) and the two `prev_` lanes hold the previous values. The `_aux` words are copied with their
/// lanes but are not read by the ported code. Block 0's `slip` shares its words with term 3 (see `WheelTerm`). The
/// names are ours.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelBlock {
    pub slip: Lane,
    pub prev_force: Lane,
    pub prev_force_aux: u32,
    pub prev_gate: IntLane,
    pub prev_gate_aux: u32,
    /// The force of the wheel (`+0x88`).
    pub force: Lane,
    pub force_aux: u32,
    /// The latch of the wheel (`+0x98`): 1 or -1 when latched, 0 otherwise.
    pub gate: IntLane,
    pub gate_aux: u32,
    /// The free wheel's inertia (`+0x84`; names ours): `E39_Step3870` steps `prev_force` by the torque over it.
    pub inertia: f32,
    /// The clutch load (`+0xb4`), read by `ClutchValues`.
    pub clutch_load: f32,
    /// The latch bound (`+0xf8`), read by `WheelSlipValue` and `WheelLatch`.
    pub latch_bound: f32,
}

/// The drive state the wheel workers read and write (`CDrive::CState`, `this+8`). `drive` is the shared part (the gear
/// and the engine speed). The `Vec` of values stays a fixed array: the state's offsets below cover the whole range the
/// workers touch. Names are ours where the meaning is not known.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkerState {
    pub drive: DriveState,
    /// The sign word of the clutch torque (`+0x368`).
    pub sign_word: i32,
    /// `+0x37c`, the drive gain of `WheelLatch` and `WheelSlipValue`.
    pub drive_gain: f32,
    /// `+0x380`, the drive gain negated: `M96C0` sets it, its pair leaves read it (names ours).
    pub drive_gain_neg: f32,
    /// `+0x460`, the pack gain of `ClutchValues` and `WheelLatch`.
    pub pack_gain: f32,
    /// The clutch matrix (`+0x470`, 16 floats, row-major). Its input to `E39_Step4990`.
    pub clutch_matrix: [f32; 16],
    /// The output of `E39_Step4990` (`+0x4b0`).
    pub step_matrix: [f32; 16],
    /// The vector of `E39_MatVec` (`+0x530`), one word per wheel from `WheelSlipValue`.
    pub step_vec: [f32; 4],
    /// The active flags, one per axle index (`+0x4f0`).
    pub lane_active: [u32; 4],
    /// The pair worker's 2x2 clutch matrix (`+0x500`, row-major, a row per wheel parity) and its inverse (`+0x510`),
    /// both set by `M7AA0` (names ours).
    pub pair_matrix: [f32; 4],
    pub pair_inverse: [f32; 4],
    /// The pair worker's active flags (`+0x520`, `+0x524`) and the two words `M7AA0` clears (`+0x528`, `+0x52c`).
    pub pair_active: [u32; 2],
    pub pair_words: [u32; 2],
    /// The gear history (`+0x544`, `+0x548`), and the word at `+0x568` that the shift copies.
    pub shift_from: [u32; 2],
    pub commanded_aux: u32,
    /// The gear-change flag (`+0x550`, the value at `+0x55c`).
    pub gear_flag: IntLane,
    /// The trim (`+0x560`) and the accumulator (`+0x574`), both f32.
    pub trim: f32,
    pub trim_accum: f32,
    /// The trigger (`+0x578`), set by each shift and gear change.
    pub trigger: u32,
    /// The random state of the gear logic (`+0x57c`).
    pub rng: u32,
    /// The car the gear change is for (`+0x580`).
    pub car: u32,
    /// The step counter (`+0x584`, the value at `+0x590`; names ours): `Step1E0` adds one each step.
    pub step_count: Lane,
    /// The reset counter (`+0x594`, the value at `+0x5a0`).
    pub reset_count: Lane,
    /// The count of the latches (`+0x5a4`).
    pub latch_count: u32,
    /// The count of the clutch state's changes (`+0x5a8`; names ours): `ngp::drive::clutch_state`
    /// adds one when it changes a block while the pending word is clear.
    pub clutch_change_count: u32,
    /// The count of the wheel worker's repeats (`+0x5ac`).
    pub repeat_count: u32,
    /// The count of the dirty bumps (`+0x5b0`).
    pub dirty_count: u32,
    /// The step count (`+0x5b4`; names ours): `Step1E0` adds one when the pending word is set.
    pub step_iter: u32,
    /// The branch word of the flags (`+0x5b8`).
    pub branch: u32,
    /// The pending word (`+0x5bc`), set by a latch and cleared by the reset.
    pub pending: u32,
    pub terms: [WheelTerm; 4],
    pub blocks: [WheelBlock; 4],
}

impl WorkerState {
    /// Sets block `i`'s slip. Block 0's slip is the words term 3 reads as its sign and scale (see `WheelTerm`), so a
    /// write to block 0 is also term 3's: its sign takes the slip's dirty flag, its scale the value.
    pub fn set_slip(&mut self, i: usize, value: f32) {
        self.blocks[i].slip.set(value);
        if i == 0 {
            self.terms[3].sign = self.blocks[0].slip.dirty as i32;
            self.terms[3].scale = value;
        }
    }
}

/// The wheel worker's words the original's callees read and write (`E39_Step4990`, `E39_MatVec`): the clutch matrix,
/// the step matrix and the step vector (state offsets), and the MatVec output (`this` offset). The host calls use them.
pub const CLUTCH_MATRIX_AT: u32 = 0x470;
pub const STEP_MATRIX_AT: u32 = 0x4B0;
pub const STEP_VEC_AT: u32 = 0x530;
pub const SOLUTION_AT: u32 = 0xA0;

/// The CDrive's own words the workers use (`this`): the value rate lane (`+0x84`), the
/// MatVec output (`+0xa0`), the pack outputs (`+0xc0`), the wheel offsets (`+0xf4`, 0x10 apart), the shift's speed and
/// trim words (`+0x1b4`, `+0x1b8`) and the gear level (`+0x1c4`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DriveOwn {
    /// The rate the driven term's value integrates at (`+0x84`, the original's `loc0`; names ours): term 0's in the
    /// wheel worker, term 1's in the pair worker, zero while the wheels latch.
    pub value_rate: Lane,
    /// The output of `E39_MatVec` (`+0xa0`, the original's `loc20`): the solved rate of each axle index.
    pub solution: [f32; 4],
    /// The lane at `+0x40` (`M5970` sets its scale).
    pub lane_scale: Lane,
    /// The lane at `+0x74` (`M5970` sets the scale times the definition's gain).
    pub lane_scaled: Lane,
    /// The gate word (`+0x50`): `M5970` copies it, `M6C90` reads it.
    pub lane_gate: u32,
    /// `+0x2c`, the gate level `M6C90` compares with 0.5 (names ours).
    pub gate_level: f32,
    /// `+0x1a0`, the scaled speed term `M6C90` returns (names ours).
    pub scaled_term: f32,
    /// `+0x1c8`, the speed factor `M6C90` writes (names ours).
    pub speed_factor: f32,
    pub pack_out: [f32; 4],
    /// The pair worker's two pack outputs (`+0xd0`), its solved pair (`+0xd8`) and its
    /// slip values (`+0xe0`), one per wheel parity (names ours).
    pub pair_pack: [f32; 2],
    pub pair_solution: [f32; 2],
    pub pair_slip: [f32; 2],
    pub wheel_offset: [f32; 4],
    pub shift_speed: f32,
    pub shift_trim: u32,
    pub gear_level: f32,
    /// The ring's mean, the result of `E39_RatioCA0` (`+0x19c`; names ours).
    pub ring_ratio: f32,
    /// The per-wheel bias added to each curve term (`+0x134`, every `0x10` bytes; names ours).
    pub wheel_bias: [f32; 4],
    /// The speed copy: the engine speed or the drive speed key, as bits (`+0x1a4`; names ours).
    pub speed_copy: u32,
    /// The magnitude of the clutch torque (`+0x1bc`; names ours).
    pub clutch_magnitude: f32,
}

/// The drive's damage block (`this+0x170`): the damage handler's block at `+0x3a18`,
/// which `E39_Link3AB0` links and `E39_StateInitC70` resets (rbr-recomp `notes/physics/damage.md` 1a,
/// `notes/sound.md` 6.4). The enable word per wheel (`+0x18`, 0xc apart; a broken driveshaft clears it), the rate
/// (`+0x74`), the enable (`+0x78`), the scale (`+0x88`) and the gear's scale (`+0x48`, four bytes per gear). The
/// field names are ours.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DriveDamage {
    /// The gate trip (`+0x70`; names ours): `Step1E0` clears it when the trip is taken.
    pub gate_trip: u32,
    /// The torque gain (`+0x84`; names ours): `Step1E0` scales the clutch torque by it.
    pub torque_gain: f32,
    pub wheel_enabled: [u32; 4],
    pub rate: f32,
    pub enable: u32,
    /// `+0x7c`, the ready word `M6C90` needs nonzero (names ours).
    pub ready: u32,
    /// `+0x8c`, the speed term `M6C90` updates (names ours).
    pub speed_term: f32,
    pub scale: f32,
}

impl DriveDamage {
    /// The block at `obj` (the damage handler's, on the image: its functors write it too).
    pub fn load(mem: &Mem, obj: u32) -> Self {
        DriveDamage {
            wheel_enabled: std::array::from_fn(|k| mem.u32(obj + 0x18 + 0xC * k as u32)),
            rate: mem.f32(obj + 0x74),
            enable: mem.u32(obj + 0x78),
            ready: mem.u32(obj + 0x7C),
            speed_term: mem.f32(obj + 0x8C),
            scale: mem.f32(obj + 0x88),
            gate_trip: mem.u32(obj + 0x70),
            torque_gain: mem.f32(obj + DAMAGE_TORQUE_GAIN),
        }
    }

    pub fn store(&self, mem: &mut Mem, obj: u32) {
        for (k, v) in self.wheel_enabled.iter().enumerate() {
            mem.set_u32(obj + 0x18 + 0xC * k as u32, *v);
        }
        mem.set_f32(obj + 0x74, self.rate);
        mem.set_u32(obj + 0x78, self.enable);
        mem.set_u32(obj + 0x7C, self.ready);
        mem.set_f32(obj + 0x8C, self.speed_term);
        mem.set_f32(obj + 0x88, self.scale);
        mem.set_u32(obj + 0x70, self.gate_trip);
        mem.set_f32(obj + DAMAGE_TORQUE_GAIN, self.torque_gain);
    }
}

/// The objects one drive worker call reads and writes: the definition (`this+4`), the state (`this+8`), the drive's own
/// words (`this`) and its damage block (`this+0x170`). The gear-indexed words are in `GearWork`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DriveWorker {
    pub def: WorkerDef,
    pub state: WorkerState,
    /// The drive's own words (`this`).
    pub own: DriveOwn,
    /// The drive's damage block (`this+0x170`).
    pub damage: DriveDamage,
}

/// The words the gear workers read at the state's gear: the gear's band (`CDef`, `+0x688`), the gear's scale (the object's
/// `+0x48` table) and the gear's time step (`this+0x174`). Only the gear workers load them, since the gear is an index
/// the other workers never read at. `DriveStore::store_gear_work` writes the scale and the time step after the worker:
/// the scale can share a word with the object's rate (at gear 11), and the rate is written first.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GearWork {
    pub gear: u32,
    pub band: GearBand,
    pub scale: f32,
    pub dt: f32,
}

/// The ring of `E39_RatioCA0` (0x503ca0): the count (`+0`, at most 8), the index of the last value written (`+4`) and the
/// eight values (`+8`). `Step1E0` keeps it at the state's `+0x388`. Names are ours.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RatioRing {
    pub count: u32,
    pub index: u32,
    pub values: [f32; 8],
}

impl Value for RatioRing {
    const LEN: u32 = 0x28;
    fn load(mem: &Mem, at: u32) -> Self {
        RatioRing { count: mem.u32(at), index: mem.u32(at + 4), values: mem.f32s(at + 8) }
    }
    fn store(self, mem: &mut Mem, at: u32) {
        mem.set_u32(at, self.count);
        mem.set_u32(at + 4, self.index);
        for (k, v) in self.values.iter().enumerate() {
            mem.set_f32(at + 8 + 4 * k as u32, *v);
        }
    }
}

/// The lane scale `M5970` takes (`x`, a struct on the stack): the scale (`+8`) and the gate word (`+0x1c`). Names are ours.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LaneScale {
    pub scale: f32,
    pub gate: u32,
}

impl LaneScale {
    /// The record at `at` (`M5970`'s argument, the caller's).
    pub fn load(mem: &Mem, at: u32) -> Self {
        LaneScale { scale: mem.f32(at + 8), gate: mem.u32(at + 0x1C) }
    }
}
