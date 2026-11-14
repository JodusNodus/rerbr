//! The car's solids in typed storage (`notes/physics.md` "Store seam"): the `CSolid` words a step writes ([`Solid`]:
//! pose, matrix, velocities, the window's and the integrator's vectors, the accumulators, the sleep words), for the car,
//! its wheels and its hinged and mounted bodies ([`CarSolids`], keyed by the solid's address), and their store
//! ([`SolidView`]: a word a held solid's table names is its typed state's, any other the image's). The pointers, the
//! flags byte, the NTweak headers and dirty words (`SetDirty`), the scale `+0xd0`, the state's diagonal and gate and
//! the scratch block's other words stay in the image. The pure step holds them (`Typed::lift_with_solids`); their
//! readers at their addresses in other subsystems run on `SolidWords` stores (notes).

use std::cell::RefCell;
use std::ops::Range;

use super::mem_store::SolidWords;
use crate::car_body::layout::{hinged_slot, mounted_slot, wheel_list_at};
use crate::mem::Mem;
use crate::store::{Object, Raw};

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

/// A solid in typed storage: its address, its name and its state, borrowed for one word at a time.
#[derive(Clone, Debug, PartialEq)]
pub struct HeldSolid {
    pub at: u32,
    pub name: String,
    pub state: RefCell<Solid>,
}

/// The car's solids: the car's own, each wheel's, each hinged and each mounted body's (the car's slots that are set,
/// each address once).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarSolids {
    pub solids: Vec<HeldSolid>,
}

impl CarSolids {
    /// The solids of the car at `car`, lifted from the image.
    pub fn lift(mem: &Mem, car: u32) -> Self {
        let mut solids: Vec<HeldSolid> = Vec::new();
        let mut add = |name: String, at: u32| {
            if at != 0 && solids.iter().all(|s| s.at != at) {
                solids.push(HeldSolid { at, name, state: RefCell::new(Solid::lift(mem, at)) });
            }
        };
        add("car solid".into(), car);
        for k in 0..4 {
            add(format!("wheel{k} solid"), mem.u32(wheel_list_at(car, k)));
            add(format!("hinged{k} solid"), mem.u32(hinged_slot(car, k)));
        }
        for k in 0..3 {
            add(format!("mounted{k} solid"), mem.u32(mounted_slot(car, k)));
        }
        CarSolids { solids }
    }

    /// Writes every solid's state back to the image.
    pub fn lower(&self, mem: &mut Mem) {
        for s in &self.solids {
            s.state.borrow().lower(mem, s.at);
        }
    }

    /// Every owned byte range, named by its solid and field.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        let field = |s: &HeldSolid| {
            let (at, name) = (s.at, s.name.clone());
            Solid::FIELDS.iter().map(move |f| (at + f.at..at + f.at + f.len, format!("{name} {}", f.name)))
        };
        self.solids.iter().flat_map(field).collect()
    }

    /// The held solid whose typed words hold `at`, and the offset in it.
    #[inline]
    fn find(&self, at: u32) -> Option<(&HeldSolid, u32)> {
        self.solids.iter().find_map(|s| {
            let off = at.wrapping_sub(s.at);
            HELD.contains(&off).then_some((s, off))
        })
    }

    /// The word at `at` if a held solid's table names it.
    #[inline]
    pub fn read(&self, at: u32) -> Option<u32> {
        let (s, off) = self.find(at)?;
        s.state.borrow().word_of(off)
    }

    /// Stores the word at `at` if a held solid's table names it; false otherwise.
    #[inline]
    pub fn write(&self, at: u32, value: u32) -> bool {
        self.find(at).is_some_and(|(s, off)| s.state.borrow_mut().set_word_of(off, value))
    }

    /// The rigid store over `mem` with these solids held.
    pub fn view<'a>(&'a self, mem: &'a mut Mem) -> SolidView<'a> {
        SolidView { mem, solids: Some(self) }
    }
}

/// The rigid body's store over held solids: a word their tables name is the typed state's, any other the image's;
/// without solids (`image`) every word is the image's.
pub struct SolidView<'a> {
    mem: &'a mut Mem,
    solids: Option<&'a CarSolids>,
}

impl<'a> SolidView<'a> {
    /// The store with every word on the image (the per-call path).
    pub fn image(mem: &'a mut Mem) -> Self {
        SolidView { mem, solids: None }
    }
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
        self.solids.is_none().then_some(&mut *self.mem)
    }

    #[inline]
    fn image_ref(&self) -> Option<&Mem> {
        self.solids.is_none().then_some(&*self.mem)
    }

    #[inline]
    fn word(&self, at: u32) -> u32 {
        self.solids.and_then(|s| s.read(at)).unwrap_or_else(|| self.mem.u32(at))
    }

    #[inline]
    fn set_word(&mut self, at: u32, value: u32) {
        if !self.solids.is_some_and(|s| s.write(at, value)) {
            self.mem.set_u32(at, value)
        }
    }

    #[inline]
    fn words<const N: usize>(&self, at: u32) -> [u32; N] {
        match self.solids {
            Some(_) => std::array::from_fn(|k| self.word(at + 4 * k as u32)),
            None => self.mem.u32s(at),
        }
    }

    #[inline]
    fn set_words4(&mut self, at: u32, words: [u32; 4]) {
        match self.solids {
            Some(_) => words.into_iter().enumerate().for_each(|(k, word)| self.set_word(at + 4 * k as u32, word)),
            None => self.mem.set_words4(at, words),
        }
    }
}

/// `lower(lift(image))` of the car's solids into an empty image, compared with `mem` on every owned byte: their count,
/// or the first byte that differs.
pub fn round_trip(mem: &Mem, car: u32) -> Result<usize, String> {
    let solids = CarSolids::lift(mem, car);
    let mut out = Mem::new();
    solids.lower(&mut out);
    let mut bytes = 0;
    for (range, name) in solids.owned() {
        let len = (range.end - range.start) as usize;
        let (want, got) = (mem.read(range.start, len), out.read(range.start, len));
        if let Some(k) = want.iter().zip(&got).position(|(a, b)| a != b) {
            return Err(format!("{name} at {:#010x}: +{k:#x} differs after lower(lift)", range.start));
        }
        bytes += len;
    }
    Ok(bytes)
}

/// The rigid body's host-free entries in a row on the solid at `this`: the window start, the point velocity of its
/// position, the row transforms, folds and quaternion loops on rows at `scratch` (seeded from its own vectors), the
/// velocity clamp along its previous angular velocity (a pointer into the solid) and the scratch accumulate; the
/// entries' return words.
fn entries<S: crate::rigid_body::store::RigidStore>(store: &mut S, this: u32, scratch: u32) -> Vec<u32> {
    use super::{solid_window_start, POSITION, PREV_ANGULAR_VELOCITY};
    use crate::rigid_body::layout as rb;
    let mut out = Vec::new();
    solid_window_start(store, this);
    rb::point_velocity(store, this, this + POSITION, scratch);
    for (k, from) in [0x190, 0x1c0, 0x230, 0x250].into_iter().enumerate() {
        let row = store.row(this + from);
        store.store_row(scratch + 0x10 + 0x10 * k as u32, row);
    }
    let rows = scratch + 0x10;
    rb::transform_body_rows(store, this, rows, 2);
    rb::transform_body_rows3(store, this, rows + 0x20, 2);
    out.push(rb::rows_to_body(store, this, rows, 4));
    rb::fold_body_rows(store, this, rows, 2);
    rb::fold_body_rows_reordered(store, this, rows + 0x20, 2);
    rb::quat_plus_body_rows(store, this, rows, 1);
    rb::quat_diff_body_rows(store, this, rows + 0x10, 1);
    rb::quat_transform_body_rows(store, this, rows + 0x20, 1);
    rb::body_rotate_rows(store, this, rows + 0x30, 1);
    rb::body_fold_rows_by_columns(store, this, rows, 1);
    rb::body_fold_rows_by_rows(store, this, rows + 0x10, 1);
    out.push(rb::scale_add_rows(store, this, rows + 0x20, 2));
    out.push(rb::clamp_velocity_along(store, this, this + PREV_ANGULAR_VELOCITY, 0.5, 0.25));
    rb::scratch_accumulate(store, this, rows, 0, &mut NoFlush).expect("no flush without the bit");
    out
}

/// The scratch accumulate's flush, never called (the entries pass no gate bit).
struct NoFlush;

impl crate::rigid_body::scratch::FlushHost for NoFlush {
    fn flush(&mut self) -> Result<(), crate::rigid_body::host::HostUnavailable> {
        unreachable!("the differential's gate has no flush bit")
    }
}

/// The scratch's bytes the entries write.
const SCRATCH_BYTES: u32 = 0x50;

/// The rigid body's host-free entries ([`entries`]) on each of the car's solids, per call on `per_call` (every word on
/// the image) and on the solids held in typed storage over the run on `held` (lifted before, lowered after), two
/// copies of one image; `scratch` is free memory of both. Compares every byte of each solid and the scratch, and the
/// entries' return words: the bytes compared, or the first that differs.
pub fn differential(per_call: &mut Mem, held: &mut Mem, car: u32, scratch: u32) -> Result<usize, String> {
    let solids = CarSolids::lift(held, car);
    let mut bytes = 0;
    for s in &solids.solids {
        let a = entries(per_call, s.at, scratch);
        let b = entries(&mut solids.view(held), s.at, scratch);
        if a != b {
            return Err(format!("{}: the entries' returns differ: {a:x?} against {b:x?}", s.name));
        }
        solids.lower(held);
        for (at, len, what) in [(s.at, Solid::SIZE, "solid"), (scratch, SCRATCH_BYTES, "scratch")] {
            let (want, got) = (per_call.read(at, len as usize), held.read(at, len as usize));
            if let Some(i) = want.iter().zip(&got).position(|(x, y)| x != y) {
                let field = Solid::field_at(i as u32).filter(|_| what == "solid").map_or("", |f| f.name);
                return Err(format!(
                    "{} {what} +{i:#x} {field} differs: per call {:#04x}, held {:#04x}",
                    s.name, want[i], got[i]
                ));
            }
            bytes += len as usize;
        }
    }
    Ok(bytes)
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
    }
}
