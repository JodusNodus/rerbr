//! The player's folder and its [`TRACKS`] folder: BTB stages dropped there, by hand or by a converter
//! (`rbr-ac-tracks`), are listed by the launcher beside the RSF install's. The player's folder (the launcher's files:
//! settings, saves, records) is the platform's user data folder (macOS `~/Library/Application Support/ReRBR`, Windows
//! `%APPDATA%\ReRBR`, Linux `$XDG_DATA_HOME/rerbr`, default `~/.local/share/rerbr`), or `$RERBR_HOME`; the launcher
//! and the converters take it from here.

use std::ffi::OsString;
use std::path::PathBuf;

/// The folder of the player's own stages in the player's folder, one folder per stage: a BTB stage's `tracks/<name>/`,
/// or a classic stage the launcher added, `tracks/<number>-<name>/` in the shape of a RallySimFans stage folder.
pub const TRACKS: &str = "tracks";

/// The player's folder: `$RERBR_HOME`, else the platform's user data folder; `None` without `HOME` (or `APPDATA` on
/// Windows).
pub fn player_folder() -> Option<PathBuf> {
    folder(|key| std::env::var_os(key), std::env::consts::OS)
}

/// The player's stages, `<player folder>/tracks`.
pub fn tracks() -> Option<PathBuf> {
    player_folder().map(|p| p.join(TRACKS))
}

/// The folder on `os` (`std::env::consts::OS`).
fn folder(env: impl Fn(&str) -> Option<OsString>, os: &str) -> Option<PathBuf> {
    let var = |key| env(key).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = var("RERBR_HOME") {
        return Some(dir);
    }
    if os == "windows" {
        return var("APPDATA").map(|d| d.join("ReRBR"));
    }
    let home = var("HOME");
    if os == "macos" {
        return home.map(|h| h.join("Library/Application Support/ReRBR"));
    }
    // The XDG spec ignores a relative XDG_DATA_HOME.
    let data = var("XDG_DATA_HOME").filter(|d| d.is_absolute()).or_else(|| home.map(|h| h.join(".local/share")));
    data.map(|d| d.join("rerbr"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn env<'a>(vars: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |key| vars.iter().find(|(k, _)| *k == key).map(|(_, v)| OsString::from(v))
    }

    #[test]
    fn the_folder_is_the_platforms_or_rerbr_home() {
        let home = [("HOME", "/h")];
        assert_eq!(folder(env(&home), "macos"), Some(PathBuf::from("/h/Library/Application Support/ReRBR")));
        assert_eq!(folder(env(&home), "linux"), Some(PathBuf::from("/h/.local/share/rerbr")));
        // Absolute on the host the test runs on.
        let x = if cfg!(windows) { r"C:\x" } else { "/x" };
        let xdg = [("HOME", "/h"), ("XDG_DATA_HOME", x)];
        assert_eq!(folder(env(&xdg), "linux"), Some(Path::new(x).join("rerbr")));
        let relative = [("HOME", "/h"), ("XDG_DATA_HOME", "x")];
        assert_eq!(folder(env(&relative), "linux"), Some(PathBuf::from("/h/.local/share/rerbr")));
        let set = [("HOME", "/h"), ("RERBR_HOME", "/r")];
        assert_eq!(folder(env(&set), "macos"), Some(PathBuf::from("/r")));
        let empty = [("HOME", "/h"), ("RERBR_HOME", "")];
        assert_eq!(folder(env(&empty), "linux"), Some(PathBuf::from("/h/.local/share/rerbr")));
        assert_eq!(folder(env(&[]), "linux"), None);
        let windows = [("HOME", "/h"), ("APPDATA", r"C:\Users\p\AppData\Roaming")];
        assert_eq!(folder(env(&windows), "windows"), Some(Path::new(r"C:\Users\p\AppData\Roaming").join("ReRBR")));
        let set = [("APPDATA", r"C:\a"), ("RERBR_HOME", r"D:\r")];
        assert_eq!(folder(env(&set), "windows"), Some(PathBuf::from(r"D:\r")));
        assert_eq!(folder(env(&home), "windows"), None);
    }
}
