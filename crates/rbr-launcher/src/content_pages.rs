//! The Content section's Co-drivers page; the other Content pages are `cars_page`, `stages_page` and `get_page`.

use crate::Form;

impl Form {
    /// The co-driver's voice and the pace notes, and their packs.
    pub fn codrivers_page(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("codrivers").show(ui, |ui| self.codriver_section(ui));
    }
}
