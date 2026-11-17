//! Content > Cars: every car, its details and skins, the cars added and their removal.

use crate::pages::section;
use crate::Form;

/// The Cars page's state.
#[derive(Default)]
pub struct CarsPage {}

impl Form {
    /// Every car: its details and skins, the cars added and their removal.
    pub fn cars_page(&mut self, ui: &mut egui::Ui) {
        section(ui, "Cars");
    }
}
