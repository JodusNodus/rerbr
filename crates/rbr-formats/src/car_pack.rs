//! An NGP car pack read in place: the physics pack and the model the game loads it with.
//!
//! What RSF does when a car is chosen (notes/formats.md "RSF cars"): it copies the car's physics pack
//! (`rsfdata/cars/<pack>/`, NGP keys in the retail `.lsp` layout) into one slot's physics folder (`Physics/s_i2003/`
//! for slot 5) and rewrites that slot's `[CarNN]` in `Cars/Cars.ini` to the car's model folder (`Cars/<model>/`, the
//! retail model layout). The game then loads the slot as a retail car. We read the pack and the model in place
//! instead; nothing is written. The same layout holds the packs the player added (`cars/` in the player's folder).
//!
//! [`resolve`] turns the launcher's explicit inputs (the pack, the model entry, the root the model resolves under
//! and the sound bank) into what a race loads: the data with the model in its slot, the skin over it, and the bank
//! with its tyres.

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
    Ok(Resolved {
        pack,
        model,
        skin: skin.map(|(name, _)| name),
        data,
        bank,
        tyre_brand: "pirelli",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
