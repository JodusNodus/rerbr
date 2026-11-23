//! `route_N/trees.bin`: the trees and bushes a route places. Little endian.
//!
//! ```text
//! 0x00  u32 0, u32 12, u32 1
//! 0x0c  f32 x6        world box: min x y z, max x y z
//! 0x24  u32 types, u32 items, u32 0
//! 0x30  u32 type table (0x40), u32 types, u32 item table, u32 items
//! type  u32 name offset, u32 index, f32 x6 local box of the type's mesh, u32 0, u32 items of this type   (40 bytes)
//! item  u32 type, u32 running index, f32 x9 the 3x3 (rows are the axes, scale included), f32 x3 position,
//!       f32 NaN, f32 1, u32 0, u32 0, u32 tag, u32 -1                                                       (80 bytes)
//! name  NUL terminated, a `trees.pssg` root's id without its ` Root` (`gre_olive_tree_01_b`)
//! ```
//! The item's matrix places the type's mesh (its own space, y up) in the world's frame like an `objects.ens`
//! transform. Layout read off Greece route_0 (120 types, 13394 items) and checked by the install tests: the counts, the
//! boxes and the placements on the terrain.
use crate::objects::Mat;

#[derive(Debug, Clone, PartialEq)]
pub struct Type {
    pub name: String,
    /// The mesh's box in its own space, `[min x, y, z, max x, y, z]`.
    pub bounds: [f32; 6],
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: usize,
    pub matrix: Mat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trees {
    /// The world's box, `[min x, y, z, max x, y, z]`.
    pub bounds: [f32; 6],
    pub types: Vec<Type>,
    pub items: Vec<Item>,
}

const TYPE_SIZE: usize = 40;
const ITEM_SIZE: usize = 80;

pub fn parse(bytes: &[u8]) -> Result<Trees, String> {
    let u32_at = |at: usize| {
        bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or("trees.bin: truncated")
    };
    let f32_at = |at: usize| u32_at(at).map(f32::from_bits);
    let boxes = |at: usize| -> Result<[f32; 6], String> {
        let mut b = [0.; 6];
        for (k, v) in b.iter_mut().enumerate() {
            *v = f32_at(at + 4 * k)?;
        }
        Ok(b)
    };
    let (types, items) = (u32_at(0x24)? as usize, u32_at(0x28)? as usize);
    let (type_table, item_table) = (u32_at(0x30)? as usize, u32_at(0x38)? as usize);
    if u32_at(0x34)? as usize != types || u32_at(0x3c)? as usize != items || u32_at(4)? != 12 {
        return Err("trees.bin: header counts disagree".into());
    }
    let mut out = Trees { bounds: boxes(0x0c)?, types: Vec::new(), items: Vec::new() };
    for k in 0..types {
        let at = type_table + k * TYPE_SIZE;
        let name_at = u32_at(at)? as usize;
        let name =
            bytes.get(name_at..).and_then(|b| b.split(|&c| c == 0).next()).ok_or("trees.bin: name past the end")?;
        out.types.push(Type {
            name: String::from_utf8_lossy(name).into_owned(),
            bounds: boxes(at + 8)?,
            count: u32_at(at + 36)?,
        });
    }
    for k in 0..items {
        let at = item_table + k * ITEM_SIZE;
        let kind = u32_at(at)? as usize;
        if kind >= types {
            return Err(format!("trees.bin: item {k} is of type {kind} of {types}"));
        }
        let f = |i: usize| f32_at(at + 8 + 4 * i);
        let mut matrix = [[0., 0., 0., 0.], [0., 0., 0., 0.], [0., 0., 0., 0.], [0., 0., 0., 1.]];
        for (i, row) in matrix.iter_mut().enumerate().take(4) {
            for (j, v) in row.iter_mut().enumerate().take(3) {
                *v = f(3 * i + j)?;
            }
        }
        out.items.push(Item { kind, matrix });
    }
    Ok(out)
}

pub fn read(route: &crate::install::Route) -> Result<Trees, String> {
    let path = route.dir.join("trees.bin");
    parse(&std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?)
}
