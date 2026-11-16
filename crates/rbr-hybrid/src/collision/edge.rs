//! The grid edge sweeps: `EdgeEdge`, its pair and walk, the edge normal and the gap store.

use rbr_physics::collision::grid_edge;
use rbr_physics::collision::layout::{self};

use super::on_grids;
use crate::{Ctx, Decline, Regs, Return};

/// thiscall `(start, end, point, distance, normal, word)`: sweep the segment through the grid's triangle faces.
pub(super) fn edge_edge(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [start_at, end_at, point_at, distance_at, normal_at, word_at, ..] = r.stack;
    let args = [start_at, end_at, point_at, distance_at, normal_at, word_at];
    let hit = on_grids(ctx.host, &mut ctx.mem, &[r.ecx], |s| grid_edge::edge_edge(s, r.ecx, args));
    Ok(Return::void(u32::from(hit), 0x18))
}

/// thiscall on the grid with `(p2, p3, p4, p5, hit, t, normal, scratch, grid, 0)`, ret 0x28: `E37GridOps::EdgePair`
/// (0x4f8560), eax 1 on a hit.
pub(super) fn edge_pair(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let args: [u32; 10] = std::array::from_fn(|i| r.stack[i]);
    let eax = on_grids(ctx.host, &mut ctx.mem, &[r.ecx], |s| layout::edge_pair(s, r.ecx, args));
    Ok(Return::void(eax, 0x28))
}
