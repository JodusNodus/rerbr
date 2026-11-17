//! Content > Stages: every stage, its details, the stages added and their removal.

use crate::pages::section;
use crate::Form;

/// The Stages page's state.
#[derive(Default)]
pub struct StagesPage {}

impl Form {
    /// Every stage: its details, the stages added and their removal.
    pub fn stages_page(&mut self, ui: &mut egui::Ui) {
        section(ui, "Stages");
    }
}
