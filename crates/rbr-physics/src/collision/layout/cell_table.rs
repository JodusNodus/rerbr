//! The cell grid's table a step holds typed (`notes/physics.md` "Store seam"): the `CCellGrid` header and its cell
//! slots (one pointer to a cell's 10x10 link block per 100 units, 0 until `LookupTwin` first asks for the cell). The
//! table is as large as the stage (a word per cell) and a step asks for the few cells around the car, so a slot is
//! lifted when a step first asks for it and only the slots a lookup filled are lowered. The link blocks, the bodies'
//! links and the cell lists stay in the image.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;

use super::cell_grid::{new_link_block, CellGrid};
use super::grid_hosts::AllocHost;
use crate::collision::cells::{self, CellMap, PointCell};
use crate::mem::Mem;
use crate::physics_core::layout::cell_grid;

/// A lookup on the held table: the point's link, or its cell and sub-cell when the cell has no link block yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellLink {
    Link(u32),
    Empty(PointCell),
}

/// The world's cell grid over a step: its address (0 without one), its header and the slots lifted so far, each with
/// whether a lookup filled it since.
#[derive(Clone, Debug, PartialEq)]
pub struct CellTable {
    grid: u32,
    header: CellGrid,
    slots: RefCell<HashMap<i32, (u32, bool)>>,
}

impl CellTable {
    /// The world's cell grid (the grid global's), its header lifted from the image.
    pub fn lift(mem: &Mem) -> Self {
        let grid = cell_grid(mem);
        let header = match grid {
            0 => CellGrid { origin_x: 0.0, origin_y: 0.0, rows: 0, columns: 0, cells: 0 },
            _ => CellGrid::load(mem, grid),
        };
        CellTable { grid, header, slots: RefCell::default() }
    }

    fn slot_at(&self, cell: i32) -> u32 {
        self.header.cells.wrapping_add(cell.wrapping_mul(4) as u32)
    }

    /// The link of the point `(x, y)` on the grid at `grid` (`CCellGrid::LookupTwin` without its lazy build), its slot
    /// lifted from `image` when no lookup asked for it before; `None` for a grid that is not the held one.
    pub fn link_at(&self, image: &Mem, grid: u32, x: f32, y: f32) -> Option<CellLink> {
        if grid == 0 || grid != self.grid {
            return None;
        }
        let h = self.header;
        let map = CellMap { origin_x: h.origin_x, origin_y: h.origin_y, columns: h.columns, rows: h.rows };
        let twin = cells::point_cell(map, x, y);
        let at = self.slot_at(twin.cell);
        let (block, _) =
            *self.slots.borrow_mut().entry(twin.cell).or_insert_with(|| (on_image(image, |m| m.u32(at)), false));
        Some(match block {
            0 => CellLink::Empty(twin),
            _ => CellLink::Link(block.wrapping_add(twin.sub_cell.wrapping_mul(0x10) as u32)),
        })
    }

    /// `CCellGrid::LookupTwin` (0x4e03b0) on the held table: the point's link, after lazily building its cell's link
    /// block in the image and filling the held slot; `None` for a grid that is not the held one.
    pub fn link_or_build(&self, host: &mut impl AllocHost, mem: &mut Mem, grid: u32, x: f32, y: f32) -> Option<u32> {
        Some(match self.link_at(mem, grid, x, y)? {
            CellLink::Link(link) => link,
            CellLink::Empty(twin) => {
                let block = new_link_block(host, mem);
                self.slots.borrow_mut().insert(twin.cell, (block, true));
                block.wrapping_add(twin.sub_cell.wrapping_mul(0x10) as u32)
            }
        })
    }

    /// Writes the slots the lookups filled back to the image.
    pub fn lower(&self, mem: &mut Mem) {
        for (&cell, &(block, filled)) in self.slots.borrow().iter() {
            if filled {
                mem.set_u32(self.slot_at(cell), block);
            }
        }
    }

    /// As [`CellTable::lower`] with every lifted slot (the round trip's).
    fn lower_all(&self, mem: &mut Mem) {
        for (&cell, &(block, _)) in self.slots.borrow().iter() {
            mem.set_u32(self.slot_at(cell), block);
        }
    }

    /// Every slot lifted (the round trip's).
    fn lift_all(mem: &Mem) -> Self {
        let table = Self::lift(mem);
        let cells = table.header.rows.wrapping_mul(table.header.columns);
        let slots = (0..cells.max(0)).map(|cell| (cell, (mem.u32(table.slot_at(cell)), false))).collect();
        CellTable { slots: RefCell::new(slots), ..table }
    }

    /// The owned byte ranges, named: the header and every slot (lifted or not).
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        if self.grid == 0 {
            return Vec::new();
        }
        let cells = self.header.rows.wrapping_mul(self.header.columns).max(0);
        vec![
            (self.grid..self.grid + 0x14, "cell grid".to_string()),
            (self.slot_at(0)..self.slot_at(cells), "cell grid slots".to_string()),
        ]
    }

    /// `lower(lift(image))` of the header and every slot into an empty image, compared with `mem`: the bytes
    /// compared, or the first that differs.
    pub fn round_trip(mem: &Mem) -> Result<usize, String> {
        let table = Self::lift_all(mem);
        if table.grid == 0 {
            return Ok(0);
        }
        let mut out = Mem::new();
        table.header.store(&mut out, table.grid);
        table.lower_all(&mut out);
        let mut bytes = 0;
        for (range, name) in table.owned() {
            let len = (range.end - range.start) as usize;
            let (want, got) = (mem.read(range.start, len), out.read(range.start, len));
            if let Some(k) = want.iter().zip(&got).position(|(a, b)| a != b) {
                return Err(format!("{name} at {:#010x}: +{k:#x} differs after lower(lift)", range.start));
            }
            bytes += len;
        }
        Ok(bytes)
    }
}

/// A bump allocator over free memory, for [`differential`].
struct Bump(u32);

impl AllocHost for Bump {
    fn allocate(&mut self, bytes: u32) -> u32 {
        let at = self.0;
        self.0 += bytes;
        at
    }
}

/// `LookupTwin` at the centre of every cell of a stride over the grid, twice, and of the first cell without a link
/// block (built into `scratch`, 0x644 free bytes of both), per call on `per_call` and on the table held over the
/// lookups on `held`, two copies of one image. Compares the links, the header, every slot and the built block: the
/// bytes compared, or the first difference.
pub fn differential(per_call: &mut Mem, held: &mut Mem, scratch: u32) -> Result<usize, String> {
    let table = CellTable::lift(held);
    if table.grid == 0 {
        return Ok(0);
    }
    let (grid, h) = (table.grid, table.header);
    let cells = h.rows.wrapping_mul(h.columns).max(0);
    let centre = |cell: i32| {
        let (row, column) = (cell / h.columns, cell % h.columns);
        (h.origin_x + 100.0 * column as f32 + 55.0, h.origin_y + 100.0 * row as f32 + 35.0)
    };
    let empty = (0..cells).find(|&cell| per_call.u32(table.slot_at(cell)) == 0);
    let stride = (cells / 97).max(1) as usize;
    let filled = (0..cells).step_by(stride).filter(|&cell| per_call.u32(table.slot_at(cell)) != 0);
    let points: Vec<_> = filled.chain(empty).chain(empty).map(centre).chain([(-1.0e9, 1.0e9)]).collect();
    let (mut a, mut b) = (Bump(scratch), Bump(scratch));
    for &(x, y) in points.iter().chain(&points) {
        let want = super::cell_link_at(&mut a, per_call, grid, x, y);
        let got = table.link_or_build(&mut b, held, grid, x, y);
        if got != Some(want) {
            return Err(format!("the link at ({x}, {y}): per call {want:#x}, held {got:x?}"));
        }
    }
    table.lower(held);
    let mut bytes = 0;
    let ranges = table.owned().into_iter().chain([(scratch..scratch + 0x644, "link block".to_string())]);
    for (range, name) in ranges {
        let len = (range.end - range.start) as usize;
        let (want, got) = (per_call.read(range.start, len), held.read(range.start, len));
        if let Some(k) = want.iter().zip(&got).position(|(x, y)| x != y) {
            return Err(format!("{name} +{k:#x} differs: per call {:#04x}, held {:#04x}", want[k], got[k]));
        }
        bytes += len;
    }
    Ok(bytes)
}

/// `f` on `mem` without the poison of a check build (a lift reads the owned words).
fn on_image<T>(mem: &Mem, f: impl FnOnce(&Mem) -> T) -> T {
    #[cfg(feature = "poison")]
    return f(&mem.unpoisoned());
    #[cfg(not(feature = "poison"))]
    f(mem)
}
