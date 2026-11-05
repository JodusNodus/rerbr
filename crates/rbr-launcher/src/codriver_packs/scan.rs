//! The owned packs on disk: listing and validating them ([`voice_packs`], [`note_packs`]).
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use rbr_formats::pacenote::{codriver, notes};

use super::{Fail, NOTES_DIR, SEVEN_Z, VOICES_DIR, ZIP};
use crate::archive::{safe_folder, safe_path};

/// A locally owned voice pack: its folder (as the plugin's folder) and what it offers, or why it is not ready
/// (the same validation `check_voice` refuses at activation, so a corrupt pack never shows ready).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoicePack {
    pub name: String,
    pub dir: PathBuf,
    pub voices: Vec<String>,
    pub styles: Vec<String>,
    pub ranges: Vec<String>,
    /// Why the pack is not ready (its missing dependency), if it is not.
    pub problem: Option<String>,
}

/// A locally owned stage-note pack: its folder, its note files and the note sets they make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotePack {
    pub name: String,
    pub dir: PathBuf,
    pub files: usize,
    pub sets: Vec<NoteSet>,
}

/// A note set's name as shown: its own, or for files named as their stage alone the pack's (the RSF install's:
/// MyPacenotes).
pub fn set_label(pack: Option<&str>, set: &str) -> String {
    match (set, pack) {
        ("", Some(pack)) => pack.to_owned(),
        ("", None) => "MyPacenotes".to_owned(),
        (set, _) => set.to_owned(),
    }
}

/// The note sets' inputs the summary follows (`Form::notes_inputs`).
pub type NotesInputs = (Option<String>, Option<String>, bool, bool, Option<String>);

/// What the co-driver says on the selected stage, as the Co-driver section and the Drive page show it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotesSummary {
    pub stage: String,
    /// Who calls which notes.
    pub text: String,
    /// The note file played, for the tooltip.
    pub file: Option<PathBuf>,
    /// The chosen set has no notes for the stage: its own notes play.
    pub fallback: bool,
}

/// A set of stage notes in a notes folder (an RSF install's MyPacenotes or an owned pack): the files that add the
/// same name to their stage's, as `LuppisV3` in `Tanner/Tanner LuppisV3.ini`. The name is empty for files named as
/// their stage alone (`Tanner/tanner.ini`, `Tanner.ini`). NGPCarMenu's `_default` exports, copies of the stages' own
/// notes, make no set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteSet {
    pub name: String,
    /// The stages it has notes for.
    pub stages: usize,
}

/// Whether the file name has an audio suffix the voices play.
fn is_audio(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".ogg", ".wav", ".mp3", ".flac", ".oga"].iter().any(|e| lower.ends_with(e))
}

/// Whether the relative path (from a plugin's folder) is a co-driver file worth owning: the settings, a config
/// definition, or voice audio. Everything else (DLLs, executables, editors, pictures) stays out.
pub(super) fn keep_voice(rel: &[&str]) -> bool {
    if rel.is_empty() {
        return false;
    }
    let lower: Vec<String> = rel.iter().map(|p| p.to_ascii_lowercase()).collect();
    if lower.len() == 1 && lower[0] == "pacenote.ini" {
        return true;
    }
    if lower[0] == "config" && lower.last().is_some_and(|n| n.ends_with(".ini")) {
        return true;
    }
    if lower[0] == "sounds" && rel.len() > 2 && is_audio(rel[rel.len() - 1]) {
        return true;
    }
    false
}

/// A pack name from the source's file or folder name: the archive's stem, else the folder's name, spaces kept.
pub(super) fn pack_name(src: &Path) -> String {
    let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    let stem = [ZIP, SEVEN_Z].iter().find_map(|e| lower.ends_with(e).then(|| name[..name.len() - e.len()].to_owned()));
    safe_folder(stem.as_deref().unwrap_or(&name)).unwrap_or_else(|| "pack".to_owned())
}

/// A free pack folder below `dir` for `name`: `name`, else `name-2`, `name-3`, ... (existing packs are never
/// overwritten).
pub(super) fn free_dir(dir: &Path, name: &str) -> PathBuf {
    if !dir.join(name).exists() {
        return dir.join(name);
    }
    for n in 2.. {
        let next = dir.join(format!("{name}-{n}"));
        if !next.exists() {
            return next;
        }
    }
    unreachable!()
}

/// The files below `dir` (relative `/`-separated paths and their absolute paths), sorted. Symlinks are never
/// followed or copied: a linked directory would loop, and a linked file belongs to its target's pack, not this one.
pub(super) fn walk(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(top) = stack.pop() {
        let Ok(entries) = fs::read_dir(&top) else { continue };
        for entry in entries.filter_map(Result::ok) {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(dir) {
                out.push((rel.to_string_lossy().replace('\\', "/"), path));
            }
        }
    }
    out.sort();
    out
}

/// Whether the folder looks like a Pacenote plugin's folder: it has the settings, the voices, or the definitions.
pub(super) fn is_plugin_dir(dir: &Path) -> bool {
    dir.join("PaceNote.ini").is_file()
        || dir.join("sounds").is_dir()
        || dir.join("config").join("pacenotes").is_dir()
        || dir.join("config").join("ranges").is_dir()
}

/// The plugin's folder below `dir`: itself, a direct `Pacenote` (or `Plugins/Pacenote`) folder beside note
/// folders, or the same under a single wrapper directory (up to two levels down). `None` when there is none
/// (notes-only sources reach here too: their stage folders stay where they are, since the note lookup is
/// recursive). Archive discovery (`archive_plugin_root`) mirrors this list.
pub(super) fn find_voice_root(dir: &Path) -> Option<PathBuf> {
    let candidates = voice_root_candidates(dir);
    candidates.into_iter().find(|c| is_plugin_dir(c))
}

/// The plugin folders to try below `dir`, in order: itself, the install's, each direct child, each child's
/// `Pacenote` and `Plugins/Pacenote`, then the same under a single wrapper directory (bounded: two levels).
fn voice_root_candidates(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![dir.to_owned(), dir.join("Plugins").join("Pacenote")];
    let children: Vec<PathBuf> = fs::read_dir(dir)
        .map(|e| e.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    // A download with the plugin beside the notes (`Pacenote/`, `MyPacenotes/`).
    for child in &children {
        out.push(child.clone());
    }
    for child in &children {
        out.push(child.join("Pacenote"));
        out.push(child.join("Plugins").join("Pacenote"));
    }
    // Up to two single wrapper directories down.
    let [child] = &children[..] else { return out };
    let grandchildren: Vec<PathBuf> = fs::read_dir(child)
        .map(|e| e.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    for grand in &grandchildren {
        out.push(grand.clone());
        out.push(grand.join("Pacenote"));
        out.push(grand.join("Plugins").join("Pacenote"));
    }
    out
}

/// Whether the text is a note file (the `[PACENOTES]` section with a count).
fn is_note_text(text: &str) -> bool {
    notes::parse(text).is_ok()
}

/// The note INI files below `dir` (relative paths), validated by parsing.
pub(super) fn note_files(dir: &Path) -> Vec<String> {
    walk(dir)
        .into_iter()
        .filter(|(rel, _)| rel.to_ascii_lowercase().ends_with(".ini"))
        .filter(|(_, path)| fs::read(path).ok().is_some_and(|b| is_note_text(&String::from_utf8_lossy(&b))))
        .map(|(rel, _)| rel)
        .collect()
}

/// Stops the import when `cancel` is set: every file and archive entry checks it.
pub(super) fn cancelled(cancel: &AtomicBool) -> Result<(), Fail> {
    match cancel.load(Ordering::Relaxed) {
        true => Err("cancelled".into()),
        false => Ok(()),
    }
}

/// The pack folders below `dir`: directories only, hidden staging folders (`.import-…`) and symlinks left out.
fn pack_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|e| {
            e.filter_map(Result::ok)
                .filter(|e| {
                    e.file_name().to_str().is_some_and(|n| !n.starts_with('.'))
                        && e.file_type().is_ok_and(|k| k.is_dir() && !k.is_symlink())
                })
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The voice packs owned (each folder of `home/codrivers/`), with their problem when not ready: the missing
/// part, or the validation (`check_voice`) failure `active_mods` would refuse with, so the list never shows a
/// corrupt pack as ready.
pub fn voice_packs(home: &Path) -> Vec<VoicePack> {
    pack_names(&home.join(VOICES_DIR))
        .into_iter()
        .map(|name| {
            let dir = home.join(VOICES_DIR).join(&name);
            let (voices, styles, ranges) = (codriver::voices(&dir), codriver::styles(&dir), codriver::range_sets(&dir));
            let mut problem = voice_problem(&voices, &styles, &ranges);
            if problem.is_none() {
                problem = voice_check_problem(&dir);
            }
            VoicePack { name, dir, voices, styles, ranges, problem }
        })
        .collect()
}

/// The `check_voice` failure `active_mods` would refuse the pack with, without its folder prefix, if it fails.
fn voice_check_problem(dir: &Path) -> Option<String> {
    check_voice(dir).err().map(|e| e.strip_prefix(&format!("{}: ", dir.display())).unwrap_or(&e).to_owned())
}

/// Why the pack's voices, styles and range sets are not a ready co-driver, if they are not: each missing part
/// is named on its own, so an audio-only download learns which definitions to add.
fn voice_problem(voices: &[String], styles: &[String], ranges: &[String]) -> Option<String> {
    match (voices.is_empty(), styles.is_empty(), ranges.is_empty()) {
        (true, true, true) => Some("no voices, call styles or distance calls: is it a co-driver pack?".into()),
        (true, _, _) => Some(
            "no voices (sounds/*/*.ogg): import the voice pack these call styles need, or pick another pack".into(),
        ),
        (_, true, _) => Some(
            "no call styles (config/pacenotes/*.ini): import the style pack this voice needs, or pick another pack"
                .into(),
        ),
        (_, _, true) => Some(
            "no distance calls (config/ranges/*.ini): import the range set this voice needs, or pick another pack"
                .into(),
        ),
        _ => None,
    }
}

/// The stage-note packs owned (each folder of `home/notes/` with at least one note file).
pub fn note_packs(home: &Path) -> Vec<NotePack> {
    pack_names(&home.join(NOTES_DIR))
        .into_iter()
        .map(|name| {
            let dir = home.join(NOTES_DIR).join(&name);
            let (files, sets) = (note_files(&dir).len(), note_sets(&dir));
            NotePack { name, dir, files, sets }
        })
        .filter(|p| p.files > 0)
        .collect()
}

/// The separators trimmed off a note set's name.
const SEPARATORS: [char; 4] = [' ', '_', '-', '.'];

/// A note INI below a notes folder: its stage folder's name (a file at the top: its stem), its stem less a `_<n>`
/// version suffix (an edited copy: `Kormoran I LuppisV3_1`, `_1.1`), and that version (none: empty, the lowest). `None` for other files and
/// for `_default` exports.
fn note_name(rel: &str) -> Option<(&str, &str, Vec<u32>)> {
    if !rel.to_ascii_lowercase().ends_with(".ini") {
        return None;
    }
    let path = Path::new(rel);
    let stem = path.file_stem()?.to_str()?;
    if stem.to_ascii_lowercase().ends_with("_default") {
        return None;
    }
    let stage = path.parent().and_then(Path::file_name).and_then(|n| n.to_str()).unwrap_or(stem);
    Some(match stem.rsplit_once('_') {
        Some((head, n))
            if n.starts_with(|c: char| c.is_ascii_digit()) && n.bytes().all(|b| b.is_ascii_digit() || b == b'.') =>
        {
            (stage, head, n.split('.').map(|part| part.parse().unwrap_or(0)).collect())
        }
        _ => (stage, stem, Vec::new()),
    })
}

/// What `stem` adds after the stage's name, matched as [`fold`] matches (`Ul_ Rajdowa LuppisV3` in `Ul. Rajdowa/`),
/// or `None` when it does not start with it.
fn after_stage<'a>(stem: &'a str, stage: &str) -> Option<&'a str> {
    let key = fold(stage);
    let cut = stem.char_indices().map(|(i, c)| i + c.len_utf8()).find(|&i| !key.is_empty() && fold(&stem[..i]) == key);
    cut.map(|i| &stem[i..])
}

/// The stem and its word suffixes, longest first: `Bareyo LuppisV3`, `LuppisV3`.
fn suffixes(stem: &str) -> impl Iterator<Item = &str> {
    std::iter::once(stem).chain(stem.match_indices(&SEPARATORS[..]).map(move |(i, _)| &stem[i + 1..]))
}

/// A note INI as a note set's file.
struct SetFile {
    /// Its stage folder's name, folded.
    stage: String,
    set: String,
    version: Vec<u32>,
    path: PathBuf,
}

/// The note INIs below the notes folder `dir` by note set. A file's set is what its stem adds to its stage's name
/// (`Tanner/Tanner LuppisV3.ini`: LuppisV3; empty when the stem is the stage's name alone), or the whole stem when
/// it does not start with it (`Bareyo LuppisV3` in `Bareyo BTB/`, a misspelt stage); unless other stages' files end
/// in that too, the word suffix the most of them end in (`Bareyo LuppisV3`, `Again LuppisV3`: LuppisV3). Names only:
/// nothing is parsed.
fn set_files(dir: &Path) -> Vec<SetFile> {
    let named: Vec<(String, String, Vec<u32>, PathBuf)> = walk(dir)
        .into_iter()
        .filter_map(|(rel, path)| {
            let (stage, stem, version) = note_name(&rel)?;
            Some((stage.to_owned(), stem.to_owned(), version, path))
        })
        .collect();
    // The stages whose file names each word suffix ends.
    let mut shared: std::collections::HashMap<String, std::collections::HashSet<String>> = Default::default();
    for (stage, stem, ..) in &named {
        for suffix in suffixes(stem) {
            shared.entry(suffix.to_ascii_lowercase()).or_default().insert(fold(stage));
        }
    }
    let stages = |suffix: &str| shared.get(&suffix.to_ascii_lowercase()).map_or(0, |stages| stages.len());
    named
        .iter()
        .map(|(stage, stem, version, path)| {
            let base = after_stage(stem, stage).map_or(stem.as_str(), |rest| rest.trim_start_matches(SEPARATORS));
            let set = match stages(base) > 1 {
                true => base,
                false => suffixes(base).filter(|s| stages(s) > 1).max_by_key(|s| (stages(s), s.len())).unwrap_or(base),
            };
            SetFile {
                stage: fold(stage),
                set: set.trim_matches(SEPARATORS).to_owned(),
                version: version.clone(),
                path: path.clone(),
            }
        })
        .collect()
}

/// The note sets in the notes folder `dir`, the most stages first (then by name).
pub fn note_sets(dir: &Path) -> Vec<NoteSet> {
    let mut pairs: Vec<(String, String, String)> =
        set_files(dir).into_iter().map(|f| (f.set.to_ascii_lowercase(), f.stage, f.set)).collect();
    pairs.sort();
    pairs.dedup_by(|a, b| (&a.0, &a.1) == (&b.0, &b.1));
    let mut sets: Vec<NoteSet> = Vec::new();
    for (key, _, name) in pairs {
        match sets.last_mut() {
            Some(last) if last.name.eq_ignore_ascii_case(&key) => last.stages += 1,
            _ => sets.push(NoteSet { name, stages: 1 }),
        }
    }
    sets.sort_by(|a, b| b.stages.cmp(&a.stages).then_with(|| a.name.cmp(&b.name)));
    sets
}

/// The note file of the set `set` in the notes folder `dir` for the stage called by one of `names` (stage folders
/// and file stems matched ignoring case, accents and punctuation, as `notes::my_pacenotes`), its highest version
/// that parses.
pub fn find_in_set(dir: &Path, names: &[&str], set: &str) -> Option<PathBuf> {
    let wanted: Vec<String> = names.iter().map(|n| fold(n)).filter(|n| !n.is_empty()).collect();
    let mut found: Vec<SetFile> =
        set_files(dir).into_iter().filter(|f| f.set.eq_ignore_ascii_case(set) && wanted.contains(&f.stage)).collect();
    found.sort_by(|a, b| b.version.cmp(&a.version));
    found.into_iter().map(|f| f.path).find(|path| is_note_file(path))
}

/// Whether the file parses as a note file.
pub(super) fn is_note_file(path: &Path) -> bool {
    fs::read(path).ok().is_some_and(|b| is_note_text(&String::from_utf8_lossy(&b)))
}

/// A name for matching: lowercase ASCII letters and digits, accented Latin letters as their base letter (as
/// `notes` matches stage folders).
fn fold(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        let base = match c {
            'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'ď' | 'đ' => 'd',
            'è'..='ë' | 'ē' | 'ė' | 'ę' | 'ě' => 'e',
            'ì'..='ï' | 'ī' | 'į' => 'i',
            'ł' => 'l',
            'ñ' | 'ń' | 'ň' => 'n',
            'ò'..='ö' | 'ø' | 'ō' | 'ő' => 'o',
            'ř' => 'r',
            'ś' | 'š' | 'ß' => 's',
            'ť' => 't',
            'ù'..='ü' | 'ū' | 'ů' | 'ű' | 'ų' => 'u',
            'ý' | 'ÿ' => 'y',
            'ź' | 'ż' | 'ž' => 'z',
            c if c.is_ascii_alphanumeric() => c,
            _ => continue,
        };
        out.push(base);
    }
    out
}

/// Writes `bytes` to `to`, making its folders.
fn copy_file(bytes: &[u8], to: &Path) -> Result<(), Fail> {
    fs::create_dir_all(to.parent().unwrap_or(to)).map_err(|e| format!("{}: {e}", to.display()))?;
    fs::write(to, bytes).map_err(|e| format!("{}: {e}", to.display()))
}

/// Copies the voice files (see [`keep_voice`]) from the plugin's folder `src` into `dest`, preserving relative
/// paths; the files copied.
pub(super) fn copy_voice(src: &Path, dest: &Path, cancel: &AtomicBool) -> Result<usize, Fail> {
    let mut n = 0;
    for (rel, path) in walk(src) {
        cancelled(cancel)?;
        let parts = safe_path(&rel).ok_or_else(|| format!("{}: a bad path {rel}", src.display()))?;
        if !keep_voice(&parts) {
            continue;
        }
        let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let to = parts.iter().fold(dest.to_owned(), |p, part| p.join(part));
        copy_file(&bytes, &to)?;
        n += 1;
    }
    Ok(n)
}

/// Copies the note files below `src` into `dest`, preserving relative paths; the files copied. Only files that
/// parse as note files are kept, so install leftovers never ride along. A source that is one stage's own folder
/// (its note INIs at the top, none in subfolders) keeps its folder's name over them, so the stage identity
/// survives: a bare `Tanner/` folder still resolves as Tanner.
pub(super) fn copy_notes(src: &Path, dest: &Path, cancel: &AtomicBool) -> Result<usize, Fail> {
    let mut found: Vec<(String, Vec<u8>)> = Vec::new();
    for (rel, path) in walk(src) {
        cancelled(cancel)?;
        if !rel.to_ascii_lowercase().ends_with(".ini") {
            continue;
        }
        let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !is_note_text(&String::from_utf8_lossy(&bytes)) {
            continue;
        }
        found.push((rel, bytes));
    }
    let nested = found.iter().any(|(rel, _)| rel.contains('/'));
    let stage = (!nested).then(|| src.file_name().and_then(|n| n.to_str()).and_then(safe_folder)).flatten();
    let mut n = 0;
    for (rel, bytes) in &found {
        cancelled(cancel)?;
        let rel = match &stage {
            Some(stage) => format!("{stage}/{rel}"),
            None => rel.clone(),
        };
        let parts = safe_path(&rel).ok_or_else(|| format!("{}: a bad path {rel}", src.display()))?;
        let to = parts.iter().fold(dest.to_owned(), |p, part| p.join(part));
        copy_file(bytes, &to)?;
        n += 1;
    }
    Ok(n)
}

/// Validates a staged voice pack: its styles and range sets read with actual call definitions, and its first
/// style with its first range set loads (catching dangling package references). Audio-only downloads fail here
/// with what to add.
pub(super) fn check_voice(dir: &Path) -> Result<(Vec<String>, Vec<String>), Fail> {
    let (voices, styles, ranges) = (codriver::voices(dir), codriver::styles(dir), codriver::range_sets(dir));
    if let Some(problem) = voice_problem(&voices, &styles, &ranges) {
        return Err(format!("{}: {problem}", dir.display()));
    }
    let calls = codriver::load(dir, &styles[0], &ranges[0]).map_err(|e| format!("{}: {e}", dir.display()))?;
    if calls.notes.is_empty() && calls.named.is_empty() {
        return Err(format!("{}: {} defines no calls", dir.display(), styles[0]));
    }
    if calls.ranges.is_empty() {
        return Err(format!("{}: {} defines no distance calls", dir.display(), ranges[0]));
    }
    Ok((styles, ranges))
}

/// Whether the plugin's styles, and its range sets, say different things: each style loaded with the first range
/// set (each range set with the first style) and compared call by call. When they say the same, the voice decides
/// the words and the launcher hides the choice. Read at rescan, never per draw.
pub fn call_sets_differ(dir: &Path, styles: &[String], ranges: &[String]) -> (bool, bool) {
    let (Some(style), Some(range)) = (styles.first(), ranges.first()) else { return (false, false) };
    let differ = |sets: Vec<codriver::Calls>, same: fn(&codriver::Calls, &codriver::Calls) -> bool| {
        sets.iter().any(|set| !same(set, &sets[0]))
    };
    let styles = styles.iter().filter_map(|s| codriver::load(dir, s, range).ok()).collect();
    let ranges = ranges.iter().filter_map(|r| codriver::load(dir, style, r).ok()).collect();
    (differ(styles, |a, b| a.notes == b.notes), differ(ranges, |a, b| a.ranges == b.ranges))
}
