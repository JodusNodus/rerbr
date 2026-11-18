//! Library > Cars: every car, its details and skins, the cars added and their removal. The car browser's lists
//! (`car_browser`, in its page mode); the skins are `car_skins`'.

use crate::car_browser::{self, Action, CarBrowser};
use crate::get_page::GetMore;
use crate::kit;
use crate::pages::Page;
use crate::Form;

/// The Cars page's state.
pub struct CarsPage {
    pub browser: CarBrowser,
}

impl Default for CarsPage {
    fn default() -> CarsPage {
        CarsPage { browser: CarBrowser::for_page() }
    }
}

impl Form {
    /// Every car: its details and skins, the cars added and their removal.
    pub fn cars_page(&mut self, ui: &mut egui::Ui) {
        if self.has_adding_output() {
            kit::body(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| self.adding_job_ui(ui));
                self.adding_status_ui(ui);
            });
        }
        if self.catalog.cars.is_empty() {
            kit::body(ui, |ui| _ = self.no_cars_yet(ui));
            return;
        }
        let root = self.game_root();
        self.cars_page.browser.idle = self.can_add_cars();
        let added = self.adding.cars.len();
        let Form { cars_page, skins, favourite_cars, recent_cars, previews, catalog, .. } = &mut *self;
        let browser = &mut cars_page.browser;
        let cars = catalog.cars.len();
        let env = car_browser::Env {
            heading: if added == 0 { format!("{cars} cars") } else { format!("{cars} cars · {added} added by you") },
            cars: &catalog.cars,
            favourites: favourite_cars,
            recent: recent_cars,
            previews,
            root: &root,
            skins,
        };
        match browser.ui(ui, env) {
            Some(Action::Choose(index)) => {
                self.choose_car(index);
                self.page = Page::Drive;
            }
            Some(Action::Favourite(name)) => match self.favourite_cars.iter().position(|n| *n == name) {
                Some(at) => drop(self.favourite_cars.remove(at)),
                None => self.favourite_cars.push(name),
            },
            Some(Action::GetMore) => {
                self.open_get_more(GetMore::Cars);
            }
            Some(Action::Remove(pack)) => {
                self.adding.status = crate::car_packs::remove(&self.cars_dir(), &pack).err().map(Err);
                self.rescan_cars();
            }
            Some(Action::Back) | None => {}
        }
    }
}
