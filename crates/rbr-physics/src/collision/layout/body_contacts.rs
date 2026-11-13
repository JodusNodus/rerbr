//! `CBody::VfD0` (0x4e1eb0) and `CBody::VfDC` (0x4e2850): the point, edge and pair contacts against another entity,
//! are `collision::body_contact`, generic over `CollisionStore`. Here are their frames.

pub use crate::collision::body_contact::{grid_contacts, pair_contacts};

/// The scratch frame of the point/edge walk (`VfD0`), addressed from its base `s`.
pub struct GridWalkFrame {
    s: u32,
}

/// The point/edge walk's frame below its entry stack pointer `sp`.
pub fn grid_walk_frame(sp: u32) -> GridWalkFrame {
    GridWalkFrame { s: super::aligned(sp) - 0xe90 }
}

impl GridWalkFrame {
    pub const MATERIAL: u32 = 0x1c;
    pub const HIT: u32 = 0x20;
    pub const DIR: u32 = 0x30;
    pub const NORMAL: u32 = 0x40;
    /// The normal's w lane: the plane offset through the probe's hit.
    pub const PLANE_OFFSET: u32 = 0x4c;
    pub const DESC_A: u32 = 0x5c;
    pub const U: u32 = 0x64;
    pub const T: u32 = 0x6c;
    /// The point walk's second descriptor and the point contact's midpoint.
    pub const DESC_B: u32 = 0x70;
    pub const FROM: u32 = 0x80;
    /// The edge contact's midpoint.
    pub const EDGE_MID: u32 = 0x90;
    pub const HIT2: u32 = 0xa0;
    pub const BACK: u32 = 0xb0;
    pub const AT: u32 = 0xc0;
    pub const PROBE: u32 = 0xd0;
    pub const NORMAL2: u32 = 0xe0;
    pub const HIT_SPAN: u32 = 0xf0;
    pub const EDGE_SPAN: u32 = 0x100;
    /// This body's grid in the other entity's frame.
    pub const GRID: u32 = 0x110;
    /// The other entity's grid in its own frame.
    pub const OTHER_GRID: u32 = 0x7c0;

    pub fn at(&self, offset: u32) -> u32 {
        self.s + offset
    }

    /// The entry stack pointer of a call with `words` words pushed below the frame.
    pub fn call_sp(&self, words: u32) -> u32 {
        self.s - 4 * words
    }
}

/// The scratch frame of the pair walk (`VfDC`), addressed from its base `s`.
pub struct PairWalkFrame {
    s: u32,
}

/// The pair walk's frame below its entry stack pointer `sp`.
pub fn pair_walk_frame(sp: u32) -> PairWalkFrame {
    PairWalkFrame { s: super::aligned(sp) - PairWalkFrame::SIZE }
}

impl PairWalkFrame {
    const SIZE: u32 = 0x14e0;
    /// The pair test's `t` and the word after it, which the test reads but does not write.
    pub const T: u32 = 0x14;
    pub const SCRATCH: u32 = 0x18;
    /// The overlap box: its corners, centre and half extent, and the w lanes of the centre and the half extent.
    pub const HI: u32 = 0x20;
    pub const LO: u32 = 0x30;
    pub const CENTRE: u32 = 0x40;
    pub const CENTRE_W: u32 = 0x4c;
    pub const HIT: u32 = 0x50;
    pub const HALF: u32 = 0x60;
    pub const HALF_W: u32 = 0x6c;
    pub const NORMAL: u32 = 0x70;
    /// The two fetched grids' boxes, joined.
    pub const UNION_LO: u32 = 0x80;
    pub const UNION_HI: u32 = 0x90;
    pub const AT: u32 = 0xa0;
    /// This body's grid fetched through `b` and through `a`, and the other entity's copied grid.
    pub const GRID_B: u32 = 0xb0;
    pub const GRID_A: u32 = 0x760;
    pub const OTHER_GRID: u32 = 0xe10;

    pub fn at(&self, offset: u32) -> u32 {
        self.s + offset
    }

    /// The entry stack pointer of a call with `words` words pushed below the frame.
    pub fn call_sp(&self, words: u32) -> u32 {
        self.s - 4 * words
    }
}
