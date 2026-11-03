//! Where a stage's files are. A retail stage is named by its track (`track-54_E`) and its files are in the install's
//! `Maps/`. A stage in a folder of its own, as RallySimFans installs them (`Maps/<id>-<name>/track-<id>_<letter>.*`,
//! `notes/formats.md` "RSF stages"), is named by the path of its files without the extension
//! (`/rsf/Maps/101-Autiovaara II/track-101_O`). The loaders take either form through these functions, so a stage name
//! stays one string from the command line to every reader.

use crate::cars;
use crate::data::Data;
use std::path::{Path, PathBuf};

/// The first word of a RallySimFans-encrypted file (`.lbs`, `.col`, `_textures.rbz`, `TextureFilenameMap<id>.ini`);
/// the track `.ini` has [`ENCRYPTED_INI`]. The rest is not readable: we do not decrypt them.
pub const ENCRYPTED: [u8; 4] = [0x01, 0xa0, 0x00, 0x00];
pub const ENCRYPTED_INI: [u8; 4] = [0x01, 0xa1, 0x00, 0x00];

/// The first word of an obscured `.lbs` (RSF stage 500): not a plain `.lbs`, and not read either.
pub const OBSCURED_LBS: [u8; 4] = [0x21, 0x19, 0x19, 0x19];

/// Whether `bytes` start like a RallySimFans-encrypted file (or an obscured `.lbs`).
pub fn encrypted(bytes: &[u8]) -> bool {
    bytes.starts_with(&ENCRYPTED) || bytes.starts_with(&ENCRYPTED_INI) || bytes.starts_with(&OBSCURED_LBS)
}

/// The track part of a stage name: `track-101_O` for both `track-101_O` and `/rsf/Maps/101-x/track-101_O`.
pub fn name(stage: &str) -> &str {
    stage.rsplit(['/', '\\']).next().unwrap_or(stage)
}

/// The folder of a stage named by its path; `None` for a retail stage name.
pub fn folder(stage: &str) -> Option<&Path> {
    (name(stage).len() != stage.len()).then(|| Path::new(stage).parent()).flatten()
}

/// The map number and variant letter of the stage: `track-54_E` gives 54 and `E`. The prefix's case is ignored, as the
/// game's file lookups do (RSF stage 352's files are `Track-352_O.*`).
pub fn parts(stage: &str) -> Option<(u32, char)> {
    let name = name(stage);
    let rest = name.get(..6).filter(|p| p.eq_ignore_ascii_case("track-")).and(name.get(6..))?;
    let (number, letter) = rest.split_once('_')?;
    let mut letters = letter.chars();
    match (number.parse().ok(), letters.next(), letters.next()) {
        (Some(map), Some(l), None) => Some((map, l.to_ascii_uppercase())),
        _ => None,
    }
}

/// The entry of `dir` named `file`, ignoring case (the game's files are matched as on Windows).
pub fn in_dir(dir: &Path, file: &str) -> Option<PathBuf> {
    cars::resolve(dir, file)
}

/// The stage's file with extension `ext` (`lbs`, `col`, ...): in its folder, else `Maps/` of the install (or the
/// patch). `None` when there is no such file.
///
/// A folder stage's file missing from its folder falls back to the install's `Maps\<track>.<ext>`, as the game does
/// for a folder that holds only some of a retail map's files (the BTB plugin's `track-41_M` replaces five files and
/// keeps map 41's `.fnc` and `.ini`: `notes/btb.md` "Playing"). A RallySimFans stage's id is never a retail map
/// number, so there is no such file for it.
pub fn file(data: &Data, stage: &str, ext: &str) -> Option<PathBuf> {
    let file = format!("{}.{ext}", name(stage));
    folder(stage).and_then(|dir| in_dir(dir, &file)).or_else(|| data.resolve(&format!("Maps\\{file}")))
}

/// The stage's file with extension `ext` for a reader that has the `Maps` folder: in the stage's folder (matched
/// ignoring case), else in `maps` (also for a folder stage, as [`file`]). The path is returned when the file is
/// missing too, so the reader reports it.
pub fn file_in(maps: &Path, stage: &str, ext: &str) -> PathBuf {
    let file = format!("{}.{ext}", name(stage));
    let dir = folder(stage).unwrap_or(maps);
    in_dir(dir, &file).or_else(|| in_dir(maps, &file)).unwrap_or_else(|| dir.join(file))
}

/// Reads the stage's file with extension `ext` ([`file`]), with the path in the error.
pub fn read(data: &Data, stage: &str, ext: &str) -> Result<Vec<u8>, String> {
    let path = file(data, stage, ext).ok_or_else(|| format!("{}.{ext}: not found", name(stage)))?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if encrypted(&bytes) {
        return Err(format!("{}: encrypted by RallySimFans (not readable)", path.display()));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retail_names_and_folder_paths() {
        assert_eq!(name("track-54_E"), "track-54_E");
        assert_eq!(folder("track-54_E"), None);
        assert_eq!(parts("track-54_E"), Some((54, 'E')));
        let rsf = "/rsf/Maps/101-Autiovaara II/track-101_O";
        assert_eq!(name(rsf), "track-101_O");
        assert_eq!(folder(rsf), Some(Path::new("/rsf/Maps/101-Autiovaara II")));
        assert_eq!(parts(rsf), Some((101, 'O')));
        assert_eq!(parts("track-x_O"), None);
        assert_eq!(parts("/rsf/Maps/352-Passo Valle/Track-352_O"), Some((352, 'O')));
        assert_eq!(file_in(Path::new("/maps"), "track-10_O", "lbs"), Path::new("/maps/track-10_O.lbs"));
    }

    #[test]
    fn a_folder_stage_falls_back_to_maps() {
        let root = std::env::temp_dir().join(format!("rbr-stage-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (maps, dir) = (root.join("Maps"), root.join("stage"));
        std::fs::create_dir_all(&maps).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(maps.join("track-41_M.fnc"), b"fnc").unwrap();
        std::fs::write(maps.join("track-41_M.lbs"), b"retail").unwrap();
        std::fs::write(dir.join("track-41_M.lbs"), b"own").unwrap();
        let stage = format!("{}/track-41_M", dir.display());
        let data = Data::new(root.clone(), None);
        // Its own file wins, the missing one comes from Maps, and a file in neither is none.
        assert_eq!(std::fs::read(file(&data, &stage, "lbs").unwrap()).unwrap(), b"own");
        assert_eq!(file(&data, &stage, "fnc"), Some(maps.join("track-41_M.fnc")));
        assert_eq!(file(&data, &stage, "col"), None);
        assert_eq!(file_in(&maps, &stage, "fnc"), maps.join("track-41_M.fnc"));
        assert_eq!(file_in(&maps, &stage, "lbs"), dir.join("track-41_M.lbs"));
        assert_eq!(file_in(&maps, &stage, "col"), dir.join("track-41_M.col"));
        // A retail name is unchanged, and so is a stage whose track has no retail file.
        assert_eq!(file(&data, "track-41_M", "lbs"), Some(maps.join("track-41_M.lbs")));
        assert_eq!(file(&data, &format!("{}/track-101_O", dir.display()), "fnc"), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn encrypted_files_are_recognised() {
        assert!(encrypted(&[1, 0xa0, 0, 0, 0x55]));
        assert!(encrypted(&[1, 0xa1, 0, 0]));
        assert!(encrypted(&[0x21, 0x19, 0x19, 0x19, 0x22]));
        assert!(!encrypted(b"OC7R"));
    }
}
