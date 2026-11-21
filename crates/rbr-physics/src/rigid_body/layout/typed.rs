//! The car's solids in typed storage (`notes/physics.md` "Store seam"): the `CSolid` words a step writes ([`Solid`]:
//! pose, matrix, velocities, the window's and the integrator's vectors, the accumulators, the sleep words), for the car,
//! its wheels and its hinged and mounted bodies ([`CarSolids`], keyed by the solid's address), and their store
//! ([`SolidView`]: a word a held solid's table names is its typed state's, any other the image's). The pointers, the
//! flags byte, the NTweak headers and dirty words (`SetDirty`), the scale `+0xd0`, the state's diagonal and gate and
//! the scratch block's other words stay in the image. The step holds them (`Typed::lift_with_solids`); their
//! readers at their addresses in other subsystems run on `SolidWords` stores (notes).

use std::cell::Cell;
use std::ops::Range;

use super::mem_store::SolidWords;
use crate::car_body::layout::{hinged_slot, mounted_slot, wheel_list_at};
use crate::entities::layout::Hinged;
use crate::mem::Mem;
use crate::physics_core::layout::mounted_body::Mounted;
use crate::store::{Field, FieldInfo, Object, Raw, Value};

crate::object! {
    /// The words of a `CSolid` a step writes (names by use, `Body`'s; offsets the solid's). The quaternions and vectors
    /// are four words each; `matrix` is the state's matrix (`+0xe0 + 0x30`) and the word after it.
    pub struct Solid, size 0x3d0 {
        orientation @ 0x100: [f32; 4],
        matrix @ 0x110: [f32; 10],
        position @ 0x140: [f32; 4],
        sampled @ 0x17c: f32,
        angular_velocity @ 0x190: [f32; 4],
        linear_velocity @ 0x1c0: [f32; 4],
        is_active @ 0x1dc: u32,
        very_soft @ 0x1ec: u32,
        prev_angular_velocity @ 0x200: [f32; 4],
        prev_linear_velocity @ 0x220: [f32; 4],
        accel_linear_est @ 0x230: [f32; 4],
        accel_angular_est @ 0x240: [f32; 4],
        accel_linear_filt @ 0x250: [f32; 4],
        accel_angular_filt @ 0x260: [f32; 4],
        sleep_timer @ 0x270: f32,
        sleep_armed @ 0x274: u32,
        progress @ 0x290: f32,
        start_orientation @ 0x2a0: [f32; 4],
        start_angular_velocity @ 0x2e0: [f32; 4],
        scratch @ 0x310: [f32; 4],
        angular_accel @ 0x320: [f32; 4],
        start_position @ 0x330: [f32; 4],
        start_linear_velocity @ 0x340: [f32; 4],
        force @ 0x370: [f32; 4],
        linear_accel @ 0x380: [f32; 4],
        step_flags @ 0x3b4: u32,
    }
}

/// The span of a solid its table covers: a word outside it is never typed.
const HELD: Range<u32> = 0x100..0x3b8;

/// The words of the held span.
const WORDS: usize = ((HELD.end - HELD.start) / 4) as usize;

/// For each word of the held span, whether [`Solid`]'s table names it (a word it does not is the image's).
const NAMED: [bool; WORDS] = {
    const ALL: [(u8, u16); (Solid::SIZE / 4) as usize] = crate::store::word_fields(Solid::FIELDS);
    let mut out = [false; WORDS];
    let mut k = 0;
    while k < WORDS {
        out[k] = ALL[k + (HELD.start / 4) as usize].0 != u8::MAX;
        k += 1;
    }
    out
};

/// The runs of named words in the held span (word indices `from..to`), and their count.
const RUNS_MAX: usize = 32;
const RUNS_AND_LEN: ([(u8, u8); RUNS_MAX], usize) = {
    let (mut runs, mut n, mut k) = ([(0, 0); RUNS_MAX], 0, 0);
    while k < WORDS {
        if NAMED[k] && (k == 0 || !NAMED[k - 1]) {
            runs[n].0 = k as u8;
        }
        if NAMED[k] && (k + 1 == WORDS || !NAMED[k + 1]) {
            runs[n].1 = (k + 1) as u8;
            n += 1;
        }
        k += 1;
    }
    (runs, n)
};
const RUNS: [(u8, u8); RUNS_MAX] = RUNS_AND_LEN.0;
const RUNS_LEN: usize = RUNS_AND_LEN.1;

/// The index in the held span of the word at byte `off` of a solid, where [`Solid`]'s table names it.
#[inline]
fn slot(off: u32) -> Option<usize> {
    let k = off.wrapping_sub(HELD.start);
    let i = (k / 4) as usize;
    (k.is_multiple_of(4) && i < WORDS && NAMED[i]).then_some(i)
}

/// The words of a body's own past its solid's that its table names (a hinged body's [`Hinged`], a mounted body's
/// [`Mounted`]), at most [`OWN_WORDS`] from the first: the object's size, its fields and, for each word from the first,
/// whether a field names it.
#[derive(Debug)]
pub struct OwnTable {
    pub size: u32,
    pub fields: &'static [FieldInfo],
    start: u32,
    named: [bool; OWN_WORDS],
}

/// The words an own table spans at most.
const OWN_WORDS: usize = 0x90;

const fn own_table(size: u32, fields: &'static [FieldInfo]) -> OwnTable {
    let start = fields[0].at;
    assert!(start >= Solid::SIZE, "a body's own words follow its solid's");
    let mut named = [false; OWN_WORDS];
    let mut f = 0;
    while f < fields.len() {
        let (at, len) = (fields[f].at, fields[f].len);
        assert!(at % 4 == 0 && len % 4 == 0 && at + len <= size, "own fields are whole words in the object");
        let mut w = (at - start) / 4;
        while w < (at - start + len) / 4 {
            named[w as usize] = true;
            w += 1;
        }
        f += 1;
    }
    OwnTable { size, fields, start, named }
}

/// The hinged and the mounted body's own tables.
pub static HINGED_OWN: OwnTable = own_table(Hinged::SIZE, Hinged::FIELDS);
pub static MOUNTED_OWN: OwnTable = own_table(Mounted::SIZE, Mounted::FIELDS);

impl OwnTable {
    /// The index of the word at byte `off` of the body, where the table names it.
    #[inline]
    fn slot(&self, off: u32) -> Option<usize> {
        let k = off.wrapping_sub(self.start);
        let i = (k / 4) as usize;
        (k.is_multiple_of(4) && i < OWN_WORDS && self.named[i]).then_some(i)
    }

    /// The runs of named words (word indices `from..to`).
    fn runs(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        let mut k = 0;
        std::iter::from_fn(move || {
            let from = (k..OWN_WORDS).find(|&i| self.named[i])?;
            let to = (from..OWN_WORDS).find(|&i| !self.named[i]).unwrap_or(OWN_WORDS);
            k = to;
            Some(from..to)
        })
    }
}

/// A body's own words in typed storage, as [`HeldSolid`]'s: the state and the image's copy (named words only).
#[derive(Clone, Debug)]
struct HeldOwn {
    table: &'static OwnTable,
    words: Cell<[u32; OWN_WORDS]>,
    lifted: Cell<[u32; OWN_WORDS]>,
}

impl HeldOwn {
    /// The named words of the body at `at`, each read from the image (the others are not the body's to read).
    fn lift(mem: &Mem, at: u32, table: &'static OwnTable) -> Self {
        let mut words = [0; OWN_WORDS];
        for run in table.runs() {
            for k in run {
                words[k] = mem.u32(at + table.start + 4 * k as u32);
            }
        }
        HeldOwn { table, words: Cell::new(words), lifted: Cell::new(words) }
    }

    /// As [`HeldSolid::lower`].
    fn lower(&self, mem: &mut Mem, at: u32, changed: bool) {
        let (words, image) = (self.words.get(), self.lifted.get());
        if changed && words == image {
            return;
        }
        for run in self.table.runs() {
            if !changed || words[run.clone()] != image[run.clone()] {
                mem.set_u32s(at + self.table.start + 4 * run.start as u32, &words[run]);
            }
        }
        self.lifted.set(words);
    }

    #[inline]
    fn cell(&self, off: u32) -> Option<&Cell<u32>> {
        self.table.slot(off).map(|i| &self.words.as_array_of_cells()[i])
    }
}

/// A solid in typed storage: its address, its name and its state, [`Solid`]'s words over the held span (one cell a
/// word, so a nested run reads and writes the same words; a word the table does not name is read and never used), and
/// the span as the image holds it (as lifted, then as last lowered: the state stays held across steps). A hinged or a
/// mounted body holds its own words too ([`OwnTable`]); `size` is its object's (a solid's, else its own table's).
#[derive(Clone, Debug)]
pub struct HeldSolid {
    pub at: u32,
    pub name: &'static str,
    size: u32,
    words: Cell<[u32; WORDS]>,
    lifted: Cell<[u32; WORDS]>,
    own: Option<HeldOwn>,
}

impl PartialEq for HeldSolid {
    fn eq(&self, other: &Self) -> bool {
        let own = |s: &Self| s.own.as_ref().map(|o| o.words.get());
        (self.at, self.name, self.words.get(), own(self)) == (other.at, other.name, other.words.get(), own(other))
    }
}

impl HeldSolid {
    /// The solid at `at`, its span read at once, and its own words where `own` tables them.
    fn lift(mem: &Mem, at: u32, name: &'static str, own: Option<&'static OwnTable>) -> Self {
        let lifted = mem.u32s(at + HELD.start);
        let size = own.map_or(Solid::SIZE, |t| t.size);
        let own = own.map(|t| HeldOwn::lift(mem, at, t));
        HeldSolid { at, name, size, words: Cell::new(lifted), lifted: Cell::new(lifted), own }
    }

    /// The fields of the body's own words its table names (none for a solid alone).
    pub fn own_fields(&self) -> &'static [FieldInfo] {
        self.own.as_ref().map_or(&[], |o| o.table.fields)
    }

    /// The object's size: the bytes from `at` whose words this solid resolves.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// Writes back the named words, each run of them at once; with `changed`, only the runs that differ from the
    /// image's copy (`lifted`): the image still holds the others, since nothing writes a held word on it while the solid
    /// is held (the `poison` check). The copy is then what was written.
    fn lower(&self, mem: &mut Mem, changed: bool) {
        if let Some(own) = &self.own {
            own.lower(mem, self.at, changed);
        }
        let (words, image) = (self.words.get(), self.lifted.get());
        if changed && words == image {
            return;
        }
        for &(from, to) in &RUNS[..RUNS_LEN] {
            let run = from as usize..to as usize;
            if !changed || words[run.clone()] != image[run.clone()] {
                mem.set_u32s(self.at + HELD.start + 4 * from as u32, &words[run]);
            }
        }
        self.lifted.set(words);
    }

    #[inline]
    fn cells(&self) -> &[Cell<u32>; WORDS] {
        self.words.as_array_of_cells()
    }

    /// The word at byte `off` of the solid, where its table (or its own) names it.
    #[inline]
    pub fn word(&self, off: u32) -> Option<u32> {
        match slot(off) {
            Some(i) => Some(self.cells()[i].get()),
            None => self.own.as_ref()?.cell(off).map(Cell::get),
        }
    }

    /// Stores the word at byte `off` of the solid; false where its table (or its own) does not name it.
    #[inline]
    pub fn set_word(&self, off: u32, value: u32) -> bool {
        match slot(off) {
            Some(i) => {
                self.cells()[i].set(value);
                true
            }
            None => self.own.as_ref().and_then(|o| o.cell(off)).map(|c| c.set(value)).is_some(),
        }
    }

    /// The field `f` of [`Solid`].
    #[inline]
    pub fn get<T: Value + Default>(&self, f: Field<T>) -> T {
        let mut value = T::default();
        let first = ((f.at - HELD.start) / 4) as usize;
        for k in 0..T::LEN / 4 {
            value.set_word(k, self.cells()[first + k as usize].get());
        }
        value
    }

    /// Stores the field `f` of [`Solid`].
    #[inline]
    pub fn set<T: Value>(&self, f: Field<T>, value: T) {
        let first = ((f.at - HELD.start) / 4) as usize;
        for k in 0..T::LEN / 4 {
            self.cells()[first + k as usize].set(value.word(k).expect("a field of whole words"));
        }
    }
}

/// The car's solids: the car's own, each wheel's, each hinged and each mounted body's (the car's slots that are set,
/// each address once).
#[derive(Clone, Debug, Default)]
pub struct CarSolids {
    pub solids: Vec<HeldSolid>,
    /// From the lowest held span's start to the highest one's end: most words a store asks for (the stack's, the
    /// globals') are outside it.
    bounds: Range<u32>,
    /// The solid the last word found was in: runs read and write one solid's words in a row.
    last: Cell<usize>,
    /// The gap between two solids the last word not found was in (another object's words, read in a row).
    gap: Cell<(u32, u32)>,
}

impl PartialEq for CarSolids {
    fn eq(&self, other: &Self) -> bool {
        self.solids == other.solids
    }
}

impl CarSolids {
    /// The solids of the car at `car`, lifted from the image.
    pub fn lift(mem: &Mem, car: u32) -> Self {
        type Found = (u32, &'static str, Option<&'static OwnTable>);
        let mut found: Vec<Found> = Vec::with_capacity(12);
        let mut add = |name: &'static str, at: u32, own: Option<&'static OwnTable>| {
            if at != 0 && found.iter().all(|&(a, ..)| a != at) {
                found.push((at, name, own));
            }
        };
        add("car solid", car, None);
        for k in 0..4 {
            add(
                ["wheel0 solid", "wheel1 solid", "wheel2 solid", "wheel3 solid"][k],
                mem.u32(wheel_list_at(car, k as u32)),
                None,
            );
            add(
                ["hinged0 solid", "hinged1 solid", "hinged2 solid", "hinged3 solid"][k],
                mem.u32(hinged_slot(car, k as u32)),
                Some(&HINGED_OWN),
            );
        }
        for k in 0..3 {
            add(
                ["mounted0 solid", "mounted1 solid", "mounted2 solid"][k],
                mem.u32(mounted_slot(car, k as u32)),
                Some(&MOUNTED_OWN),
            );
        }
        found.sort_unstable_by_key(|&(at, ..)| at);
        let solids: Vec<HeldSolid> =
            found.into_iter().map(|(at, name, own)| HeldSolid::lift(mem, at, name, own)).collect();
        assert!(solids.windows(2).all(|w| w[1].at - w[0].at >= w[0].size), "two solids overlap");
        let bounds = match (solids.first(), solids.last()) {
            (Some(lo), Some(hi)) => lo.at + HELD.start..hi.at + hi.size,
            _ => 0..0,
        };
        CarSolids { solids, bounds, last: Cell::new(0), gap: Cell::new((0, 0)) }
    }

    /// Writes every solid's state back to the image it was lifted from: the words that changed.
    pub fn lower(&self, mem: &mut Mem) {
        for s in &self.solids {
            s.lower(mem, true);
        }
    }

    /// Every owned byte range, named by its solid and field.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        let field = |s: &HeldSolid| {
            let (at, name) = (s.at, s.name);
            let fields = Solid::FIELDS.iter().chain(s.own_fields());
            fields.map(move |f| (at + f.at..at + f.at + f.len, format!("{name} {}", f.name)))
        };
        self.solids.iter().flat_map(field).collect()
    }

    /// The held solid whose object holds `at` (which [`slot`] then tells held or not), and the offset in it; `None`
    /// outside every held span. The solid found last is tried first: runs read one solid's words in a row, its held
    /// and its other words alike.
    #[inline(always)]
    fn find(&self, at: u32) -> Option<(&HeldSolid, u32)> {
        if !self.bounds.contains(&at) {
            return None;
        }
        if let Some(s) = self.solids.get(self.last.get()) {
            let off = at.wrapping_sub(s.at);
            if off < s.size {
                return Some((s, off));
            }
        }
        let (start, end) = self.gap.get();
        if (start..end).contains(&at) {
            return None;
        }
        self.search(at)
    }

    /// [`CarSolids::find`] past the solid and the gap found last: the last solid that starts at or below `at` is the
    /// only one whose object can hold it; else the gap between two solids that holds `at` is kept.
    #[inline(never)]
    fn search(&self, at: u32) -> Option<(&HeldSolid, u32)> {
        let next = self.solids.partition_point(|s| s.at <= at);
        let found = next.checked_sub(1).map(|k| (k, &self.solids[k])).filter(|(_, s)| at - s.at < s.size);
        let Some((k, s)) = found else {
            let start = next.checked_sub(1).map_or(self.bounds.start, |k| self.solids[k].at + self.solids[k].size);
            let end = self.solids.get(next).map_or(self.bounds.end, |s| s.at);
            self.gap.set((start, end));
            return None;
        };
        self.last.set(k);
        Some((s, at - s.at))
    }

    /// Whether the `len` bytes from `at` reach a held span's bounds: a block that does not is the image's.
    #[inline]
    pub fn reaches(&self, at: u32, len: u32) -> bool {
        at < self.bounds.end && at.saturating_add(len) > self.bounds.start
    }

    /// The held solid at `at` (its address).
    #[inline]
    pub fn held(&self, at: u32) -> Option<&HeldSolid> {
        self.solids.iter().find(|s| s.at == at)
    }

    /// The word at `at` if a held solid's table names it.
    #[inline]
    pub fn read(&self, at: u32) -> Option<u32> {
        let (s, off) = self.find(at)?;
        s.word(off)
    }

    /// Stores the word at `at` if a held solid's table names it; false otherwise.
    #[inline]
    pub fn write(&self, at: u32, value: u32) -> bool {
        self.find(at).is_some_and(|(s, off)| s.set_word(off, value))
    }

    /// The word at `at` as a step leaves it: a held solid's where its table names it, else `mem`'s.
    #[inline]
    pub fn word(&self, mem: &Mem, at: u32) -> u32 {
        self.read(at).unwrap_or_else(|| mem.u32(at))
    }

    /// Stores the word at `at`: into a held solid where its table names it, else into `mem`.
    #[inline]
    pub fn set_word(&self, mem: &mut Mem, at: u32, value: u32) {
        if !self.write(at, value) {
            mem.set_u32(at, value)
        }
    }

    /// The `N` words from `at` ([`CarSolids::word`]), the held solid found once. A block no held span reaches is
    /// `mem`'s at once. A span is longer than any block, and two are further apart, so a block that starts outside every
    /// span and ends outside every span touches none.
    #[inline]
    pub fn words<const N: usize>(&self, mem: &Mem, at: u32) -> [u32; N] {
        const { assert!(4 * N < HELD.start as usize, "a block reaches past the solid it starts in into the next") };
        match self.find(at) {
            Some((s, off)) => std::array::from_fn(|k| {
                let k = 4 * k as u32;
                s.word(off + k).unwrap_or_else(|| mem.u32(at.wrapping_add(k)))
            }),
            None if self.find(at.wrapping_add(4 * (N as u32).saturating_sub(1))).is_none() => mem.u32s(at),
            None => std::array::from_fn(|k| self.word(mem, at.wrapping_add(4 * k as u32))),
        }
    }

    /// Stores four words from `at` ([`CarSolids::set_word`]), the held solid found once (see [`CarSolids::words`]).
    #[inline]
    pub fn set_words4(&self, mem: &mut Mem, at: u32, words: [u32; 4]) {
        match self.find(at) {
            Some((s, off)) => {
                for (k, word) in (0..16).step_by(4).zip(words) {
                    if !s.set_word(off + k, word) {
                        mem.set_u32(at.wrapping_add(k), word)
                    }
                }
            }
            None if self.find(at.wrapping_add(12)).is_none() => mem.set_words4(at, words),
            None => (0..16).step_by(4).zip(words).for_each(|(k, word)| self.set_word(mem, at.wrapping_add(k), word)),
        }
    }

    /// The rigid store over `mem` with these solids held.
    pub fn view<'a>(&'a self, mem: &'a mut Mem) -> SolidView<'a> {
        SolidView { mem, solids: self }
    }
}

/// The rigid body's store over held solids: a word their tables name is the typed state's, any other the image's.
pub struct SolidView<'a> {
    mem: &'a mut Mem,
    solids: &'a CarSolids,
}

/// The view's word accesses resolve a held solid's words too (a pointer a caller passes may point into a solid); the
/// bytes and `mem()` are the image's.
impl Raw for SolidView<'_> {
    #[inline]
    fn mem(&self) -> &Mem {
        self.mem
    }
    #[inline]
    fn mem_mut(&mut self) -> &mut Mem {
        self.mem
    }
    #[inline]
    fn u32(&self, at: u32) -> u32 {
        self.word(at)
    }
    #[inline]
    fn f32(&self, at: u32) -> f32 {
        f32::from_bits(self.word(at))
    }
    #[inline]
    fn u32s<const N: usize>(&self, at: u32) -> [u32; N] {
        self.words(at)
    }
    #[inline]
    fn f32s<const N: usize>(&self, at: u32) -> [f32; N] {
        self.words::<N>(at).map(f32::from_bits)
    }
    #[inline]
    fn set_u32(&mut self, at: u32, value: u32) {
        self.set_word(at, value)
    }
    #[inline]
    fn set_f32(&mut self, at: u32, value: f32) {
        self.set_word(at, value.to_bits())
    }
}

impl SolidWords for SolidView<'_> {
    #[inline]
    fn image(&mut self) -> Option<&mut Mem> {
        None
    }

    #[inline]
    fn image_ref(&self) -> Option<&Mem> {
        None
    }

    #[inline]
    fn word(&self, at: u32) -> u32 {
        self.solids.word(self.mem, at)
    }

    #[inline]
    fn set_word(&mut self, at: u32, value: u32) {
        self.solids.set_word(self.mem, at, value)
    }

    #[inline]
    fn words<const N: usize>(&self, at: u32) -> [u32; N] {
        self.solids.words(self.mem, at)
    }

    #[inline]
    fn set_words4(&mut self, at: u32, words: [u32; 4]) {
        self.solids.set_words4(self.mem, at, words)
    }
    #[inline]
    fn held(&self, this: u32) -> Option<&HeldSolid> {
        self.solids.held(this)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        ACCEL_ANGULAR_EST, ACCEL_ANGULAR_FILT, ACCEL_LINEAR_EST, ACCEL_LINEAR_FILT, ANGULAR_ACCEL, ANGULAR_VELOCITY,
        FORCE_ACCUM, LINEAR_ACCEL, LINEAR_VELOCITY, ORIENTATION, POSITION, PREV_ANGULAR_VELOCITY, PREV_LINEAR_VELOCITY,
        SCRATCH_ACCUM, SLEEP_TIMER, START_ANGULAR_VELOCITY, START_LINEAR_VELOCITY, START_ORIENTATION, START_POSITION,
        STEP_FLAGS, WINDOW_PROGRESS,
    };
    use super::*;
    use crate::rigid_body::store::RigidStore;
    use crate::store::check_table;

    #[test]
    fn table_is_ordered_and_fits_and_names_the_layout() {
        check_table::<Solid>();
        for (at, named) in [
            (Solid::orientation.at, ORIENTATION),
            (Solid::position.at, POSITION),
            (Solid::angular_velocity.at, ANGULAR_VELOCITY),
            (Solid::linear_velocity.at, LINEAR_VELOCITY),
            (Solid::prev_angular_velocity.at, PREV_ANGULAR_VELOCITY),
            (Solid::prev_linear_velocity.at, PREV_LINEAR_VELOCITY),
            (Solid::accel_linear_est.at, ACCEL_LINEAR_EST),
            (Solid::accel_angular_est.at, ACCEL_ANGULAR_EST),
            (Solid::accel_linear_filt.at, ACCEL_LINEAR_FILT),
            (Solid::accel_angular_filt.at, ACCEL_ANGULAR_FILT),
            (Solid::sleep_timer.at, SLEEP_TIMER),
            (Solid::progress.at, WINDOW_PROGRESS),
            (Solid::start_orientation.at, START_ORIENTATION),
            (Solid::start_angular_velocity.at, START_ANGULAR_VELOCITY),
            (Solid::scratch.at, SCRATCH_ACCUM),
            (Solid::angular_accel.at, ANGULAR_ACCEL),
            (Solid::start_position.at, START_POSITION),
            (Solid::start_linear_velocity.at, START_LINEAR_VELOCITY),
            (Solid::force.at, FORCE_ACCUM),
            (Solid::linear_accel.at, LINEAR_ACCEL),
            (Solid::step_flags.at, STEP_FLAGS),
        ] {
            assert_eq!(at, named);
        }
        for f in Solid::FIELDS {
            assert!(HELD.start <= f.at && f.at + f.len <= HELD.end, "{} outside the held span", f.name);
        }
        // The own tables: whole words in their span, the object sizes the constructors allocate.
        crate::store::check_table::<Hinged>();
        crate::store::check_table::<Mounted>();
        assert_eq!(Hinged::SIZE, crate::entities::layout::HINGED_SIZE);
        assert_eq!(Mounted::SIZE, crate::physics_core::layout::mounted_body::BODY_SIZE);
        for t in [&HINGED_OWN, &MOUNTED_OWN] {
            let last = t.fields.last().unwrap();
            assert!(last.at + last.len <= t.start + 4 * OWN_WORDS as u32, "an own table past its span");
        }
    }

    /// A hinged body's own words are held with its solid: read and written held, lowered back, the rest the image's.
    #[test]
    fn hinged_own_words_are_held_with_the_solid() {
        let (car, hinged) = (0x10000, 0x20000);
        let mut mem = Mem::new();
        mem.map(car, &[0; 0x1d60]);
        mem.map(hinged, &[0; 0x660]);
        mem.set_u32(hinged_slot(car, 0), hinged);
        mem.set_f32(hinged + Hinged::angle.at, 0.5);
        mem.set_u32(hinged + 0x3f8, 7);
        let solids = CarSolids::lift(&mem, car);
        assert_eq!(solids.read(hinged + Hinged::angle.at), Some(0.5f32.to_bits()));
        assert_eq!(solids.read(hinged + 0x3f8), None, "the angle's dirty word is the image's");
        assert!(solids.write(hinged + Hinged::solved.at, 2.0f32.to_bits()));
        assert!(!solids.write(hinged + 0x610, 1), "the walk's output is the image's");
        assert_eq!(mem.f32(hinged + Hinged::solved.at), 0.0);
        solids.lower(&mut mem);
        assert_eq!(mem.f32(hinged + Hinged::solved.at), 2.0);
        assert!(solids.owned().iter().any(|(r, name)| r.start == hinged + 0x624 && name == "hinged0 solid solved"));
    }

    /// A held solid's words are its typed state's, the rest the image's; lower writes them back.
    #[test]
    fn view_reads_and_writes_held_words_only() {
        let (car, other) = (0x10000, 0x20000);
        let mut mem = Mem::new();
        mem.map(car, &[0; 0x1d60]);
        mem.map(other, &[0; 0x400]);
        mem.set_f32(car + POSITION, 1.0);
        mem.set_u32(car + 0x34, 0x1234);
        let solids = CarSolids::lift(&mem, car);
        assert_eq!(solids.solids.len(), 1);
        let mut view = solids.view(&mut mem);
        assert_eq!(view.position(car).x, 1.0);
        view.store_linear_velocity(car, crate::math::vec::Vec4 { x: 2.0, y: 3.0, z: 4.0, w: 5.0 });
        view.store_linear_velocity(other, crate::math::vec::Vec4 { x: 6.0, y: 0.0, z: 0.0, w: 0.0 });
        assert_eq!(view.u32(car + 0x34), 0x1234);
        assert_eq!(mem.f32(car + LINEAR_VELOCITY), 0.0);
        assert_eq!(mem.f32(other + LINEAR_VELOCITY), 6.0);
        solids.lower(&mut mem);
        assert_eq!(mem.f32s::<4>(car + LINEAR_VELOCITY), [2.0, 3.0, 4.0, 5.0]);
        // Held on: a word back at its lifted value is lowered too (the image holds the last lower's).
        solids.view(&mut mem).store_linear_velocity(car, crate::math::vec::Vec4::default());
        solids.lower(&mut mem);
        assert_eq!(mem.f32s::<4>(car + LINEAR_VELOCITY), [0.0; 4]);
    }
}
