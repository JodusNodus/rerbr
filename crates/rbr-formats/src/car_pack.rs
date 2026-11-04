//! An NGP car pack read in place: the physics pack and the model the game loads it with.
//!
//! What RSF does when a car is chosen (notes/formats.md "RSF cars"): it copies the car's physics pack
//! (`rsfdata/cars/<pack>/`, NGP keys in the retail `.lsp` layout) into one slot's physics folder (`Physics/s_i2003/`
//! for slot 5) and rewrites that slot's `[CarNN]` in `Cars/Cars.ini` to the car's model folder (`Cars/<model>/`, the
//! retail model layout). The game then loads the slot as a retail car. We read the pack and the model in place
//! instead; nothing is written. The same layout holds the packs the player added (`cars/` in the player's folder).
//!
//! [`Flags`] are the launcher's explicit inputs for a pack car (its pack, its model's `.sgc` path, its sound
//! bank and its saved key), parsed and checked once here and shared by the viewer, `rbr-standalone` and
//! `rbr-sound`. [`resolve`] turns them into what a race loads: the data with the model in its slot, the skin
//! over it, and the bank with its tyres.

use crate::cars::{self, CarEntry};
use crate::data::Data;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The game slot every pack car races in: `[Car05]`, so NGP's `_Car5` keys and the game's records see it there. The
/// setups saved before pack cars had keys of their own are offered to every pack car under this number too.
pub const SLOT: u32 = 5;

/// An NGP physics pack (`rsfdata/cars/<pack>/` or `<pack>.zip`, whose entries sit in a `<pack>/` folder, some zips
/// with `\` separators): the files
/// RSF copies into a slot's `Physics/<dir>/`: `common.lsp`, `<surface>.lsp`, `r_<surface>.lsp`,
/// `setups/d_<surface>.lsp`, `repair.ini` and a text file named after the car.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pack {
    Dir(PathBuf),
    Zip(PathBuf),
}

impl Pack {
    pub fn open(path: &Path) -> Result<Pack, String> {
        if path.is_dir() {
            Ok(Pack::Dir(path.to_owned()))
        } else if path.is_file() {
            Ok(Pack::Zip(path.to_owned()))
        } else {
            Err(format!("{}: no pack there", path.display()))
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Pack::Dir(p) | Pack::Zip(p) => p,
        }
    }

    /// The pack's file `name` (`setups/d_gravel.lsp`), matched ignoring case.
    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        let bytes = self.read_raw(name)?;
        match encrypted(&bytes) {
            true => Err(format!("{}/{name}: RSF-encrypted", self.path().display())),
            false => Ok(bytes),
        }
    }

    fn read_raw(&self, name: &str) -> Result<Vec<u8>, String> {
        let missing = || format!("{}: no {name}", self.path().display());
        match self {
            Pack::Dir(dir) => {
                let path = cars::resolve(dir, name).ok_or_else(missing)?;
                std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
            }
            Pack::Zip(path) => {
                let mut zip = crate::rbz::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
                let want = name.replace('\\', "/");
                let entry = (0..zip.len())
                    .find(|&k| {
                        zip.name_for_index(k)
                            .and_then(|n| {
                                Some(n.ok()?.replace('\\', "/").split_once('/')?.1.eq_ignore_ascii_case(&want))
                            })
                            .unwrap_or(false)
                    })
                    .ok_or_else(missing)?;
                let mut bytes = Vec::new();
                zip.by_index(entry)
                    .and_then(|mut f| f.read_to_end(&mut bytes).map_err(Into::into))
                    .map_err(|e| format!("{}/{name}: {e}", path.display()))?;
                Ok(bytes)
            }
        }
    }

    /// The pack's text file (named after the car): its first line, the car's full name.
    pub fn title(&self) -> Option<String> {
        Some(self.about()?.lines().next()?.trim().to_owned())
    }

    /// The pack's text file (named after the car): the name, `revision N / date`, the specification date and the 3D
    /// model's credit, a line each.
    pub fn about(&self) -> Option<String> {
        let names: Vec<String> = match self {
            Pack::Dir(dir) => {
                std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect()
            }
            Pack::Zip(path) => {
                let zip = crate::rbz::open(path).ok()?;
                zip.file_names()
                    .filter_map(|n| Some(n.ok()?.replace('\\', "/").split_once('/')?.1.to_owned()))
                    .collect()
            }
        };
        let known = |n: &str| [".lsp", ".ini"].iter().any(|e| n.to_ascii_lowercase().ends_with(e));
        let name = names.into_iter().find(|n| !n.is_empty() && !n.contains('/') && !known(n) && n != "setups")?;
        let text = self.read(&name).ok()?;
        Some(String::from_utf8_lossy(&text).into_owned())
    }
}

/// The model `--car-model` names: its `.sgc` path (absolute) split into the root holding its `Cars\\` folder and
/// its slot entry (`/root/Cars/evo/evo.sgc` loads `Cars\\evo\\evo.sgc`'s entry under `/root`). No search: the path
/// names the folder, and its parent's parent must be the `Cars` folder.
pub fn model_of(path: &Path) -> Result<(PathBuf, CarEntry), String> {
    let bad = || format!("--car-model {}: want the model's .sgc path, under a Cars folder", path.display());
    let file = path.file_name().and_then(|f| f.to_str()).ok_or_else(&bad)?;
    let stem = (file.len() > 4 && file[file.len() - 4..].eq_ignore_ascii_case(".sgc"))
        .then(|| &file[..file.len() - 4])
        .filter(|s| !s.is_empty())
        .ok_or_else(&bad)?;
    let folder = path.parent().and_then(|p| p.file_name()).and_then(|f| f.to_str()).ok_or_else(&bad)?;
    let cars = path.parent().and_then(|p| p.parent()).ok_or_else(&bad)?;
    if !cars.file_name().and_then(|f| f.to_str()).is_some_and(|c| c.eq_ignore_ascii_case("Cars")) {
        return Err(bad());
    }
    let root = cars.parent().filter(|r| !r.as_os_str().is_empty()).ok_or_else(&bad)?.to_owned();
    Ok((root, model_entry(folder, stem)))
}

/// The root a model's `.sgc` path resolves under: the folder holding its `Cars\\` folder
/// ([`model_of`]; `/root/Cars/evo/evo.sgc` resolves under `/root`).
pub fn root_of(path: &Path) -> Result<PathBuf, String> {
    model_of(path).map(|(root, _)| root)
}

/// The entry's file under `root`: its `Cars\\` path in native separators (`Cars\\evo\\evo.sgc` under `/root` is
/// `/root/Cars/evo/evo.sgc`).
pub fn file_under(root: &Path, entry: &str) -> PathBuf {
    let mut path = root.to_owned();
    path.extend(entry.split('\\'));
    path
}

/// The model a pack car is loaded with in [`SLOT`]: its `Cars.ini` entry, and, when its own model folder is not
/// installed, why it falls back to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub entry: CarEntry,
    pub fallback: Option<String>,
}

/// The `[CarNN]` paths a pack car with model folder `path` and file name `filename` is loaded with (`Cars.ini`'s
/// `[Car05]` in the sample: `Cars\evo_IX\evo_IX.sgc`, `.ini`, `_shaders.ini`, `_shader_settings`, `Textures\`).
pub fn model_entry(path: &str, filename: &str) -> CarEntry {
    let dir = format!("Cars\\{path}\\");
    CarEntry {
        index: SLOT as usize,
        file_name: format!("{dir}{filename}.sgc"),
        ini_file: format!("{dir}{filename}.ini"),
        shader_file: format!("{dir}{filename}_shaders.ini"),
        shader_settings: format!("{dir}{filename}_shader_settings"),
        texture_path: format!("{dir}Textures\\"),
    }
}

/// The retail stand-in: `retail` slot `slot`'s entry rewritten for the pack car's slot, with the reason `why`
/// (its own model folder is not installed).
pub fn stand_in(retail: &[CarEntry], slot: usize, why: String) -> Result<Model, String> {
    let retail = retail.iter().find(|e| e.index == slot).ok_or(format!("Cars.ini: no [Car{slot:02}]"))?;
    Ok(Model { entry: CarEntry { index: SLOT as usize, ..retail.clone() }, fallback: Some(why) })
}

/// The skin selected for model folder `folder` (`Cars/<folder>/.rsfconfig`, `[skin]`'s `folder`), and its folder
/// (`Cars/<folder>/skins/<skin>/`) when installed. Without a `.rsfconfig` the model's own `Textures\` are drawn.
/// Ours: the selected skin's files replacing the model's by name is inferred from the samples (the Fiesta Rally3's
/// skins hold 6 to 18 of its 18 textures, some its shader settings).
pub fn selected_skin(root: &Path, folder: &str) -> Option<(String, PathBuf)> {
    let config = cars::resolve(root, &format!("Cars\\{folder}\\.rsfconfig"))?;
    let text = std::fs::read(config).ok()?;
    let sections = crate::ini::parse(&String::from_utf8_lossy(&text)).ok()?;
    let skin = crate::car_parts::section(&sections, "skin")?.get("folder")?.trim().to_owned();
    let dir = cars::resolve(root, &format!("Cars\\{folder}\\skins\\{skin}"))?;
    dir.is_dir().then_some((skin, dir))
}

/// Whether `bytes` start like an RSF-protected file (`01 a0 00 00`, `.ini`s `01 a1 00 00`; lead survey of the RSF
/// stages). We never decrypt them: a car with one is unloadable.
pub fn encrypted(bytes: &[u8]) -> bool {
    matches!(bytes.get(..4), Some([0x01, 0xa0 | 0xa1, 0, 0]))
}

/// A pack car the launcher resolved: `--car-pack DIR --car-model PATH [--car-bank PATH] [--car-key KEY]`,
/// parsed and checked once here and shared by the viewer, `rbr-standalone` and `rbr-sound` (each binary forwards
/// its unknown flags here and calls [`Flags::check`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Flags {
    /// Its physics pack (`rsfdata/cars/<pack>/` or `<pack>.zip`).
    pub pack: Option<PathBuf>,
    /// Its model's `.sgc` path (absolute): the folder holding its `Cars\\` folder is the root it resolves under.
    pub model: Option<PathBuf>,
    /// Its FMOD bank, when it has one and it is installed.
    pub bank: Option<PathBuf>,
    /// The key its setups, camera nudges, FFB sensitivities and ghosts are saved under.
    pub key: Option<String>,
}

impl Flags {
    /// The help's fragment for these flags.
    pub const USAGE: &str = "--car-pack DIR --car-model PATH [--car-bank PATH] [--car-key KEY]";

    /// Whether `flag` is one of ours; takes its `value` when it is.
    pub fn take(&mut self, flag: &str, value: &str) -> bool {
        match flag {
            "--car-pack" => self.pack = Some(PathBuf::from(value)),
            "--car-model" => self.model = Some(PathBuf::from(value)),
            "--car-bank" => self.bank = Some(PathBuf::from(value)),
            "--car-key" => self.key = Some(value.to_owned()),
            _ => return false,
        }
        true
    }

    /// Why these flags do not combine with `rsf` (`--rsf-car` naming a car too), or `None` when they are a pack car
    /// (a pack with its model) or no car at all.
    pub fn check(&self, rsf: bool) -> Result<(), String> {
        if self.pack.is_some() && rsf {
            return Err("give one of --rsf-car and --car-pack".into());
        }
        if self.pack.is_some() && self.model.is_none() {
            return Err("--car-pack needs --car-model".into());
        }
        if self.pack.is_none() && (self.model.is_some() || self.bank.is_some() || self.key.is_some()) {
            return Err("--car-model, --car-bank and --car-key need --car-pack".into());
        }
        Ok(())
    }

    /// What a race loads for these flags: [`resolve`] of the pack, the model's `.sgc` path and the bank (`check`
    /// first: a pack without its model, or the other flags without a pack, do not load).
    pub fn resolve(&self, data: Data) -> Result<Resolved, String> {
        self.check(false)?;
        let pack = self.pack.as_deref().ok_or("--car-pack needs --car-model")?;
        let model = self.model.as_deref().ok_or("--car-pack needs --car-model")?;
        let (root, entry) = model_of(model)?;
        resolve(data, Pack::open(pack)?, Model { entry, fallback: None }, root, self.bank.clone())
    }

    /// These flags as arguments (the launcher's command, `rbr-sound`'s standalone): the pack, the model, the bank
    /// and the key, each only when set.
    pub fn argv(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(pack) = &self.pack {
            args.extend(["--car-pack".into(), pack.display().to_string()]);
        }
        if let Some(model) = &self.model {
            args.extend(["--car-model".into(), model.display().to_string()]);
        }
        if let Some(bank) = &self.bank {
            args.extend(["--car-bank".into(), bank.display().to_string()]);
        }
        if let Some(key) = &self.key {
            args.extend(["--car-key".into(), key.clone()]);
        }
        args
    }
}

/// A pack car chosen for a race: its pack, its model, and the data with the model in its slot.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub pack: Pack,
    pub model: Model,
    /// The skin selected for the model ([`selected_skin`]), drawn over its own textures.
    pub skin: Option<String>,
    pub data: Data,
    /// Its FMOD bank, when it has one and it is installed.
    pub bank: Option<PathBuf>,
    /// The tyre brand whose `Physics/<brand>/tyres.lsp` it runs on: Pirelli's, which the game gives its slot 5, where
    /// every pack car races (the packs have no tyre file of their own).
    pub tyre_brand: &'static str,
}

impl Resolved {
    /// The model's folder (`evo_IX`), the viewer's car name.
    pub fn folder(&self) -> &str {
        self.model.entry.file_name.split(['\\', '/']).rev().nth(1).unwrap_or_default()
    }

    /// One line for the log: the car, its group, the pack and the model (with its fallback).
    pub fn describe(&self, name: &str, group: Option<&str>) -> String {
        format!(
            "pack car {name} ({}), pack {}, model {}{}{}",
            group.unwrap_or("no group"),
            self.pack.path().display(),
            self.model.entry.file_name,
            self.model.fallback.as_deref().map(|f| format!(" (fallback: {f})")).unwrap_or_default(),
            self.skin.as_deref().map(|s| format!(", skin {s}")).unwrap_or_default()
        )
    }
}

/// What a race loads for a pack car: the data with `model` in [`SLOT`] under `model_root` (where its `Cars\` files
/// resolve), the selected skin over it, and `bank` with its tyres.
pub fn resolve(
    data: Data,
    pack: Pack,
    model: Model,
    model_root: PathBuf,
    bank: Option<PathBuf>,
) -> Result<Resolved, String> {
    let mut data = data.with_car(model_root.clone(), model.entry.clone());
    let folder = model.entry.file_name.split(['\\', '/']).rev().nth(1).unwrap_or_default();
    let skin = match model.fallback {
        None => selected_skin(&model_root, folder),
        Some(_) => None,
    };
    if let Some((_, dir)) = &skin {
        data = data.with_skin(dir.clone());
    }
    Ok(Resolved { pack, model, skin: skin.map(|(name, _)| name), data, bank, tyre_brand: "pirelli" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_car_flags_parse_check_and_print() {
        let mut flags = Flags::default();
        assert!(!flags.take("--car", "5"));
        assert_eq!(flags.check(false), Ok(()));
        assert!(flags.take("--car-pack", "P"));
        assert_eq!(flags.check(false), Err("--car-pack needs --car-model".into()));
        assert!(flags.take("--car-bank", "B.bank"));
        assert!(flags.take("--car-key", "rsf-24"));
        assert_eq!(flags.check(false), Err("--car-pack needs --car-model".into()));
        assert!(flags.take("--car-model", "/r/Cars/evo/evo.sgc"));
        assert_eq!(flags.check(false), Ok(()));
        assert_eq!(flags.check(true), Err("give one of --rsf-car and --car-pack".into()));
        assert_eq!(
            flags.argv(),
            ["--car-pack", "P", "--car-model", "/r/Cars/evo/evo.sgc", "--car-bank", "B.bank", "--car-key", "rsf-24"]
        );
        let mut orphans = Flags::default();
        assert!(orphans.take("--car-key", "rsf-24"));
        assert_eq!(orphans.check(false), Err("--car-model, --car-bank and --car-key need --car-pack".into()));
        assert_eq!(orphans.argv(), ["--car-key", "rsf-24"]);
    }

    #[test]
    fn model_paths_split_into_the_root_and_the_entry() {
        let (root, entry) = model_of(Path::new("/r/Cars/evo/evo.sgc")).unwrap();
        assert_eq!(root, Path::new("/r"));
        assert_eq!(entry.file_name, "Cars\\evo\\evo.sgc");
        assert_eq!(root_of(Path::new("/r/Cars/evo/evo.SGC")).unwrap(), Path::new("/r"));
        for bad in ["/r/evo/evo.sgc", "/r/Cars/evo/evo.ini", "/r/Cars/evo.sgc", "/r/Cars/evo/", "evo.sgc"] {
            assert!(model_of(Path::new(bad)).is_err(), "{bad}");
        }
        assert_eq!(file_under(Path::new("/r"), "Cars\\evo\\evo.sgc"), Path::new("/r/Cars/evo/evo.sgc"));
    }

    #[test]
    fn the_stand_in_rewrites_the_retail_slots_entry() {
        let mut gone = model_entry("a", "a");
        gone.index = 0;
        let mut slot = model_entry("impreza03", "impreza03");
        slot.index = 5;
        let retail = [gone, slot];
        let model = stand_in(&retail, 5, "Cars/evo not installed, retail car 5".into()).unwrap();
        assert_eq!(model.entry.index, SLOT as usize);
        assert_eq!(model.entry.file_name, "Cars\\impreza03\\impreza03.sgc");
        assert_eq!(model.fallback.as_deref(), Some("Cars/evo not installed, retail car 5"));
        assert_eq!(stand_in(&retail, 3, String::new()), Err("Cars.ini: no [Car03]".into()));
    }

    #[test]
    fn flags_resolve_loads_the_model_in_slot_5() {
        let dir = std::env::temp_dir().join(format!("car-pack-flags-{}", std::process::id()));
        let pack = dir.join("rsfdata").join("cars").join("evo");
        let model = dir.join("Cars").join("evo").join("evo.sgc");
        std::fs::create_dir_all(pack.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::create_dir_all(model.parent().unwrap()).unwrap();
        std::fs::write(&model, [0u8; 4]).unwrap();
        let flags = Flags { pack: Some(pack), model: Some(model), bank: Some(dir.join("evo.bank")), key: None };
        let resolved = flags.resolve(Data::new(&dir, None)).unwrap();
        assert_eq!(resolved.model.entry.index, SLOT as usize);
        assert_eq!(resolved.folder(), "evo");
        assert_eq!(resolved.bank, Some(dir.join("evo.bank")));
        assert_eq!(resolved.data.install(), dir);
        assert!(resolved.data.car_slots(vec![]).iter().any(|e| e.file_name == "Cars\\evo\\evo.sgc"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn model_entry_is_the_slot_the_game_loads() {
        let e = model_entry("evo_IX", "evo_IX");
        assert_eq!(e.index, 5);
        assert_eq!(e.file_name, "Cars\\evo_IX\\evo_IX.sgc");
        assert_eq!(e.shader_settings, "Cars\\evo_IX\\evo_IX_shader_settings");
        assert_eq!(e.texture_path, "Cars\\evo_IX\\Textures\\");
    }

    #[test]
    fn rsf_protected_files_are_told_by_their_magic() {
        assert!(encrypted(&[0x01, 0xa0, 0, 0, 0x5c]));
        assert!(encrypted(&[0x01, 0xa1, 0, 0]));
        assert!(!encrypted(b"((\"PhysicsEngine\""));
        assert!(!encrypted(&[0x01, 0xa0]));
    }
}
