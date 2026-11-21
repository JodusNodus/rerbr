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
    /// block in the image and filling the held slot (`built` gets the block); `None` for a grid that is not the held
    /// one.
    pub fn link_or_build(
        &self,
        host: &mut impl AllocHost,
        mem: &mut Mem,
        grid: u32,
        x: f32,
        y: f32,
        built: impl FnOnce(u32),
    ) -> Option<u32> {
        Some(match self.link_at(mem, grid, x, y)? {
            CellLink::Link(link) => link,
            CellLink::Empty(twin) => {
                let block = new_link_block(host, mem);
                self.slots.borrow_mut().insert(twin.cell, (block, true));
                built(block);
                block.wrapping_add(twin.sub_cell.wrapping_mul(0x10) as u32)
            }
        })
    }

    /// The link blocks the image's slots point to, in cell order.
    pub(super) fn blocks(&self, mem: &Mem) -> Vec<u32> {
        if self.grid == 0 {
            return Vec::new();
        }
        let cells = self.header.rows.wrapping_mul(self.header.columns).max(0) as usize;
        let words = mem.read(self.slot_at(0), 4 * cells);
        words.chunks(4).map(|w| u32::from_le_bytes(w.try_into().unwrap())).filter(|&b| b != 0).collect()
    }

    /// Writes the slots the lookups filled back to the image.
    pub fn lower(&self, mem: &mut Mem) {
        for (&cell, &(block, filled)) in self.slots.borrow().iter() {
            if filled {
                mem.set_u32(self.slot_at(cell), block);
            }
        }
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
}

/// `f` on `mem` without the poison of a check build (a lift reads the owned words).
fn on_image<T>(mem: &Mem, f: impl FnOnce(&Mem) -> T) -> T {
    #[cfg(feature = "poison")]
    return f(&mem.unpoisoned());
    #[cfg(not(feature = "poison"))]
    f(mem)
}
