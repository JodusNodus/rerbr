//! Content > Cars: every car, its details and skins, the cars added and their removal. The car browser's lists
//! (`car_browser`, in its page mode) with this page's own detail panel; the skins are `car_skins`'.

use std::path::PathBuf;

use egui::{Color32, RichText};

use crate::browser::{favourite_button, GREY};
use crate::car_browser::{self, Action, CarBrowser, CarRow, Model};
use crate::car_skins::Skins;
use crate::catalog::{self, Car};
use crate::pages::{Page, ACCENT, WARN};
use crate::preview::Previews;
use crate::Form;

/// The Cars page's state.
pub struct CarsPage {
    pub browser: CarBrowser,
    /// The pack folder of the added car whose removal waits for its confirmation.
    confirm: Option<String>,
}

impl Default for CarsPage {
    fn default() -> CarsPage {
        CarsPage { browser: CarBrowser::for_page(), confirm: None }
    }
}

/// The folder holding the car's files: its model's.
fn files_folder(car: &Car, root: &str) -> Option<PathBuf> {
    match &car.model_path {
        Some(path) => path.parent().map(PathBuf::from),
        None => rbr_formats::cars::resolve(std::path::Path::new(root.trim()), &format!("Cars\\{}", car.name)),
    }
}

/// The pack folder of an added car.
fn added_pack(car: &Car) -> Option<String> {
    match &car.physics {
        catalog::Physics::Pack(path) if car.added => path.file_name().map(|n| n.to_string_lossy().into_owned()),
        _ => None,
    }
}

/// What the detail panel draws from.
struct Panel<'a> {
    row: &'a CarRow,
    car: &'a Car,
    root: &'a str,
    favourite: bool,
    /// Whether cars can be removed now (not while adding or driving).
    idle: bool,
}

/// The highlighted car: its turntable with the skin picked, its facts and skins, and what the player can do with it.
fn detail(
    ui: &mut egui::Ui,
    p: Panel,
    previews: &mut Previews,
    skins: &mut Skins,
    confirm: &mut Option<String>,
) -> Option<Action> {
    let Panel { row, car, root, favourite, idle } = p;
    let mut action = None;
    // The actions keep their room below; the rest scrolls when the window is short.
    let below = if added_pack(car).is_some() { 130.0 } else { 100.0 };
    egui::ScrollArea::vertical()
        .max_height((ui.available_height() - below).max(120.0))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(&row.title).size(22.0).strong());
            ui.label(RichText::new(&row.group).color(GREY));
            ui.add_space(6.0);
            previews.car_ui(ui, car, skins.choice(car), root);
            ui.add_space(6.0);
            skins.ui(ui, car);
            ui.add_space(6.0);
            egui::Grid::new("car facts").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
                let label = |ui: &mut egui::Ui, text: &str| ui.label(RichText::new(text).color(GREY));
                label(ui, "Source");
                ui.label(car_browser::source(car));
                ui.end_row();
                label(ui, "Drive");
                ui.label(row.drive.map_or("unknown", car_browser::Drive::label));
                ui.end_row();
                label(ui, "Model");
                match &row.model {
                    Model::Own => ui.label("its own"),
                    Model::StandIn(why) => ui.add(egui::Label::new(format!("a stand-in: {why}")).wrap()),
                    Model::Missing => ui.label("none"),
                };
                ui.end_row();
                label(ui, "Files");
                ui.vertical(|ui| {
                    if let Some(path) = files_folder(car, root) {
                        ui.add(egui::Label::new(path.display().to_string()).truncate());
                        if ui.link("Open folder").clicked() {
                            crate::crash::open(path.as_os_str());
                        }
                    } else {
                        ui.label("not found");
                    }
                });
                ui.end_row();
            });
        });
    ui.add_space(10.0);
    match row.problem.as_deref() {
        None => {
            ui.horizontal(|ui| {
                if favourite_button(ui, favourite).clicked() {
                    action = Some(Action::Favourite(row.name.clone()));
                }
                let go = RichText::new("Drive this car").size(18.0).strong().color(Color32::WHITE);
                let go = egui::Button::new(go).fill(ACCENT).min_size(egui::vec2(ui.available_width(), 44.0));
                if ui.add(go).on_hover_text("Enter").clicked() {
                    action = Some(Action::Choose(row.index));
                }
            });
        }
        Some(_) => {
            if favourite_button(ui, favourite).clicked() {
                action = Some(Action::Favourite(row.name.clone()));
            }
            ui.add_space(6.0);
            let reason = row.problem.as_deref().unwrap_or_default();
            ui.label(RichText::new(format!("Cannot start: {reason}")).color(WARN));
        }
    }
    if let Some(pack) = added_pack(car) {
        ui.add_space(8.0);
        if confirm.as_deref() == Some(pack.as_str()) {
            ui.label("Remove this car and its model from your files? Nothing in the game folder is touched.");
            ui.horizontal(|ui| {
                if ui.add_enabled(idle, egui::Button::new("Remove")).clicked() {
                    action = Some(Action::Remove(pack));
                }
                if ui.button("Keep it").clicked() {
                    *confirm = None;
                }
            });
        } else if ui.add_enabled(idle, egui::Button::new("Remove…")).on_hover_text("Added by you").clicked() {
            *confirm = Some(pack);
        }
    }
    action
}

impl Form {
    /// Every car: its details and skins, the cars added and their removal.
    pub fn cars_page(&mut self, ui: &mut egui::Ui) {
        if self.has_adding_output() {
            ui.horizontal_wrapped(|ui| self.adding_job_ui(ui));
            self.adding_status_ui(ui);
        }
        if self.no_cars_yet(ui) {
            return;
        }
        let root = self.game_root();
        let idle = self.can_add_cars();
        let added = self.adding.cars.len();
        let Form { cars_page, skins, favourite_cars, recent_cars, previews, catalog, .. } = &mut *self;
        let CarsPage { browser, confirm } = cars_page;
        let starred: &Vec<String> = favourite_cars;
        let mut panel =
            |ui: &mut egui::Ui, row: &CarRow, car: &Car, previews: &mut Previews, skins: &mut Skins, root: &str| {
                let favourite = starred.contains(&row.name);
                detail(ui, Panel { row, car, root, favourite, idle }, previews, skins, confirm)
            };
        let cars = catalog.cars.len();
        let env = car_browser::Env {
            heading: if added == 0 { format!("{cars} cars") } else { format!("{cars} cars · {added} added by you") },
            cars: &catalog.cars,
            favourites: favourite_cars,
            recent: recent_cars,
            previews,
            root: &root,
            skins,
            detail: Some(&mut panel),
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
                self.page = Page::Get;
                self.open_car_list();
            }
            Some(Action::Remove(pack)) => {
                self.cars_page.confirm = None;
                self.adding.status = crate::car_packs::remove(&self.cars_dir(), &pack).err().map(Err);
                self.rescan_cars();
            }
            Some(Action::Back) | None => {}
        }
    }
}
