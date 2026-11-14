//! The engine's typed state on the image (`notes/physics.md` "Store seam"): its tables ([`EngineObj`], the `CEngine`'s
//! own words a step changes; [`EngineStateObj`], its state's), where they live ([`EngineImage`]) with their lift and
//! lower, the car's engine in typed storage ([`CarEngine`]), its store ([`EngineView`]) and the round trip.
//!
//! The engine's ports reach its words by the original's offsets from `this` and the state (the intake entries, the
//! damper blocks and the lanes are records several kernels read differently), so the view serves the store's words
//! (`Raw`): a word a held engine's table names is its typed state's, any other the image's. The records
//! (`EngineStore`) and the damper blocks are those words. Per call (no step holds the engine) the view is the image.
//! The definition, the pointers, the option block (the damage handler's engine block: its functors and NGP's engine
//! wrapper read and write it there) and the state's `EngineNoise` payload (`+0x2c..+0x5c`, read at its address by the
//! emit's listeners) stay in the image.

use std::cell::RefCell;
use std::ops::Range;

use super::drive_objects::{IntLane, Lane};
use crate::mem::Mem;
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

/// The engine's state pointer (`+8`).
const ENGINE_STATE_PTR: u32 = 0x8;

/// The engine's typed state: its own words and its state's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineTyped {
    pub own: EngineObj,
    pub state: EngineStateObj,
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

    /// The word at the aligned address `at` where `e` holds it.
    #[inline]
    fn word(&self, e: &EngineTyped, at: u32) -> Option<u32> {
        let (own, state) = (at.wrapping_sub(self.engine), at.wrapping_sub(self.state));
        if own < EngineObj::SIZE {
            e.own.word_of(own)
        } else if state < EngineStateObj::SIZE {
            e.state.word_of(state)
        } else {
            None
        }
    }

    /// Writes the word at the aligned address `at` where `e` holds it; false where it does not.
    #[inline]
    fn set_word(&self, e: &mut EngineTyped, at: u32, value: u32) -> bool {
        let (own, state) = (at.wrapping_sub(self.engine), at.wrapping_sub(self.state));
        if own < EngineObj::SIZE {
            e.own.set_word_of(own, value)
        } else if state < EngineStateObj::SIZE {
            e.state.set_word_of(state, value)
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
        EngineView { mem, held: Some((self.image, &self.typed)) }
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
/// tables name is its typed state's, every other the image's.
pub struct EngineView<'a> {
    mem: &'a mut Mem,
    held: Option<(EngineImage, &'a RefCell<EngineTyped>)>,
}

impl<'a> EngineView<'a> {
    /// The engine's store on the image (per call: no step holds it).
    pub fn image(mem: &'a mut Mem) -> Self {
        EngineView { mem, held: None }
    }

    /// The aligned word at `at`: the held engine's where it holds it, else the image's.
    #[inline]
    fn aligned(&self, at: u32) -> u32 {
        if let Some((image, typed)) = self.held {
            if let Some(w) = image.word(&typed.borrow(), at) {
                return w;
            }
        }
        self.mem.u32(at)
    }

    #[inline]
    fn set_aligned(&mut self, at: u32, value: u32) {
        if let Some((image, typed)) = self.held {
            if image.set_word(&mut typed.borrow_mut(), at, value) {
                return;
            }
        }
        self.mem.set_u32(at, value)
    }
}

impl Raw for EngineView<'_> {
    fn mem(&self) -> &Mem {
        self.mem
    }

    fn mem_mut(&mut self) -> &mut Mem {
        self.mem
    }

    #[inline]
    fn u8(&self, at: u32) -> u8 {
        self.aligned(at & !3).to_le_bytes()[(at & 3) as usize]
    }

    #[inline]
    fn u32(&self, at: u32) -> u32 {
        if at & 3 == 0 {
            self.aligned(at)
        } else {
            u32::from_le_bytes(std::array::from_fn(|i| self.u8(at.wrapping_add(i as u32))))
        }
    }

    #[inline]
    fn f32(&self, at: u32) -> f32 {
        f32::from_bits(self.u32(at))
    }

    #[inline]
    fn u32s<const N: usize>(&self, at: u32) -> [u32; N] {
        std::array::from_fn(|k| self.u32(at.wrapping_add(4 * k as u32)))
    }

    #[inline]
    fn f32s<const N: usize>(&self, at: u32) -> [f32; N] {
        std::array::from_fn(|k| self.f32(at.wrapping_add(4 * k as u32)))
    }

    #[inline]
    fn set_u8(&mut self, at: u32, value: u8) {
        let mut bytes = self.aligned(at & !3).to_le_bytes();
        bytes[(at & 3) as usize] = value;
        self.set_aligned(at & !3, u32::from_le_bytes(bytes))
    }

    #[inline]
    fn set_u32(&mut self, at: u32, value: u32) {
        if at & 3 == 0 {
            self.set_aligned(at, value)
        } else {
            for (i, byte) in value.to_le_bytes().into_iter().enumerate() {
                self.set_u8(at.wrapping_add(i as u32), byte);
            }
        }
    }

    #[inline]
    fn set_f32(&mut self, at: u32, value: f32) {
        self.set_u32(at, value.to_bits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_driveline::store::EngineStore;
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
                    assert!(img.word(&e, base + f.at + k).is_some(), "{} +{k:#x}", f.name);
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
