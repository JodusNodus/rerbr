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

impl Install {
    /// The RSF install at `root` (the game folder holding `Cars/` and `rsfdata/`).
    pub fn open(root: &Path) -> Result<Install, String> {
        let cache = root.join("rsfdata").join("cache");
        let groups: Vec<(u32, String)> = json(&cache.join("cargroups.json"))
            .unwrap_or_default()
            .iter()
            .filter_map(|g| Some((number(g, "id")?, text(g, "name"))))
            .collect();
        let drive_trains = drive_trains(&json(&cache.join("cars_data.json")).unwrap_or_default());
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
                let retail = retail
                    .iter()
                    .find(|e| e.index == model.fallback as usize)
                    .ok_or(format!("Cars.ini: no [Car{:02}]", model.fallback))?;
                why.push(format!("retail car {}", retail.index));
                return Ok(car_pack::Model {
                    entry: CarEntry { index: car_pack::SLOT as usize, ..retail.clone() },
                    fallback: Some(why.join(", ")),
                });
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
}
