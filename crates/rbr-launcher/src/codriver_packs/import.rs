//! Importing downloads and folders into the owned packs ([`import`], [`cli`]).
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rbr_formats::pacenote::notes;

use super::scan::INSTALL_DEPTH;
use super::scan::{
    cancelled, check_voice, copy_notes, copy_voice, find_installs, find_voice_root, free_dir, has_voices, install_name,
    is_note_file, is_plugin_dir, keep_voice, note_files, pack_name, walk,
};
use super::{Fail, NOTES_DIR, SEVEN_Z, VOICES_DIR, ZIP};
use crate::archive::{safe_folder, safe_path};
use crate::job::with_path;
use crate::job::Temp;

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
        return match (voice, notes::my_pacenotes_dir(src)) {
            (None, None) => Err(format!(
                "{}: no Pacenote plugin (Plugins/Pacenote) and no stage notes (Plugins/NGPCarMenu/MyPacenotes): is it \
                 an RBR install?",
                src.display()
            )),
            (Some(plugin), None) => import_voice_from(home, &name, cancel, |d, c| copy_voice(&plugin, d, c)),
            (None, Some(notes)) => import_notes_from(home, &name, &notes, cancel, |d, c| copy_notes(&notes, d, c)),
            (Some(plugin), Some(notes)) => import_both(home, &name, cancel, &plugin, &notes),
        };
    }
    let installs = find_installs(src);
    if installs.len() > 1 {
        return import_folder_installs(home, src, &installs, cancel);
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

/// The most packs of one source that still get a line each; more get one summary line.
const LINES_PER_SOURCE: usize = 4;

/// Imports every install of one source as its own pack `<source> - <install>`: `one` imports the install at an index
/// under a pack name. A failing install is reported (`Failed co-driver …`) and the others still import; the source
/// fails only when none did, and a cancel stops the whole job. `notes` imports what lies beside the installs.
fn import_each(
    src: &Path,
    names: &[String],
    cancel: &AtomicBool,
    one: impl Fn(usize, &str) -> Result<Vec<String>, Fail>,
    notes: impl FnOnce() -> Option<Result<Vec<String>, Fail>>,
) -> Result<Vec<String>, Fail> {
    let base = pack_name(src);
    let (mut added, mut failed) = (Vec::new(), Vec::new());
    for (i, name) in names.iter().enumerate() {
        cancelled(cancel)?;
        let pack = format!("{base} - {name}");
        match one(i, &pack) {
            Ok(lines) => added.extend(lines),
            Err(e) => {
                cancelled(cancel)?;
                failed.push(format!("Failed co-driver {pack}: {e}"));
            }
        }
    }
    let noted = notes();
    match noted {
        Some(Ok(lines)) => added.extend(lines),
        Some(Err(e)) => {
            cancelled(cancel)?;
            failed.push(format!("Failed stage notes {base}: {e}"));
        }
        None => {}
    }
    if added.is_empty() {
        return Err(failed.join("\n"));
    }
    if added.len() > LINES_PER_SOURCE {
        let packs: Vec<&str> = added.iter().filter_map(|l| l.strip_prefix("Co-driver ")?.split(':').next()).collect();
        let summary = format!("Co-driver {base}: {} packs ({})", packs.len(), packs.join(", "));
        let others = added.iter().filter(|l| !l.starts_with("Co-driver ")).cloned();
        added = std::iter::once(summary).chain(others).collect();
    }
    added.extend(failed);
    Ok(added)
}

/// Imports a folder holding several plugin installs: each one's plugin folder as its own pack.
fn import_folder_installs(
    home: &Path,
    src: &Path,
    installs: &[(PathBuf, Option<String>)],
    cancel: &AtomicBool,
) -> Result<Vec<String>, Fail> {
    let fallback = pack_name(src);
    let names: Vec<String> = installs.iter().map(|(_, n)| n.clone().unwrap_or_else(|| fallback.clone())).collect();
    import_each(
        src,
        &names,
        cancel,
        |i, pack| import_voice_from(home, pack, cancel, |d, c| copy_voice(&installs[i].0, d, c)),
        || {
            (!note_files(src).is_empty())
                .then(|| import_notes_from(home, &pack_name(src), src, cancel, |d, c| copy_notes(src, d, c)))
        },
    )
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
    fs::create_dir_all(&base).map_err(with_path(&base))?;
    let dest = free_dir(&base, &safe_folder(name).unwrap_or_else(|| "pack".to_owned()));
    let temp = Temp::create(&base, "import")?;
    let staged = temp.0.join("pack");
    fs::create_dir(&staged).map_err(with_path(&staged))?;
    Ok(Staged { _temp: temp, staged, dest })
}

/// Publishes a validated staged pack by rename; dropping the staging removes its temp folder. Checks the
/// cancellation flag immediately before the commit, so a cancel after staging publishes nothing.
pub(super) fn publish(op: &Staged, cancel: &AtomicBool) -> Result<(), Fail> {
    cancelled(cancel)?;
    fs::rename(&op.staged, &op.dest).map_err(with_path(&op.dest))
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
    format!("Co-driver {}: {} call styles, {} range sets.", folder_name(dest), styles.len(), ranges.len())
}

/// The line saying what note pack published.
fn notes_line(dest: &Path, n: usize) -> String {
    format!("Stage notes {}: {n} files.", folder_name(dest))
}

fn folder_name(dest: &Path) -> std::borrow::Cow<'_, str> {
    dest.file_name().unwrap_or_default().to_string_lossy()
}

/// Whether `src` is named as a ZIP (else a 7z).
fn is_zip(src: &Path) -> bool {
    src.to_string_lossy().to_ascii_lowercase().ends_with(ZIP)
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

/// Every plugin install in the entries (raw path parts of the plugin's folder, sorted): at any depth up to a bound,
/// never inside a found plugin, each with voices or definitions. Mirrors `find_installs`.
fn archive_installs(entries: &[Vec<String>]) -> Vec<Vec<String>> {
    let dirs: std::collections::BTreeSet<&[String]> =
        entries.iter().flat_map(|p| (0..p.len().min(INSTALL_DEPTH + 1)).map(move |n| &p[..n])).collect();
    let mut found: Vec<Vec<String>> = Vec::new();
    for dir in dirs {
        if found.iter().any(|f| dir.starts_with(f)) {
            continue;
        }
        let rels = entries.iter().filter_map(|p| p.strip_prefix(dir)).map(<[String]>::to_vec);
        if archive_plugin_at(entries, dir) && has_voices(rels) {
            found.push(dir.to_vec());
        }
    }
    found
}

/// The entries below the plugin `root` worth owning: (raw path, path in the pack).
fn archive_voice(entries: &[Vec<String>], root: &[String]) -> Vec<(Vec<String>, Vec<String>)> {
    entries
        .iter()
        .filter_map(|p| p.strip_prefix(root))
        .filter(|p| !p.is_empty() && safe_path(&p.join("/")).is_some_and(|s| keep_voice(&s)))
        .map(|p| ([root, p].concat(), p.to_vec()))
        .collect()
}

/// The note files among the entries: every INI except the voice files below the plugins' `roots` (the style and
/// range definitions are not stage notes; anything else unparseable is pruned after extraction). Paths stay whole,
/// so a single-stage archive's stage folder keeps its name.
fn archive_note_candidates(entries: &[Vec<String>], roots: &[Vec<String>]) -> Vec<Vec<String>> {
    entries
        .iter()
        .filter(|p| p.last().is_some_and(|n| n.to_ascii_lowercase().ends_with(".ini")))
        .filter(|p| match roots.iter().find(|root| p.len() > root.len() && p.starts_with(root)) {
            Some(root) => {
                let below: Vec<&str> = p[root.len()..].iter().map(String::as_str).collect();
                !keep_voice(&below)
            }
            None => true,
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
    let listed = match is_zip(src) {
        true => crate::archive::zip_entries(src),
        false => crate::archive::seven_entries(src),
    }
    .map_err(with_path(src))?;
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
    let installs = archive_installs(&entries);
    if installs.len() > 1 {
        return import_archive_installs(home, src, &entries, &installs, cancel);
    }
    let name = pack_name(src);
    let extract = |dest: &Path, wanted: &[(Vec<String>, Vec<String>)]| extract_from(src, dest, wanted, cancel);
    let root: Vec<Vec<String>> = archive_plugin_root(&entries).into_iter().collect();
    let voice = root.first().map(|root| archive_voice(&entries, root)).unwrap_or_default();
    let extract_voice = |d: &Path, _: &AtomicBool| {
        extract(d, &voice)?;
        Ok(walk(d).len())
    };
    let notes = archive_note_candidates(&entries, root.as_slice());
    match (voice.is_empty(), notes.is_empty()) {
        (true, true) => Err(format!(
            "{}: no co-driver voices, call styles or stage notes in it: is it a co-driver or note pack?",
            src.display()
        )),
        (false, true) => import_voice_from(home, &name, cancel, extract_voice),
        (true, false) => import_notes_from(home, &name, src, cancel, |d, _| {
            extract_notes(src, d, &notes, cancel)?;
            Ok(note_files(d).len())
        }),
        (false, false) => {
            let (v, (styles, ranges)) = stage_voice(home, &name, cancel, extract_voice)?;
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

/// Imports an archive holding several plugin installs: each one's plugin folder as its own pack.
fn import_archive_installs(
    home: &Path,
    src: &Path,
    entries: &[Vec<String>],
    installs: &[Vec<String>],
    cancel: &AtomicBool,
) -> Result<Vec<String>, Fail> {
    let fallback = pack_name(src);
    let names: Vec<String> =
        installs.iter().map(|r| install_name(r).map_or_else(|| fallback.clone(), str::to_owned)).collect();
    let notes = archive_note_candidates(entries, installs);
    import_each(
        src,
        &names,
        cancel,
        |i, pack| {
            let voice = archive_voice(entries, &installs[i]);
            import_voice_from(home, pack, cancel, |d, _| {
                extract_from(src, d, &voice, cancel)?;
                Ok(walk(d).len())
            })
        },
        || {
            // INIs that prune to no note file are unrelated, as in a single install: nothing to report.
            let result = (!notes.is_empty()).then(|| {
                import_notes_from(home, &pack_name(src), src, cancel, |d, _| {
                    extract_notes(src, d, &notes, cancel)?;
                    Ok(note_files(d).len())
                })
            });
            result.filter(|r| r.is_ok() || cancel.load(std::sync::atomic::Ordering::Relaxed))
        },
    )
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
    match is_zip(src) {
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
        true => fs::remove_dir_all(&path).map_err(with_path(&path)),
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
