//! The grid edge sweeps: `EdgeEdge`, its pair and walk, the edge normal and the gap store.

use rbr_physics::collision::grid_edge;
use rbr_physics::collision::layout::{self};
use rbr_physics::mem::Mem;

use super::on_grids;
use crate::{Ctx, Decline, Host, Regs, Return};

/// `E37GridOps::EdgeEdge` on `grid` with `(start, end, point, distance, normal, word)`: whether the segment hit.
pub(super) fn edge_edge_at(host: &dyn Host, mem: &mut Mem, grid: u32, args: [u32; 6]) -> bool {
    on_grids(host, mem, &[grid], |s| grid_edge::edge_edge(s, grid, args))
}

/// `E37GridOps::EdgePair` on `grid` with its ten words: 1 on a hit.
pub(super) fn edge_pair_at(host: &dyn Host, mem: &mut Mem, grid: u32, args: [u32; 10]) -> u32 {
    on_grids(host, mem, &[grid], |s| layout::edge_pair(s, grid, args))
}

/// thiscall `(start, end, point, distance, normal, word)`: sweep the segment through the grid's triangle faces.
pub(super) fn edge_edge(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let args = std::array::from_fn(|i| r.stack[i]);
    let hit = edge_edge_at(ctx.host, &mut ctx.mem, r.ecx, args);
    Ok(Return::void(u32::from(hit), 0x18))
}
