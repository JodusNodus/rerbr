//! Content > Cars: every car, its details and skins, the cars added and their removal. The car browser's lists
//! (`car_browser`, in its page mode) with this page's own detail panel; the skin the player picks for a model folder
//! is kept in the settings (`[skin]`, [`CarSkins`]) and reaches the viewer as `--car-skin`.

use std::path::PathBuf;

use egui::{Color32, RichText};
use rbr_formats::car_pack::{self, OWN_SKIN};
use rbr_formats::ini;

use crate::browser::{favourite_button, GREY};
use crate::car_browser::{self, Action, CarBrowser, CarRow, Model};
use crate::catalog::{self, Car};
use crate::pages::{combo, Page, ACCENT, WARN};
use crate::preview::Previews;
use crate::Form;

const SKIN: &str = "skin";

/// The skin the player picked for each car model folder (`<folder> = <skin>`, `-` for the model's own; ours). A
/// folder without one draws RSF's choice (`.rsfconfig`), when it has one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CarSkins(Vec<(String, String)>);

impl CarSkins {
    pub fn parse(sections: &[ini::Section]) -> CarSkins {
        let entries = sections.iter().find(|s| s.name == SKIN).map(|s| s.entries.clone()).unwrap_or_default();
        CarSkins(entries.into_iter().filter(|(folder, skin)| !folder.is_empty() && !skin.is_empty()).collect())
    }

    pub fn to_text(&self) -> String {
        if self.0.is_empty() {
            return String::new();
        }
        let lines = self.0.iter().map(|(folder, skin)| format!("{folder} = {skin}\n"));
        format!("\n[{SKIN}]\n{}", lines.collect::<String>())
    }

    fn get(&self, folder: &str) -> Option<&str> {
        self.0.iter().find(|(f, _)| f.eq_ignore_ascii_case(folder)).map(|(_, skin)| skin.as_str())
    }

    fn set(&mut self, folder: &str, skin: &str) {
        self.0.retain(|(f, _)| !f.eq_ignore_ascii_case(folder));
        self.0.push((folder.to_owned(), skin.to_owned()));
    }

    /// The choice for `car`'s model (`--car-skin`'s word), none when it made none or has no model of its own.
    pub fn of(&self, car: &Car) -> Option<String> {
        self.get(&model_folder(car)?).map(str::to_owned)
    }
}

/// The folder of the model a pack car is drawn with, when it is its own (a stand-in draws no skin).
fn model_folder(car: &Car) -> Option<String> {
    let model = car.model.as_ref().ok().filter(|m| m.fallback.is_none())?;
    matches!(car.physics, catalog::Physics::Pack(_)).then_some(())?;
    model.entry.file_name.split(['\\', '/']).rev().nth(1).map(str::to_owned)
}

/// A car's skins as the panel lists them.
struct SkinList {
    /// The model's `.sgc` the list was read for.
    key: Option<PathBuf>,
    skins: Vec<String>,
    /// The skin RSF selected (`.rsfconfig`), drawn while the player has made no choice.
    rsf: Option<String>,
}

/// The Cars page's state.
pub struct CarsPage {
    pub browser: CarBrowser,
    skins: Option<SkinList>,
    /// The pack folder of the added car whose removal waits for its confirmation.
    confirm: Option<String>,
}

impl Default for CarsPage {
    fn default() -> CarsPage {
        CarsPage { browser: CarBrowser::for_page(), skins: None, confirm: None }
    }
}

/// The skins of `car`'s model, read again when another car is shown.
fn skin_list<'a>(cache: &'a mut Option<SkinList>, car: &Car) -> &'a SkinList {
    let list = cache.take().filter(|l| l.key == car.model_path);
    cache.insert(list.unwrap_or_else(|| {
        let (root, folder) = (car.model_path.as_deref().and_then(|p| car_pack::root_of(p).ok()), model_folder(car));
        let (skins, rsf) = match root.zip(folder) {
            Some((root, folder)) => {
                (car_pack::skins(&root, &folder), car_pack::selected_skin(&root, &folder).map(|(name, _)| name))
            }
            None => (Vec::new(), None),
        };
        SkinList { key: car.model_path.clone(), skins, rsf }
    }))
}

/// Whether `car`'s model has skins installed (`--screenshot` shows one that does).
pub fn has_skins(car: &Car) -> bool {
    let root = car.model_path.as_deref().and_then(|p| car_pack::root_of(p).ok());
    root.zip(model_folder(car)).is_some_and(|(root, folder)| !car_pack::skins(&root, &folder).is_empty())
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
    skins: &'a SkinList,
    picked: &'a CarSkins,
    favourite: bool,
    /// Whether cars can be removed now (not while adding or driving).
    idle: bool,
}

/// The highlighted car: its turntable with the skin picked, its facts and skins, and what the player can do with it.
fn detail(ui: &mut egui::Ui, p: Panel, previews: &mut Previews, confirm: &mut Option<String>) -> Option<Action> {
    let Panel { row, car, root, skins, picked, favourite, idle } = p;
    let mut action = None;
    // The actions keep their room below; the rest scrolls when the window is short.
    let below = if added_pack(car).is_some() { 130.0 } else { 100.0 };
    let folder = model_folder(car);
    egui::ScrollArea::vertical()
        .max_height((ui.available_height() - below).max(120.0))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(&row.title).size(22.0).strong());
            ui.label(RichText::new(&row.group).color(GREY));
            ui.add_space(6.0);
            previews.car_ui(ui, car, root, picked.of(car).as_deref());
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
                label(ui, "Skins");
                match (&folder, skins.skins.is_empty()) {
                    (None, _) => ui.label(RichText::new("not used: no model of its own").color(GREY)),
                    (Some(_), true) => ui.label(RichText::new("none installed").color(GREY)),
                    (Some(folder), false) => {
                        // The choice, else RSF's, else the model's own.
                        let current = match picked.get(folder) {
                            Some(OWN_SKIN) => None,
                            Some(name) => Some(name.to_owned()),
                            None => skins.rsf.clone(),
                        };
                        let mut choice = current.clone();
                        let options: Vec<Option<String>> =
                            std::iter::once(None).chain(skins.skins.iter().cloned().map(Some)).collect();
                        let response =
                            combo(ui, "skin", 210.0, &mut choice, &options, |s| s.unwrap_or("Model's own".into()));
                        if choice != current {
                            action = Some(Action::Skin(folder.clone(), choice.unwrap_or_else(|| OWN_SKIN.to_owned())));
                        }
                        response.on_hover_text(format!("{} skins; the drive and the preview use it", skins.skins.len()))
                    }
                };
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
    /// The skin the player picked for `car`'s model, for the previews and the viewer.
    pub fn car_skin(&self, car: &Car) -> Option<String> {
        self.car_skins.of(car)
    }

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
        let Form { cars_page, car_skins, favourite_cars, recent_cars, previews, catalog, .. } = &mut *self;
        let CarsPage { browser, skins, confirm } = cars_page;
        let starred: &Vec<String> = favourite_cars;
        let mut panel = |ui: &mut egui::Ui, row: &CarRow, car: &Car, previews: &mut Previews, root: &str| {
            let favourite = starred.contains(&row.name);
            let skins = skin_list(skins, car);
            detail(ui, Panel { row, car, root, skins, picked: car_skins, favourite, idle }, previews, confirm)
        };
        let cars = catalog.cars.len();
        let env = car_browser::Env {
            heading: if added == 0 { format!("{cars} cars") } else { format!("{cars} cars · {added} added by you") },
            cars: &catalog.cars,
            favourites: favourite_cars,
            recent: recent_cars,
            previews,
            root: &root,
            skins: car_skins,
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
            Some(Action::Skin(folder, skin)) => self.car_skins.set(&folder, &skin),
            Some(Action::Remove(pack)) => {
                self.cars_page.confirm = None;
                self.adding.status = crate::car_packs::remove(&self.cars_dir(), &pack).err().map(Err);
                self.rescan_cars();
            }
            Some(Action::Back) | None => {}
        }
    }
}
