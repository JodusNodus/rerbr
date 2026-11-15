//! Rows carried along the frame path between two nodes, as values: the pair walk (`walk::run`) over a local copy of
//! the rows, each frame on the way applying its owner's map to them. Nothing is written while the walk runs; the
//! caller stores the rows where they belong when every frame on the path was known.

use super::frame::Hook;
use super::layout::{GRID_FIRST_BOXES, GRID_SECOND_BOXES};
use super::walk::{self, Node, Walk, Walker};
use crate::math::vec::Vec3;
use crate::rigid_body::layout::Vec4;
use crate::rigid_body::rows::RowMap;

/// What a frame does to the rows passing through it.
pub enum Passage {
    /// Nothing: the root, a frame of depth 0, or an owner that keeps the rows.
    Unchanged,
    /// Its owner body's map.
    Body(RowMap),
    /// A car body's point scatter: its base subtracted from (`sub`) or added to each point's `x, y, z`.
    Points { base: Vec3, sub: bool },
}

/// The frames a carry walks: the node reads and what each frame does.
pub trait Frames {
    /// What the walk keeps of where it is while it runs a child walk.
    type Mark;
    fn node(&self, at: u32) -> Node;
    /// What `hook` of the frame at `at` does to the rows; `None` when it is not known.
    fn passage(&mut self, at: u32, hook: Hook, rows: u32, count: u32) -> Option<Passage>;
    /// A child walk `which` of `receiver` against `o2` with `(a, b)` starts; `ascend` follows it with the mark.
    fn descend(&mut self, which: Walker, receiver: u32, o2: u32, a: u32, b: u32) -> Self::Mark;
    fn ascend(&mut self, mark: Self::Mark);
}

/// Rows the walk passes to the hooks at `at` (the hooks' first argument).
struct Rows<'r> {
    at: u32,
    rows: &'r mut [Vec4],
}

struct Carry<'a, 'r, F> {
    frames: &'a mut F,
    tables: &'a mut [Rows<'r>],
    /// The grid's first count, for the grid walks (`walk::grid_hooks`): the hooks leave it as it is.
    count: u32,
    known: bool,
}

impl<F: Frames> Walk for Carry<'_, '_, F> {
    fn node(&self, at: u32) -> Node {
        self.frames.node(at)
    }

    /// The frame's map on the first `count` rows at `at` (a walk's second argument is its row count).
    fn hook(&mut self, at: u32, hook: Hook, rows: u32, count: u32) {
        if !self.known {
            return;
        }
        let rows_at = rows;
        let rows = self.tables.iter_mut().find(|t| t.at == rows).and_then(|t| t.rows.get_mut(..count as usize));
        match (self.frames.passage(at, hook, rows_at, count), rows) {
            (Some(Passage::Unchanged), Some(_)) => {}
            (Some(Passage::Body(map)), Some(rows)) => map.apply(rows),
            (Some(Passage::Points { base, sub }), Some(rows)) => crate::car_body::body::shift_points(base, rows, sub),
            _ => self.known = false,
        }
    }

    fn walk(&mut self, which: Walker, receiver: u32, o2: u32, a: u32, b: u32) -> u32 {
        let mark = self.frames.descend(which, receiver, o2, a, b);
        let result = walk::run(self, which, receiver, o2, a, b);
        self.frames.ascend(mark);
        result
    }

    /// No walker copies; the callers of a carry read the rows before it.
    fn copy_vec4(&mut self, _to: u32, _from: u32) {
        self.known = false;
    }

    fn grid_count(&self, _grid: u32) -> u32 {
        self.count
    }
}

/// Carries `rows` from `receiver`'s frame to `o2`'s with the dispatcher `route` (one of the three frame-hook
/// dispatchers), as its walk on `(rows_at, rows.len())` would in place. False when a frame on the path is not known:
/// `rows` are then not to be used.
pub fn carry<F: Frames>(
    frames: &mut F,
    route: Walker,
    receiver: u32,
    o2: u32,
    rows_at: u32,
    rows: &mut [Vec4],
) -> bool {
    debug_assert!(matches!(route, Walker::RoutePairOneTwo | Walker::RoutePairThreeFour | Walker::RoutePairFiveSix));
    let count = rows.len() as u32;
    let mut tables = [Rows { at: rows_at, rows }];
    let mut carry = Carry { frames, tables: &mut tables, count: 0, known: true };
    carry.walk(route, receiver, o2, rows_at, count);
    carry.known
}

/// Carries the box tables of the contact grid at `grid` (its first `first.len()` boxes and as many second boxes, the
/// grid's first count) from `receiver`'s frame to `o2`'s, as the grid walk (`RoutePairGrids` on `(grid, 1)`) would in
/// place. False when a frame on the path is not known.
pub fn carry_grid<F: Frames>(
    frames: &mut F,
    receiver: u32,
    o2: u32,
    grid: u32,
    first: &mut [Vec4],
    second: &mut [Vec4],
) -> bool {
    debug_assert_eq!(first.len(), second.len());
    let count = first.len() as u32;
    let mut tables =
        [Rows { at: grid + GRID_FIRST_BOXES, rows: first }, Rows { at: grid + GRID_SECOND_BOXES, rows: second }];
    let mut carry = Carry { frames, tables: &mut tables, count, known: true };
    carry.walk(Walker::RoutePairGrids, receiver, o2, grid, 1);
    carry.known
}
