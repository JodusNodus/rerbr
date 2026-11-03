//! NGP 7.4.778's package: read and checked ([`read_ngp`], for the disc setup, `disc_install`, which writes it into the
//! new game folder it makes), and its DLL and archive kept in the player's folder ([`KEPT`], [`add_ngp_to`]) for the
//! Settings page's download and `rerbr --download-ngp`, which change no game folder.

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rbr_formats::rbz;

use crate::archive::open_seven;
use crate::disc_install::{require, with_folders, write_to, Reporter};
use crate::job::Report;

/// The folder in the player's folder with NGP 7.4.778's `NGP.dll` and `physics.rbz` ([`add_ngp_to`]).
pub(crate) const KEPT: &str = "NGP 7.4.778";

/// The kept copy in the player's folder `here` ([`KEPT`]), when its `NGP.dll` is 7.4.778's and its `physics.rbz` is
/// there: its DLL and archive.
pub(crate) fn kept(here: &Path) -> Option<(PathBuf, PathBuf)> {
    let dir = here.join(KEPT);
    let (dll, physics) = (dir.join("NGP.dll"), dir.join("physics.rbz"));
    let supported = fs::read(&dll).is_ok_and(|b| rbr_formats::ngp::version(&b).is_some());
    (supported && physics.is_file()).then_some((dll, physics))
}

/// A failed or cancelled install's message.
type Fail = String;

/// A few files that show the package is what we expect (ignoring case).
pub(crate) const NGP: [&str; 3] = ["physics.rbz", "Plugins/NGP.dll", "AudioFMOD"];

/// Whether a `Physics/physics.lsp` sets NGP's revision (`install::is_ngp_archive` reads one from a path).
pub(crate) fn has_ngp_revision(lsp: &[u8]) -> bool {
    lsp.windows(b"PhysicsRevision_NGP".len()).any(|w| w == b"PhysicsRevision_NGP")
}

/// NGP's files (`/`-separated paths and their bytes) and folders.
type NgpFiles = (Vec<(String, Vec<u8>)>, Vec<String>);

/// NGP's files and folders: from the `.7z`, or the folder.
fn ngp_files(path: &Path) -> Result<NgpFiles, Fail> {
    let (mut files, mut dirs) = (Vec::new(), Vec::new());
    let error = |e: String| format!("{}: {e}", path.display());
    if path.is_dir() {
        let mut stack = vec![(path.to_owned(), String::new())];
        while let Some((dir, rel)) = stack.pop() {
            for entry in fs::read_dir(&dir).map_err(|e| error(e.to_string()))? {
                let entry = entry.map_err(|e| error(e.to_string()))?;
                let name = entry.file_name().to_string_lossy().into_owned();
                let rel = if rel.is_empty() { name } else { format!("{rel}/{name}") };
                if entry.path().is_dir() {
                    dirs.push(rel.clone());
                    stack.push((entry.path(), rel));
                } else {
                    files.push((rel, fs::read(entry.path()).map_err(|e| error(e.to_string()))?));
                }
            }
        }
    } else {
        let mut archive = open_seven(path)
            .map_err(|e| format!("{} is not the NGP package (its .7z or its folder): {e}", path.display()))?;
        archive
            .for_each_entries(|entry, reader| {
                let rel = entry.name().replace('\\', "/");
                if entry.is_directory() {
                    dirs.push(rel);
                } else {
                    let mut bytes = Vec::with_capacity(entry.size() as usize);
                    reader.read_to_end(&mut bytes)?;
                    files.push((rel, bytes));
                }
                Ok(true)
            })
            .map_err(|e| error(e.to_string()))?;
    }
    // As a sorted walk of the extracted folder would merge them.
    files.sort_by(|a, b| a.0.cmp(&b.0));
    dirs.sort();
    Ok((files, dirs))
}

/// The NGP package's files below the game folder, its folders, and its `physics.rbz` (checked to be NGP's).
pub(crate) struct Package {
    pub(crate) files: Vec<(String, Vec<u8>)>,
    pub(crate) dirs: Vec<String>,
    pub(crate) physics: Vec<u8>,
}

/// The NGP package at `path` (its `.7z` or its folder), checked to hold what NGP's install does.
pub(crate) fn read_ngp(path: &Path) -> Result<Package, Fail> {
    let (files, dirs) = ngp_files(path)?;
    let names = with_folders(files.iter().map(|(p, _)| p.as_str()).chain(dirs.iter().map(String::as_str)));
    require(&names, &NGP, format!("the NGP package {}", path.display()))?;
    let (physics, files): (Vec<_>, Vec<_>) =
        files.into_iter().partition(|(p, _)| p.eq_ignore_ascii_case("physics.rbz"));
    let physics = physics.into_iter().next().map(|(_, bytes)| bytes).unwrap_or_default();
    if !is_ngp_archive(&physics) {
        let path = path.display();
        return Err(format!("{path}: its physics.rbz is not NGP's (no PhysicsRevision_NGP in Physics/physics.lsp)"));
    }
    Ok(Package { files, dirs, physics })
}

/// Whether `game` is a Richard Burns Rally folder (it has `Cars/` and `Maps/`).
pub fn check_game_folder(game: &Path) -> Result<(), Fail> {
    match game.join("Cars").is_dir() && game.join("Maps").is_dir() {
        true => Ok(()),
        false => Err(format!("{} is not a Richard Burns Rally folder: choose it first (Game folder)", game.display())),
    }
}

/// The package's `Plugins/NGP.dll`, when it is NGP 7.4.778's.
pub(crate) fn supported_dll(files: &[(String, Vec<u8>)]) -> Result<&[u8], Fail> {
    let dll = files.iter().find(|(p, _)| p.eq_ignore_ascii_case("Plugins/NGP.dll")).map_or(&[][..], |(_, b)| b);
    rbr_formats::ngp::validate(dll).map_err(|e| format!("the downloaded NGP: {e}"))?;
    Ok(dll)
}

/// Writes only the `NGP.dll` and `physics.rbz` of the NGP package at `package` into the folder `to`, when it is NGP
/// 7.4.778's: the Settings page's download, which leaves the game folder (and any NGP in it) as it is.
pub fn add_ngp_to(package: &Path, to: &Path, report: Report<'_>, cancel: &AtomicBool) -> Result<(), Fail> {
    let mut reporter = Reporter::new(report, cancel);
    reporter.step("Reading NGP")?;
    let Package { files, physics, .. } = read_ngp(package)?;
    let dll = supported_dll(&files)?;
    reporter.total = (dll.len() + physics.len()) as u64;
    reporter.step("Writing NGP")?;
    write_to(&to.join("NGP.dll"), dll, &reporter)?;
    write_to(&to.join("physics.rbz"), &physics, &reporter)
}

/// Whether the `physics.rbz` bytes are NGP's (`install::is_ngp_archive` reads one from a path).
pub(crate) fn is_ngp_archive(bytes: &[u8]) -> bool {
    let Ok(mut zip) = rbz::ZipArchive::new(Cursor::new(bytes)) else {
        return false;
    };
    let mut text = Vec::new();
    rbz::by_name(&mut zip, "Physics/physics.lsp").is_ok_and(|mut f| f.read_to_end(&mut text).is_ok())
        && has_ngp_revision(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;

    use crate::job::Progress;

    /// A `physics.rbz` (NGP's when `ngp`) holding its `Physics/physics.lsp`.
    fn physics_rbz(ngp: bool) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("Physics/physics.lsp", options).unwrap();
        zip.write_all(if ngp { b"PhysicsRevision_NGP 778" } else { b"retail" }).unwrap();
        zip.finish().unwrap().into_inner()
    }

    /// An NGP package `.7z` at `path`: `physics.rbz`, `Plugins/NGP.dll` and `AudioFMOD/` with a bank.
    fn ngp_package(path: &Path, dll: &[u8]) {
        let mut out = sevenz_rust2::ArchiveWriter::create(path).unwrap();
        let physics = physics_rbz(true);
        for (name, bytes) in
            [("physics.rbz", &physics[..]), ("Plugins/NGP.dll", dll), ("AudioFMOD/a.bank", b"bank".as_slice())]
        {
            out.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(bytes)).unwrap();
        }
        out.finish().unwrap();
    }

    #[test]
    fn a_package_that_is_not_ngp_778_changes_nothing() {
        let dir = crate::test_dir("ngp-bad");
        let package = dir.join("ngp.7z");
        ngp_package(&package, b"another NGP.dll");
        let to = dir.join("player").join(KEPT);
        let add = |package: &Path| add_ngp_to(package, &to, &|_| {}, &AtomicBool::new(false));
        let err = add(&package).unwrap_err();
        assert!(err.contains("the downloaded NGP: unsupported NGP.dll") && !to.exists(), "{err}");
        assert!(add(&dir.join("none.7z")).is_err() && !to.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// With the real NGP 7.4.778 DLL (`RBR_NGP_DLL`, or the data's): its DLL and archive kept in the player's folder,
    /// in order, again over a kept copy, and found there while the DLL is 7.4.778's.
    #[test]
    fn ngp_is_kept_in_the_players_folder() {
        let data = rbr_formats::data::Data::from_env().unwrap_or_else(|| rbr_formats::data::Data::new("", None));
        let Some(dll) = rbr_formats::ngp::path(&data).and_then(|p| fs::read(p).ok()) else {
            return;
        };
        let dir = crate::test_dir("ngp-ok");
        let package = dir.join("ngp.7z");
        ngp_package(&package, &dll);
        let player = dir.join("player");
        let to = player.join(KEPT);
        assert_eq!(kept(&player), None);
        let progress = Mutex::new(Vec::new());
        let report = |p: &Progress| progress.lock().unwrap().push(p.done);
        assert_eq!(add_ngp_to(&package, &to, &report, &AtomicBool::new(false)), Ok(()));
        assert!(progress.lock().unwrap().windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(fs::read(to.join("NGP.dll")).unwrap(), dll);
        assert_eq!(fs::read(to.join("physics.rbz")).unwrap(), physics_rbz(true));
        assert_eq!(fs::read_dir(&to).unwrap().count(), 2, "only the DLL and the archive");
        assert_eq!(add_ngp_to(&package, &to, &|_| {}, &AtomicBool::new(false)), Ok(()));
        assert!(add_ngp_to(&package, &to, &|_| {}, &AtomicBool::new(true)).unwrap_err().ends_with("cancelled"));
        assert_eq!(kept(&player), Some((to.join("NGP.dll"), to.join("physics.rbz"))));
        fs::write(to.join("NGP.dll"), b"another version").unwrap();
        assert_eq!(kept(&player), None);
        fs::remove_dir_all(&dir).unwrap();
    }
}
