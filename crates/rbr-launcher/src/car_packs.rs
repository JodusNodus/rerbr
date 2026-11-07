//! Adding gvrc.de's NGP car packs, so that the cars drive without RallySimFans: the Settings page's "Download cars…"
//! downloads the packs and adds them through [`add_downloads`]: every `<Car> - NGP Physics.zip` with its
//! `<Model> - Model.7z` and its `<Car> - banks.7z`.
//!
//! The cars go into `cars/` in the player's folder (`home`):
//! - `cars/rsfdata/cars/<pack>/`: the physics pack, unpacked (the zip's `<pack>/` folder, else the download's name
//!   with `_` for spaces);
//! - `cars/Cars/<model>/`: the model, the `.7z`'s folder holding a `.sgc`;
//! - `cars/AudioFMOD/`: the car's FMOD sound banks, the `.7z`'s folder holding a `.bank`;
//! - `cars/cars.ini`: the list, one section per pack folder (the name, the revision, the bank, the model, the group
//!   and drive train; ids of our own, so the launcher does not match them with RSF's ghosts or gearboxes).
//!
//! A pack's model is the download, or one added before, whose name starts the car's download name, the pack's title
//! or its "3D model" credit, the longest such (`Mitsubishi Lancer Evo IX - Model.7z` for the Evo IX N4 and R4). A
//! pack without one is added with RSF's stand-in for a missing model, a retail car's ([`STAND_IN`]). Adding a pack
//! again replaces it; models no car uses any more are removed. Nothing outside `cars/` is written; `gvrc` downloads
//! the packs and adds them through [`add_downloads`].

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rbr_formats::car_pack;
use rbr_formats::rsf;

use crate::archive::safe_folder;
use crate::gvrc::Entry;
use crate::job::{Counter, Job, Report, Temp};
use crate::pages::{GOOD, WARN};
use crate::Form;

/// The folder in the player's folder with the added cars.
pub const DIR: &str = "cars";
/// The download names' ends: the physics pack, the model and the sound banks.
pub(crate) const PHYSICS: &str = " - NGP Physics.zip";
pub(crate) const MODEL: &str = " - Model.7z";
pub(crate) const BANKS: &str = " - banks.7z";
/// The folder in `cars/` with the sound banks, as RSF's.
const AUDIO: &str = "AudioFMOD";
/// The retail car whose model a pack without one is drawn with (ours: RSF names one per model; the slot's own).
pub const STAND_IN: u32 = car_pack::SLOT;
/// [`STAND_IN`]'s name, for the player.
const STAND_IN_NAME: &str = "Subaru Impreza 2003";

type Fail = String;

/// A physics pack download and the model and sound bank downloads it goes with, with the group and drive train
/// gvrc.de's list names for the car (`None` keeps the added car's own on a re-add).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub physics: PathBuf,
    pub model: Option<PathBuf>,
    pub banks: Option<PathBuf>,
    pub group: Option<String>,
    pub drive_train: Option<String>,
}

/// An added pack: its car's name, and its model folder or why it has none; or why it was not added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    pub download: String,
    pub result: Result<(String, Option<String>), Fail>,
}

/// `name` without the end `suffix`, ignoring case.
pub(crate) fn strip_end<'a>(name: &'a str, suffix: &str) -> Option<&'a str> {
    let cut = name.len().checked_sub(suffix.len())?;
    (name.is_char_boundary(cut) && name[cut..].eq_ignore_ascii_case(suffix)).then(|| &name[..cut])
}

/// `text` lower case, `_` as spaces, the spaces single.
pub(crate) fn words(text: &str) -> String {
    text.to_lowercase().replace('_', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The model of `models` (their names) whose name starts one of `names` (the car's download name, title, credit) as
/// whole words: the longest such.
pub fn pick_model(names: &[&str], models: &[&str]) -> Option<usize> {
    let names: Vec<String> = names.iter().map(|n| words(n)).filter(|n| !n.is_empty()).collect();
    let starts = |model: &str| {
        let model = words(model);
        !model.is_empty()
            && names.iter().any(|n| n.strip_prefix(&model).is_some_and(|rest| rest.is_empty() || rest.starts_with(' ')))
    };
    (0..models.len()).filter(|&k| starts(models[k])).max_by_key(|&k| words(models[k]).len())
}

/// A physics pack download: the zip, its pack folder's name and the entries below it (archive path, path below
/// it).
struct PhysicsZip {
    path: PathBuf,
    folder: String,
    entries: Vec<(Vec<String>, Vec<String>)>,
    size: u64,
}

impl PhysicsZip {
    /// The zip at `path`: its files at its top (the pack folder is then the download's name, `_` for spaces), or in
    /// the `<pack>/` folder holding `common.lsp` (as RSF's `rsfdata/cars/<pack>.zip`). Folders beside that one are
    /// left out: gvrc.de's Fiesta Rally2 and i20 N Rally2 add a `<pack> - rev1/` holding only an old about file.
    fn open(path: &Path) -> Result<PhysicsZip, Fail> {
        let error = |e: String| format!("{}: {e}", path.display());
        let mut files = Vec::new();
        for entry in crate::archive::zip_entries(path).map_err(error)? {
            let parts = entry.parts.ok_or_else(|| error(format!("a bad entry path {}", entry.name)))?;
            if !entry.dir {
                files.push((parts, entry.size));
            }
        }
        let common = |p: &[String], depth: usize| p.len() == depth && p[depth - 1].eq_ignore_ascii_case("common.lsp");
        let nested = match files.iter().any(|(p, _)| common(p, 1)) {
            true => None,
            false => files.iter().find(|(p, _)| common(p, 2)).map(|(p, _)| p[0].clone()),
        };
        let download = path.file_name().and_then(|n| strip_end(n.to_str()?, PHYSICS)).unwrap_or_default();
        let folder = nested.clone().unwrap_or_else(|| download.replace(' ', "_"));
        let folder = safe_folder(&folder).ok_or_else(|| error(format!("a bad pack folder name {folder}")))?;
        let kept: Vec<_> = match &nested {
            Some(top) => files.into_iter().filter(|(p, _)| p.len() > 1 && p[0] == *top).collect(),
            None => files,
        };
        let size = kept.iter().map(|(_, n)| n).sum();
        let below = if nested.is_some() { 1 } else { 0 };
        let entries: Vec<_> = kept.into_iter().map(|(p, _)| (p.clone(), p[below..].to_vec())).collect();
        if !entries.iter().any(|(_, p)| common(p, 1)) {
            return Err(error("no common.lsp: is it an NGP physics pack?".into()));
        }
        Ok(PhysicsZip { path: path.to_owned(), folder, entries, size })
    }

    /// Unpacks the pack into `dest`.
    fn extract(&self, dest: &Path, count: &mut Counter) -> Result<(), Fail> {
        crate::archive::extract_zip(&self.path, dest, &self.entries, &mut |n| count.add(n))
    }
}

/// A `.7z` download (a model or sound banks): the folder holding its main file (the path of it in the `.7z`), and
/// that file's name without its end.
struct Archive {
    path: PathBuf,
    prefix: Vec<String>,
    filename: String,
    size: u64,
}

impl Archive {
    /// A model: the folder holding a `.sgc`, below the top.
    fn model(path: &Path) -> Result<Archive, Fail> {
        Archive::open(path, ".sgc", 2, "no Cars/<model>/<model>.sgc in it")
    }

    /// Sound banks: the folder holding a `.bank` (the strings banks beside them are not one).
    fn banks(path: &Path) -> Result<Archive, Fail> {
        Archive::open(path, ".bank", 1, "no AudioFMOD/<car>.bank in it")
    }

    /// The `.7z` at `path` whose main file ends `end` and lies `depth` or more names deep: the one nearest the top,
    /// then by name.
    fn open(path: &Path, end: &str, depth: usize, missing: &str) -> Result<Archive, Fail> {
        let error = |e: String| format!("{}: {e}", path.display());
        let listed = crate::archive::seven_entries(path).map_err(error)?;
        let main = |name: &str| {
            let name = name.to_lowercase();
            name.ends_with(end) && !name.ends_with(".strings.bank")
        };
        let mut mains: Vec<&Vec<String>> = listed
            .iter()
            .filter(|f| !f.dir && main(&f.name))
            .filter_map(|f| f.parts.as_ref())
            .filter(|p| p.len() >= depth)
            .collect();
        mains.sort_by_key(|p| (p.len(), p.join("/")));
        let first = mains.first().ok_or_else(|| error(missing.into()))?;
        let prefix = first[..first.len() - 1].to_vec();
        let name = &first[first.len() - 1];
        let filename = name[..name.len() - end.len()].to_owned();
        let size = listed
            .iter()
            .filter(|f| !f.dir && f.parts.as_ref().is_some_and(|p| within(p, &prefix)))
            .map(|f| f.size)
            .sum();
        Ok(Archive { path: path.to_owned(), prefix, filename, size })
    }

    /// The model folder's name.
    fn folder(&self) -> Result<String, Fail> {
        let last = self.prefix.last().map(String::as_str).unwrap_or_default();
        safe_folder(last).ok_or(format!("{}: a bad model folder name {last}", self.path.display()))
    }

    /// Unpacks the folder into `dest`.
    fn extract(&self, dest: &Path, count: &mut Counter) -> Result<(), Fail> {
        let listed = crate::archive::seven_entries(&self.path).map_err(|e| format!("{}: {e}", self.path.display()))?;
        let wanted: Vec<(Vec<String>, Vec<String>)> = listed
            .into_iter()
            .filter(|e| !e.dir)
            .filter_map(|e| e.parts)
            .filter(|parts| within(parts, &self.prefix))
            .map(|parts| (parts.clone(), parts[self.prefix.len()..].to_vec()))
            .collect();
        crate::archive::extract_seven(&self.path, dest, &wanted, &mut |n| count.add(n))
    }
}

/// Whether `path` lies below the folder `prefix` (its names ignoring case).
fn within(path: &[String], prefix: &[String]) -> bool {
    path.len() > prefix.len() && path.iter().zip(prefix).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// An added car: its name, its physics pack folder, and what it drives with. Stored in `cars/cars.ini`, one
/// section per pack folder; the id stays with the pack, so its saved keys (`rsf-<id>`) survive rewrites and re-adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCar {
    pub id: u32,
    pub name: String,
    pub pack: String,
    pub group: Option<String>,
    pub drive_train: Option<String>,
    pub revision: String,
    /// The FMOD bank's name (`AudioFMOD/<audio>.bank`), when it has one.
    pub audio: Option<String>,
    pub model: StoredModel,
}

/// What an added car is drawn with: its model folder and file, or the retail slot standing in for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredModel {
    Files { folder: String, file: String, name: String },
    StandIn { slot: u32 },
}

/// The added cars' list in `cars` (none without one): `cars.ini`, migrating the old RSF-format JSON cache
/// (`cars/rsfdata/cache/*.json`, ours, from when the list was written for RSF's reader) when it is there instead.
/// The migration keeps the ids and deletes the JSON files.
pub fn list(cars: &Path) -> Vec<StoredCar> {
    migrate(cars);
    read_ini(cars)
}

/// `cars.ini`'s cars, in file order; bad sections are left out with the reason on stderr.
fn read_ini(cars_dir: &Path) -> Vec<StoredCar> {
    let path = cars_dir.join("cars.ini");
    let Ok(text) = fs::read_to_string(&path) else { return Vec::new() };
    let Ok(sections) = rbr_formats::ini::parse(&text) else { return Vec::new() };
    let mut cars = Vec::new();
    for section in sections {
        match stored_car(&section) {
            Ok(car) => cars.push(car),
            Err(e) => eprintln!("launcher: {} [{}]: {e}", path.display(), section.name),
        }
    }
    cars
}

/// The car of a `cars.ini` section (named by its pack folder).
fn stored_car(section: &rbr_formats::ini::Section) -> Result<StoredCar, String> {
    let pack = section.name.clone();
    let value = |key: &str| section.get(key).map(str::to_owned);
    let id: u32 = section.get("id").ok_or("no id")?.parse().map_err(|_| "bad id")?;
    let model = match (value("model"), value("standin")) {
        (Some(files), _) => {
            let (folder, file) = files.split_once('/').ok_or("bad model: want folder/file")?;
            if folder.is_empty() || file.is_empty() {
                return Err("bad model: want folder/file".into());
            }
            StoredModel::Files {
                folder: folder.to_owned(),
                file: file.to_owned(),
                name: value("model_name").unwrap_or_default(),
            }
        }
        (None, Some(slot)) => {
            StoredModel::StandIn { slot: slot.parse().map_err(|_| "bad standin: want a retail slot")? }
        }
        (None, None) => return Err("no model or standin".into()),
    };
    Ok(StoredCar {
        id,
        name: value("name").filter(|n| !n.is_empty()).unwrap_or_else(|| pack.clone()),
        pack,
        group: value("group").filter(|g| !g.is_empty()),
        drive_train: value("drive").filter(|d| !d.is_empty()),
        revision: value("revision").unwrap_or_default(),
        audio: value("bank").filter(|b| !b.is_empty()),
        model,
    })
}

/// Writes the added cars to `cars/cars.ini` (one section per pack folder), dropping the model folders no car
/// uses. Each car keeps its id, so its saved keys survive a rewrite.
pub fn write_cars(cars_dir: &Path, cars: &[StoredCar]) -> Result<(), Fail> {
    let used: Vec<&str> = cars
        .iter()
        .filter_map(|c| match &c.model {
            StoredModel::Files { folder, .. } => Some(folder.as_str()),
            StoredModel::StandIn { .. } => None,
        })
        .collect();
    for dir in model_folders(cars_dir) {
        if !used.iter().any(|u| u.eq_ignore_ascii_case(&dir)) {
            let _ = fs::remove_dir_all(cars_dir.join("Cars").join(dir));
        }
    }
    let mut text = String::from("; ReRBR's added cars: one section per physics pack folder in rsfdata/cars/.\n");
    for car in cars {
        text.push_str(&format!("\n[{}]\n", car.pack));
        let mut row = |key: &str, value: &str| text.push_str(&format!("{key} = {value}\n"));
        row("name", &car.name);
        row("id", &car.id.to_string());
        if let Some(group) = car.group.as_deref() {
            row("group", group);
        }
        if let Some(drive) = car.drive_train.as_deref() {
            row("drive", drive);
        }
        if !car.revision.is_empty() {
            row("revision", &car.revision);
        }
        if let Some(audio) = car.audio.as_deref() {
            row("bank", audio);
        }
        match &car.model {
            StoredModel::Files { folder, file, name } => {
                row("model", &format!("{folder}/{file}"));
                if !name.is_empty() {
                    row("model_name", name);
                }
            }
            StoredModel::StandIn { slot } => row("standin", &slot.to_string()),
        }
    }
    let (temp, path) = (cars_dir.join(".cars.ini.new"), cars_dir.join("cars.ini"));
    fs::create_dir_all(cars_dir).map_err(|e| format!("{}: {e}", cars_dir.display()))?;
    fs::write(&temp, text).and_then(|()| fs::rename(&temp, &path)).map_err(|e| format!("{}: {e}", path.display()))
}

/// The model folders in `cars/Cars/`.
fn model_folders(cars_dir: &Path) -> Vec<String> {
    fs::read_dir(cars_dir.join("Cars"))
        .map(|entries| {
            entries.filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|n| !n.starts_with('.')).collect()
        })
        .unwrap_or_default()
}

/// Converts the old RSF-format JSON cache (`cars/rsfdata/cache/cars.json`, `carmodels.json`, `cargroups.json` and
/// `cars_data.json`, ours, from when the added cars were listed for RSF's reader) to `cars/cars.ini` once, keeping
/// the ids, then deletes the JSON files. Nothing happens when `cars.ini` is there or the cache is not.
fn migrate(cars_dir: &Path) {
    if cars_dir.join("cars.ini").exists() || !cars_dir.join("rsfdata").join("cache").join("cars.json").exists() {
        return;
    }
    let Ok(install) = rsf::Install::open(cars_dir) else { return };
    let stored: Vec<StoredCar> = install
        .cars
        .iter()
        .map(|c| {
            let model = match install.models.iter().find(|m| m.id == c.model) {
                Some(m) => {
                    StoredModel::Files { folder: m.path.clone(), file: m.filename.clone(), name: m.name.clone() }
                }
                None => StoredModel::StandIn { slot: STAND_IN },
            };
            StoredCar {
                id: c.id,
                name: c.name.clone(),
                pack: c.pack.clone(),
                group: c.group.clone(),
                drive_train: c.drive_train.clone(),
                revision: c.revision.clone(),
                audio: c.audio.clone(),
                model,
            }
        })
        .collect();
    if write_cars(cars_dir, &stored).is_err() {
        return;
    }
    for name in ["cargroups.json", "carmodels.json", "cars.json", "cars_data.json"] {
        let _ = fs::remove_file(cars_dir.join("rsfdata").join("cache").join(name));
    }
}

/// Fills in the group and drive train gvrc.de's list names for added cars that lack them: the cars whose pack
/// folder is an entry's pack folder (ignoring case) take its normalized category and its drive. Rewrites the list
/// only when a car gained something, and says whether it did.
pub fn backfill(cars_dir: &Path, entries: &[Entry]) -> Result<bool, Fail> {
    let mut cars = list(cars_dir);
    let mut changed = false;
    for car in &mut cars {
        let Some(entry) = entries.iter().find(|e| e.pack.eq_ignore_ascii_case(&car.pack)) else {
            continue;
        };
        for (field, value) in [(&mut car.group, entry.group()), (&mut car.drive_train, entry.drive_train())] {
            if field.as_deref().is_none_or(str::is_empty) && value.is_some() {
                *field = value;
                changed = true;
            }
        }
    }
    if changed {
        write_cars(cars_dir, &cars)?;
    }
    Ok(changed)
}

/// Moves the `.bank` files in the folder `from` into `to`, replacing those of the same name.
fn move_banks(from: &Path, to: &Path) -> Result<(), Fail> {
    let error = |e: io::Error| format!("{}: {e}", to.display());
    fs::create_dir_all(to).map_err(error)?;
    for entry in fs::read_dir(from).map_err(error)? {
        let entry = entry.map_err(error)?;
        let bank = entry.file_name().to_str().is_some_and(|n| n.to_lowercase().ends_with(".bank"));
        if bank && entry.file_type().map_err(error)?.is_file() {
            fs::rename(entry.path(), to.join(entry.file_name())).map_err(error)?;
        }
    }
    Ok(())
}

/// Moves the folder `from` to `to`, replacing what is there.
fn replace(from: &Path, to: &Path) -> Result<(), Fail> {
    let error = |e: io::Error| format!("{}: {e}", to.display());
    if to.exists() {
        fs::remove_dir_all(to).map_err(error)?;
    }
    fs::create_dir_all(to.parent().unwrap_or(to)).map_err(error)?;
    fs::rename(from, to).map_err(error)
}

/// The value after `key` on a line of the pack's text file (`revision            3 / 2024-12-10`).
fn about_value<'a>(about: &'a str, keys: &[&str]) -> Option<&'a str> {
    about.lines().find_map(|line| {
        let line = line.trim();
        keys.iter().find_map(|key| {
            let rest = line.get(key.len()..)?;
            (line[..key.len()].eq_ignore_ascii_case(key) && rest.starts_with([' ', '\t'])).then(|| rest.trim())
        })
    })
}

/// Adds `download` to `cars_dir`'s list, unpacking through `temp`: its car's name and model folder.
fn add_one(
    cars_dir: &Path,
    temp: &Path,
    download: &Download,
    cars: &mut Vec<StoredCar>,
    count: &mut Counter,
) -> Result<(String, Option<String>), Fail> {
    let physics = PhysicsZip::open(&download.physics)?;
    let model = download.model.as_deref().map(Archive::model).transpose()?;
    let banks = download.banks.as_deref().map(Archive::banks).transpose()?;
    let staged = temp.join("pack");
    physics.extract(&staged, count)?;
    let pack = car_pack::Pack::open(&staged)?;
    let about = pack.about().unwrap_or_default();
    let title = pack.title().filter(|t| !t.is_empty()).unwrap_or_else(|| physics.folder.replace('_', " "));
    let revision = about_value(&about, &["revision"]).unwrap_or_default().to_owned();
    let next = |ids: &mut dyn Iterator<Item = u32>| ids.max().map_or(1, |m| m + 1);
    let stored = match &model {
        Some(archive) => {
            let folder = archive.folder()?;
            archive.extract(&temp.join("model"), count)?;
            replace(&temp.join("model"), &cars_dir.join("Cars").join(&folder))?;
            let name = download
                .model
                .as_deref()
                .and_then(|p| strip_end(p.file_name()?.to_str()?, MODEL))
                .unwrap_or(&folder)
                .to_owned();
            StoredModel::Files { folder, file: archive.filename.clone(), name }
        }
        None => {
            // A model added before, else the retail stand-in.
            let download_name = download.physics.file_name().and_then(|n| strip_end(n.to_str()?, PHYSICS));
            let credit = about_value(&about, &["3D model", "model"]).unwrap_or_default();
            let mut real: Vec<&StoredModel> = cars
                .iter()
                .filter_map(|c| match &c.model {
                    StoredModel::Files { folder, .. } if cars_dir.join("Cars").join(folder).is_dir() => Some(&c.model),
                    _ => None,
                })
                .collect();
            real.sort_by_key(|m| match m {
                StoredModel::Files { folder, .. } => folder.clone(),
                StoredModel::StandIn { .. } => String::new(),
            });
            real.dedup_by_key(|m| match m {
                StoredModel::Files { folder, .. } => folder.clone(),
                StoredModel::StandIn { .. } => String::new(),
            });
            let names: Vec<&str> = real
                .iter()
                .map(|m| match m {
                    StoredModel::Files { name, .. } => name.as_str(),
                    StoredModel::StandIn { .. } => "",
                })
                .collect();
            match pick_model(&[download_name.unwrap_or_default(), &title, credit], &names) {
                Some(k) => real[k].clone(),
                None => StoredModel::StandIn { slot: STAND_IN },
            }
        }
    };
    if let Some(banks) = &banks {
        banks.extract(&temp.join("banks"), count)?;
        move_banks(&temp.join("banks"), &cars_dir.join(AUDIO))?;
    }
    replace(&staged, &cars_dir.join("rsfdata").join("cars").join(&physics.folder))?;
    // A re-add without metadata keeps the car's own group and drive train.
    let (group, drive_train) = match cars.iter().find(|c| c.pack.eq_ignore_ascii_case(&physics.folder)) {
        Some(known) => (
            download.group.clone().or_else(|| known.group.clone()),
            download.drive_train.clone().or_else(|| known.drive_train.clone()),
        ),
        None => (download.group.clone(), download.drive_train.clone()),
    };
    let row = StoredCar {
        id: 0,
        name: title.clone(),
        pack: physics.folder.clone(),
        group,
        model: stored,
        revision,
        audio: banks.map(|b| b.filename),
        drive_train,
    };
    match cars.iter_mut().find(|c| c.pack.eq_ignore_ascii_case(&physics.folder)) {
        Some(c) => *c = StoredCar { id: c.id, ..row },
        None => {
            let id = next(&mut cars.iter().map(|c| c.id));
            cars.push(StoredCar { id, ..row });
        }
    }
    write_cars(cars_dir, cars)?;
    let folder = match &cars.iter().find(|c| c.pack.eq_ignore_ascii_case(&physics.folder)).map(|c| &c.model) {
        Some(StoredModel::Files { folder, .. }) => Some(folder.clone()),
        _ => None,
    };
    Ok((title, folder.filter(|f| cars_dir.join("Cars").join(f).is_dir())))
}

/// Adds `downloads` (each physics pack with its model and sound banks, if any) to `cars_dir`; `report` gets the
/// progress, and `cancel` stops it. One pack that fails does not stop the others; a cancel keeps those added.
pub fn add_downloads(
    cars_dir: &Path,
    downloads: &[Download],
    report: Report<'_>,
    cancel: &AtomicBool,
) -> Result<Vec<Added>, Fail> {
    fs::create_dir_all(cars_dir).map_err(|e| format!("{}: {e}", cars_dir.display()))?;
    // The bytes to write (a download that does not open fails again when it is added).
    let physics = |d: &Download| PhysicsZip::open(&d.physics).map_or(0, |z| z.size);
    let size =
        |a: Option<&Path>, open: fn(&Path) -> Result<Archive, Fail>| a.and_then(|a| open(a).ok()).map_or(0, |a| a.size);
    let archives = |d: &Download| size(d.model.as_deref(), Archive::model) + size(d.banks.as_deref(), Archive::banks);
    let total = downloads.iter().map(|d| physics(d) + archives(d)).sum();
    let mut count = Counter { step: "Adding cars", done: 0, total, report, cancel };
    count.add(0)?;
    let mut list = list(cars_dir);
    let mut added = Vec::new();
    for download in downloads {
        let temp = Temp::create(cars_dir, "adding")?;
        let result = add_one(cars_dir, &temp.0, download, &mut list, &mut count);
        drop(temp);
        let name = download.physics.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match result {
            Err(e) if e.ends_with("cancelled") => return Err(e),
            result => added.push(Added { download: name, result }),
        }
    }
    Ok(added)
}

/// Removes the added car whose pack folder is `pack` from `cars_dir`, and its model when no other car uses it.
pub fn remove(cars_dir: &Path, pack: &str) -> Result<(), Fail> {
    let mut cars = list(cars_dir);
    let audio = cars.iter().find(|c| c.pack.eq_ignore_ascii_case(pack)).and_then(|c| c.audio.clone());
    cars.retain(|c| !c.pack.eq_ignore_ascii_case(pack));
    write_cars(cars_dir, &cars)?;
    // Its sound banks, when no other car plays them.
    if let Some(name) =
        audio.filter(|a| !cars.iter().any(|c| c.audio.as_ref() == Some(a))).and_then(|a| safe_folder(&a))
    {
        for end in [".bank", ".strings.bank"] {
            let _ = fs::remove_file(cars_dir.join(AUDIO).join(format!("{name}{end}")));
        }
    }
    let folder = safe_folder(pack).ok_or(format!("a bad pack folder name {pack}"))?;
    let path = cars_dir.join("rsfdata").join("cars").join(folder);
    match path.exists() {
        true => fs::remove_dir_all(&path).map_err(|e| format!("{}: {e}", path.display())),
        false => Ok(()),
    }
}

/// The result line: the cars added, those drawn with the stand-in model, and those not added.
pub fn summary(added: &[Added]) -> Result<String, String> {
    let ok: Vec<&(String, Option<String>)> = added.iter().filter_map(|a| a.result.as_ref().ok()).collect();
    let mut text = match ok.len() {
        1 => format!("Added {}.", ok[0].0),
        n => format!("Added {n} cars."),
    };
    let bare: Vec<&str> = ok.iter().filter(|(_, m)| m.is_none()).map(|(n, _)| n.as_str()).collect();
    if !bare.is_empty() {
        text += &format!(
            " No model for {}: drawn as the {STAND_IN_NAME}, as RallySimFans does, until its \"<Model>{MODEL}\" is \
             added.",
            bare.join(", ")
        );
    }
    let failed: Vec<String> =
        added.iter().filter_map(|a| Some(format!("{}: {}", a.download, a.result.as_ref().err()?))).collect();
    match (ok.is_empty(), failed.is_empty()) {
        (_, true) => Ok(text),
        (true, false) => Err(format!("Nothing added. {}", failed.join("; "))),
        (false, false) => Err(format!("{text} Not added: {}", failed.join("; "))),
    }
}

/// "Download cars…"'s job and its last result, and the added cars (name, pack folder) for the Settings page's list.
#[derive(Default)]
pub struct Adding {
    job: Option<Job<Vec<Added>>>,
    pub(crate) status: Option<Result<String, String>>,
    pub cars: Vec<(String, String)>,
}

impl Adding {
    /// The added cars of `cars_dir`.
    pub fn new(cars_dir: &Path) -> Adding {
        let cars = list(cars_dir).into_iter().map(|c| (c.name, c.pack)).collect();
        Adding { cars, ..Adding::default() }
    }
}

impl Form {
    /// Whether cars can be added now: not while adding or driving.
    pub fn can_add_cars(&self) -> bool {
        self.adding.job.is_none() && self.child.is_none()
    }

    /// Whether a job runs or a result waits to be shown.
    pub fn has_adding_output(&self) -> bool {
        self.adding.job.is_some() || self.adding.status.is_some()
    }

    /// The running job's progress and Cancel; when it ends, its result is kept and the lists read again.
    pub fn adding_job_ui(&mut self, ui: &mut egui::Ui) {
        let Some(job) = &mut self.adding.job else { return };
        if let Some(result) = job.ui(ui) {
            self.adding.job = None;
            self.adding.status = Some(match result {
                Err(e) if e.ends_with("cancelled") => Err("Cancelled; the cars added before are kept.".into()),
                result => result.and_then(|added| summary(&added)),
            });
            self.rescan_cars();
        }
    }

    /// The last result line.
    pub fn adding_status_ui(&self, ui: &mut egui::Ui) {
        match &self.adding.status {
            Some(Ok(text)) => _ = ui.colored_label(GOOD, format!("✔ {text}")),
            Some(Err(text)) => _ = ui.colored_label(WARN, format!("⚠ {text}")),
            None => {}
        }
    }

    /// Runs `work`, which adds cars, with its progress, Cancel and result line, unless adding or driving already.
    pub fn start_adding(
        &mut self,
        work: impl FnOnce(Report<'_>, &AtomicBool) -> Result<Vec<Added>, Fail> + Send + 'static,
    ) {
        if self.can_add_cars() {
            self.adding.status = None;
            self.adding.job = Some(Job::start(work));
        }
    }

    /// The added cars, each with Remove.
    pub fn added_cars_ui(&mut self, ui: &mut egui::Ui) {
        let mut removed = None;
        let idle = self.adding.job.is_none() && self.child.is_none();
        egui::Grid::new("added cars").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
            for (name, pack) in &self.adding.cars {
                ui.label(name).on_hover_text(pack);
                if ui.add_enabled(idle, egui::Button::new("Remove")).clicked() {
                    removed = Some(pack.clone());
                }
                ui.end_row();
            }
        });
        if let Some(pack) = removed {
            self.adding.status = remove(&self.cars_dir(), &pack).err().map(Err);
            self.rescan_cars();
        }
    }

    /// Reads the added cars again, into the lists.
    pub(crate) fn rescan_cars(&mut self) {
        let root = self.game_root();
        self.catalog.set_community_cars(&self.paths.data, &root, &self.cars_dir());
        self.list_cars();
        self.adding =
            Adding { job: self.adding.job.take(), status: self.adding.status.take(), ..Adding::new(&self.cars_dir()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    use crate::archive::{safe_folder, safe_path};

    #[test]
    fn models_are_picked_by_their_names_start() {
        let models = ["Mitsubishi Lancer Evo IX", "Mitsubishi Lancer Evo X", "Ford Fiesta", "Ford Fiesta Rally2"];
        let pick = |names: &[&str]| pick_model(names, &models);
        assert_eq!(pick(&["Mitsubishi Lancer Evo IX N4"]), Some(0));
        assert_eq!(pick(&["Mitsubishi Lancer Evo IX R4"]), Some(0));
        assert_eq!(pick(&["Mitsubishi Lancer Evo X R4"]), Some(1));
        assert_eq!(pick(&["Ford Fiesta Rally2"]), Some(3));
        assert_eq!(pick(&["Ford Fiesta R5"]), Some(2));
        assert_eq!(pick(&["Ford Fiesta Rally3", "x", "FORD FIESTA RALLY2 2023 by G. Rodriguez"]), Some(3));
        assert_eq!(pick(&["Mitsubishi_Lancer_Evo_IXR"]), None);
        assert_eq!(pick(&["", "Audi quattro"]), None);
    }

    #[test]
    fn archive_paths_stay_below_their_folder() {
        assert_eq!(safe_path("Pack\\setups/d_gravel.lsp"), Some(vec!["Pack", "setups", "d_gravel.lsp"]));
        assert_eq!(safe_path("/abs/./x"), Some(vec!["abs", "x"]));
        assert_eq!(safe_path("a/../../x"), None);
        assert_eq!(safe_path("C:\\x"), None);
        assert_eq!(safe_path(""), None);
        assert_eq!(safe_folder("evo_IX"), Some("evo_IX".into()));
        assert_eq!(safe_folder(".."), None);
        assert_eq!(safe_folder("a/b"), None);
        assert_eq!(safe_folder(".hidden"), None);
        assert_eq!(about_value("Evo\r\nrevision            8 / 2019-10-08\r\n", &["revision"]), Some("8 / 2019-10-08"));
        assert_eq!(about_value("3D model\t\tKUBRT v1.1\n", &["3D model", "model"]), Some("KUBRT v1.1"));
    }

    /// A physics pack download: `<Car> - NGP Physics.zip` holding `<pack>/` (or its files at the top).
    fn physics_zip(dir: &Path, car: &str, pack: Option<&str>, credit: &str) {
        let file = File::create(dir.join(format!("{car}{PHYSICS}"))).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let path = |name: &str| pack.map_or(name.to_owned(), |p| format!("{p}\\{name}"));
        let about = format!("{car}\r\nrevision            3 / 2024-12-10\r\n3D model            {credit}\r\n");
        for (name, text) in
            [("common.lsp", "((\"PhysicsEngine\"))"), ("setups\\d_gravel.lsp", "x"), (car, about.as_str())]
        {
            zip.start_file(path(name), options).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    /// A pack folder with a stray folder beside it (gvrc.de's Fiesta Rally2): the pack's files alone are kept.
    #[test]
    fn a_stray_folder_beside_the_pack_is_left_out() {
        let root = crate::test_dir("car-stray");
        let path = root.join(format!("Ford Fiesta Rally2{PHYSICS}"));
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for name in ["Pack/common.lsp", "Pack/setups/d_gravel.lsp", "Pack - rev1/Ford Fiesta Rally2"] {
            zip.start_file(name, options).unwrap();
            zip.write_all(b"x").unwrap();
        }
        zip.finish().unwrap();
        let pack = PhysicsZip::open(&path).unwrap();
        assert_eq!(pack.folder, "Pack");
        let below: Vec<String> = pack.entries.iter().map(|(_, p)| p.join("/")).collect();
        assert_eq!(below, ["common.lsp", "setups/d_gravel.lsp"]);
        assert_eq!(pack.size, 2);
        fs::remove_dir_all(&root).unwrap();
    }

    /// A model download: `<Model> - Model.7z` holding `Cars/<folder>/<file>.sgc` and a texture.
    fn model_7z(dir: &Path, model: &str, folder: &str, file: &str) {
        let mut out = sevenz_rust2::ArchiveWriter::create(dir.join(format!("{model}{MODEL}"))).unwrap();
        for (name, bytes) in [
            (format!("Cars/{folder}/{file}.sgc"), b"sgc".as_slice()),
            (format!("Cars/{folder}/textures/a.dds"), b"dds".as_slice()),
            ("readme.txt".to_owned(), b"read me".as_slice()),
        ] {
            out.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(&name), Some(bytes)).unwrap();
        }
        out.finish().unwrap();
    }

    /// Two packs sharing one model and one without a model are added, listed in `cars.ini`, added again (replaced,
    /// same ids) and removed (the shared model stays until its last car goes).
    #[test]
    fn packs_are_added_listed_and_removed() {
        let root = crate::test_dir("car-add");
        let (downloads_dir, cars) = (root.join("downloads"), root.join("cars"));
        fs::create_dir_all(&downloads_dir).unwrap();
        physics_zip(&downloads_dir, "Mitsubishi Lancer Evo IX N4", Some("Evo_IX_N4_ngp6"), "KUBRT");
        physics_zip(&downloads_dir, "Mitsubishi Lancer Evo IX R4", None, "KUBRT");
        physics_zip(&downloads_dir, "Ford Fiesta Rally2", Some("Ford_Fiesta_Rally2_ngp7"), "by someone");
        model_7z(&downloads_dir, "Mitsubishi Lancer Evo IX", "evo_IX", "evo_IX");
        fs::write(downloads_dir.join("Broken - NGP Physics.zip"), b"not a zip").unwrap();
        let download = |physics: &str, model: Option<&str>| Download {
            physics: downloads_dir.join(physics),
            model: model.map(|m| downloads_dir.join(m)),
            banks: None,
            group: None,
            drive_train: None,
        };
        let mut packs = vec![
            download("Broken - NGP Physics.zip", None),
            download("Ford Fiesta Rally2 - NGP Physics.zip", None),
            download("Mitsubishi Lancer Evo IX N4 - NGP Physics.zip", Some("Mitsubishi Lancer Evo IX - Model.7z")),
            download("Mitsubishi Lancer Evo IX R4 - NGP Physics.zip", Some("Mitsubishi Lancer Evo IX - Model.7z")),
        ];
        // What gvrc.de's list would name for the two Evo IX: the group and the drive travel with the download.
        packs[2].group = Some("Group N4".into());
        packs[2].drive_train = Some("4WD".into());
        packs[3].group = Some("Group R4".into());
        packs[3].drive_train = Some("4WD".into());

        let progress = std::sync::Mutex::new(Vec::new());
        let added =
            add_downloads(&cars, &packs, &|p| progress.lock().unwrap().push(p.done), &AtomicBool::new(false)).unwrap();
        let line = summary(&added).unwrap_err();
        assert!(line.starts_with("Added 3 cars. No model for Ford Fiesta Rally2: drawn as the Subaru"), "{line}");
        assert!(line.contains("Not added: Broken - NGP Physics.zip: "), "{line}");
        assert!(progress.lock().unwrap().windows(2).all(|w| w[0] <= w[1]));

        let found = list(&cars);
        let names: Vec<(&str, &str)> = found.iter().map(|c| (c.name.as_str(), c.pack.as_str())).collect();
        assert_eq!(
            names,
            [
                ("Ford Fiesta Rally2", "Ford_Fiesta_Rally2_ngp7"),
                ("Mitsubishi Lancer Evo IX N4", "Evo_IX_N4_ngp6"),
                ("Mitsubishi Lancer Evo IX R4", "Mitsubishi_Lancer_Evo_IX_R4"),
            ]
        );
        assert_eq!(found[1].revision, "3 / 2024-12-10");
        // The downloads' group and drive train are on the cars.
        assert_eq!(found[1].group.as_deref(), Some("Group N4"));
        assert_eq!(found[1].drive_train.as_deref(), Some("4WD"));
        assert_eq!(found[2].group.as_deref(), Some("Group R4"));
        let evo = car_pack::Pack::open(&cars.join("rsfdata/cars/Evo_IX_N4_ngp6")).unwrap();
        assert_eq!(evo.read("setups/d_gravel.lsp").unwrap(), b"x");
        assert_eq!(found[1].model, found[2].model);
        assert_eq!(
            found[1].model,
            StoredModel::Files {
                folder: "evo_IX".into(),
                file: "evo_IX".into(),
                name: "Mitsubishi Lancer Evo IX".into()
            }
        );
        assert_eq!(found[0].model, StoredModel::StandIn { slot: STAND_IN });
        assert!(cars.join("Cars/evo_IX/textures/a.dds").is_file());
        assert!(!cars.join("Cars/readme.txt").exists() && !cars.join("readme.txt").exists());
        assert!(cars.join("cars.ini").is_file());

        // Again: replaced, the same ids; nothing left in the temp folders.
        let ids: Vec<u32> = found.iter().map(|c| c.id).collect();
        add_downloads(&cars, &packs, &|_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(list(&cars).iter().map(|c| c.id).collect::<Vec<_>>(), ids);
        let mut top: Vec<String> =
            fs::read_dir(&cars).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        top.sort();
        assert_eq!(top, ["Cars", "cars.ini", "rsfdata"]);

        // The R4 alone again, its model already added: it keeps it.
        let single = root.join("single");
        fs::create_dir_all(&single).unwrap();
        fs::copy(
            downloads_dir.join(format!("Mitsubishi Lancer Evo IX R4{PHYSICS}")),
            single.join(format!("Mitsubishi Lancer Evo IX R4{PHYSICS}")),
        )
        .unwrap();
        let again = add_downloads(
            &cars,
            &[Download {
                physics: single.join(format!("Mitsubishi Lancer Evo IX R4{PHYSICS}")),
                model: None,
                banks: None,
                group: None,
                drive_train: None,
            }],
            &|_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(again[0].result, Ok(("Mitsubishi Lancer Evo IX R4".into(), Some("evo_IX".into()))));
        // A re-add without metadata keeps the car's group and drive train.
        let kept = list(&cars).into_iter().find(|c| c.pack == "Mitsubishi_Lancer_Evo_IX_R4").unwrap();
        assert_eq!(kept.group.as_deref(), Some("Group R4"));
        assert_eq!(kept.drive_train.as_deref(), Some("4WD"));

        remove(&cars, "Evo_IX_N4_ngp6").unwrap();
        assert!(!cars.join("rsfdata/cars/Evo_IX_N4_ngp6").exists());
        assert!(cars.join("Cars/evo_IX").is_dir());
        remove(&cars, "Mitsubishi_Lancer_Evo_IX_R4").unwrap();
        assert!(!cars.join("Cars/evo_IX").exists());
        assert_eq!(list(&cars).len(), 1);
        assert!(remove(&cars, "../x").is_err());

        // Cancelled: nothing half-written.
        let err = add_downloads(&cars, &packs, &|_| {}, &AtomicBool::new(true)).unwrap_err();
        assert!(err.ends_with("cancelled"));
        assert_eq!(fs::read_dir(&cars).unwrap().count(), 3);

        fs::remove_dir_all(&root).unwrap();
    }

    /// A pack with its sound banks: they go to `AudioFMOD/` and in the list, and go when the car is removed.
    #[test]
    fn sound_banks_are_added_and_removed_with_their_car() {
        let root = crate::test_dir("car-banks");
        let (downloads_dir, cars) = (root.join("downloads"), root.join("cars"));
        fs::create_dir_all(&downloads_dir).unwrap();
        physics_zip(&downloads_dir, "Citroen C2 GT S1600", Some("C2_S1600"), "x");
        let mut out =
            sevenz_rust2::ArchiveWriter::create(downloads_dir.join(format!("Citroen C2 GT S1600{BANKS}"))).unwrap();
        for name in [
            "AudioFMOD/C2_S1600.strings.bank",
            "AudioFMOD/C2_S1600.bank",
            "AudioFMOD/C2_S1600_broken.bank",
            "AudioFMOD/readme.txt",
        ] {
            out.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(b"bank".as_slice())).unwrap();
        }
        out.finish().unwrap();
        let downloads = [Download {
            physics: downloads_dir.join(format!("Citroen C2 GT S1600{PHYSICS}")),
            model: None,
            banks: Some(downloads_dir.join(format!("Citroen C2 GT S1600{BANKS}"))),
            group: None,
            drive_train: None,
        }];
        add_downloads(&cars, &downloads, &|_| {}, &AtomicBool::new(false)).unwrap();

        let found = list(&cars);
        assert_eq!(found[0].audio.as_deref(), Some("C2_S1600"));
        assert_eq!(
            rbr_formats::cars::resolve(&cars, "AudioFMOD\\C2_S1600.bank"),
            Some(cars.join("AudioFMOD/C2_S1600.bank"))
        );
        assert!(cars.join("AudioFMOD/C2_S1600.strings.bank").is_file());
        assert!(!cars.join("AudioFMOD/readme.txt").exists());
        remove(&cars, "C2_S1600").unwrap();
        assert!(
            !cars.join("AudioFMOD/C2_S1600.bank").exists() && !cars.join("AudioFMOD/C2_S1600.strings.bank").exists()
        );
        fs::remove_dir_all(&root).unwrap();
    }

    /// A `cars.ini` round trip keeps each car's fields, and a second rewrite is byte-identical.
    #[test]
    fn group_and_drive_survive_a_rewrite() {
        let root = crate::test_dir("car-groups");
        let cars_dir = root.join("cars");
        let car = |id, pack: &str, group: Option<&str>, drive: Option<&str>| StoredCar {
            id,
            name: format!("{pack} car"),
            pack: pack.into(),
            group: group.map(str::to_owned),
            model: StoredModel::Files { folder: "m".into(), file: "m".into(), name: "m".into() },
            revision: "3 / 2024-12-10".into(),
            audio: None,
            drive_train: drive.map(str::to_owned),
        };
        let cars = vec![
            car(1, "evo_IX", Some("Group N4"), Some("4WD")),
            car(2, "quattro", Some("Group B"), Some("AWD with torque split")),
            car(3, "zr", None, None),
        ];
        write_cars(&cars_dir, &cars).unwrap();
        let text = fs::read_to_string(cars_dir.join("cars.ini")).unwrap();
        assert!(text.contains("[evo_IX]\nname = evo_IX car\nid = 1\ngroup = Group N4\ndrive = 4WD\n"), "{text}");
        assert!(text.contains("[zr]\nname = zr car\nid = 3\n"), "{text}");
        assert!(text.contains("model = m/m\n"), "{text}");
        let found = list(&cars_dir);
        assert_eq!(found, cars);
        // A second rewrite changes nothing.
        let before = fs::read(cars_dir.join("cars.ini")).unwrap();
        write_cars(&cars_dir, &found).unwrap();
        assert_eq!(fs::read(cars_dir.join("cars.ini")).unwrap(), before);
        fs::remove_dir_all(&root).unwrap();
    }

    /// Cars added before the group and drive train that lack them take them from the list's entries: only matching
    /// packs without values change, and the list is not rewritten when nothing changed.
    #[test]
    fn missing_groups_and_drives_are_backfilled_from_the_list() {
        let root = crate::test_dir("car-backfill");
        let cars_dir = root.join("cars");
        let car = |id, pack: &str, group: Option<&str>, drive: Option<&str>| StoredCar {
            id,
            name: format!("{pack} car"),
            pack: pack.into(),
            group: group.map(str::to_owned),
            model: StoredModel::Files { folder: "m".into(), file: "m".into(), name: "m".into() },
            revision: String::new(),
            audio: None,
            drive_train: drive.map(str::to_owned),
        };
        write_cars(
            &cars_dir,
            &[
                car(1, "BMW M1 GrpB", None, None),
                car(2, "Fiesta", Some("Group B"), None),
                car(3, "Ghost", None, None),
                car(4, "evo_IX", Some("WRC 1.6"), Some("4WD")),
            ],
        )
        .unwrap();
        let entry = |pack: &str, cat: &str, trans: &str| crate::gvrc::Entry {
            name: pack.into(),
            cat: cat.into(),
            year: String::new(),
            pack: pack.into(),
            trans: trans.into(),
            physics: String::new(),
            model: None,
            banks: None,
        };
        let entries = vec![
            entry("bmw m1 grpb", "Group B", "RWD"),
            entry("Fiesta", "Rally2", "FWD"),
            entry("evo_IX", "Group B", "RWD"),
        ];
        assert!(backfill(&cars_dir, &entries).unwrap());
        let found = list(&cars_dir);
        let group = |pack: &str| found.iter().find(|c| c.pack == pack).unwrap().group.clone();
        let drive = |pack: &str| found.iter().find(|c| c.pack == pack).unwrap().drive_train.clone();
        assert_eq!((group("BMW M1 GrpB").as_deref(), drive("BMW M1 GrpB").as_deref()), (Some("Group B"), Some("RWD")));
        assert_eq!((group("Fiesta").as_deref(), drive("Fiesta").as_deref()), (Some("Group B"), Some("FWD")));
        assert_eq!((group("Ghost").as_deref(), drive("Ghost").as_deref()), (None, None));
        assert_eq!((group("evo_IX").as_deref(), drive("evo_IX").as_deref()), (Some("WRC 1.6"), Some("4WD")));
        // Nothing left to fill: no rewrite.
        let before = fs::read(cars_dir.join("cars.ini")).unwrap();
        assert!(!backfill(&cars_dir, &entries).unwrap());
        assert_eq!(fs::read(cars_dir.join("cars.ini")).unwrap(), before);
        fs::remove_dir_all(&root).unwrap();
    }

    /// The old RSF-format JSON cache converts to `cars.ini` once, keeping the ids, and the JSON files go.
    #[test]
    fn the_old_json_cache_migrates_to_cars_ini() {
        let root = crate::test_dir("car-migrate");
        let cars_dir = root.join("cars");
        let cache = cars_dir.join("rsfdata").join("cache");
        fs::create_dir_all(&cache).unwrap();
        let json = |rows: &[&str]| format!("[\n{}\n]\n", rows.join(",\n"));
        fs::write(cache.join("cargroups.json"), json(&["{\"id\":1,\"name\":\"Group N4\"}"])).unwrap();
        fs::write(
            cache.join("carmodels.json"),
            json(&["{\"id\":7,\"name\":\"Evo IX\",\"path\":\"evo_IX\",\"filename\":\"evo_IX\",\"fallback\":5}"]),
        )
        .unwrap();
        fs::write(
            cache.join("cars.json"),
            json(&[
                "{\"id\":4,\"name\":\"Evo IX\",\"path\":\"Evo_IX\",\"carmodel_id\":7,\"rev\":\"3\",\"base_group_id\":1,\"audio\":\"Evo\"}",
                "{\"id\":9,\"name\":\"Ghost\",\"path\":\"Ghost\",\"carmodel_id\":77,\"rev\":\"\"}",
            ]),
        )
        .unwrap();
        fs::write(cache.join("cars_data.json"), json(&["{\"car_id\":4,\"drive_train\":\"4WD\"}"])).unwrap();
        let found = list(&cars_dir);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, 4);
        assert_eq!(found[0].group.as_deref(), Some("Group N4"));
        assert_eq!(found[0].drive_train.as_deref(), Some("4WD"));
        assert_eq!(found[0].audio.as_deref(), Some("Evo"));
        assert_eq!(
            found[0].model,
            StoredModel::Files { folder: "evo_IX".into(), file: "evo_IX".into(), name: "Evo IX".into() }
        );
        // A model the cache does not name is the retail stand-in.
        assert_eq!(found[1].model, StoredModel::StandIn { slot: STAND_IN });
        for name in ["cargroups.json", "carmodels.json", "cars.json", "cars_data.json"] {
            assert!(!cache.join(name).exists(), "{name} deleted");
        }
        // A second read keeps the converted list, byte-identical.
        let before = fs::read(cars_dir.join("cars.ini")).unwrap();
        assert_eq!(list(&cars_dir), found);
        assert_eq!(fs::read(cars_dir.join("cars.ini")).unwrap(), before);
        fs::remove_dir_all(&root).unwrap();
    }
}
