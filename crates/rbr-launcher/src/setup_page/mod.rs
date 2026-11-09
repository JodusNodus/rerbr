//! The Car setup page: the manager of the car's saved setups on the stage's surface. It opens one (or the default),
//! edits its values grouped by area, within the car options' ranges and by the game's steps (`rbr_gameplay::setup`),
//! and saves them in the game's `Setups/*.lsp` format. The areas are picked on a map of the car (`board`), each
//! value is drawn against a comparison the player picks (`values`, `editor`), and the areas' diagrams (`charts`)
//! show what the values do. The Drive page picks the one to drive with
//! ([`SetupPage::choice_ui`]), kept per car and surface (`[setup]` of the settings, [`SetupChoices`]).
//!
//! Ours are `setups/<car>/<surface>_<name>.lsp` in the player's folder for every car (`<car>`: a pack car's
//! pack, an archive car's physics folder), named as RSF names its own. Listed beside them, read only: a pack car's
//! setups of the RSF install (its `SavedGames/<pack>/` and the ones it ships in `rsfdata/setups/<pack>/`) and ours of
//! earlier versions ([`Folders`]); the 1.02 install's `Physics/<car>/Setups/` are not read (made for 1.02's car
//! ranges, not NGP's). Start
//! writes the chosen values to `setup-current.lsp` in the player's folder and passes it to `rbr-standalone --setup`,
//! unless they are the default.
//!
//! The game's three save slots per car (`CSetupSaveSlotButtonUI`, `SavedGames/<car>slot<n>_<name>.lsp`,
//! `rbr_formats::saves::setup_slots`) are not offered; earlier versions' slot files are among the read-only ones. The
//! championship offers the stage's default, the current setup or a saved one before each stage
//! (`CSeasonChooseSetupUI`, [`SetupPage::choose`]).

mod board;
mod charts;
mod editor;
mod values;

use std::path::{Path, PathBuf};

use charts::DIFF;
use egui::{ComboBox, RichText, Ui};
use rbr_formats::saves::{is_file_name_part, setup_slots, DIR as SAVES};
use rbr_gameplay::setup::{Area, Bounds, CarSetup, SeasonSetup, Setup, Spec, CAR_FOLDERS};

use crate::catalog::{self, Car};
use crate::pages::{GOOD, WARN};
use crate::settings::SetupChoices;

/// How the lists name the car's default setup.
const DEFAULT: &str = "Default setup";

/// The defaults and ranges of the NGP car pack at `path` on `surface`.
fn pack_setup(path: &Path, surface: &str) -> Result<CarSetup, String> {
    let pack = rbr_formats::car_pack::Pack::open(path)?;
    let text = |name: String| pack.read(&name).map(|b| String::from_utf8_lossy(&b).into_owned());
    CarSetup::parse(&text(format!("setups/d_{surface}.lsp"))?, &text(format!("r_{surface}.lsp"))?)
}

/// The setups' surfaces by the map's surface (`catalog::Map::surface`), as the files name them.
pub const SURFACES: [&str; 3] = ["gravel", "tarmac", "snow"];

/// Where a saved setup is from. Ours are saved and deleted here; the others are only read: ours from earlier versions
/// (see [`Folders`]), the game's save slots, and the RSF install's (its players' `SavedGames/<pack>/` and the ones it
/// ships in `rsfdata/setups/<pack>/`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Own,
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
            Source::Earlier => "earlier",
            Source::Slot => "game slot",
            Source::Rsf => "RSF",
            Source::RsfShipped => "RSF's",
        }
    }
}

/// A saved setup of the car, listed for a stage's surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    /// The surface it was saved for when that is not the stage's; `None` for the stage's or an unknown one.
    pub away: Option<&'static str>,
    /// How the lists and the remembered choice tell it: the name, after `<surface>/` when it is `away`.
    pub id: String,
    pub source: Source,
    path: PathBuf,
}

impl Profile {
    pub fn label(&self) -> String {
        let tags: Vec<&str> = self.away.into_iter().chain(Some(self.source.tag()).filter(|t| !t.is_empty())).collect();
        if tags.is_empty() {
            self.name.clone()
        } else {
            format!("{} ({})", self.name, tags.join(", "))
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

/// Our setups' folder in the player's folder: `setups/<car>/<surface>_<name>.lsp` for every car, named as RSF names
/// them (so a file copies into the install's `SavedGames/<pack>/` as it is).
const SETUPS: &str = "setups";

/// A folder of setup files, and whether their names start with the surface (`<surface>_<name>.lsp`, RSF's and ours;
/// RSF's `<id>_d_<surface>.lsp` copies of the defaults don't match).
#[derive(Debug)]
struct Dir {
    source: Source,
    path: PathBuf,
    by_surface: bool,
}

/// The folders a car's setups are in: ours in [`SETUPS`], then the read-only ones. A pack car's: the install's
/// `SavedGames/<pack>/` and `rsfdata/setups/<pack>/`. Then ours of earlier versions, named as the player typed: a
/// archive car's `Physics/<car>/Setups/`, a pack car's
/// `SavedGames/<pack>/` and `Physics/<key>/Setups/` under its setup keys (`Car::setup_keys`: its own, then slot 5's),
/// and last the game's save slots of the keys in our `SavedGames/` (`<key>slot<n>_<name>.lsp`).
#[derive(Debug, Default)]
struct Folders {
    dirs: Vec<Dir>,
    keys: Vec<String>,
}

impl Folders {
    fn of(here: &Path, rsf: &Path, car: &Car) -> Folders {
        let Some(folder) = car_folder(car) else {
            return Folders::default();
        };
        let keys = car.setup_keys();
        let dir = |source, path, by_surface| Dir { source, path, by_surface };
        // Our earlier `Physics/<car>/Setups/` of a setup key.
        let physics = |key: &String| {
            let folder = match key.parse::<usize>() {
                Ok(n) => CAR_FOLDERS.get(n)?.to_string(),
                Err(_) => format!("Physics/{key}/"),
            };
            Some(here.join(folder).join("Setups"))
        };
        let mut dirs = vec![dir(Source::Own, here.join(SETUPS).join(&folder), true)];
        if matches!(car.physics, catalog::Physics::Pack(_)) {
            dirs.push(dir(Source::Rsf, rsf.join(SAVES).join(&folder), true));
            dirs.push(dir(Source::RsfShipped, rsf.join("rsfdata/setups").join(&folder), true));
            dirs.push(dir(Source::Earlier, here.join(SAVES).join(&folder), true));
        }
        dirs.extend(keys.iter().filter_map(physics).map(|p| dir(Source::Earlier, p, false)));
        dirs.push(dir(Source::Slot, here.join(SAVES), false));
        Folders { dirs, keys }
    }

    /// Our file of setup `name` on `surface`.
    fn file(&self, surface: &str, name: &str) -> Option<PathBuf> {
        Some(self.dirs.first()?.path.join(format!("{surface}_{name}.lsp")))
    }

    /// The surface (`None` when its name has none) and the name of the setup a file named `file` in `dir` is.
    fn name_of(&self, dir: &Dir, file: &str) -> Option<(Option<&'static str>, String)> {
        if dir.source == Source::Slot {
            let (key, _, name) = setup_slots::parse_name(file)?;
            return self.keys.contains(&key).then_some((None, name));
        }
        let stem = strip_suffix_ignore_case(file, ".lsp")?;
        if !dir.by_surface {
            return Some((None, stem.to_owned()));
        }
        SURFACES.into_iter().find_map(|s| Some((Some(s), strip_prefix_ignore_case(stem, &format!("{s}_"))?.to_owned())))
    }

    /// Every setup of the car, listed for a stage on `surface`: the surface's first, then the other surfaces', then
    /// those of no known surface; within each, ours first, then each other folder's, by name. A setup is listed once,
    /// from the first folder that has it.
    fn scan(&self, surface: &str) -> Vec<Profile> {
        // Each setup with its group: 0 the surface's, 1 another surface's (by `SURFACES`), 2 of no known surface.
        let group = |on: Option<&str>| match on {
            Some(s) if s == surface => 0,
            Some(s) => 1 + SURFACES.iter().position(|x| *x == s).unwrap_or(0),
            None => SURFACES.len() + 1,
        };
        let mut found: Vec<(usize, Profile)> = Vec::new();
        for dir in &self.dirs {
            let Ok(entries) = std::fs::read_dir(&dir.path) else { continue };
            let mut here: Vec<(usize, Profile)> = entries
                .filter_map(|e| {
                    let path = e.ok()?.path();
                    let (on, name) = self.name_of(dir, path.file_name()?.to_str()?)?;
                    let away = on.filter(|s| *s != surface);
                    let id = away.map_or_else(|| name.clone(), |s| format!("{s}/{name}"));
                    (!name.is_empty() && path.is_file())
                        .then(|| (group(on), Profile { name, away, id, source: dir.source, path }))
                })
                .filter(|(_, p)| !found.iter().any(|(_, q)| q.id.eq_ignore_ascii_case(&p.id)))
                .collect();
            here.sort_by_key(|(_, p)| p.name.to_lowercase());
            found.extend(here);
        }
        found.sort_by_key(|(group, _)| *group);
        found.into_iter().map(|(_, p)| p).collect()
    }
}

/// The folder a car's setups are kept under: a pack car's pack name, as RSF's own folders name it; an archive
/// car's physics folder (`c_xsara`).
fn car_folder(car: &Car) -> Option<String> {
    match &car.physics {
        catalog::Physics::Pack(pack) => pack_folder(pack),
        catalog::Physics::Archive => {
            let folder = CAR_FOLDERS.get(car.slot as usize)?.trim_end_matches('/');
            Some(folder.rsplit('/').next()?.to_owned())
        }
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
    /// The car's values: their specs and ranges (`values::fields`).
    fields: Vec<values::Field>,
    folders: Folders,
    profiles: Vec<Profile>,
    /// The setup each car drives with on each surface, the Drive page's choice.
    pub choices: SetupChoices,
    /// The values Start drives with: the chosen setup's, or a championship's (`choose`, `season`).
    pub setup: Setup,
    season: bool,
    /// The editor: the setup it opened (`None` the default), its values as opened and as edited, and the name it
    /// saves under.
    opened: Option<String>,
    opened_values: Setup,
    draft: Setup,
    name: String,
    confirm_delete: bool,
    /// What the editor's values are compared with, and its values.
    compare: Compare,
    base: Setup,
    /// By `values::WHEEL_AREAS`, front and rear: whether the left and right wheels are set together.
    linked: [[bool; 2]; 4],
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
            fields: Vec::new(),
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
            compare: Compare::Default,
            base: Setup::default(),
            linked: [[true; 2]; 4],
            area: Area::Tyres,
            map: 0,
            status: String::new(),
        }
    }

    /// Loads the car's defaults and ranges when the car or surface changes, then the setup chosen for them, which
    /// the editor opens. A pack car's come from its pack (`setups/d_<surface>.lsp`, `r_<surface>.lsp`), as the game
    /// reads them once RSF has copied the pack into the slot; its saved setups are RSF's per pack and its game slots
    /// the slot's. An archive slot's come from NGP's `physics.rbz` (`archive`), which has the NGP car installed in it.
    pub fn select(&mut self, here: &Path, rsf: &Path, car: &Car, archive: &Path, surface: &'static str) {
        if self.key == Some((car.slot, surface)) && self.car_name == car.name {
            return;
        }
        self.key = Some((car.slot, surface));
        self.car_name = car.name.clone();
        self.car = match &car.physics {
            catalog::Physics::Pack(pack) => match &car.model {
                Ok(_) => pack_setup(pack, surface),
                Err(e) => Err(e.clone()),
            },
            catalog::Physics::Archive => CarSetup::load_in(archive, car.slot as usize, surface),
        };
        self.fields = values::fields(self.car.as_ref().map_or(&[][..], |c| &c.ranges));
        self.folders = Folders::of(here, rsf, car);
        if matches!(self.compare, Compare::Saved(_)) {
            self.compare = Compare::Default;
        }
        // The first diff map of a diff the car has (its torque not fixed at 0).
        let torques = ["CenterDiffMaxTorque", "CenterDiffMaxTorque", "FrontDiffMaxTorque", "RearDiffMaxTorque"];
        let fitted = |key: &str| values::find(&self.fields, key).is_some_and(|i| self.fields[i].range.max > 0.0);
        self.map = torques.iter().position(|k| fitted(k)).unwrap_or(0);
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

    /// Steps some values away from the opened setup's (`--screenshot`), so the captures show differences.
    pub fn screenshot_changes(&mut self) {
        let fields = &self.fields;
        values::edit(&mut self.draft, &self.base, |now, _| {
            for (i, field) in fields.iter().enumerate() {
                let (key, index) = (field.spec.key.as_str(), field.spec.index);
                let knot = key.strip_prefix("FrontDiffThrottle_").and_then(|n| n.parse::<usize>().ok());
                let steps = match key {
                    "SpringStiffness" | "DampingBump" | "DampingRebound" | "WheelAxisInclination"
                        if index < Some(2) =>
                    {
                        3
                    }
                    "Pressure" if index == Some(2) => 2,
                    "FrontRollBarStiffness" | "MaxBrakePressureFront" => 1,
                    _ if knot.is_some_and(|n| n >= 4) => 25,
                    _ => 0,
                };
                now[i] = (0..steps).fold(now[i], |v, _| field.range.up(v));
            }
        });
        self.map = 2;
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
        self.profiles.iter().find(|p| p.id == name).ok_or(format!("{name}: not found"))?.text()
    }

    /// The names of the car's saved setups on the map surface `surface` (`catalog::Map::surface`).
    pub fn saved_names(&self, surface: u32) -> Vec<(String, String)> {
        let surface = SURFACES.get(surface as usize).copied().unwrap_or(SURFACES[0]);
        self.folders.scan(surface).into_iter().map(|p| (p.id.clone(), p.label())).collect()
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
        self.name =
            name.and_then(|id| self.profiles.iter().find(|p| p.id == id)).map_or_else(String::new, |p| p.name.clone());
        self.opened = name.map(str::to_owned);
        self.opened_values = values;
        self.draft = values;
        self.confirm_delete = false;
        self.linked = editor::linked_for(&values, &self.fields);
        self.rebase();
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
                self.profiles.iter().find(|p| p.id == *name).map_or(format!("{name} (missing)"), Profile::label)
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
                        if ui.selectable_label(chosen.as_deref() == Some(&p.id), p.label()).clicked() {
                            pick = Some(Some(p.id.clone()));
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
        let Some(profile) = self.opened.as_ref().and_then(|n| self.profiles.iter().find(|p| p.id == *n)).cloned()
        else {
            return;
        };
        if let Err(e) = std::fs::remove_file(&profile.path) {
            self.status = format!("{}: {e}", profile.path.display());
            return;
        }
        self.profiles = self.folders.scan(self.surface());
        if self.chosen() == Some(profile.id.as_str()) {
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
        self.header_ui(ui, &car);
        ui.separator();
        let (now, base) = (values::values(&self.draft), values::values(&self.base));
        let lines = values::changes(&self.fields, &base, &now, |i, v| self.show(&self.fields[i].spec, v));
        let differs = board::Differs::of(&self.fields, &base, &now);
        let gear = values::find(&self.fields, "DropGearId").map(|i| self.show(&self.fields[i].spec, now[i]));
        let view = board::View::of(&self.fields, &self.draft, &lines, differs, gear);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(board::SIZE.x);
                if let Some(area) = board::ui(ui, self.area, &view) {
                    self.area = area;
                }
                ui.add_space(8.0);
                self.changes_ui(ui, &lines);
            });
            ui.separator();
            egui::ScrollArea::vertical().id_salt("setup-area").show(ui, |ui| {
                ui.vertical(|ui| self.area_ui(ui));
            });
        });
    }

    /// The setup opened, the one driven with, the name to save under and the buttons.
    fn header_ui(&mut self, ui: &mut Ui, car: &CarSetup) {
        let opened = self.opened.as_ref().and_then(|n| self.profiles.iter().find(|p| p.id == *n)).cloned();
        ui.horizontal(|ui| {
            ui.label("Setup").on_hover_text(format!(
                "Every setup of this car, the {} ones first; Save keeps it for {0}",
                self.surface()
            ));
            let selected = opened.as_ref().map_or(DEFAULT.to_owned(), Profile::label);
            if let Some(name) = self.pick_ui(ui, "setup-open", selected, self.opened.clone()) {
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
                ui.label(RichText::new("✔ driving with this").color(GOOD));
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
                self.profiles.iter().any(|p| p.id.eq_ignore_ascii_case(self.name.trim()) && p.source == Source::Own);
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
            let surface = self.surface();
            let note = match (changed, opened.as_ref().map(|p| (p.source, p.away))) {
                (true, _) => "unsaved changes: Save to drive with them".to_owned(),
                (false, Some((_, Some(away)))) => format!("a {away} setup: Save keeps a copy for {surface}"),
                (false, Some((Source::Own, None))) | (false, None) => String::new(),
                (false, Some((source, None))) => {
                    format!("{} setup, read only: Save keeps a copy as yours", source.tag())
                }
            };
            ui.add_space(6.0);
            ui.colored_label(if changed { WARN } else { egui::Color32::GRAY }, note);
        });
        if !self.status.is_empty() {
            ui.label(&self.status);
        }
    }

    /// A list of the default and the saved setups, `chosen` marked; the one picked (`Some(None)` the default).
    fn pick_ui(&self, ui: &mut Ui, id: &str, selected: String, chosen: Option<String>) -> Option<Option<String>> {
        let mut pick = None;
        ComboBox::from_id_salt(id).width(220.0).selected_text(selected).show_ui(ui, |ui| {
            if ui.selectable_label(chosen.is_none(), DEFAULT).clicked() {
                pick = Some(None);
            }
            for p in &self.profiles {
                if ui.selectable_label(chosen.as_deref() == Some(&p.id), p.label()).clicked() {
                    pick = Some(Some(p.id.clone()));
                }
            }
        });
        pick
    }

    /// The comparison's values: the default's, the opened setup's as opened, or a saved setup's.
    fn rebase(&mut self) {
        self.base = match &self.compare {
            Compare::Default => self.default_values(),
            Compare::Opened => self.opened_values,
            Compare::Saved(id) => match self.read(id) {
                Ok(values) => values,
                Err(e) => {
                    self.status = format!("{e}; comparing with the default setup");
                    self.compare = Compare::Default;
                    self.default_values()
                }
            },
        };
    }

    /// The comparison's picker and the lines that differ from it, each with its way back.
    fn changes_ui(&mut self, ui: &mut Ui, lines: &[values::Change]) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Compared with").strong()).on_hover_text(
                "The ticks on the tracks, the dashed curves and the orange marks show this setup's values",
            );
            let selected = match &self.compare {
                Compare::Default => DEFAULT.to_owned(),
                Compare::Opened => "As opened".to_owned(),
                Compare::Saved(id) => self.profiles.iter().find(|p| p.id == *id).map_or(id.clone(), Profile::label),
            };
            let mut pick = None;
            ComboBox::from_id_salt("setup-compare").width(200.0).selected_text(selected).show_ui(ui, |ui| {
                if ui.selectable_label(self.compare == Compare::Default, DEFAULT).clicked() {
                    pick = Some(Compare::Default);
                }
                let opened = ui.selectable_label(self.compare == Compare::Opened, "As opened");
                if opened.on_hover_text("The opened setup's values before this page's changes").clicked() {
                    pick = Some(Compare::Opened);
                }
                for p in &self.profiles {
                    let on = self.compare == Compare::Saved(p.id.clone());
                    if ui.selectable_label(on, p.label()).clicked() {
                        pick = Some(Compare::Saved(p.id.clone()));
                    }
                }
            });
            if let Some(compare) = pick {
                self.compare = compare;
                self.rebase();
            }
        });
        if lines.is_empty() {
            ui.label(RichText::new("No differences.").weak());
            return;
        }
        let count = if lines.len() == 1 { "1 difference".to_owned() } else { format!("{} differences", lines.len()) };
        ui.label(RichText::new(count).color(DIFF));
        let mut back = None;
        egui::ScrollArea::vertical().id_salt("setup-changes").auto_shrink([false, true]).show(ui, |ui| {
            for (n, line) in lines.iter().enumerate() {
                ui.horizontal_top(|ui| {
                    if ui.small_button("↺").on_hover_text("Back to the compared value").clicked() {
                        back = Some(n);
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let what = ui.add(egui::Label::new(&line.what).sense(egui::Sense::click()));
                        if what.on_hover_text(format!("Show {}", line.area.name())).clicked() {
                            self.area = line.area;
                        }
                        let text = if line.from.is_empty() {
                            line.to.clone()
                        } else {
                            format!("{}  ⏵  {}", line.from, line.to)
                        };
                        ui.label(RichText::new(text).color(DIFF).size(12.0));
                    });
                });
                ui.add_space(2.0);
            }
        });
        if let Some(n) = back {
            values::edit(&mut self.draft, &self.base, |now, base| {
                lines[n].indices.iter().for_each(|&i| now[i] = base[i])
            });
        }
    }

    /// A value as the page shows it: the drop gear by its teeth.
    fn show(&self, spec: &Spec, value: f32) -> String {
        if spec.bounds == Bounds::DropGear {
            let gears = self.car.as_ref().map_or(&[][..], |c| &c.gears[..]);
            let id = value as u32;
            return gears.iter().find(|g| g.0 == id).map_or(format!("{id}"), |&(_, p, g)| format!("{p}:{g}"));
        }
        spec.unit.show(value)
    }
}

/// What the page compares the editor's values with.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Compare {
    Default,
    Opened,
    Saved(String),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: PathBuf) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    fn names(profiles: &[Profile]) -> Vec<(String, Source)> {
        profiles.iter().map(|p| (p.id.clone(), p.source)).collect()
    }

    /// A pack car's setups are ours, then the install's saved ones, then its shipped ones, on the surface only: the install's
    /// `<surface>_<name>.lsp`, without its copies of the defaults, a name once from the first folder with it. Then
    /// ours of earlier versions under its own key and slot 5's.
    #[test]
    fn a_pack_cars_setups_are_by_pack_and_surface() {
        let root = crate::test_dir("setup-rsf");
        let (here, rsf) = (root.join("here"), root.join("rsf"));
        let pack = "Ford_Fiesta_R5_ngp6";
        write(here.join("setups").join(pack).join("gravel_Mine.lsp"));
        write(here.join("SavedGames").join(pack).join("gravel_Today.lsp"));
        write(here.join("SavedGames").join(pack).join("gravel_mine.lsp"));
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
        let car = Car {
            name: "Ford Fiesta R5".into(),
            slot: rbr_formats::car_pack::SLOT,
            physics: catalog::Physics::Pack(rsf.join("rsfdata/cars").join(format!("{pack}.zip"))),
            model: Ok(rbr_formats::car_pack::Model {
                entry: rbr_formats::car_pack::model_entry("Fiesta_R5", "Fiesta_R5"),
                fallback: None,
            }),
            key: "rsf-37".into(),
            ..Car::default()
        };
        let folders = Folders::of(&here, &rsf, &car);
        assert_eq!(
            names(&folders.scan("gravel")),
            [
                ("Mine".to_owned(), Source::Own),
                ("Vasgabi".to_owned(), Source::Rsf),
                ("Asphalt Kid".to_owned(), Source::RsfShipped),
                ("Today".to_owned(), Source::Earlier),
                ("tarmac/Vasgabi".to_owned(), Source::Rsf),
                ("before".to_owned(), Source::Earlier),
                ("slot five".to_owned(), Source::Earlier),
                ("old slot".to_owned(), Source::Slot),
                ("own slot".to_owned(), Source::Slot),
            ]
        );
        // On tarmac every setup is still there, the tarmac one first and the gravel ones told by their surface.
        let tarmac = folders.scan("tarmac");
        assert_eq!(
            names(&tarmac[..5]),
            [
                ("Vasgabi".to_owned(), Source::Rsf),
                ("gravel/Mine".to_owned(), Source::Own),
                ("gravel/Vasgabi".to_owned(), Source::Rsf),
                ("gravel/Asphalt Kid".to_owned(), Source::RsfShipped),
                ("gravel/Today".to_owned(), Source::Earlier),
            ]
        );
        assert_eq!(tarmac.len(), 9);
        assert_eq!((tarmac[0].label(), tarmac[1].label()), ("Vasgabi (RSF)".to_owned(), "Mine (gravel)".to_owned()));
        assert_eq!(tarmac[2].label(), "Vasgabi (gravel, RSF)");
        assert_eq!(folders.file("gravel", "fast"), Some(here.join("setups").join(pack).join("gravel_fast.lsp")));
    }

    /// An archive car's setups are ours on the surface, then on the others, then ours of earlier versions under the
    /// slot's physics folder, by any name, then the car's game slots; the 1.02 install's are not read.
    #[test]
    fn a_retail_cars_setups_are_ours() {
        let root = crate::test_dir("setup-retail");
        let (here, install) = (root.join("here"), root.join("install"));
        write(here.join("setups/c_xsara/tarmac_b fast.lsp"));
        write(here.join("setups/c_xsara/gravel_loose.lsp"));
        write(here.join("Physics/c_xsara/Setups/old.lsp"));
        write(install.join("Physics/c_xsara/Setups/a safe.lsp"));
        write(install.join("Physics/c_xsara/Setups/notes.txt"));
        write(here.join("SavedGames/0slot2_rally.lsp"));
        write(here.join("SavedGames/3slot0_lancer.lsp"));
        write(here.join("SavedGames/pfplayer.rbr"));
        let car = Car { slot: 0, name: "XSARA".into(), key: "XSARA".into(), ..Car::default() };
        let folders = Folders::of(&here, &root.join("rsf"), &car);
        assert_eq!(
            names(&folders.scan("tarmac")),
            [
                ("b fast".to_owned(), Source::Own),
                ("gravel/loose".to_owned(), Source::Own),
                ("old".to_owned(), Source::Earlier),
                ("rally".to_owned(), Source::Slot)
            ]
        );
        assert_eq!(folders.file("tarmac", "x"), Some(here.join("setups/c_xsara/tarmac_x.lsp")));
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
