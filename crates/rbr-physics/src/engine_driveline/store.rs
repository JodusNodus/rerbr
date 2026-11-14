//! The engine's and the drive's stores (`crate::store`). [`DriveStore`] is one drive's typed state (its records) and
//! the words of its objects the original's index arithmetic reaches; `layout/drive_typed.rs` implements it on
//! `DriveView`, over typed storage or per access on the image. [`EngineStore`] is the engine's records and the
//! table and word readers; `Mem` implements it in `layout/mem_store.rs`.

use super::drive_line::DriveLinePoint;
use super::layout::{
    BlendKnot, CurveInput, CurveKnot, CurveState, Drive, DriveObjects, DriveState, DriveWorker, EngineDef,
    EngineObjects, EngineOptions, EngineOwn, EngineState, GearWork, LoadDef, RatioRing, UnitSplit,
};
use super::sim::{BlendRec, Bracket};
use crate::spring_damper::store::DamperBlocks;
use crate::spring_damper::{BlendTerms, Relaxation};
use crate::store::Raw;

/// Which of a drive's objects a word is in: the drive itself or its state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Drive,
    State,
}

/// One drive: its records, each load a view of its typed state and each store writing the words the original's
/// writes. The definition, the damage handler's block and the pointers are the image's (`Raw`).
pub trait DriveStore: Raw {
    /// The drive's address (the original's `this`).
    fn at(&self) -> u32;
    /// Its definition, state and damage block (the image's pointers).
    fn objects(&self) -> DriveObjects;
    fn lanes(&self) -> Drive;
    fn store_lanes(&mut self, value: &Drive);
    fn state(&self) -> DriveState;
    fn store_state(&mut self, value: &DriveState);
    /// Writes the commanded gear's lane alone.
    fn store_commanded_gear(&mut self, value: &DriveState);
    /// The worker: the definition, the state, the drive's own words and the damage block.
    fn worker(&self) -> DriveWorker;
    fn store_worker(&mut self, value: &DriveWorker);
    /// The words at the state's gear.
    fn gear_work(&self) -> GearWork;
    /// Writes the gear's scale and time step (at the gear `gear_work` read).
    fn store_gear_work(&mut self, value: &GearWork);
    fn ratio_ring(&self) -> RatioRing;
    fn store_ratio_ring(&mut self, value: &RatioRing);
    /// Wheel `wheel`'s curve state (`wheel < 4`).
    fn curve_state(&self, wheel: u32) -> CurveState;
    fn store_curve_state(&mut self, wheel: u32, value: &CurveState);
    /// The curve workers' input block (the brake torque and the handbrake torque).
    fn curve_input(&self) -> CurveInput;
    /// Wheel `wheel`'s latch bound (`wheel < 4`).
    fn latch_bound(&self, wheel: u32) -> f32;
    fn store_latch_bound(&mut self, wheel: u32, value: f32);
    /// The word at `offset` of the drive or its state, wherever the original's index arithmetic puts it (a typed
    /// state's word, or the image's).
    fn word(&self, part: Part, offset: u32) -> u32;
    fn set_word(&mut self, part: Part, offset: u32, value: u32);
}

pub trait EngineStore: DamperBlocks {
    fn engine_objects(&self, engine: u32) -> EngineObjects;
    fn load_load_def(&self, at: u32) -> LoadDef;
    fn load_engine_def(&self, at: u32) -> EngineDef;
    fn load_engine_own(&self, engine: u32) -> EngineOwn;
    fn store_engine_own(&mut self, engine: u32, value: &EngineOwn);
    fn load_engine_state(&self, at: u32) -> EngineState;
    /// Writes every field back (the ones a kernel did not change keep their bytes).
    fn store_engine_state(&mut self, at: u32, value: &EngineState);
    fn load_engine_options(&self, at: u32) -> EngineOptions;
    /// Writes the words the stall roll changes (the stall terms); the rest of the block keeps its bytes.
    fn store_stall_terms(&mut self, at: u32, value: &EngineOptions);
    /// Writes the record at `at` (the four words; the rest of the object is left alone).
    fn store_unit_split(&mut self, at: u32, value: &UnitSplit);
    /// The table manager's word after its header (named by use).
    fn table_manager_word(&self, manager: u32) -> u32;
    /// Entry `idx` of the table the manager points to.
    fn table_manager_entry(&self, manager: u32, idx: u32) -> u32;
    /// The drive line's stores: the entries, the count, and entries `n..10000` with zero position and tangent and distance -1. The
    /// tags of the filled entries are left as they were. The log lines are skipped; a count over 10000 traps in the
    /// original.
    fn store_drive_line(&mut self, this: u32, entries: &[DriveLinePoint]);
    /// Row `i` of the definition's exhaust table: eight floats.
    fn exhaust_row(&self, def: u32, i: u32) -> [f32; 8];
    /// The `n` knots of a curve at `at`.
    fn curve_knots(&self, at: u32, n: usize) -> Vec<CurveKnot>;
    /// The keys of a bracket table (the argument of `find_bracket`): each record's key.
    fn bracket_keys(&self, at: u32) -> Vec<f32>;
    /// The knots of a two-level blend table at `at`: each outer record has a key and a sub-table, which is read with its
    /// own count.
    fn blend_table(&self, at: u32) -> Vec<BlendKnot>;
    /// The four words of a `BlendTerms` block at `at`.
    fn load_blend_terms(&self, at: u32) -> BlendTerms;
    fn store_blend_terms(&mut self, at: u32, block: BlendTerms);
    /// The four words of a `Relaxation` block at `at`.
    fn load_relaxation(&self, at: u32) -> Relaxation;
    fn store_relaxation(&mut self, at: u32, block: Relaxation);
    /// The four outputs of `find_bracket`, written through the pointers the caller passed (`hi`, `lo` as ints).
    fn store_bracket(&mut self, b: &Bracket, out: [u32; 4]);
    /// `n` consecutive floats at `at`.
    fn f32_table(&self, at: u32, n: usize) -> Vec<f32>;
    /// The `count` records of a blend table (the argument of `blend_tables`), described at `desc`. The count is the first
    /// table's, and the second table is read with the same count.
    fn blend_records(&self, desc: u32, count: usize) -> Vec<BlendRec>;
    fn load_entry(&self, at: u32) -> DriveLinePoint;
    fn store_entry(&mut self, at: u32, e: DriveLinePoint);
}
