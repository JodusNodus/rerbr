//! The collision entry's memory (E36): its accessors and initializer, the collect's transforms and frame.

/// E36Nop (0x4f2cd0): empty collision callback barrier.
pub fn nop() {}

pub use crate::collision::entry::{collect, CollectHost};

/// The collect's transforms through the receiver the body's slot 12 answers: the compose (`ComposeA`, 0x51b4c0) for
/// the entry's centre and the shapes' positions, and the pair walk (`E43PairWalk::MB560`, 0x51b560) for the axes.
pub const COMPOSE: u32 = 0x51B4C0;
pub const PAIR_WALK: u32 = 0x51B560;

/// The collect's frame words at `frame`: the cell query's lo and hi corners and its cells, the box and sphere test
/// inputs, and in the box's trailing words the three transformed axes and the scaled vectors (x, y, z); and the entry
/// stack pointers of the transforms, the cell query, the collision pointer and the lengths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollectFrame {
    pub cells: u32,
    pub lo: u32,
    pub hi: u32,
    pub box_input: u32,
    pub sphere_input: u32,
    pub axes: [u32; 3],
    pub scaled: [u32; 3],
    pub transform_sp: u32,
    pub query_sp: u32,
    pub pointer_sp: u32,
    pub length_sp: u32,
}

pub fn collect_frame(frame: u32) -> CollectFrame {
    let box_input = frame.wrapping_add(0x50);
    CollectFrame {
        cells: frame.wrapping_add(0x40),
        lo: frame.wrapping_add(0x28),
        hi: frame.wrapping_add(0x34),
        box_input,
        sphere_input: frame.wrapping_add(0xA0),
        axes: [box_input + 0x20, box_input + 0x30, box_input + 0x40],
        scaled: [box_input + 0x80, box_input + 0x70, box_input + 0x90],
        transform_sp: frame.wrapping_sub(16),
        query_sp: frame.wrapping_sub(16),
        pointer_sp: frame.wrapping_sub(4),
        length_sp: frame.wrapping_sub(4),
    }
}
