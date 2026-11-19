//! Downloading NGP and cars from gvrc.de, so that a game folder without NGP, or a player without cars, is set up from
//! the launcher: the Settings page's "Download NGP 7.4.778…" and "Download cars…", `launcher --download-ngp` and
//! `launcher --download-cars [NAME…]`.
//!
//! The list is gvrc.de's `carList.ini`, the one RBRCIT (the Windows car installer) reads and WorkerBee maintains: `[Plugins]`
//! names the NGP package (`plugin_physics_url`) and each `[Car_<n>]` a car's `name`, `cat`, `year`, `physics` (its
//! pack's folder), `trans` (its drive) and downloads (`link_physics`, `link_model`, `link_banks`). It is fetched
//! live, each time the list is opened.
//! Only `https://gvrc.de/` links are followed, and a car needs its `<Car> - NGP Physics.zip` (a model is a
//! `<Model> - Model.7z`, sound banks a `<Car> - banks.7z`; the original eight cars, which NGP's package covers, are left
//! out).
//!
//! - NGP's package is downloaded to a temp folder in the player's folder and checked to be NGP 7.4.778; only its
//!   `NGP.dll` and `physics.rbz` are kept, there (`ngp_install::KEPT`, `ngp_install::add_ngp_to`), and the DLL and
//!   physics.rbz fields set to them (`--download-ngp` leaves the fields, which then find the copy). The game folder,
//!   and any NGP in it (RallySimFans', or a version ReRBR does not read), is left as it is.
//! - A car's downloads go to a temp folder in `cars/` and are added as downloaded packs are (`car_packs::add_downloads`),
//!   each car with the model its list entry names.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::car_packs::{self, Added, Download, BANKS, MODEL, PHYSICS};
use crate::disc_install;
use crate::job::{with_path, Counter, Job, Progress, Report, Temp};
use crate::kit;
use crate::ngp_install;
use crate::theme::*;
use crate::Form;

/// gvrc.de's car list.
pub const LIST_URL: &str = "https://gvrc.de/ngp/tools/rbrcit/carlist/carList.ini";
/// The only links followed.
const HOST: &str = "https://gvrc.de/";
/// A download's size limit.
const MAX_BYTES: u64 = 2 << 30;

type Fail = String;

/// A car of the list: its name, category and year, its physics pack's folder (`physics`, the added pack's folder)
/// and its drive (`trans`), its physics pack, its model and its sound banks, as links.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub cat: String,
    pub year: String,
    /// The physics pack's folder name (`physics=`, e.g. `BMW M1 GrpB`): what the added pack's folder is.
    pub pack: String,
    /// The drive (`trans=`: `4WD`, `FWD`, `RWD`, `AWD`).
    pub trans: String,
    pub physics: String,
    pub model: Option<String>,
    pub banks: Option<String>,
}

impl Entry {
    /// Its category as RSF names the group (`Rally2` is `Rally 2`, a bare `A8` is `Group A8`), when it has one.
    pub fn group(&self) -> Option<String> {
        let cat = self.cat.trim();
        let group = match (cat.strip_prefix("Rally"), cat) {
            (Some(n), _) if !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) => format!("Rally {n}"),
            (_, "A5" | "A6" | "A7" | "A8") => format!("Group {cat}"),
            _ => cat.to_owned(),
        };
        (!group.is_empty()).then_some(group)
    }

    /// Its model year (`year=`), when it is a number.
    pub fn year(&self) -> Option<u32> {
        self.year.trim().parse().ok()
    }

    /// Its drive (`4WD`, `FWD`, `RWD`, `AWD`), when it has one.
    pub fn drive_train(&self) -> Option<String> {
        Some(self.trans.trim().to_owned()).filter(|t| !t.is_empty())
    }
}

/// What the list gives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub version: String,
    /// NGP's package.
    pub ngp: Option<String>,
    /// The cars to add, by name.
    pub cars: Vec<Entry>,
}

/// `text` (a `carList.ini`): its version, NGP's package and its cars. A car whose links are not gvrc.de's or not
/// named as downloads are, and the original cars, are left out; so are the commented lines.
pub fn parse(text: &str) -> Catalog {
    let mut catalog = Catalog::default();
    let mut sections: Vec<(&str, HashMap<&str, &str>)> = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if let Some(comment) = line.strip_prefix(';') {
            if let Some(version) = comment.trim().strip_prefix("Version:") {
                catalog.version = version.trim().to_owned();
            }
        } else if line.is_empty() || line.starts_with('#') {
            continue;
        } else if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name, HashMap::new()));
        } else if let (Some((key, value)), Some(last)) = (line.split_once('='), sections.last_mut()) {
            last.1.insert(key.trim(), value.trim());
        }
    }
    let link = |fields: &HashMap<&str, &str>, key: &str, suffix: &str| {
        let url = fields.get(key).copied().filter(|u| is_gvrc(u))?;
        let name = file_name(url)?;
        car_packs::strip_end(&name, suffix).is_some().then(|| url.to_owned())
    };
    for (section, fields) in &sections {
        if section.eq_ignore_ascii_case("Plugins") {
            catalog.ngp = fields
                .get("plugin_physics_url")
                .filter(|u| is_gvrc(u) && file_name(u).is_some())
                .map(|u| u.to_string());
        } else if section.get(..4).is_some_and(|s| s.eq_ignore_ascii_case("Car_")) {
            let field = |key: &str| fields.get(key).copied().unwrap_or_default().to_owned();
            let cat = field("cat");
            let (Some(physics), false) = (link(fields, "link_physics", PHYSICS), cat.eq_ignore_ascii_case("Original"))
            else {
                continue;
            };
            let model = link(fields, "link_model", MODEL);
            if model.is_none() && fields.get("link_model").is_some_and(|m| !m.is_empty()) {
                continue;
            }
            let banks = link(fields, "link_banks", BANKS);
            catalog.cars.push(Entry {
                name: field("name"),
                cat,
                year: field("year"),
                pack: field("physics"),
                trans: field("trans"),
                physics,
                model,
                banks,
            });
        }
    }
    catalog.cars.sort_by_key(|c| c.name.to_lowercase());
    catalog
}

/// Whether `url` is one of gvrc.de's.
fn is_gvrc(url: &str) -> bool {
    url.starts_with(HOST)
}

/// The download's file name: the link's last part, its `%XX`s decoded, when it is one file's (no folder, not hidden).
fn file_name(url: &str) -> Option<String> {
    let last = url.rsplit('/').next()?;
    let mut bytes = Vec::new();
    let mut raw = last.bytes();
    while let Some(b) = raw.next() {
        if b == b'%' {
            let hex: Vec<u8> = raw.by_ref().take(2).collect();
            bytes.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
        } else {
            bytes.push(b);
        }
    }
    let name = String::from_utf8(bytes).ok()?;
    let bad = |c: char| matches!(c, '/' | '\\' | ':' | '?' | '#') || c.is_control();
    (!name.is_empty() && !name.starts_with('.') && !name.contains(bad)).then_some(name)
}

pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("ReRBR/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(900)))
        .build()
        .into()
}

/// The list at `url`, read and parsed.
fn read_list(url: &str) -> Result<Catalog, Fail> {
    let bytes = agent()
        .get(url)
        .call()
        .and_then(|mut r| r.body_mut().read_to_vec())
        .map_err(|e| format!("Could not read the car list at {url}: {e}"))?;
    Ok(parse(&String::from_utf8_lossy(&bytes)))
}

/// The car list as a job for the Settings page.
fn list_job() -> Job<Catalog> {
    Job::start(|report, _| {
        report(&Progress { step: "Reading the car list", ..Progress::default() });
        read_list(LIST_URL)
    })
}

/// The size of the download at `url`, 0 when the server does not say.
pub(crate) fn size_of(agent: &ureq::Agent, url: &str) -> u64 {
    let response = agent.head(url).call().ok();
    let length = response.as_ref().and_then(|r| r.headers().get("content-length")?.to_str().ok());
    length.and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// Downloads `url` to the file `dest`.
pub(crate) fn fetch(agent: &ureq::Agent, url: &str, dest: &Path, count: &mut Counter) -> Result<(), Fail> {
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", file_name(url).unwrap_or_else(|| url.to_owned()));
    let mut response = agent.get(url).call().map_err(|e| error(&e))?;
    fs::create_dir_all(dest.parent().unwrap_or(dest)).map_err(|e| error(&e))?;
    let mut out = File::create(dest).map_err(|e| error(&e))?;
    let mut reader = response.body_mut().as_reader().take(MAX_BYTES);
    let mut buffer = vec![0; 64 << 10];
    loop {
        let n = reader.read(&mut buffer).map_err(|e| error(&e))?;
        if n == 0 {
            break;
        }
        out.write_all(&buffer[..n]).map_err(|e| error(&e))?;
        count.add(n as u64)?;
    }
    out.flush().map_err(|e| error(&e))
}

/// Downloads NGP's package into the folder `dir`: its path (for `disc_install`'s setup, which reads it).
pub fn download_ngp_package(dir: &Path, report: Report<'_>, cancel: &AtomicBool) -> Result<PathBuf, Fail> {
    report(&Progress { step: "Reading the car list", ..Progress::default() });
    let url = read_list(LIST_URL)?.ngp.ok_or("gvrc.de's list names no NGP package")?;
    let package = dir.join(file_name(&url).ok_or("a bad NGP link")?);
    let agent = agent();
    let mut count = Counter { step: "Downloading NGP", done: 0, total: size_of(&agent, &url), report, cancel };
    count.add(0)?;
    fetch(&agent, &url, &package, &mut count)?;
    Ok(package)
}

/// Downloads NGP's package into a temp folder beside `to` and writes only its `NGP.dll` and `physics.rbz` into `to`.
fn download_ngp_to(to: &Path, report: Report<'_>, cancel: &AtomicBool) -> Result<(), Fail> {
    let temp = Temp::create(to.parent().unwrap_or(Path::new(".")), "downloading")?;
    let package = download_ngp_package(&temp.0, report, cancel)?;
    ngp_install::add_ngp_to(&package, to, report, cancel)
}

/// The downloads of `cars`, each once (cars share models).
fn links(cars: &[Entry]) -> Vec<&str> {
    let mut urls: Vec<&str> = Vec::new();
    for entry in cars {
        for url in [Some(&entry.physics), entry.model.as_ref(), entry.banks.as_ref()].into_iter().flatten() {
            if !urls.contains(&url.as_str()) {
                urls.push(url);
            }
        }
    }
    urls
}

/// The sizes of the downloads of `cars`, asked eight at a time (the list's ~250 take under a second).
fn sizes_of(cars: &[Entry], cancel: &AtomicBool) -> HashMap<String, u64> {
    let (agent, urls) = (agent(), links(cars));
    let (next, sizes) = (AtomicUsize::new(0), Mutex::new(HashMap::new()));
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                while let Some(url) = urls.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let size = size_of(&agent, url);
                    sizes.lock().unwrap().insert(url.to_string(), size);
                }
            });
        }
    });
    sizes.into_inner().unwrap()
}

/// The download size of `cars` as shown (`1.5 GB`), `None` unless every download's size is known.
fn size_text(cars: &[Entry], sizes: Option<&HashMap<String, u64>>) -> Option<String> {
    let sizes = sizes?;
    let each: Option<Vec<u64>> = links(cars).iter().map(|u| sizes.get(*u).copied().filter(|&n| n > 0)).collect();
    match each.map(|n| n.iter().sum::<u64>() >> 20)? {
        mb if mb >= 1024 => Some(format!("{:.1} GB", mb as f64 / 1024.0)),
        mb => Some(format!("{} MB", mb.max(1))),
    }
}

/// Downloads `wanted` into a temp folder in `cars_dir` and adds them there (`car_packs::add_downloads`). One car whose
/// download fails does not stop the others; a cancel keeps the cars added before.
pub fn download_cars(
    cars_dir: &Path,
    wanted: &[Entry],
    report: Report<'_>,
    cancel: &AtomicBool,
) -> Result<Vec<Added>, Fail> {
    fs::create_dir_all(cars_dir).map_err(with_path(cars_dir))?;
    let temp = Temp::create(cars_dir, "downloading")?;
    let agent = agent();
    let urls = links(wanted);
    let total = urls.iter().map(|u| size_of(&agent, u)).sum();
    let mut count = Counter { step: "Downloading cars", done: 0, total, report, cancel };
    count.add(0)?;
    let mut files: HashMap<&str, Result<PathBuf, Fail>> = HashMap::new();
    for (k, url) in urls.iter().enumerate() {
        let dest = temp.0.join(k.to_string()).join(file_name(url).unwrap_or_default());
        let result = fetch(&agent, url, &dest, &mut count).map(|()| dest);
        if result.as_ref().is_err_and(|e| e.ends_with("cancelled")) {
            return Err("cancelled".into());
        }
        files.insert(url, result);
    }
    let (mut downloads, mut added) = (Vec::new(), Vec::new());
    for entry in wanted {
        let file = |url: &Option<String>| url.as_deref().map(|u| files[u].clone()).transpose();
        match (files[entry.physics.as_str()].clone(), file(&entry.model), file(&entry.banks)) {
            (Ok(physics), Ok(model), Ok(banks)) => downloads.push(Download {
                physics,
                model,
                banks,
                group: entry.group(),
                drive_train: entry.drive_train(),
                year: entry.year(),
            }),
            (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                added.push(Added { download: entry.name.clone(), result: Err(e) })
            }
        }
    }
    if !downloads.is_empty() {
        added.extend(car_packs::add_downloads(cars_dir, &downloads, report, cancel)?);
    }
    Ok(added)
}

/// The cars of `catalog` whose name contains `word` (ignoring case); the exact name wins.
fn find<'a>(catalog: &'a Catalog, word: &str) -> Vec<&'a Entry> {
    let word = word.to_lowercase();
    let exact: Vec<&Entry> = catalog.cars.iter().filter(|c| c.name.to_lowercase() == word).collect();
    match exact.is_empty() {
        true => catalog.cars.iter().filter(|c| c.name.to_lowercase().contains(&word)).collect(),
        false => exact,
    }
}

/// The command line's `--download-ngp`: into the player's folder `home` ([`ngp_install::KEPT`]).
pub fn ngp_cli(home: &Path, args: &[String]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("usage: rerbr --download-ngp".into());
    }
    let to = home.join(ngp_install::KEPT);
    download_ngp_to(&to, &disc_install::print_steps(), &AtomicBool::new(false))?;
    println!("NGP 7.4.778's NGP.dll and physics.rbz saved in {}", to.display());
    Ok(())
}

/// The command line's `--download-cars [NAME…]`, into `cars_dir`: without names, the list. Added cars that
/// still lack their group or drive train take them from the list first.
pub fn cars_cli(cars_dir: &Path, args: &[String]) -> Result<(), String> {
    let catalog = read_list(LIST_URL)?;
    if let Err(e) = car_packs::backfill(cars_dir, &catalog.cars) {
        eprintln!("launcher: added cars: {e}");
    }
    if args.is_empty() {
        println!("gvrc.de car list {}", catalog.version);
        for car in &catalog.cars {
            println!("{} ({}, {})", car.name, car.cat, car.year);
        }
        return Ok(());
    }
    let mut wanted = Vec::new();
    for word in args {
        match find(&catalog, word)[..] {
            [car] => wanted.push(car.clone()),
            [] => return Err(format!("no car named {word:?} in gvrc.de's list (run --download-cars alone for it)")),
            ref many => {
                let names: Vec<&str> = many.iter().map(|c| c.name.as_str()).collect();
                return Err(format!("{word:?} names {} cars: {}", many.len(), names.join(", ")));
            }
        }
    }
    let added = download_cars(cars_dir, &wanted, &|_| {}, &AtomicBool::new(false))?;
    for a in &added {
        match &a.result {
            Ok((name, Some(model))) => println!("{}: {name}, model Cars/{model}", a.download),
            Ok((name, None)) => println!("{}: {name}, no model", a.download),
            Err(e) => println!("{}: not added: {e}", a.download),
        }
    }
    println!("{}", cars_dir.display());
    car_packs::summary(&added).map(|line| println!("{line}"))
}

/// The Settings page's downloads: the car list and its choice, and NGP's job.
#[derive(Default)]
pub struct Downloads {
    open: bool,
    list: Option<Result<Catalog, Fail>>,
    list_job: Option<Job<Catalog>>,
    search: String,
    /// The class shown (a car's group); empty: all.
    class: String,
    /// The chosen cars' physics links.
    picked: HashSet<String>,
    /// Each download's size (0: not said), asked once the list is read.
    sizes: Option<HashMap<String, u64>>,
    size_job: Option<Job<HashMap<String, u64>>>,
    confirm_ngp: bool,
    /// The download into the player's folder (`ngp_install::KEPT`).
    ngp_job: Option<Job<()>>,
    ngp_status: Option<Result<String, String>>,
}

impl Downloads {
    /// For `--screenshot`: the NGP confirmation and the car list open with some cars chosen, from a fixed list.
    pub fn screenshot(&mut self) {
        let car = |name: &str, cat: &str, year: &str| Entry {
            name: name.into(),
            cat: cat.into(),
            year: year.into(),
            pack: name.into(),
            trans: "4WD".into(),
            physics: format!("https://gvrc.de/ngp/physics/{name} - NGP Physics.zip"),
            ..Entry::default()
        };
        let cars = vec![
            car("Audi Sport quattro GrpB", "Group B", "1985"),
            car("Citroen C2 R2 Max", "Group R2", "2011"),
            car("Ford Fiesta R5", "Group R5", "2014"),
            car("Lancia Delta HF 4WD GrpA", "Group A8", "1988"),
            car("Mitsubishi Lancer Evo IX N4", "Group N4", "2011"),
        ];
        self.picked = cars[1..3].iter().map(|c| c.physics.clone()).collect();
        self.sizes =
            Some(cars.iter().enumerate().map(|(k, c)| (c.physics.clone(), (40 + 25 * k as u64) << 20)).collect());
        self.list = Some(Ok(Catalog { version: "6.175 2025-12-07".into(), ngp: None, cars }));
        (self.open, self.confirm_ngp) = (true, true);
    }
}

impl Form {
    /// Whether NGP can be downloaded: the game folder is a Richard Burns Rally folder, and NGP 7.4.778's
    /// `physics.rbz` (none, or another version's) or its DLL is missing.
    pub fn can_download_ngp(&self) -> bool {
        let game = self.folder.game.as_deref();
        let there = game.is_some_and(|g| ngp_install::check_game_folder(g).is_ok());
        let wanted = self.ngp_revision != Some(778) || self.ngp_dll.problem(&self.data, &self.game_root()).is_some();
        there && wanted && self.child.is_none() && self.downloads.ngp_job.is_none()
    }

    /// "Download NGP 7.4.778…" asked: the confirmation shows.
    pub fn ask_download_ngp(&mut self) {
        self.downloads.ngp_status = None;
        self.downloads.confirm_ngp = true;
    }

    /// The NGP download's confirmation, progress and result line.
    pub fn ngp_download_ui(&mut self, ui: &mut egui::Ui) {
        let to = self.here().join(ngp_install::KEPT);
        if self.downloads.confirm_ngp {
            ui.label(format!(
                "Download NGP 7.4.778 from gvrc.de and keep its NGP.dll and physics.rbz in {}? ReRBR uses them; your \
                 game folder and any NGP in it are not changed.",
                to.display()
            ));
            ui.horizontal(|ui| {
                if ui.button("Download").clicked() {
                    self.downloads.confirm_ngp = false;
                    let to = to.clone();
                    self.downloads.ngp_job =
                        Some(Job::start(move |report, cancel| download_ngp_to(&to, report, cancel)));
                }
                if ui.button("Cancel").clicked() {
                    self.downloads.confirm_ngp = false;
                }
            });
        }
        if let Some(job) = &mut self.downloads.ngp_job {
            if let Some(result) = ui.horizontal_wrapped(|ui| job.ui(ui)).inner {
                self.downloads.ngp_job = None;
                self.downloads.ngp_status = Some(match result {
                    Err(e) if e.ends_with("cancelled") => Err("Cancelled; nothing was changed.".into()),
                    Err(e) => Err(e),
                    Ok(()) => {
                        self.ngp_dll.path = to.join("NGP.dll").display().to_string();
                        self.ngp_physics = to.join("physics.rbz").display().to_string();
                        self.save();
                        self.reload = true;
                        Ok(format!("NGP 7.4.778's NGP.dll and physics.rbz saved in {}.", to.display()))
                    }
                });
            }
        }
        match &self.downloads.ngp_status {
            Some(Ok(text)) => _ = ui.colored_label(GOOD, format!("✔ {text}")),
            Some(Err(text)) => _ = ui.colored_label(WARN, format!("⚠ {text}")),
            None => {}
        }
    }

    /// Opens gvrc.de's car list (the Get more cars modal).
    pub fn open_car_list(&mut self) {
        self.downloads.open = true;
    }

    /// Closes gvrc.de's car list.
    pub fn close_car_list(&mut self) {
        self.downloads.open = false;
    }

    /// The cars added, by the words of their names and packs.
    fn added_words(&self) -> HashSet<String> {
        let cars = self.adding.cars.iter();
        cars.flat_map(|(name, pack)| [car_packs::words(name), car_packs::words(pack)]).collect()
    }

    /// The Get more cars modal's subtitle: where the list is from and its version.
    pub fn cars_subtitle(&self) -> String {
        match &self.downloads.list {
            Some(Ok(catalog)) => format!(
                "NGP cars from gvrc.de's car list · version {} · {} available · {} added",
                catalog.version,
                catalog.cars.len(),
                self.adding.cars.len()
            ),
            _ => "NGP cars from gvrc.de's car list".into(),
        }
    }

    /// What the modal's footer shows and offers, from the list as it is.
    pub fn cars_footer(&self) -> CarsFooter {
        let Some(Ok(catalog)) = &self.downloads.list else { return CarsFooter::default() };
        let d = &self.downloads;
        let added = self.added_words();
        let missing: Vec<Entry> =
            catalog.cars.iter().filter(|c| !added.contains(&car_packs::words(&c.name))).cloned().collect();
        // Only what the table shows can be chosen: a car added since it was ticked drops out.
        let wanted: Vec<Entry> = missing.iter().filter(|c| d.picked.contains(&c.physics)).cloned().collect();
        CarsFooter {
            loaded: true,
            busy: !self.can_add_cars(),
            size: size_text(&wanted, d.sizes.as_ref()),
            missing_size: size_text(&missing, d.sizes.as_ref()),
            all_missing: missing.len() == catalog.cars.len(),
            wanted,
            missing,
        }
    }

    /// Does what the footer asked.
    pub fn apply_cars_action(&mut self, action: CarsAction) {
        match action {
            CarsAction::Download(wanted) => {
                self.downloads.picked.clear();
                let dir = self.cars_dir();
                self.start_adding(move |report, cancel| download_cars(&dir, &wanted, report, cancel));
            }
            CarsAction::Clear => self.downloads.picked.clear(),
        }
    }

    /// The Get more cars modal's body: the list loading or its error, the progress and result of a download, and the
    /// search, class and the table of the cars not added. `import` is set when its Import link is clicked.
    pub fn cars_body(&mut self, ui: &mut egui::Ui, import: &mut bool) {
        if self.downloads.list.is_none() && self.downloads.list_job.is_none() {
            self.downloads.list_job = Some(list_job());
        }
        if let Some(job) = &mut self.downloads.list_job {
            if let Some(result) = ui.horizontal_wrapped(|ui| job.ui(ui)).inner {
                self.downloads.list_job = None;
                if let Ok(catalog) = &result {
                    let cars = catalog.cars.clone();
                    self.downloads.size_job = Some(Job::start(move |_, cancel| Ok(sizes_of(&cars, cancel))));
                }
                self.downloads.list = Some(result);
                // Added cars that still lack their group or drive train take them from the finished list.
                let backfilled = match &self.downloads.list {
                    Some(Ok(catalog)) => car_packs::backfill(&self.cars_dir(), &catalog.cars),
                    _ => Ok(false),
                };
                match backfilled {
                    Ok(true) => self.rescan_cars(),
                    Ok(false) => {}
                    Err(e) => self.adding.status = Some(Err(e)),
                }
            }
        }
        if let Some(result) = self.downloads.size_job.as_mut().map(Job::result) {
            match result {
                Some(sizes) => (self.downloads.sizes, self.downloads.size_job) = (sizes.ok(), None),
                None => ui.ctx().request_repaint_after(Duration::from_millis(200)),
            }
        }
        if self.adding_running().is_some() {
            ui.horizontal_wrapped(|ui| self.adding_job_ui(ui));
        }
        self.adding_status_ui(ui);
        if self.child.is_some() {
            ui.label(egui::RichText::new("⚠ Close the drive to download cars.").color(WARN));
        }
        let added = self.added_words();
        let mut reload = false;
        let Downloads { list, search, class, picked, sizes, .. } = &mut self.downloads;
        match list {
            None => {}
            Some(Err(e)) => {
                ui.add(egui::Label::new(egui::RichText::new(format!("⚠ {e}")).color(WARN)).wrap());
                ui.add_space(6.0);
                reload = ui.button("Try again").clicked();
            }
            Some(Ok(catalog)) => {
                let classes: std::collections::BTreeSet<String> =
                    catalog.cars.iter().filter_map(Entry::group).collect();
                let shown = filter_cars(catalog, search, class, &added);
                let available = catalog.cars.iter().filter(|c| !added.contains(&car_packs::words(&c.name))).count();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let edit = egui::TextEdit::singleline(search).hint_text("Search car or class");
                    ui.add_sized([210.0, 28.0], edit.margin(egui::Margin::symmetric(11, 6)));
                    let all = "All classes";
                    egui::ComboBox::from_id_salt("gvrc class")
                        .selected_text(if class.is_empty() { all } else { class.as_str() })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(class, String::new(), all);
                            for name in &classes {
                                ui.selectable_value(class, name.clone(), name);
                            }
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        reload = kit::link(ui, "Reload").clicked();
                        let count = format!("{} of {available} cars", shown.len());
                        ui.label(egui::RichText::new(count).size(12.0).color(TEXT_2));
                    });
                });
                ui.add_space(10.0);
                cars_table(ui, &shown, picked, sizes.as_ref());
                ui.add_space(8.0);
                kit::hint(
                    ui,
                    "ReRBR downloads each car's physics, model and sounds into your cars folder; the game folder \
                     isn't changed. Have the files already?",
                );
                *import = kit::link(ui, "Import them").clicked();
            }
        }
        if reload {
            (self.downloads.list, self.downloads.sizes, self.downloads.size_job) = (None, None, None);
        }
    }
}

/// What the Get more cars modal's footer shows, from the list ([`Form::cars_footer`]).
#[derive(Default)]
pub struct CarsFooter {
    loaded: bool,
    busy: bool,
    size: Option<String>,
    missing_size: Option<String>,
    all_missing: bool,
    wanted: Vec<Entry>,
    missing: Vec<Entry>,
}

/// What the footer asked for.
pub enum CarsAction {
    Download(Vec<Entry>),
    Clear,
}

/// The footer, right to left: the primary Download, Get all not added, and on the left the choice's count and size
/// with Clear.
pub fn cars_footer_ui(foot: &CarsFooter, ui: &mut egui::Ui) -> Option<CarsAction> {
    if !foot.loaded {
        return None;
    }
    let mut action = None;
    let n = foot.wanted.len();
    let label = format!("Download {n} car{}", if n == 1 { "" } else { "s" });
    if ui.add_enabled(!foot.busy && n > 0, kit::primary_button(&label, None)).clicked() {
        action = Some(CarsAction::Download(foot.wanted.clone()));
    }
    if !foot.missing.is_empty() {
        let m = foot.missing.len();
        let mut text = if foot.all_missing { format!("Get all {m}") } else { format!("Get all {m} not added") };
        if let Some(size) = &foot.missing_size {
            text += &format!(" · {size}");
        }
        if ui.add_enabled(!foot.busy, egui::Button::new(text)).clicked() {
            action = Some(CarsAction::Download(foot.missing.clone()));
        }
    }
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let mut text = format!("{n} selected");
        if let Some(size) = &foot.size {
            text += &format!(" · {size}");
        }
        ui.label(egui::RichText::new(text).color(TEXT_2));
        if n > 0 && kit::link(ui, "Clear").clicked() {
            action = Some(CarsAction::Clear);
        }
    });
    action
}

/// The cars not added that the search and the class leave.
fn filter_cars<'a>(catalog: &'a Catalog, search: &str, class: &str, added: &HashSet<String>) -> Vec<&'a Entry> {
    let word = search.to_lowercase();
    let has_word = |c: &Entry| {
        word.is_empty()
            || format!("{} {} {}", c.name, c.cat, c.group().unwrap_or_default()).to_lowercase().contains(&word)
    };
    let of_class = |c: &Entry| class.is_empty() || c.group().as_deref() == Some(class);
    let hidden = |c: &Entry| added.contains(&car_packs::words(&c.name));
    catalog.cars.iter().filter(|c| has_word(c) && of_class(c) && !hidden(c)).collect()
}

/// The table's column starts for a width: car, class, year, drive, and the right edge of size.
fn columns(width: f32) -> [f32; 5] {
    [40.0, width * 0.46, width * 0.61, width * 0.70, width * 0.88]
}

/// The table: a header, then a row per car in a scroll area of its own. Ticked rows are tinted.
fn cars_table(ui: &mut egui::Ui, shown: &[&Entry], picked: &mut HashSet<String>, sizes: Option<&HashMap<String, u64>>) {
    let width = ui.available_width();
    let cols = columns(width);
    let (head, _) = ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::hover());
    let painter = ui.painter();
    let at = |x: f32| egui::pos2(head.left() + x, head.center().y);
    for (x, text, align) in [
        (cols[0], "Car", egui::Align2::LEFT_CENTER),
        (cols[1], "Class", egui::Align2::LEFT_CENTER),
        (cols[2], "Year", egui::Align2::LEFT_CENTER),
        (cols[3], "Drive", egui::Align2::LEFT_CENTER),
        (cols[4], "Size", egui::Align2::RIGHT_CENTER),
    ] {
        let font = egui::FontId::new(11.5, medium());
        painter.text(at(x), align, text, font, TEXT_3);
    }
    egui::ScrollArea::vertical().id_salt("gvrc cars").max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for car in shown {
            let on = picked.contains(&car.physics);
            let size = size_text(std::slice::from_ref(car), sizes).unwrap_or_default();
            if car_row(ui, car, cols, on, &size).clicked() {
                match on {
                    true => picked.remove(&car.physics),
                    false => picked.insert(car.physics.clone()),
                };
            }
        }
        if shown.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("No cars match.").color(TEXT_2));
        }
    });
}

/// One car's row (32 px): tick box, name, class, year, drive and size.
fn car_row(ui: &mut egui::Ui, car: &Entry, cols: [f32; 5], on: bool, size: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click());
    let painter = ui.painter();
    if on {
        painter.rect_filled(rect, 6, ROW_TICKED);
    } else if response.hovered() {
        painter.rect_filled(rect, 6, white(8));
    }
    let box_rect =
        egui::Rect::from_center_size(egui::pos2(rect.left() + 20.0, rect.center().y), egui::Vec2::splat(16.0));
    if on {
        painter.rect_filled(box_rect, 4, ACCENT);
        let c = box_rect.center();
        let points = vec![c + egui::vec2(-4.0, 0.0), c + egui::vec2(-1.0, 3.0), c + egui::vec2(4.5, -3.0)];
        painter.add(egui::Shape::line(points, egui::Stroke::new(1.8, egui::Color32::WHITE)));
    } else {
        painter.rect_stroke(box_rect, 4, egui::Stroke::new(1.5, CHECK_BORDER), egui::StrokeKind::Inside);
    }
    let at = |x: f32| egui::pos2(rect.left() + x, rect.center().y);
    let name_clip = egui::Rect::from_min_max(rect.left_top(), egui::pos2(rect.left() + cols[1] - 8.0, rect.bottom()));
    painter.with_clip_rect(name_clip).text(
        at(cols[0]),
        egui::Align2::LEFT_CENTER,
        &car.name,
        egui::FontId::proportional(13.0),
        TEXT_LIST,
    );
    let dim = TEXT_2;
    let group = car.group().unwrap_or_default();
    painter.text(at(cols[1]), egui::Align2::LEFT_CENTER, group, egui::FontId::proportional(12.5), dim);
    let year = car.year().map_or_else(String::new, |y| y.to_string());
    painter.text(at(cols[2]), egui::Align2::LEFT_CENTER, year, mono(11.5), dim);
    painter.text(at(cols[3]), egui::Align2::LEFT_CENTER, car.drive_train().unwrap_or_default(), mono(11.5), dim);
    painter.text(at(cols[4]), egui::Align2::RIGHT_CENTER, size, mono(11.5), dim);
    response.on_hover_text("Tick to download").on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::atomic::Ordering;

    const LIST: &str = "\u{feff}; File: carList.ini\n; Version: 6.175 2025-12-07\n\n[Plugins]\n\
        plugin_physics_url=https://gvrc.de/ngp/plugin/RBR_NGP-7.4.778.507.7z\n\n\
        [Car_1]\nname=Citroen Xsara (0)\ncat=Original\nlink_physics=https://gvrc.de/ngp/original/physics/c_xsara.zip\nlink_model=\n\n\
        [Car_10]\nname=Audi Sport quattro GrpB\ncat=Group B\nyear=1985\nphysics=Audi Sport quattro GrpB\ntrans=4WD\n\
        link_physics=https://gvrc.de/ngp/physics/Audi%20Sport%20quattro%20GrpB%20-%20NGP%20Physics.zip\n\
        link_model=https://gvrc.de/ngp/models/Audi%20Sport%20quattro%20GrpB%20-%20Model.7z\n\
        link_banks=https://gvrc.de/ngp/sounds/Audi%20Sport%20quattro%20GrpB%20-%20banks.7z\n\n\
        ;[Car_11]\n;name=Commented out\n\n\
        [Car_12]\nname=Another host\ncat=Group B\nlink_physics=https://example.com/Another%20-%20NGP%20Physics.zip\n\n\
        [Car_13]\nname=No model link name\ncat=Group A8\nlink_physics=https://gvrc.de/ngp/physics/X%20-%20NGP%20Physics.zip\n\
        link_model=https://gvrc.de/ngp/models/X.zip\n\n\
        [Car_14]\nname=Ford Fiesta R5\ncat=Group R5\nyear=2014\n\
        link_physics=https://gvrc.de/ngp/physics/Ford%20Fiesta%20R5%20-%20NGP%20Physics.zip\n\n[Misc]\naudio_fmod_url=\n";

    /// A model two cars share counts once; a download of unknown size hides the label.
    #[test]
    fn the_download_size_counts_each_file_once() {
        let car = |physics: &str, model: &str| Entry {
            name: physics.into(),
            pack: physics.into(),
            physics: physics.into(),
            model: Some(model.into()),
            ..Entry::default()
        };
        let cars = [car("a", "m"), car("b", "m")];
        let sizes: HashMap<String, u64> =
            [("a", 1 << 20), ("b", 1 << 20), ("m", 1534 << 20)].into_iter().map(|(u, n)| (u.to_owned(), n)).collect();
        assert_eq!(size_text(&cars, Some(&sizes)).as_deref(), Some("1.5 GB"));
        assert_eq!(
            size_text(&cars[..1], Some(&HashMap::from([("a".into(), 5 << 20), ("m".into(), 1)]))).as_deref(),
            Some("5 MB")
        );
        assert_eq!(size_text(&cars, Some(&HashMap::from([("a".into(), 1)]))), None);
        assert_eq!(size_text(&cars, None), None);
    }

    #[test]
    fn the_list_gives_ngp_and_the_downloadable_cars() {
        let catalog = parse(LIST);
        assert_eq!(catalog.version, "6.175 2025-12-07");
        assert_eq!(catalog.ngp.as_deref(), Some("https://gvrc.de/ngp/plugin/RBR_NGP-7.4.778.507.7z"));
        let names: Vec<&str> = catalog.cars.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Audi Sport quattro GrpB", "Ford Fiesta R5"]);
        assert_eq!(catalog.cars[0].year, "1985");
        assert_eq!(catalog.cars[0].pack, "Audi Sport quattro GrpB");
        assert_eq!(catalog.cars[0].trans, "4WD");
        assert_eq!((catalog.cars[1].pack.as_str(), catalog.cars[1].trans.as_str()), ("", ""));
        assert_eq!(
            catalog.cars[0].model.as_deref(),
            Some("https://gvrc.de/ngp/models/Audi%20Sport%20quattro%20GrpB%20-%20Model.7z")
        );
        assert_eq!(catalog.cars[1].model, None);
        assert_eq!(
            catalog.cars[0].banks.as_deref(),
            Some("https://gvrc.de/ngp/sounds/Audi%20Sport%20quattro%20GrpB%20-%20banks.7z")
        );
        assert_eq!(catalog.cars[1].banks, None);
        assert_eq!(find(&catalog, "fiesta").len(), 1);
        assert_eq!(find(&catalog, "ford fiesta r5").len(), 1);
        assert_eq!(find(&catalog, "s").len(), 2);
    }

    #[test]
    fn links_are_gvrcs_and_one_file() {
        assert!(is_gvrc("https://gvrc.de/ngp/x.zip"));
        assert!(!is_gvrc("https://gvrc.de.evil.com/x.zip") && !is_gvrc("http://gvrc.de/x.zip"));
        assert!(!parse("[Plugins]\nplugin_physics_url=http://gvrc.de/x.7z\n").ngp.is_some());
        let name = |u: &str| file_name(u);
        assert_eq!(
            name("https://gvrc.de/ngp/physics/Honda%20Civic%20Type%20R%28FN2%29%20R3%20-%20NGP%20Physics.zip")
                .as_deref(),
            Some("Honda Civic Type R(FN2) R3 - NGP Physics.zip")
        );
        assert_eq!(name("https://gvrc.de/a/%2e%2e%2fx.zip"), None);
        assert_eq!(name("https://gvrc.de/a/..%5Cx"), None);
        assert_eq!(name("https://gvrc.de/a/%zz"), None);
        assert_eq!(name("https://gvrc.de/a/"), None);
        assert_eq!(name("https://gvrc.de/a/.hidden"), None);
    }

    /// Serves `routes` (path, body) on a local port until the process ends; 404 for others.
    fn serve(routes: Vec<(&'static str, Vec<u8>)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let routes = routes.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(&stream);
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    loop {
                        let mut header = String::new();
                        if reader.read_line(&mut header).unwrap() <= 2 {
                            break;
                        }
                    }
                    let mut parts = line.split_whitespace();
                    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                    let mut out = &stream;
                    match routes.iter().find(|(p, _)| *p == path) {
                        Some((_, body)) => {
                            let _ = write!(
                                out,
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            );
                            if method != "HEAD" {
                                let _ = out.write_all(body);
                            }
                        }
                        None => {
                            _ = write!(out, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        }
                    }
                });
            }
        });
        base
    }

    #[test]
    fn files_are_fetched_with_their_size_and_progress() {
        let body: Vec<u8> = (0..300_000u32).map(|n| n as u8).collect();
        let base = serve(vec![("/a.bin", body.clone())]);
        let dir = crate::test_dir("gvrc-fetch");
        let (agent, cancel) = (agent(), AtomicBool::new(false));
        let url = format!("{base}/a.bin");
        assert_eq!(size_of(&agent, &url), 300_000);
        assert_eq!(size_of(&agent, &format!("{base}/none")), 0);
        let seen = std::sync::Mutex::new(Vec::new());
        let report = |p: &Progress| seen.lock().unwrap().push(p.done);
        let mut count = Counter { step: "x", done: 0, total: 300_000, report: &report, cancel: &cancel };
        fetch(&agent, &url, &dir.join("sub/a.bin"), &mut count).unwrap();
        assert_eq!(fs::read(dir.join("sub/a.bin")).unwrap(), body);
        assert_eq!(*seen.lock().unwrap().last().unwrap(), 300_000);
        let missing = fetch(&agent, &format!("{base}/none"), &dir.join("n"), &mut count).unwrap_err();
        assert_eq!(missing, "none: http status: 404");
        cancel.store(true, Ordering::Relaxed);
        let stopped = fetch(&agent, &url, &dir.join("b.bin"), &mut count).unwrap_err();
        assert!(stopped.ends_with("cancelled"), "{stopped}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_car_download_is_reported_and_leaves_no_temp_folder() {
        let base = serve(vec![]);
        let dir = crate::test_dir("gvrc-cars");
        let car = Entry {
            name: "Ghost Car".into(),
            cat: "Group B".into(),
            year: "1985".into(),
            pack: "Ghost Car".into(),
            trans: "4WD".into(),
            physics: format!("{base}/Ghost%20Car%20-%20NGP%20Physics.zip"),
            ..Entry::default()
        };
        // The links are not gvrc.de's: `file_name` is empty, the download still fails by its 404.
        let added = download_cars(&dir, std::slice::from_ref(&car), &|_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(added.len(), 1);
        assert!(added[0].result.as_ref().unwrap_err().contains("404"), "{added:?}");
        assert!(car_packs::summary(&added).unwrap_err().starts_with("Nothing added."));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        let stopped = download_cars(&dir, &[car], &|_| {}, &AtomicBool::new(true));
        assert_eq!(stopped.unwrap_err(), "cancelled");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// gvrc.de's categories as RSF names the groups: `RallyN` takes a space, a bare `A5`..`A8` takes `Group`.
    #[test]
    fn groups_are_named_as_rsf_names_them() {
        let entry = |cat: &str, trans: &str| Entry {
            name: "x".into(),
            cat: cat.into(),
            pack: "x".into(),
            trans: trans.into(),
            ..Entry::default()
        };
        let group = |cat: &str| entry(cat, "").group();
        for (cat, want) in [
            ("Rally2", Some("Rally 2")),
            ("Rally 2", Some("Rally 2")),
            ("A8", Some("Group A8")),
            ("Group A8", Some("Group A8")),
            ("WRC 1.6", Some("WRC 1.6")),
            ("  Group N4  ", Some("Group N4")),
            ("Rally", Some("Rally")),
            ("", None),
        ] {
            assert_eq!(group(cat).as_deref(), want, "{cat}");
        }
        assert_eq!(entry("", " FWD ").drive_train().as_deref(), Some("FWD"));
        assert_eq!(entry("", "").drive_train(), None);
    }
}
