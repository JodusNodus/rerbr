//! The car browser: the Cars page, or the picker the Drive, Circuit and Championship pages open. A page header, a filter
//! bar (search, the scopes all / favourites / recent / added / cannot start, class, drive and sort), the list with its
//! header row and key hints, and the highlighted car's inspector on the right (the car turning, its facts, its buttons).
//! A car RSF has not downloaded stays in its class, greyed with the reason, and cannot be chosen. [`lines`] makes the
//! list from the rows and what the player filtered, apart from any drawing; the keys and the list's plumbing are the
//! stage browser's (`browser`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use egui::{FontId, Pos2, Rect, RichText};

use crate::browser::{
    self, block, drive_button, favourite_button, header_line, items, key_action, list_rows, paint_cell,
    paint_selection, paint_star, random_row, row_slot, step_nav, Line, Nav,
};
use crate::car_skins::Skins;
use crate::catalog::{self, Car};
use crate::kit;
use crate::preview::Previews;
use crate::theme::{self, TEXT, TEXT_2, TEXT_3, TEXT_DISABLED, TEXT_LIST};

/// The classes of the filter bar, newest to oldest, with the RSF base groups each takes in (RSF splits some finely).
const CLASSES: [(&str, &[&str]); 14] = [
    ("WRC 1.6", &["WRC 1.6"]),
    ("WRC 2.0", &["WRC 2.0"]),
    ("Rally 2 / R5", &["Rally 2", "Group R5"]),
    ("Rally 3 / R3", &["Rally 3", "Group R3"]),
    ("Rally 4 / R2", &["Rally 4", "Group R2"]),
    ("Rally 5 / R1", &["Rally 5", "Group R1"]),
    ("Group R4 / N4", &["Group R4", "Group N4"]),
    ("Super 2000", &["Super 2000"]),
    ("Super 1600", &["Super 1600"]),
    ("Group RGT", &["Group RGT"]),
    ("Group A", &["Group A5", "Group A6", "Group A7", "Group A8"]),
    ("Group B", &["Group B"]),
    ("Group 4", &["Group 4"]),
    ("Group 2", &["Group 2"]),
];

/// The 1.02 cars by their folder (`Cars.ini`): the name (`Root/audio.dat`'s car names; its slot 7 repeats the Corolla's,
/// so the Impreza's is the folder's), the RSF group it belongs to, and the drive. The classes are ours.
const RETAIL: [(&str, &str, &str, Drive); 8] = [
    ("XSARA", "Citroen Xsara 00", "WRC 2.0", Drive::Awd),
    ("ACCENT", "Hyundai Accent 00", "WRC 2.0", Drive::Awd),
    ("ZR", "MG ZR Super 1600", "Super 1600", Drive::Fwd),
    ("LANCER_EVO_VII", "Mitsubishi Lancer Evo VII", "Group N4", Drive::Awd),
    ("PEUGEOT_206", "Peugeot 206 99", "WRC 2.0", Drive::Awd),
    ("IMPREZA03", "Subaru Impreza 2001 A8", "Group A8", Drive::Awd),
    ("COROLLA", "Toyota Corolla 98", "WRC 2.0", Drive::Awd),
    ("IMPREZA00", "Subaru Impreza 2000", "WRC 2.0", Drive::Awd),
];

/// The class for cars without one: the added cars' packs have none.
const OTHER: &str = "Other";

/// Where the car comes from, as the detail panel says it: the same car can be both RSF's and added (one key,
/// [`catalog::pack_key`]), listed twice.
pub fn source(car: &Car) -> String {
    match (&car.physics, car.rsf_id) {
        (catalog::Physics::Archive, _) => format!("RBR 1.02, slot {}", car.slot),
        (_, _) if car.added => "Added, in your cars folder".to_owned(),
        (_, Some(id)) => format!("RallySimFans install, car {id}"),
        (_, None) => "RallySimFans install".to_owned(),
    }
}

/// Which wheels drive the car.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drive {
    Awd,
    Fwd,
    Rwd,
}

impl Drive {
    pub const ALL: [Drive; 3] = [Drive::Awd, Drive::Fwd, Drive::Rwd];

    pub fn label(self) -> &'static str {
        match self {
            Drive::Awd => "4WD",
            Drive::Fwd => "FWD",
            Drive::Rwd => "RWD",
        }
    }

    /// RSF's words for the drive train (`AWD with torque split`, `FWD`).
    fn parse(words: &str) -> Option<Drive> {
        let words = words.trim().to_ascii_uppercase();
        match words.get(..3)? {
            "AWD" | "4WD" => Some(Drive::Awd),
            "FWD" => Some(Drive::Fwd),
            "RWD" => Some(Drive::Rwd),
            _ => None,
        }
    }
}

/// The model a car is drawn with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Model {
    Own,
    /// A retail car's model standing in for its own (why).
    StandIn(String),
    /// None: the car cannot be drawn.
    Missing,
}

/// The class of the filter bar's RSF base group `group` is in, and its place among them.
fn family(group: Option<&str>) -> (String, usize) {
    let Some(group) = group else { return (OTHER.to_owned(), CLASSES.len() + 1) };
    match CLASSES.iter().position(|(_, groups)| groups.contains(&group)) {
        Some(at) => (CLASSES[at].0.to_owned(), at),
        None => (group.to_owned(), CLASSES.len()),
    }
}

/// One car of the catalog as the browser lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct CarRow {
    /// Its position in the catalog's cars.
    pub index: usize,
    /// The catalog's name: what the favourites and the recent picks keep.
    pub name: String,
    /// The name shown: the 1.02 cars' are ours ([`RETAIL`]).
    pub title: String,
    /// Its group (`Group A7`) and the class of the filter bar it is in (`Group A`), which comes with its place.
    pub group: String,
    pub class: String,
    rank: usize,
    /// Where it comes from: `RBR 1.02`, `RSF 94`, `Added`.
    pub source: String,
    pub drive: Option<Drive>,
    pub model: Model,
    /// Why it cannot start.
    pub problem: Option<String>,
    /// A pack the launcher downloaded (`Car::added`).
    pub added: bool,
    /// What the search matches, lower case.
    haystack: String,
}

impl CarRow {
    pub fn new(index: usize, car: &Car) -> CarRow {
        let problem = car.unavailable();
        let (title, group, source, drive, model) = match &car.physics {
            catalog::Physics::Pack(_) => {
                let source = match car.rsf_id {
                    Some(id) if !car.added => format!("RSF {id}"),
                    _ => "Added".to_owned(),
                };
                let model = match &car.model {
                    Ok(m) if m.fallback.is_none() => Model::Own,
                    Ok(m) => Model::StandIn(m.fallback.clone().unwrap_or_default()),
                    Err(_) => Model::Missing,
                };
                let drive = car.drive_train.as_deref().and_then(Drive::parse);
                (car.name.clone(), car.group.clone(), source, drive, model)
            }
            catalog::Physics::Archive => match RETAIL.iter().find(|(folder, ..)| *folder == car.name) {
                Some((_, title, group, drive)) => {
                    (title.to_string(), Some(group.to_string()), "RBR 1.02".to_owned(), Some(*drive), Model::Own)
                }
                None => (car.name.clone(), None, "RBR 1.02".to_owned(), None, Model::Own),
            },
        };
        let (class, rank) = family(group.as_deref());
        let group = group.unwrap_or_else(|| OTHER.to_owned());
        let haystack = format!("{title} {} {group} {class} {source}", car.name).to_lowercase();
        CarRow {
            index,
            name: car.name.clone(),
            title,
            group,
            class,
            rank,
            source,
            drive,
            model,
            problem,
            added: car.added,
            haystack,
        }
    }

    pub fn startable(&self) -> bool {
        self.problem.is_none()
    }

    /// The group and the source, as the Drive page's field shows them.
    pub fn detail(&self) -> String {
        format!("{} · {}", self.group, self.source)
    }

    /// Why it cannot start, shortly.
    fn reason(&self) -> Option<&str> {
        self.problem.as_deref().map(|p| if p.starts_with("not downloaded") { "not downloaded" } else { p })
    }
}

/// What the player narrowed the list to.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Filter {
    /// Words that must all be in the car's name, group, class, source or RSF number; they search every car.
    pub query: String,
    pub drive: Option<Drive>,
    /// A class of the filter bar, by name.
    pub class: Option<String>,
}

impl Filter {
    fn accepts(&self, row: &CarRow) -> bool {
        self.drive.is_none_or(|d| row.drive == Some(d)) && self.class.as_ref().is_none_or(|c| row.class == *c)
    }
}

/// How a list is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Name,
    /// The ones driven most recently first.
    Recent,
}

impl Sort {
    const ALL: [Sort; 2] = [Sort::Name, Sort::Recent];

    fn label(self) -> &'static str {
        match self {
            Sort::Name => "Name A–Z",
            Sort::Recent => "Recently driven",
        }
    }
}

/// A set of cars the filter bar's scopes offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Collection {
    Favourites,
    Recent,
    All,
    CannotStart,
    /// The cars the launcher downloaded (the Cars page only).
    Added,
}

/// The list of `collection` for `filter` sorted by `sort`. `favourites` and `recent` are car names, in the order
/// starred and newest first. The library and the classes list the cars that can start; the favourites, the recent
/// picks and a search list the others last, and [`Collection::CannotStart`] lists only them. A search covers every
/// car whatever the collection: the favourites first, then each class.
pub fn lines(
    rows: &[CarRow],
    collection: &Collection,
    filter: &Filter,
    sort: Sort,
    favourites: &[String],
    recent: &[String],
) -> Vec<Line> {
    let words: Vec<String> = filter.query.to_lowercase().split_whitespace().map(str::to_owned).collect();
    let accepted = |k: &usize| filter.accepts(&rows[*k]);
    // A search covers every car whatever the class: only the drive narrows it.
    let searched = |k: &usize| Filter { class: None, ..filter.clone() }.accepts(&rows[*k]);
    let sorted = |mut members: Vec<usize>| {
        match sort {
            Sort::Name => members.sort_by_cached_key(|&k| (rows[k].title.to_lowercase(), rows[k].index)),
            Sort::Recent => members.sort_by_cached_key(|&k| {
                let at = recent.iter().position(|n| *n == rows[k].name);
                (at.unwrap_or(usize::MAX), rows[k].title.to_lowercase())
            }),
        }
        members
    };
    let split =
        |members: Vec<usize>| -> (Vec<usize>, Vec<usize>) { members.into_iter().partition(|&k| rows[k].startable()) };
    let named = |names: &[String]| -> Vec<usize> {
        names.iter().filter_map(|n| rows.iter().position(|r| r.name == *n)).filter(accepted).collect()
    };
    let mut out = Vec::new();
    if !words.is_empty() {
        let found = (0..rows.len()).filter(|&k| words.iter().all(|w| rows[k].haystack.contains(w.as_str())));
        let (ok, blocked) = split(found.filter(searched).collect());
        let (starred, rest): (Vec<usize>, Vec<usize>) =
            ok.into_iter().partition(|&k| favourites.contains(&rows[k].name));
        out.extend(block(Some("Favourites"), sorted(starred)));
        let mut classes: BTreeMap<(usize, &str), Vec<usize>> = BTreeMap::new();
        for k in rest {
            classes.entry((rows[k].rank, rows[k].class.as_str())).or_default().push(k);
        }
        for ((_, name), members) in classes {
            out.extend(block(Some(name), sorted(members)));
        }
        out.extend(block(Some("Cannot start"), sorted(blocked)));
        return out;
    }
    match collection {
        Collection::Favourites | Collection::Recent => {
            let starred = *collection == Collection::Favourites;
            let (ok, blocked) = split(named(if starred { favourites } else { recent }));
            // The recent picks keep their order of use whatever the sort says.
            let (ok, blocked) = if starred { (sorted(ok), sorted(blocked)) } else { (ok, blocked) };
            out.extend(block(None, ok));
            out.extend(block(Some("Cannot start"), blocked));
        }
        Collection::All => {
            let members = (0..rows.len()).filter(|&k| rows[k].startable()).filter(accepted);
            out.extend(block(None, sorted(members.collect())));
        }
        Collection::CannotStart => {
            let members = (0..rows.len()).filter(|&k| !rows[k].startable()).filter(accepted);
            out.extend(block(None, sorted(members.collect())));
        }
        Collection::Added => {
            let (ok, blocked) = split((0..rows.len()).filter(|&k| rows[k].added).filter(accepted).collect());
            out.extend(block(None, sorted(ok)));
            out.extend(block(Some("Cannot start"), sorted(blocked)));
        }
    }
    out
}

/// What the browser asks of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Choose the car at this position of the catalog and go back.
    Choose(usize),
    /// Star or unstar the car with this name.
    Favourite(String),
    /// Go back without choosing.
    Back,
    /// Open the Get more cars dialog (the Cars page).
    GetMore,
    /// Remove the car added by the launcher with this pack folder (the Cars page).
    Remove(String),
}

/// What the page gives the browser to draw.
pub struct Env<'a> {
    /// The Cars page's line above the search: how many cars.
    pub heading: String,
    pub cars: &'a [Car],
    /// Car names: starred in that order, and recently chosen newest first.
    pub favourites: &'a [String],
    pub recent: &'a [String],
    pub previews: &'a mut Previews,
    /// The skin chosen for each car, and the import of one.
    pub skins: &'a mut Skins,
    /// The game folder the cars' models are read from.
    pub root: &'a str,
}

/// What a list was made for.
type Made = (Collection, Filter, Sort, Vec<String>, Vec<String>);

/// Whose car the browser chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Target {
    /// The Drive page's.
    #[default]
    Drive,
    /// The car the championship drives (`championship_page`).
    Championship,
    /// The Circuit page's.
    Circuit,
}

#[derive(Default)]
pub struct CarBrowser {
    pub open: bool,
    /// The Cars page's browser: no Back, no Esc, the added cars' scope and the removal of an added car.
    page: bool,
    /// Whether cars can be removed now (not while adding or driving): the page sets it.
    pub idle: bool,
    /// The pack folder of the added car whose removal waits for its confirmation.
    confirm: Option<String>,
    pub target: Target,
    rows: Vec<CarRow>,
    collection: Option<Collection>,
    filter: Filter,
    sort: Sort,
    lines: Vec<Line>,
    made: Option<Made>,
    /// The highlighted car (its position in the catalog) and the list's scrolling.
    nav: Nav,
    /// Focus the search box on the next draw.
    focus: bool,
    visited: bool,
}

impl CarBrowser {
    /// An added car was removed: its confirmation is done.
    pub fn removed(&mut self) {
        self.confirm = None;
    }

    /// The Content > Cars page's browser.
    pub fn for_page() -> CarBrowser {
        CarBrowser { page: true, ..CarBrowser::default() }
    }

    /// Lists the cars of the catalog.
    pub fn set_rows(&mut self, rows: Vec<CarRow>) {
        let state = self.state();
        self.rows = rows;
        self.made = None;
        self.restore(&state);
    }

    pub fn row(&self, index: usize) -> Option<&CarRow> {
        self.rows.get(index)
    }

    pub fn state(&self) -> crate::browser_state::State {
        if !self.visited {
            return Default::default();
        }
        let collection = match self.collection.as_ref().unwrap_or(&Collection::All) {
            Collection::Favourites => "favourites".into(),
            Collection::Recent => "recent".into(),
            Collection::All => "all".into(),
            Collection::CannotStart => "unavailable".into(),
            Collection::Added => "added".into(),
        };
        crate::browser_state::State {
            collection: Some(collection),
            query: self.filter.query.clone(),
            sort: if self.sort == Sort::Recent { "recent" } else { "name" }.into(),
            filter: self.filter.drive.map(|d| d.label().to_owned()).unwrap_or_default(),
            length: String::new(),
            class: self.filter.class.clone().unwrap_or_default(),
            selected: self.rows.get(self.nav.cursor).map(|r| r.name.clone()),
            scroll: self.nav.offset(),
        }
    }

    pub fn restore(&mut self, state: &crate::browser_state::State) {
        let Some(collection) = state.collection.as_deref() else { return };
        // A class kept as the collection (an older save) is the class dropdown now.
        let old_class = collection.strip_prefix("class:").filter(|c| self.rows.iter().any(|r| r.class == *c));
        self.collection = Some(match collection {
            "favourites" => Collection::Favourites,
            "recent" => Collection::Recent,
            "unavailable" => Collection::CannotStart,
            "added" => Collection::Added,
            _ => Collection::All,
        });
        let class = Some(state.class.as_str()).filter(|c| !c.is_empty()).or(old_class);
        let class = class.filter(|c| self.rows.iter().any(|r| r.class == *c)).map(str::to_owned);
        self.filter = Filter { query: state.query.clone(), drive: Drive::parse(&state.filter), class };
        self.sort = if state.sort == "recent" { Sort::Recent } else { Sort::Name };
        let cursor = self.rows.iter().position(|r| Some(&r.name) == state.selected.as_ref()).unwrap_or(0);
        self.nav.restore(cursor, state.scroll);
        self.visited = true;
        self.made = None;
    }

    /// Highlights the car at this position of the catalog (`--screenshot`).
    pub fn show(&mut self, index: usize) {
        self.nav.show(index);
    }

    /// The highlighted car's position in the catalog.
    pub fn highlighted(&self) -> Option<usize> {
        self.rows.get(self.nav.cursor).map(|r| r.index)
    }

    /// The row of the car named `name`.
    pub fn row_of(&self, name: &str) -> Option<&CarRow> {
        self.rows.iter().find(|r| r.name == name)
    }

    /// Opens the browser for `target` with `selected` (a catalog position) highlighted: on the favourites when it is
    /// one, else on all cars. `collection` opens it on that instead.
    pub fn open_on(&mut self, target: Target, selected: usize, favourites: &[String], collection: Option<Collection>) {
        let favourite = self.rows.get(selected).is_some_and(|r| favourites.contains(&r.name));
        if !self.visited || collection.is_some() {
            self.collection =
                Some(collection.unwrap_or(if favourite { Collection::Favourites } else { Collection::All }));
            self.filter.query.clear();
            self.filter.class = None;
            self.nav.show(selected);
        } else {
            self.nav.resume();
        }
        self.visited = true;
        self.target = target;
        self.open = true;
        self.focus = true;
    }

    /// The list, made again when what it depends on changed.
    fn refresh(&mut self, favourites: &[String], recent: &[String]) {
        let collection = self.collection.clone().unwrap_or(Collection::All);
        let made = (collection, self.filter.clone(), self.sort, favourites.to_vec(), recent.to_vec());
        if self.made.as_ref() == Some(&made) {
            return;
        }
        let changed = self.made.as_ref().is_none_or(|m| m.0 != made.0 || m.1 != made.1 || m.2 != made.2);
        self.lines = lines(&self.rows, &made.0, &made.1, made.2, favourites, recent);
        self.made = Some(made);
        if changed {
            self.nav.settle(&self.lines);
        }
    }

    /// Draws the browser in `ui`; what the player asked for.
    pub fn ui(&mut self, ui: &mut egui::Ui, mut env: Env) -> Option<Action> {
        self.refresh(env.favourites, env.recent);
        let keys = step_nav(ui, &mut self.nav, &self.lines);
        let mut action = key_action(
            &self.nav,
            &self.rows,
            &keys,
            |row| row.startable().then_some(Action::Choose(row.index)),
            |row| Action::Favourite(row.name.clone()),
            Action::Back,
        );
        if self.page {
            // A page has no Back, and Esc does nothing.
            action = action.filter(|a| *a != Action::Back);
        }
        let subtitle = Some(env.heading.as_str()).filter(|h| self.page && !h.is_empty());
        let (title, get_more) =
            if self.page { ("Cars", Some(("Get more cars…", "gvrc.de's cars"))) } else { ("Choose a car", None) };
        let clicks = browser::header(ui, title, subtitle, get_more);
        if clicks.back {
            action = Some(Action::Back);
        } else if clicks.get_more {
            action = Some(Action::GetMore);
        }
        if clicks.random {
            self.random();
        }
        self.filter_bar(ui, env.favourites, env.recent);
        let chosen = self.body(ui, &mut env);
        chosen.or(action)
    }

    /// A random car of the list as filtered becomes the highlighted one.
    fn random(&mut self) {
        if let Some(k) = random_row(&self.lines, |k| self.rows[k].startable()) {
            self.nav.show(k);
        }
    }

    /// The search, the scopes and the class, drive and sort dropdowns.
    fn filter_bar(&mut self, ui: &mut egui::Ui, favourites: &[String], recent: &[String]) {
        let scopes = self.scopes(favourites, recent);
        let labels: Vec<&str> = scopes.iter().map(|(_, l)| *l).collect();
        let startable = |class: &str| self.rows.iter().filter(|r| r.startable() && r.class == class).count();
        let classes: Vec<(Option<String>, String)> = std::iter::once((None, "All classes".to_owned()))
            .chain(self.classes().into_iter().map(|c| (Some(c.clone()), format!("{c}  {}", startable(&c)))))
            .collect();
        let drives: Vec<(Option<Drive>, String)> = std::iter::once((None, "Any drive".to_owned()))
            .chain(Drive::ALL.map(|d| (Some(d), d.label().to_owned())))
            .collect();
        let sorts: Vec<(Sort, String)> = Sort::ALL.map(|s| (s, s.label().to_owned())).to_vec();
        const DROPS: [f32; 3] = [112.0, 84.0, 118.0];
        let drops_width = DROPS.iter().sum::<f32>() + 3.0 * 8.0;
        let seg = browser::segmented_width(ui, &labels);
        let before = (self.filter.clone(), self.sort, self.collection.clone());
        browser::bar(ui, |ui| {
            let room = ui.available_width() - seg - 16.0;
            let one_row = room - drops_width >= 180.0;
            let search = (if one_row { room - drops_width - 8.0 } else { room }).clamp(180.0, 300.0);
            let hint = "Search name, class, RSF no.";
            ui.horizontal(|ui| {
                browser::search_box(ui, hint, &mut self.filter.query, &mut self.focus, search);
                let mut scope = self.collection.clone().unwrap_or(Collection::All);
                let counts = self.scope_counts(favourites, recent);
                let segments = kit::segmented(ui, "cars scope", &mut scope, &scopes_for(&scopes));
                if segments.on_hover_text(counts).changed() {
                    self.collection = Some(scope);
                    self.filter.query.clear();
                }
                if one_row {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.dropdowns(ui, &classes, &drives, &sorts, DROPS);
                    });
                }
            });
            if !one_row {
                ui.add_space(8.0);
                ui.horizontal(|ui| self.dropdowns(ui, &classes, &drives, &sorts, DROPS));
            }
        });
        if before != (self.filter.clone(), self.sort, self.collection.clone()) {
            // Back to the search box, where the arrows move through the list.
            self.focus = true;
        }
    }

    /// The class, drive and sort dropdowns, right to left when in a right-to-left layout (the last is the sort).
    fn dropdowns(
        &mut self,
        ui: &mut egui::Ui,
        classes: &[(Option<String>, String)],
        drives: &[(Option<Drive>, String)],
        sorts: &[(Sort, String)],
        widths: [f32; 3],
    ) {
        let rtl = ui.layout().main_dir() == egui::Direction::RightToLeft;
        let class_text = self.filter.class.clone().unwrap_or_else(|| "All classes".to_owned());
        let drive_text = self.filter.drive.map_or("Any drive", Drive::label).to_owned();
        let sort_text = format!("Sort: {}", self.sort.label());
        let class = |s: &mut Self, ui: &mut egui::Ui| {
            browser::dropdown(ui, "cars class", widths[0], &class_text, &mut s.filter.class, classes)
        };
        let drive = |s: &mut Self, ui: &mut egui::Ui| {
            browser::dropdown(ui, "cars drive", widths[1], &drive_text, &mut s.filter.drive, drives)
        };
        let sort = |s: &mut Self, ui: &mut egui::Ui| {
            browser::dropdown(ui, "cars sort", widths[2], &sort_text, &mut s.sort, sorts)
        };
        if rtl {
            sort(self, ui);
            drive(self, ui);
            class(self, ui);
        } else {
            class(self, ui);
            drive(self, ui);
            sort(self, ui);
        }
    }

    /// The scopes of the filter bar: all cars, the favourites, the recent picks, the cars added by you (the page's, when
    /// there are some) and the cars that cannot start (when there are some).
    fn scopes(&self, favourites: &[String], recent: &[String]) -> Vec<(Collection, &'static str)> {
        let _ = (favourites, recent);
        let mut out =
            vec![(Collection::All, "All"), (Collection::Favourites, "Favourites"), (Collection::Recent, "Recent")];
        if self.page && self.rows.iter().any(|r| r.added) {
            out.push((Collection::Added, "Added by you"));
        }
        if self.rows.iter().any(|r| !r.startable()) {
            out.push((Collection::CannotStart, "Cannot start"));
        }
        out
    }

    /// The scopes' counts for their hover text: the favourites and recent picks that are cars, the cars that can
    /// start, the cars added and the ones that cannot start.
    fn scope_counts(&self, favourites: &[String], recent: &[String]) -> String {
        let known = |names: &[String]| names.iter().filter(|n| self.rows.iter().any(|r| r.name == **n)).count();
        let startable = self.rows.iter().filter(|r| r.startable()).count();
        let mut text = format!("All {startable} · Favourites {} · Recent {}", known(favourites), known(recent));
        let added = self.rows.iter().filter(|r| r.added).count();
        if self.page && added > 0 {
            text += &format!(" · Added by you {added}");
        }
        if startable < self.rows.len() {
            text += &format!(" · Cannot start {}", self.rows.len() - startable);
        }
        text
    }

    /// The classes of the cars, newest to oldest.
    fn classes(&self) -> Vec<String> {
        let mut classes: BTreeMap<(usize, &str), ()> = BTreeMap::new();
        for row in &self.rows {
            classes.insert((row.rank, row.class.as_str()), ());
        }
        classes.into_keys().map(|(_, name)| name.to_owned()).collect()
    }

    /// The list and the inspector below the filter bar.
    fn body(&mut self, ui: &mut egui::Ui, env: &mut Env) -> Option<Action> {
        let mut action = None;
        let selected = self.rows.get(self.nav.cursor).and_then(|row| Some((row.clone(), env.cars.get(row.index)?)));
        let page = self.page;
        let (idle, confirm) = (self.idle, &mut self.confirm);
        let favourite = selected.as_ref().is_some_and(|(row, _)| env.favourites.contains(&row.name));
        let (previews, skins, root) = (&mut *env.previews, &mut *env.skins, env.root);
        browser::inspector(
            ui,
            |ui| {
                if let Some((row, car)) = &selected {
                    facts(ui, row, car, previews, skins, root);
                }
            },
            |ui| {
                if let Some((row, car)) = &selected {
                    action = actions(ui, row, car, page, favourite, idle, confirm);
                }
            },
        );
        let chosen = self.list(ui, env.favourites, env.recent);
        chosen.or(action)
    }

    /// The list with its header row and its hint bar; a car chosen or starred with the mouse.
    fn list(&mut self, ui: &mut egui::Ui, favourites: &[String], recent: &[String]) -> Option<Action> {
        let mut action = None;
        let collection = self.collection.clone().unwrap_or(Collection::All);
        let count = items(&self.lines).count();
        let searching = !self.filter.query.trim().is_empty();
        let shown = if searching { format!("{count} found in all cars") } else { format!("{count} shown") };
        // Inside a class every car has the class: the column tells where the car is from instead.
        let by_source = self.filter.class.is_some() && !searching;
        let verb = if self.page { "Drive" } else { "Choose" };
        let mut keys = vec![("↑↓", "Move"), ("↵", verb), ("Ctrl D", "Favourite")];
        if !self.page {
            keys.push(("Esc", "Back"));
        }
        browser::hint_bar(ui, &shown, &keys, |ui| {
            browser::list_header(
                ui,
                "Name",
                &WIDTHS,
                &[(if by_source { "From" } else { "Class" }, false), ("Model", false), ("Drive", false)],
            );
            if count == 0 {
                let empty = self.filter.query.trim().is_empty();
                let hint = match &collection {
                    Collection::Favourites if empty && favourites.is_empty() => {
                        "Click ☆ next to a car to keep it here."
                    }
                    Collection::Recent if empty && recent.is_empty() => "The cars you choose are listed here.",
                    _ => "Nothing matches.",
                };
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label(RichText::new(hint).color(TEXT_2));
                });
            }
            list_rows(ui, &mut self.nav, "cars", &self.lines, |ui, nav, range| {
                for at in range {
                    let k = match &self.lines[at] {
                        Line::Header(name) => {
                            header_line(ui, name);
                            continue;
                        }
                        Line::Row(k) => *k,
                    };
                    let row = &self.rows[k];
                    let (rect, response, star) = row_slot(ui, k);
                    let favourite = favourites.contains(&row.name);
                    draw_row(ui, rect, row, by_source, k == nav.cursor, response.hovered(), favourite);
                    let response = match row.problem.as_deref() {
                        Some(problem) => response.on_hover_text(format!("cannot start: {problem}")),
                        None => response,
                    };
                    if star.clicked() {
                        action = Some(Action::Favourite(row.name.clone()));
                    } else if response.double_clicked() && row.startable() {
                        action = Some(Action::Choose(row.index));
                    } else if response.clicked() {
                        nav.point(k);
                    }
                }
            });
        });
        action
    }
}

/// The scopes as the segmented control takes them.
fn scopes_for<'a>(scopes: &'a [(Collection, &'static str)]) -> Vec<(Collection, &'a str)> {
    scopes.iter().map(|(c, l)| (c.clone(), *l)).collect()
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

/// `text` shortened from the left with an ellipsis to fit `width`.
fn left_truncated(ui: &egui::Ui, text: &str, font: &FontId, width: f32) -> String {
    let fits = |t: &str| ui.painter().layout_no_wrap(t.to_owned(), font.clone(), TEXT).size().x <= width;
    if fits(text) {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let keep =
        (0..chars.len()).rev().find(|&n| fits(&format!("…{}", chars[chars.len() - n..].iter().collect::<String>())));
    format!("…{}", chars[chars.len() - keep.unwrap_or(0)..].iter().collect::<String>())
}

/// The inspector's scrolling part: the car turning, its name and class, and its facts in a panel.
fn facts(ui: &mut egui::Ui, row: &CarRow, car: &Car, previews: &mut Previews, skins: &mut Skins, root: &str) {
    previews.car_ui(ui, car, skins.choice(car), root);
    browser::title(ui, &row.title, &row.group);
    kit::panel(ui, |rows| {
        skins.rows(rows, car);
        rows.row("Drive", |ui| _ = ui.label(row.drive.map_or("unknown", Drive::label)));
        rows.row("Source", |ui| _ = ui.label(source(car)));
        rows.row("Model", |ui| {
            match &row.model {
                Model::Own => ui.label("Own model"),
                Model::StandIn(why) => ui.label("Stand-in model").on_hover_text(why),
                Model::Missing => ui.label("None"),
            };
        });
        rows.row("Folder", |ui| match files_folder(car, root) {
            Some(path) => {
                if kit::link(ui, "Open").on_hover_text("Open the folder").clicked() {
                    crate::crash::open(path.as_os_str());
                }
                let font = theme::mono(11.0);
                let text = left_truncated(ui, &path.display().to_string(), &font, ui.available_width() - 8.0);
                ui.label(RichText::new(text).font(font).color(TEXT_2)).on_hover_text(path.display().to_string());
            }
            None => _ = ui.label("not found"),
        });
    });
}

/// The inspector's pinned part: why the car cannot start, Favourite and the primary button, and the removal of an
/// added car (the page's) with its confirmation.
fn actions(
    ui: &mut egui::Ui,
    row: &CarRow,
    car: &Car,
    page: bool,
    favourite: bool,
    idle: bool,
    confirm: &mut Option<String>,
) -> Option<Action> {
    let mut action = None;
    if let Some(reason) = row.reason() {
        kit::status_dot(ui, theme::WARN, &format!("Cannot start: {reason}"));
        ui.add_space(8.0);
    }
    ui.horizontal(|ui| {
        if favourite_button(ui, favourite).clicked() {
            action = Some(Action::Favourite(row.name.clone()));
        }
        let (label, tip) =
            if page { ("Drive this car", "Enter") } else { ("Choose car", "Enter, or a double click on the car") };
        if drive_button(ui, label, row.startable(), tip).clicked() {
            action = Some(Action::Choose(row.index));
        }
    });
    let Some(pack) = added_pack(car).filter(|_| page) else { return action };
    ui.add_space(10.0);
    if confirm.as_deref() == Some(pack.as_str()) {
        ui.label(
            RichText::new("Remove this car and its model from your files? Nothing in the game folder is touched.")
                .color(TEXT_2),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let remove = egui::Button::new(RichText::new("Remove").color(theme::ERROR));
            if ui.add_enabled(idle, remove).clicked() {
                action = Some(Action::Remove(pack));
            }
            if ui.button("Keep it").clicked() {
                *confirm = None;
            }
        });
    } else if ui.add_enabled(idle, egui::Button::new("Remove…")).on_hover_text("Added by you").clicked() {
        *confirm = Some(pack);
    }
    action
}

/// The right-aligned columns of the list, widths: class, model, drive.
const WIDTHS: [f32; 3] = [110.0, 92.0, 36.0];

/// A car's row: its star, name, class, model and drive; the highlighted one filled.
fn draw_row(ui: &egui::Ui, rect: Rect, row: &CarRow, by_source: bool, cursor: bool, hovered: bool, favourite: bool) {
    let p = ui.painter();
    let (c, name_right) = browser::columns(rect, &WIDTHS);
    let startable = row.startable();
    paint_selection(ui, rect, cursor, hovered);
    paint_star(p, Pos2::new(rect.left() + 14.0, rect.center().y), favourite, hovered || cursor);
    let (name_colour, dim) = if startable { (TEXT_LIST, TEXT_2) } else { (TEXT_DISABLED, TEXT_3) };
    let font = FontId::proportional(13.0);
    paint_cell(p, rect, browser::name_x(rect), name_right, &row.title, font.clone(), name_colour);
    let third = if by_source { &row.source } else { &row.group };
    paint_cell(p, rect, c[0], c[1] - 8.0, third, font.clone(), dim);
    match (row.reason(), &row.model) {
        (Some(reason), _) => paint_cell(p, rect, c[1], c[2] - 8.0, reason, FontId::proportional(12.0), theme::WARN),
        (None, model) => {
            let (dot, word) = match model {
                Model::Own => (theme::GOOD, "Own"),
                Model::StandIn(_) => (theme::WARN, "Stand-in"),
                Model::Missing => (TEXT_3, "None"),
            };
            p.circle_filled(Pos2::new(c[1] + 3.0, rect.center().y), 3.0, dot);
            paint_cell(p, rect, c[1] + 14.0, c[2] - 8.0, word, font, dim);
        }
    }
    let drive = row.drive.map_or("—", Drive::label);
    paint_cell(p, rect, c[2], c[2] + WIDTHS[2], drive, theme::mono(11.0), dim);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(index: usize, name: &str, group: &str, problem: Option<&str>) -> CarRow {
        let (class, rank) = family(Some(group));
        CarRow {
            index,
            name: name.into(),
            title: name.into(),
            group: group.into(),
            class,
            rank,
            source: format!("RSF {index}"),
            drive: Some(Drive::Awd),
            model: Model::Own,
            problem: problem.map(str::to_owned),
            added: false,
            haystack: format!("{name} {group}").to_lowercase(),
        }
    }

    fn rows() -> Vec<CarRow> {
        vec![
            row(0, "Zeta", "Group A7", None),
            row(1, "Alpha", "Group A8", Some("not downloaded in RallySimFans")),
            row(2, "Beta", "WRC 2.0", None),
            row(3, "Gamma", "Group A6", None),
        ]
    }

    #[test]
    fn reopening_and_restoring_keep_the_car_picker_state() {
        let mut browser = CarBrowser::default();
        browser.set_rows(rows());
        browser.open_on(Target::Drive, 0, &[], None);
        browser.collection = Some(Collection::Favourites);
        browser.filter = Filter { query: "gamma".into(), drive: Some(Drive::Awd), class: Some("Group A".into()) };
        browser.sort = Sort::Recent;
        browser.nav.restore(3, 90.0);
        browser.refresh(&[], &[]);
        let state = browser.state();
        browser.open = false;
        browser.open_on(Target::Championship, 0, &["Zeta".into()], None);
        browser.refresh(&[], &[]);
        assert_eq!(browser.state(), state);
        assert_eq!(browser.target, Target::Championship);
        let mut restored = CarBrowser::default();
        let mut rows = rows();
        rows.swap(0, 3);
        restored.set_rows(rows);
        restored.restore(&state);
        restored.open_on(Target::Drive, 0, &[], None);
        restored.refresh(&[], &[]);
        assert_eq!(restored.state(), state, "highlight follows the car name across catalogue changes");
        restored.open_on(Target::Drive, 0, &[], Some(Collection::Favourites));
        assert_eq!(restored.collection, Some(Collection::Favourites));
        assert!(restored.filter.query.is_empty());
        assert_eq!(restored.sort, Sort::Recent);
    }

    fn names(rows: &[CarRow], lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| match l {
                Line::Header(h) => format!("#{h}"),
                Line::Row(k) => rows[*k].name.clone(),
            })
            .collect()
    }

    #[test]
    fn the_a_groups_share_a_class_of_the_cars_that_can_start() {
        let rows = rows();
        let class = Filter { class: Some("Group A".into()), ..Filter::default() };
        let found = lines(&rows, &Collection::All, &class, Sort::Name, &[], &[]);
        assert_eq!(names(&rows, &found), ["Gamma", "Zeta"]);
        let blocked = lines(&rows, &Collection::CannotStart, &Filter::default(), Sort::Name, &[], &[]);
        assert_eq!(names(&rows, &blocked), ["Alpha"]);
        let rwd = Filter { drive: Some(Drive::Rwd), ..Filter::default() };
        assert!(lines(&rows, &Collection::CannotStart, &rwd, Sort::Name, &[], &[]).is_empty());
    }

    #[test]
    fn a_search_covers_every_car_by_class() {
        let rows = rows();
        let filter = Filter { query: "group a".into(), ..Filter::default() };
        let found = lines(&rows, &Collection::Favourites, &filter, Sort::Name, &["Zeta".into()], &[]);
        assert_eq!(names(&rows, &found), ["#Favourites", "Zeta", "#Group A", "Gamma", "#Cannot start", "Alpha"]);
    }

    #[test]
    fn the_filter_bar_orders_classes_newest_first_and_offers_what_exists() {
        let mut browser = CarBrowser::for_page();
        browser.set_rows(rows());
        assert_eq!(browser.classes(), ["WRC 2.0", "Group A"]);
        let scopes: Vec<&str> = browser.scopes(&[], &[]).iter().map(|(_, l)| *l).collect();
        assert_eq!(scopes, ["All", "Favourites", "Recent", "Cannot start"]);
        browser.rows[0].added = true;
        assert!(browser.scopes(&[], &[]).iter().any(|(c, _)| *c == Collection::Added));
    }

    #[test]
    fn drive_words_of_rsf_parse() {
        assert_eq!(Drive::parse("AWD with torque split"), Some(Drive::Awd));
        assert_eq!(Drive::parse("fwd"), Some(Drive::Fwd));
        assert_eq!(Drive::parse("?"), None);
    }
}
