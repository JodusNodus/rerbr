//! The collision state a step holds typed (`notes/physics.md` "Store seam"): the solver's contact and offset rows with
//! their counts, and the step-written bytes of the car's grids (the sample bytes the edge tests leave, which the event
//! builder and the grid hit take). Their tables ([`RowsHeadObj`] and [`ContactRowObj`] in [`ContactRows`],
//! [`OffsetRowObj`], the counts, [`GridHitsObj`]), the storage a step holds ([`CarCollision`]), its store
//! ([`CollisionView`]) and the round trip.
//!
//! The solver's rows are 59 KB and a step touches few of them (none in most steps), so a row is lifted whole when the
//! step first touches it and only lifted rows are lowered; the counts and the grids' bytes are lifted with the rest.
//! Per call (no step holds them) the view is the image. The rest stays in the image: the grids' load-time words (the
//! vertices the bodies' transforms read at their addresses, the edges, counts and sample tables), the solver's event
//! list and the events, the bodies' offset-row link (`+0x3b0`), and each contact row's composed point (`+0x40`): the
//! rigid body's contact composes it, samples it and hands it to the body's hook at its address (`CSolid::BigA`'s
//! `ComposeA`, `Sample` and slot-2 calls). The bodies' position, progress and contact-row link (`+0x140`, `+0x290`,
//! `+0x274`) are the solids' words: held ones through [`CollisionView::with_solids`], else the image.

use std::cell::RefCell;
use std::ops::Range;

use super::cell_links::CellLinks;
use super::cell_table::CellTable;
use super::col_grids::{col_grid_at, ColGrid};
use super::mem_store::open_rows_header;
use super::solver::{
    BODY_CONTACT_ROW, BODY_OFFSET_ROW, BODY_POSITION, BODY_PROGRESS, SOLVER_CONTACT_ROW, SOLVER_CONTACT_ROWS,
    SOLVER_OFFSET_COUNT, SOLVER_OFFSET_ROW, SOLVER_OFFSET_ROWS,
};
use super::Body;
use crate::collision::contact_solver::ContactRow;
use crate::collision::store::CollisionStore;
use crate::math::vec::Vec4;
use crate::mem::Mem;
use crate::physics_core::layout::PHYSICS_GLOBAL;
use crate::rigid_body::contact::Contact;
use crate::rigid_body::layout::CarSolids;
use crate::store::{Object, Raw, Tabled};

/// The solver's rows: 64 contact rows (a body's, 8 rows each) and 64 offset rows (a body's, 8 offsets each).
const ROWS: usize = 64;
/// The car's grids: the collision ids 1 to 9 (`alloc_collision_id` leaves them to the car).
const CAR_GRIDS: usize = 9;
/// The physics' scratch pointer (`+0x28`) and the solver in it (`+0x10`).
const PHYSICS_SCRATCH: u32 = 0x28;
const SCRATCH_SOLVER: u32 = 0x10;

/// A body's contact rows hold 8 rows of 0x60 bytes after their 16-byte header.
const CONTACT_ROWS: usize = 8;
const CONTACT_ROW_SIZE: u32 = 0x60;
const CONTACT_ROW_FIRST: u32 = 0x10;
/// A contact row's composed point, which stays in the image.
const CONTACT_ROW_COMPOSED: u32 = 0x40;

crate::object! {
    /// One contact row (0x60 bytes, `contact_solver::ContactRow`) but its composed point (`+0x40`, in the image): the
    /// suppression flag, the material tag, the ids, the restitutions, the frictions, the point, the plane and the other
    /// point. The rigid body reads it as its contact ([`Contact`]).
    pub struct ContactRowObj, size 0x60 {
        suppressed @ 0x00: u32,
        tag @ 0x04: u32,
        ids @ 0x08: [u32; 2],
        restitution @ 0x10: [f32; 2],
        friction @ 0x18: [f32; 2],
        point @ 0x20: Vec4,
        plane @ 0x30: Vec4,
        other @ 0x50: Vec4,
    }
}

impl ContactRowObj {
    fn of(row: &ContactRow) -> Self {
        ContactRowObj {
            suppressed: row.suppressed,
            tag: row.tag,
            ids: row.ids,
            restitution: row.restitution,
            friction: row.friction,
            point: row.point,
            plane: row.plane,
            other: row.other,
        }
    }

    /// The row as the rigid body's contact reads it, with its composed point `composed` (`rigid_body::layout::
    /// load_contact`'s words: the tag, the restitutions as the stiffness and force cap, the frictions as the tangent
    /// caps, the point, the plane as the normal and the composed point as the vector).
    pub fn contact(&self, composed: Vec4) -> Contact {
        Contact {
            id: self.tag,
            stiffness: self.restitution[0],
            force_cap: self.restitution[1],
            tangent_cap_0: self.friction[0],
            tangent_cap_1: self.friction[1],
            point: self.point,
            normal: self.plane,
            vector: composed,
        }
    }
}

crate::object! {
    /// The header of a body's contact rows in the solver (0x310 bytes with the rows): the body, the row count and two
    /// words nothing writes.
    pub struct RowsHeadObj, size 0x10 {
        body @ 0x00: u32,
        count @ 0x04: u32,
        spare @ 0x08: [u32; 2],
    }
}

/// A body's contact rows in the solver: the header and the 8 rows (their composed points in the image).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactRows {
    pub head: RowsHeadObj,
    pub rows: [ContactRowObj; CONTACT_ROWS],
}

impl ContactRows {
    fn row_at(at: u32, j: usize) -> u32 {
        at + CONTACT_ROW_FIRST + CONTACT_ROW_SIZE * j as u32
    }

    fn lift(mem: &Mem, at: u32) -> Self {
        ContactRows {
            head: RowsHeadObj::lift(mem, at),
            rows: std::array::from_fn(|j| ContactRowObj::lift(mem, Self::row_at(at, j))),
        }
    }

    fn lower(&self, mem: &mut Mem, at: u32) {
        self.head.lower(mem, at);
        for (j, row) in self.rows.iter().enumerate() {
            row.lower(mem, Self::row_at(at, j));
        }
    }

    /// The owned byte ranges of the rows at `at`.
    fn owned(at: u32) -> impl Iterator<Item = Range<u32>> {
        RowsHeadObj::owned(at).chain((0..CONTACT_ROWS).flat_map(move |j| ContactRowObj::owned(Self::row_at(at, j))))
    }

    /// The tables of the rows at `at`, named after `k`.
    fn tabled(k: usize, at: u32) -> impl Iterator<Item = Tabled> {
        std::iter::once(Tabled::of::<RowsHeadObj>(format!("solver rows {k}"), at)).chain(
            (0..CONTACT_ROWS)
                .map(move |j| Tabled::of::<ContactRowObj>(format!("solver rows {k} row {j}"), Self::row_at(at, j))),
        )
    }
}

crate::object! {
    /// A body's offset row in the solver (0x90 bytes): the body, the offset count, two words nothing writes, the
    /// offsets.
    pub struct OffsetRowObj, size 0x90 {
        body @ 0x00: u32,
        count @ 0x04: u32,
        spare @ 0x08: [u32; 2],
        offsets @ 0x10: [Vec4; 8],
    }
}

crate::object! {
    /// The solver's contact-row count (`CCollisionSolver+0x18`).
    pub struct ContactCountObj, size 0x20 {
        contact_count @ 0x18: u32,
    }
}

crate::object! {
    /// The solver's offset-row count (`CCollisionSolver+0xc420`).
    pub struct OffsetCountObj, size 0x4 {
        offset_count @ 0x0: u32,
    }
}

crate::object! {
    /// A `CColGrid`'s bytes a step writes: the two sample bytes the edge tests leave (`+0x688`), which the event
    /// builder and the grid hit read and clear.
    pub struct GridHitsObj, size 0x6b0 {
        hits @ 0x688: [u8; 2],
    }
}

/// Where the held collision state lives in the image: the solver (0 without one) and the car's first grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollisionImage {
    pub solver: u32,
}

impl CollisionImage {
    /// The world's solver, as the physics object's scratch holds it (0 without a physics object or scratch).
    pub fn of(mem: &Mem) -> Self {
        let physics = mem.u32(PHYSICS_GLOBAL);
        let scratch = if physics == 0 { 0 } else { mem.u32(physics + PHYSICS_SCRATCH) };
        CollisionImage { solver: if scratch == 0 { 0 } else { scratch + SCRATCH_SOLVER } }
    }

    fn contact_rows_at(&self, k: usize) -> u32 {
        self.solver + SOLVER_CONTACT_ROWS + SOLVER_CONTACT_ROW * k as u32
    }

    fn offset_row_at(&self, k: usize) -> u32 {
        self.solver + SOLVER_OFFSET_ROWS + SOLVER_OFFSET_ROW * k as u32
    }

    /// The index of the contact rows at `rows`, when it is one of the solver's.
    #[inline]
    fn contact_index(&self, rows: u32) -> Option<usize> {
        let k = rows.wrapping_sub(self.contact_rows_at(0));
        (self.solver != 0 && k.is_multiple_of(SOLVER_CONTACT_ROW) && k / SOLVER_CONTACT_ROW < ROWS as u32)
            .then_some((k / SOLVER_CONTACT_ROW) as usize)
    }

    /// The index of the offset row at `row`, when it is one of the solver's.
    #[inline]
    fn offset_index(&self, row: u32) -> Option<usize> {
        let k = row.wrapping_sub(self.offset_row_at(0));
        (self.solver != 0 && k.is_multiple_of(SOLVER_OFFSET_ROW) && k / SOLVER_OFFSET_ROW < ROWS as u32)
            .then_some((k / SOLVER_OFFSET_ROW) as usize)
    }

    /// The index of the grid at `grid` among the car's.
    #[inline]
    fn grid_index(grid: u32) -> Option<usize> {
        let size = ColGrid::SIZE as u32;
        let k = grid.wrapping_sub(col_grid_at(1));
        (k.is_multiple_of(size) && k / size < CAR_GRIDS as u32).then_some((k / size) as usize)
    }

    /// Whether `grid` is one of the car's grids, whose sample bytes a step holds.
    pub fn is_car_grid(grid: u32) -> bool {
        Self::grid_index(grid).is_some()
    }

    fn holds(&self, solver: u32) -> bool {
        self.solver != 0 && solver == self.solver
    }

    /// The tables of the held state at their addresses: the counts, every row, the car's grids.
    pub fn tabled(&self) -> Vec<Tabled> {
        let mut out: Vec<Tabled> = (0..CAR_GRIDS)
            .map(|k| Tabled::of::<GridHitsObj>(format!("collision grid {}", k + 1), col_grid_at(k as u32 + 1)))
            .collect();
        if self.solver != 0 {
            out.push(Tabled::of::<ContactCountObj>("solver contact count", self.solver));
            out.push(Tabled::of::<OffsetCountObj>("solver offset count", self.solver + SOLVER_OFFSET_COUNT));
            out.extend((0..ROWS).flat_map(|k| ContactRows::tabled(k, self.contact_rows_at(k))));
            out.extend(
                (0..ROWS).map(|k| Tabled::of::<OffsetRowObj>(format!("solver offset row {k}"), self.offset_row_at(k))),
            );
        }
        out
    }

    /// The owned byte ranges with their names: the counts, the rows (lifted or not) and the car's grids' bytes.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        let mut out: Vec<_> = (0..CAR_GRIDS)
            .flat_map(|k| {
                GridHitsObj::owned(col_grid_at(k as u32 + 1)).map(move |r| (r, format!("grid {} hits", k + 1)))
            })
            .collect();
        if self.solver != 0 {
            let s = self.solver;
            out.extend(ContactCountObj::owned(s).map(|r| (r, "solver contact count".to_string())));
            out.extend(
                (0..ROWS)
                    .flat_map(|k| ContactRows::owned(self.contact_rows_at(k)))
                    .map(|r| (r, "solver contact rows".to_string())),
            );
            out.extend(OffsetCountObj::owned(s + SOLVER_OFFSET_COUNT).map(|r| (r, "solver offset count".to_string())));
            out.push((self.offset_row_at(0)..self.offset_row_at(ROWS), "solver offset rows".to_string()));
        }
        out
    }
}

/// The rows a step touched so far, by index, in the order it touched them (any other is still in the image). A step
/// touches few, so they are found by a walk.
#[derive(Clone, Debug, PartialEq)]
pub struct Touched<T>(Vec<(usize, T)>);

impl<T> Default for Touched<T> {
    fn default() -> Self {
        Touched(Vec::new())
    }
}

impl<T> Touched<T> {
    /// Row `k`, `lift`ed first when the step had not touched it.
    fn get(&mut self, k: usize, lift: impl FnOnce() -> T) -> &mut T {
        let at = match self.0.iter().position(|(j, _)| *j == k) {
            Some(at) => at,
            None => {
                self.0.push((k, lift()));
                self.0.len() - 1
            }
        };
        &mut self.0[at].1
    }

    /// The touched rows with their indices.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &T)> {
        self.0.iter().map(|(k, t)| (*k, t))
    }
}

/// The held state: the counts and the car's grids' bytes, and the rows a step touched so far.
#[derive(Clone, Debug, PartialEq)]
pub struct CollisionTyped {
    pub contact_count: u32,
    pub offset_count: u32,
    pub contact_rows: Touched<ContactRows>,
    pub offset_rows: Touched<OffsetRowObj>,
    pub grids: [[u8; 2]; CAR_GRIDS],
}

/// The collision state in typed storage over a step: where it lives and the state, in a cell borrowed by each access
/// alone (the passes call each other and the bodies through the host).
#[derive(Clone, Debug, PartialEq)]
pub struct CarCollision {
    pub image: CollisionImage,
    pub typed: RefCell<CollisionTyped>,
    /// The cell grid's table (its slots on first touch).
    pub cells: CellTable,
    /// The cells' lists and the bodies' cell links (a record on its first write).
    pub links: CellLinks,
}

impl CarCollision {
    /// The world's collision state for a step, lifted from the image: the counts and the grids' bytes (the rows when
    /// touched).
    pub fn lift(mem: &Mem) -> Self {
        let image = CollisionImage::of(mem);
        let (contact_count, offset_count) = if image.solver == 0 {
            (0, 0)
        } else {
            (
                ContactCountObj::contact_count.load(mem, image.solver),
                OffsetCountObj::offset_count.load(mem, image.solver + SOLVER_OFFSET_COUNT),
            )
        };
        let grids = std::array::from_fn(|k| GridHitsObj::hits.load(mem, col_grid_at(k as u32 + 1)));
        let typed = CollisionTyped {
            contact_count,
            offset_count,
            contact_rows: Touched::default(),
            offset_rows: Touched::default(),
            grids,
        };
        let cells = CellTable::lift(mem);
        let links = CellLinks::lift(mem, &cells);
        CarCollision { image, typed: RefCell::new(typed), cells, links }
    }

    /// As [`CarCollision::lift`] with every row lifted (the round trip's).
    pub fn lift_all(mem: &Mem) -> Self {
        let held = Self::lift(mem);
        if held.image.solver != 0 {
            let mut t = held.typed.borrow_mut();
            for k in 0..ROWS {
                t.contact_rows.get(k, || ContactRows::lift(mem, held.image.contact_rows_at(k)));
                t.offset_rows.get(k, || OffsetRowObj::lift(mem, held.image.offset_row_at(k)));
            }
        }
        held
    }

    /// Writes the held state back: the counts, the grids' bytes and every lifted row.
    pub fn lower(&self, mem: &mut Mem) {
        self.cells.lower(mem);
        self.links.lower(mem);
        let t = self.typed.borrow();
        for (k, hits) in t.grids.iter().enumerate() {
            GridHitsObj::hits.store(mem, col_grid_at(k as u32 + 1), *hits);
        }
        let image = self.image;
        if image.solver == 0 {
            return;
        }
        ContactCountObj::contact_count.store(mem, image.solver, t.contact_count);
        OffsetCountObj::offset_count.store(mem, image.solver + SOLVER_OFFSET_COUNT, t.offset_count);
        for (k, rows) in t.contact_rows.iter() {
            rows.lower(mem, image.contact_rows_at(k));
        }
        for (k, row) in t.offset_rows.iter() {
            row.lower(mem, image.offset_row_at(k));
        }
    }

    /// Every owned byte range, named.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        let mut owned = self.image.owned();
        owned.extend(self.cells.owned());
        owned.extend(self.links.owned());
        owned
    }

    /// The held state as a store: its words here, the rest of the image in `mem`.
    pub fn view<'a>(&'a self, mem: &'a mut Mem) -> CollisionView<'a> {
        CollisionView { mem, held: Some(self), solids: None }
    }

    /// The contact rows `k`, lifted from `image` when the step had not touched them.
    fn rows<T>(&self, image: &Mem, k: usize, f: impl FnOnce(&mut ContactRows) -> T) -> T {
        let mut t = self.typed.borrow_mut();
        let at = self.image.contact_rows_at(k);
        f(t.contact_rows.get(k, || on_image(image, |m| ContactRows::lift(m, at))))
    }

    /// The offset row `k`, lifted from `image` when the step had not touched it.
    fn offset_row<T>(&self, image: &Mem, k: usize, f: impl FnOnce(&mut OffsetRowObj) -> T) -> T {
        let mut t = self.typed.borrow_mut();
        let at = self.image.offset_row_at(k);
        f(t.offset_rows.get(k, || on_image(image, |m| OffsetRowObj::lift(m, at))))
    }

    /// The contact record at `at` as the rigid body reads it, when it is a row of the held solver's (`None` for any
    /// other address).
    pub fn contact(&self, mem: &Mem, at: u32) -> Option<Contact> {
        let offset = at.wrapping_sub(self.image.contact_rows_at(0));
        let (k, within) = (offset / SOLVER_CONTACT_ROW, offset % SOLVER_CONTACT_ROW);
        let first = within.wrapping_sub(CONTACT_ROW_FIRST);
        if self.image.solver == 0
            || k >= ROWS as u32
            || within < CONTACT_ROW_FIRST
            || !first.is_multiple_of(CONTACT_ROW_SIZE)
        {
            return None;
        }
        let composed = Vec4::load(mem, at + CONTACT_ROW_COMPOSED);
        Some(self.rows(mem, k as usize, |r| r.rows[(first / CONTACT_ROW_SIZE) as usize].contact(composed)))
    }

    /// The count of the contact rows at `rows`, when they are the held solver's.
    pub fn row_count(&self, mem: &Mem, rows: u32) -> Option<u32> {
        let k = self.image.contact_index(rows)?;
        Some(self.rows(mem, k, |r| r.head.count))
    }

    /// Clears the body word of the contact rows at `rows` (the car's respawn), when they are the held solver's; false
    /// for any other address.
    pub fn clear_row_body(&self, mem: &Mem, rows: u32) -> bool {
        let Some(k) = self.image.contact_index(rows) else {
            return false;
        };
        self.rows(mem, k, |r| r.head.body = 0);
        true
    }
}

/// `f` on `mem` without the poison of a check build (a lift reads the owned words).
pub(super) fn on_image<T>(mem: &Mem, f: impl FnOnce(&Mem) -> T) -> T {
    #[cfg(feature = "poison")]
    return f(&mem.unpoisoned());
    #[cfg(not(feature = "poison"))]
    f(mem)
}

/// The collision store over the held state: a held word from it, any other from the image. Per call (`held` none) it
/// is the image. With the car's solids held (`with_solids`), the bodies' words the solver reads and writes (position,
/// window progress, the contact-row link `+0x274`) are theirs.
pub struct CollisionView<'a> {
    mem: &'a mut Mem,
    held: Option<&'a CarCollision>,
    solids: Option<&'a CarSolids>,
}

impl<'a> CollisionView<'a> {
    /// The image as the store (per call).
    pub fn image(mem: &'a mut Mem) -> Self {
        CollisionView { mem, held: None, solids: None }
    }

    /// The store with the car's solids held too (none: the bodies' words on the image).
    pub fn with_solids(self, solids: Option<&'a CarSolids>) -> Self {
        CollisionView { solids, ..self }
    }

    /// A body's word: a held solid's, else the image's.
    #[inline]
    fn body_word(&self, at: u32) -> u32 {
        self.solids.and_then(|s| s.read(at)).unwrap_or_else(|| self.mem.u32(at))
    }

    #[inline]
    fn set_body_word(&mut self, at: u32, value: u32) {
        if !self.solids.is_some_and(|s| s.write(at, value)) {
            self.mem.set_u32(at, value);
        }
    }

    #[inline]
    fn solver(&self, solver: u32) -> Option<&'a CarCollision> {
        self.held.filter(|h| h.image.holds(solver))
    }

    #[inline]
    fn contact_rows(&self, rows: u32) -> Option<(&'a CarCollision, usize)> {
        self.held.and_then(|h| Some((h, h.image.contact_index(rows)?)))
    }

    #[inline]
    fn offset_row(&self, row: u32) -> Option<(&'a CarCollision, usize)> {
        self.held.and_then(|h| Some((h, h.image.offset_index(row)?)))
    }

    #[inline]
    fn grid(&self, grid: u32) -> Option<(&'a CarCollision, usize)> {
        self.held.and_then(|h| Some((h, CollisionImage::grid_index(grid)?)))
    }

    /// The cells' lists and links held, if any.
    #[inline]
    fn links(&self) -> Option<&'a CellLinks> {
        self.held.map(|h| &h.links)
    }

    /// The grid's two sample bytes.
    fn hits(&self, grid: u32) -> [u8; 2] {
        match self.grid(grid) {
            Some((h, k)) => h.typed.borrow().grids[k],
            None => GridHitsObj::hits.load(self.mem, grid),
        }
    }

    fn set_hits(&mut self, grid: u32, hits: [u8; 2]) {
        match self.grid(grid) {
            Some((h, k)) => h.typed.borrow_mut().grids[k] = hits,
            None => GridHitsObj::hits.store(self.mem, grid, hits),
        }
    }
}

impl Raw for CollisionView<'_> {
    fn mem(&self) -> &Mem {
        self.mem
    }
    fn mem_mut(&mut self) -> &mut Mem {
        self.mem
    }
}

impl CollisionStore for CollisionView<'_> {
    fn body_progress(&self, body: u32) -> f32 {
        f32::from_bits(self.body_word(body + BODY_PROGRESS))
    }

    fn body_position(&self, body: u32) -> Vec4 {
        Vec4::from([0, 4, 8, 12].map(|k| f32::from_bits(self.body_word(body + BODY_POSITION + k))))
    }

    fn store_body_position(&mut self, body: u32, value: Vec4) {
        for (k, lane) in [value.x, value.y, value.z, value.w].into_iter().enumerate() {
            self.set_body_word(body + BODY_POSITION + 4 * k as u32, lane.to_bits());
        }
    }

    fn body_contact_rows(&self, body: u32) -> u32 {
        self.body_word(body + BODY_CONTACT_ROW)
    }

    fn set_body_contact_rows(&mut self, body: u32, rows: u32) {
        self.set_body_word(body + BODY_CONTACT_ROW, rows);
    }

    fn solver_contact_count(&self, solver: u32) -> u32 {
        match self.solver(solver) {
            Some(h) => h.typed.borrow().contact_count,
            None => CollisionStore::solver_contact_count(&*self.mem, solver),
        }
    }

    fn set_solver_contact_count(&mut self, solver: u32, count: u32) {
        match self.solver(solver) {
            Some(h) => h.typed.borrow_mut().contact_count = count,
            None => CollisionStore::set_solver_contact_count(self.mem, solver, count),
        }
    }

    fn solver_contact_body(&self, solver: u32, index: u32) -> u32 {
        match self.solver(solver).filter(|_| index < ROWS as u32) {
            Some(h) => h.rows(self.mem, index as usize, |r| r.head.body),
            None => CollisionStore::solver_contact_body(&*self.mem, solver, index),
        }
    }

    fn open_contact_row(&mut self, rows: u32, body: u32) {
        match self.contact_rows(rows) {
            Some((h, k)) => h.rows(self.mem, k, |r| {
                r.head.body = body;
                r.head.count = 0;
            }),
            None => open_rows_header(self.mem, rows, body),
        }
        self.set_body_contact_rows(body, rows);
    }

    fn contact_row_count(&self, rows: u32) -> u32 {
        match self.contact_rows(rows) {
            Some((h, k)) => h.rows(self.mem, k, |r| r.head.count),
            None => CollisionStore::contact_row_count(&*self.mem, rows),
        }
    }

    fn set_contact_row_count(&mut self, rows: u32, count: u32) {
        match self.contact_rows(rows) {
            Some((h, k)) => h.rows(self.mem, k, |r| r.head.count = count),
            None => CollisionStore::set_contact_row_count(self.mem, rows, count),
        }
    }

    fn store_contact_row(&mut self, rows: u32, index: u32, row: &ContactRow) {
        match self.contact_rows(rows).filter(|_| index < CONTACT_ROWS as u32) {
            Some((h, k)) => {
                h.rows(self.mem, k, |r| r.rows[index as usize] = ContactRowObj::of(row));
                row.composed.store(self.mem, ContactRows::row_at(rows, index as usize) + CONTACT_ROW_COMPOSED);
            }
            None => CollisionStore::store_contact_row(self.mem, rows, index, row),
        }
    }

    fn solver_offset_count(&self, solver: u32) -> u32 {
        match self.solver(solver) {
            Some(h) => h.typed.borrow().offset_count,
            None => CollisionStore::solver_offset_count(&*self.mem, solver),
        }
    }

    fn set_solver_offset_count(&mut self, solver: u32, count: u32) {
        match self.solver(solver) {
            Some(h) => h.typed.borrow_mut().offset_count = count,
            None => CollisionStore::set_solver_offset_count(self.mem, solver, count),
        }
    }

    fn offset_row_body(&self, row: u32) -> u32 {
        match self.offset_row(row) {
            Some((h, k)) => h.offset_row(self.mem, k, |r| r.body),
            None => CollisionStore::offset_row_body(&*self.mem, row),
        }
    }

    fn offset_row_count(&self, row: u32) -> u32 {
        match self.offset_row(row) {
            Some((h, k)) => h.offset_row(self.mem, k, |r| r.count),
            None => CollisionStore::offset_row_count(&*self.mem, row),
        }
    }

    fn set_offset_row_count(&mut self, row: u32, count: u32) {
        match self.offset_row(row) {
            Some((h, k)) => h.offset_row(self.mem, k, |r| r.count = count),
            None => CollisionStore::set_offset_row_count(self.mem, row, count),
        }
    }

    fn offset(&self, row: u32, index: u32) -> Vec4 {
        match self.offset_row(row).filter(|_| index < 8) {
            Some((h, k)) => h.offset_row(self.mem, k, |r| r.offsets[index as usize]),
            None => CollisionStore::offset(&*self.mem, row, index),
        }
    }

    fn store_offset(&mut self, row: u32, index: u32, value: Vec4) {
        match self.offset_row(row).filter(|_| index < 8) {
            Some((h, k)) => h.offset_row(self.mem, k, |r| r.offsets[index as usize] = value),
            None => CollisionStore::store_offset(self.mem, row, index, value),
        }
    }

    fn open_offset_row(&mut self, row: u32, body: u32) {
        match self.offset_row(row) {
            Some((h, k)) => {
                h.offset_row(self.mem, k, |r| r.body = body);
                self.mem.set_u32(body + BODY_OFFSET_ROW, row);
                h.offset_row(self.mem, k, |r| r.count = 1);
            }
            None => CollisionStore::open_offset_row(self.mem, row, body),
        }
    }

    fn take_body_grid_flags(&mut self, body: u32) -> (u32, u32) {
        let grid = Body::new(body).grid(self.mem);
        let [first, second] = self.hits(grid);
        self.set_hits(grid, [0, 0]);
        (u32::from(first), u32::from(second))
    }

    fn take_grid_hit_flags(&mut self, grid: u32) -> [u32; 2] {
        let hits = self.hits(grid);
        self.set_hits(grid, [0, 0]);
        hits.map(u32::from)
    }

    fn store_grid_sample_bytes(&mut self, grid: u32, bytes: [u8; 2]) {
        self.set_hits(grid, bytes);
    }

    fn cell_first(&self, cell: u32) -> u32 {
        self.links().map_or_else(|| self.mem.u32(cell), |l| l.first(self.mem, cell))
    }

    fn node_next(&self, node: u32) -> u32 {
        self.links().map_or_else(|| self.mem.u32(node), |l| l.next(self.mem, node))
    }

    fn copy_grid(&mut self, grid: u32, src: u32) {
        if self.grid(grid).is_none() && self.grid(src).is_none() {
            return CollisionStore::copy_grid(self.mem, grid, src);
        }
        // The grid's bytes, the image's around the held ones (two grids never overlap, so the parts copy as one).
        let hits_at = GridHitsObj::hits.at;
        let (after, words) = (hits_at + 2, hits_at + 4);
        self.mem.copy(grid, src, hits_at as usize);
        let hits = self.hits(src);
        self.set_hits(grid, hits);
        for k in after..words {
            self.mem.set_u8(grid + k, self.mem.u8(src + k));
        }
        self.mem.copy(grid + words, src + words, ColGrid::SIZE - words as usize);
    }

    fn clear_ray_seeds(&mut self, object: u32) {
        if self.grid(object).is_none() {
            return CollisionStore::clear_ray_seeds(self.mem, object);
        }
        // The seeds' clears in the original's order; the sample bytes are the held grid's.
        self.set_hits(object, [0, 0]);
        for field in [0x680, 0x400, 0x408, 0x40c, 0x410] {
            self.mem.set_u32(object + field, 0);
        }
        for field in [0x68b, 0x68c, 0x68d, 0x68e] {
            self.mem.set_u8(object + field, 0);
        }
    }
}

/// `lower(lift(image))` of the collision state, every row lifted, into an empty image, compared with `mem` on every
/// owned byte: their count, or the first byte that differs.
pub fn round_trip(mem: &Mem) -> Result<usize, String> {
    let held = CarCollision::lift_all(mem);
    let mut out = Mem::new();
    held.lower(&mut out);
    let mut bytes = 0;
    for (range, name) in held.image.owned() {
        let len = (range.end - range.start) as usize;
        let (want, got) = (mem.read(range.start, len), out.read(range.start, len));
        if let Some(k) = want.iter().zip(&got).position(|(a, b)| a != b) {
            return Err(format!("{name} at {:#010x}: +{k:#x} differs after lower(lift)", range.start));
        }
        bytes += len;
    }
    Ok(bytes + CellTable::round_trip(mem)? + CellLinks::round_trip(mem)?)
}

/// The held state's host-free entries in a row on `store`, for [`differential`]: the offset rows (two bodies' offsets
/// and their application), the contact rows (opened, a row stored, counted, read as the rigid body's contact, cleared),
/// and the car's grids' bytes (stored, taken by the event builder and by the body's hit, copied, the seeds' clears).
/// `car` is a body with a grid, `scratch` 0x800 bytes of free memory. Returns what the callers are given.
fn entries<S: CollisionStore>(
    store: &mut S,
    solver: u32,
    car: u32,
    scratch: u32,
    contact: impl Fn(&S, u32) -> Contact,
) -> Vec<u32> {
    use crate::collision::solver_pass::{add_body_offset, apply_offsets, clear_contact_rows};
    let mut out = Vec::new();
    let body = Body::new(car);
    let offset = Vec4 { x: 0.25, y: -1.5, z: 3.0, w: 7.0 };
    for k in 0..3 {
        out.push(u32::from(add_body_offset(store, solver, car, offset.scale(k as f32 + 1.0))));
    }
    out.push(store.solver_offset_count(solver));
    let row = store.solver_offset_row(solver, 0);
    out.extend([store.offset_row_body(row), store.offset_row_count(row)]);
    apply_offsets(store, row);
    store.set_solver_offset_count(solver, 0);
    let rows = store.solver_contact_row(solver, 1);
    store.set_solver_contact_count(solver, 2);
    store.open_contact_row(rows, car);
    let first = store.solver_contact_row(solver, 0);
    store.open_contact_row(first, car);
    let row = ContactRow {
        suppressed: 1,
        tag: 7,
        ids: [3, 4],
        restitution: [0.5, f32::from_bits(0x7fc0_0001)],
        friction: [0.75, 0.125],
        point: offset,
        plane: offset.scale(-1.0),
        composed: offset.scale(2.0),
        other: offset.scale(0.5),
    };
    for j in 0..3 {
        let count = store.contact_row_count(rows);
        store.store_contact_row(rows, count, &ContactRow { tag: j, ..row });
        store.set_contact_row_count(rows, count + 1);
    }
    out.push(store.contact_row_count(rows));
    let c = contact(store, rows + CONTACT_ROW_FIRST + CONTACT_ROW_SIZE);
    out.extend([c.id, c.stiffness.to_bits(), c.force_cap.to_bits(), c.vector.x.to_bits(), c.normal.z.to_bits()]);
    out.push(store.solver_contact_body(solver, 1));
    out.push(clear_contact_rows(store, solver));
    let (grid, other) = (col_grid_at(1), col_grid_at(2));
    store.store_grid_sample_bytes(grid, [0x12, 0x34]);
    out.extend(store.take_grid_hit_flags(grid));
    store.store_grid_sample_bytes(body.grid(store.mem()), [0x56, 0x78]);
    let (a, b) = store.take_body_grid_flags(car);
    out.extend([a, b]);
    store.store_grid_sample_bytes(other, [0x9a, 0xbc]);
    store.copy_grid(scratch, other);
    store.copy_grid(grid, scratch);
    store.clear_ray_seeds(other);
    out.extend(store.take_grid_hit_flags(grid));
    out
}

/// [`entries`] on the solver and the car at `car` (a body with a grid), per call on `per_call` (every word on the
/// image) and on the state held over the row on `held` (lifted before, lowered after), two copies of one image;
/// `scratch` is free memory of both (0x800 bytes). Compares every byte of the solver, the car's grids, the car's
/// solid words the rows link and move, and the scratch, and what the callers were given, then the cell grid's lookups
/// (`cell_table::differential`): the bytes compared, or the first that differs.
pub fn differential(per_call: &mut Mem, held: &mut Mem, car: u32, scratch: u32) -> Result<usize, String> {
    let image = CollisionImage::of(per_call);
    if image.solver == 0 {
        return Ok(0);
    }
    let state = CarCollision::lift(held);
    let load = |mem: &Mem, at| crate::rigid_body::layout::load_contact(mem, at);
    let a = entries(per_call, image.solver, car, scratch, |m, at| load(m, at));
    let b = entries(&mut state.view(held), image.solver, car, scratch, |v, at| {
        state.contact(v.mem(), at).expect("a held row")
    });
    if a != b {
        let k = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
        return Err(format!("the callers differ at word {k}: per call {:x?}, held {:x?}", a.get(k), b.get(k)));
    }
    state.lower(held);
    let grids = (col_grid_at(1), ColGrid::SIZE as u32 * CAR_GRIDS as u32, "car grids");
    let mut bytes = 0;
    for (at, len, name) in [
        (image.solver, image.offset_row_at(ROWS) - image.solver, "solver"),
        grids,
        (car, 0x400, "car"),
        (scratch, 0x800, "scratch"),
    ] {
        let (want, got) = (per_call.read(at, len as usize), held.read(at, len as usize));
        if let Some(i) = want.iter().zip(&got).position(|(x, y)| x != y) {
            return Err(format!("{name} +{i:#x} differs: per call {:#04x}, held {:#04x}", want[i], got[i]));
        }
        bytes += len as usize;
    }
    let bytes = bytes + super::cell_table::differential(per_call, held, scratch)?;
    Ok(bytes + super::cell_links::differential(per_call, held)?)
}

#[cfg(test)]
mod tests {
    use super::super::solver::SOLVER_CONTACT_COUNT;
    use super::*;
    use crate::store::check_table;

    #[test]
    fn tables_are_ordered_and_fit() {
        check_table::<ContactRowObj>();
        check_table::<RowsHeadObj>();
        check_table::<OffsetRowObj>();
        check_table::<ContactCountObj>();
        check_table::<OffsetCountObj>();
        check_table::<GridHitsObj>();
        assert_eq!(CONTACT_ROW_FIRST + CONTACT_ROW_SIZE * CONTACT_ROWS as u32, SOLVER_CONTACT_ROW);
        assert_eq!(OffsetRowObj::SIZE, SOLVER_OFFSET_ROW);
        assert_eq!(SOLVER_CONTACT_ROWS + SOLVER_CONTACT_ROW * ROWS as u32, SOLVER_OFFSET_COUNT);
        assert_eq!(ContactCountObj::contact_count.at, SOLVER_CONTACT_COUNT);
    }
}
