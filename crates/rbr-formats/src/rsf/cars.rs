//! RallySimFans (RSF) car installs: the NGP car packs and the car models the RSF plugin puts into the game's slots.
//!
//! The packs and models themselves are [`crate::car_pack`]: RSF copies the chosen car's physics pack into one
//! slot's physics folder and rewrites that slot's `[CarNN]` in `Cars/Cars.ini` to the car's model folder; we read
//! the pack and the model in place instead ([`Install::model_entry`]); nothing is written.
//!
//! The car list comes from RSF's cache (`rsfdata/cache/`): `cars.json` (id, name, pack folder, model id, base group),
//! `carmodels.json` (model folder, file name, fallback) and `cargroups.json` (group names).

use super::json::{rows, Row};
use crate::car_pack::{self, Pack};
use crate::cars::{self, CarEntry};
use crate::data::Data;
use crate::ini::Section;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// A car of RSF's `cars.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Car {
    pub id: u32,
    pub name: String,
    /// The physics pack's folder name under `rsfdata/cars/` (`Mitsubishi_Lancer_Evo_IX_N4_ngp6`).
    pub pack: String,
    /// The base group's name (`Group N`), when `cargroups.json` has it.
    pub group: Option<String>,
    /// The `carmodels.json` id of its model.
    pub model: u32,
    /// `rev` (`8 / 2019-10-08`).
    pub revision: String,
    /// `audio`: its FMOD bank's name (`Lancer_Evo_IX_X`, `AudioFMOD/<audio>.bank`), when it has one.
    pub audio: Option<String>,
    /// `cars_data.json` `drive_train` (`FWD`, `RWD`, `AWD with torque split`), when the file has the car.
    pub drive_train: Option<String>,
    /// `cars_data.json` `year` (`2019`), when the file has the car and a year for it.
    pub year: Option<u32>,
}

/// A model of RSF's `carmodels.json`: `Cars/<path>/<filename>.sgc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub id: u32,
    pub name: String,
    pub path: String,
    pub filename: String,
    /// Below 8 a retail slot, else another model's id (ours, from the values: Evo IX falls back to 3, the Lancer
    /// Evo VII; the Focus 2006 to model 16). Used when the model folder is not installed.
    pub fallback: u32,
}

/// An RSF install's car list.
#[derive(Debug, Clone)]
pub struct Install {
    pub root: PathBuf,
    pub cars: Vec<Car>,
    pub models: Vec<Model>,
}

/// RSF's cache file at `path`: an array of flat objects (the only shape its `cars`, `carmodels` and `cargroups` files
/// have).
fn json(path: &Path) -> Result<Vec<Row>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(rows(&text))
}

fn text(row: &Row, key: &str) -> String {
    row.get(key).cloned().unwrap_or_default()
}

fn number(row: &Row, key: &str) -> Option<u32> {
    row.get(key)?.parse().ok()
}

/// The `drive_train` of each car of `cars_data.json`'s rows, by car id.
fn drive_trains(rows: &[Row]) -> HashMap<u32, String> {
    let known = |d: &Row| Some((number(d, "car_id")?, d.get("drive_train").filter(|t| !t.is_empty())?.clone()));
    rows.iter().filter_map(known).collect()
}

/// The `year` of each car of `cars_data.json`'s rows, by car id.
fn years(rows: &[Row]) -> HashMap<u32, u32> {
    rows.iter().filter_map(|d| Some((number(d, "car_id")?, number(d, "year")?))).collect()
}

impl Install {
    /// The RSF install at `root` (the game folder holding `Cars/` and `rsfdata/`).
    pub fn open(root: &Path) -> Result<Install, String> {
        let cache = root.join("rsfdata").join("cache");
        let groups: Vec<(u32, String)> = json(&cache.join("cargroups.json"))
            .unwrap_or_default()
            .iter()
            .filter_map(|g| Some((number(g, "id")?, text(g, "name"))))
            .collect();
        let cars_data = json(&cache.join("cars_data.json")).unwrap_or_default();
        let (drive_trains, years) = (drive_trains(&cars_data), years(&cars_data));
        let cars = json(&cache.join("cars.json"))?
            .iter()
            .filter_map(|c| {
                let group = number(c, "base_group_id").and_then(|id| groups.iter().find(|g| g.0 == id));
                Some(Car {
                    id: number(c, "id")?,
                    name: text(c, "name"),
                    pack: text(c, "path"),
                    group: group.map(|g| g.1.clone()),
                    model: number(c, "carmodel_id")?,
                    revision: text(c, "rev"),
                    audio: c.get("audio").filter(|a| !a.is_empty()).cloned(),
                    drive_train: number(c, "id").and_then(|id| drive_trains.get(&id).cloned()),
                    year: number(c, "id").and_then(|id| years.get(&id).copied()),
                })
            })
            .collect();
        let models = json(&cache.join("carmodels.json"))?
            .iter()
            .filter_map(|m| {
                Some(Model {
                    id: number(m, "id")?,
                    name: text(m, "name"),
                    path: text(m, "path"),
                    filename: text(m, "filename"),
                    fallback: number(m, "fallback").unwrap_or(0),
                })
            })
            .collect();
        Ok(Install { root: root.to_owned(), cars, models })
    }

    /// The car `key` names: its id or its pack folder (ignoring case).
    pub fn find(&self, key: &str) -> Option<&Car> {
        let id: Option<u32> = key.parse().ok();
        self.cars.iter().find(|c| Some(c.id) == id || c.pack.eq_ignore_ascii_case(key))
    }

    /// The car whose pack is at `path` (`rsfdata/cars/<pack>` or `<pack>.zip`).
    pub fn find_pack(&self, path: &Path) -> Option<&Car> {
        let name = path.file_name()?.to_str()?;
        self.find(name.strip_suffix(".zip").unwrap_or(name))
    }

    /// Car `car`'s FMOD bank (`AudioFMOD/<audio>.bank`, `crate::fmod`), when it has one and it is installed.
    pub fn audio_bank(&self, car: &Car) -> Option<PathBuf> {
        cars::resolve(&self.root, &format!("AudioFMOD\\{}.bank", car.audio.as_deref()?))
    }

    /// Car `car`'s physics pack: the unpacked folder, else the zip.
    pub fn pack(&self, car: &Car) -> Result<Pack, String> {
        let cars = self.root.join("rsfdata").join("cars");
        cars::resolve(&cars, &car.pack)
            .or_else(|| cars::resolve(&cars, &format!("{}.zip", car.pack)))
            .ok_or(format!("{}: no pack in {}", car.pack, cars.display()))
            .and_then(|p| Pack::open(&p))
    }

    /// The model car `car` is loaded with in [`car_pack::SLOT`]: its own when its folder is installed, else its
    /// fallback's (a retail car of `retail`, the install's `Cars.ini`), as `[CarNN]` paths under [`Install::root`]
    /// or the install.
    pub fn model_entry(&self, car: &Car, retail: &[CarEntry]) -> Result<car_pack::Model, String> {
        let mut id = car.model;
        let mut why: Vec<String> = Vec::new();
        for _ in 0..8 {
            let model =
                self.models.iter().find(|m| m.id == id).ok_or(format!("car model {id}: not in carmodels.json"))?;
            let entry = car_pack::model_entry(&model.path, &model.filename);
            if let Some(path) = cars::resolve(&self.root, &entry.file_name) {
                let locked = path.parent().map_or(0, encrypted_files);
                if locked > 0 {
                    return Err(format!("Cars/{}: {locked} RSF-encrypted files", model.path));
                }
                let fallback = (!why.is_empty()).then(|| why.join(", "));
                return Ok(car_pack::Model { entry, fallback });
            }
            why.push(format!("Cars/{} not installed", model.path));
            if model.fallback < 8 {
                why.push(format!("retail car {}", model.fallback));
                return car_pack::stand_in(retail, model.fallback as usize, why.join(", "));
            }
            id = model.fallback;
        }
        Err(format!("car model {}: fallback loop", car.model))
    }
}

/// The car-parts option of model folder `model` that switches the headlights on: the first by folder name of
/// `carparts/<option>/` whose `metadata.ini` names it `Headlights` (`Lights` on the BMW M3 E36) in `[rsf_info]` with
/// an `on` word in its `state` (`on`, `on+`, `All on`, `On, inner off`), else a `light_on/` folder beside the model
/// (the Fiesta Rally3 Evo, which has no `carparts/`). RSF's picker copies the chosen option's files over the model's;
/// we only read this one's, for the headlights' colour and points (`notes/formats.md` "RSF cars").
pub fn headlights_on(model: &Path) -> Option<PathBuf> {
    let mut options: Vec<PathBuf> = cars::resolve(model, "carparts")
        .and_then(|parts| std::fs::read_dir(parts).ok())
        .into_iter()
        .flatten()
        .filter_map(|e| Some(e.ok()?.path()))
        .collect();
    options.sort();
    let on = options.into_iter().find(|option| {
        option_info(option).is_some_and(|(name, state)| {
            ["Headlights", "Lights"].iter().any(|n| name.eq_ignore_ascii_case(n))
                && state.split(|c: char| !c.is_ascii_alphabetic()).any(|word| word.eq_ignore_ascii_case("on"))
        })
    });
    on.or_else(|| cars::resolve(model, "light_on").filter(|folder| folder.is_dir()))
}

/// A car-parts option's `name` and `state` (`metadata.ini`, `[rsf_info]`).
pub fn option_info(option: &Path) -> Option<(String, String)> {
    let text = std::fs::read(cars::resolve(option, "metadata.ini")?).ok()?;
    let sections = crate::ini::parse(&String::from_utf8_lossy(&text)).ok()?;
    let info = crate::car_parts::section(&sections, "rsf_info")?;
    Some((info.get("name")?.trim().to_owned(), info.get("state")?.trim().to_owned()))
}

/// A visual option folder of a car model: an RSF `carparts/<option>/` folder with its `metadata.ini`
/// `[rsf_info]` name and state, or a bare folder beside the model holding `.ini` files (the added pack cars'
/// `lights_on/`, `Close Roof/`, no metadata; the Fiesta Rally3 Evo's `light_on/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOption {
    /// The option's folder.
    pub dir: PathBuf,
    /// Its `[rsf_info]` name, or the folder's name for a bare option.
    pub name: String,
    /// Its `[rsf_info]` state, or empty for a bare option.
    pub state: String,
    /// Whether it has no `metadata.ini`.
    pub bare: bool,
}

/// Folders beside the model that are never bare options: the option store itself, and folders of textures.
const NON_OPTION_FOLDERS: [&str; 4] = ["carparts", "textures", "skins", "carplate"];

/// The model's option folders, sorted by folder name: the `carparts/` options with `metadata.ini`, else the
/// bare folders beside the model holding `.ini` files (never both: a model with `carparts/` leaves its
/// top-level preset folders alone).
///
/// Only the light ones matter for a night drive ([`night_options`]); the rest (mudflaps, plates, mirrors,
/// wheels, bumpers) always stay as installed.
pub fn model_options(model: &Path) -> Vec<ModelOption> {
    let mut options: Vec<ModelOption> = cars::resolve(model, "carparts")
        .and_then(|parts| std::fs::read_dir(parts).ok())
        .into_iter()
        .flatten()
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|dir| dir.is_dir() && cars::resolve(dir, "metadata.ini").is_some())
        .filter_map(|dir| option_info(&dir).map(|(name, state)| ModelOption { dir, name, state, bare: false }))
        .collect();
    if !options.is_empty() {
        options.sort_by(|a, b| a.dir.cmp(&b.dir));
        return options;
    }
    let mut bare: Vec<ModelOption> = std::fs::read_dir(model)
        .into_iter()
        .flatten()
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|dir| {
            dir.is_dir()
                && dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| !NON_OPTION_FOLDERS.iter().any(|x| n.eq_ignore_ascii_case(x)))
                && has_ini_files(dir)
        })
        .map(|dir| ModelOption {
            name: dir.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned(),
            dir,
            state: String::new(),
            bare: true,
        })
        .collect();
    bare.sort_by(|a, b| a.dir.cmp(&b.dir));
    bare
}

/// Whether `dir` holds `.ini` files directly below it.
fn has_ini_files(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_str().is_some_and(|n| n.to_lowercase().ends_with(".ini")))
    })
}

/// Whether the option group `name` is a front light (`Headlights`, `Extralights`, `Bonnet extralights`,
/// `Bumper lights`, `Safari lights`, `Foglights`, `Winglights`, ...). Rear and brake lights stay as installed.
fn is_light_group(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains("light") && !["rear", "brake", "tail"].iter().any(|w| name.contains(w))
}

/// The night pick from bare option folders (no `metadata.ini` to group by): the folder with an `on`
/// token and no `off` one, preferring one packing extra lights (`lights on (with extralights)` over
/// `lights on`); of several, the first by name.
fn pick_bare(bare: &[ModelOption], pick: &mut NightPick) {
    let light: Vec<&ModelOption> = bare.iter().filter(|o| is_light_group(&o.name)).collect();
    let on: Vec<&ModelOption> =
        light.iter().filter(|o| folder_has_word(&o.name, "on") && !folder_has_word(&o.name, "off")).copied().collect();
    let full = on.iter().find(|o| {
        ["extra", "pod", "ramp", "all", "bonnet", "bumper"].iter().any(|w| o.name.to_lowercase().contains(w))
    });
    match full.or(on.first()) {
        Some(option) => pick.headlights.push(option.dir.clone()),
        None if !light.is_empty() => pick.skipped.push("bare options: no on variant".to_owned()),
        None => {}
    }
}

/// An option's folder name.
fn option_folder(option: &ModelOption) -> &str {
    option.dir.file_name().and_then(|n| n.to_str()).unwrap_or_default()
}

/// Whether `text` holds `word` as a whole word (letters only, ignoring ASCII case): `on` in `All on` and
/// `on+`, but not in `bonnet` or `Not mounted`.
fn has_word(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphabetic()).any(|w| w.eq_ignore_ascii_case(word))
}

/// Whether the folder name holds `word` as a whole token (letters and digits, ignoring ASCII case): `on` in
/// `headlights_on` and `lights on (with extralights)`, but not in `bonnet` or `bumperon`.
fn folder_has_word(folder: &str, word: &str) -> bool {
    folder.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| w.eq_ignore_ascii_case(word))
}

/// The light options a night drive applies to the model in `model`: the switched-on option of each light
/// group (headlights on, plus extra lights and light pods where the car has them), in the order applied.
/// A stage by day keeps the installed parts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NightPick {
    /// The selected "on" option of the headlights group, if any (never more than one).
    pub headlights: Vec<PathBuf>,
    /// The selected "on" options of the other light groups (extra lights, pods), sorted.
    pub extras: Vec<PathBuf>,
    /// Light groups left as installed, each with why (no on variant, or not an on/off group).
    pub skipped: Vec<String>,
}

impl NightPick {
    /// Every selected option folder, headlights first.
    pub fn options(&self) -> Vec<PathBuf> {
        self.headlights.iter().chain(&self.extras).cloned().collect()
    }
}

/// The model's [`NightPick`]: one "on" option per light group of [`model_options`]. Mudflaps, plates,
/// mirrors, wheels, bumpers and any other group stay as installed and are never picked.
///
/// A group with several on variants takes the full one (`All on`, `Mounted, all on`, `Full`); a bare `On`
/// over a qualified one (`On` over `On (Safari)`); a headlights group the high beam (`highbeam`, `On, high
/// beam`, the headlight key's default). Of variants still tied (the quattro A2's outer and inner pods, its
/// 1983 and 1984 Africa lights) the first by folder name (user, 2026-11-05: "just take whatever").
pub fn night_options(model: &Path) -> NightPick {
    let (bare, meta): (Vec<ModelOption>, Vec<ModelOption>) = model_options(model).into_iter().partition(|o| o.bare);
    let mut pick = NightPick::default();
    if !bare.is_empty() {
        pick_bare(&bare, &mut pick);
        return pick;
    }
    let mut groups: BTreeMap<String, Vec<ModelOption>> = BTreeMap::new();
    for option in meta.into_iter().filter(|o| is_light_group(&o.name)) {
        groups.entry(option.name.to_lowercase()).or_default().push(option);
    }
    for (group, options) in &groups {
        // The on candidates: the `on` states, else the folders claiming `on` of an on/off group.
        let mut candidates: Vec<&ModelOption> = options.iter().filter(|o| has_word(&o.state, "on")).collect();
        if candidates.is_empty() {
            if !options.iter().any(|o| has_word(&o.state, "off")) {
                pick.skipped.push(format!("{group}: not an on/off group"));
                continue;
            }
            candidates = options
                .iter()
                .filter(|o| {
                    let folder = option_folder(o);
                    folder_has_word(folder, "on") && !folder_has_word(folder, "off")
                })
                .collect();
            if candidates.is_empty() {
                pick.skipped.push(format!("{group}: no on variant"));
                continue;
            }
        }
        let all: Vec<&&ModelOption> = candidates
            .iter()
            .filter(|o| o.state.to_lowercase().contains("all") || has_word(&o.state, "full"))
            .collect();
        let bare: Vec<&&ModelOption> = candidates.iter().filter(|o| o.state.to_lowercase() == "on").collect();
        let high: Vec<&&ModelOption> = if group.contains("head") && all.is_empty() && bare.is_empty() {
            candidates
                .iter()
                .filter(|o| option_folder(o).to_lowercase().contains("high") || o.state.to_lowercase().contains("high"))
                .collect()
        } else {
            Vec::new()
        };
        let narrowest = [&all, &bare, &high].into_iter().find(|set| !set.is_empty());
        let chosen = narrowest.map_or(candidates[0], |set| *set[0]);
        let list = if group.contains("head") { &mut pick.headlights } else { &mut pick.extras };
        list.push(chosen.dir.clone());
    }
    pick.extras.sort();
    pick
}

/// An option folder's files: its `<model>.ini` and `<model>_settings.ini` replacements, as RSF's picker
/// takes them over the model's (in memory only; never written).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionFiles {
    /// The option's folder.
    pub dir: PathBuf,
    /// Its `<model>.ini` replacement's sections (empty without one).
    pub parts: Vec<Section>,
    /// Its `<model>_settings.ini` replacement's sections (empty without one).
    pub settings: Vec<Section>,
}

/// The `.ini` replacements of option folder `dir`: the single non-settings `.ini` (not `metadata.ini`) and
/// the single `_settings.ini`.
///
/// Only the model's `.ini` and `_settings.ini` are ever replaced: option folders hold nothing else
/// (`tests` assert every installed option file is one of those, a preview picture or `metadata.ini`).
pub fn read_option_files(dir: &Path) -> Result<OptionFiles, String> {
    let mut inis: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_lowercase().ends_with(".ini"))
        })
        .collect();
    inis.sort();
    inis.retain(|p| p.file_name().and_then(|n| n.to_str()).is_none_or(|n| !n.eq_ignore_ascii_case("metadata.ini")));
    let (settings, parts): (Vec<PathBuf>, Vec<PathBuf>) = inis
        .into_iter()
        .partition(|p| p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.to_lowercase().ends_with("_settings")));
    let one = |mut paths: Vec<PathBuf>, what: &str| -> Result<Option<PathBuf>, String> {
        if paths.len() > 1 {
            return Err(format!("{}: several {what} ({}))", dir.display(), paths.len()));
        }
        Ok(paths.pop())
    };
    let (settings, parts) = (one(settings, "_settings.ini")?, one(parts, "model .ini")?);
    let read = |path: Option<PathBuf>| -> Result<Vec<Section>, String> {
        path.map(|path| {
            std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display())).and_then(|text| {
                crate::ini::parse(&String::from_utf8_lossy(&text)).map_err(|e| format!("{}: {e}", path.display()))
            })
        })
        .transpose()
        .map(|sections| sections.unwrap_or_default())
    };
    Ok(OptionFiles { dir: dir.to_owned(), parts: read(parts)?, settings: read(settings)? })
}

/// A value with its whitespace runs collapsed, for comparing sections across an option's formatting.
fn collapsed(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether two sections hold the same entries: same keys (ignoring ASCII case) with the same values
/// (whitespace runs collapsed), whatever their order.
fn same_entries(a: &Section, b: &Section) -> bool {
    let entries = |s: &Section| {
        let mut entries: Vec<(String, String)> =
            s.entries.iter().map(|(k, v)| (k.to_lowercase(), collapsed(v))).collect();
        entries.sort();
        entries
    };
    entries(a) == entries(b)
}

/// The sections of `over` (an option's file) over `base` (the model's): matching names (ignoring ASCII
/// case) merged key by key, new sections appended. Sections an option repeats unchanged keep the model's
/// text, so a stale formatting never spreads.
pub fn overlay_sections(base: &[Section], over: &[Section]) -> Vec<Section> {
    let mut merged: Vec<Section> = base.to_vec();
    for section in over {
        match merged.iter_mut().find(|s| s.name.eq_ignore_ascii_case(&section.name)) {
            Some(base) if same_entries(base, section) => {}
            Some(base) => {
                for (key, value) in &section.entries {
                    match base.entries.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
                        Some(entry) => entry.1 = value.clone(),
                        None => base.entries.push((key.clone(), value.clone())),
                    }
                }
            }
            None => merged.push(section.clone()),
        }
    }
    merged
}

/// Whether the extra option's lamp points are all already in `merged_parts` unchanged: then it only
/// renumbers the headlights' lamps (the Fiesta Rally3's `extralights_on` over its `Lights_on`) and is
/// redundant.
fn adds_no_points(merged_parts: &[Section], extra: &OptionFiles) -> bool {
    extra
        .parts
        .iter()
        .filter(|s| s.name.to_lowercase().starts_with("point_"))
        .all(|s| crate::car_parts::section(merged_parts, &s.name).is_some_and(|have| same_entries(have, s)))
}

/// `option`'s sections over `parts` and `settings`, recording whose value each section key holds in
/// `seen`: two options setting the same key differently disagree.
fn overlay_checked(
    parts: &mut Vec<Section>,
    settings: &mut Vec<Section>,
    option: &OptionFiles,
    seen: &mut BTreeMap<(String, String), (String, PathBuf)>,
) -> Result<(), String> {
    for (target, sections) in [(parts, &option.parts), (settings, &option.settings)] {
        for section in sections {
            for (key, value) in &section.entries {
                let at = (section.name.to_lowercase(), key.to_lowercase());
                if let Some((have, from)) = seen.get(&at) {
                    if collapsed(have) != collapsed(value) {
                        return Err(format!(
                            "{} and {} disagree on [{}] {}",
                            from.display(),
                            option.dir.display(),
                            section.name,
                            key
                        ));
                    }
                } else {
                    seen.insert(at, (value.clone(), option.dir.clone()));
                }
            }
        }
        *target = overlay_sections(target, sections);
    }
    Ok(())
}

/// The model's `.ini` and `_settings.ini` with `options` (of [`night_options`], in order) over them. Two
/// selected options setting the same section's key differently disagree: the installed parts are kept and
/// why is returned.
pub fn merge_options(
    base_parts: &[Section],
    base_settings: &[Section],
    options: &[OptionFiles],
) -> Result<(Vec<Section>, Vec<Section>), String> {
    let mut parts = base_parts.to_vec();
    let mut settings = base_settings.to_vec();
    let mut seen: BTreeMap<(String, String), (String, PathBuf)> = BTreeMap::new();
    for option in options {
        overlay_checked(&mut parts, &mut settings, option, &mut seen)?;
    }
    Ok((parts, settings))
}

/// The model's `.ini` and `_settings.ini` with a [`NightPick`] over them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MergedNight {
    /// The merged `.ini` sections.
    pub parts: Vec<Section>,
    /// The merged `_settings.ini` sections.
    pub settings: Vec<Section>,
    /// Extras left out as redundant with the headlights, each with why.
    pub skipped: Vec<String>,
}

/// The model's `.ini` and `_settings.ini` with the [`NightPick`] over them: the headlights' option, then
/// each extra. An extra disagreeing with the headlights that adds no lamp point of its own only renumbers
/// them and is left out (returned, with why); any other disagreement keeps the installed parts.
pub fn merge_night_options(
    base_parts: &[Section],
    base_settings: &[Section],
    headlights: &[OptionFiles],
    extras: &[OptionFiles],
) -> Result<MergedNight, String> {
    let (mut parts, mut settings) = merge_options(base_parts, base_settings, headlights)?;
    let mut seen: BTreeMap<(String, String), (String, PathBuf)> = BTreeMap::new();
    for option in headlights {
        for sections in [&option.parts, &option.settings] {
            for section in sections {
                for (key, value) in &section.entries {
                    seen.insert((section.name.to_lowercase(), key.to_lowercase()), (value.clone(), option.dir.clone()));
                }
            }
        }
    }
    let mut skipped = Vec::new();
    for extra in extras {
        let mut trial_parts = parts.clone();
        let mut trial_settings = settings.clone();
        let mut trial_seen = seen.clone();
        match overlay_checked(&mut trial_parts, &mut trial_settings, extra, &mut trial_seen) {
            Ok(()) => {
                (parts, settings, seen) = (trial_parts, trial_settings, trial_seen);
            }
            Err(conflict) if adds_no_points(&parts, extra) => {
                skipped.push(format!("{}: {conflict}", extra.dir.display()));
            }
            Err(conflict) => return Err(conflict),
        }
    }
    Ok(MergedNight { parts, settings, skipped })
}

/// The RSF-encrypted files under `dir`, counted by their first four bytes.
fn encrypted_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .map(|p| match p.is_dir() {
            true => encrypted_files(&p),
            false => {
                let mut head = [0; 4];
                let read = std::fs::File::open(&p).and_then(|mut f| f.read_exact(&mut head));
                usize::from(read.is_ok() && car_pack::encrypted(&head))
            }
        })
        .sum()
}

/// An RSF car chosen for a race: the car, its pack, its model, and the data with the model in its slot.
#[derive(Debug, Clone)]
pub struct Selected {
    /// The RSF install's folder.
    pub root: PathBuf,
    pub car: Car,
    pub pack: Pack,
    pub model: car_pack::Model,
    /// The skin RSF's launcher selected for the model ([`car_pack::selected_skin`]), drawn over its own textures.
    pub skin: Option<String>,
    pub data: Data,
    /// Its FMOD bank ([`Install::audio_bank`]).
    pub audio_bank: Option<PathBuf>,
    /// The tyre brand whose `Physics/<brand>/tyres.lsp` it runs on: Pirelli's (see [`car_pack::resolve`]).
    pub tyre_brand: &'static str,
}

impl Selected {
    /// The model's folder (`evo_IX`), the viewer's car name.
    pub fn folder(&self) -> &str {
        self.model.entry.file_name.split(['\\', '/']).rev().nth(1).unwrap_or_default()
    }

    /// One line for the log: the car, its group, the pack and the model (with its fallback).
    pub fn describe(&self) -> String {
        format!(
            "rsf car {} {} ({}), pack {}, model {}{}{}",
            self.car.id,
            self.car.name,
            self.car.group.as_deref().unwrap_or("no group"),
            self.pack.path().display(),
            self.model.entry.file_name,
            self.model.fallback.as_deref().map(|f| format!(" (fallback: {f})")).unwrap_or_default(),
            self.skin.as_deref().map(|s| format!(", skin {s}")).unwrap_or_default()
        )
    }
}

/// Car `key` (an id or pack folder of the RSF install `root`) in [`car_pack::SLOT`] of `data`: what RSF would
/// install, read in place, through [`car_pack::resolve`].
pub fn select(data: Data, key: &str, root: Option<&Path>) -> Result<Selected, String> {
    let root = root.ok_or(format!("{key}: not an id or pack, and no RSF install (RBR_RSF) to find it in"))?;
    let install = Install::open(root)?;
    let car = install.find(key).ok_or(format!("{key}: not an id or pack in {}'s cars.json", root.display()))?.clone();
    let pack = install.pack(&car)?;
    let ini = data.resolve("Cars/Cars.ini").ok_or("Cars/Cars.ini: not found")?;
    let text = std::fs::read(&ini).map_err(|e| format!("{}: {e}", ini.display()))?;
    let retail = cars::parse(&String::from_utf8_lossy(&text)).map_err(|e| e.to_string())?;
    let model = install.model_entry(&car, &retail)?;
    let resolved =
        car_pack::resolve(data, pack.clone(), model.clone(), install.root.clone(), install.audio_bank(&car))?;
    Ok(Selected {
        root: install.root.clone(),
        car,
        pack,
        model,
        skin: resolved.skin,
        data: resolved.data,
        audio_bank: resolved.bank,
        tyre_brand: resolved.tyre_brand,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cars_drive_train_is_in_cars_data() {
        let data = rows(
            r#"[{"car_id":"8","drive_train":"AWD with torque split"},{"car_id":"9","drive_train":""},{"car_id":"10"}]"#,
        );
        assert_eq!(drive_trains(&data), HashMap::from([(8, "AWD with torque split".to_owned())]));
    }

    #[test]
    fn a_cars_year_is_in_cars_data() {
        let data = rows(r#"[{"car_id":"8","year":"2014"},{"car_id":"9","year":""},{"car_id":"10"}]"#);
        assert_eq!(years(&data), HashMap::from([(8, 2014)]));
    }

    /// Every pack of `RBR_RSF` (an RSF install, or the samples) reads, and each cache car finds its pack or says why.
    #[test]
    fn rsf_install_cars_and_packs() {
        let Some(root) = std::env::var_os("RBR_RSF") else {
            eprintln!("skipping: RBR_RSF not set");
            return;
        };
        let install = Install::open(Path::new(&root)).unwrap();
        assert!(!install.cars.is_empty());
        let mut packs = 0;
        for car in &install.cars {
            let Ok(pack) = install.pack(car) else { continue };
            packs += 1;
            for file in ["common.lsp", "gravel.lsp", "tarmac.lsp", "snow.lsp", "setups/d_gravel.lsp", "r_gravel.lsp"] {
                pack.read(file).unwrap_or_else(|e| panic!("{}: {e}", car.pack));
            }
            assert!(pack.title().is_some(), "{}", car.pack);
            let zip = install.root.join("rsfdata").join("cars").join(format!("{}.zip", car.pack));
            if let (Pack::Dir(_), true) = (&pack, zip.is_file()) {
                let zip = Pack::open(&zip).unwrap();
                assert_eq!(zip.read("common.lsp").unwrap(), pack.read("common.lsp").unwrap(), "{}", car.pack);
                assert_eq!(zip.title(), pack.title(), "{}", car.pack);
            }
        }
        assert!(packs > 0, "no pack under rsfdata/cars");
        if install.root.join("rsfdata/cache/cars_data.json").is_file() {
            assert!(install.cars.iter().any(|c| c.year.is_some()), "no car of cars.json has a year in cars_data.json");
        }
    }

    /// The samples' Fiesta Rally3 has `.rsfconfig` selecting `Pajari_Monte`: its six textures come from that skin, the
    /// rest from the model's `Textures\`. The BMW M3 E30 has no `.rsfconfig` and keeps its own.
    #[test]
    fn the_selected_skin_draws_over_the_model() {
        let (Some(root), Some(data)) = (std::env::var_os("RBR_RSF"), Data::from_env()) else {
            eprintln!("skipping: RBR_RSF or RBR_DATA not set");
            return;
        };
        let root = Path::new(&root);
        let fiesta = select(data.clone(), "Ford_Fiesta_Rally3_ngp6", Some(root)).unwrap();
        assert_eq!(fiesta.skin.as_deref(), Some("Pajari_Monte"));
        let dir = |path: &str| fiesta.data.resolve(path).unwrap().parent().unwrap().to_owned();
        let skin = cars::resolve(root, "Cars\\Fiesta_rally3\\skins\\Pajari_Monte\\textures").unwrap();
        assert_eq!(dir("Cars\\Fiesta_rally3\\Textures\\Fiesta_rally3_external.dds"), skin);
        let own = cars::resolve(root, "Cars\\Fiesta_rally3\\Textures").unwrap();
        assert_eq!(dir("Cars\\Fiesta_rally3\\Textures\\Fiesta_rally3_internal_parts.dds"), own);
        let bmw = select(data, "BMW_M3_E30_GrpA_ngp6", Some(root)).unwrap();
        assert_eq!(bmw.skin, None);
    }

    /// Every car-parts option of the samples' models has its `[rsf_info]` name and state, and the lights-off models
    /// find their headlights' "on" option.
    #[test]
    fn car_parts_options_and_the_headlights_on() {
        let Some(root) = std::env::var_os("RBR_RSF") else {
            eprintln!("skipping: RBR_RSF not set");
            return;
        };
        let cars = cars::resolve(Path::new(&root), "Cars").unwrap();
        let mut options = 0;
        for model in std::fs::read_dir(&cars).unwrap().map(|e| e.unwrap().path()) {
            let Some(parts) = cars::resolve(&model, "carparts") else { continue };
            for option in std::fs::read_dir(parts).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()) {
                if cars::resolve(&option, "metadata.ini").is_some() {
                    assert!(option_info(&option).is_some(), "{}", option.display());
                    options += 1;
                }
            }
        }
        assert!(options > 0, "no car-parts options under {}", cars.display());
        for (model, on) in [
            ("FORD_FOCUS_2006", "lights_on"),
            ("fiesta_wrc_19", "headlights_on"),
            ("delta_4wd", "lights_on_all"),
            ("ESCORT_COSWORTH_GRA", "lights_on"),
            ("bmw_m3_e30", "Lights_On"),
            ("evo_IX", "light_on"),
            ("bmw_m3_e36", "lights_on"),
            ("fiesta_rally3_evo", "light_on"),
        ] {
            let found = headlights_on(&cars::resolve(&cars, model).unwrap()).unwrap();
            assert_eq!(found.file_name().unwrap(), on, "{model}");
        }
    }

    /// A fresh model folder for one test, with a file at each `(path, contents)` pair.
    fn model_tree(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("car-options-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, contents) in files {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().expect("parent")).expect("dirs");
            std::fs::write(file, contents).expect("write");
        }
        root
    }

    #[test]
    fn overlay_merges_sections_key_by_key() {
        let base = crate::ini::parse("[a]\r\nx = 1\r\ny = 2\r\n[b]\r\nz = 3\r\n").unwrap();
        let over = crate::ini::parse("[A]\r\nX = 1\r\ny = 20\r\nw = 4\r\n[c]\r\nv = 5\r\n").unwrap();
        let merged = overlay_sections(&base, &over);
        assert_eq!(merged.len(), 3);
        // Unchanged sections keep the model's text; changed ones take the option's keys.
        let a = crate::car_parts::section(&merged, "a").unwrap();
        assert_eq!(
            a.entries,
            [("x".to_owned(), "1".to_owned()), ("y".to_owned(), "20".to_owned()), ("w".to_owned(), "4".to_owned())]
        );
        assert_eq!(merged[1].name, "b");
        assert_eq!(crate::car_parts::section(&merged, "c").unwrap().entries.len(), 1);
        // Whitespace-only differences are no change.
        let spaced = crate::ini::parse("[a]\r\nx=1\r\ny=2\r\n").unwrap();
        assert_eq!(overlay_sections(&base, &spaced)[0].entries, base[0].entries);
    }

    #[test]
    fn merge_options_rejects_disagreeing_options() {
        let base: Vec<Section> = Vec::new();
        let a = |entries: &[(&str, &str)]| Section {
            name: "e_light_lf".to_owned(),
            entries: entries.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        };
        let one =
            OptionFiles { dir: PathBuf::from("one"), parts: vec![a(&[("Predicate", "on")])], settings: Vec::new() };
        let same =
            OptionFiles { dir: PathBuf::from("two"), parts: vec![a(&[("Predicate", "on")])], settings: Vec::new() };
        assert!(merge_options(&base, &base, &[one.clone(), same]).is_ok());
        let other =
            OptionFiles { dir: PathBuf::from("two"), parts: vec![a(&[("Predicate", "off")])], settings: Vec::new() };
        assert!(merge_options(&base, &base, &[one, other]).is_err());
    }

    #[test]
    fn bare_options_pick_the_on_folder() {
        let root = model_tree(
            "bare",
            &[("light_on/light.ini", "[a]\r\nx = 1\r\n"), ("light_off/light.ini", "[a]\r\nx = 2\r\n")],
        );
        let pick = night_options(&root);
        assert_eq!(pick.options().len(), 1);
        assert!(pick.options()[0].ends_with("light_on"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn tied_on_variants_take_the_first_by_folder() {
        // As the quattro A2's car parts: two full pod variants, a winter pod, two Africa lights years.
        let option = |folder: &str, name: &str, state: &str| {
            (format!("carparts/{folder}/metadata.ini"), format!("[rsf_info]\r\nname={name}\r\nstate={state}\r\n"))
        };
        let files = [
            option("headlights_on", "Headlights", "Normal"),
            option("headlights_off", "Headlights", "Off"),
            option("extralights_all_on_outer", "Extralights", "All on, outer"),
            option("extralights_all_on_inner", "Extralights", "All on, inner"),
            option("extralights_winter_on", "Extralights", "Winter lightpod on"),
            option("extralights_no", "Extralights", "Not mounted"),
            option("africalights_on_84", "Africa lights", "Safari 1984, on"),
            option("africalights_on_83", "Africa lights", "Ivory 1983, on"),
        ];
        let files: Vec<(&str, &str)> = files.iter().map(|(p, c)| (p.as_str(), c.as_str())).collect();
        let root = model_tree("tied", &files);
        let names: Vec<String> = night_options(&root)
            .options()
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();
        assert_eq!(names, ["headlights_on", "africalights_on_83", "extralights_all_on_inner"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Every installed option file of `RBR_RSF` (an RSF install, or the samples) parses, holds only visual
    /// files, and merges over its model; every installed car without options needs none.
    #[test]
    fn night_options_cover_every_installed_car() {
        let Some(root) = std::env::var_os("RBR_RSF") else {
            eprintln!("skipping: RBR_RSF not set");
            return;
        };
        let cars = cars::resolve(Path::new(&root), "Cars").unwrap();
        // The expected "on" options (folder names) of models with an unambiguous night setup.
        let expected: &[(&str, &[&str])] = &[
            ("FORD_FOCUS_2006", &["lights_on", "lights_on_extralights"]),
            ("fiesta_wrc_19", &["headlights_on", "bonnet_extralights_on", "bumper_extralights_on"]),
            ("delta_4wd", &["lights_on_all", "extralights_on"]),
            ("ESCORT_COSWORTH_GRA", &["lights_on", "bonnet_extralights_on", "bumper_extralights_on"]),
            ("bmw_m3_e30", &["Lights_On", "bumperextralights_ON"]),
            ("evo_IX", &["light_on", "extralights_on"]),
            ("Fiesta_rally3", &["Lights_on", "extralights_on"]),
            ("fiesta_rally3_evo", &["light_on"]),
            ("OPEL_MANTA_400", &["headlights_highbeam", "extralights_on"]),
            ("YARIS_WRC18", &["light_on", "light_on_all", "light_on_bonnet"]),
            ("vw_golf_86", &["light_on_full", "light_on_ramp"]),
        ];
        let mut models = 0;
        let mut with_night = 0;
        for model in std::fs::read_dir(&cars).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()) {
            let name = model.file_name().unwrap().to_str().unwrap().to_owned();
            let options = model_options(&model);
            for option in &options {
                // Option folders hold the replacements, their metadata and a preview picture: visual only,
                // never physics (no `.lsp`) or textures.
                for file in std::fs::read_dir(&option.dir).unwrap().map(|e| e.unwrap().path()) {
                    let name = file.file_name().unwrap().to_str().unwrap().to_lowercase();
                    assert!(
                        name.ends_with(".ini") || name.ends_with(".jpg") || name.ends_with(".png"),
                        "{}",
                        file.display()
                    );
                }
                // Some options are guides with no replacement files (`animalguard_guide`).
                let files = read_option_files(&option.dir).unwrap();
                if files.parts.is_empty() && files.settings.is_empty() {
                    eprintln!("{}: option with no .ini", option.dir.display());
                }
            }
            let pick = night_options(&model);
            let names: Vec<String> =
                pick.options().iter().map(|p| p.file_name().unwrap().to_str().unwrap().to_owned()).collect();
            if let Some((_, want)) = expected.iter().find(|(m, _)| m == &name) {
                assert_eq!(names, *want, "{name}");
                with_night += 1;
            } else if !options.is_empty() {
                with_night += usize::from(!pick.options().is_empty());
            }
            if !pick.options().is_empty() || !pick.skipped.is_empty() {
                eprintln!(
                    "{name}: night {}{}",
                    pick.options()
                        .iter()
                        .map(|p| p.file_name().unwrap().to_str().unwrap().to_owned())
                        .collect::<Vec<_>>()
                        .join(", "),
                    if pick.skipped.is_empty() {
                        String::new()
                    } else {
                        format!(" skipped: {}", pick.skipped.join("; "))
                    }
                );
            }
            // The selection merges over the model without disagreeing.
            if !pick.options().is_empty() {
                let stem =
                    std::fs::read_dir(&model).unwrap().filter_map(Result::ok).map(|e| e.path()).find(|p| {
                        p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("sgc"))
                    });
                let (base_parts, base_settings) = stem
                    .map(|sgc| {
                        let stem = sgc.file_stem().unwrap().to_str().unwrap().to_owned();
                        let read = |suffix: &str| {
                            cars::resolve(&model, &format!("{stem}{suffix}"))
                                .and_then(|p| std::fs::read(p).ok())
                                .and_then(|text| crate::ini::parse(&String::from_utf8_lossy(&text)).ok())
                                .unwrap_or_default()
                        };
                        (read(".ini"), read("_settings.ini"))
                    })
                    .unwrap_or_default();
                let heads: Vec<OptionFiles> =
                    pick.headlights.iter().map(|dir| read_option_files(dir).unwrap()).collect();
                let extras: Vec<OptionFiles> = pick.extras.iter().map(|dir| read_option_files(dir).unwrap()).collect();
                let merged = merge_night_options(&base_parts, &base_settings, &heads, &extras)
                    .unwrap_or_else(|e| panic!("{name}: {e}"));
                // The Rally3's extras only renumber the headlights' pods: left out, reported.
                if name == "Fiesta_rally3" {
                    assert!(merged.skipped.iter().any(|s| s.contains("extralights_on")), "{:?}", merged.skipped);
                }
            }
            models += 1;
        }
        assert!(models > 0, "no car models under {}", cars.display());
        eprintln!("{models} installed models, {with_night} checked with a night option");
        // Every model the install has options for is either expected above or merges cleanly.
        for (model, want) in expected {
            assert!(cars::resolve(&cars, model).is_some(), "{model}: not installed");
            let _ = want;
        }
    }

    /// Retail cars carry no option folders and need no night option.
    #[test]
    fn retail_cars_have_no_options() {
        let Some(data) = Data::from_env() else {
            eprintln!("skipping: RBR_DATA not set");
            return;
        };
        let mut models = 0;
        for model in
            std::fs::read_dir(data.install().join("Cars")).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir())
        {
            assert_eq!(model_options(&model), Vec::new(), "{}", model.display());
            assert_eq!(night_options(&model), NightPick::default(), "{}", model.display());
            models += 1;
        }
        assert!(models > 0, "no retail cars");
    }
}
