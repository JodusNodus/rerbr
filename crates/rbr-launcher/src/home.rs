//! Where the player's files go: the settings, saves, records, ghosts, setups and added cars. The platform's user data
//! folder (macOS `~/Library/Application Support/ReRBR`, Windows `%APPDATA%\ReRBR`, Linux `$XDG_DATA_HOME/rerbr`,
//! default `~/.local/share/rerbr`), or `$RERBR_HOME`. Not next to the launcher: inside a downloaded app bundle that is read
//! only. The launcher passes its files' paths to the viewer and `rbr-standalone`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The folder of the viewer's performance logs (`viewer --perf-log`), in the player's folder.
pub const LOGS: &str = "logs";

/// The player's folder: `$RERBR_HOME`, else the platform's user data folder, else (no `HOME`, or no `APPDATA` on
/// Windows) `fallback`.
pub fn dir(fallback: &Path) -> PathBuf {
    folder(|key| std::env::var_os(key), std::env::consts::OS).unwrap_or_else(|| fallback.to_path_buf())
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
