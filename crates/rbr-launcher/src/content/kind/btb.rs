//! BTB stages: each stage folder of a dropped folder or archive (one holding `track.ini`, `objectlist.ini`,
//! `Objects.zip` and `Textures.zip`, names ignoring case, as `rbr_btb::track::is_track_folder`) is copied into
//! `<player folder>/tracks/<name>/`. Every path goes through [`crate::archive::safe_path`], executables and DLLs are
//! left out, and the stage is built in a hidden folder beside its destination and renamed into place only when it
//! opens (`rbr_btb::track`): a cancel or a failure leaves nothing behind.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use rbr_btb::track;

use super::{wanted, within, Ctx, Found, Item, Kind};
use crate::archive::safe_folder;
use crate::content::source::{Entry, Source};
use crate::content::{stem, Goto, Outcome, Refresh};
use crate::job::{Counter, Report, Temp};

/// File endings that are never copied.
const SKIPPED: [&str; 9] = ["exe", "dll", "msi", "bat", "cmd", "com", "scr", "sh", "so"];

/// A BTB stage's files, by name (ignoring case).
const STAGE_FILES: [&str; 4] = ["track.ini", "objectlist.ini", "objects.zip", "textures.zip"];

type Fail = String;

pub struct Btb;

/// What a stage was added as: its folder in `tracks/` and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    pub folder: String,
    pub name: String,
}

/// The stage folders among `entries`: each folder holding all of [`STAGE_FILES`], the outermost of nested ones.
pub fn stage_dirs(entries: &[Entry]) -> Vec<Vec<String>> {
    // Lowercase folder path to its original parts and the lowercase names of the files directly in it.
    let mut folders: BTreeMap<String, (&[String], HashSet<String>)> = BTreeMap::new();
    for entry in entries {
        let Some((file, dir)) = entry.path.split_last() else { continue };
        let key = dir.join("/").to_ascii_lowercase();
        folders.entry(key).or_insert_with(|| (dir, HashSet::new())).1.insert(file.to_ascii_lowercase());
    }
    let mut found: Vec<(&String, &[String])> = Vec::new();
    // The sorted keys put a folder before the ones inside it.
    for (key, (dir, files)) in &folders {
        let complete = STAGE_FILES.iter().all(|f| files.contains(*f));
        let inside = found.iter().any(|(outer, _)| outer.is_empty() || key.starts_with(&format!("{outer}/")));
        if complete && !inside {
            found.push((key, dir));
        }
    }
    found.into_iter().map(|(_, dir)| dir.to_vec()).collect()
}

/// Whether `path` lies below one of the stage folders `stages`.
pub fn in_stage(path: &[String], stages: &[Vec<String>]) -> bool {
    stages.iter().any(|s| within(path, s))
}

/// The stage's folder name: its folder's, else the item's.
fn stage_name(item: &str, dir: &[String]) -> String {
    dir.last().cloned().unwrap_or_else(|| stem(item).to_owned())
}

impl Kind for Btb {
    fn detect(&self, src: &dyn Source) -> Option<Found> {
        let at = stage_dirs(src.entries());
        let label = if at.len() == 1 { "BTB stage" } else { "BTB stages" };
        (!at.is_empty()).then(|| Found { at, ..Found::new(label) })
    }

    fn add(&self, item: &Item, ctx: &Ctx) -> Vec<Outcome> {
        let (name, tracks) = (item.src.name(), ctx.home.join(rbr_btb::player::TRACKS));
        ctx.step("Adding BTB stage");
        let added = |dir: &Vec<String>| {
            let stage = stage_name(&name, dir);
            match add_stage(item.src, dir, &stage, &tracks, ctx.report, ctx.cancel) {
                Ok(a) => {
                    let goto = Goto::Stage(a.folder);
                    Outcome::added(&name, "BTB stage", &a.name, "it's in Stages", goto)
                }
                Err(e) => Outcome::failed(&stage, "BTB stage", e),
            }
            .refreshing(Refresh::Stages)
        };
        item.found.at.iter().map(added).collect()
    }
}

/// Whether `tracks` already has a stage folder called `name` (ignoring case).
fn taken(tracks: &Path, name: &str) -> bool {
    fs::read_dir(tracks)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
}

/// Adds the stage folder `dir` of `src` to `tracks` as the folder `name`.
pub fn add_stage(
    src: &dyn Source,
    dir: &[String],
    name: &str,
    tracks: &Path,
    report: Report<'_>,
    cancel: &AtomicBool,
) -> Result<Added, Fail> {
    let name = safe_folder(name).ok_or_else(|| format!("a bad stage folder name {name}"))?;
    if taken(tracks, &name) {
        return Err("already added".into());
    }
    let wanted = wanted(src, dir, &SKIPPED);
    let pairs: Vec<(Vec<String>, Vec<String>)> =
        wanted.iter().map(|e| (e.path.clone(), e.path[dir.len()..].to_vec())).collect();
    fs::create_dir_all(tracks).map_err(|e| format!("{}: {e}", tracks.display()))?;
    let temp = Temp::create(tracks, "adding")?;
    let staged = temp.0.join(&name);
    let total = wanted.iter().map(|e| e.size).sum();
    let mut count = Counter { step: "Adding BTB stage", done: 0, total, report, cancel };
    count.add(0)?;
    src.extract(&pairs, &staged, &mut |n| count.add(n))?;
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
    let dest = tracks.join(&name);
    // The name could have been taken since: never over an existing stage.
    if dest.exists() {
        return Err("already added".into());
    }
    fs::rename(&staged, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(Added { folder: name, name: shown })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;
    use std::path::PathBuf;

    use super::*;
    use crate::content::source::{self, Entry};

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

    fn entries(files: &[&str]) -> Vec<Entry> {
        files.iter().map(|f| Entry { path: f.split('/').map(str::to_owned).collect(), size: 0 }).collect()
    }

    fn stage(root: &str) -> Vec<String> {
        ["track.ini", "ObjectList.ini", "objects.zip", "TEXTURES.ZIP", "driveline.ini"]
            .iter()
            .map(|f| if root.is_empty() { f.to_string() } else { format!("{root}/{f}") })
            .collect()
    }

    fn dirs(files: &[String]) -> Vec<String> {
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        stage_dirs(&entries(&files)).iter().map(|d| d.join("/")).collect()
    }

    #[test]
    fn stages_are_found_at_the_top_in_a_folder_in_a_tree_and_in_twos() {
        assert_eq!(dirs(&stage("")), [""]);
        assert_eq!(dirs(&stage("Autio I Gravel")), ["Autio I Gravel"]);
        assert_eq!(dirs(&stage("RX_CONTENT/Tracks/Lappi")), ["RX_CONTENT/Tracks/Lappi"]);
        let mut two = stage("Pack/A");
        two.extend(stage("Pack/B"));
        assert_eq!(dirs(&two), ["Pack/A", "Pack/B"]);
        // Nested ones are the outer's.
        let mut nested = stage("A");
        nested.extend(stage("A/B"));
        assert_eq!(dirs(&nested), ["A"]);
        // One of the four missing: no stage.
        let mut partial = stage("Lappi");
        partial.retain(|e| !e.to_lowercase().ends_with("objects.zip"));
        assert!(dirs(&partial).is_empty());
        assert_eq!(stage_name("Autio.zip", &[]), "Autio");
    }

    fn add(src: &Path, dir: &[&str], name: &str, tracks: &Path) -> Result<Added, Fail> {
        let src = source::open(src).unwrap();
        let dir: Vec<String> = dir.iter().map(|s| s.to_string()).collect();
        add_stage(&*src, &dir, name, tracks, &|_| {}, &AtomicBool::new(false))
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
        let added = add(&src, &[], "Autio I Gravel", &tracks).unwrap();
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
        assert_eq!(add(&src, &[], "Autio I Gravel", &tracks).unwrap_err(), "already added");
        assert_eq!(fs::read_dir(&tracks).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    fn pack(src: &Path, dir: &Path) -> (PathBuf, PathBuf) {
        let archive = dir.join("pack.7z");
        let mut out = sevenz_rust2::ArchiveWriter::create(&archive).unwrap();
        for (rel, path) in crate::codriver_packs::walk(src) {
            let data = fs::read(path).unwrap();
            out.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(&rel), Some(&data[..])).unwrap();
        }
        out.finish().unwrap();
        let zip = dir.join("pack.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&zip).unwrap());
        for (rel, path) in crate::codriver_packs::walk(src) {
            writer.start_file(rel, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(&fs::read(path).unwrap()).unwrap();
        }
        writer.finish().unwrap();
        (archive, zip)
    }

    #[test]
    fn an_archive_with_a_stage_in_rx_content_adds_and_a_bad_one_leaves_nothing() {
        let dir = crate::test_dir("content-btb-archive");
        let src = dir.join("src");
        synthetic_stage(&src.join("RX_CONTENT/Tracks/Lappi"), "Lappi");
        // A folder that is not a stage (no textures): nothing is added.
        synthetic_stage(&src.join("RX_CONTENT/Tracks/Broken"), "Broken");
        fs::remove_file(src.join("RX_CONTENT/Tracks/Broken/Textures.ZIP")).unwrap();
        let (seven, zip) = pack(&src, &dir);
        for (k, archive) in [seven, zip].iter().enumerate() {
            let tracks = dir.join(format!("home{k}/tracks"));
            let lappi = ["RX_CONTENT", "Tracks", "Lappi"];
            assert_eq!(add(archive, &lappi, "Lappi", &tracks).unwrap().folder, "Lappi");
            assert!(tracks.join("Lappi/Textures.ZIP").is_file() && !tracks.join("Lappi/tool.exe").exists());
            let err = add(archive, &["RX_CONTENT", "Tracks", "Broken"], "Broken", &tracks).unwrap_err();
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
        let source = source::open(&src).unwrap();
        let err = add_stage(&*source, &[], "Lappi", &tracks, &|_| {}, &AtomicBool::new(true)).unwrap_err();
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
            let entry = sevenz_rust2::ArchiveEntry::new_file(&format!("RX_CONTENT/Tracks/{name}/{rel}"));
            out.push_archive_entry(entry, Some(&data[..])).unwrap();
        }
        out.finish().unwrap();
        let tracks = dir.join("home/tracks");
        let added = add(&archive, &["RX_CONTENT", "Tracks", &name], &name, &tracks).unwrap();
        assert_eq!(added.folder, name);
        let maps = crate::btb::folder_maps(&tracks, |_| false);
        assert_eq!(maps.len(), 1, "{name} should list");
        fs::remove_dir_all(&dir).unwrap();
    }
}
