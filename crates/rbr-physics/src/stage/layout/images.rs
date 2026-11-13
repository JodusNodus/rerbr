//! The words the load reads from the relocated `.col` image and the `.mat` image.

use crate::stage::{ColRoot, MaterialMap, MeshList};

/// The `.col` file header: the root object, the cell count and the cell table.
const COL_ROOT: u32 = 0x4;
const COL_CELL_COUNT: u32 = 0x8;
const COL_CELL_TABLE: u32 = 0xC;
/// A `.col` object: its vertex slot (a `u16`), its vertex count (named by use: 0 for an object without vertices) and
/// the offset of its vertices from the object.
const OBJECT_SLOT: u32 = 0x2;
const OBJECT_VERTEX_COUNT: u32 = 0x8;
const OBJECT_VERTICES: u32 = 0xC;
/// The root object's header (mode 3): offsets from the root of its tree and brake-wall definition, and the wet-surface
/// and water mesh lists (count, offset).
const ROOT_TREE: u32 = 0x10;
const ROOT_WALL_DEF: u32 = 0x14;
const ROOT_WET_COUNT: u32 = 0x18;
const ROOT_WET: u32 = 0x1C;
const ROOT_WATER_COUNT: u32 = 0x20;
const ROOT_WATER: u32 = 0x24;

/// Bytes per water-mesh record: four corners of four floats.
pub const MESH_RECORD: u32 = 0x40;

/// The `.mat` header's entry count, and the first entry after it. An entry is a NUL-terminated name, a run count and
/// its runs.
const MAT_ENTRY_COUNT: u32 = 0x0;
const MAT_ENTRIES: u32 = 0x4;
/// A `.mat` entry's run count word, after the name's NUL.
const ENTRY_RUN_COUNT: u32 = 1;
/// A `.mat` run: the width word, the height word and 256 cell bytes.
const RUN_SIZE: u32 = 0x108;
const RUN_WIDTH: u32 = 0x0;
const RUN_HEIGHT: u32 = 0x4;
const RUN_CELLS: u32 = 0x8;

fn word(image: &[u8], at: u32) -> u32 {
    let at = at as usize;
    u32::from_le_bytes(image[at..at + 4].try_into().unwrap())
}

/// The root object's header, from the file header.
pub fn col_root(image: &[u8]) -> ColRoot {
    let root = word(image, COL_ROOT);
    let field = |off| word(image, root + off);
    let def = field(ROOT_WALL_DEF);
    ColRoot {
        tree: root + field(ROOT_TREE),
        wall_def: (def != 0).then_some(root + def),
        wet: MeshList { count: field(ROOT_WET_COUNT), at: root + field(ROOT_WET) },
        water: MeshList { count: field(ROOT_WATER_COUNT), at: root + field(ROOT_WATER) },
    }
}

/// The vertex slots a relocated image's objects took: the root, then the cells in table order, each with vertices
/// holding its slot. Each gives `(slot, offset of its vertices)`, which the collision system's slot table keeps.
pub fn vertex_slots(image: &[u8]) -> Vec<(u32, u32)> {
    let (count, table) = (word(image, COL_CELL_COUNT), word(image, COL_CELL_TABLE));
    std::iter::once(word(image, COL_ROOT))
        .chain((0..count).map(|i| word(image, table + 4 * i)))
        .filter(|&object| word(image, object + OBJECT_VERTEX_COUNT) != 0)
        .map(|object| {
            let slot = (object + OBJECT_SLOT) as usize;
            let slot = u16::from_le_bytes([image[slot], image[slot + 1]]);
            (u32::from(slot), object + word(image, object + OBJECT_VERTICES))
        })
        .collect()
}

/// On a `.mat` image: the runs of the first entry whose name holds both `wear` and `wetness` (the `wear` and
/// `wetness` words), or `None` when no entry does (then nothing is emitted).
pub fn select_maps(image: &[u8], wear: &[u8], wetness: &[u8]) -> Option<Vec<MaterialMap>> {
    let contains = |name: &[u8], word: &[u8]| name.windows(word.len()).any(|w| w == word);
    let mut at = MAT_ENTRIES;
    for _ in 0..word(image, MAT_ENTRY_COUNT) {
        let len = image[at as usize..].iter().position(|&b| b == 0).unwrap() as u32;
        let name = &image[at as usize..(at + len) as usize];
        let runs = word(image, at + len + ENTRY_RUN_COUNT);
        at += len + ENTRY_RUN_COUNT + 4;
        let maps: Vec<_> = (0..runs)
            .map(|j| {
                let run = at + j * RUN_SIZE;
                MaterialMap {
                    width: image[(run + RUN_WIDTH) as usize],
                    height: image[(run + RUN_HEIGHT) as usize],
                    cells: run + RUN_CELLS,
                }
            })
            .collect();
        at += runs * RUN_SIZE;
        if contains(name, wear) && contains(name, wetness) {
            return Some(maps);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_maps_takes_the_first_match() {
        let mut image = 2u32.to_le_bytes().to_vec();
        for (name, width) in [(&b"x dry new\0"[..], 7u8), (b"x damp new\0", 16)] {
            image.extend_from_slice(name);
            image.extend_from_slice(&1u32.to_le_bytes());
            image.extend_from_slice(&u32::from(width).to_le_bytes());
            image.extend_from_slice(&16u32.to_le_bytes());
            image.extend_from_slice(&[0; 256]);
        }
        let maps = select_maps(&image, b"new", b"damp").unwrap();
        assert_eq!(maps, [MaterialMap { width: 16, height: 16, cells: 4 + 10 + 4 + 264 + 11 + 4 + 8 }]);
        assert_eq!(select_maps(&image, b"worn", b"dry"), None);
    }
}
