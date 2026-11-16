//! Small E37 accessors of the collision grid (`ColGrid`) and of the collision-mesh descriptor (`load_mesh_desc`'s
//! `CSigCollisionMesh`), and the grid's segment-face test.

use crate::mem::Mem;

/// E37GridOps::AtE8 (0x4f58a0) (name ours): the address of the grid's vertex normal `index` (+0x220, 16 bytes each).
pub fn normal_at(grid: u32, index: i32) -> u32 {
    let offset = index.wrapping_add(0x22).wrapping_mul(0x10) as u32;
    grid.wrapping_add(offset)
}

/// E37GridOps::Get684 (0x4f58e0) (name ours): the grid's name (+0x684).
pub fn name(mem: &Mem, grid: u32) -> u32 {
    mem.u32(grid + 0x684)
}

/// E37Nop5CF0 (0x4f5cf0) (name ours): an empty function.
pub fn nop() {}
