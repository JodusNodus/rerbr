//! Adding gvrc.de's NGP car packs, so that the cars drive without RallySimFans: the Settings page's "Download cars…"
//! downloads the packs and adds them through [`add_downloads`]: every `<Car> - NGP Physics.zip` with its
//! `<Model> - Model.7z` and its `<Car> - banks.7z`.
//!
//! The cars go into `cars/` in the player's folder (`home`), laid out as an RSF install so that `rbr_formats::rsf` reads them:
//! - `cars/rsfdata/cars/<pack>/`: the physics pack, unpacked (the zip's `<pack>/` folder, else the download's name
//!   with `_` for spaces);
//! - `cars/Cars/<model>/`: the model, the `.7z`'s folder holding a `.sgc`;
//! - `cars/AudioFMOD/`: the car's FMOD sound banks, the `.7z`'s folder holding a `.bank`;
//! - `cars/rsfdata/cache/cars.json` and `carmodels.json`: the list, written by us from the packs' text files (the
//!   name, the revision, the sound bank; ids of our own, so the launcher does not match them with RSF's ghosts or
//!   gearboxes), and `cargroups.json` with `cars_data.json`: each car's group (gvrc.de's list category, as RSF names
//!   it) and drive train, the fields the car browser lists and filters by.
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
pub const STAND_IN: u32 = rsf::SLOT;
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
    /// The zip at `path`: its files in one `<pack>/` folder (as RSF's `rsfdata/cars/<pack>.zip`), or at its top (the
    /// pack folder is then the download's name, `_` for spaces).
    fn open(path: &Path) -> Result<PhysicsZip, Fail> {
        let error = |e: String| format!("{}: {e}", path.display());
        let mut files = Vec::new();
        let mut size = 0;
        for entry in crate::archive::zip_entries(path).map_err(error)? {
            let parts = entry.parts.ok_or_else(|| error(format!("a bad entry path {}", entry.name)))?;
            if !entry.dir {
                size += entry.size;
                files.push(parts);
            }
        }
        let first = files.first().map(|p| p[0].clone()).unwrap_or_default();
        let nested = files.iter().all(|p| p.len() > 1 && p[0] == first);
        let download = path.file_name().and_then(|n| strip_end(n.to_str()?, PHYSICS)).unwrap_or_default();
        let folder = match nested {
            true => first,
            false => download.replace(' ', "_"),
        };
        let folder = safe_folder(&folder).ok_or_else(|| error(format!("a bad pack folder name {folder}")))?;
        let entries: Vec<_> =
            files.into_iter().map(|p| (p.clone(), if nested { p[1..].to_vec() } else { p })).collect();
        if !entries.iter().any(|(_, p)| p.len() == 1 && p[0].eq_ignore_ascii_case("common.lsp")) {
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

/// The added cars' list in `cars` (none without one).
pub fn list(cars: &Path) -> (Vec<rsf::Car>, Vec<rsf::Model>) {
    match rsf::Install::open(cars) {
        Ok(install) => (install.cars, install.models),
        Err(_) => (Vec::new(), Vec::new()),
    }
}

/// `text` as a JSON string.
fn json(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out + "\""
}

/// gvrc.de's list category as RSF names the car's group (`Rally2` is `Rally 2`, a bare `A8` is `Group A8`);
/// anything else is kept as named, trimmed. What an added car is listed and filtered by.
pub(crate) fn normalize_group(cat: &str) -> String {
    let cat = cat.trim();
    if let Some(n) = cat.strip_prefix("Rally") {
        if !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) {
            return format!("Rally {n}");
        }
    }
    match cat {
        "A5" | "A6" | "A7" | "A8" => format!("Group {cat}"),
        _ => cat.to_owned(),
    }
}

/// Writes the list RSF's reader takes (`rsfdata/cache/cargroups.json`, `cars.json`, `carmodels.json` and
/// `cars_data.json`), dropping the models no car uses and their folders. Each car keeps its `base_group_id` (one
/// row per distinct group name, ids of our own) and its `cars_data.json` drive train, so both survive a rewrite.
pub(crate) fn write_list(cars_dir: &Path, cars: &[rsf::Car], models: &[rsf::Model]) -> Result<(), Fail> {
    let used: Vec<&rsf::Model> = models.iter().filter(|m| cars.iter().any(|c| c.model == m.id)).collect();
    for model in models.iter().filter(|m| !used.iter().any(|u| u.path.eq_ignore_ascii_case(&m.path))) {
        if let Some(folder) = safe_folder(&model.path) {
            let _ = fs::remove_dir_all(cars_dir.join("Cars").join(folder));
        }
    }
    let rows = |rows: Vec<String>| format!("[\n{}\n]\n", rows.join(",\n"));
    let mut groups: Vec<&str> = cars.iter().filter_map(|c| c.group.as_deref()).filter(|g| !g.is_empty()).collect();
    groups.sort();
    groups.dedup();
    let groups_json = rows(
        groups.iter().enumerate().map(|(k, name)| format!("{{\"id\":{},\"name\":{}}}", k + 1, json(name))).collect(),
    );
    let cars_json = rows(
        cars.iter()
            .map(|c| {
                let (id, name, path, rev) = (c.id, json(&c.name), json(&c.pack), json(&c.revision));
                let group = c
                    .group
                    .as_deref()
                    .filter(|g| !g.is_empty())
                    .and_then(|g| groups.iter().position(|h| *h == g))
                    .map(|k| format!(",\"base_group_id\":{}", k + 1))
                    .unwrap_or_default();
                let audio = c.audio.as_deref().map(|a| format!(",\"audio\":{}", json(a))).unwrap_or_default();
                format!(
                    "{{\"id\":{id},\"name\":{name},\"path\":{path},\"carmodel_id\":{},\"rev\":{rev}{group}{audio}}}",
                    c.model
                )
            })
            .collect(),
    );
    let models_json = rows(
        used.iter()
            .map(|m| {
                let (id, name, path, file) = (m.id, json(&m.name), json(&m.path), json(&m.filename));
                format!(
                    "{{\"id\":{id},\"name\":{name},\"path\":{path},\"filename\":{file},\"fallback\":{}}}",
                    m.fallback
                )
            })
            .collect(),
    );
    let data_json = rows(
        cars.iter()
            .filter_map(|c| {
                let drive = c.drive_train.as_deref().filter(|d| !d.is_empty())?;
                Some(format!("{{\"car_id\":{},\"drive_train\":{}}}", c.id, json(drive)))
            })
            .collect(),
    );
    let cache = cars_dir.join("rsfdata").join("cache");
    fs::create_dir_all(&cache).map_err(|e| format!("{}: {e}", cache.display()))?;
    for (name, text) in [
        ("cargroups.json", groups_json),
        ("carmodels.json", models_json),
        ("cars.json", cars_json),
        ("cars_data.json", data_json),
    ] {
        let (temp, path) = (cache.join(format!(".{name}.new")), cache.join(name));
        fs::write(&temp, text)
            .and_then(|()| fs::rename(&temp, &path))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// Fills in the group and drive train gvrc.de's list names for added cars that lack them: the cars whose pack
/// folder is an entry's pack folder (ignoring case) take its normalized category and its drive. Rewrites the list
/// only when a car gained something, and says whether it did.
pub fn backfill(cars_dir: &Path, entries: &[Entry]) -> Result<bool, Fail> {
    let (mut cars, models) = list(cars_dir);
    let mut changed = false;
    for car in &mut cars {
        let missing = car.group.as_deref().filter(|g| !g.is_empty()).is_none()
            || car.drive_train.as_deref().filter(|d| !d.is_empty()).is_none();
        let Some(entry) = missing.then(|| entries.iter().find(|e| e.pack.eq_ignore_ascii_case(&car.pack))).flatten()
        else {
            continue;
        };
        if car.group.as_deref().filter(|g| !g.is_empty()).is_none() {
            let group = normalize_group(&entry.cat);
            if !group.is_empty() {
                car.group = Some(group);
                changed = true;
            }
        }
        if car.drive_train.as_deref().filter(|d| !d.is_empty()).is_none() {
            let drive = entry.trans.trim();
            if !drive.is_empty() {
                car.drive_train = Some(drive.to_owned());
                changed = true;
            }
        }
    }
    if changed {
        write_list(cars_dir, &cars, &models)?;
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

/// Adds `download` to `cars_dir`'s list (`cars`, `models`), unpacking through `temp`: its car's name and model folder.
fn add_one(
    cars_dir: &Path,
    temp: &Path,
    download: &Download,
    list: &mut (Vec<rsf::Car>, Vec<rsf::Model>),
    count: &mut Counter,
) -> Result<(String, Option<String>), Fail> {
    let physics = PhysicsZip::open(&download.physics)?;
    let model = download.model.as_deref().map(Archive::model).transpose()?;
    let banks = download.banks.as_deref().map(Archive::banks).transpose()?;
    let staged = temp.join("pack");
    physics.extract(&staged, count)?;
    let pack = rsf::Pack::open(&staged)?;
    let about = pack.about().unwrap_or_default();
    let title = pack.title().filter(|t| !t.is_empty()).unwrap_or_else(|| physics.folder.replace('_', " "));
    let revision = about_value(&about, &["revision"]).unwrap_or_default().to_owned();
    let (cars, models) = list;
    let next = |ids: &mut dyn Iterator<Item = u32>| ids.max().map_or(1, |m| m + 1);
    let model_id = match &model {
        Some(archive) => {
            let folder = archive.folder()?;
            archive.extract(&temp.join("model"), count)?;
            replace(&temp.join("model"), &cars_dir.join("Cars").join(&folder))?;
            let name = download.model.as_deref().and_then(|p| strip_end(p.file_name()?.to_str()?, MODEL));
            let row = rsf::Model {
                id: 0,
                name: name.unwrap_or(&folder).to_owned(),
                path: folder.clone(),
                filename: archive.filename.clone(),
                fallback: STAND_IN,
            };
            match models.iter_mut().find(|m| m.path.eq_ignore_ascii_case(&folder)) {
                Some(m) => {
                    *m = rsf::Model { id: m.id, ..row };
                    m.id
                }
                None => {
                    let id = next(&mut models.iter().map(|m| m.id));
                    models.push(rsf::Model { id, ..row });
                    id
                }
            }
        }
        None => {
            // A model added before, else the stand-in's row: a folder that is never there.
            let download_name = download.physics.file_name().and_then(|n| strip_end(n.to_str()?, PHYSICS));
            let credit = about_value(&about, &["3D model", "model"]).unwrap_or_default();
            let real: Vec<&rsf::Model> =
                models.iter().filter(|m| cars_dir.join("Cars").join(&m.path).is_dir()).collect();
            let names: Vec<&str> = real.iter().map(|m| m.name.as_str()).collect();
            match pick_model(&[download_name.unwrap_or_default(), &title, credit], &names) {
                Some(k) => real[k].id,
                None => {
                    let id = next(&mut models.iter().map(|m| m.id));
                    let missing = format!("{} (no model)", physics.folder);
                    let row = rsf::Model {
                        id,
                        name: missing.clone(),
                        path: missing.clone(),
                        filename: missing,
                        fallback: STAND_IN,
                    };
                    models.push(row);
                    id
                }
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
    let row = rsf::Car {
        id: 0,
        name: title.clone(),
        pack: physics.folder.clone(),
        group,
        model: model_id,
        revision,
        audio: banks.map(|b| b.filename),
        drive_train,
    };
    match cars.iter_mut().find(|c| c.pack.eq_ignore_ascii_case(&physics.folder)) {
        Some(c) => *c = rsf::Car { id: c.id, ..row },
        None => {
            let id = next(&mut cars.iter().map(|c| c.id));
            cars.push(rsf::Car { id, ..row });
        }
    }
    write_list(cars_dir, cars, models)?;
    let folder = models.iter().find(|m| m.id == model_id).map(|m| m.path.clone());
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
    let (mut cars, models) = list(cars_dir);
    let audio = cars.iter().find(|c| c.pack.eq_ignore_ascii_case(pack)).and_then(|c| c.audio.clone());
    cars.retain(|c| !c.pack.eq_ignore_ascii_case(pack));
    write_list(cars_dir, &cars, &models)?;
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
        let cars = list(cars_dir).0.into_iter().map(|c| (c.name, c.pack)).collect();
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
        let root = self.rsf();
        self.catalog.set_rsf_cars(&self.paths.data, &root, &self.cars_dir());
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
        assert_eq!(json("a \"b\" \\ \u{1}"), "\"a \\\"b\\\" \\\\ \\u0001\"");
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

    /// Two packs sharing one model and one without a model are added, listed by RSF's reader, added again (replaced,
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

        let install = rsf::Install::open(&cars).unwrap();
        let names: Vec<(&str, &str)> = install.cars.iter().map(|c| (c.name.as_str(), c.pack.as_str())).collect();
        assert_eq!(
            names,
            [
                ("Ford Fiesta Rally2", "Ford_Fiesta_Rally2_ngp7"),
                ("Mitsubishi Lancer Evo IX N4", "Evo_IX_N4_ngp6"),
                ("Mitsubishi Lancer Evo IX R4", "Mitsubishi_Lancer_Evo_IX_R4"),
            ]
        );
        assert_eq!(install.cars[1].revision, "3 / 2024-12-10");
        // The downloads' group and drive train are on the cars, and read back through the list.
        assert_eq!(install.cars[1].group.as_deref(), Some("Group N4"));
        assert_eq!(install.cars[1].drive_train.as_deref(), Some("4WD"));
        assert_eq!(install.cars[2].group.as_deref(), Some("Group R4"));
        assert_eq!(list(&cars).0[1].group.as_deref(), Some("Group N4"));
        let evo = install.pack(&install.cars[1]).unwrap();
        assert_eq!(evo.read("setups/d_gravel.lsp").unwrap(), b"x");
        assert_eq!(evo.path(), cars.join("rsfdata/cars/Evo_IX_N4_ngp6"));
        assert_eq!(install.cars[1].model, install.cars[2].model);
        let model = |c: &rsf::Car| install.model_entry(c, rsf::SLOT, &[retail()]).unwrap();
        assert_eq!(model(&install.cars[1]).entry.file_name, "Cars\\evo_IX\\evo_IX.sgc");
        assert_eq!(model(&install.cars[1]).fallback, None);
        assert_eq!(model(&install.cars[0]).entry.file_name, retail().file_name);
        assert!(cars.join("Cars/evo_IX/textures/a.dds").is_file());
        assert!(!cars.join("Cars/readme.txt").exists() && !cars.join("readme.txt").exists());
        let (found, car, _) = rsf::locate(&evo.path().display().to_string(), Some(&root)).unwrap();
        assert_eq!((found.root, car.name.as_str()), (cars.canonicalize().unwrap(), "Mitsubishi Lancer Evo IX N4"));

        // Again: replaced, the same ids; nothing left in the temp folders.
        let ids: Vec<u32> = install.cars.iter().map(|c| c.id).collect();
        add_downloads(&cars, &packs, &|_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(list(&cars).0.iter().map(|c| c.id).collect::<Vec<_>>(), ids);
        let mut top: Vec<String> =
            fs::read_dir(&cars).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        top.sort();
        assert_eq!(top, ["Cars", "rsfdata"]);

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
        let kept = list(&cars).0.into_iter().find(|c| c.pack == "Mitsubishi_Lancer_Evo_IX_R4").unwrap();
        assert_eq!(kept.group.as_deref(), Some("Group R4"));
        assert_eq!(kept.drive_train.as_deref(), Some("4WD"));

        remove(&cars, "Evo_IX_N4_ngp6").unwrap();
        assert!(!cars.join("rsfdata/cars/Evo_IX_N4_ngp6").exists());
        assert!(cars.join("Cars/evo_IX").is_dir());
        remove(&cars, "Mitsubishi_Lancer_Evo_IX_R4").unwrap();
        assert!(!cars.join("Cars/evo_IX").exists());
        assert_eq!(list(&cars).0.len(), 1);
        assert!(remove(&cars, "../x").is_err());

        // Cancelled: nothing half-written.
        let err = add_downloads(&cars, &packs, &|_| {}, &AtomicBool::new(true)).unwrap_err();
        assert!(err.ends_with("cancelled"));
        assert_eq!(fs::read_dir(&cars).unwrap().count(), 2);
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

        let install = rsf::Install::open(&cars).unwrap();
        assert_eq!(install.cars[0].audio.as_deref(), Some("C2_S1600"));
        assert_eq!(install.audio_bank(&install.cars[0]), Some(cars.join("AudioFMOD/C2_S1600.bank")));
        assert!(cars.join("AudioFMOD/C2_S1600.strings.bank").is_file());
        assert!(!cars.join("AudioFMOD/readme.txt").exists());
        remove(&cars, "C2_S1600").unwrap();
        assert!(
            !cars.join("AudioFMOD/C2_S1600.bank").exists() && !cars.join("AudioFMOD/C2_S1600.strings.bank").exists()
        );
        fs::remove_dir_all(&root).unwrap();
    }

    /// gvrc.de's categories as RSF names the groups: `RallyN` takes a space, a bare `A5`..`A8` takes `Group`.
    #[test]
    fn groups_are_normalized_to_rsf_names() {
        assert_eq!(normalize_group("Rally2"), "Rally 2");
        assert_eq!(normalize_group("Rally5"), "Rally 5");
        assert_eq!(normalize_group("Rally 2"), "Rally 2");
        assert_eq!(normalize_group("A5"), "Group A5");
        assert_eq!(normalize_group("A8"), "Group A8");
        assert_eq!(normalize_group("Group A8"), "Group A8");
        assert_eq!(normalize_group("Group B"), "Group B");
        assert_eq!(normalize_group("WRC 1.6"), "WRC 1.6");
        assert_eq!(normalize_group("  Group N4  "), "Group N4");
        assert_eq!(normalize_group("Rally"), "Rally");
        assert_eq!(normalize_group(""), "");
    }

    /// A `write_list` round trip keeps each car's group and drive train, and a second rewrite is byte-identical.
    #[test]
    fn group_and_drive_survive_a_rewrite() {
        let root = crate::test_dir("car-groups");
        let cars_dir = root.join("cars");
        let model = rsf::Model { id: 7, name: "m".into(), path: "m".into(), filename: "m".into(), fallback: 5 };
        let car = |id, pack: &str, group: Option<&str>, drive: Option<&str>| rsf::Car {
            id,
            name: format!("{pack} car"),
            pack: pack.into(),
            group: group.map(str::to_owned),
            model: 7,
            revision: "3 / 2024-12-10".into(),
            audio: None,
            drive_train: drive.map(str::to_owned),
        };
        let cars = vec![
            car(1, "evo_IX", Some("Group N4"), Some("4WD")),
            car(2, "quattro", Some("Group B"), Some("AWD with torque split")),
            car(3, "zr", None, None),
        ];
        write_list(&cars_dir, &cars, &[model]).unwrap();
        let cache = cars_dir.join("rsfdata").join("cache");
        let groups = fs::read_to_string(cache.join("cargroups.json")).unwrap();
        assert!(
            groups.contains("\"id\":1,\"name\":\"Group B\"") && groups.contains("\"id\":2,\"name\":\"Group N4\""),
            "{groups}"
        );
        let list_cars = fs::read_to_string(cache.join("cars.json")).unwrap();
        assert!(
            list_cars.contains("\"path\":\"evo_IX\",\"carmodel_id\":7,\"rev\":\"3 / 2024-12-10\",\"base_group_id\":2")
                && list_cars.contains(
                    "{\"id\":3,\"name\":\"zr car\",\"path\":\"zr\",\"carmodel_id\":7,\"rev\":\"3 / 2024-12-10\"}"
                ),
            "{list_cars}"
        );
        let data = fs::read_to_string(cache.join("cars_data.json")).unwrap();
        assert!(
            data.contains("\"car_id\":1,\"drive_train\":\"4WD\"")
                && data.contains("\"car_id\":2,\"drive_train\":\"AWD with torque split\"")
                && !data.contains("\"car_id\":3"),
            "{data}"
        );
        let (found, found_models) = list(&cars_dir);
        assert_eq!(found, cars);
        assert_eq!(found_models.len(), 1);
        // A second rewrite changes nothing.
        let before: Vec<Vec<u8>> = ["cargroups.json", "carmodels.json", "cars.json", "cars_data.json"]
            .iter()
            .map(|n| fs::read(cache.join(n)).unwrap())
            .collect();
        write_list(&cars_dir, &found, &found_models).unwrap();
        let after: Vec<Vec<u8>> = ["cargroups.json", "carmodels.json", "cars.json", "cars_data.json"]
            .iter()
            .map(|n| fs::read(cache.join(n)).unwrap())
            .collect();
        assert_eq!(before, after);
        fs::remove_dir_all(&root).unwrap();
    }

    /// Cars added before the group and drive train that lack them take them from the list's entries: only matching
    /// packs without values change, and the list is not rewritten when nothing changed.
    #[test]
    fn missing_groups_and_drives_are_backfilled_from_the_list() {
        let root = crate::test_dir("car-backfill");
        let cars_dir = root.join("cars");
        let model = rsf::Model { id: 7, name: "m".into(), path: "m".into(), filename: "m".into(), fallback: 5 };
        let car = |id, pack: &str, group: Option<&str>, drive: Option<&str>| rsf::Car {
            id,
            name: format!("{pack} car"),
            pack: pack.into(),
            group: group.map(str::to_owned),
            model: 7,
            revision: String::new(),
            audio: None,
            drive_train: drive.map(str::to_owned),
        };
        write_list(
            &cars_dir,
            &[
                car(1, "BMW M1 GrpB", None, None),
                car(2, "Fiesta", Some("Group B"), None),
                car(3, "Ghost", None, None),
                car(4, "evo_IX", Some("WRC 1.6"), Some("4WD")),
            ],
            &[model],
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
        let found = list(&cars_dir).0;
        let group = |pack: &str| found.iter().find(|c| c.pack == pack).unwrap().group.clone();
        let drive = |pack: &str| found.iter().find(|c| c.pack == pack).unwrap().drive_train.clone();
        assert_eq!((group("BMW M1 GrpB").as_deref(), drive("BMW M1 GrpB").as_deref()), (Some("Group B"), Some("RWD")));
        assert_eq!((group("Fiesta").as_deref(), drive("Fiesta").as_deref()), (Some("Group B"), Some("FWD")));
        assert_eq!((group("Ghost").as_deref(), drive("Ghost").as_deref()), (None, None));
        assert_eq!((group("evo_IX").as_deref(), drive("evo_IX").as_deref()), (Some("WRC 1.6"), Some("4WD")));
        // Nothing left to fill: no rewrite.
        let cache = cars_dir.join("rsfdata").join("cache");
        let before: Vec<Vec<u8>> = ["cargroups.json", "cars.json", "cars_data.json"]
            .iter()
            .map(|n| fs::read(cache.join(n)).unwrap())
            .collect();
        assert!(!backfill(&cars_dir, &entries).unwrap());
        let after: Vec<Vec<u8>> = ["cargroups.json", "cars.json", "cars_data.json"]
            .iter()
            .map(|n| fs::read(cache.join(n)).unwrap())
            .collect();
        assert_eq!(before, after);
        fs::remove_dir_all(&root).unwrap();
    }

    fn retail() -> rbr_formats::cars::CarEntry {
        rbr_formats::cars::CarEntry {
            index: 5,
            file_name: "Cars\\s_i2003\\s_i2003.sgc".into(),
            ini_file: String::new(),
            shader_file: String::new(),
            shader_settings: String::new(),
            texture_path: String::new(),
        }
    }
}
