//! The engine's typed state on the image (`notes/physics.md` "Store seam"): its tables ([`EngineObj`], the `CEngine`'s
//! own words a step changes; [`EngineStateObj`], its state's), where they live ([`EngineImage`]) with their lift and
//! lower, the car's engine in typed storage ([`CarEngine`]), its store ([`EngineView`]) and the round trip.
//!
//! The engine's ports reach its words by the original's offsets from `this` and the state (the intake entries, the
//! damper blocks and the lanes are records several kernels read differently), so the view serves the store's words
//! (`Raw`): a word a held engine's table names is its typed state's, any other the image's. The records
//! (`EngineStore`) and the damper blocks are those words. Per call (no step holds the engine) the view is the image.
//! A step's view also serves the engine's definition from the step's held definitions (`store::defs`, read only;
//! `EngineDefinition`). The pointers, the option block (the damage handler's engine block: its functors and NGP's
//! engine wrapper read and write it there) and the state's `EngineNoise` payload (`+0x2c..+0x5c`, read at its address
//! by the emit's listeners) stay in the image.

use std::cell::RefCell;
use std::ops::Range;

use super::drive_objects::{IntLane, Lane};
use super::engine_definition::EngineDefinition;
use super::engine_objects::{
    EngineDef, EngineObjects, EngineOptions, EngineOwn, EngineState, LoadDef, ENGINE_STATE_PTR,
};
use crate::engine_driveline::store::EngineStore;
use crate::mem::Mem;
use crate::store::defs::{self, Defs, Held};
use crate::store::{Object, Raw, Tabled};

crate::object! {
    /// `CEngine`'s own words a step changes (the car's `+0x690`). The pedal is the scratch's first `CReal` (`+0x14`),
    /// its cosine the second (`+0x24`, `SimEBB0`); `heat_total` the `CReal` at `+0x4c` (`Sim8610`). The heat rates are
    /// `Sim8890`'s outputs (the radiator's, the sim's accumulated, the engine's), `abs_term` `Sim8B20`'s, `signal` and
    /// `stall_sum` `Sim8CF0`'s, `heat_output` `Sim8890`'s clamped sum.
    pub struct EngineObj, size 0x19c {
        crank_delta @ 0x10: f32,
        pedal @ 0x1c: Lane,
        pedal_cosine @ 0x2c: Lane,
        heat_total @ 0x54: Lane,
        radiator_rate @ 0x64: f32,
        heat_rate @ 0x68: f32,
        engine_rate @ 0x6c: f32,
        abs_term @ 0x78: f32,
        signal @ 0x184: f32,
        stall_sum @ 0x190: f32,
        misfire_level @ 0x194: f32,
        heat_output @ 0x198: f32,
    }
}

crate::object! {
    /// `CEngine::CState`'s words a step changes, outside its `EngineNoise` payload (names by use in
    /// `EngineState`). `radiator` and `coolant` are `Sim8890`'s two weighted values (`+0x28` the coolant temperature),
    /// `boost` and `band` the turbo's two damper blocks (`+0x68` the boost pressure), `entries` the four intake entries
    /// (0x38 bytes each: `Sim8D90`'s key and value, `SimD6F0`'s split, `SimED70`'s curve, `Sim80D0`'s flow words). The
    /// lanes are `CReal`s' dirty and value words; `cycle` the step counter `Sim8E20` counts (an unsigned's).
    pub struct EngineStateObj, size 0x280 {
        misfire @ 0x10: [f32; 3],
        radiator @ 0x1c: [f32; 2],
        coolant @ 0x24: [f32; 2],
        turbo_speed @ 0x5c: f32,
        boost @ 0x60: [f32; 4],
        band @ 0x70: [f32; 4],
        extra_stage @ 0x80: f32,
        extra_share @ 0x84: f32,
        entries @ 0x88: [[u32; 14]; 4],
        cut_flags @ 0x168: [u32; 4],
        crank_angle @ 0x178: f32,
        crank_count @ 0x17c: u32,
        crank_count_prev @ 0x180: u32,
        signed_rate @ 0x184: f32,
        speed @ 0x188: f32,
        intake_temperature @ 0x1a4: Lane,
        intake_pressure @ 0x1b4: Lane,
        intake_flow @ 0x1d4: Lane,
        air_numerator @ 0x1dc: f32,
        entry_torque @ 0x1e0: f32,
        smoothed @ 0x1ec: Lane,
        solved @ 0x1fc: Lane,
        torque_average @ 0x20c: Lane,
        signed_average @ 0x21c: Lane,
        cycle @ 0x230: IntLane,
        torque_accum @ 0x238: f32,
        threshold_value @ 0x23c: f32,
        abs_value @ 0x248: Lane,
        scaled_value @ 0x258: Lane,
        compliance @ 0x268: Lane,
        torque @ 0x270: f32,
    }
}

/// The state's misfire output (`EngineState::misfire_out`), a word of the `EngineNoise` payload: on the image.
const STATE_MISFIRE_OUT: u32 = 0x3c;
/// The intake entries' words the records name: the stage (`+0x30`), the load (`+0x34`), the two slot words (`+0xc`,
/// `+0x24`).
const ENTRY_STAGE: usize = 0x30 / 4;
const ENTRY_LOAD: usize = 0x34 / 4;
const ENTRY_SLOT_A: usize = 0xc / 4;
const ENTRY_SLOT_B: usize = 0x24 / 4;

/// The engine's typed state: its own words and its state's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineTyped {
    pub own: EngineObj,
    pub state: EngineStateObj,
}

impl EngineTyped {
    /// The state's record (`EngineState`), `misfire_out` the image's.
    fn state_record(&self, misfire_out: f32) -> EngineState {
        let s = &self.state;
        let entry = |k: usize, w: usize| f32::from_bits(s.entries[k][w]);
        EngineState {
            crank_angle: s.crank_angle,
            crank_count: s.crank_count,
            crank_count_prev: s.crank_count_prev,
            misfire_rate: s.misfire[0],
            misfire_time: s.misfire[1],
            misfire_count: s.misfire[2],
            misfire_out,
            stage: std::array::from_fn(|k| entry(k, ENTRY_STAGE)),
            extra_stage: s.extra_stage,
            compliance_dirty: s.compliance.dirty,
            compliance_inverse: s.compliance.value,
            speed: s.speed,
            abs_value_dirty: s.abs_value.dirty,
            abs_value: s.abs_value.value,
            scaled_value_dirty: s.scaled_value.dirty,
            scaled_value: s.scaled_value.value,
            signed_rate: s.signed_rate,
            load_a: entry(0, ENTRY_LOAD),
            load_b: entry(1, ENTRY_LOAD),
            load_c: entry(2, ENTRY_LOAD),
            load_d: entry(3, ENTRY_LOAD),
            threshold_value: s.threshold_value,
            torque_accum: s.torque_accum,
            torque: s.torque,
            slot_a: std::array::from_fn(|k| entry(k, ENTRY_SLOT_A)),
            slot_b: std::array::from_fn(|k| entry(k, ENTRY_SLOT_B)),
            smoothed: s.smoothed,
            solved: s.solved,
        }
    }

    /// Writes the state's record back (`misfire_out` is the caller's, on the image).
    fn set_state_record(&mut self, v: &EngineState) {
        let s = &mut self.state;
        s.crank_angle = v.crank_angle;
        s.crank_count = v.crank_count;
        s.crank_count_prev = v.crank_count_prev;
        s.misfire = [v.misfire_rate, v.misfire_time, v.misfire_count];
        s.extra_stage = v.extra_stage;
        s.compliance = Lane { dirty: v.compliance_dirty, value: v.compliance_inverse };
        s.speed = v.speed;
        s.abs_value = Lane { dirty: v.abs_value_dirty, value: v.abs_value };
        s.scaled_value = Lane { dirty: v.scaled_value_dirty, value: v.scaled_value };
        s.signed_rate = v.signed_rate;
        s.threshold_value = v.threshold_value;
        s.torque_accum = v.torque_accum;
        s.torque = v.torque;
        for (k, entry) in s.entries.iter_mut().enumerate() {
            entry[ENTRY_STAGE] = v.stage[k].to_bits();
            entry[ENTRY_LOAD] = [v.load_a, v.load_b, v.load_c, v.load_d][k].to_bits();
            entry[ENTRY_SLOT_A] = v.slot_a[k].to_bits();
            entry[ENTRY_SLOT_B] = v.slot_b[k].to_bits();
        }
        s.smoothed = v.smoothed;
        s.solved = v.solved;
    }
}

/// Where an engine's typed state lives in the image: the engine and the state it points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineImage {
    pub engine: u32,
    pub state: u32,
}

impl EngineImage {
    /// The engine at `engine`, its state as its pointer gives it.
    pub fn of(mem: &Mem, engine: u32) -> Self {
        EngineImage { engine, state: mem.u32(engine + ENGINE_STATE_PTR) }
    }

    /// The engine's typed state, read from the image.
    pub fn lift(&self, mem: &Mem) -> EngineTyped {
        EngineTyped { own: EngineObj::lift(mem, self.engine), state: EngineStateObj::lift(mem, self.state) }
    }

    /// Writes the engine's typed state back: every owned word, nothing else.
    pub fn lower(&self, e: &EngineTyped, mem: &mut Mem) {
        e.own.lower(mem, self.engine);
        e.state.lower(mem, self.state);
    }

    /// The owned byte ranges with their names.
    pub fn owned(&self) -> impl Iterator<Item = (Range<u32>, String)> + '_ {
        let own = EngineObj::owned(self.engine).map(|r| (r, "engine".to_string()));
        own.chain(EngineStateObj::owned(self.state).map(|r| (r, "engine state".to_string())))
    }

    /// The engine's tables at their addresses, for the twin's compare.
    pub fn tabled(&self) -> Vec<Tabled> {
        vec![Tabled::of::<EngineObj>("engine", self.engine), Tabled::of::<EngineStateObj>("engine state", self.state)]
    }

    /// The word at the aligned address `at` where `e` holds it; `typed` is borrowed only for an address in its objects.
    #[inline(always)]
    fn word(&self, typed: &RefCell<EngineTyped>, at: u32) -> Option<u32> {
        let (own, state) = (at.wrapping_sub(self.engine), at.wrapping_sub(self.state));
        if own < EngineObj::SIZE {
            typed.borrow().own.word_of(own)
        } else if state < EngineStateObj::SIZE {
            typed.borrow().state.word_of(state)
        } else {
            None
        }
    }

    /// Writes the word at the aligned address `at` where `typed` holds it; false where it does not.
    #[inline(always)]
    fn set_word(&self, typed: &RefCell<EngineTyped>, at: u32, value: u32) -> bool {
        let (own, state) = (at.wrapping_sub(self.engine), at.wrapping_sub(self.state));
        if own < EngineObj::SIZE {
            typed.borrow_mut().own.set_word_of(own, value)
        } else if state < EngineStateObj::SIZE {
            typed.borrow_mut().state.set_word_of(state, value)
        } else {
            false
        }
    }
}

/// An engine in typed storage: where it lives in the image and its state. The state is in a cell borrowed by each word
/// access alone: the engine's step calls other engine entries through the host (`Sim80D0`), which run on the same state.
#[derive(Clone, Debug, PartialEq)]
pub struct CarEngine {
    pub image: EngineImage,
    pub typed: RefCell<EngineTyped>,
}

impl CarEngine {
    /// The engine of the car at `car`, lifted from the image.
    pub fn lift(mem: &Mem, car: u32) -> Self {
        Self::of(mem, crate::car_body::layout::engine_at(car))
    }

    /// The engine at `engine`, lifted from the image.
    pub fn of(mem: &Mem, engine: u32) -> Self {
        let image = EngineImage::of(mem, engine);
        CarEngine { image, typed: RefCell::new(image.lift(mem)) }
    }

    /// Writes the engine's state back to the image.
    pub fn lower(&self, mem: &mut Mem) {
        self.image.lower(&self.typed.borrow(), mem);
    }

    /// Whether this is the engine at `engine`.
    pub fn is(&self, engine: u32) -> bool {
        self.image.engine == engine
    }

    /// The engine as a store: its state here, the rest of the image in `mem`.
    pub fn view<'a>(&'a self, mem: &'a mut Mem) -> EngineView<'a> {
        EngineView { mem, held: Some((self.image, &self.typed)), def: None }
    }

    /// Every owned byte range, named.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        self.image.owned().collect()
    }
}

/// `lower(lift(image))` of the car's engine into an empty image, compared with `mem` on every owned byte: their count,
/// or the first byte that differs.
pub fn round_trip(mem: &Mem, car: u32) -> Result<usize, String> {
    let engine = CarEngine::lift(mem, car);
    let mut out = Mem::new();
    engine.lower(&mut out);
    let mut bytes = 0;
    for (range, name) in engine.owned() {
        let len = (range.end - range.start) as usize;
        let (want, got) = (mem.read(range.start, len), out.read(range.start, len));
        if let Some(k) = want.iter().zip(&got).position(|(a, b)| a != b) {
            return Err(format!("{name} at {:#010x}: +{k:#x} differs after lower(lift)", range.start));
        }
        bytes += len;
    }
    Ok(bytes)
}

/// The engine's stores (`EngineStore`, `EngineTables`, `DamperBlocks`) over a held engine's typed state and the image
/// ([`CarEngine::view`]), or over the image alone ([`EngineView::image`], per call): each word the held engine's
/// tables name is its typed state's, each word of a held definition ([`EngineView::with_defs`]) the definition's, every
/// other the image's.
pub struct EngineView<'a> {
    mem: &'a mut Mem,
    held: Option<(EngineImage, &'a RefCell<EngineTyped>)>,
    def: Option<&'a Held<EngineDefinition>>,
}

impl<'a> EngineView<'a> {
    /// The engine's store on the image (per call: no step holds it).
    pub fn image(mem: &'a mut Mem) -> Self {
        EngineView { mem, held: None, def: None }
    }

    /// The view with the engine definition `defs` holds read from it (read only: a write goes to the image).
    pub fn with_defs(self, defs: Option<&'a Defs>) -> Self {
        EngineView { def: defs.and_then(|d| d.engine.as_ref()), ..self }
    }

    /// The held engine's word at the aligned address `at`, where it holds it.
    #[inline(always)]
    fn engine_word(&self, at: u32) -> Option<u32> {
        let (image, typed) = self.held?;
        image.word(typed, at)
    }

    /// The held engine's or the held definition's word at the aligned address `at`, where one holds it.
    #[inline(always)]
    fn held_word(&self, at: u32) -> Option<u32> {
        if let Some(word) = self.engine_word(at) {
            return Some(word);
        }
        let held = self.def?;
        let word = held.def.word(at.wrapping_sub(held.at))?;
        Some(defs::checked(word, || self.mem.u32(at)))
    }

    /// Writes the held engine's word at the aligned address `at`; false where it does not hold it.
    #[inline(always)]
    fn set_held_word(&mut self, at: u32, value: u32) -> bool {
        self.held.is_some_and(|(image, typed)| image.set_word(typed, at, value))
    }
}

/// Each byte the held engine's tables name is its typed state's, every other byte the image's (an unaligned word is
/// its bytes).
impl Raw for EngineView<'_> {
    fn mem(&self) -> &Mem {
        self.mem
    }

    fn mem_mut(&mut self) -> &mut Mem {
        self.mem
    }

    #[inline(always)]
    fn u8(&self, at: u32) -> u8 {
        match self.held_word(at & !3) {
            Some(w) => w.to_le_bytes()[(at & 3) as usize],
            None => self.mem.u8(at),
        }
    }

    #[inline(always)]
    fn u32(&self, at: u32) -> u32 {
        if at & 3 == 0 {
            self.held_word(at).unwrap_or_else(|| self.mem.u32(at))
        } else if self.held.is_some() || self.def.is_some() {
            u32::from_le_bytes(std::array::from_fn(|i| self.u8(at.wrapping_add(i as u32))))
        } else {
            self.mem.u32(at)
        }
    }

    #[inline(always)]
    fn f32(&self, at: u32) -> f32 {
        f32::from_bits(self.u32(at))
    }

    #[inline(always)]
    fn u32s<const N: usize>(&self, at: u32) -> [u32; N] {
        std::array::from_fn(|k| self.u32(at.wrapping_add(4 * k as u32)))
    }

    #[inline(always)]
    fn f32s<const N: usize>(&self, at: u32) -> [f32; N] {
        std::array::from_fn(|k| self.f32(at.wrapping_add(4 * k as u32)))
    }

    #[inline(always)]
    fn set_u8(&mut self, at: u32, value: u8) {
        match self.engine_word(at & !3) {
            Some(w) => {
                let mut bytes = w.to_le_bytes();
                bytes[(at & 3) as usize] = value;
                self.set_held_word(at & !3, u32::from_le_bytes(bytes));
            }
            None => self.mem.set_u8(at, value),
        }
    }

    #[inline(always)]
    fn set_u32(&mut self, at: u32, value: u32) {
        if at & 3 == 0 {
            if !self.set_held_word(at, value) {
                self.mem.set_u32(at, value)
            }
        } else if self.held.is_some() {
            for (i, byte) in value.to_le_bytes().into_iter().enumerate() {
                self.set_u8(at.wrapping_add(i as u32), byte);
            }
        } else {
            self.mem.set_u32(at, value)
        }
    }

    #[inline(always)]
    fn set_f32(&mut self, at: u32, value: f32) {
        self.set_u32(at, value.to_bits())
    }
}

/// The engine's records: a held engine's from its typed state (the option block and `misfire_out` on the image), any
/// other the image's.
impl EngineStore for EngineView<'_> {
    fn engine_objects(&self, engine: u32) -> EngineObjects {
        self.mem.engine_objects(engine)
    }

    fn load_load_def(&self, at: u32) -> LoadDef {
        LoadDef::read(self, at)
    }

    fn load_engine_def(&self, at: u32) -> EngineDef {
        EngineDef::read(self, at)
    }

    fn load_engine_own(&self, engine: u32) -> EngineOwn {
        match self.held.filter(|(image, _)| image.engine == engine) {
            Some((_, typed)) => {
                let own = &typed.borrow().own;
                EngineOwn { pedal: own.pedal, misfire_level: own.misfire_level }
            }
            None => self.mem.load_engine_own(engine),
        }
    }

    fn store_engine_own(&mut self, engine: u32, value: &EngineOwn) {
        match self.held.filter(|(image, _)| image.engine == engine) {
            Some((_, typed)) => {
                let own = &mut typed.borrow_mut().own;
                own.pedal = value.pedal;
                own.misfire_level = value.misfire_level;
            }
            None => self.mem.store_engine_own(engine, value),
        }
    }

    fn load_engine_state(&self, at: u32) -> EngineState {
        match self.held.filter(|(image, _)| image.state == at) {
            Some((_, typed)) => typed.borrow().state_record(self.mem.f32(at + STATE_MISFIRE_OUT)),
            None => self.mem.load_engine_state(at),
        }
    }

    fn store_engine_state(&mut self, at: u32, value: &EngineState) {
        match self.held.filter(|(image, _)| image.state == at) {
            Some((_, typed)) => {
                typed.borrow_mut().set_state_record(value);
                self.mem.set_f32(at + STATE_MISFIRE_OUT, value.misfire_out);
            }
            None => self.mem.store_engine_state(at, value),
        }
    }

    fn load_engine_options(&self, at: u32) -> EngineOptions {
        self.mem.load_engine_options(at)
    }

    fn store_stall_terms(&mut self, at: u32, value: &EngineOptions) {
        self.mem.store_stall_terms(at, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::check_table;

    const ENGINE: u32 = 0x1000;
    const STATE: u32 = 0x4000;

    #[test]
    fn tables_are_ordered_and_fit_and_hold_whole_words() {
        check_table::<EngineObj>();
        check_table::<EngineStateObj>();
        let e = EngineTyped { own: EngineObj::lift(&image(0), ENGINE), state: EngineStateObj::lift(&image(0), STATE) };
        for (base, fields) in [(ENGINE, EngineObj::FIELDS), (STATE, EngineStateObj::FIELDS)] {
            for f in fields {
                assert!(f.at % 4 == 0 && f.len % 4 == 0, "{}", f.name);
                let img = EngineImage { engine: ENGINE, state: STATE };
                for k in (0..f.len).step_by(4) {
                    assert!(img.word(&RefCell::new(e), base + f.at + k).is_some(), "{} +{k:#x}", f.name);
                }
            }
        }
        // The pointers, the payload and the graphs are not owned.
        for off in [0x4, 0x8, 0x44, 0x48] {
            assert!(EngineObj::field_at(off).is_none(), "engine {off:#x}");
        }
        for off in (0x2c..0x5c).step_by(4).chain([0xc, 0x274, 0x278]) {
            assert!(EngineStateObj::field_at(off).is_none(), "state {off:#x}");
        }
    }

    /// The engine and its state mapped with `pattern`-derived bytes, the state pointer set.
    fn image(pattern: u8) -> Mem {
        let mut mem = Mem::new();
        let bytes = |n: u32, salt: u8| (0..n).map(|k| (k as u8).wrapping_mul(29) ^ salt ^ pattern).collect::<Vec<_>>();
        mem.map(ENGINE, &bytes(0x200, 1));
        mem.map(STATE, &bytes(0x280, 2));
        mem.set_u32(ENGINE + ENGINE_STATE_PTR, STATE);
        mem
    }

    #[test]
    fn lower_writes_the_lifted_owned_words_and_nothing_else() {
        let a = image(0);
        let image_a = EngineImage::of(&a, ENGINE);
        let e = image_a.lift(&a);
        let mut b = image(0x5a);
        image_a.lower(&e, &mut b);
        let owned: Vec<_> = image_a.owned().map(|(r, _)| r).collect();
        for (base, len) in [(ENGINE, 0x200), (STATE, 0x280)] {
            for at in base..base + len {
                let want = if owned.iter().any(|r| r.contains(&at)) { &a } else { &b };
                assert_eq!(b.u8(at), want.u8(at), "{at:#x}");
            }
        }
        assert_eq!(EngineImage::of(&b, ENGINE).lift(&b), e);
    }

    #[test]
    fn the_held_view_reads_and_writes_as_the_image_does() {
        let mut mem = image(0);
        let engine = CarEngine::of(&mem, ENGINE);
        let mut view = engine.view(&mut mem);
        // Every word and byte of both objects, aligned or not, reads as the image's and writes through.
        for (base, len) in [(ENGINE, 0x200), (STATE, 0x280)] {
            for at in base..base + len {
                assert_eq!(view.u8(at), image(0).u8(at), "{at:#x}");
                if at + 4 <= base + len {
                    assert_eq!(view.u32(at), image(0).u32(at), "{at:#x}");
                }
            }
        }
        let words: Vec<u32> = (ENGINE..ENGINE + 0x1fc).step_by(4).chain((STATE..STATE + 0x27c).step_by(4)).collect();
        for &at in &words {
            if at != ENGINE + ENGINE_STATE_PTR {
                view.set_u32(at, at ^ 0xdead_beef);
            }
        }
        view.set_u32(STATE + 0x189, 0x1122_3344);
        view.set_u8(ENGINE + 0x1d, 0x77);
        let state = view.load_engine_state(STATE);
        let own = view.load_engine_own(ENGINE);
        engine.lower(&mut mem);
        let mut want = image(0);
        for &at in &words {
            if at != ENGINE + ENGINE_STATE_PTR {
                want.set_u32(at, at ^ 0xdead_beef);
            }
        }
        want.set_u32(STATE + 0x189, 0x1122_3344);
        want.set_u8(ENGINE + 0x1d, 0x77);
        assert_eq!(mem.read(ENGINE, 0x200), want.read(ENGINE, 0x200));
        assert_eq!(mem.read(STATE, 0x280), want.read(STATE, 0x280));
        // The records read through the view are the image's (compared as the bytes they store: NaN patterns).
        let mut stored = want.clone();
        state.store(&mut stored, STATE);
        own.store(&mut stored, ENGINE);
        assert_eq!(stored.read(ENGINE, 0x200), want.read(ENGINE, 0x200));
        assert_eq!(stored.read(STATE, 0x280), want.read(STATE, 0x280));
    }
}
