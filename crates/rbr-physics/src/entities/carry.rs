//! Rows carried along the frame path between two nodes, as values: the pair walk (`walk::run`) over a local copy of
//! the rows, each frame on the way applying its owner's map to them. Nothing is written while the walk runs; the
//! caller stores the rows where they belong when every frame on the path was known.

use super::frame::Hook;
use super::walk::{self, Node, Walk, Walker};
use crate::rigid_body::layout::Vec4;
use crate::rigid_body::rows::RowMap;

/// What a frame does to the rows passing through it.
pub enum Passage {
    /// Nothing: the root, a frame of depth 0, or an owner that keeps the rows.
    Unchanged,
    /// Its owner body's map.
    Body(RowMap),
}

/// The frames a carry walks: the node reads and what each frame does.
pub trait Frames {
    /// What the walk keeps of where it is while it runs a child walk.
    type Mark;
    fn node(&self, at: u32) -> Node;
    /// What `hook` of the frame at `at` does to the rows; `None` when it is not known.
    fn passage(&mut self, at: u32, hook: Hook) -> Option<Passage>;
    /// A child walk `which` of `receiver` against `o2` with `(a, b)` starts; `ascend` follows it with the mark.
    fn descend(&mut self, which: Walker, receiver: u32, o2: u32, a: u32, b: u32) -> Self::Mark;
    fn ascend(&mut self, mark: Self::Mark);
}

struct Carry<'a, F> {
    frames: &'a mut F,
    rows: &'a mut [Vec4],
    known: bool,
}

impl<F: Frames> Walk for Carry<'_, F> {
    fn node(&self, at: u32) -> Node {
        self.frames.node(at)
    }

    /// The frame's map on the first `count` rows (a walk's second argument is its row count).
    fn hook(&mut self, at: u32, hook: Hook, _rows: u32, count: u32) {
        if !self.known {
            return;
        }
        match self.frames.passage(at, hook) {
            Some(Passage::Unchanged) => {}
            Some(Passage::Body(map)) => match self.rows.get_mut(..count as usize) {
                Some(rows) => map.apply(rows),
                None => self.known = false,
            },
            None => self.known = false,
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

    /// The grid walks carry box tables, not rows: not a carry.
    fn grid_count(&self, _grid: u32) -> u32 {
        0
    }
}

/// Carries `rows` from `receiver`'s frame to `o2`'s with the dispatcher `route` (one of the three frame-hook
/// dispatchers), as its walk on `(rows_at, rows.len())` would in place. False when a frame on the path is not known:
/// `rows` are then not to be used. `rows_at` is only passed on to `Frames::descend`.
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
    let mut carry = Carry { frames, rows, known: true };
    carry.walk(route, receiver, o2, rows_at, count);
    carry.known
}
