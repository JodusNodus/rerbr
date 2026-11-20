//! The engine's stores on the image (`super::super::store::{EngineStore, EngineTables}`): the bodies are the layout's
//! reads and writes at the records' addresses. The tables are on every word store, so the engine's view
//! (`super::engine_typed::EngineView`) reads its typed state where it holds the words; its records are its own.

use super::drive_line::{entries_held, entry_at, COUNT_AT};
use super::drive_objects::Lane;
use super::engine_definition::EXHAUST_ROWS;
use super::engine_objects::{
    EngineDef, EngineObjects, EngineOptions, EngineOwn, EngineState, LoadDef, ENGINE_DEF_PTR, ENGINE_OPTIONS_PTR,
    ENGINE_PEDAL, ENGINE_STATE_ABS_VALUE, ENGINE_STATE_ABS_VALUE_DIRTY, ENGINE_STATE_COMPLIANCE_DIRTY,
    ENGINE_STATE_COMPLIANCE_INVERSE, ENGINE_STATE_CRANK_ANGLE, ENGINE_STATE_CRANK_COUNT, ENGINE_STATE_CRANK_COUNT_PREV,
    ENGINE_STATE_ENGINE_SPEED, ENGINE_STATE_PTR, ENGINE_STATE_SCALED_VALUE, ENGINE_STATE_SCALED_VALUE_DIRTY,
    ENGINE_STATE_SMOOTHED, ENGINE_STATE_TORQUE_ACCUM,
};
use super::tables::{BlendKnot, CurveKnot, UnitSplit};
use crate::engine_driveline::drive_line::DriveLinePoint;
use crate::engine_driveline::sim::{BlendRec, Bracket};
use crate::engine_driveline::store::{EngineStore, EngineTables};
use crate::math::vec::Vec4;
use crate::mem::Mem;
use crate::store::Raw;

/// The image's records (the engine's view serves a held engine's from its typed state, `super::engine_typed`).
impl EngineStore for Mem {
    fn engine_objects(&self, engine: u32) -> EngineObjects {
        EngineObjects {
            def: self.u32(engine + ENGINE_DEF_PTR),
            state: self.u32(engine + ENGINE_STATE_PTR),
            options: self.u32(engine + ENGINE_OPTIONS_PTR),
        }
    }

    fn load_load_def(&self, at: u32) -> LoadDef {
        LoadDef::read(self, at)
    }

    fn load_engine_def(&self, at: u32) -> EngineDef {
        EngineDef::read(self, at)
    }

    fn load_engine_own(&self, engine: u32) -> EngineOwn {
        EngineOwn { pedal: Lane::load(self, engine + ENGINE_PEDAL), misfire_level: self.f32(engine + 0x194) }
    }

    fn store_engine_own(&mut self, engine: u32, value: &EngineOwn) {
        value.pedal.store(self, engine + ENGINE_PEDAL);
        self.set_f32(engine + 0x194, value.misfire_level);
    }

    fn load_engine_state(&self, at: u32) -> EngineState {
        let f = |off: u32| self.f32(at + off);
        EngineState {
            crank_angle: f(ENGINE_STATE_CRANK_ANGLE),
            crank_count: self.u32(at + ENGINE_STATE_CRANK_COUNT),
            crank_count_prev: self.u32(at + ENGINE_STATE_CRANK_COUNT_PREV),
            misfire_rate: f(0x10),
            misfire_time: f(0x14),
            misfire_count: f(0x18),
            misfire_out: f(0x3c),
            stage: [f(0xb8), f(0xf0), f(0x128), f(0x160)],
            extra_stage: f(0x80),
            compliance_dirty: self.u32(at + ENGINE_STATE_COMPLIANCE_DIRTY),
            compliance_inverse: f(ENGINE_STATE_COMPLIANCE_INVERSE),
            speed: f(ENGINE_STATE_ENGINE_SPEED),
            abs_value_dirty: self.u32(at + ENGINE_STATE_ABS_VALUE_DIRTY),
            abs_value: f(ENGINE_STATE_ABS_VALUE),
            scaled_value_dirty: self.u32(at + ENGINE_STATE_SCALED_VALUE_DIRTY),
            scaled_value: f(ENGINE_STATE_SCALED_VALUE),
            signed_rate: f(0x184),
            load_a: f(0xbc),
            load_b: f(0xf4),
            load_c: f(0x12c),
            load_d: f(0x164),
            threshold_value: f(0x23c),
            torque_accum: f(ENGINE_STATE_TORQUE_ACCUM),
            torque: f(0x270),
            slot_a: std::array::from_fn(|i| f(0x94 + 0x38 * i as u32)),
            slot_b: std::array::from_fn(|i| f(0xac + 0x38 * i as u32)),
            smoothed: Lane::load(self, at + ENGINE_STATE_SMOOTHED),
            solved: Lane::load(self, at + 0x1f4),
        }
    }

    /// Writes every field back (the ones a kernel did not change keep their bytes).
    fn store_engine_state(&mut self, at: u32, value: &EngineState) {
        self.set_f32(at + ENGINE_STATE_CRANK_ANGLE, value.crank_angle);
        self.set_u32(at + ENGINE_STATE_CRANK_COUNT, value.crank_count);
        self.set_u32(at + ENGINE_STATE_CRANK_COUNT_PREV, value.crank_count_prev);
        self.set_f32(at + 0x10, value.misfire_rate);
        self.set_f32(at + 0x14, value.misfire_time);
        self.set_f32(at + 0x18, value.misfire_count);
        self.set_f32(at + 0x3c, value.misfire_out);
        for (off, v) in [0xb8, 0xf0, 0x128, 0x160].into_iter().zip(value.stage) {
            self.set_f32(at + off, v);
        }
        self.set_f32(at + 0x80, value.extra_stage);
        self.set_u32(at + ENGINE_STATE_COMPLIANCE_DIRTY, value.compliance_dirty);
        self.set_f32(at + ENGINE_STATE_COMPLIANCE_INVERSE, value.compliance_inverse);
        self.set_f32(at + ENGINE_STATE_ENGINE_SPEED, value.speed);
        self.set_u32(at + ENGINE_STATE_ABS_VALUE_DIRTY, value.abs_value_dirty);
        self.set_f32(at + ENGINE_STATE_ABS_VALUE, value.abs_value);
        self.set_u32(at + ENGINE_STATE_SCALED_VALUE_DIRTY, value.scaled_value_dirty);
        self.set_f32(at + ENGINE_STATE_SCALED_VALUE, value.scaled_value);
        self.set_f32(at + 0x184, value.signed_rate);
        self.set_f32(at + 0xbc, value.load_a);
        self.set_f32(at + 0xf4, value.load_b);
        self.set_f32(at + 0x12c, value.load_c);
        self.set_f32(at + 0x164, value.load_d);
        self.set_f32(at + 0x23c, value.threshold_value);
        self.set_f32(at + ENGINE_STATE_TORQUE_ACCUM, value.torque_accum);
        self.set_f32(at + 0x270, value.torque);
        for i in 0..4 {
            self.set_f32(at + 0x94 + 0x38 * i as u32, value.slot_a[i]);
            self.set_f32(at + 0xac + 0x38 * i as u32, value.slot_b[i]);
        }
        value.smoothed.store(self, at + ENGINE_STATE_SMOOTHED);
        value.solved.store(self, at + 0x1f4);
    }

    fn load_engine_options(&self, at: u32) -> EngineOptions {
        let f = |off: u32| self.f32(at + off);
        EngineOptions {
            first_gate: f(0x58),
            second_gate: f(0x5c),
            stall_terms: [f(0x8), f(0x18), f(0x28), f(0x38)],
            stage_t: [f(0xc), f(0x1c), f(0x2c), f(0x3c)],
            fifth_t: f(0x70),
            rev_gate: self.u32(at + 0x74),
            load_terms: [f(0x14), f(0x24), f(0x34), f(0x44)],
            roll_gate: self.u32(at + 0x4),
        }
    }

    /// Writes the words `SimD870` changes (the stall terms); the rest of the block keeps its bytes.
    fn store_stall_terms(&mut self, at: u32, value: &EngineOptions) {
        for (i, v) in value.stall_terms.iter().enumerate() {
            self.set_f32(at + 0x8 + 0x10 * i as u32, *v);
        }
    }
}

impl<R: Raw> EngineTables for R {
    /// Writes the record at `at` (the four words; the rest of the object is left alone).
    fn store_unit_split(&mut self, at: u32, value: &UnitSplit) {
        self.set_f32(at + 0x08, value.at_zero);
        self.set_f32(at + 0x0c, value.w_low);
        self.set_f32(at + 0x10, value.w_mid);
        self.set_f32(at + 0x14, value.w_high);
    }

    /// `E39_MgrB` (0x501fa0, name ours): the word at `manager + 8`.
    fn table_manager_word(&self, manager: u32) -> u32 {
        self.u32(manager + 8)
    }

    /// `E39_MgrB` (0x501fb0, name ours): entry `idx` (20 bytes each) of the table the word at `manager + 0xc` points to.
    fn table_manager_entry(&self, manager: u32, idx: u32) -> u32 {
        self.u32(manager + 0xc).wrapping_add(idx.wrapping_mul(20))
    }

    /// `SetDriveLine`'s stores: the entries, the count, and the held entries past `n` (`entries_held`) with zero `q`
    /// and `v` and `s = -1`. The tags of the filled entries are left as they were. The log lines are skipped; a count
    /// over 10000 traps in the original, and is a longer line here (ours, `layout::drive_line`).
    fn store_drive_line(&mut self, this: u32, entries: &[DriveLinePoint]) {
        let n = entries.len() as u32;
        for (k, e) in (0..).zip(entries) {
            self.store_entry(entry_at(this, k), *e);
        }
        self.set_u32(this + COUNT_AT, n);
        for k in n..entries_held(n) {
            let at = entry_at(this, k);
            Vec4::default().store(self, at);
            Vec4::default().store(self, at + 0x10);
            self.set_f32(at + 0x20, -1.0);
        }
    }

    /// One row of the exhaust table (`SimEC40`): eight floats at `+0x1fc + 0x20 * i` of the definition.
    fn exhaust_row(&self, def: u32, i: u32) -> [f32; 8] {
        self.f32s(def + EXHAUST_ROWS + 0x20 * i)
    }

    /// The `n` knots of a curve at `at`.
    fn curve_knots(&self, at: u32, n: usize) -> Vec<CurveKnot> {
        (0..n as u32)
            .map(|k| CurveKnot { key: self.f32(at + 0x20 * k + 0x0c), value: self.f32(at + 0x20 * k + 0x1c) })
            .collect()
    }

    /// The knots of a bracket table (`E41_BlendW`'s argument): the count at `at`, the records' pointer at `at + 4`, and
    /// each record's key at +0 (0xc bytes apart).
    fn bracket_keys(&self, at: u32) -> Vec<f32> {
        let count = self.u32(at) as usize;
        let records = self.u32(at + 4);
        (0..count as u32).map(|k| self.f32(records + 0xc * k)).collect()
    }

    /// The knots of a two-level blend table at `at`: the outer count at +0 and the records' pointer at +4, each record 0xc
    /// bytes (key, sub-table count, sub-table pointer). Each sub-table is read with its own count.
    fn blend_table(&self, at: u32) -> Vec<BlendKnot> {
        let count = self.u32(at) as usize;
        let records = self.u32(at + 4);
        (0..count as u32)
            .map(|k| {
                let row = records + 0xC * k;
                BlendKnot { key: self.f32(row), records: self.blend_records(row, self.u32(row + 4) as usize) }
            })
            .collect()
    }

    /// The four outputs of `E41_BlendW`, written through the pointers the caller passed (`hi`, `lo` as ints).
    fn store_bracket(&mut self, b: &Bracket, out: [u32; 4]) {
        self.set_u32(out[0], b.hi as u32);
        self.set_u32(out[1], b.lo as u32);
        self.set_f32(out[2], b.w_upper);
        self.set_f32(out[3], b.w_lower);
    }

    /// `n` consecutive floats at `at`.
    fn f32_table(&self, at: u32, n: usize) -> Vec<f32> {
        (0..n as u32).map(|k| self.f32(at + 4 * k)).collect()
    }

    /// The `count` records of a blend table (`E41_BlendX`/`BlendY`'s argument `desc`): the table's pointer is at
    /// `desc + 8`.
    /// The count is the first table's (at its `+4`), and the second table is read with the same count.
    fn blend_records(&self, desc: u32, count: usize) -> Vec<BlendRec> {
        let base = self.u32(desc + 8);
        (0..count as u32)
            .map(|k| BlendRec {
                key: self.f32(base + 0xc * k),
                y1: self.f32(base + 0xc * k + 4),
                y2: self.f32(base + 0xc * k + 8),
            })
            .collect()
    }

    fn load_entry(&self, at: u32) -> DriveLinePoint {
        DriveLinePoint {
            position: Vec4::load(self, at),
            tangent: Vec4::load(self, at + 0x10),
            distance: self.f32(at + 0x20),
            tag: self.u32(at + 0x24),
        }
    }

    fn store_entry(&mut self, at: u32, e: DriveLinePoint) {
        e.position.store(self, at);
        e.tangent.store(self, at + 0x10);
        self.set_f32(at + 0x20, e.distance);
        self.set_u32(at + 0x24, e.tag);
    }
}
