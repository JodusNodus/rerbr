//! Small E37 accessors of the collision grid (`ColGrid`) and of the collision-mesh descriptor (`load_mesh_desc`'s
//! `CSigCollisionMesh`), and the grid's segment-face test.

use crate::collision::triangle::segment_triangle;
use crate::math::vec::Vec3;
use crate::mem::Mem;

/// E37GridOps::T68A (0x4f5890) (name ours): the grid's status byte (+0x68a) bit 0: the descriptor's flag 4
/// (`mesh_grid::MeshGrid::flags`).
pub fn status_bit0(mem: &Mem, grid: u32) -> u32 {
    u32::from(mem.u8(grid + 0x68a) & 1)
}

/// E37GridOps::AtE8 (0x4f58a0) (name ours): the address of the grid's vertex normal `index` (+0x220, 16 bytes each).
pub fn normal_at(grid: u32, index: i32) -> u32 {
    let offset = index.wrapping_add(0x22).wrapping_mul(0x10) as u32;
    grid.wrapping_add(offset)
}

/// E37GridOps::AtE10 (0x4f58b0) (name ours): the address of the grid's vertex `index` (its low byte; +0x40, 16 bytes
/// each).
pub fn vertex_at(grid: u32, index: i32) -> u32 {
    let offset = (u32::from(index as u8) + 4) * 0x10;
    grid.wrapping_add(offset)
}

/// E37GridOps::Idx7 (0x4f58c0) (name ours): the address of the descriptor face `index` in the grid's face table
/// (+0x410, 7 bytes each).
pub fn face_at(mem: &Mem, grid: u32, index: i32) -> u32 {
    let base = mem.u32(grid + 0x410) as i32;
    base.wrapping_add(index.wrapping_mul(7)) as u32
}

/// E37GridOps::Get684 (0x4f58e0) (name ours): the grid's name (+0x684).
pub fn name(mem: &Mem, grid: u32) -> u32 {
    mem.u32(grid + 0x684)
}

/// E37GridOps::Set68A (0x4f58f0) (name ours): sets or clears bit 0 of the grid's status byte (+0x68a); eax keeps
/// `value`'s upper bytes over the byte.
pub fn set_status_bit0(mem: &mut Mem, grid: u32, value: u32) -> u32 {
    let flag = mem.u8(grid + 0x68a);
    let flag = if value != 0 { flag | 1 } else { flag & !1 };
    mem.map(grid + 0x68a, &[flag]);
    (value & 0xffff_ff00) | u32::from(flag)
}

/// E37GridOps::T158_2 (0x4f5920) (name ours): bit 1 (2) of the word at +0x158 (named by use; the object and the word's
/// meaning are not known).
pub fn flag_word_bit1(mem: &Mem, object: u32) -> u32 {
    mem.u32(object + 0x158) & 2
}

/// E37GridOps::T158_4 (0x4f5930) (name ours): bit 2 (4) of the word at +0x158 (named by use, as `flag_word_bit1`).
pub fn flag_word_bit2(mem: &Mem, object: u32) -> u32 {
    mem.u32(object + 0x158) & 4
}

/// E37GridOps::Get30 (0x4f5940) (name ours): the collision-mesh descriptor's material byte (+0x30, `load_mesh_desc`).
pub fn desc_material(mem: &Mem, desc: u32) -> u32 {
    u32::from(mem.u8(desc + 0x30))
}

/// E37GridOps::Get04 (0x4f5950) (name ours): the descriptor's flags (+4).
pub fn desc_flags(mem: &Mem, desc: u32) -> u32 {
    mem.u32(desc + 4)
}

/// E37GridOps::Set04 (0x4f5960) (name ours): stores the descriptor's flags (+4).
pub fn set_desc_flags(mem: &mut Mem, desc: u32, value: u32) {
    mem.set_u32(desc + 4, value);
}

/// E37GridOps::Get00 (0x4f5970) (name ours): the descriptor's collision id (+0).
pub fn desc_id(mem: &Mem, desc: u32) -> u32 {
    mem.u32(desc)
}

/// E37GridOps::T04_2 (0x4f5980) (name ours): the descriptor's flag 2 (+4), which the mesh factory skips the mesh for.
pub fn desc_skipped(mem: &Mem, desc: u32) -> u32 {
    mem.u32(desc + 4) & 2
}

/// E37GridOps::Valid5990 (0x4f5990) (name ours): 1 when the descriptor has vertices (+0xc) or a shape record (+0x2c),
/// the factory's build test.
pub fn desc_has_geometry(mem: &Mem, desc: u32) -> u32 {
    if mem.u16(desc + 0xc) != 0 || mem.u32(desc + 0x2c) != 0 {
        1
    } else {
        0
    }
}

/// E37GridOps::Get08 (0x4f59b0) (name ours): the descriptor's name (+8).
pub fn desc_name(mem: &Mem, desc: u32) -> u32 {
    mem.u32(desc + 8)
}

/// E37GridOps::Get0Cw (0x4f59c0) (name ours): the descriptor's vertex count (+0xc, a word).
pub fn desc_vertex_count(mem: &Mem, desc: u32) -> u32 {
    u32::from(mem.u16(desc + 0xc))
}

/// E37GridOps::Get10 (0x4f59d0) (name ours): the descriptor's vertex array (+0x10).
pub fn desc_vertices(mem: &Mem, desc: u32) -> u32 {
    mem.u32(desc + 0x10)
}

/// E37GridOps::Get0Ew (0x4f59e0) (name ours): the descriptor's face count (+0xe, a word).
pub fn desc_face_count(mem: &Mem, desc: u32) -> u32 {
    u32::from(mem.u16(desc + 0xe))
}

/// E37GridOps::Get14 (0x4f59f0) (name ours): the descriptor's face array (+0x14).
pub fn desc_faces(mem: &Mem, desc: u32) -> u32 {
    mem.u32(desc + 0x14)
}

/// E37GridOps::Get18 (0x4f5a00) (name ours): the descriptor's sampled word (+0x18; the build reads its low byte).
pub fn desc_sampled(mem: &Mem, desc: u32) -> u32 {
    mem.u32(desc + 0x18)
}

/// E37GridOps::Get1C (0x4f5a10) (name ours): the descriptor's sample data (+0x1c).
pub fn desc_sample_data(mem: &Mem, desc: u32) -> u32 {
    mem.u32(desc + 0x1c)
}

/// E37GridOps::Get20w (0x4f5a20) (name ours): the descriptor's sample width (+0x20, a word).
pub fn desc_sample_width(mem: &Mem, desc: u32) -> u32 {
    u32::from(mem.u16(desc + 0x20))
}

/// E37GridOps::Get22w (0x4f5a30) (name ours): the descriptor's sample height (+0x22, a word).
pub fn desc_sample_height(mem: &Mem, desc: u32) -> u32 {
    u32::from(mem.u16(desc + 0x22))
}

/// E37GridOps::Get24 (0x4f5a40) (name ours): the descriptor's first sample table (+0x24).
pub fn desc_sample_table_a(mem: &Mem, desc: u32) -> u32 {
    mem.u32(desc + 0x24)
}

/// E37GridOps::Get28 (0x4f5a50) (name ours): the descriptor's second sample table (+0x28).
pub fn desc_sample_table_b(mem: &Mem, desc: u32) -> u32 {
    mem.u32(desc + 0x28)
}

/// E37GridOps::Get2C (0x4f5a60) (name ours): the descriptor's shape record (+0x2c).
pub fn desc_shape(mem: &Mem, desc: u32) -> u32 {
    mem.u32(desc + 0x2c)
}

/// The word at 0x8f7010, before the grid bank (named by use: these two only clear and read it).
const GLOBAL_WORD: u32 = 0x008f_7010;

/// E37Zero5A70 (0x4f5a70) (name ours): clears `GLOBAL_WORD`.
pub fn clear_global_word(mem: &mut Mem) {
    mem.set_u32(GLOBAL_WORD, 0);
}

/// E37Get5A80 (0x4f5a80) (name ours): `GLOBAL_WORD`.
pub fn global_word(mem: &Mem) -> u32 {
    mem.u32(GLOBAL_WORD)
}

/// E37Nop5CF0 (0x4f5cf0) (name ours): an empty function.
pub fn nop() {}

/// E37GridOps::Elem5F20 (0x4f5f20) (name ours): the `t` (0 < t < 1) at which the segment `start` to `end` crosses the
/// grid's face `index`, else 2.0 (a face of material 0xff, or a miss).
pub fn segment_face_t(mem: &Mem, grid: u32, index: u32, start: u32, end: u32) -> f32 {
    let table = mem.u32(grid.wrapping_add(0x410));
    let row = table.wrapping_add(index.wrapping_mul(7));
    if mem.u8(row.wrapping_add(3)) == 0xff {
        return 2.0;
    }

    let vertex = |slot: u32| grid.wrapping_add((u32::from(mem.u8(row.wrapping_add(slot))) + 4).wrapping_mul(0x10));
    let start = Vec3::load(mem, start);
    let end = Vec3::load(mem, end);
    let hit = segment_triangle(
        start,
        end - start,
        Vec3::load(mem, vertex(0)),
        Vec3::load(mem, vertex(1)),
        Vec3::load(mem, vertex(2)),
    );
    match hit.t {
        Some(t) if hit.hit && 0.0 < t && t < 1.0 => t,
        _ => 2.0,
    }
}
