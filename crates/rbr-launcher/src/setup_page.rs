//! The Car setup page: the manager of the car's saved setups on the stage's surface. It opens one (or the default),
//! edits its values grouped by area, within the car options' ranges and by the game's steps (`rbr_gameplay::setup`),
//! and saves them in the game's `Setups/*.lsp` format. The Drive page picks the one to drive with
//! ([`SetupPage::choice_ui`]), kept per car and surface (`[setup]` of the settings, [`SetupChoices`]).
//!
//! A retail car's setups are ours in `Physics/<car>/Setups/` of the player's folder (the game's `%sSetups/` under its
//! own folder) and the install's, where the game saves them. An RSF car's are ours in `SavedGames/<pack>/` of the
//! player's folder, then the RSF install's own `SavedGames/<pack>/` and the ones it ships in `rsfdata/setups/<pack>/`,
//! all named `<surface>_<name>.lsp` as RSF names them. Only ours are saved and deleted. Start writes the chosen
//! values to `setup-current.lsp` in the player's folder and passes it to `rbr-standalone --setup`, unless they are
//! the default.
//!
//! The game's three save slots per car (`CSetupSaveSlotButtonUI`, `SavedGames/<car>slot<n>_<name>.lsp`,
//! `rbr_formats::saves::setup_slots`) are not offered: earlier versions' slot files in the player's folder, and an
//! RSF car's earlier `Physics/<key>/Setups/`, are listed with the car's setups, read only (`Car::setup_keys`). The championship offers the stage's default, the current setup or a saved
//! one before each stage (`CSeasonChooseSetupUI`, [`SetupPage::choose`]).

use std::path::{Path, PathBuf};

use egui::{ComboBox, DragValue, RichText, Ui};
use rbr_formats::data::Data;
use rbr_formats::saves::{is_file_name_part, setup_slots, DIR as SAVES};
use rbr_gameplay::setup::{Area, CarSetup, Range, SeasonSetup, Setup, Slot, Spec, CAR_FOLDERS, MAP_POINTS, WHEELS};

use crate::catalog::Car;
use crate::pages::{combo, WARN};
use crate::settings::SetupChoices;

/// How the lists name the car's default setup.
const DEFAULT: &str = "Default setup";

/// The defaults and ranges of the NGP car pack at `path` on `surface`.
fn pack_setup(path: &Path, surface: &str) -> Result<CarSetup, String> {
    let pack = rbr_formats::rsf::Pack::open(path)?;
    let text = |name: String| pack.read(&name).map(|b| String::from_utf8_lossy(&b).into_owned());
    CarSetup::parse(&text(format!("setups/d_{surface}.lsp"))?, &text(format!("r_{surface}.lsp"))?)
}

/// The setups' surfaces by the map's surface (`catalog::Map::surface`), as the files name them.
pub const SURFACES: [&str; 3] = ["gravel", "tarmac", "snow"];

/// Where a saved setup is from. Ours are saved and deleted here; the others are only read: the 1.02 install's, ours
/// from earlier versions (an RSF car's `Physics/<key>/Setups/` and the game's save slots), and the RSF install's (its
/// players' `SavedGames/<pack>/` and the ones it ships in `rsfdata/setups/<pack>/`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Own,
    Game,
    Earlier,
    Slot,
    Rsf,
    RsfShipped,
}

impl Source {
    /// How a list names the source after the setup's name; ours have none.
    pub fn tag(self) -> &'static str {
        match self {
            Source::Own => "",
            Source::Game => "game",
            Source::Earlier => "earlier",
            Source::Slot => "game slot",
            Source::Rsf => "RSF",
            Source::RsfShipped => "RSF's",
        }
    }
}

/// A saved setup of the car on the surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub source: Source,
    path: PathBuf,
}

impl Profile {
    pub fn label(&self) -> String {
        match self.source.tag() {
            "" => self.name.clone(),
            tag => format!("{} ({tag})", self.name),
        }
    }

    /// The setup file's text (a slot file's, `setup_slots::text`).
    fn text(&self) -> Result<String, String> {
        let bytes = std::fs::read(&self.path).map_err(|e| format!("{}: {e}", self.path.display()))?;
        Ok(match self.source {
            Source::Slot => setup_slots::text(&bytes),
            _ => String::from_utf8_lossy(&bytes).into_owned(),
        })
    }
}

/// The folders a car's setups are in, ours first. A retail car's are the game's `Physics/<car>/Setups/`, named as
/// the player typed. An RSF car's are RSF's `SavedGames/<pack>/`, named `<surface>_<name>.lsp` (`by_surface`; RSF's
/// `<id>_d_<surface>.lsp` copies of the defaults don't match), then ours of earlier versions under its setup keys'
/// `Physics/<key>/Setups/` (`Car::setup_keys`: its own, then slot 5's). Last, the game's save slots of the keys in
/// our `SavedGames/` (`<key>slot<n>_<name>.lsp`).
#[derive(Debug, Default)]
struct Folders {
    dirs: Vec<(Source, PathBuf)>,
    by_surface: bool,
    keys: Vec<String>,
}

impl Folders {
    fn of(install: &Path, here: &Path, rsf: &Path, car: &Car) -> Folders {
        let keys = car.setup_keys();
        let setups = |root: &Path, key: &String| {
            let folder = match key.parse::<usize>() {
                Ok(n) => CAR_FOLDERS.get(n)?.to_string(),
                Err(_) => format!("Physics/{key}/"),
            };
            Some(root.join(folder).join("Setups"))
        };
        let slots = (Source::Slot, here.join(SAVES));
        match &car.rsf {
            Some(r) => {
                let Some(pack) = r.pack.as_ref().ok().and_then(|p| pack_folder(p)) else {
                    return Folders::default();
                };
                let saved = |root: &Path| root.join(SAVES).join(&pack);
                let mut dirs = vec![
                    (Source::Own, saved(here)),
                    (Source::Rsf, saved(rsf)),
                    (Source::RsfShipped, rsf.join("rsfdata/setups").join(&pack)),
                ];
                dirs.extend(keys.iter().filter_map(|k| setups(here, k)).map(|d| (Source::Earlier, d)));
                dirs.push(slots);
                Folders { dirs, by_surface: true, keys }
            }
            None => {
                let (Some(own), Some(game)) = (setups(here, &keys[0]), setups(install, &keys[0])) else {
                    return Folders::default();
                };
                Folders { dirs: vec![(Source::Own, own), (Source::Game, game), slots], by_surface: false, keys }
            }
        }
    }

    /// The prefix of a file name of `source` on `surface`: RSF's names in RSF's folders and ours of an RSF car.
    fn prefix(&self, source: Source, surface: &str) -> String {
        if self.by_surface && matches!(source, Source::Own | Source::Rsf | Source::RsfShipped) {
            format!("{surface}_")
        } else {
            String::new()
        }
    }

    /// Our file of setup `name` on `surface`.
    fn file(&self, surface: &str, name: &str) -> Option<PathBuf> {
        let (_, own) = self.dirs.first()?;
        Some(own.join(format!("{}{name}.lsp", self.prefix(Source::Own, surface))))
    }

    /// The setup a file named `file` in a folder of `source` is, on `surface`.
    fn name_of(&self, source: Source, file: &str, surface: &str) -> Option<String> {
        if source == Source::Slot {
            return setup_slots::parse_name(file)
                .filter(|(key, _, _)| self.keys.contains(key))
                .map(|(_, _, name)| name);
        }
        let stem = strip_suffix_ignore_case(file, ".lsp")?;
        Some(strip_prefix_ignore_case(stem, &self.prefix(source, surface))?.to_owned())
    }

    /// The setups on `surface`: ours first, then each other folder's, by name; a name is listed once, from the first
    /// folder that has it.
    fn scan(&self, surface: &str) -> Vec<Profile> {
        let mut profiles: Vec<Profile> = Vec::new();
        for &(source, ref dir) in &self.dirs {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            let mut found: Vec<Profile> = entries
                .filter_map(|e| {
                    let path = e.ok()?.path();
                    let name = self.name_of(source, path.file_name()?.to_str()?, surface)?;
                    (!name.is_empty() && path.is_file()).then_some(Profile { name, source, path })
                })
                .filter(|p| !profiles.iter().any(|q| q.name.eq_ignore_ascii_case(&p.name)))
                .collect();
            found.sort_by_key(|p| p.name.to_lowercase());
            profiles.extend(found);
        }
        profiles
    }
}

/// RSF's folder for a setup of the car pack at `pack` (`rsfdata/cars/<pack>` or `<pack>.zip`): the pack's name.
fn pack_folder(pack: &Path) -> Option<String> {
    let name = pack.file_name()?.to_str()?;
    Some(strip_suffix_ignore_case(name, ".zip").unwrap_or(name).to_owned())
}

fn strip_suffix_ignore_case<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    let at = s.len().checked_sub(suffix.len())?;
    (s.is_char_boundary(at) && s[at..].eq_ignore_ascii_case(suffix)).then(|| &s[..at])
}

fn strip_prefix_ignore_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let at = prefix.len();
    (s.len() >= at && s.is_char_boundary(at) && s[..at].eq_ignore_ascii_case(prefix)).then(|| &s[at..])
}

/// The page's state for one car and surface.
pub struct SetupPage {
    /// The car (`[CarNN]`) and surface the setup is for, and the car's name.
    key: Option<(u32, &'static str)>,
    car_name: String,
    car: Result<CarSetup, String>,
    folders: Folders,
    profiles: Vec<Profile>,
    /// The setup each car drives with on each surface, the Drive page's choice.
    pub choices: SetupChoices,
    /// The values Start drives with: the chosen setup's, or a championship's (`choose`, `load_text`, `season`).
    pub setup: Setup,
    season: bool,
    /// The editor: the setup it opened (`None` the default), its values as opened and as edited, and the name it
    /// saves under.
    opened: Option<String>,
    opened_values: Setup,
    draft: Setup,
    name: String,
    confirm_delete: bool,
    area: Area,
    map: usize,
    status: String,
}

impl SetupPage {
    pub fn new(choices: SetupChoices) -> SetupPage {
        SetupPage {
            key: None,
            car_name: String::new(),
            car: Err(String::new()),
            folders: Folders::default(),
            profiles: Vec::new(),
            choices,
            setup: Setup::default(),
            season: false,
            opened: None,
            opened_values: Setup::default(),
            draft: Setup::default(),
            name: String::new(),
            confirm_delete: false,
            area: Area::Tyres,
            map: 0,
            status: String::new(),
        }
    }

    /// Loads the car's defaults and ranges when the car or surface changes, then the setup chosen for them, which
    /// the editor opens. An RSF car's come from its pack (`setups/d_<surface>.lsp`, `r_<surface>.lsp`), as the game
    /// reads them once RSF has copied the pack into the slot; its saved setups are RSF's per pack and its game slots
    /// the slot's. A retail slot's come from NGP's `physics.rbz` (`archive`), which has the NGP car installed in it.
    pub fn select(&mut self, data: &Data, here: &Path, rsf: &Path, car: &Car, archive: &Path, surface: &'static str) {
        if self.key == Some((car.index, surface)) && self.car_name == car.name {
            return;
        }
        self.key = Some((car.index, surface));
        self.car_name = car.name.clone();
        self.car = match car.rsf.as_ref().map(|r| &r.pack) {
            Some(Ok(pack)) => pack_setup(pack, surface),
            Some(Err(e)) => Err(e.clone()),
            None => CarSetup::load_in(archive, car.index as usize, surface),
        };
        self.folders = Folders::of(data.install(), here, rsf, car);
        self.profiles = self.folders.scan(surface);
        self.status.clear();
        self.drive_chosen();
        let chosen = self.chosen().map(str::to_owned);
        self.open(chosen.as_deref());
    }

    /// Shows `area` (`--screenshot`).
    pub fn show_area(&mut self, area: Area) {
        self.area = area;
    }

    fn surface(&self) -> &'static str {
        self.key.map_or("", |(_, s)| s)
    }

    fn default_values(&self) -> Setup {
        self.car.as_ref().map_or_else(|_| Setup::default(), |c| c.default)
    }

    /// The name of the setup the car drives with on the surface; `None` for the default.
    pub fn chosen(&self) -> Option<&str> {
        self.choices.of(&self.car_name, self.surface())
    }

    /// Reads the saved setup `name` over the car's defaults.
    fn read(&self, name: &str) -> Result<Setup, String> {
        let car = self.car.as_ref().map_err(|e| e.clone())?;
        Setup::from_text(&car.default, &self.text(name)?)
    }

    fn text(&self, name: &str) -> Result<String, String> {
        self.profiles.iter().find(|p| p.name == name).ok_or(format!("{name}: not found"))?.text()
    }

    /// The names of the car's saved setups on the map surface `surface` (`catalog::Map::surface`).
    pub fn saved_names(&self, surface: u32) -> Vec<String> {
        let surface = SURFACES.get(surface as usize).copied().unwrap_or(SURFACES[0]);
        self.folders.scan(surface).into_iter().map(|p| p.name).collect()
    }

    /// Makes the values Start drives with the chosen setup's (the default when it is gone).
    pub fn drive_chosen(&mut self) {
        self.season = false;
        let chosen = self.chosen().map(str::to_owned);
        self.setup = match chosen.map(|name| self.read(&name)) {
            Some(Ok(setup)) => setup,
            Some(Err(e)) => {
                self.status = format!("{e}; driving with the default setup");
                self.default_values()
            }
            None => self.default_values(),
        };
    }

    /// Makes `name` (`None` the default) the setup the car drives with on the surface.
    pub fn drive_with(&mut self, name: Option<&str>) {
        let surface = self.surface();
        self.choices.set(&self.car_name, surface, name);
        self.drive_chosen();
    }

    /// Opens saved setup `name` (`None` the default) in the editor.
    fn open(&mut self, name: Option<&str>) {
        let values = match name {
            Some(n) => match self.read(n) {
                Ok(values) => values,
                Err(e) => {
                    self.status = e;
                    return;
                }
            },
            None => self.default_values(),
        };
        self.opened = name.map(str::to_owned);
        self.opened_values = values;
        self.draft = values;
        self.name = name.unwrap_or_default().to_owned();
        self.confirm_delete = false;
    }

    /// The setup file to start with: the values Start drives with, unless they are the default (none then).
    pub fn current_file(&self, here: &Path) -> Result<Option<PathBuf>, String> {
        let car = self.car.as_ref().map_err(|e| e.clone())?;
        if self.setup == car.default {
            return Ok(None);
        }
        let path = here.join("setup-current.lsp");
        std::fs::write(&path, self.setup.to_text()).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Some(path))
    }

    /// The current setup's `MaxSteeringLock`, when the car's setups loaded.
    pub fn steering_lock(&self) -> Option<f32> {
        self.car.as_ref().ok().map(|_| self.setup.steering_lock)
    }

    /// The setup's text as a setup file holds it (the championship's save keeps it, `PTSC`).
    pub fn current_text(&self) -> String {
        self.setup.to_text()
    }

    /// Makes the setup of `CSeasonChooseSetupUI`'s row `choice` the one Start drives with.
    pub fn choose(&mut self, choice: &SeasonSetup) -> Result<(), String> {
        let car = self.car.as_ref().map_err(|_| "no setup for this car".to_owned())?;
        let text = match choice {
            SeasonSetup::Saved(name) => Some(self.text(name)?),
            _ => None,
        };
        self.setup = choice.setup(&car.default, &self.setup, text.as_deref())?;
        self.season = true;
        Ok(())
    }

    /// Makes a setup file's text over the default the one Start drives with (a season's saved setup).
    pub fn load_text(&mut self, text: &str) -> Result<(), String> {
        let car = self.car.as_ref().map_err(|e| e.clone())?;
        self.setup = Setup::from_text(&car.default, text)?;
        self.season = true;
        Ok(())
    }

    /// The Drive page's Setup field: the default or a saved setup of the car on the surface, and Edit…. Whether
    /// Edit… was pressed.
    pub fn choice_ui(&mut self, ui: &mut Ui, width: f32) -> bool {
        if let Err(e) = &self.car {
            ui.colored_label(WARN, format!("no setup for this car: {e}"));
            return false;
        }
        let chosen = self.chosen().map(str::to_owned);
        let selected = match &chosen {
            Some(name) => {
                self.profiles.iter().find(|p| p.name == *name).map_or(format!("{name} (missing)"), Profile::label)
            }
            None => DEFAULT.to_owned(),
        };
        let mut pick = None;
        let edit = ui
            .horizontal(|ui| {
                let combo = ComboBox::from_id_salt("drive-setup").width(width - 70.0).selected_text(selected);
                combo.show_ui(ui, |ui| {
                    if ui.selectable_label(chosen.is_none(), DEFAULT).clicked() {
                        pick = Some(None);
                    }
                    for p in &self.profiles {
                        if ui.selectable_label(chosen.as_deref() == Some(&p.name), p.label()).clicked() {
                            pick = Some(Some(p.name.clone()));
                        }
                    }
                });
                ui.button("Edit…").on_hover_text("Make, change and save setups on the Car setup page").clicked()
            })
            .inner;
        if let Some(name) = pick {
            self.drive_with(name.as_deref());
            // The editor follows, unless it holds unsaved changes.
            if self.draft == self.opened_values {
                self.open(name.as_deref());
            }
        }
        edit
    }

    /// Saves the editor's values as our setup under the name field's name, and drives with it.
    fn save(&mut self) {
        let name = match valid_name(&self.name) {
            Ok(name) => name.to_owned(),
            Err(e) => {
                self.status = e;
                return;
            }
        };
        let Some(path) = self.folders.file(self.surface(), &name) else {
            return;
        };
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&path, self.draft.to_text()));
        if let Err(e) = written {
            self.status = format!("{}: {e}", path.display());
            return;
        }
        self.profiles = self.folders.scan(self.surface());
        self.drive_with(Some(&name));
        self.open(Some(&name));
        self.status = format!("saved {name}; the Drive page drives with it");
    }

    /// Deletes our setup the editor opened, and opens the default.
    fn delete(&mut self) {
        let Some(profile) = self.opened.as_ref().and_then(|n| self.profiles.iter().find(|p| p.name == *n)).cloned()
        else {
            return;
        };
        if let Err(e) = std::fs::remove_file(&profile.path) {
            self.status = format!("{}: {e}", profile.path.display());
            return;
        }
        self.profiles = self.folders.scan(self.surface());
        if self.chosen() == Some(profile.name.as_str()) {
            self.drive_with(None);
        }
        self.open(None);
        self.status = format!("deleted {}", profile.name);
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let car = match &self.car {
            Ok(car) => car.clone(),
            Err(e) => {
                ui.colored_label(WARN, format!("no setup for this car: {e}"));
                return;
            }
        };
        let opened = self.opened.as_ref().and_then(|n| self.profiles.iter().find(|p| p.name == *n)).cloned();
        ui.horizontal(|ui| {
            ui.label(format!("Setups on {}", self.surface()));
            let selected = opened.as_ref().map_or(DEFAULT.to_owned(), Profile::label);
            let mut pick = None;
            ComboBox::from_id_salt("setup-open").width(220.0).selected_text(selected).show_ui(ui, |ui| {
                if ui.selectable_label(self.opened.is_none(), DEFAULT).clicked() {
                    pick = Some(None);
                }
                for p in &self.profiles {
                    if ui.selectable_label(self.opened.as_deref() == Some(&p.name), p.label()).clicked() {
                        pick = Some(Some(p.name.clone()));
                    }
                }
            });
            if let Some(name) = pick {
                self.open(name.as_deref());
                self.status.clear();
            }
            if self.season
                && ui
                    .button("Open the championship's")
                    .on_hover_text("The values the championship drives with; Save keeps them as a setup")
                    .clicked()
            {
                self.draft = self.setup;
            }
            if self.opened.as_deref() == self.chosen() {
                ui.weak("driving with this");
            } else if ui
                .button("Drive with this")
                .on_hover_text("Make it the Drive page's setup for this car")
                .clicked()
            {
                let name = self.opened.clone();
                self.drive_with(name.as_deref());
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(200.0).hint_text("name to save under"));
            let replaces =
                self.profiles.iter().any(|p| p.name.eq_ignore_ascii_case(self.name.trim()) && p.source == Source::Own);
            let save = if replaces { "Save (replace)" } else { "Save" };
            if ui.button(save).on_hover_text("Save as your setup and drive with it").clicked() {
                self.save();
            }
            let changed = self.draft != self.opened_values;
            if ui
                .add_enabled(changed, egui::Button::new("Revert"))
                .on_hover_text("Back to the values as opened")
                .clicked()
            {
                self.draft = self.opened_values;
            }
            if ui.add_enabled(self.draft != car.default, egui::Button::new("Default values")).clicked() {
                self.draft = car.default;
            }
            if opened.as_ref().is_some_and(|p| p.source == Source::Own) {
                if self.confirm_delete {
                    if ui.button(RichText::new("Delete it").color(WARN)).clicked() {
                        self.delete();
                    }
                    if ui.button("Keep").clicked() {
                        self.confirm_delete = false;
                    }
                } else if ui.button("Delete…").clicked() {
                    self.confirm_delete = true;
                }
            }
        });
        let changed = self.draft != self.opened_values;
        let note = match (changed, opened.as_ref().map(|p| p.source)) {
            (true, _) => "unsaved changes: Save to drive with them".to_owned(),
            (false, Some(Source::Own)) | (false, None) => String::new(),
            (false, Some(source)) => format!("{} setup, read only: Save keeps a copy as yours", source.tag()),
        };
        if !note.is_empty() {
            ui.colored_label(if changed { WARN } else { egui::Color32::GRAY }, note);
        }
        if !self.status.is_empty() {
            ui.label(&self.status);
        }
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(190.0);
                for area in Area::ALL {
                    ui.selectable_value(&mut self.area, area, area.name());
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| ui.vertical(|ui| self.area_ui(ui, &car)));
        });
    }

    fn area_ui(&mut self, ui: &mut Ui, car: &CarSetup) {
        let area = self.area;
        let mut specs: Vec<(Spec, Range)> = Setup::specs().into_iter().zip(car.ranges.iter().copied()).collect();
        specs.retain(|(spec, _)| spec.area == area);
        match area {
            Area::Tyres | Area::Springs | Area::Dampers | Area::Geometry => self.wheels_ui(ui, &specs),
            Area::Differentials => self.differentials_ui(ui, &specs),
            Area::Gears => self.gears_ui(ui, car, &specs),
            Area::RollBars | Area::Brakes => {
                self.rows_ui(ui, &specs);
                if area == Area::Brakes {
                    let (f, r) = (self.draft.front_brake_pressure, self.draft.rear_brake_pressure);
                    ui.label(format!("Brake balance: {:.0}% front", 100.0 * f / (f + r).max(1.0)));
                }
            }
        }
    }

    /// One slider per value, with its range.
    fn rows_ui(&mut self, ui: &mut Ui, specs: &[(Spec, Range)]) {
        egui::Grid::new("setup-rows").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            for (spec, range) in specs.iter().filter(|(s, _)| s.index.is_none()) {
                ui.label(spec.label).on_hover_text(range_text(spec, range));
                self.with_slot(spec, |slot| value_ui(ui, spec, *range, slot, Widget::Slider));
                ui.end_row();
            }
        });
    }

    /// The per-wheel values: one row per value, one column per wheel.
    fn wheels_ui(&mut self, ui: &mut Ui, specs: &[(Spec, Range)]) {
        egui::Grid::new("setup-wheels").num_columns(5).spacing([12.0, 8.0]).striped(true).show(ui, |ui| {
            ui.label("");
            for wheel in ["Left front", "Right front", "Left rear", "Right rear"] {
                ui.strong(wheel);
            }
            ui.end_row();
            let mut labels: Vec<&'static str> = Vec::new();
            for (spec, _) in specs {
                if !labels.contains(&spec.label) {
                    labels.push(spec.label);
                }
            }
            for label in labels {
                let first = specs.iter().find(|(s, _)| s.label == label).map(|(s, r)| range_text(s, r));
                ui.label(label).on_hover_text(first.unwrap_or_default());
                for w in 0..WHEELS.len() {
                    if let Some((spec, range)) = specs.iter().find(|(s, _)| s.label == label && s.index == Some(w)) {
                        self.with_slot(spec, |slot| value_ui(ui, spec, *range, slot, Widget::Field));
                    }
                }
                ui.end_row();
            }
        });
    }

    fn differentials_ui(&mut self, ui: &mut Ui, specs: &[(Spec, Range)]) {
        self.rows_ui(ui, specs);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.strong("Diff map");
            for (m, name) in ["Centre", "Centre (left-foot brake)", "Front", "Rear"].into_iter().enumerate() {
                ui.selectable_value(&mut self.map, m, name);
            }
        });
        let map: Vec<&(Spec, Range)> = specs.iter().filter(|(s, _)| s.index == Some(self.map)).collect();
        egui::Grid::new("setup-map").num_columns(MAP_POINTS + 1).spacing([6.0, 6.0]).show(ui, |ui| {
            ui.label("point");
            for n in 0..MAP_POINTS {
                ui.label(format!("{}", n * 10));
            }
            ui.end_row();
            for (row, suffix) in
                map.chunks(MAP_POINTS).zip(["throttle lock", "brake lock", "speed", "speed lock factor"])
            {
                let unit = row[0].0.unit.suffix;
                ui.label(format!("{suffix} ({unit})")).on_hover_text(range_text(&row[0].0, &row[0].1));
                for (spec, range) in row.iter().map(|p| (&p.0, p.1)) {
                    self.with_slot(spec, |slot| value_ui(ui, spec, range, slot, Widget::Cell));
                }
                ui.end_row();
            }
        });
    }

    fn gears_ui(&mut self, ui: &mut Ui, car: &CarSetup, specs: &[(Spec, Range)]) {
        let Some((_, range)) = specs.first() else {
            return;
        };
        let ratio =
            |id: u32| car.gears.iter().find(|g| g.0 == id).map_or(format!("{id}"), |&(_, p, g)| format!("{p}:{g}"));
        ui.horizontal(|ui| {
            ui.label("Drop gear").on_hover_text("the car options' drop gear rows; the gearbox ratios are fixed");
            let gear = &mut self.draft.drop_gear;
            ui.add_enabled_ui(range.max > range.min, |ui| {
                let ids: Vec<u32> = (range.min as u32..=range.max as u32).collect();
                combo(ui, "drop-gear", None, gear, &ids, ratio);
            });
        });
    }

    /// Calls `f` with the setup's slot of `spec`.
    fn with_slot(&mut self, spec: &Spec, f: impl FnOnce(Slot<'_>)) {
        let mut f = Some(f);
        self.draft.visit(&mut |s, slot| {
            if s.group == spec.group && s.key == spec.key {
                if let Some(f) = f.take() {
                    f(slot);
                }
            }
        });
    }
}

/// The longest setup name in bytes: the file name (`<surface>_<name>.lsp`) stays well inside a file system's 255.
const MAX_NAME: usize = 100;

/// A setup name the page saves under: trimmed, 1 to [`MAX_NAME`] bytes, usable in a file name.
fn valid_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if !is_file_name_part(name) || name.len() > MAX_NAME {
        return Err(format!("a name of 1 to {MAX_NAME} bytes, without / \\ : * ? \" < > |"));
    }
    Ok(name)
}

fn range_text(spec: &Spec, range: &Range) -> String {
    let unit = spec.unit;
    let wrap = if range.wrap { ", wraps" } else { "" };
    format!("{} to {}, step {}{wrap}", unit.show(range.min), unit.show(range.max), unit.show(range.step))
}

/// How a value is edited.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Widget {
    Slider,
    Field,
    /// A field without the unit or the step buttons (the diff map's table).
    Cell,
}

/// A value's widget: − and + take the game's steps; the field or slider takes the steps toward what is set.
fn value_ui(ui: &mut Ui, spec: &Spec, range: Range, slot: Slot<'_>, widget: Widget) {
    let Slot::Real(value) = slot else {
        return;
    };
    let unit = spec.unit;
    let fixed = range.max <= range.min || range.step <= 0.0;
    ui.horizontal(|ui| {
        ui.add_enabled_ui(!fixed, |ui| {
            if widget != Widget::Cell && ui.small_button("−").clicked() {
                *value = range.down(*value);
            }
            let mut shown = *value * unit.scale;
            let bounds = range.min * unit.scale..=range.max * unit.scale;
            let suffix = if widget == Widget::Cell { String::new() } else { format!(" {}", unit.suffix) };
            let response = if widget == Widget::Slider {
                ui.add(
                    egui::Slider::new(&mut shown, bounds)
                        .fixed_decimals(unit.decimals)
                        .suffix(suffix)
                        .clamping(egui::SliderClamping::Never),
                )
            } else {
                ui.add(
                    DragValue::new(&mut shown)
                        .range(bounds)
                        .speed(range.step * unit.scale)
                        .fixed_decimals(unit.decimals)
                        .suffix(suffix),
                )
            };
            if response.changed() {
                *value = range.toward(*value, shown / unit.scale);
            }
            if widget != Widget::Cell && ui.small_button("+").clicked() {
                *value = range.up(*value);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: PathBuf) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    fn names(profiles: &[Profile]) -> Vec<(String, Source)> {
        profiles.iter().map(|p| (p.name.clone(), p.source)).collect()
    }

    /// An RSF car's setups are ours, then the install's saved ones, then its shipped ones, on the surface only: RSF's
    /// `<surface>_<name>.lsp`, without its copies of the defaults, a name once from the first folder with it. Then
    /// ours of earlier versions under its own key and slot 5's.
    #[test]
    fn an_rsf_cars_setups_are_by_pack_and_surface() {
        let root = crate::test_dir("setup-rsf");
        let (here, rsf, install) = (root.join("here"), root.join("rsf"), root.join("install"));
        let pack = "Ford_Fiesta_R5_ngp6";
        write(here.join("SavedGames").join(pack).join("gravel_Mine.lsp"));
        write(rsf.join("SavedGames").join(pack).join("gravel_Vasgabi.lsp"));
        write(rsf.join("SavedGames").join(pack).join("gravel_mine.lsp"));
        write(rsf.join("SavedGames").join(pack).join("tarmac_Vasgabi.lsp"));
        write(rsf.join("SavedGames").join(pack).join("37_d_gravel.lsp"));
        write(rsf.join("rsfdata/setups").join(pack).join("GRAVEL_Vasgabi.LSP"));
        write(rsf.join("rsfdata/setups").join(pack).join("gravel_Asphalt Kid.lsp"));
        write(here.join("Physics/rsf-37/Setups/before.lsp"));
        write(here.join("Physics/s_i2003/Setups/slot five.lsp"));
        write(here.join("SavedGames/rsf-37slot0_own slot.lsp"));
        write(here.join("SavedGames/5slot1_old slot.lsp"));
        write(here.join("SavedGames/rsf-38slot0_other car.lsp"));
        let rsf_car = crate::catalog::RsfCar {
            id: 37,
            added: false,
            group: None,
            pack: Ok(rsf.join("rsfdata/cars").join(format!("{pack}.zip"))),
            model: Ok(None),
            drive_train: None,
        };
        let car = Car { index: rbr_formats::rsf::SLOT, name: "Ford Fiesta R5".into(), rsf: Some(rsf_car) };
        let folders = Folders::of(&install, &here, &rsf, &car);
        assert_eq!(
            names(&folders.scan("gravel")),
            [
                ("Mine".to_owned(), Source::Own),
                ("Vasgabi".to_owned(), Source::Rsf),
                ("Asphalt Kid".to_owned(), Source::RsfShipped),
                ("before".to_owned(), Source::Earlier),
                ("slot five".to_owned(), Source::Earlier),
                ("old slot".to_owned(), Source::Slot),
                ("own slot".to_owned(), Source::Slot),
            ]
        );
        let tarmac = names(&folders.scan("tarmac"));
        assert_eq!(tarmac[0], ("Vasgabi".to_owned(), Source::Rsf));
        assert_eq!(tarmac.len(), 5, "the earlier ones are on every surface");
        assert_eq!(folders.file("gravel", "fast"), Some(here.join("SavedGames").join(pack).join("gravel_fast.lsp")));
    }

    /// A retail car's setups are ours, then the install's, under the slot's folder, by any name, then the car's game
    /// slots of earlier versions.
    #[test]
    fn a_retail_cars_setups_are_the_games() {
        let root = crate::test_dir("setup-retail");
        let (here, install) = (root.join("here"), root.join("install"));
        write(here.join("Physics/c_xsara/Setups/b fast.lsp"));
        write(install.join("Physics/c_xsara/Setups/a safe.lsp"));
        write(install.join("Physics/c_xsara/Setups/notes.txt"));
        write(here.join("SavedGames/0slot2_rally.lsp"));
        write(here.join("SavedGames/3slot0_lancer.lsp"));
        write(here.join("SavedGames/pfplayer.rbr"));
        let car = Car { index: 0, name: "XSARA".into(), rsf: None };
        let folders = Folders::of(&install, &here, &root.join("rsf"), &car);
        assert_eq!(
            names(&folders.scan("tarmac")),
            [
                ("b fast".to_owned(), Source::Own),
                ("a safe".to_owned(), Source::Game),
                ("rally".to_owned(), Source::Slot)
            ]
        );
        assert_eq!(folders.file("tarmac", "x"), Some(here.join("Physics/c_xsara/Setups/x.lsp")));
    }

    #[test]
    fn setup_names_take_a_long_name_but_nothing_that_leaves_the_folder() {
        assert_eq!(
            valid_name("  fast gravel, wet, soft tyres, long final drive  "),
            Ok("fast gravel, wet, soft tyres, long final drive")
        );
        assert!(valid_name(&"a".repeat(MAX_NAME)).is_ok());
        for bad in ["", "  ", "../x", "a\\b", "c:x", &"a".repeat(MAX_NAME + 1)] {
            assert!(valid_name(bad).is_err(), "{bad:?}");
        }
    }

    /// Every NGP car pack of the RSF install (`RBR_RSF`) with plain setup files gives the page its defaults and ranges
    /// on each surface.
    #[test]
    fn every_rsf_pack_has_its_setup() {
        let Some(rsf) = std::env::var_os("RBR_RSF") else { return };
        let packs = std::fs::read_dir(Path::new(&rsf).join("rsfdata/cars")).unwrap();
        let mut checked = 0;
        for pack in packs.filter_map(|e| Some(e.ok()?.path())) {
            for surface in ["gravel", "tarmac", "snow"] {
                match pack_setup(&pack, surface) {
                    Ok(car) => assert!(!car.ranges.is_empty(), "{}: {surface}: no ranges", pack.display()),
                    Err(e) if e.contains("RSF-encrypted") => continue,
                    Err(e) => panic!("{}: {surface}: {e}", pack.display()),
                }
                checked += 1;
            }
        }
        eprintln!("{checked} pack setups");
        assert!(checked > 0);
    }
}
