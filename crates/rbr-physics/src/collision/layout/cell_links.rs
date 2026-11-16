//! The cells' lists and the bodies' cell links a step holds typed (`notes/physics.md` "Store seam"), beside
//! [`CellTable`]: each cell's list head in its link block (first, last, count) and each body's link in a cell's list
//! (next, previous, the cell). A record is the three words at its address. The address stays its key and its handle
//! (`LinkRef`, `ListRef`): the image keeps it in the slots, in the other records, in the bodies and in the callers'
//! frames, and the walks hand it on. A record is held from its first write and lowered where it differs from the image
//! then; a record never written reads the image, which holds it. The fourth word (a link's body, a head's spare word)
//! only the constructors write: it stays in the image.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::ops::Range;

use super::cell_table::CellTable;
use super::typed::on_image;
use crate::car_body::links::{self, Link, LinkRef, Links, List, ListRef};
use crate::mem::Mem;

/// The heads of a link block (`LookupTwin`'s 10x10 sub-cells) and their stride.
const HEADS: u32 = 100;
const RECORD: u32 = 0x10;
/// The held words of a record.
const WORDS: u32 = 3;

/// A multiplicative hash of an address: the probe walks look a record up for every node they visit.
#[derive(Default)]
struct AddressHasher(u64);

impl Hasher for AddressHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        bytes.iter().for_each(|&b| self.write_u32(u32::from(b)));
    }

    fn write_u32(&mut self, i: u32) {
        self.0 = (self.0.rotate_left(5) ^ u64::from(i)).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

type Records = HashMap<u32, [u32; 3], BuildHasherDefault<AddressHasher>>;

/// The held records over a step: the link blocks known (for which lists are cells'), the bodies' links that were in
/// the cells' lists at the lift (owned with the blocks' heads), and every record written since, by address.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellLinks {
    /// Each link block's first head, sorted: the filled slots' at the lift and those a lookup built since.
    blocks: RefCell<Vec<u32>>,
    /// The bodies' links in the cells' lists at the lift.
    listed: Vec<u32>,
    records: RefCell<Records>,
}

impl CellLinks {
    /// The grid's link blocks and the links in their lists, from the image; nothing held yet.
    pub fn lift(mem: &Mem, table: &CellTable) -> Self {
        let mut blocks = table.blocks(mem);
        blocks.sort_unstable();
        blocks.dedup();
        let mut listed = Vec::new();
        for &block in &blocks {
            for head in (0..HEADS).map(|k| block + RECORD * k) {
                let mut link = mem.u32(head);
                // A list longer than the links that exist is a cycle: stop there.
                while link != 0 && listed.len() < 1 << 20 {
                    listed.push(link);
                    link = mem.u32(link);
                }
            }
        }
        listed.sort_unstable();
        listed.dedup();
        CellLinks { blocks: RefCell::new(blocks), listed, records: RefCell::default() }
    }

    /// Whether `head` is a cell's list head: one of a known link block's.
    pub fn is_cell(&self, head: u32) -> bool {
        let blocks = self.blocks.borrow();
        let k = blocks.partition_point(|&b| b <= head);
        k > 0 && {
            let offset = head - blocks[k - 1];
            offset < HEADS * RECORD && offset.is_multiple_of(RECORD)
        }
    }

    /// A link block `LookupTwin` built (its first head).
    pub fn add_block(&self, block: u32) {
        let mut blocks = self.blocks.borrow_mut();
        if let Err(k) = blocks.binary_search(&block) {
            blocks.insert(k, block);
        }
    }

    /// Whether the list code that moves the link at `link` to the list `to` (0 for none) runs on the held records: the
    /// list it is in and `to` are cells' (or none), and one of them is a cell or the link is held. Any other list
    /// (the car's connections, the profiler's samples) is the image's.
    pub fn moves(&self, image: &Mem, link: u32, to: u32) -> bool {
        let from = self.word(image, link, 2);
        let cell = |head| head == 0 || self.is_cell(head);
        cell(from) && cell(to) && (from != 0 || to != 0 || self.records.borrow().contains_key(&link))
    }

    /// Word `k` (0 next or first, 1 previous or last, 2 the cell or the count) of the record at `at`.
    #[inline]
    pub fn word(&self, image: &Mem, at: u32, k: u32) -> u32 {
        match self.records.borrow().get(&at) {
            Some(r) => r[k as usize],
            None => on_image(image, |m| m.u32(at + 4 * k)),
        }
    }

    /// A cell's first link.
    #[inline]
    pub fn first(&self, image: &Mem, cell: u32) -> u32 {
        self.word(image, cell, 0)
    }

    /// A link's next.
    #[inline]
    pub fn next(&self, image: &Mem, link: u32) -> u32 {
        self.word(image, link, 0)
    }

    /// The record at `at` held (lifted from `image` on its first write) with word `k` set to `value`.
    fn set(&self, image: &Mem, at: u32, k: u32, value: u32) {
        let mut records = self.records.borrow_mut();
        let r = records.entry(at).or_insert_with(|| on_image(image, |m| [0, 4, 8].map(|o| m.u32(at + o))));
        r[k as usize] = value;
    }

    /// The records held so far, by address.
    pub fn held(&self) -> Vec<u32> {
        self.records.borrow().keys().copied().collect()
    }

    /// The held records as the list code's store, the image's for the others.
    pub fn view<'a>(&'a self, image: &'a Mem) -> HeldLinks<'a> {
        HeldLinks { links: self, image }
    }

    /// Writes the held records where they differ from the image now.
    pub fn lower(&self, mem: &mut Mem) {
        for (&at, r) in self.records.borrow().iter() {
            for (k, &w) in r.iter().enumerate() {
                let word = at + 4 * k as u32;
                if mem.u32(word) != w {
                    mem.set_u32(word, w);
                }
            }
        }
    }

    /// Every record that can be held: each known block's heads and each link listed at the lift, and those written
    /// since.
    fn records_owned(&self) -> Vec<(u32, &'static str)> {
        let blocks = self.blocks.borrow();
        let heads = blocks.iter().flat_map(|&b| (0..HEADS).map(move |k| (b + RECORD * k, "cell list")));
        let mut all: Vec<_> = heads.chain(self.listed.iter().map(|&l| (l, "cell link"))).collect();
        all.extend(self.records.borrow().keys().map(|&at| (at, "cell link")));
        all.sort_unstable();
        all.dedup_by_key(|(at, _)| *at);
        all
    }

    /// The owned byte ranges, named: the three words of each record [`CellLinks::records_owned`] lists.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        self.records_owned().into_iter().map(|(at, name)| (at..at + 4 * WORDS, name.to_string())).collect()
    }

    /// `lower(lift(image))` of every owned record into an empty image, compared with `mem`: the bytes compared, or the
    /// first that differs.
    pub fn round_trip(mem: &Mem) -> Result<usize, String> {
        let links = Self::lift(mem, &CellTable::lift(mem));
        for (at, _) in links.records_owned() {
            for k in 0..WORDS {
                links.set(mem, at, k, mem.u32(at + 4 * k));
            }
        }
        let mut out = Mem::new();
        for (&at, r) in links.records.borrow().iter() {
            out.set_u32s(at, r);
        }
        compare(mem, &out, &links.owned())
    }
}

/// The first byte of `ranges` that differs between `want` and `got`, or the bytes compared.
fn compare(want: &Mem, got: &Mem, ranges: &[(Range<u32>, String)]) -> Result<usize, String> {
    let mut bytes = 0;
    for (range, name) in ranges {
        let len = (range.end - range.start) as usize;
        let (a, b) = (want.read(range.start, len), got.read(range.start, len));
        if let Some(k) = a.iter().zip(&b).position(|(x, y)| x != y) {
            return Err(format!("{name} at {:#010x}: +{k:#x} differs: {:#04x} vs {:#04x}", range.start, a[k], b[k]));
        }
        bytes += len;
    }
    Ok(bytes)
}

/// The list code's store over the held records ([`CellLinks::view`]).
pub struct HeldLinks<'a> {
    links: &'a CellLinks,
    image: &'a Mem,
}

fn optional(word: u32) -> Option<u32> {
    (word != 0).then_some(word)
}

impl Links for HeldLinks<'_> {
    fn link(&self, at: LinkRef) -> Link {
        let w = |k| optional(self.links.word(self.image, at.addr(), k));
        Link { next: w(0).map(LinkRef::new), prev: w(1).map(LinkRef::new), owner: w(2).map(ListRef::new) }
    }

    fn set_link(&mut self, at: LinkRef, link: Link) {
        self.set_next(at, link.next);
        self.set_prev(at, link.prev);
        self.links.set(self.image, at.addr(), 2, link.owner.map_or(0, ListRef::addr));
    }

    fn set_next(&mut self, at: LinkRef, next: Option<LinkRef>) {
        self.links.set(self.image, at.addr(), 0, next.map_or(0, LinkRef::addr));
    }

    fn set_prev(&mut self, at: LinkRef, prev: Option<LinkRef>) {
        self.links.set(self.image, at.addr(), 1, prev.map_or(0, LinkRef::addr));
    }

    fn list(&self, at: ListRef) -> List {
        let w = |k| self.links.word(self.image, at.addr(), k);
        List { first: optional(w(0)).map(LinkRef::new), last: optional(w(1)).map(LinkRef::new), count: w(2) }
    }

    fn set_list(&mut self, at: ListRef, list: List) {
        let at = at.addr();
        self.links.set(self.image, at, 0, list.first.map_or(0, LinkRef::addr));
        self.links.set(self.image, at, 1, list.last.map_or(0, LinkRef::addr));
        self.links.set(self.image, at, 2, list.count);
    }
}

/// `CBodyLink::Unlink` (0x4e0020, `cell_grid::unlink`) on a list store: the link out of its list, its three words
/// cleared. Returns the eax the original leaves: `previous` when both neighbours exist, the owner otherwise, 0 for an
/// unattached link.
pub fn unlink(store: &mut impl Links, link: u32) -> u32 {
    let at = LinkRef::new(link);
    let current = store.link(at);
    let eax = match (current.owner, current.next.zip(current.prev)) {
        (None, _) => 0,
        (Some(_), Some((_, prev))) => prev.addr(),
        (Some(owner), None) => owner.addr(),
    };
    links::unlink(store, at);
    eax
}

/// `cell_grid::move_to_cell` on a list store: the link out of its cell (its count down), then at the head of `cell`
/// unless it is 0 (`links::append`, the same words).
pub fn move_to_cell(store: &mut impl Links, link: u32, cell: u32) {
    links::append(store, optional(cell).map(ListRef::new), LinkRef::new(link));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grid of one cell with a block at 0x2004 (heads 0x2004.., count at 0x2000) and three links at 0x3000,
    /// 0x3010, 0x3020 in its first head's list, bodies 0xb0, 0xb1, 0xb2.
    fn image() -> Mem {
        let mut m = Mem::new();
        let grid = 0x1000;
        crate::physics_core::layout::set_cell_grid(&mut m, grid);
        super::super::CellGrid { origin_x: 0.0, origin_y: 0.0, rows: 1, columns: 2, cells: 0x1100 }.store(&mut m, grid);
        m.set_u32s(0x2000, &[0; 0x644 / 4]);
        m.set_u32(0x1100, 0x2004);
        m.set_u32(0x1104, 0);
        m.set_u32s(0x4000, &[0; 4]);
        m.set_u32(0x2000, HEADS);
        let links = [0x3000, 0x3010, 0x3020];
        for (k, &l) in links.iter().enumerate() {
            let next = links.get(k + 1).copied().unwrap_or(0);
            let prev = if k == 0 { 0 } else { links[k - 1] };
            super::super::Link { next, previous: prev, owner: 0x2004, item: 0xb0 + k as u32 }.store(&mut m, l);
        }
        m.set_u32(0x2004, 0x3000);
        m.set_u32(0x2008, 0x3020);
        m.set_u32(0x200c, 3);
        m
    }

    #[test]
    fn a_record_round_trips_and_cells_are_the_blocks_heads() {
        let m = image();
        assert_eq!(CellLinks::round_trip(&m), Ok((100 + 3) * 12));
        let links = CellLinks::lift(&m, &CellTable::lift(&m));
        assert!(links.is_cell(0x2004) && links.is_cell(0x2004 + 99 * 16));
        assert!(!links.is_cell(0x2008) && !links.is_cell(0x2004 + 100 * 16) && !links.is_cell(0x2000));
        assert!(links.moves(&m, 0x3010, 0) && links.moves(&m, 0x4000, 0x2014) && !links.moves(&m, 0x4000, 0));
        assert!(!links.moves(&m, 0x3010, 0x2008) && !links.moves(&m, 0x4000, 0x5000));
        assert_eq!(links.listed, [0x3000, 0x3010, 0x3020]);
    }

    #[test]
    fn the_unlink_eax_is_the_originals() {
        for link in [0x3000, 0x3010, 0x3020, 0x4000] {
            let (mut a, b) = (image(), image());
            let links = CellLinks::lift(&b, &CellTable::lift(&b));
            assert_eq!(unlink(&mut links.view(&b), link), super::super::unlink(&mut a, link), "{link:#x}");
        }
    }
}
