//! The player's recent downloads that look like a co-driver or pace-note pack, offered with an "Add" button: the
//! Downloads folder, the archives modified in the last 30 days, and the reading of their entry names (only the
//! names, never the contents). The rules are pure functions over names and times; [`scan`] is the one that reads
//! the disk, on a thread.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::scan::pack_name;
use crate::archive::{seven_entries, zip_entries};
use crate::codriver_packs::{SEVEN_Z, ZIP};
use crate::job::Job;

/// How many archives are offered, and how old they may be.
const MAX: usize = 5;
const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
const AUDIO: [&str; 5] = [".ogg", ".wav", ".mp3", ".flac", ".oga"];

/// The `XDG_DOWNLOAD_DIR` of a `user-dirs.dirs` file, with `$HOME` expanded.
pub fn user_dirs_download(text: &str, home: &Path) -> Option<PathBuf> {
    let value = text.lines().find_map(|l| l.trim().strip_prefix("XDG_DOWNLOAD_DIR="))?;
    let value = value.trim().trim_matches('"');
    let path = match value.strip_prefix("$HOME") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(value),
    };
    (!value.is_empty() && path.is_absolute()).then_some(path)
}

/// The player's Downloads folder from the environment (`None` when there is no home).
pub fn downloads_dir() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    let home = if cfg!(windows) { var("USERPROFILE") } else { var("HOME") }.map(PathBuf::from)?;
    if cfg!(all(unix, not(target_os = "macos"))) {
        let config = var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
        let text = fs::read_to_string(config.join("user-dirs.dirs")).unwrap_or_default();
        if let Some(dir) = user_dirs_download(&text, &home) {
            return Some(dir);
        }
    }
    Some(home.join("Downloads"))
}

/// Whether an archive named `archive` with these entry names looks like a co-driver or pace-note pack.
pub fn looks_like_pack(archive: &str, entries: &[String]) -> bool {
    let lower = archive.to_ascii_lowercase();
    let named = ["pacenote", "luppis", "codriver", "co-driver"].iter().any(|k| lower.contains(k));
    entries.iter().any(|entry| {
        let e = entry.replace('\\', "/").to_ascii_lowercase();
        e.rsplit('/').next() == Some("pacenote.ini")
            || e.contains("config/pacenotes/")
            || e.contains("mypacenotes/")
            || (e.contains("sounds/") && AUDIO.iter().any(|a| e.ends_with(a)))
            || (named && e.contains('/') && e.ends_with(".ini"))
    })
}

/// Whether the pack `owned` is the one `name` was added as: the same name or with an `-N` suffix.
fn is_name_of(owned: &str, name: &str) -> bool {
    owned == name
        || owned.strip_prefix(name).and_then(|r| r.strip_prefix('-')).is_some_and(|n| n.parse::<u32>().is_ok())
}

/// Whether a download named `name` (an archive's pack name) is among the `owned` pack names.
pub fn already_added(name: &str, owned: &[String]) -> bool {
    owned.iter().any(|o| is_name_of(o, name))
}

/// The offered downloads: scanned on a thread when the section is first shown and whenever the owned packs change.
#[derive(Default)]
pub struct Recent {
    pub list: Vec<PathBuf>,
    /// The owned pack names the list was scanned for (`None`: not yet).
    scanned_for: Option<Vec<String>>,
    job: Option<Job<Vec<PathBuf>>>,
}

impl Recent {
    /// Starts a scan when `owned` differs from the last scanned and none runs; takes a finished one's list.
    pub fn poll(&mut self, ctx: &egui::Context, owned: Vec<String>) {
        if let Some(result) = self.job.as_mut().and_then(Job::result) {
            self.job = None;
            self.list = result.unwrap_or_default();
        }
        if self.job.is_none() && self.scanned_for.as_ref() != Some(&owned) {
            self.scanned_for = Some(owned.clone());
            if let Some(dir) = downloads_dir() {
                self.job = Some(Job::start(move |_, _| Ok(scan(&dir, &owned))));
            }
        }
        if self.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }
}

/// The newest `MAX` of the candidates (path, modified) no older than 30 days at `now`, newest first.
pub fn recent(mut candidates: Vec<(PathBuf, SystemTime)>, now: SystemTime) -> Vec<(PathBuf, SystemTime)> {
    candidates.retain(|(_, t)| now.duration_since(*t).map_or(true, |age| age <= MAX_AGE));
    candidates.sort_by_key(|c| std::cmp::Reverse(c.1));
    candidates
}

fn is_archive(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(ZIP) || lower.ends_with(SEVEN_Z)
}

fn entry_names(path: &Path) -> Vec<String> {
    let entries = match path.extension().map(OsString::from) {
        Some(e) if e.eq_ignore_ascii_case("7z") => seven_entries(path),
        _ => zip_entries(path),
    };
    entries.map(|e| e.into_iter().map(|e| e.name).collect()).unwrap_or_default()
}

/// The recent pack-like archives of `dir` not yet in `owned`, newest first, at most five. Reads the disk.
pub fn scan(dir: &Path, owned: &[String]) -> Vec<PathBuf> {
    let files = fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok).filter_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        let meta = e.metadata().ok().filter(|m| m.is_file())?;
        let modified = meta.modified().ok()?;
        (is_archive(&name) && !already_added(&pack_name(&e.path()), owned)).then(|| (e.path(), modified))
    });
    let mut found = Vec::new();
    for (path, _) in recent(files.collect(), SystemTime::now()) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if looks_like_pack(&name, &entry_names(&path)) {
            found.push(path);
            if found.len() == MAX {
                break;
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn user_dirs_are_read() {
        let home = Path::new("/home/p");
        let text = "# c\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_DOWNLOAD_DIR=\"$HOME/Hent\"\n";
        assert_eq!(user_dirs_download(text, home), Some(PathBuf::from("/home/p/Hent")));
        assert_eq!(user_dirs_download("XDG_DOWNLOAD_DIR=\"/data/dl\"", home), Some(PathBuf::from("/data/dl")));
        assert_eq!(user_dirs_download("XDG_DOWNLOAD_DIR=\"$HOME/\"", home), Some(PathBuf::from("/home/p")));
        assert_eq!(user_dirs_download("XDG_DOWNLOAD_DIR=\"rel\"", home), None);
        assert_eq!(user_dirs_download("XDG_MUSIC_DIR=\"$HOME/Music\"", home), None);
    }

    #[test]
    fn packs_are_recognised_by_their_entries() {
        assert!(looks_like_pack("a.zip", &names(&["Voice/PaceNote.ini"])));
        assert!(looks_like_pack("a.zip", &names(&["x\\config\\pacenotes\\v3.ini"])));
        assert!(looks_like_pack("a.zip", &names(&["Plugins/Pacenote/sounds/pkg/1.ogg"])));
        assert!(looks_like_pack("a.zip", &names(&["MyPacenotes/Finland/x.ini"])));
        assert!(!looks_like_pack("a.zip", &names(&["sounds/readme.txt", "game.exe"])));
        assert!(!looks_like_pack("a.zip", &names(&["Finland/x.ini"])));
        assert!(looks_like_pack("Luppis-notes.zip", &names(&["Finland/x.ini"])));
        assert!(looks_like_pack("My Co-Driver.7z", &names(&["Finland/x.ini"])));
        assert!(!looks_like_pack("Luppis.zip", &names(&["x.ini"])));
    }

    #[test]
    fn added_packs_are_hidden() {
        let owned = names(&["Luppis", "Anna-2"]);
        assert!(already_added("Luppis", &owned));
        assert!(already_added("Anna", &owned));
        assert!(!already_added("Ann", &owned));
        assert!(!already_added("Luppis V3", &owned));
    }

    #[test]
    fn old_downloads_drop_and_the_newest_come_first() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100 * 24 * 3600);
        let ago = |days: u64| now - Duration::from_secs(days * 24 * 3600);
        let list = vec![("a".into(), ago(3)), ("old".into(), ago(31)), ("b".into(), ago(1)), ("c".into(), ago(30))];
        let paths: Vec<PathBuf> = recent(list, now).into_iter().map(|(p, _)| p).collect();
        assert_eq!(paths, ["b", "a", "c"].map(PathBuf::from));
    }
}
