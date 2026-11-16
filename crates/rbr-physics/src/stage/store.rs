//! The stage's store (`crate::store`): the step's reads of the tables the load builds and the step never writes, the
//! drive line and the material table with its sheets. `Mem` implements it in `layout/queries.rs` (the image, the
//! per-call path); `layout::StageView` on the typed stage ([`super::typed::Stage`]) a step holds. The rest of the
//! stage is the load's construction and stays on `&mut Mem`.

use crate::entities::store::SlotLookup;
use crate::math::vec::Vec4;

pub trait StageStore {
    /// The drive line `DRIVE_LINE` names.
    fn drive_line(&self) -> u32;
    /// The entry count of the drive line at `line`.
    fn line_count(&self, line: u32) -> u32;
    /// Entry `k`'s position, tangent and distance of the drive line at `line` (`k` wraps as the original's 32-bit
    /// index does, `engine_driveline::layout::entry_at`).
    fn line_position(&self, line: u32, k: u32) -> Vec4;
    fn line_tangent(&self, line: u32, k: u32) -> Vec4;
    fn line_distance(&self, line: u32, k: u32) -> f32;
    /// Entry `k`'s tag word, the one the `.trk`'s two words make (zero in every retail stage).
    fn line_tag(&self, line: u32, k: u32) -> u32;
    /// Ours: the drive line is closed (a circuit's), marked by its first entry's tag
    /// ([`rbr_formats::trk::CLOSED_LINE_WORDS`]) with at least two segments, the last entry the first's copy.
    fn line_closed(&self, line: u32) -> bool {
        let [high, low] = rbr_formats::trk::CLOSED_LINE_WORDS;
        self.line_count(line) >= 3 && self.line_tag(line, 0) == u32::from(high) << 16 | u32::from(low)
    }
    /// Slot `index` of the material table at `table`.
    fn material_slot(&self, table: u32, index: u8) -> SlotLookup;
    /// The byte at `column`, `row` (16 a row) of the sheet of the material slot at `slot`, 0 when it has none.
    fn material_texel(&self, slot: u32, column: u32, row: u32) -> u32;
}
