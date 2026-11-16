//! Adding BTB stages: each stage folder of a dropped folder or archive is copied into `<player folder>/tracks/<name>/`.
//! Every path goes through [`crate::archive::safe_path`], executables and DLLs are left out, and the stage is built
//! in a hidden folder beside its destination and renamed into place only when it opens (`rbr_btb::track`): a cancel
//! or a failure leaves nothing behind.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rbr_btb::track;

use super::detect::StageDir;
use crate::archive::{extract_seven, extract_zip, safe_folder, seven_entries, zip_entries};
use crate::job::{Counter, Report, Temp};

/// File endings that are never copied.
const SKIPPED: [&str; 9] = ["exe", "dll", "msi", "bat", "cmd", "com", "scr", "sh", "so"];

type Fail = String;

/// A file of the item: its path parts below it and its size.
pub type File = (Vec<String>, u64);

/// What a stage was added as: its folder in `tracks/` and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    pub folder: String,
    pub name: String,
}

/// The files of the folder `dir`, below it, without links.
pub fn folder_files(dir: &Path) -> Vec<File> {
    crate::codriver_packs::walk(dir)
        .into_iter()
        .map(|(rel, path)| {
            let size = path.metadata().map_or(0, |m| m.len());
            (rel.split('/').map(str::to_owned).collect(), size)
        })
        .collect()
}

/// The files of the zip or 7z `path` with safe paths.
pub fn archive_files(path: &Path) -> Result<Vec<File>, Fail> {
    let seven = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("7z"));
    let entries = if seven { seven_entries(path)? } else { zip_entries(path)? };
    Ok(entries.into_iter().filter(|e| !e.dir).filter_map(|e| Some((e.parts?, e.size))).collect())
}

/// Whether the path lies below the folder `prefix` (names ignoring case).
fn within(path: &[String], prefix: &[String]) -> bool {
    path.len() > prefix.len() && path.iter().zip(prefix).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn skipped(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, e)| SKIPPED.iter().any(|s| e.eq_ignore_ascii_case(s)))
}

/// Whether `tracks` already has a stage folder called `name` (ignoring case).
fn taken(tracks: &Path, name: &str) -> bool {
    fs::read_dir(tracks)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
}

/// Adds the stage `stage` of the item `src` (a folder or a zip or 7z) holding `files` to `tracks`.
pub fn add_stage(
    src: &Path,
    files: &[File],
    stage: &StageDir,
    tracks: &Path,
    report: Report<'_>,
    cancel: &AtomicBool,
) -> Result<Added, Fail> {
    let name = safe_folder(&stage.name).ok_or_else(|| format!("a bad stage folder name {}", stage.name))?;
    if taken(tracks, &name) {
        return Err("already added".into());
    }
    let wanted: Vec<(Vec<String>, u64)> =
        files.iter().filter(|(p, _)| within(p, &stage.dir) && !p.last().is_some_and(|f| skipped(f))).cloned().collect();
    let pairs: Vec<(Vec<String>, Vec<String>)> =
        wanted.iter().map(|(p, _)| (p.clone(), p[stage.dir.len()..].to_vec())).collect();
    fs::create_dir_all(tracks).map_err(|e| format!("{}: {e}", tracks.display()))?;
    let temp = Temp::create(tracks, "adding")?;
    let staged = temp.0.join(&name);
    let mut count =
        Counter { step: "Adding BTB stage", done: 0, total: wanted.iter().map(|(_, n)| n).sum(), report, cancel };
    count.add(0)?;
    if src.is_dir() {
        for (from, to) in &pairs {
            let source = from.iter().fold(src.to_owned(), |p, part| p.join(part));
            let dest = to.iter().fold(staged.clone(), |p, part| p.join(part));
            fs::create_dir_all(dest.parent().unwrap_or(&staged)).map_err(|e| format!("{}: {e}", dest.display()))?;
            let n = fs::copy(&source, &dest).map_err(|e| format!("{}: {e}", source.display()))?;
            count.add(n)?;
        }
    } else if src.extension().is_some_and(|e| e.eq_ignore_ascii_case("7z")) {
        extract_seven(src, &staged, &pairs, &mut |n| count.add(n))?;
    } else {
        extract_zip(src, &staged, &pairs, &mut |n| count.add(n))?;
    }
    fs::create_dir_all(&staged).map_err(|e| format!("{}: {e}", staged.display()))?;
    if !track::is_track_folder(&staged) {
        return Err(
            "not a BTB stage: track.ini, objectlist.ini, Objects.zip and Textures.zip were not all there".into()
        );
    }
    let shown = match track::scan_dir(&temp.0).into_iter().next() {
        Some(Ok(track)) => Some(track.info.name.trim().to_owned()).filter(|n| !n.is_empty()).unwrap_or(track.name),
        Some(Err((_, e))) => return Err(format!("the stage doesn't open: {e}")),
        None => return Err("the stage doesn't open".into()),
    };
    let dest: PathBuf = tracks.join(&name);
    // The name could have been taken since: never over an existing stage.
    if dest.exists() {
        return Err("already added".into());
    }
    fs::rename(&staged, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(Added { folder: name, name: shown })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::io::Write;

    pub fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    /// A synthetic stage folder at `dir`, with a program and a DLL that must not be copied.
    pub fn synthetic_stage(dir: &Path, title: &str) {
        write(&dir.join("track.ini"), format!("[INFO]\nname = {title}\nauthor = Test\n").as_bytes());
        write(&dir.join("ObjectList.ini"), b"");
        write(&dir.join("objects.zip"), b"PK");
        write(&dir.join("Textures.ZIP"), b"PK");
        write(&dir.join("driveline.ini"), b"[DRIVELINE]\n");
        write(&dir.join("tool.exe"), b"MZ");
        write(&dir.join("sub/hook.dll"), b"MZ");
        write(&dir.join("sub/keep.txt"), b"k");
    }

    fn add(src: &Path, files: &[File], stage: &StageDir, tracks: &Path) -> Result<Added, Fail> {
        add_stage(src, files, stage, tracks, &|_| {}, &AtomicBool::new(false))
    }

    fn hidden_left(tracks: &Path) -> Vec<String> {
        let names = fs::read_dir(tracks)
            .map(|r| r.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()));
        names.into_iter().flatten().filter(|n| n.starts_with('.')).collect()
    }

    #[test]
    fn a_stage_folder_is_copied_without_programs_and_listed() {
        let dir = crate::test_dir("content-btb-folder");
        let src = dir.join("Autio I Gravel");
        synthetic_stage(&src, "Autio I Gravel");
        let tracks = dir.join("home/tracks");
        let files = folder_files(&src);
        let stage = StageDir { dir: vec![], name: "Autio I Gravel".into() };
        let added = add(&src, &files, &stage, &tracks).unwrap();
        assert_eq!(added, Added { folder: "Autio I Gravel".into(), name: "Autio I Gravel".into() });
        let there = tracks.join("Autio I Gravel");
        assert!(there.join("track.ini").is_file() && there.join("sub/keep.txt").is_file());
        assert!(!there.join("tool.exe").exists() && !there.join("sub/hook.dll").exists());
        assert!(hidden_left(&tracks).is_empty());
        // The catalog lists it.
        let maps = crate::btb::folder_maps(&tracks, |_| false);
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].btb().unwrap().name, "Autio I Gravel");
        // Again: already there, and nothing else appears.
        assert_eq!(add(&src, &files, &stage, &tracks).unwrap_err(), "already added");
        assert_eq!(fs::read_dir(&tracks).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_archive_with_a_stage_in_rx_content_adds_and_a_bad_one_leaves_nothing() {
        let dir = crate::test_dir("content-btb-archive");
        let src = dir.join("src");
        synthetic_stage(&src.join("RX_CONTENT/Tracks/Lappi"), "Lappi");
        // A folder that is not a stage (no textures): nothing is added.
        synthetic_stage(&src.join("RX_CONTENT/Tracks/Broken"), "Broken");
        fs::remove_file(src.join("RX_CONTENT/Tracks/Broken/Textures.ZIP")).unwrap();
        let archive = dir.join("pack.7z");
        let mut out = sevenz_rust2::ArchiveWriter::create(&archive).unwrap();
        for (rel, path) in crate::codriver_packs::walk(&src) {
            let data = fs::read(path).unwrap();
            out.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(&rel), Some(&data[..])).unwrap();
        }
        out.finish().unwrap();
        let zip = dir.join("pack.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&zip).unwrap());
        for (rel, path) in crate::codriver_packs::walk(&src) {
            writer.start_file(rel, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(&fs::read(path).unwrap()).unwrap();
        }
        writer.finish().unwrap();
        for (k, archive) in [archive, zip].iter().enumerate() {
            let tracks = dir.join(format!("home{k}/tracks"));
            let files = archive_files(archive).unwrap();
            let lappi =
                StageDir { dir: ["RX_CONTENT", "Tracks", "Lappi"].map(Into::into).into(), name: "Lappi".into() };
            let broken =
                StageDir { dir: ["RX_CONTENT", "Tracks", "Broken"].map(Into::into).into(), name: "Broken".into() };
            assert_eq!(add(archive, &files, &lappi, &tracks).unwrap().folder, "Lappi");
            assert!(tracks.join("Lappi/Textures.ZIP").is_file() && !tracks.join("Lappi/tool.exe").exists());
            let err = add(archive, &files, &broken, &tracks).unwrap_err();
            assert!(err.starts_with("not a BTB stage"), "{err}");
            assert!(!tracks.join("Broken").exists() && hidden_left(&tracks).is_empty());
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_cancel_leaves_nothing_behind() {
        let dir = crate::test_dir("content-btb-cancel");
        let src = dir.join("Lappi");
        synthetic_stage(&src, "Lappi");
        let tracks = dir.join("home/tracks");
        let stage = StageDir { dir: vec![], name: "Lappi".into() };
        let files = folder_files(&src);
        let err = add_stage(&src, &files, &stage, &tracks, &|_| {}, &AtomicBool::new(true)).unwrap_err();
        assert!(err.ends_with("cancelled"), "{err}");
        assert!(fs::read_dir(&tracks).unwrap().next().is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// With `RBR_RSF`: the first real `RX_CONTENT/Tracks/*` stage, packed as a `.7z`, is added into a temp home and
    /// lists.
    #[test]
    fn a_real_stage_adds_and_lists() {
        let Some(rsf) = std::env::var_os("RBR_RSF").map(PathBuf::from) else {
            eprintln!("skipping: RBR_RSF not set");
            return;
        };
        let tracks_dir = rbr_formats::stage_files::in_dir(&rsf, "RX_CONTENT\\Tracks").unwrap_or_default();
        let mut stages: Vec<PathBuf> = fs::read_dir(&tracks_dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| track::is_track_folder(p))
            .collect();
        stages.sort();
        let Some(first) = stages.first() else {
            eprintln!("skipping: no BTB stage in {}", rsf.display());
            return;
        };
        let name = first.file_name().unwrap().to_string_lossy().into_owned();
        let dir = crate::test_dir("content-btb-real");
        let archive = dir.join("stage.7z");
        let mut out = sevenz_rust2::ArchiveWriter::create(&archive).unwrap();
        for (rel, path) in crate::codriver_packs::walk(first) {
            let data = fs::read(path).unwrap();
            out.push_archive_entry(
                sevenz_rust2::ArchiveEntry::new_file(&format!("RX_CONTENT/Tracks/{name}/{rel}")),
                Some(&data[..]),
            )
            .unwrap();
        }
        out.finish().unwrap();
        let files = archive_files(&archive).unwrap();
        let stage = StageDir { dir: ["RX_CONTENT".into(), "Tracks".into(), name.clone()].into(), name: name.clone() };
        let tracks = dir.join("home/tracks");
        let added = add(&archive, &files, &stage, &tracks).unwrap();
        assert_eq!(added.folder, name);
        let maps = crate::btb::folder_maps(&tracks, |_| false);
        assert_eq!(maps.len(), 1, "{name} should list");
        fs::remove_dir_all(&dir).unwrap();
    }
}
