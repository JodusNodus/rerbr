//! Where the player's files go: the settings, saves, records, ghosts, setups and added cars. The platform's user data
//! folder (macOS `~/Library/Application Support/ReRBR`, Windows `%APPDATA%\ReRBR`, Linux `$XDG_DATA_HOME/rerbr`,
//! default `~/.local/share/rerbr`), or `$RERBR_HOME`. Not next to the launcher: inside a downloaded app bundle that is read
//! only. The launcher passes its files' paths to the viewer and `rbr-standalone`.

use std::path::{Path, PathBuf};

/// The folder of the viewer's performance logs (`viewer --perf-log`), in the player's folder.
pub const LOGS: &str = "logs";

/// The player's folder ([`rbr_btb::converted::player_folder`], shared with converters), else (no `HOME`, or no
/// `APPDATA` on Windows) `fallback`.
pub fn dir(fallback: &Path) -> PathBuf {
    rbr_btb::converted::player_folder().unwrap_or_else(|| fallback.to_path_buf())
}
