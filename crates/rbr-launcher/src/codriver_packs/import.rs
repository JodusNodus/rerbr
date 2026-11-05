//! Importing downloads and folders into the owned packs ([`import`], [`cli`]).
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rbr_formats::pacenote::notes;

use super::scan::{
    cancelled, check_voice, copy_notes, copy_voice, find_voice_root, free_dir, is_note_file, is_plugin_dir, keep_voice,
    note_files, pack_name, walk,
};
use super::{Fail, NOTES_DIR, SEVEN_Z, VOICES_DIR, ZIP};
use crate::archive::{safe_folder, safe_path};
use crate::job::Temp;

/// absolute), else NGPCarMenu's default; `None` when disabled or missing.
fn install_notes_dir(root: &Path) -> Option<PathBuf> {
    notes::my_pacenotes_dir(root)
}

/// Imports the folder `src` (a plugin folder, a note folder, a folder with both, or a whole install to copy from)
/// into `home`: the lines saying what was added.
fn import_folder(home: &Path, src: &Path, cancel: &AtomicBool) -> Result<Vec<String>, Fail> {
    if !src.is_dir() {
        return Err(format!("{} is not a folder", src.display()));
    }
    let name = pack_name(src);
    // A whole existing install: its plugin and its MyPacenotes folder are the sources, copied, never modified.
    if src.join("Plugins").is_dir() {
        let plugin = src.join("Plugins").join("Pacenote");
        let voice = is_plugin_dir(&plugin).then(|| plugin.clone());
        let notes = install_notes_dir(src);
        return match (voice, notes) {
            (None, None) => Err(format!(
                "{}: no Pacenote plugin (Plugins/Pacenote) and no stage notes ({}): is it an RBR install?",
                src.display(),
                notes::my_pacenotes_dir(src)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| { "Plugins/NGPCarMenu/MyPacenotes".into() })
            )),
            (Some(plugin), None) => import_voice_from(home, &name, cancel, |d, c| copy_voice(&plugin, d, c)),
            (None, Some(notes)) => import_notes_from(home, &name, &notes, cancel, |d, c| copy_notes(&notes, d, c)),
            (Some(plugin), Some(notes)) => import_both(home, &name, cancel, &plugin, &notes),
        };
    }
    let voice = find_voice_root(src);
    // The notes are searched recursively, so wrappers stay on: the stage folders keep their names.
    let notes = note_files(src).len();
    if let Some(root) = voice {
        if notes == 0 {
            return import_voice_from(home, &name, cancel, |d, c| copy_voice(&root, d, c));
        }
        return import_both(home, &name, cancel, &root, src);
    }
    if notes > 0 {
        return import_notes_from(home, &name, src, cancel, |d, c| copy_notes(src, d, c));
    }
    Err(format!(
        "{}: no co-driver voices, call styles or stage notes in it: choose the plugin's folder \
         (PaceNote.ini, sounds/, config/), a MyPacenotes folder, or a whole install to copy from",
        src.display()
    ))
}

/// Imports a voice pack and a note pack as one atomic operation: both stage into temp folders and validate, and
/// only then both publish by rename. The cancellation flag is checked immediately before the commit, and a
/// failure publishing the notes removes the just-published voice again. A failure anywhere else leaves no pack
/// behind (no half-installed operation).
pub(super) fn import_both(
    home: &Path,
    name: &str,
    cancel: &AtomicBool,
    voice_src: &Path,
    notes_src: &Path,
) -> Result<Vec<String>, Fail> {
    let (voice, (styles, ranges)) = stage_voice(home, name, cancel, |d, c| copy_voice(voice_src, d, c))?;
    let (notes, n) = stage_notes(home, name, notes_src, cancel, |d, c| copy_notes(notes_src, d, c))?;
    publish_pair(&voice, &notes, cancel)?;
    Ok(vec![voice_line(&voice.dest, &styles, &ranges), notes_line(&notes.dest, n)])
}

/// A staged import: the temp folder holding it, the validated folder inside, and the published folder it renames to.
pub(super) struct Staged {
    _temp: Temp,
    pub(super) staged: PathBuf,
    dest: PathBuf,
}

/// Stages a pack named `name` below `home/dir`, in a temp folder renamed to a free pack folder on publish.
pub(super) fn stage(home: &Path, dir: &str, name: &str) -> Result<Staged, Fail> {
    let base = home.join(dir);
    fs::create_dir_all(&base).map_err(|e| format!("{}: {e}", base.display()))?;
    let dest = free_dir(&base, &safe_folder(name).unwrap_or_else(|| "pack".to_owned()));
    let temp = Temp::create(&base, "import")?;
    let staged = temp.0.join("pack");
    fs::create_dir(&staged).map_err(|e| format!("{}: {e}", staged.display()))?;
    Ok(Staged { _temp: temp, staged, dest })
}

/// Publishes a validated staged pack by rename; dropping the staging removes its temp folder. Checks the
/// cancellation flag immediately before the commit, so a cancel after staging publishes nothing.
pub(super) fn publish(op: &Staged, cancel: &AtomicBool) -> Result<(), Fail> {
    cancelled(cancel)?;
    fs::rename(&op.staged, &op.dest).map_err(|e| format!("{}: {e}", op.dest.display()))
}

/// Publishes a staged voice and note pair: the notes failing (or a cancel between the two renames) removes the
/// just-published voice again, so a dual publish never leaves a lone voice behind despite the atomicity claim.
pub(super) fn publish_pair(voice: &Staged, notes: &Staged, cancel: &AtomicBool) -> Result<(), Fail> {
    publish(voice, cancel)?;
    match publish(notes, cancel) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_dir_all(&voice.dest);
            Err(e)
        }
    }
}

/// The line saying what voice pack published.
fn voice_line(dest: &Path, styles: &[String], ranges: &[String]) -> String {
    let pack = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    format!("Co-driver {pack}: {} call styles, {} range sets.", styles.len(), ranges.len())
}

/// The line saying what note pack published.
fn notes_line(dest: &Path, n: usize) -> String {
    let pack = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    format!("Stage notes {pack}: {n} files.")
}

/// Requires staged notes to hold at least one note file: the files, else what to choose instead.
fn check_notes(staged: &Path, src: &Path) -> Result<usize, Fail> {
    match note_files(staged).len() {
        0 => Err(format!(
            "{}: no stage notes (*.ini with [PACENOTES]) in it: choose a MyPacenotes folder or a note pack",
            src.display()
        )),
        n => Ok(n),
    }
}

/// A staged voice pack and what its validation says: its call styles and range sets.
type StagedVoice = (Staged, (Vec<String>, Vec<String>));

/// Stages a voice pack named `name` with `copy` (into a temp folder, validated): the staging, its styles and
/// range sets. A failure leaves no pack behind (the staging drops with its temp folder).
fn stage_voice<R>(
    home: &Path,
    name: &str,
    cancel: &AtomicBool,
    copy: impl FnOnce(&Path, &AtomicBool) -> Result<R, Fail>,
) -> Result<StagedVoice, Fail> {
    let op = stage(home, VOICES_DIR, name)?;
    copy(&op.staged, cancel)?;
    let checked = check_voice(&op.staged)?;
    Ok((op, checked))
}

/// Stages a note pack named `name` with `copy`: the staging and its note files.
fn stage_notes<R>(
    home: &Path,
    name: &str,
    src: &Path,
    cancel: &AtomicBool,
    copy: impl FnOnce(&Path, &AtomicBool) -> Result<R, Fail>,
) -> Result<(Staged, usize), Fail> {
    let op = stage(home, NOTES_DIR, name)?;
    copy(&op.staged, cancel)?;
    let n = check_notes(&op.staged, src)?;
    Ok((op, n))
}

/// Stages a voice pack named `name` with `copy` (into a temp folder, validated, then published): the lines saying
/// what was added. A failure leaves no pack behind.
pub(super) fn import_voice_from<R>(
    home: &Path,
    name: &str,
    cancel: &AtomicBool,
    copy: impl FnOnce(&Path, &AtomicBool) -> Result<R, Fail>,
) -> Result<Vec<String>, Fail> {
    let (op, (styles, ranges)) = stage_voice(home, name, cancel, copy)?;
    publish(&op, cancel)?;
    Ok(vec![voice_line(&op.dest, &styles, &ranges)])
}

/// Stages a note pack named `name` with `copy`: the lines saying what was added.
fn import_notes_from<R>(
    home: &Path,
    name: &str,
    src: &Path,
    cancel: &AtomicBool,
    copy: impl FnOnce(&Path, &AtomicBool) -> Result<R, Fail>,
) -> Result<Vec<String>, Fail> {
    let (op, n) = stage_notes(home, name, src, cancel, copy)?;
    publish(&op, cancel)?;
    Ok(vec![notes_line(&op.dest, n)])
}

/// Whether the archive entries hold a plugin at `root` (raw path parts): its settings, voices or definitions
/// below it.
fn archive_plugin_at(entries: &[Vec<String>], root: &[String]) -> bool {
    let below = entries.iter().filter_map(|p| p.strip_prefix(root)).map(|p| p.join("/").to_ascii_lowercase());
    let mut has = (false, false, false);
    for rel in below {
        has.0 |= rel == "pacenote.ini";
        has.1 |= rel.starts_with("sounds/");
        has.2 |= rel.starts_with("config/pacenotes/") || rel.starts_with("config/ranges/");
    }
    has.0 || has.1 || has.2
}

/// The plugin's root in the entries: top level, an install's `Plugins/Pacenote`, a single folder holding one
/// (a direct `Pacenote` folder beside the notes), or the same under a single wrapper directory (including
/// `wrapper/Pacenote` and `wrapper/Plugins/Pacenote`). Mirrors `find_voice_root`; note paths always stay whole,
/// so a single-stage archive's stage folder keeps its name.
fn archive_plugin_root(entries: &[Vec<String>]) -> Option<Vec<String>> {
    let mut candidates: Vec<Vec<String>> = vec![Vec::new(), vec!["Plugins".into(), "Pacenote".into()]];
    let mut tops: Vec<String> = entries.iter().filter_map(|p| p.first().cloned()).collect();
    tops.sort();
    tops.dedup();
    for top in &tops {
        candidates.push(vec![top.clone()]);
    }
    for top in &tops {
        candidates.push(vec![top.clone(), "Pacenote".into()]);
        candidates.push(vec![top.clone(), "Plugins".into(), "Pacenote".into()]);
    }
    // The same under a single wrapper directory (two levels down, bounded).
    if let [wrapper] = &tops[..] {
        let prefix = [wrapper.clone()];
        let mut subs: Vec<String> =
            entries.iter().filter_map(|p| p.strip_prefix(&prefix)).filter_map(|rest| rest.first().cloned()).collect();
        subs.sort();
        subs.dedup();
        for sub in &subs {
            candidates.push(vec![wrapper.clone(), sub.clone()]);
            candidates.push(vec![wrapper.clone(), sub.clone(), "Pacenote".into()]);
            candidates.push(vec![wrapper.clone(), sub.clone(), "Plugins".into(), "Pacenote".into()]);
        }
    }
    candidates.into_iter().find(|root| archive_plugin_at(entries, root))
}

/// The note files among the entries: every INI except the voice files below the plugin's `root` (the style and
/// range definitions are not stage notes; anything else unparseable is pruned after extraction). Paths stay whole,
/// so a single-stage archive's stage folder keeps its name.
fn archive_note_candidates(entries: &[Vec<String>], root: Option<&[String]>) -> Vec<Vec<String>> {
    entries
        .iter()
        .filter(|p| p.last().is_some_and(|n| n.to_ascii_lowercase().ends_with(".ini")))
        .filter(|p| match root {
            Some(root) if p.len() > root.len() && p[..root.len()] == root[..] => {
                let below: Vec<&str> = p[root.len()..].iter().map(String::as_str).collect();
                !keep_voice(&below)
            }
            _ => true,
        })
        .cloned()
        .collect()
}

/// Imports the archive `src` (ZIP or 7z) as one atomic operation: the voice and the notes stage and validate
/// first, and only then both publish (a notes publish failure rolls the just-published voice back). Note
/// candidates that prune to no parseable note file fall back to a voice-only import, so an unrelated INI next
/// to a valid voice never rejects it; an archive with no note INIs at all imports its voice alone, and a
/// notes-only archive with no parseable notes fails loudly. Extraction errors propagate, never swallowed.
fn import_archive(home: &Path, src: &Path, cancel: &AtomicBool) -> Result<Vec<String>, Fail> {
    let lower = src.to_string_lossy().to_ascii_lowercase();
    let listed = match lower.ends_with(ZIP) {
        true => crate::archive::zip_entries(src),
        false => crate::archive::seven_entries(src),
    }
    .map_err(|e| format!("{}: {e}", src.display()))?;
    // Raw path parts (no wrapper stripped, so a single-stage archive's stage folder keeps its name). Unsafe paths
    // and directory entries are left out.
    let entries: Vec<Vec<String>> = listed.into_iter().filter(|e| !e.dir).filter_map(|e| e.parts).collect();
    if entries.iter().all(|p| {
        let n = p.last().map(|n| n.to_ascii_lowercase()).unwrap_or_default();
        n.ends_with(".dll") || n.ends_with(".exe")
    }) && !entries.is_empty()
    {
        return Err(format!("{}: no co-driver files in it, only executables", src.display()));
    }
    let name = pack_name(src);
    let extract = |dest: &Path, wanted: &[(Vec<String>, Vec<String>)]| extract_from(src, dest, wanted, cancel);
    let root = archive_plugin_root(&entries);
    let voice: Vec<(Vec<String>, Vec<String>)> = root
        .clone()
        .map(|root| {
            entries
                .iter()
                .filter(|p| p.len() >= root.len() && p[..root.len()] == root[..])
                .map(|p| p[root.len()..].to_vec())
                .filter(|p| !p.is_empty() && safe_path(&p.join("/")).is_some_and(|s| keep_voice(&s)))
                .map(|p| {
                    let mut from = root.clone();
                    from.extend(p.clone());
                    (from, p)
                })
                .collect()
        })
        .unwrap_or_default();
    let notes: Vec<Vec<String>> = archive_note_candidates(&entries, root.as_deref());
    match (voice.is_empty(), notes.is_empty()) {
        (true, true) => Err(format!(
            "{}: no co-driver voices, call styles or stage notes in it: is it a co-driver or note pack?",
            src.display()
        )),
        (false, true) => import_voice_from(home, &name, cancel, |d, _| {
            extract(d, &voice)?;
            Ok(walk(d).len())
        }),
        (true, false) => import_notes_from(home, &name, src, cancel, |d, _| {
            extract_notes(src, d, &notes, cancel)?;
            Ok(note_files(d).len())
        }),
        (false, false) => {
            let (v, (styles, ranges)) = stage_voice(home, &name, cancel, |d, _| {
                extract(d, &voice)?;
                Ok(walk(d).len())
            })?;
            let n = stage(home, NOTES_DIR, &name)?;
            extract_notes(src, &n.staged, &notes, cancel)?;
            if note_files(&n.staged).is_empty() {
                // Only unrelated INIs beside a valid voice: keep the voice, drop the empty notes staging.
                publish(&v, cancel)?;
                return Ok(vec![voice_line(&v.dest, &styles, &ranges)]);
            }
            let count = check_notes(&n.staged, src)?;
            publish_pair(&v, &n, cancel)?;
            Ok(vec![voice_line(&v.dest, &styles, &ranges), notes_line(&n.dest, count)])
        }
    }
}

/// Extracts every candidate note INI whole (paths stay whole, so stage folders keep their names) and prunes what
/// does not parse as a note file.
fn extract_notes(src: &Path, dest: &Path, notes: &[Vec<String>], cancel: &AtomicBool) -> Result<(), Fail> {
    let wanted: Vec<(Vec<String>, Vec<String>)> = notes.iter().map(|p| (p.clone(), p.clone())).collect();
    extract_from(src, dest, &wanted, cancel)?;
    prune_not_notes(dest);
    cancelled(cancel)
}

/// Extracts the ZIP's or 7z's `wanted` (raw entry path to pack-relative path) into `dest`, stopping on cancel.
fn extract_from(
    src: &Path,
    dest: &Path,
    wanted: &[(Vec<String>, Vec<String>)],
    cancel: &AtomicBool,
) -> Result<(), Fail> {
    let progress = &mut |_| cancelled(cancel);
    match src.to_string_lossy().to_ascii_lowercase().ends_with(ZIP) {
        true => crate::archive::extract_zip(src, dest, wanted, progress),
        false => crate::archive::extract_seven(src, dest, wanted, progress),
    }
}

/// Deletes the staged INIs that do not parse as note files.
fn prune_not_notes(dir: &Path) {
    for (_, path) in walk(dir) {
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ini")) && !is_note_file(&path) {
            let _ = fs::remove_file(&path);
        }
    }
}

/// Imports `src` (a folder, a ZIP/7z archive, or a whole install to copy from) into `home`: the lines saying what
/// was added. The source is only read, never modified; existing packs are never overwritten. `cancel` stops the
/// copy and the extraction; a cancelled or failed import publishes nothing.
pub fn import(home: &Path, src: &Path, cancel: &AtomicBool) -> Result<Vec<String>, Fail> {
    if !src.exists() {
        return Err(format!("{} does not exist", src.display()));
    }
    if src.is_dir() {
        return import_folder(home, src, cancel);
    }
    let lower = src.to_string_lossy().to_ascii_lowercase();
    match lower.ends_with(ZIP) || lower.ends_with(SEVEN_Z) {
        true => import_archive(home, src, cancel),
        false => Err(format!("{}: take a folder or a .zip/.7z download", src.display())),
    }
}

/// Removes the owned pack `name` below `home/dir` (its folder when there).
fn remove_pack(home: &Path, dir: &str, name: &str) -> Result<(), Fail> {
    let folder = safe_folder(name).ok_or_else(|| format!("a bad pack name {name}"))?;
    let path = home.join(dir).join(folder);
    match path.is_dir() {
        true => fs::remove_dir_all(&path).map_err(|e| format!("{}: {e}", path.display())),
        false => Ok(()),
    }
}

/// Removes the voice pack `name` from `home`.
pub fn remove_voice(home: &Path, name: &str) -> Result<(), Fail> {
    remove_pack(home, VOICES_DIR, name)
}

/// Removes the note pack `name` from `home`.
pub fn remove_notes(home: &Path, name: &str) -> Result<(), Fail> {
    remove_pack(home, NOTES_DIR, name)
}

/// The command line's `--import-codriver SRC`, into the player's folder.
pub fn cli(home: &Path, args: &[String]) -> Result<(), String> {
    let [src] = args else { return Err("usage: rerbr --import-codriver SRC".into()) };
    let lines = import(home, Path::new(src), &AtomicBool::new(false))?;
    lines.iter().for_each(|l| println!("{l}"));
    println!("{}", home.display());
    Ok(())
}
