//! The NGP DLL choice: follow the installed plugin until the player chooses an override.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rbr_formats::{cars, data::Data, ngp};

/// What the launcher says of an `NGP.dll` it does not read, of `FileVersion` `version`, short for the status line
/// (`pages::NGP_WHY` says why on the Settings page).
fn unsupported(version: Option<&str>) -> String {
    match version {
        Some(v) if v.starts_with("7.5.779") => "This NGP.dll is RallySimFans' 7.5.779; ReRBR needs NGP 7.4.778.".into(),
        Some(v) if v.starts_with("7.4.778") => {
            "This NGP.dll is a changed NGP 7.4.778; ReRBR needs the original.".into()
        }
        Some(v) => format!("This NGP.dll is NGP {v}; ReRBR needs NGP 7.4.778."),
        None => "This file is not NGP 7.4.778's NGP.dll.".into(),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct File {
    path: PathBuf,
    stamp: Option<(u64, Option<SystemTime>)>,
}

#[derive(Default)]
pub struct Choice {
    /// Empty follows the installed plugin. Only an explicit choice is saved.
    pub path: String,
    checked: RefCell<Option<(File, Option<String>)>>,
}

impl Choice {
    pub fn new(path: String) -> Self {
        Self { path, ..Self::default() }
    }

    pub fn automatic(data: &Data, root: &str) -> Option<PathBuf> {
        cars::resolve(Path::new(root.trim()), "Plugins/NGP.dll").or_else(|| data.resolve("Plugins/NGP.dll"))
    }

    pub fn resolved(&self, data: &Data, root: &str) -> Option<PathBuf> {
        match self.path.trim() {
            "" => Self::automatic(data, root),
            path => Some(PathBuf::from(path)),
        }
    }

    /// Hash only when the selected file or its metadata changes, rather than on every UI frame.
    pub fn problem(&self, data: &Data, root: &str) -> Option<String> {
        let Some(path) = self.resolved(data, root) else {
            return Some("No NGP.dll found; ReRBR needs NGP 7.4.778.".into());
        };
        let stamp = std::fs::metadata(&path).ok().map(|m| (m.len(), m.modified().ok()));
        let file = File { path, stamp };
        let mut checked = self.checked.borrow_mut();
        if let Some((_, problem)) = checked.as_ref().filter(|(old, _)| *old == file) {
            return problem.clone();
        }
        let problem = match std::fs::read(&file.path) {
            Ok(bytes) => {
                let found = ngp::compat::identify(&bytes);
                found.supported.is_none().then(|| unsupported(found.file_version.as_deref()))
            }
            Err(e) => Some(format!("{}: {e}", file.path.display())),
        };
        *checked = Some((file, problem.clone()));
        problem
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, data: &Data, root: &str, enabled: bool) {
        let automatic = Self::automatic(data, root);
        ui.add_enabled_ui(enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label("NGP.dll");
                let hint = automatic.as_ref().map_or("Choose your NGP.dll".into(), |p| p.display().to_string());
                ui.add(egui::TextEdit::singleline(&mut self.path).hint_text(hint).desired_width(420.0));
                if ui.button("Browse…").clicked() {
                    if let Some(path) = crate::job::pick_file("Choose NGP 7.4.778's NGP.dll") {
                        self.path = path;
                    }
                }
                if !self.path.is_empty() && automatic.is_some() && ui.button("Use installed DLL").clicked() {
                    self.path.clear();
                }
            });
        });
        if let Some(problem) = self.problem(data, root) {
            ui.colored_label(crate::pages::WARN, problem);
        } else if self.path.trim().is_empty() {
            ui.label("Linked to your installed NGP.dll. The game folder is read only.");
        } else {
            ui.label("Using your selected NGP 7.4.778 DLL.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let root = crate::test_dir(&format!("dll-{name}"));
        std::fs::create_dir_all(root.join("Plugins")).unwrap();
        root
    }

    #[test]
    fn follows_the_game_folder_and_keeps_an_explicit_choice() {
        let game = root("game");
        let rsf = root("rsf");
        std::fs::write(game.join("Plugins/NGP.dll"), b"game").unwrap();
        std::fs::write(rsf.join("Plugins/NGP.dll"), b"rsf").unwrap();
        let data = Data::new(&game, None);
        let mut choice = Choice::default();
        assert_eq!(choice.resolved(&data, rsf.to_str().unwrap()), Some(rsf.join("Plugins/NGP.dll")));
        choice.path = "my-plugin.dll".into();
        assert_eq!(choice.resolved(&data, ""), Some(PathBuf::from("my-plugin.dll")));
        choice.path.clear();
        assert_eq!(choice.resolved(&data, ""), Some(game.join("Plugins/NGP.dll")));
        std::fs::remove_dir_all(game).unwrap();
        std::fs::remove_dir_all(rsf).unwrap();
    }

    #[test]
    fn a_plugin_is_required_and_an_unsupported_one_blocks_start() {
        let game = root("missing");
        let data = Data::new(&game, None);
        let choice = Choice::default();
        assert_eq!(choice.problem(&data, "").unwrap(), "No NGP.dll found; ReRBR needs NGP 7.4.778.");
        std::fs::write(game.join("Plugins/NGP.dll"), b"another version").unwrap();
        assert_eq!(choice.problem(&data, "").unwrap(), "This file is not NGP 7.4.778's NGP.dll.");
        std::fs::remove_dir_all(game).unwrap();
    }

    #[test]
    fn names_the_version_found() {
        assert_eq!(unsupported(Some("7.5.779.508")), "This NGP.dll is RallySimFans' 7.5.779; ReRBR needs NGP 7.4.778.");
        assert!(unsupported(Some("7.4.778.0")).contains("a changed NGP 7.4.778"));
        assert_eq!(unsupported(Some("7.6.800.0")), "This NGP.dll is NGP 7.6.800.0; ReRBR needs NGP 7.4.778.");
    }

    #[test]
    fn notices_a_selected_file_being_replaced() {
        let data = Data::from_env().unwrap_or_else(|| Data::new("", None));
        let Some(source) = ngp::path(&data) else {
            return;
        };
        let game = root("replaced");
        let target = game.join("Plugins/NGP.dll");
        std::fs::copy(source, &target).unwrap();
        let choice = Choice::new(target.display().to_string());
        assert_eq!(choice.problem(&data, ""), None);
        std::fs::write(&target, b"unsupported").unwrap();
        assert_eq!(choice.problem(&data, "").unwrap(), "This file is not NGP 7.4.778's NGP.dll.");
        std::fs::remove_dir_all(game).unwrap();
    }
}
