//! E33 CColGrid host calls and status helpers; the arithmetic of the cell grid and the samples is `cells`.

/// Start of the original's 0x6b0-byte collision-grid bank.
pub const GRID_TABLE_BANK: u32 = 0x008f_7030;

/// Calls outside CCellGrid::LookupTwin (0x4e03b0) and CCellGrid::CCellGrid (0x4e0550): `operator new` for the cell
/// block (`bytes` in, the block's address out, 0 on failure). The host owns the allocator.
pub trait AllocHost {
    fn allocate(&mut self, bytes: u32) -> u32;
}

/// The calls `E37GridOps::M6610` (0x4f6610) makes, each at its callee's entry stack pointer `sp`: `E38RaySeeds`
/// (0x4f6550) on a grid built from no source, and the copies of the grid's two blocks of `n` points in place through
/// the transform `a` (`E43PairWalk` 0x51b460, whose walk is `RoutePairOneTwo`, and its twin 0x51b500,
/// `RoutePairThreeFour`; thiscall on `a` with `(src, o2, dst, n)`).
pub trait GridOpsHost {
    fn ray_seeds(&mut self, sp: u32, grid: u32);
    fn copy_blocks_one_two(&mut self, sp: u32, a: u32, src: u32, o2: u32, dst: u32, n: u32);
    fn copy_blocks_three_four(&mut self, sp: u32, a: u32, src: u32, o2: u32, dst: u32, n: u32);
}
