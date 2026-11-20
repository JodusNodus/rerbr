//! The typed stage's image side ([`crate::stage::typed`]): its lift from the image after the load, its owned bytes,
//! and [`StageView`], the stage's store over it. Nothing lowers it: the step only reads these tables.
//!
//! The drive line is the table `DRIVE_LINE` names, entries and count (`line + 0x10 .. line + COUNT_AT + 4`, and a longer
//! line's entries past the count, `engine_driveline::layout::drive_line`); the
//! material table is `MATERIAL_TABLE`'s slots, slot address and count, with the 256 bytes of each sheet a slot names.
//! A read the typed stage does not hold (an index past the table, another table's address, a sheet the image did not
//! have) reads the image, which a step leaves as the load did.

use std::ops::Range;

use super::{DRIVE_LINE, MATERIAL_TABLE};
use crate::engine_driveline::drive_line::DriveLinePoint;
use crate::engine_driveline::layout::{entries_held, entry_at, COUNT_AT, MAX_ENTRIES};
use crate::entities::layout::{SLOT_SIZE, TABLE_COUNT, TABLE_SELF};
use crate::entities::material_table::MaterialSlot;
use crate::entities::store::SlotLookup;
use crate::math::vec::Vec4;
use crate::mem::Mem;
use crate::ngp::layout::material::SLOT_DATA;
use crate::stage::store::StageStore;
use crate::stage::typed::{DriveLine, LineEntry, Materials, Stage};

/// A drive line entry's bytes, from `entry_at`: position, tangent, distance, tag, two spare words.
const ENTRY_BYTES: usize = 0x30;
/// The drive line's typed bytes from its address: the entries from `+0x10`, the count up to `COUNT_AT + 4`.
const LINE_START: u32 = 0x10;
const LINE_END: u32 = COUNT_AT + 4;
/// The material table's slots, and its bytes (the slots, their address and count).
const SLOTS: u32 = 0x100;
const TABLE_BYTES: u32 = TABLE_COUNT + 4;
/// A sheet's bytes.
const SHEET_BYTES: u32 = 0x100;

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn vec4(bytes: &[u8], at: usize) -> Vec4 {
    let f = |k: usize| f32::from_bits(word(bytes, at + 4 * k));
    Vec4 { x: f(0), y: f(1), z: f(2), w: f(3) }
}

fn vec4_bytes(v: Vec4) -> impl Iterator<Item = u8> {
    [v.x, v.y, v.z, v.w].into_iter().flat_map(|f| f.to_le_bytes())
}

/// A longer line's entries past the count (ours), for a table of `count` entries at `line`.
fn overflow(line: u32, count: u32) -> Option<Range<u32>> {
    let held = entries_held(count);
    (held > MAX_ENTRIES).then(|| entry_at(line, MAX_ENTRIES)..entry_at(line, held))
}

/// The drive line at `line`, from the image, if the image holds its table.
fn lift_line(mem: &Mem, line: u32) -> Option<DriveLine> {
    let bytes = mem.try_read(line.checked_add(LINE_START)?, (LINE_END - LINE_START) as usize)?;
    let count = word(&bytes, (COUNT_AT - LINE_START) as usize);
    let mut table = bytes[..MAX_ENTRIES as usize * ENTRY_BYTES].to_vec();
    if let Some(range) = overflow(line, count) {
        table.extend(mem.try_read(range.start, (range.end - range.start) as usize)?);
    }
    let entries = table
        .chunks_exact(ENTRY_BYTES)
        .map(|e| LineEntry {
            point: DriveLinePoint {
                position: vec4(e, 0),
                tangent: vec4(e, 0x10),
                distance: f32::from_bits(word(e, 0x20)),
                tag: word(e, 0x24),
            },
            spare: [word(e, 0x28), word(e, 0x2c)],
        })
        .collect();
    Some(DriveLine { entries, count })
}

/// The entries' typed bytes, in the image's order.
fn entry_bytes(entries: &[LineEntry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(entries.len() * ENTRY_BYTES);
    for e in entries {
        let p = &e.point;
        out.extend(vec4_bytes(p.position).chain(vec4_bytes(p.tangent)));
        for w in [p.distance.to_bits(), p.tag, e.spare[0], e.spare[1]] {
            out.extend(w.to_le_bytes());
        }
    }
    out
}

/// The material table at `table`, from the image, if the image holds it; each sheet a slot names, where it holds that.
fn lift_materials(mem: &Mem, table: u32) -> Option<Materials> {
    let bytes = mem.try_read(table, TABLE_BYTES as usize)?;
    let slots: Box<[MaterialSlot]> = (0..SLOTS as usize)
        .map(|k| MaterialSlot { low: word(&bytes, 8 * k), high: word(&bytes, 8 * k + SLOT_DATA as usize) })
        .collect();
    let sheets = slots
        .iter()
        .map(|s| (s.high != 0).then(|| mem.try_read(s.high, SHEET_BYTES as usize)).flatten().map(Vec::into_boxed_slice))
        .collect();
    Some(Materials {
        slots,
        slots_at: word(&bytes, TABLE_SELF as usize),
        count: word(&bytes, TABLE_COUNT as usize),
        sheets,
    })
}

fn materials_bytes(m: &Materials) -> Vec<u8> {
    let slots = m.slots.iter().flat_map(|s| s.low.to_le_bytes().into_iter().chain(s.high.to_le_bytes()));
    slots.chain(m.slots_at.to_le_bytes()).chain(m.count.to_le_bytes()).collect()
}

impl Stage {
    /// The stage the image holds after the load: the drive line `DRIVE_LINE` names and the material table, each
    /// where the image has it.
    pub fn lift(mem: &Mem) -> Self {
        let line = mem.try_read(DRIVE_LINE, 4).map(|w| word(&w, 0)).filter(|&at| at != 0);
        Stage {
            line: line.and_then(|at| Some((at, lift_line(mem, at)?))),
            materials: lift_materials(mem, MATERIAL_TABLE).map(|m| (MATERIAL_TABLE, m)),
        }
    }

    /// The byte ranges the typed stage holds, named, with its bytes for them when `bytes` (what it stands for in the
    /// image).
    fn parts(&self, bytes: bool) -> Vec<(Range<u32>, String, Vec<u8>)> {
        let mut out = Vec::new();
        if let Some((at, line)) = &self.line {
            let (table, past) = line.entries.split_at(MAX_ENTRIES as usize);
            let typed = if bytes {
                let mut typed = entry_bytes(table);
                typed.extend(line.count.to_le_bytes());
                typed
            } else {
                Vec::new()
            };
            out.push((at + LINE_START..at + LINE_END, "drive line".to_string(), typed));
            if let Some(range) = overflow(*at, line.count) {
                let typed = if bytes { entry_bytes(past) } else { Vec::new() };
                out.push((range, "drive line past entry 10000".to_string(), typed));
            }
        }
        if let Some((at, m)) = &self.materials {
            let typed = if bytes { materials_bytes(m) } else { Vec::new() };
            out.push((*at..at + TABLE_BYTES, "material table".to_string(), typed));
            for (k, (slot, sheet)) in m.slots.iter().zip(m.sheets.iter()).enumerate() {
                if let Some(sheet) = sheet {
                    let typed = if bytes { sheet.to_vec() } else { Vec::new() };
                    out.push((slot.high..slot.high + SHEET_BYTES, format!("material sheet {k}"), typed));
                }
            }
        }
        out
    }

    /// Every byte range the typed stage holds, named; overlapping sheets (two slots naming one) once.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        let mut owned: Vec<_> = self.parts(false).into_iter().map(|(r, name, _)| (r, name)).collect();
        owned.sort_by_key(|(r, _)| r.start);
        owned.dedup_by(|b, a| {
            let overlaps = b.0.start < a.0.end;
            if overlaps {
                a.0.end = a.0.end.max(b.0.end);
            }
            overlaps
        });
        owned
    }

    /// The store over the typed stage, the image for what it does not hold.
    pub fn view<'a>(&'a self, mem: &'a Mem) -> StageView<'a> {
        StageView { stage: Some(self), mem }
    }
}

/// The typed stage lifted from `mem`, compared with `mem` on every byte it holds: their count, or the first byte that
/// differs.
pub fn round_trip(mem: &Mem) -> Result<usize, String> {
    let mut bytes = 0;
    for (range, name, typed) in Stage::lift(mem).parts(true) {
        let image = mem.read(range.start, typed.len());
        if let Some(k) = (0..typed.len()).find(|&k| typed[k] != image[k]) {
            return Err(format!("{name} +{k:#x}: typed {:#04x}, image {:#04x}", typed[k], image[k]));
        }
        bytes += typed.len();
    }
    Ok(bytes)
}

/// The stage's store ([`StageStore`]) a binding runs on: the typed stage while a step holds it, the image for the
/// rest (and without one, the image alone, as `Mem`'s store).
pub struct StageView<'a> {
    stage: Option<&'a Stage>,
    mem: &'a Mem,
}

impl<'a> StageView<'a> {
    pub fn new(stage: Option<&'a Stage>, mem: &'a Mem) -> Self {
        StageView { stage, mem }
    }

    fn point(&self, line: u32, k: u32) -> Option<&'a DriveLinePoint> {
        self.stage?.line_at(line)?.point(k)
    }

    fn materials(&self, table: u32) -> Option<&'a Materials> {
        self.stage?.materials_at(table)
    }

    /// The held slot at `slot`: its table and index.
    fn slot_of(&self, slot: u32) -> Option<(&'a Materials, usize)> {
        let (at, m) = self.stage?.materials.as_ref()?;
        let offset = slot.wrapping_sub(*at);
        (offset < SLOTS * SLOT_SIZE && offset % SLOT_SIZE == 0).then_some((m, (offset / SLOT_SIZE) as usize))
    }

    /// With the `poison` feature, `typed` checked against the image's answer (`image` on the unpoisoned memory).
    #[inline(always)]
    fn checked<T: PartialEq + std::fmt::Debug>(&self, typed: T, _image: impl FnOnce(&Mem) -> T) -> T {
        #[cfg(feature = "poison")]
        {
            let image = _image(&self.mem.unpoisoned());
            assert_eq!(typed, image, "the typed stage's answer differs from the image's");
        }
        typed
    }
}

fn bits(v: Vec4) -> [u32; 4] {
    [v.x, v.y, v.z, v.w].map(f32::to_bits)
}

impl StageStore for StageView<'_> {
    fn drive_line(&self) -> u32 {
        self.mem.u32(DRIVE_LINE)
    }

    fn line_count(&self, line: u32) -> u32 {
        match self.stage.and_then(|s| s.line_at(line)) {
            Some(l) => self.checked(l.count, |mem| mem.line_count(line)),
            None => self.mem.line_count(line),
        }
    }

    fn line_position(&self, line: u32, k: u32) -> Vec4 {
        match self.point(line, k) {
            Some(p) => {
                self.checked(bits(p.position), |mem| bits(mem.line_position(line, k)));
                p.position
            }
            None => self.mem.line_position(line, k),
        }
    }

    fn line_tangent(&self, line: u32, k: u32) -> Vec4 {
        match self.point(line, k) {
            Some(p) => {
                self.checked(bits(p.tangent), |mem| bits(mem.line_tangent(line, k)));
                p.tangent
            }
            None => self.mem.line_tangent(line, k),
        }
    }

    fn line_distance(&self, line: u32, k: u32) -> f32 {
        match self.point(line, k) {
            Some(p) => {
                self.checked(p.distance.to_bits(), |mem| mem.line_distance(line, k).to_bits());
                p.distance
            }
            None => self.mem.line_distance(line, k),
        }
    }

    fn line_tag(&self, line: u32, k: u32) -> u32 {
        match self.point(line, k) {
            Some(p) => {
                self.checked(p.tag, |mem| mem.line_tag(line, k));
                p.tag
            }
            None => self.mem.line_tag(line, k),
        }
    }

    fn material_slot(&self, table: u32, index: u8) -> SlotLookup {
        let Some(m) = self.materials(table) else {
            return self.mem.material_slot(table, index);
        };
        let found = if m.slots_at == 0 {
            SlotLookup::NoTable
        } else if index as u32 >= m.count {
            SlotLookup::OutOfRange
        } else {
            SlotLookup::At(m.slots_at + SLOT_SIZE * index as u32)
        };
        self.checked(found, |mem| mem.material_slot(table, index))
    }

    fn material_texel(&self, slot: u32, column: u32, row: u32) -> u32 {
        match self.slot_of(slot).and_then(|(m, k)| m.texel(k, column, row)) {
            Some(texel) => self.checked(texel, |mem| mem.material_texel(slot, column, row)),
            None => self.mem.material_texel(slot, column, row),
        }
    }
}

/// Every query of the typed stage lifted from `mem` against the same query on `mem`: each drive line entry's position,
/// tangent and distance and the count, each slot lookup and each slot's 256 texels (past the table both read the
/// image). Their bytes compared, or the first query that differs.
pub fn differential(mem: &Mem) -> Result<usize, String> {
    let stage = Stage::lift(mem);
    let view = stage.view(mem);
    let mut bytes = 0;
    let mut same = |what: &dyn Fn() -> String, typed: &[u32], image: &[u32]| {
        bytes += 4 * typed.len();
        (typed == image).then_some(()).ok_or_else(|| format!("{}: typed {typed:#x?}, image {image:#x?}", what()))
    };
    if let Some((line, _)) = &stage.line {
        let line = *line;
        same(&|| "drive line count".into(), &[view.line_count(line)], &[mem.line_count(line)])?;
        for k in 0..entries_held(mem.line_count(line)) {
            let what = || format!("drive line entry {k}");
            same(&what, &bits(view.line_position(line, k)), &bits(mem.line_position(line, k)))?;
            same(&what, &bits(view.line_tangent(line, k)), &bits(mem.line_tangent(line, k)))?;
            same(&what, &[view.line_distance(line, k).to_bits()], &[mem.line_distance(line, k).to_bits()])?;
        }
    }
    if let Some((table, m)) = &stage.materials {
        let lookup = |s: SlotLookup| match s {
            SlotLookup::NoTable => [0, 0],
            SlotLookup::OutOfRange => [1, 0],
            SlotLookup::At(at) => [2, at],
        };
        for index in 0..=u8::MAX {
            let (typed, image) = (view.material_slot(*table, index), mem.material_slot(*table, index));
            same(&|| format!("material slot {index}"), &lookup(typed), &lookup(image))?;
        }
        for k in 0..SLOTS {
            let slot = table + SLOT_SIZE * k;
            if m.slots[k as usize].high != 0 && m.sheets[k as usize].is_none() {
                continue; // a sheet the image does not have: both would read unmapped bytes
            }
            for (column, row) in (0..16).flat_map(|r| (0..16).map(move |c| (c, r))) {
                let what = || format!("material slot {k} texel {column},{row}");
                same(&what, &[view.material_texel(slot, column, row)], &[mem.material_texel(slot, column, row)])?;
            }
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_driveline::store::EngineTables;
    use crate::entities::material_table::{MaterialEntry, MaterialTable};

    /// A stage of three drive line points and two material sheets, as the load stores them.
    fn image() -> Mem {
        let mut mem = Mem::new();
        let line = 0x10_0000;
        mem.map(line, &vec![0; (LINE_END + 0x20) as usize]);
        mem.set_u32(DRIVE_LINE, line);
        let point = |k: u32| DriveLinePoint {
            position: Vec4 { x: k as f32, y: 2.0 * k as f32, z: 1.0, w: 0.0 },
            tangent: Vec4 { x: 1.0, y: 2.0, z: 0.0, w: 0.0 },
            distance: 10.0 * k as f32,
            tag: k,
        };
        mem.store_drive_line(line, &[point(0), point(1), point(2)]);
        mem.map(MATERIAL_TABLE, &[0; TABLE_BYTES as usize]);
        let sheets = 0x20_0000;
        mem.map(sheets, &(0..=255).chain((0..=255).rev()).collect::<Vec<u8>>());
        let entries = [0, 1].map(|k| MaterialEntry { tag: k, low: 16, high: sheets + 0x100 * k as u32 });
        crate::entities::layout::store_material_table(
            &mut mem,
            MATERIAL_TABLE,
            &MaterialTable::build(MATERIAL_TABLE, &entries),
        );
        mem
    }

    #[test]
    fn the_typed_stage_holds_the_image_s_bytes_and_answers_as_the_image() {
        let mem = image();
        assert_eq!(round_trip(&mem), Ok((LINE_END - LINE_START + TABLE_BYTES + 2 * SHEET_BYTES) as usize));
        let compared = differential(&mem).unwrap();
        assert_eq!(compared, 4 + 4 * 9 * MAX_ENTRIES as usize + 4 * 2 * 256 + 4 * 256 * 256);
        let stage = Stage::lift(&mem);
        let view = stage.view(&mem);
        assert_eq!(view.line_distance(0x10_0000, 2), 20.0);
        assert_eq!(view.material_texel(MATERIAL_TABLE + 8, 1, 2), 255 - 33);
        assert_eq!(view.material_slot(MATERIAL_TABLE, 2), SlotLookup::OutOfRange);
    }

    #[test]
    fn a_line_over_10000_entries_carries_on_past_the_count() {
        use crate::engine_driveline::layout::{block_size, last_distance, sample};
        let n = 12_000;
        let line = 0x10_0000;
        let mut mem = Mem::new();
        mem.map(line, &vec![0; block_size(n) as usize]);
        mem.set_u32(DRIVE_LINE, line);
        let points: Vec<_> = (0..n)
            .map(|k| DriveLinePoint {
                position: Vec4 { x: 2.5 * k as f32, y: 0.0, z: 0.0, w: 0.0 },
                tangent: Vec4 { x: 2.5, y: 0.0, z: 0.0, w: 0.0 },
                distance: 2.5 * k as f32,
                tag: 0,
            })
            .collect();
        mem.store_drive_line(line, &points);
        assert_eq!(mem.line_count(line), n);
        assert_eq!(mem.line_distance(line, n), -1.0, "the entry past a long line is filled as the original's are");
        assert_eq!(last_distance(&mem, line), 2.5 * (n - 1) as f32);
        assert_eq!(sample(&mem, line, 27_501.25).x, 27_501.25);

        let held = 10_000 * ENTRY_BYTES + 4 + (n as usize + 1 - 10_000) * ENTRY_BYTES;
        assert_eq!(round_trip(&mem), Ok(held));
        differential(&mem).unwrap();
        let stage = Stage::lift(&mem);
        assert_eq!(stage.view(&mem).line_distance(line, 11_000), 27_500.0);
        let names: Vec<_> = stage.owned().into_iter().map(|(_, name)| name).collect();
        assert_eq!(names, ["drive line", "drive line past entry 10000"]);
    }

    #[test]
    fn owned_ranges_merge_a_sheet_two_slots_name() {
        let mut mem = image();
        mem.set_u32(MATERIAL_TABLE + 8 + SLOT_DATA, 0x20_0080);
        let owned = Stage::lift(&mem).owned();
        let sheets: Vec<_> =
            owned.iter().filter(|(_, n)| n.starts_with("material sheet")).map(|(r, _)| (r.start, r.end)).collect();
        assert_eq!(sheets, [(0x20_0000, 0x20_0180)]);
    }
}
