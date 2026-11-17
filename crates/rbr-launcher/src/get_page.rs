//! Content > Get more: where new content comes from: gvrc.de's cars, the stage sites, the recent downloads and Add.

use crate::Form;

/// The Get more page's state.
#[derive(Default)]
pub struct GetPage {}

impl Form {
    /// Where new content comes from: gvrc.de's cars, the recent downloads, Add, and the stage sites.
    pub fn get_page(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("get").show(ui, |ui| {
            crate::pages::section(ui, "Cars from gvrc.de");
            ui.horizontal(|ui| self.download_cars_button(ui));
            if self.has_adding_output() {
                ui.horizontal_wrapped(|ui| self.adding_job_ui(ui));
                self.adding_status_ui(ui);
            }
            self.download_cars_panel(ui);
            if !self.adding.cars.is_empty() {
                let title = format!("Added cars ({})", self.adding.cars.len());
                egui::CollapsingHeader::new(title).id_salt("added cars").show(ui, |ui| self.added_cars_ui(ui));
            }
            self.add_content_section(ui);
        });
    }
}
