//! The stage browser: the Stages page, or the picker the Drive, Circuit and Championship pages open. A page header, a
//! filter bar (search, the scopes all / favourites / recent / added / circuits / cannot start, surface, country, length
//! and sort), the list with its header row and key hints, and the highlighted stage's inspector on the right (its
//! picture, facts and buttons).
//!
//! [`lines`] makes the list from the rows and what the player filtered, apart from any drawing. The list draws only its
//! visible rows, so a full RallySimFans install (about 450 stages) opens and filters at once. A click or the arrow keys
//! and Page Up/Down move the highlight, Enter or a double click chooses, Ctrl/Cmd+D
//! favourites, Escape goes back without choosing. A stage that cannot start is listed greyed with its reason, under
//! "Cannot start", and last in the favourites, the recent picks and a search.

use std::collections::BTreeMap;

use egui::{Color32, FontId, Pos2, Rect, RichText};

use crate::browser::{
    self, block, drive_button, favourite_button, header_line, items, key_action, list_rows, paint_cell,
    paint_selection, paint_star, random_row, row_slot, step_nav, Line, Nav,
};
use crate::catalog::{Map, Origin};
use crate::kit;
use crate::preview::{self, Previews};
use crate::theme::{self, TEXT, TEXT_2, TEXT_3, TEXT_DISABLED, TEXT_LIST};

pub const SURFACES: [&str; 3] = ["Gravel", "Tarmac", "Snow"];

const SURFACE_DOTS: [Color32; 3] =
    [Color32::from_rgb(190, 145, 95), Color32::from_rgb(150, 155, 170), Color32::from_rgb(185, 215, 250)];

/// The ISO country codes RallySimFans gives its stages, and the names the browser shows.
const COUNTRIES: [(&str, &str); 39] = [
    ("AR", "Argentina"),
    ("AT", "Austria"),
    ("AU", "Australia"),
    ("BE", "Belgium"),
    ("BR", "Brazil"),
    ("CA", "Canada"),
    ("CH", "Switzerland"),
    ("CN", "China"),
    ("CY", "Cyprus"),
    ("CZ", "Czechia"),
    ("DE", "Germany"),
    ("EE", "Estonia"),
    ("ES", "Spain"),
    ("FI", "Finland"),
    ("FR", "France"),
    ("GB", "Great Britain"),
    ("GR", "Greece"),
    ("HU", "Hungary"),
    ("IE", "Ireland"),
    ("IT", "Italy"),
    ("JP", "Japan"),
    ("KE", "Kenya"),
    ("LB", "Lebanon"),
    ("LT", "Lithuania"),
    ("LV", "Latvia"),
    ("MC", "Monaco"),
    ("MG", "Madagascar"),
    ("MX", "Mexico"),
    ("NL", "Netherlands"),
    ("NZ", "New Zealand"),
    ("PL", "Poland"),
    ("PT", "Portugal"),
    ("RO", "Romania"),
    ("SE", "Sweden"),
    ("SI", "Slovenia"),
    ("SK", "Slovakia"),
    ("SM", "San Marino"),
    ("UA", "Ukraine"),
    ("US", "USA"),
];

/// A country's name by its code; the code itself when the table has none.
pub fn country_name(code: &str) -> &str {
    COUNTRIES.iter().find(|(c, _)| c.eq_ignore_ascii_case(code)).map_or(code, |(_, name)| name)
}

/// The country of a retail map, by its base map (the game's menus group them by rally).
fn retail_country(map: u32) -> Option<&'static str> {
    Some(match rbr_render::stage_picture::base_map(map) {
        10..=15 => "FI",
        20..=25 => "GB",
        31..=36 => "AU",
        41..=46 => "FR",
        51..=56 => "JP",
        61..=66 => "US",
        _ => return None,
    })
}

/// One stage of the catalog as the browser lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct StageRow {
    /// Its position in the catalog's maps.
    pub index: usize,
    pub number: u32,
    pub name: String,
    /// The country's code, upper case.
    pub country: Option<String>,
    pub author: Option<String>,
    /// A 1.02 stage, not a RallySimFans one.
    pub retail: bool,
    /// A BTB stage (`crate::btb`): listed as experimental.
    pub btb: bool,
    pub km: Option<f32>,
    /// 0 gravel, 1 tarmac, 2 snow.
    pub surface: u32,
    /// Why it cannot start.
    pub problem: Option<String>,
    /// A BTB stage that looks like a circuit and is not marked as one (`rbr_btb::circuit`): the circuit browser's.
    pub candidate: bool,
    /// The stage's folder, when it has one of its own (not a retail stage).
    pub folder: Option<std::path::PathBuf>,
    /// Added by the player to `<files>/tracks/` and a circuit by its files: set by the Stages page, which has
    /// collections for them.
    pub added: bool,
    pub circuit: bool,
    /// What the search matches, lower case.
    haystack: String,
}

impl StageRow {
    /// The row of map `index`, `problem` its reason not to start ([`catalog::Catalog::availability`]).
    pub fn new(index: usize, map: &Map, problem: Option<String>) -> StageRow {
        let (country, author, km) = match &map.origin {
            Origin::Community(stage) => {
                (stage.country.as_ref().map(|c| c.to_uppercase()), stage.author.clone(), stage.length)
            }
            Origin::Btb(btb) => (btb.country.clone(), btb.author.clone(), btb.km),
            Origin::Retail => (
                retail_country(map.number).map(str::to_owned),
                Some("Warthog Games".to_owned()),
                preview::game_length(map.number).map(|m| m / 1000.0),
            ),
        };
        let name = map.name();
        let folder = match &map.origin {
            Origin::Community(stage) => Some(stage.folder.clone()),
            Origin::Btb(btb) => Some(btb.folder.clone()),
            Origin::Retail => None,
        };
        let origin = match &map.origin {
            Origin::Community(_) => "rsf rallysimfans",
            Origin::Btb(_) => "rsf btb experimental",
            Origin::Retail => "retail 1.02",
        };
        let place = country.as_deref().map(|c| format!("{c} {}", country_name(c))).unwrap_or_default();
        let haystack =
            format!("{name} {} {place} {} {origin}", map.number, author.as_deref().unwrap_or("")).to_lowercase();
        StageRow {
            index,
            number: map.number,
            name,
            country,
            author,
            retail: matches!(map.origin, Origin::Retail),
            btb: matches!(map.origin, Origin::Btb(_)),
            km,
            surface: map.surface().min(2),
            problem: problem.map(plain_problem),
            candidate: false,
            folder,
            added: false,
            circuit: false,
            haystack,
        }
    }

    /// The country's name, or "No country".
    pub fn country_name(&self) -> &str {
        self.country.as_deref().map_or("No country", country_name)
    }

    /// The number, length and surface, as the Drive page's field shows them.
    pub fn detail(&self) -> String {
        let mut parts = vec![self.number.to_string()];
        parts.extend(self.km.map(|km| format!("{km:.1} km")));
        parts.push(SURFACES[self.surface as usize].to_owned());
        if self.candidate {
            parts.push("looks like a circuit".to_owned());
        }
        parts.join(" · ")
    }

    pub fn startable(&self) -> bool {
        self.problem.is_none()
    }
}

/// A stage's reason not to start in the player's words (`--list` prints it as it is).
fn plain_problem(problem: String) -> String {
    if problem.contains("textures are in") && problem.contains("encrypted by RallySimFans") {
        "its textures are in a stage RallySimFans encrypts".into()
    } else if problem.contains("encrypted by RallySimFans") {
        "encrypted by RallySimFans: ReRBR can't read it".into()
    } else if problem.contains("weather row") {
        "the game has no weather for it".into()
    } else {
        problem
    }
}

/// The stage length filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Length {
    #[default]
    Any,
    Short,
    Medium,
    Long,
    Longest,
}

impl Length {
    /// The word for the filter bar's dropdown.
    pub fn long_label(self) -> &'static str {
        match self {
            Length::Any => "Any length",
            other => other.label(),
        }
    }

    pub const ALL: [Length; 5] = [Length::Any, Length::Short, Length::Medium, Length::Long, Length::Longest];

    pub fn label(self) -> &'static str {
        match self {
            Length::Any => "Any",
            Length::Short => "< 5 km",
            Length::Medium => "5-10 km",
            Length::Long => "10-15 km",
            Length::Longest => "> 15 km",
        }
    }

    /// Whether a stage of `km` passes; one of unknown length passes only `Any`.
    pub fn contains(self, km: Option<f32>) -> bool {
        let range = match self {
            Length::Any => return true,
            Length::Short => 0.0..5.0,
            Length::Medium => 5.0..10.0,
            Length::Long => 10.0..15.0,
            Length::Longest => 15.0..f32::INFINITY,
        };
        km.is_some_and(|km| range.contains(&km))
    }
}

/// What the player narrowed the list to.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Filter {
    /// Words that must all be in the stage's name, number, country, author or origin; they search every stage.
    pub query: String,
    pub surface: Option<u32>,
    pub length: Length,
    /// A country by its code (`Some(None)`: the stages without one).
    pub country: Option<Option<String>>,
}

impl Filter {
    fn accepts(&self, row: &StageRow) -> bool {
        self.surface.is_none_or(|s| row.surface == s)
            && self.length.contains(row.km)
            && self.country.as_ref().is_none_or(|c| row.country == *c)
    }
}

/// How a list is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Name,
    /// The shortest first, those of unknown length last.
    Length,
    /// The ones driven most recently first.
    Recent,
    /// The highest map number first: RSF numbers its maps as it adds them.
    Newest,
}

impl Sort {
    fn word(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Length => "length",
            Self::Recent => "recent",
            Self::Newest => "newest",
        }
    }
    fn from_word(word: &str) -> Self {
        match word {
            "length" => Self::Length,
            "recent" => Self::Recent,
            "newest" => Self::Newest,
            _ => Self::Name,
        }
    }

    const ALL: [Sort; 4] = [Sort::Name, Sort::Length, Sort::Recent, Sort::Newest];

    fn label(self) -> &'static str {
        match self {
            Sort::Name => "Name A–Z",
            Sort::Length => "Length",
            Sort::Recent => "Recently driven",
            Sort::Newest => "Newest",
        }
    }
}

/// A set of stages the filter bar's scopes offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Collection {
    Favourites,
    Recent,
    All,
    /// A country, by its code (`None`: the stages without one).
    Country(Option<String>),
    CannotStart,
    /// The stages the player added to `<files>/tracks/` (the Stages page's).
    Added,
    /// The circuits by their files (the Stages page's).
    Circuits,
}

/// The list of `collection` for `filter` sorted by `sort`. `favourites` and `recent` are map numbers, in the order
/// starred and newest first. A search covers every stage whatever the collection: the favourites first, then each
/// country, then the ones that cannot start.
pub fn lines(
    rows: &[StageRow],
    collection: &Collection,
    filter: &Filter,
    sort: Sort,
    favourites: &[u32],
    recent: &[u32],
) -> Vec<Line> {
    let words: Vec<String> = filter.query.to_lowercase().split_whitespace().map(str::to_owned).collect();
    let accepted = |k: &usize| filter.accepts(&rows[*k]);
    // A search covers every stage whatever the country: only the surface and the length narrow it.
    let searched = |k: &usize| Filter { country: None, ..filter.clone() }.accepts(&rows[*k]);
    let sorted = |mut members: Vec<usize>| {
        match sort {
            Sort::Name => members.sort_by_cached_key(|&k| (rows[k].name.to_lowercase(), rows[k].number)),
            Sort::Length => members.sort_by(|&a, &b| {
                let km = |k: usize| rows[k].km.unwrap_or(f32::INFINITY);
                km(a).total_cmp(&km(b)).then_with(|| rows[a].name.cmp(&rows[b].name))
            }),
            Sort::Newest => members.sort_by_key(|&k| std::cmp::Reverse(rows[k].number)),
            Sort::Recent => members.sort_by_cached_key(|&k| {
                (recent.iter().position(|&n| n == rows[k].number).unwrap_or(usize::MAX), rows[k].name.to_lowercase())
            }),
        }
        members
    };
    let split =
        |members: Vec<usize>| -> (Vec<usize>, Vec<usize>) { members.into_iter().partition(|&k| rows[k].startable()) };
    let mut out = Vec::new();
    if !words.is_empty() {
        let found = (0..rows.len()).filter(|&k| words.iter().all(|w| rows[k].haystack.contains(w.as_str())));
        let (ok, blocked) = split(found.filter(searched).collect());
        if sort == Sort::Newest {
            out.extend(block(None, sorted(ok)));
            out.extend(block(Some("Cannot start"), sorted(blocked)));
            return out;
        }
        let (starred, rest): (Vec<usize>, Vec<usize>) =
            ok.into_iter().partition(|&k| favourites.contains(&rows[k].number));
        out.extend(block(Some("Favourites"), sorted(starred)));
        let mut countries: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for k in rest {
            countries.entry(rows[k].country_name()).or_default().push(k);
        }
        for (name, members) in countries {
            out.extend(block(Some(name), sorted(members)));
        }
        out.extend(block(Some("Cannot start"), sorted(blocked)));
        return out;
    }
    match collection {
        Collection::Favourites | Collection::Recent => {
            let starred = *collection == Collection::Favourites;
            let numbers = if starred { favourites } else { recent };
            let found = numbers.iter().filter_map(|n| rows.iter().position(|r| r.number == *n)).filter(accepted);
            let (ok, blocked) = split(found.collect());
            // The recent picks keep their order of use whatever the sort says.
            let (ok, blocked) = if starred { (sorted(ok), sorted(blocked)) } else { (ok, blocked) };
            out.extend(block(None, ok));
            out.extend(block(Some("Cannot start"), blocked));
        }
        Collection::Added | Collection::Circuits => {
            let wanted = |r: &StageRow| if *collection == Collection::Added { r.added } else { r.circuit };
            let (ok, blocked) = split((0..rows.len()).filter(|&k| wanted(&rows[k])).filter(accepted).collect());
            out.extend(block(None, sorted(ok)));
            out.extend(block(Some("Cannot start"), sorted(blocked)));
        }
        Collection::All | Collection::Country(_) => {
            let wanted = |r: &StageRow| match collection {
                Collection::Country(code) => r.country == *code,
                _ => true,
            };
            let members = (0..rows.len()).filter(|&k| rows[k].startable() && wanted(&rows[k])).filter(accepted);
            out.extend(block(None, sorted(members.collect())));
        }
        Collection::CannotStart => {
            let members = (0..rows.len()).filter(|&k| !rows[k].startable()).filter(accepted);
            out.extend(block(None, sorted(members.collect())));
        }
    }
    out
}

/// What the browser asks of the Drive page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Choose the map at this position of the catalog and go back.
    Choose(usize),
    /// Star or unstar the map with this number.
    Favourite(u32),
    /// Go back without choosing.
    Back,
}

/// What the page gives the browser to draw.
pub struct Env<'a> {
    pub maps: &'a [Map],
    /// Map numbers: starred in that order, and recently chosen newest first.
    pub favourites: &'a [u32],
    pub recent: &'a [u32],
    pub previews: &'a mut Previews,
    /// The detail panel's rows under the picture for a stage (its weathers, whether it was driven).
    pub facts: &'a dyn Fn(&StageRow) -> Vec<(&'static str, String)>,
}

/// What a list was made for.
type Made = (Collection, Filter, Sort, Vec<u32>, Vec<u32>);

pub struct StageBrowser {
    pub open: bool,
    /// Whose stage it chooses: the Drive page's, or one of the championship's itinerary.
    pub target: crate::car_browser::Target,
    rows: Vec<StageRow>,
    collection: Collection,
    filter: Filter,
    sort: Sort,
    lines: Vec<Line>,
    made: Option<Made>,
    /// The highlighted stage (its position in the catalog) and the list's scrolling.
    nav: Nav,
    /// Focus the search box on the next draw.
    focus: bool,
    visited: bool,
    /// The Stages page's browser: a page, not a picker (no Back, no Random), with the stage's facts and its removal.
    page: bool,
    /// The added stage (its map number) whose removal is being confirmed.
    confirm: Option<u32>,
    /// The added stage (its position in the catalog) the player confirmed removing: the page takes it.
    pub removal: Option<usize>,
    /// The Stages page's subtitle: how many stages, how many added.
    pub heading: String,
    /// "Get more stages…" was pressed: the page takes it.
    pub get_more: bool,
}

impl Default for StageBrowser {
    fn default() -> Self {
        StageBrowser {
            open: false,
            target: crate::car_browser::Target::Drive,
            rows: Vec::new(),
            collection: Collection::All,
            filter: Filter::default(),
            sort: Sort::Name,
            lines: Vec::new(),
            made: None,
            nav: Nav::default(),
            focus: false,
            visited: false,
            page: false,
            confirm: None,
            removal: None,
            heading: String::new(),
            get_more: false,
        }
    }
}

impl StageBrowser {
    /// Lists the stages: each map of the catalog with its reason not to start.
    pub fn set_rows(&mut self, rows: Vec<StageRow>) {
        let state = self.state();
        self.rows = rows;
        self.made = None;
        self.restore(&state);
    }

    pub fn row(&self, index: usize) -> Option<&StageRow> {
        self.rows.get(index)
    }

    /// The position of the stage with `number` in the list (the catalog's position, in the Drive page's browser).
    pub fn position(&self, number: u32) -> Option<usize> {
        self.rows.iter().position(|r| r.number == number)
    }

    /// What the browser is for in the title and the button: "stage" or "circuit".
    fn noun(&self) -> &'static str {
        match self.target {
            crate::car_browser::Target::Circuit => "circuit",
            _ => "stage",
        }
    }

    pub fn state(&self) -> crate::browser_state::State {
        if !self.visited {
            return Default::default();
        }
        use crate::browser_state::State;
        let collection = match &self.collection {
            Collection::Favourites => "favourites".into(),
            Collection::Recent => "recent".into(),
            Collection::All => "all".into(),
            Collection::CannotStart => "unavailable".into(),
            Collection::Added => "added".into(),
            Collection::Circuits => "circuits".into(),
            Collection::Country(code) => format!("country:{}", code.as_deref().unwrap_or("")),
        };
        State {
            collection: Some(collection),
            query: self.filter.query.clone(),
            sort: self.sort.word().into(),
            filter: self.filter.surface.map(|n| n.to_string()).unwrap_or_default(),
            length: (self.filter.length as usize).to_string(),
            class: match &self.filter.country {
                None => String::new(),
                Some(code) => code.clone().unwrap_or_else(|| "-".to_owned()),
            },
            selected: self.rows.get(self.nav.cursor).map(|r| r.number.to_string()),
            scroll: self.nav.offset(),
        }
    }

    pub fn restore(&mut self, state: &crate::browser_state::State) {
        let Some(collection) = state.collection.as_deref() else { return };
        // A country kept as the collection (an older save) is the country dropdown now.
        let old_country = collection.strip_prefix("country:").map(|c| (!c.is_empty()).then(|| c.to_owned()));
        self.collection = match collection {
            "favourites" => Collection::Favourites,
            "recent" => Collection::Recent,
            "unavailable" => Collection::CannotStart,
            "added" => Collection::Added,
            "circuits" => Collection::Circuits,
            _ => Collection::All,
        };
        let country = match state.class.as_str() {
            "" => old_country,
            "-" => Some(None),
            code => Some(Some(code.to_owned())),
        };
        let country = country.filter(|c| self.rows.iter().any(|r| r.country == *c));
        self.filter = Filter {
            query: state.query.clone(),
            surface: state.filter.parse().ok().filter(|n| *n < 3),
            length: state.length.parse::<usize>().ok().and_then(|n| Length::ALL.get(n)).copied().unwrap_or_default(),
            country,
        };
        self.sort = Sort::from_word(&state.sort);
        let number = state.selected.as_deref().and_then(|s| s.parse::<u32>().ok());
        let cursor = self.rows.iter().position(|r| Some(r.number) == number).unwrap_or(0);
        self.nav.restore(cursor, state.scroll);
        self.visited = true;
        self.made = None;
    }

    /// The row of the stage with `number`.
    pub fn row_of(&self, number: u32) -> Option<&StageRow> {
        self.rows.iter().find(|r| r.number == number)
    }

    /// Opens the browser with `selected` (a catalog position) highlighted: on the favourites when it is one, else
    /// on all stages. `collection` opens it on that instead.
    pub fn open_on(&mut self, selected: usize, favourites: &[u32], collection: Option<Collection>) {
        self.target = crate::car_browser::Target::Drive;
        let favourite = self.rows.get(selected).is_some_and(|r| favourites.contains(&r.number));
        if !self.visited || collection.is_some() {
            self.filter.country = None;
            self.collection =
                match collection.unwrap_or(if favourite { Collection::Favourites } else { Collection::All }) {
                    Collection::Country(code) => {
                        self.filter.country = Some(code);
                        Collection::All
                    }
                    other => other,
                };
            self.filter.query.clear();
            self.nav.show(selected);
        } else {
            self.nav.resume();
        }
        self.visited = true;
        self.open = true;
        self.focus = true;
    }

    /// Shows stage `index` (a catalog position) in the list of all stages, with no search.
    pub fn show_stage(&mut self, index: usize) {
        self.collection = Collection::All;
        self.filter.query.clear();
        self.filter.country = None;
        self.nav.show(index);
        self.visited = true;
    }

    /// Makes this the Stages page's browser, on all stages with `selected` (a catalog position) highlighted the first
    /// time.
    pub fn start_page(&mut self, selected: usize) {
        self.page = true;
        if !self.visited {
            self.nav.show(selected);
            self.visited = true;
        }
    }

    /// Opens the browser for `target`, as [`StageBrowser::open_on`].
    pub fn open_for(&mut self, target: crate::car_browser::Target, selected: usize, favourites: &[u32]) {
        self.open_on(selected, favourites, None);
        self.target = target;
    }

    /// The list, made again when what it depends on changed; the cursor moves to a stage in it when it is not.
    fn refresh(&mut self, favourites: &[u32], recent: &[u32]) {
        let made = (self.collection.clone(), self.filter.clone(), self.sort, favourites.to_vec(), recent.to_vec());
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
            |row| Action::Favourite(row.number),
            Action::Back,
        );
        if self.page && action == Some(Action::Back) {
            // A page: Esc does nothing.
            action = None;
        }
        let noun = self.noun();
        let title = if self.page { "Stages".to_owned() } else { format!("Choose a {noun}") };
        let subtitle = Some(self.heading.as_str()).filter(|h| self.page && !h.is_empty());
        let get_more = self.page.then_some(("Get more stages…", "Stages from RSF, BTBfin and RBR Czech"));
        let clicks = browser::header(ui, &title, subtitle, get_more);
        if clicks.back {
            action = Some(Action::Back);
        }
        self.get_more |= clicks.get_more;
        if clicks.random {
            self.random();
        }
        self.filter_bar(ui, noun, env.favourites, env.recent);
        let chosen = self.body(ui, &mut env);
        chosen.or(action)
    }

    /// A random stage of the list as filtered becomes the highlighted one.
    fn random(&mut self) {
        if let Some(k) = random_row(&self.lines, |k| self.rows[k].startable()) {
            self.nav.show(k);
        }
    }

    /// The scopes of the filter bar: all stages, the favourites, the recent picks, the stages added by you and the
    /// circuits (the Stages page's, when there are some) and the stages that cannot start (when there are some).
    fn scopes(&self) -> Vec<(Collection, &'static str)> {
        let mut out =
            vec![(Collection::All, "All"), (Collection::Favourites, "Favourites"), (Collection::Recent, "Recent")];
        for (collection, name, any) in [
            (Collection::Added, "Added by you", self.rows.iter().any(|r| r.added)),
            (Collection::Circuits, "Circuits", self.rows.iter().any(|r| r.circuit)),
            (Collection::CannotStart, "Cannot start", self.rows.iter().any(|r| !r.startable())),
        ] {
            if any {
                out.push((collection, name));
            }
        }
        out
    }

    /// The scopes' counts for their hover text: the favourites and recent picks that are stages, the stages that can
    /// start, and the added, circuits and blocked ones.
    fn scope_counts(&self, favourites: &[u32], recent: &[u32]) -> String {
        let known = |numbers: &[u32]| numbers.iter().filter(|n| self.rows.iter().any(|r| r.number == **n)).count();
        let startable = self.rows.iter().filter(|r| r.startable()).count();
        let mut text = format!("All {startable} · Favourites {} · Recent {}", known(favourites), known(recent));
        for (name, n) in [
            ("Added by you", self.rows.iter().filter(|r| r.added).count()),
            ("Circuits", self.rows.iter().filter(|r| r.circuit).count()),
            ("Cannot start", self.rows.len() - startable),
        ] {
            if n > 0 {
                text += &format!(" · {name} {n}");
            }
        }
        text
    }

    /// The countries of the stages by name, and whether some have none.
    fn countries(&self) -> Vec<(Option<String>, String)> {
        let named: BTreeMap<&str, &str> =
            self.rows.iter().filter_map(|r| Some((r.country_name(), r.country.as_deref()?))).collect();
        let mut out: Vec<(Option<String>, String)> =
            named.into_iter().map(|(name, code)| (Some(code.to_owned()), name.to_owned())).collect();
        if self.rows.iter().any(|r| r.country.is_none()) {
            out.push((None, "No country".to_owned()));
        }
        out
    }

    /// The search, the scopes and the surface, country, length and sort dropdowns.
    fn filter_bar(&mut self, ui: &mut egui::Ui, noun: &str, favourites: &[u32], recent: &[u32]) {
        let scopes = self.scopes();
        let labels: Vec<&str> = scopes.iter().map(|(_, l)| *l).collect();
        let scope_options: Vec<(Collection, &str)> = scopes.iter().map(|(c, l)| (c.clone(), *l)).collect();
        let surfaces: Vec<(Option<u32>, String)> = std::iter::once((None, "Any surface".to_owned()))
            .chain(SURFACES.iter().enumerate().map(|(n, s)| (Some(n as u32), (*s).to_owned())))
            .collect();
        let countries: Vec<(Option<Option<String>>, String)> = std::iter::once((None, "All countries".to_owned()))
            .chain(self.countries().into_iter().map(|(code, name)| {
                let n = self.rows.iter().filter(|r| r.startable() && r.country == code).count();
                (Some(code), format!("{name}  {n}"))
            }))
            .collect();
        let lengths: Vec<(Length, String)> = Length::ALL.map(|l| (l, l.long_label().to_owned())).to_vec();
        let sorts: Vec<(Sort, String)> = Sort::ALL.map(|s| (s, s.label().to_owned())).to_vec();
        const DROPS: [f32; 4] = [104.0, 124.0, 100.0, 128.0];
        let drops_width = DROPS.iter().sum::<f32>() + 4.0 * 8.0;
        let seg = browser::segmented_width(ui, &labels);
        let before = (self.filter.clone(), self.sort, self.collection.clone());
        let hint = format!("Search {noun}s: name, country, author, number");
        browser::bar(ui, |ui| {
            let room = ui.available_width() - seg - 16.0;
            let one_row = room - drops_width >= 150.0;
            let search = (if one_row { room - drops_width - 8.0 } else { room }).clamp(150.0, 320.0);
            let drops =
                |s: &mut Self, ui: &mut egui::Ui| s.dropdowns(ui, (&surfaces, &countries, &lengths, &sorts), DROPS);
            ui.horizontal(|ui| {
                browser::search_box(ui, &hint, &mut self.filter.query, &mut self.focus, search);
                let mut scope = self.collection.clone();
                let counts = self.scope_counts(favourites, recent);
                if kit::segmented(ui, "stages scope", &mut scope, &scope_options).on_hover_text(counts).changed() {
                    self.collection = scope;
                    self.filter.query.clear();
                }
                if one_row {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| drops(self, ui));
                }
            });
            if !one_row {
                ui.add_space(8.0);
                ui.horizontal(|ui| drops(self, ui));
            }
        });
        if before != (self.filter.clone(), self.sort, self.collection.clone()) {
            // Back to the search box, where the arrows move through the list.
            self.focus = true;
        }
    }

    /// The surface, country, length and sort dropdowns, reversed in a right-to-left layout (the sort is the rightmost).
    fn dropdowns(&mut self, ui: &mut egui::Ui, options: Options, widths: [f32; 4]) {
        let (surfaces, countries, lengths, sorts) = options;
        let surface_text = self.filter.surface.map_or("Any surface", |s| SURFACES[s as usize]).to_owned();
        let country_text = match &self.filter.country {
            None => "All countries".to_owned(),
            Some(code) => code.as_deref().map_or("No country", country_name).to_owned(),
        };
        let length_text = self.filter.length.long_label().to_owned();
        let sort_text = format!("Sort: {}", self.sort.label());
        let surface = |s: &mut Self, ui: &mut egui::Ui| {
            browser::dropdown(ui, "stages surface", widths[0], &surface_text, &mut s.filter.surface, surfaces)
        };
        let country = |s: &mut Self, ui: &mut egui::Ui| {
            browser::dropdown(ui, "stages country", widths[1], &country_text, &mut s.filter.country, countries)
        };
        let length = |s: &mut Self, ui: &mut egui::Ui| {
            browser::dropdown(ui, "stages length", widths[2], &length_text, &mut s.filter.length, lengths)
        };
        let sort = |s: &mut Self, ui: &mut egui::Ui| {
            browser::dropdown(ui, "stages sort", widths[3], &sort_text, &mut s.sort, sorts)
        };
        if ui.layout().main_dir() == egui::Direction::RightToLeft {
            sort(self, ui);
            length(self, ui);
            country(self, ui);
            surface(self, ui);
        } else {
            surface(self, ui);
            country(self, ui);
            length(self, ui);
            sort(self, ui);
        }
    }

    /// The list and the inspector below the filter bar.
    fn body(&mut self, ui: &mut egui::Ui, env: &mut Env) -> Option<Action> {
        let mut action = None;
        let noun = self.noun();
        let page = self.page;
        let selected = self.rows.get(self.nav.cursor).and_then(|row| Some((row.clone(), env.maps.get(row.index)?)));
        let favourite = selected.as_ref().is_some_and(|(row, _)| env.favourites.contains(&row.number));
        let confirm = &mut self.confirm;
        let mut removal = None;
        let (previews, facts) = (&mut *env.previews, env.facts);
        browser::inspector(
            ui,
            |ui| {
                if let Some((row, map)) = &selected {
                    inspect(ui, row, map, previews, facts);
                }
            },
            |ui| {
                if let Some((row, _)) = &selected {
                    let (chosen, remove) = actions(ui, row, noun, page, favourite, confirm);
                    action = chosen;
                    removal = remove.then_some(row.index);
                }
            },
        );
        self.removal = removal.or(self.removal);
        let chosen = self.list(ui, env.favourites, env.recent);
        chosen.or(action)
    }

    /// The list with its header row and its hint bar; a stage chosen or starred with the mouse.
    fn list(&mut self, ui: &mut egui::Ui, favourites: &[u32], recent: &[u32]) -> Option<Action> {
        let mut action = None;
        let noun = self.noun();
        let count = items(&self.lines).count();
        let shown = if self.filter.query.trim().is_empty() {
            format!("{count} shown")
        } else {
            format!("{count} found in all {noun}s")
        };
        let verb = if self.page { "Drive" } else { "Choose" };
        let mut keys = vec![("↑↓", "Move"), ("↵", verb), ("Ctrl D", "Favourite")];
        if !self.page {
            keys.push(("Esc", "Back"));
        }
        // The author in place of the country within a country.
        let third = if self.filter.country.is_some() { Column::Author } else { Column::Country };
        let (collection, empty) = (self.collection.clone(), self.filter.query.trim().is_empty());
        browser::hint_bar(ui, &shown, &keys, |ui| {
            let titles = [(third.label(), false), ("Length", true), ("Surface", false)];
            browser::list_header(ui, "Name", &WIDTHS, &titles);
            if count == 0 {
                let hint = match (&collection, empty) {
                    (Collection::Favourites, true) if favourites.is_empty() => {
                        format!("Click ☆ next to a {noun} to keep it here.")
                    }
                    (Collection::Recent, true) if recent.is_empty() => {
                        format!("The {noun}s you choose are listed here.")
                    }
                    _ => "Nothing matches.".to_owned(),
                };
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label(RichText::new(hint).color(TEXT_2));
                });
            }
            list_rows(ui, &mut self.nav, "stages", &self.lines, |ui, nav, range| {
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
                    let favourite = favourites.contains(&row.number);
                    draw_row(ui, rect, row, third, k == nav.cursor, response.hovered(), favourite);
                    let response = match &row.problem {
                        Some(problem) => response.on_hover_text(format!("cannot start: {problem}")),
                        None => response,
                    };
                    if star.clicked() {
                        action = Some(Action::Favourite(row.number));
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

/// The dropdowns' options: surface, country, length, sort.
type Options<'a> =
    (&'a [(Option<u32>, String)], &'a [(Option<Option<String>>, String)], &'a [(Length, String)], &'a [(Sort, String)]);

/// What kind of stage it is, as the inspector says it.
fn kind(row: &StageRow) -> &'static str {
    match (row.retail, row.btb, row.added) {
        (true, ..) => "RBR 1.02",
        (_, true, true) => "Added by you · BTB",
        (_, false, true) => "Added by you",
        (_, true, false) => "RallySimFans · BTB",
        _ => "RallySimFans",
    }
}

/// The inspector's scrolling part: the picture, the name with its length and surface, and the facts in a panel.
fn inspect(
    ui: &mut egui::Ui,
    row: &StageRow,
    map: &Map,
    previews: &mut Previews,
    facts: &dyn Fn(&StageRow) -> Vec<(&'static str, String)>,
) {
    previews.stage_ui(ui, map);
    let length = row.km.map_or("unknown length".to_owned(), |km| format!("{km:.1} km"));
    browser::title(ui, &row.name, &format!("{length} · {}", SURFACES[row.surface as usize]));
    kit::panel(ui, |rows| {
        // A BTB stage's number is ours, made up from its folder: not shown.
        if !row.btb {
            rows.row("Number", |ui| _ = ui.label(row.number.to_string()));
        }
        rows.row("Country", |ui| _ = ui.label(row.country_name()));
        rows.row("Author", |ui| _ = ui.label(row.author.as_deref().unwrap_or("unknown")));
        rows.row("Source", |ui| _ = ui.label(kind(row)));
        for (label, value) in facts(row) {
            rows.row(label, |ui| _ = ui.add(egui::Label::new(value).wrap()));
        }
        if let Some(folder) = &row.folder {
            rows.row("Folder", |ui| {
                if kit::link(ui, "Open").on_hover_text("Open the folder").clicked() {
                    crate::crash::open(folder.as_os_str());
                }
                let name = folder.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                let font = theme::mono(11.0);
                let text = left_truncated(ui, &name, &font, ui.available_width() - 8.0);
                ui.label(RichText::new(text).font(font).color(TEXT_2)).on_hover_text(folder.display().to_string());
            });
        }
    });
    if row.btb {
        ui.add_space(10.0);
        kit::status_dot(ui, theme::WARN, "BTB stage: experimental.");
    }
}

/// The inspector's pinned part: why the stage cannot start, Favourite and the primary button, and the removal of an added
/// stage (the page's) with its confirmation; what was chosen, and whether the removal was confirmed.
fn actions(
    ui: &mut egui::Ui,
    row: &StageRow,
    noun: &str,
    page: bool,
    favourite: bool,
    confirm: &mut Option<u32>,
) -> (Option<Action>, bool) {
    let mut action = None;
    if let Some(problem) = &row.problem {
        kit::status_dot(ui, theme::WARN, &format!("Cannot start: {problem}"));
        ui.add_space(8.0);
    }
    ui.horizontal(|ui| {
        if favourite_button(ui, favourite).clicked() {
            action = Some(Action::Favourite(row.number));
        }
        let label = match (page, row.circuit) {
            (true, true) => "Drive this circuit".to_owned(),
            (true, false) => "Drive this stage".to_owned(),
            (false, _) => format!("Choose {noun}"),
        };
        let tip = if page { "Enter" } else { "Enter, or a double click" };
        if drive_button(ui, &label, row.startable(), tip).clicked() {
            action = Some(Action::Choose(row.index));
        }
    });
    let mut removal = false;
    if page && row.added {
        ui.add_space(10.0);
        if *confirm == Some(row.number) {
            ui.label(RichText::new("Delete its folder? Your records and ghosts on it stay.").color(TEXT_2));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new("Remove").color(theme::ERROR))).clicked() {
                    removal = true;
                    *confirm = None;
                }
                if ui.button("Cancel").clicked() {
                    *confirm = None;
                }
            });
        } else if ui.button("Remove…").on_hover_text("Delete the stage's folder from your files").clicked() {
            *confirm = Some(row.number);
        }
    } else {
        *confirm = None;
    }
    (action, removal)
}

/// `text` shortened from the left with an ellipsis to fit `width`.
fn left_truncated(ui: &egui::Ui, text: &str, font: &FontId, width: f32) -> String {
    let fits = |t: &str| ui.painter().layout_no_wrap(t.to_owned(), font.clone(), TEXT).size().x <= width;
    if fits(text) {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let tail = |n: usize| format!("…{}", chars[chars.len() - n..].iter().collect::<String>());
    tail((0..chars.len()).rev().find(|&n| fits(&tail(n))).unwrap_or(0))
}

/// The right-aligned columns of the list, widths: country or author, length, surface.
const WIDTHS: [f32; 3] = [130.0, 64.0, 84.0];

#[derive(Clone, Copy)]
enum Column {
    Country,
    Author,
}
impl Column {
    fn label(self) -> &'static str {
        match self {
            Self::Country => "Country",
            Self::Author => "Author",
        }
    }
    fn text(self, row: &StageRow) -> &str {
        match self {
            Self::Country => row.country.as_deref().map_or("—", country_name),
            Self::Author => row.author.as_deref().unwrap_or(""),
        }
    }
}

/// A stage's row: its star, name, country or author, length and surface; the highlighted one filled.
fn draw_row(ui: &egui::Ui, rect: Rect, row: &StageRow, column: Column, cursor: bool, hovered: bool, favourite: bool) {
    let p = ui.painter();
    let (c, name_right) = browser::columns(rect, &WIDTHS);
    let startable = row.startable();
    paint_selection(ui, rect, cursor, hovered);
    paint_star(p, Pos2::new(rect.left() + 14.0, rect.center().y), favourite, hovered || cursor);
    let (name_colour, dim) = if startable { (TEXT_LIST, TEXT_2) } else { (TEXT_DISABLED, TEXT_3) };
    let font = FontId::proportional(13.0);
    paint_cell(p, rect, browser::name_x(rect), name_right, &row.name, font.clone(), name_colour);
    paint_cell(p, rect, c[0], c[1] - 8.0, column.text(row), font.clone(), dim);
    let km = row.km.map_or(String::new(), |km| format!("{km:.1} km"));
    let km = p.layout_no_wrap(km, theme::mono(11.0), dim);
    p.galley(Pos2::new(c[1] + WIDTHS[1] - km.size().x, rect.center().y - km.size().y / 2.0), km, dim);
    match &row.problem {
        Some(problem) => paint_cell(p, rect, c[2], rect.right(), problem, FontId::proportional(12.0), theme::WARN),
        None => {
            p.circle_filled(Pos2::new(c[2] + 3.0, rect.center().y), 3.0, SURFACE_DOTS[row.surface as usize]);
            paint_cell(p, rect, c[2] + 13.0, rect.right(), SURFACES[row.surface as usize], font, dim);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Stage;
    use egui::{Key, Modifiers};
    use rbr_formats::rsf;
    use std::time::{Duration, Instant};

    const COUNTRIES: [&str; 8] = ["FI", "CZ", "FR", "PL", "SK", "ES", "GB", "DE"];

    /// A made-up RallySimFans stage, encrypted 4 in 11 (as about 160 of a full install's 450); no third-party data.
    fn rsf_map(k: u32) -> Map {
        let id = 100 + k;
        let surface = [rsf::Surface::Gravel, rsf::Surface::Tarmac, rsf::Surface::Snow][k as usize % 3];
        let stage = rsf::Stage {
            id,
            name: format!("Synthetic {k} {}", ["Forest", "Hills", "Pass", "Valley"][k as usize % 4]),
            folder: format!("/synthetic/Maps/{id}-x").into(),
            track: format!("track-{id}"),
            variants: vec!['M'],
            broken: Vec::new(),
            particles: None,
            surface: Some(surface),
            length: Some(2.0 + (k % 25) as f32),
            author: Some(format!("author{}", k % 17)),
            country: Some(COUNTRIES[k as usize % COUNTRIES.len()].into()),
            reference_time: None,
            default_wetness: None,
            animation_path: None,
            weathers: Vec::new(),
            problem: (k % 11 < 4).then(|| format!("track-{id}_M.lbs is encrypted by RallySimFans")),
        };
        Map {
            number: id,
            stages: vec![Stage { name: stage.stage('M'), map: id, letter: 'M' }],
            origin: Origin::Community(stage),
        }
    }

    fn rows_of(maps: &[Map]) -> Vec<StageRow> {
        let problem = |m: &Map| m.problem().cloned();
        maps.iter().enumerate().map(|(i, m)| StageRow::new(i, m, problem(m))).collect()
    }

    /// The names of `lines`, headers in brackets.
    fn names(rows: &[StageRow], lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| match l {
                Line::Header(h) => format!("[{h}]"),
                Line::Row(k) => rows[*k].name.clone(),
            })
            .collect()
    }

    fn count(lines: &[Line]) -> usize {
        lines.iter().filter(|l| matches!(l, Line::Row(_))).count()
    }

    /// Tanner (retail 54, Japan, gravel, 3.9 km) among the synthetic stages 1 (Czechia, tarmac, blocked), 3 (Poland,
    /// gravel, blocked), 5 (Spain, snow, 7 km), 8 (Finland, snow, 10 km) and 10 (France, tarmac, 12 km).
    fn some() -> Vec<StageRow> {
        let retail = Map { number: 54, stages: Vec::new(), origin: Origin::Retail };
        rows_of(&[retail, rsf_map(5), rsf_map(1), rsf_map(8), rsf_map(3), rsf_map(10)])
    }

    #[test]
    fn a_btb_stage_has_its_country() {
        let btb = crate::btb::Btb {
            folder: "/rsf/RX_CONTENT/Tracks/Hokkara".into(),
            name: "Hokkara".into(),
            author: None,
            country: Some("FI".into()),
            km: None,
            surface: "gravel",
            circuit: false,
            problem: None,
        };
        let rows = rows_of(&[Map { number: 1040, stages: Vec::new(), origin: Origin::Btb(btb) }]);
        assert_eq!(rows[0].country.as_deref(), Some("FI"));
        assert_eq!(rows[0].country_name(), "Finland");
    }

    #[test]
    fn a_retail_stage_has_its_rallys_country_and_the_game_length() {
        let rows = some();
        assert_eq!(rows[0].country.as_deref(), Some("JP"));
        assert_eq!(rows[0].detail(), "54 · 3.9 km · Gravel");
        assert_eq!(rows[0].country_name(), "Japan");
        assert!(rows[0].retail && rows[0].startable());
        assert_eq!(rows[1].country_name(), "Spain");
        assert_eq!(country_name("ZZ"), "ZZ");
    }

    #[test]
    fn all_stages_lists_the_startable_ones_and_the_blocked_ones_have_their_own_collection() {
        let rows = some();
        let all = lines(&rows, &Collection::All, &Filter::default(), Sort::Name, &[], &[]);
        assert_eq!(names(&rows, &all), ["Synthetic 10 Pass", "Synthetic 5 Hills", "Synthetic 8 Forest", "Tanner"]);
        let blocked = lines(&rows, &Collection::CannotStart, &Filter::default(), Sort::Name, &[], &[]);
        assert_eq!(names(&rows, &blocked), ["Synthetic 1 Hills", "Synthetic 3 Valley"]);
        let reasons: Vec<_> = rows.iter().filter_map(|r| r.problem.clone()).collect();
        assert!(reasons.iter().all(|r| r == "encrypted by RallySimFans: ReRBR can't read it"));
        let by_length = lines(&rows, &Collection::All, &Filter::default(), Sort::Length, &[], &[]);
        assert_eq!(
            names(&rows, &by_length),
            ["Tanner", "Synthetic 5 Hills", "Synthetic 8 Forest", "Synthetic 10 Pass"]
        );
    }

    #[test]
    fn a_country_has_the_retail_and_the_rallysimfans_stages_together() {
        let mut maps = vec![Map { number: 54, stages: Vec::new(), origin: Origin::Retail }, rsf_map(5)];
        let Origin::Community(stage) = &mut maps[1].origin else { panic!("synthetic RSF map") };
        stage.country = Some("jp".into());
        let rows = rows_of(&maps);
        let filter = Filter { country: Some(Some("JP".into())), ..Filter::default() };
        let japan = lines(&rows, &Collection::All, &filter, Sort::Name, &[], &[]);
        assert_eq!(names(&rows, &japan), ["Synthetic 5 Hills", "Tanner"]);
        let mut browser = StageBrowser::default();
        browser.set_rows(rows);
        let japan = browser.countries().into_iter().find(|(code, _)| *code == Some("JP".to_owned()));
        assert_eq!(japan, Some((Some("JP".to_owned()), "Japan".to_owned())));
    }

    #[test]
    fn favourites_sort_like_any_list_and_keep_the_ones_that_cannot_start_last() {
        let rows = some();
        // Starred in the order 3 (blocked), 8, 54, and 999 (not in the catalog).
        let starred = [103, 108, 54, 999];
        let by_name = lines(&rows, &Collection::Favourites, &Filter::default(), Sort::Name, &starred, &[]);
        assert_eq!(names(&rows, &by_name), ["Synthetic 8 Forest", "Tanner", "[Cannot start]", "Synthetic 3 Valley"]);
        let snow = Filter { surface: Some(2), ..Filter::default() };
        let snowy = lines(&rows, &Collection::Favourites, &snow, Sort::Name, &starred, &[]);
        assert_eq!(names(&rows, &snowy), ["Synthetic 8 Forest"]);
        // No favourites: an empty list.
        assert!(lines(&rows, &Collection::Favourites, &Filter::default(), Sort::Name, &[], &[]).is_empty());
    }

    #[test]
    fn the_recent_picks_keep_their_order_of_use() {
        let rows = some();
        let recent = [108, 54, 105];
        let list = lines(&rows, &Collection::Recent, &Filter::default(), Sort::Name, &[], &recent);
        assert_eq!(names(&rows, &list), ["Synthetic 8 Forest", "Tanner", "Synthetic 5 Hills"]);
        // Sorting by recently driven puts the others after them.
        let all = lines(&rows, &Collection::All, &Filter::default(), Sort::Recent, &[], &recent);
        assert_eq!(names(&rows, &all), ["Synthetic 8 Forest", "Tanner", "Synthetic 5 Hills", "Synthetic 10 Pass"]);
    }

    #[test]
    fn a_search_covers_every_stage_grouped_by_country_with_the_favourites_first() {
        let rows = some();
        let find = |query: &str, collection: Collection, favourites: &[u32]| {
            let filter = Filter { query: query.into(), ..Filter::default() };
            names(&rows, &lines(&rows, &collection, &filter, Sort::Name, favourites, &[]))
        };
        // Whatever the collection is: the country's name and code, the author, the number, retail.
        assert_eq!(find("japan", Collection::CannotStart, &[]), ["[Japan]", "Tanner"]);
        assert_eq!(find("jp", Collection::Favourites, &[]), ["[Japan]", "Tanner"]);
        assert_eq!(find("author5", Collection::All, &[]), ["[Spain]", "Synthetic 5 Hills"]);
        assert_eq!(find("108", Collection::All, &[]), ["[Finland]", "Synthetic 8 Forest"]);
        assert_eq!(find("retail", Collection::All, &[]), ["[Japan]", "Tanner"]);
        assert_eq!(
            find("SYNTHETIC hills", Collection::All, &[]),
            ["[Spain]", "Synthetic 5 Hills", "[Cannot start]", "Synthetic 1 Hills"]
        );
        assert_eq!(
            find("synthetic", Collection::All, &[108]),
            [
                "[Favourites]",
                "Synthetic 8 Forest",
                "[France]",
                "Synthetic 10 Pass",
                "[Spain]",
                "Synthetic 5 Hills",
                "[Cannot start]",
                "Synthetic 1 Hills",
                "Synthetic 3 Valley"
            ]
        );
        assert!(find("nothing", Collection::All, &[]).is_empty());
    }

    #[test]
    fn the_surface_and_length_filters_narrow_every_list() {
        let rows = some();
        let list = |filter: Filter| count(&lines(&rows, &Collection::All, &filter, Sort::Name, &[], &[]));
        assert_eq!(list(Filter { surface: Some(1), ..Filter::default() }), 1);
        assert_eq!(list(Filter { surface: Some(2), ..Filter::default() }), 2);
        assert_eq!(list(Filter { length: Length::Short, ..Filter::default() }), 1);
        assert_eq!(list(Filter { length: Length::Medium, ..Filter::default() }), 1);
        assert_eq!(list(Filter { length: Length::Long, ..Filter::default() }), 2);
        assert_eq!(list(Filter { length: Length::Longest, ..Filter::default() }), 0);
        assert!(Length::Short.contains(Some(3.9)) && !Length::Short.contains(None) && Length::Any.contains(None));
    }

    #[test]
    fn newest_sort_puts_the_highest_numbers_first() {
        let rows = some();
        let newest = lines(&rows, &Collection::All, &Filter::default(), Sort::Newest, &[], &[]);
        assert_eq!(items(&newest).collect::<Vec<_>>(), [5, 3, 1, 0]);
        let filter = Filter { query: "synthetic".into(), ..Default::default() };
        let found = lines(&rows, &Collection::All, &filter, Sort::Newest, &[110], &[]);
        assert_eq!(items(&found).take(3).collect::<Vec<_>>(), [5, 3, 1], "search also sorts globally by number");
        let favourite = lines(&rows, &Collection::Favourites, &Filter::default(), Sort::Newest, &[54, 110, 105], &[]);
        assert_eq!(items(&favourite).collect::<Vec<_>>(), [5, 1, 0]);
    }

    #[test]
    fn reopening_and_restoring_keep_the_stage_picker_state() {
        let mut browser = StageBrowser::default();
        browser.set_rows(some());
        browser.open_on(1, &[], None);
        browser.collection = Collection::Recent;
        browser.filter = Filter {
            query: "synthetic".into(),
            surface: Some(2),
            length: Length::Medium,
            country: Some(Some("ES".into())),
        };
        browser.sort = Sort::Newest;
        browser.nav.restore(1, 140.0);
        browser.refresh(&[], &[]);
        let state = browser.state();
        browser.open = false;
        browser.open_on(0, &[54], None);
        browser.refresh(&[], &[]);
        assert_eq!(browser.state(), state, "reopening must not replace browsing state with the chosen stage");
        let mut restored = StageBrowser::default();
        let mut rows = some();
        rows.swap(0, 1);
        restored.set_rows(rows);
        restored.restore(&state);
        restored.open_on(0, &[], None);
        restored.refresh(&[], &[]);
        assert_eq!(restored.state(), state, "selection uses stage number, not a stale catalogue position");
        restored.open_on(0, &[54], Some(Collection::Favourites));
        assert_eq!(restored.collection, Collection::Favourites);
        assert!(restored.filter.query.is_empty());
        assert_eq!(restored.sort, Sort::Newest);
    }

    /// Draws the browser open in a window-sized frame, without a GPU; the time it took.
    fn frame(ctx: &egui::Context, browser: &mut StageBrowser, maps: &[Map], previews: &mut Previews) -> Duration {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 700.0))),
            ..Default::default()
        };
        let facts = |_: &StageRow| vec![("Weather", "dry".to_owned())];
        let start = Instant::now();
        ctx.run_pass(input, |ctx| {
            ctx.root_ui(|root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let env =
                        Env { maps, favourites: &[105, 110], recent: &[120], previews: &mut *previews, facts: &facts };
                    browser.ui(ui, env)
                })
            });
        })
        .textures_delta
        .clear();
        start.elapsed()
    }

    #[test]
    fn a_scrolled_picker_keeps_its_viewport_on_reopen_and_after_restart() {
        let maps: Vec<Map> = (0..500).map(rsf_map).collect();
        let mut browser = StageBrowser::default();
        browser.set_rows(rows_of(&maps));
        browser.open_on(250, &[], None);
        browser.nav.restore(250, 2000.0);
        let mut previews = Previews::new(rbr_formats::data::Data::new("/nonexistent", None));
        let ctx = egui::Context::default();
        frame(&ctx, &mut browser, &maps, &mut previews);
        assert_eq!(browser.nav.offset(), 2000.0);
        let state = browser.state();
        browser.open = false;
        browser.open_on(5, &[], None);
        frame(&ctx, &mut browser, &maps, &mut previews);
        assert_eq!(browser.state(), state);
        let mut restarted = StageBrowser::default();
        restarted.set_rows(rows_of(&maps));
        restarted.restore(&state);
        restarted.open_on(5, &[], None);
        frame(&egui::Context::default(), &mut restarted, &maps, &mut previews);
        assert_eq!(restarted.state(), state);
    }

    /// The scale of a full RallySimFans install, and more: 500 stages open, list and filter within a frame. The
    /// fastest of a few runs is timed, so a busy CI runner's stall does not fail it and a slowdown still does.
    #[test]
    fn a_big_catalog_opens_and_filters_at_once() {
        let maps: Vec<Map> = (0..500).map(rsf_map).collect();
        let (mut built, mut slowest) = (Duration::MAX, Duration::MAX);
        let mut browser = StageBrowser::default();
        for _ in 0..3 {
            let start = Instant::now();
            let rows = rows_of(&maps);
            built = built.min(start.elapsed());
            browser = StageBrowser::default();
            browser.set_rows(rows);
            let mut previews = Previews::new(rbr_formats::data::Data::new("/nonexistent", None));
            let ctx = egui::Context::default();
            browser.open_on(250, &[105, 110], None);
            let mut run = Duration::ZERO;
            for query in ["", "s", "sy", "syn", "synthetic 4", "synthetic 4 cz", "zzz", ""] {
                browser.filter.query = query.into();
                run = run.max(frame(&ctx, &mut browser, &maps, &mut previews));
            }
            browser.filter = Filter { surface: Some(2), length: Length::Long, ..Filter::default() };
            slowest = slowest.min(run.max(frame(&ctx, &mut browser, &maps, &mut previews)));
        }
        eprintln!("stage browser: rows built in {built:?}, slowest frame {slowest:?}");
        // The snowy 10-15 km stages that start.
        let snowy = (0..500).filter(|k| k % 3 == 2 && (8..13).contains(&(k % 25)) && k % 11 >= 4).count();
        assert_eq!(count(&browser.lines), snowy);
        assert!(browser.open);
        // Debug builds too: well within one 60 Hz frame for the rows and a 20 fps frame for a draw.
        assert!(built < Duration::from_millis(16), "{built:?}");
        assert!(slowest < Duration::from_millis(50), "{slowest:?}");
    }

    #[test]
    fn it_opens_on_the_favourites_with_the_selection_and_moves_over_the_stages_only() {
        let mut browser = StageBrowser::default();
        browser.set_rows(some());
        browser.open_on(0, &[54], None);
        assert_eq!(browser.collection, Collection::Favourites);
        browser.open_on(3, &[54], Some(Collection::All));
        assert_eq!(browser.collection, Collection::All);
        assert_eq!(browser.nav.cursor, 3);
        // All: Synthetic 10 (position 5 in the rows), Synthetic 5 (1), Synthetic 8 (3), Tanner (0).
        browser.refresh(&[54], &[]);
        browser.nav.step(&browser.lines, 1);
        assert_eq!(browser.nav.cursor, 0);
        browser.nav.step(&browser.lines, 1);
        assert_eq!(browser.nav.cursor, 0, "stays on the last one");
        browser.nav.step(&browser.lines, -10);
        assert_eq!(browser.nav.cursor, 5);
        // A collection without the cursor's stage moves it to the first stage of the list.
        browser.collection = Collection::CannotStart;
        browser.refresh(&[54], &[]);
        assert_eq!(browser.nav.cursor, 2);
    }

    /// One frame with `key` pressed (with `modifiers`); what the browser asked for.
    fn press(browser: &mut StageBrowser, maps: &[Map], key: Key, modifiers: Modifiers) -> Option<Action> {
        let mut previews = Previews::new(rbr_formats::data::Data::new("/nonexistent", None));
        let event = egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 700.0))),
            events: vec![egui::Event::ModifiersChanged(modifiers), event],
            ..Default::default()
        };
        let facts = |_: &StageRow| Vec::new();
        let mut asked = None;
        egui::Context::default()
            .run_pass(input, |ctx| {
                ctx.root_ui(|root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let env = Env { maps, favourites: &[], recent: &[], previews: &mut previews, facts: &facts };
                        // A second pass of the frame (egui's layout) has the key consumed already.
                        asked = browser.ui(ui, env).or(asked);
                    })
                });
            })
            .textures_delta
            .clear();
        asked
    }

    #[test]
    fn the_keys_choose_favourite_and_go_back() {
        let maps = [
            Map { number: 54, stages: Vec::new(), origin: Origin::Retail },
            rsf_map(5),
            rsf_map(1), // cannot start
        ];
        let mut browser = StageBrowser::default();
        browser.set_rows(rows_of(&maps));
        browser.open_on(1, &[], None);
        assert_eq!(press(&mut browser, &maps, Key::Enter, Modifiers::NONE), Some(Action::Choose(1)));
        assert_eq!(press(&mut browser, &maps, Key::D, Modifiers::COMMAND), Some(Action::Favourite(105)));
        assert_eq!(press(&mut browser, &maps, Key::Escape, Modifiers::NONE), Some(Action::Back));
        // The arrow moves the highlight to the next stage of the list (Tanner after Synthetic 5), which Enter chooses.
        assert_eq!(press(&mut browser, &maps, Key::ArrowDown, Modifiers::NONE), None);
        assert_eq!(press(&mut browser, &maps, Key::Enter, Modifiers::NONE), Some(Action::Choose(0)));
        // A stage that cannot start is not chosen.
        browser.collection = Collection::CannotStart;
        assert_eq!(press(&mut browser, &maps, Key::Enter, Modifiers::NONE), None);
        assert_eq!(browser.nav.cursor, 2);
    }

    #[test]
    fn the_stages_page_has_added_and_circuit_collections_and_ignores_escape() {
        let maps = [Map { number: 54, stages: Vec::new(), origin: Origin::Retail }, rsf_map(5), rsf_map(1)];
        let mut rows = rows_of(&maps);
        rows[1].added = true;
        rows[2].added = true; // cannot start, but removable
        rows[0].circuit = true;
        let added = lines(&rows, &Collection::Added, &Filter::default(), Sort::Name, &[], &[]);
        assert_eq!(names(&rows, &added), ["Synthetic 5 Hills", "[Cannot start]", "Synthetic 1 Hills"]);
        let circuits = lines(&rows, &Collection::Circuits, &Filter::default(), Sort::Name, &[], &[]);
        assert_eq!(names(&rows, &circuits), ["Tanner"]);
        let mut browser = StageBrowser::default();
        browser.set_rows(rows);
        let scopes: Vec<&str> = browser.scopes().iter().map(|(_, l)| *l).collect();
        assert_eq!(scopes, ["All", "Favourites", "Recent", "Added by you", "Circuits", "Cannot start"]);
        browser.start_page(1);
        assert_eq!(press(&mut browser, &maps, Key::Escape, Modifiers::NONE), None);
        assert_eq!(press(&mut browser, &maps, Key::Enter, Modifiers::NONE), Some(Action::Choose(1)));
    }

    /// `RBR_LAUNCHER_SHOTS=DIR`: the big catalog's browser drawn to `DIR/stage-browser-*.png`.
    #[test]
    fn big_catalog_screenshots() {
        let Some(dir) = std::env::var_os("RBR_LAUNCHER_SHOTS").map(std::path::PathBuf::from) else {
            return;
        };
        let mut maps: Vec<Map> = (0..500).map(rsf_map).collect();
        maps.insert(0, Map { number: 54, stages: Vec::new(), origin: Origin::Retail });
        let favourites = [54, 105, 110, 128];
        let mut previews = Previews::new(rbr_formats::data::Data::new("/nonexistent", None));
        for (name, collection) in [("favourites", Collection::Favourites), ("all", Collection::All)] {
            let mut browser = StageBrowser::default();
            browser.set_rows(rows_of(&maps));
            browser.open_on(1, &favourites, Some(collection));
            let facts = |_: &StageRow| vec![("Weather", "good dry · bad wet".to_owned()), ("Ghost", "none yet".into())];
            let rgba = crate::capture::draw(|ctx| {
                ctx.root_ui(|root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let env = Env {
                            maps: &maps,
                            favourites: &favourites,
                            recent: &[110],
                            previews: &mut previews,
                            facts: &facts,
                        };
                        browser.ui(ui, env)
                    })
                });
            })
            .unwrap();
            crate::capture::write_png(&dir.join(format!("stage-browser-{name}.png")), &rgba).unwrap();
        }
    }
}
