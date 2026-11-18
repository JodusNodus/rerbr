//! The player's recent downloads that the Add recognises, offered with an "Add" button: the Downloads folder, the
//! archives modified in the last 30 days, and the reading of their entry names (only the names, never the
//! contents). The rules are pure functions over names and times; [`scan`] is the one that reads the disk, on a
//! thread.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{detect, source, KINDS};
use crate::codriver_packs::{SEVEN_Z, ZIP};
use crate::job::Job;

/// How many archives are offered, and how old they may be.
const MAX: usize = 5;
const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);

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

/// Whether the pack `owned` is the one `name` was added as: the same name or with an `-N` suffix.
fn is_name_of(owned: &str, name: &str) -> bool {
    owned == name
        || owned.strip_prefix(name).and_then(|r| r.strip_prefix('-')).is_some_and(|n| n.parse::<u32>().is_ok())
}

/// Whether a download named `name` (an archive's pack name) is among the `owned` pack names.
pub fn already_added(name: &str, owned: &[String]) -> bool {
    owned.iter().any(|o| is_name_of(o, name))
}

/// What the Add already has, to hide downloads of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Owned {
    /// The co-driver and pace-note pack names.
    pub packs: Vec<String>,
    /// The BTB stage folders in `tracks/`, lowercase.
    pub stages: Vec<String>,
    /// The cars added, by `car_packs::words`.
    pub cars: Vec<String>,
    /// The car skins imported, lower case (`car_skins::imported`).
    pub skins: Vec<String>,
    /// NGP 7.4.778 is in use.
    pub ngp: bool,
}

/// An offered download: the archive and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub label: String,
}

/// The offered downloads: scanned on a thread when the section is first shown and whenever what is owned changes.
#[derive(Default)]
pub struct Recent {
    pub list: Vec<Found>,
    /// What the list was scanned for (`None`: not yet).
    scanned_for: Option<Owned>,
    job: Option<Job<Vec<Found>>>,
    /// A folder to scan in place of the Downloads folder (tests).
    #[cfg(test)]
    pub dir: Option<PathBuf>,
}

impl Recent {
    /// Whether a scan of the downloads folder runs.
    pub fn scanning(&self) -> bool {
        self.job.is_some()
    }

    /// Starts a scan when `owned` differs from the last scanned and none runs; takes a finished one's list.
    pub fn poll(&mut self, ctx: &egui::Context, owned: Owned) {
        if let Some(result) = self.job.as_mut().and_then(Job::result) {
            self.job = None;
            self.list = result.unwrap_or_default();
        }
        if self.job.is_none() && self.scanned_for.as_ref() != Some(&owned) {
            self.scanned_for = Some(owned.clone());
            #[cfg(test)]
            let found = self.dir.clone().or_else(downloads_dir);
            #[cfg(not(test))]
            let found = downloads_dir();
            if let Some(dir) = found {
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

/// What the download `src` is for the player, when it is offered alone: its kinds' labels. `None`: not recognised,
/// not offered alone (a model or sound archive without its physics zip, a disc), or already added (`owned`).
pub fn offer(src: &dyn source::Source, owned: &Owned) -> Option<String> {
    let labels: Vec<&str> = detect(src).iter().filter_map(|(at, found)| KINDS[*at].offer(src, found, owned)).collect();
    (!labels.is_empty()).then(|| labels.join(" and "))
}

/// The recent downloads of `dir` the Add recognises and does not have yet, newest first, at most five. Reads the
/// disk.
pub fn scan(dir: &Path, owned: &Owned) -> Vec<Found> {
    let files = fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok).filter_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        let meta = e.metadata().ok().filter(|m| m.is_file())?;
        if !is_archive(&name) {
            return None;
        }
        Some((e.path(), meta.modified().ok()?))
    });
    let mut found = Vec::new();
    for (path, _) in recent(files.collect(), SystemTime::now()) {
        let Ok(src) = source::open(&path) else { continue };
        if let Some(label) = offer(&*src, owned) {
            found.push(Found { path, label });
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

    #[test]
    fn what_is_offered_and_what_is_hidden() {
        use crate::content::source::Fake;
        let stage = ["Lappi/track.ini", "Lappi/objectlist.ini", "Lappi/Objects.zip", "Lappi/Textures.zip"];
        let lappi = Fake::new("Lappi.zip", false, &stage);
        let none = Owned::default();
        assert_eq!(offer(&lappi, &none).as_deref(), Some("BTB stage"));
        let have = Owned { stages: names(&["lappi"]), ..Owned::default() };
        assert_eq!(offer(&lappi, &have), None);
        let mut both = stage.to_vec();
        both.push("Luppis/PaceNote.ini");
        let both = Fake::new("Luppis.zip", false, &both);
        assert_eq!(offer(&both, &have).as_deref(), Some("co-driver or pace notes"));
        let packs = Owned { packs: names(&["Luppis"]), ..have };
        assert_eq!(offer(&both, &packs), None);
        let physics = |name: &str| Fake::new(name, false, &["common.lsp"]);
        let car = Owned { cars: names(&["ford fiesta r5"]), ..Owned::default() };
        assert_eq!(offer(&physics("Ford Fiesta R5 - NGP Physics.zip"), &car), None);
        assert!(offer(&physics("Audi - NGP Physics.zip"), &car).is_some());
        let model = Fake::new("x - Model.7z", false, &["Cars/x/x.sgc"]);
        assert_eq!(offer(&model, &none), None);
        assert_eq!(offer(&Fake::new("RBR.iso", false, &[]), &none), None);
        assert_eq!(offer(&Fake::new("a.zip", false, &["a.txt"]), &none), None);
    }
}
