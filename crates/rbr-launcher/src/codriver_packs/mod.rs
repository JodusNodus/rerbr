//! Locally owned co-driver voice packs and stage-note packs, so a complete setup works without RallySimFans:
//! "Import co-driver…" (the Graphics & sound page) and `launcher --import-codriver SRC` copy downloads into ReRBR's
//! own user content directories, where the launcher reads them in place of the RSF install's.
//!
//! The general launcher policy (user decision): ReRBR supports both (1) easy reuse of an existing RBR/RSF
//! installation read-only and (2) a complete setup without RSF, using retail+patch+NGP plus locally installed
//! content. Downloaded packs are COPIED into ReRBR-owned user content directories (`cars/` for car packs, here
//! `codrivers/` and `notes/`). Existing RSF content may be read in place. Player settings, saves and personal edits
//! always belong to ReRBR. This module applies that policy to co-driver voice/style/range and stage-note packs.
//!
//! Layout, each pack isolated so different `Rbr.ini` definitions never collide:
//! - `codrivers/<pack>/`: a Pacenote plugin's folder as `rbr_formats::pacenote::codriver` reads it (`PaceNote.ini`,
//!   `sounds/...`, `config/pacenotes/*.ini`, `config/ranges/*.ini` with their package/category trees, relative paths
//!   preserved);
//! - `notes/<pack>/`: stage-note INI files (the plugin's BTB/MyPacenotes format, `rbr_formats::pacenote::notes`),
//!   relative paths preserved; looked up recursively, so stage folders may nest.
//!
//! Sources: an extracted folder (a Pacenote plugin folder, a MyPacenotes folder, a folder with both, or a whole
//! existing install to COPY from — never modified), or a downloaded ZIP/7z archive of the same. The plugin is
//! found at the top, in a direct `Pacenote` (or `Plugins/Pacenote`) folder beside the notes, or under a single
//! wrapper directory (including `wrapper/Pacenote` and `wrapper/Plugins/Pacenote`); note paths are always kept
//! whole, so stage folders keep their names. Only relevant files are copied: `PaceNote.ini`,
//! `config/**/*.ini`, voice audio (`sounds/`, `.ogg`/`.wav`/`.mp3`/`.flac`/`.oga`) and parseable note INIs. No
//! DLLs, executables or anything else. A failed import leaves no active partial pack: everything stages in a temp
//! folder beside the destination and is renamed only when validated. Existing packs are never overwritten: a taken
//! name gets a `-2`, `-3` suffix. An audio-only download without the style/range definitions it needs fails with an
//! actionable error (or a pack listed with its missing dependency), and never appears ready.

#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(test)]
use rbr_formats::pacenote::codriver;
use rbr_formats::pacenote::notes;

use crate::job::{Job, Progress};
use crate::pages::{GOOD, WARN};
use crate::Form;

/// The folders in the player's folder with the owned packs.
pub const VOICES_DIR: &str = "codrivers";
pub const NOTES_DIR: &str = "notes";
/// The archive suffixes taken as downloads.
const ZIP: &str = ".zip";
const SEVEN_Z: &str = ".7z";

type Fail = String;

mod import;
mod scan;

pub use import::{cli, import, remove_notes, remove_voice};
#[cfg(test)]
use import::{import_both, import_voice_from, publish, publish_pair, stage};
pub use scan::{
    call_sets_differ, find_in_set, note_packs, note_sets, set_label, voice_packs, NotePack, NoteSet, NotesInputs,
    NotesSummary, VoicePack,
};
use scan::{check_voice, is_plugin_dir};
#[cfg(test)]
use scan::{copy_voice, pack_name, walk};
impl Form {
    /// Re-reads the owned packs and points the co-driver at the chosen ones: the local voice pack
    /// (`codriver_pack`), else the RSF install's plugin; a missing local pack keeps the last lists and says so.
    /// Caches the RSF install's voices and note sets. Marks the notes summary for refresh (it refreshes on draw).
    pub fn rescan_codrivers(&mut self) {
        let home = self.here();
        self.owned_voices = voice_packs(&home);
        self.owned_notes = note_packs(&home);
        self.codriver_mods = self.active_mods();
        let root = self.game_root();
        let mods = crate::command::CodriverMods::scan(&root);
        self.rsf_plugin = mods.is_some();
        self.rsf_voices = mods.map(|mods| mods.voices).unwrap_or_default();
        self.rsf_note_sets = notes::my_pacenotes_dir(Path::new(root.trim())).map(|d| note_sets(&d)).unwrap_or_default();
        (self.codriver_styles_differ, self.codriver_ranges_differ) = match &self.codriver_mods {
            Some(mods) => call_sets_differ(&mods.dir, &mods.styles, &mods.ranges),
            None => (false, false),
        };
        self.cached_notes_dirty = true;
    }

    /// Refreshes the cached summary of what the co-driver says on the selected stage when the map, the notes
    /// sources, the co-driver switch or voice, or an import changed it (never scanned per draw). It says what the
    /// launch passes: custom notes only play with a custom voice (the viewer only gets `--pacenotes` with one).
    pub fn refresh_notes_cache(&mut self) {
        let map = self.catalog.maps.get(self.map).map(|m| m.number).unwrap_or(0);
        if !self.cached_notes_dirty && self.cached_notes_map == map {
            return;
        }
        self.cached_notes_map = map;
        self.cached_notes_dirty = false;
        let found = self.catalog.map(map);
        let names = found.and_then(|m| m.stages.first()).map(|stage| self.stage_note_names(stage));
        let stage = found.map(|m| m.name()).unwrap_or_default();
        let voice = self.codriver_mods.as_ref().and_then(|m| m.voice(self.codriver_voice.as_deref()));
        let mut summary = NotesSummary { stage, ..NotesSummary::default() };
        summary.text = match (self.codriver, voice, self.active_set(), names) {
            (false, ..) => "no co-driver, no notes are called".into(),
            (true, None, ..) => "the 1.02 co-driver calls the stage's own notes".into(),
            (true, Some(voice), None, _) | (true, Some(voice), _, None) => {
                format!("{voice} calls the stage's own notes")
            }
            (true, Some(voice), Some(set), Some(names)) => {
                let label = set_label(self.notes_pack.as_deref(), &set.name);
                let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                summary.file = self.resolve_notes(&refs);
                match summary.file.as_deref().map(notes::read) {
                    Some(Ok(notes)) if notes.len() == 1 => format!("{voice} calls {label} (1 note)"),
                    Some(Ok(notes)) => format!("{voice} calls {label} ({} notes)", notes.len()),
                    _ => {
                        summary.fallback = true;
                        format!("{label} has no notes for this stage: {voice} calls the stage's own notes")
                    }
                }
            }
        };
        self.notes_summary = summary;
    }

    /// Marks the summary dirty when its inputs changed since `before` (note pack, note set, notes on, the
    /// co-driver switch or the voice): it refreshes on the next draw, so repaint for it.
    pub fn invalidate_notes_cache(&mut self, ctx: &egui::Context, before: NotesInputs) {
        if before != self.notes_inputs() {
            self.cached_notes_dirty = true;
            ctx.request_repaint();
        }
    }

    /// What the notes summary depends on besides the map and the scanned packs.
    pub fn notes_inputs(&self) -> NotesInputs {
        (self.notes_pack.clone(), self.notes_set.clone(), self.my_pacenotes, self.codriver, self.codriver_voice.clone())
    }

    /// The active co-driver plugin: the chosen local pack's folder, else the RSF install's. None uses the
    /// original 1.02 voice. A chosen pack listed as not ready, or failing validation, is refused: the
    /// viewer never gets a pack that cannot speak.
    pub fn active_mods(&self) -> Option<crate::command::CodriverMods> {
        if let Some(pack) = self.codriver_pack.as_deref() {
            let listed = self.owned_voices.iter().find(|p| p.name == pack).is_some_and(|p| p.problem.is_none());
            let dir = self.here().join(VOICES_DIR).join(pack);
            if listed && is_plugin_dir(&dir) && check_voice(&dir).is_ok() {
                return crate::command::CodriverMods::from_dir(dir);
            }
            return None;
        }
        crate::command::CodriverMods::scan(&self.game_root())
    }

    /// The note sets of the chosen source: the local pack's, else the RSF install's MyPacenotes.
    pub fn source_sets(&self) -> &[NoteSet] {
        match self.notes_pack.as_deref() {
            Some(pack) => self.owned_notes.iter().find(|p| p.name == pack).map_or(&[], |p| p.sets.as_slice()),
            None => &self.rsf_note_sets,
        }
    }

    /// The note set that plays: the chosen one, else (unset, or gone from the source) the source's largest. None
    /// for the stages' own notes.
    pub fn active_set(&self) -> Option<&NoteSet> {
        if !self.my_pacenotes {
            return None;
        }
        let sets = self.source_sets();
        let chosen = self.notes_set.as_deref();
        sets.iter().find(|s| chosen.is_some_and(|c| c.eq_ignore_ascii_case(&s.name))).or(sets.first())
    }

    /// The note file the active set plays on the stage with catalogue `names`, or `None` for the stage's own notes.
    pub fn resolve_notes(&self, names: &[&str]) -> Option<PathBuf> {
        let set = self.active_set()?;
        let dir = match self.notes_pack.as_deref() {
            Some(pack) => self.here().join(NOTES_DIR).join(pack),
            None => notes::my_pacenotes_dir(Path::new(self.game_root().trim()))?,
        };
        find_in_set(&dir, names, &set.name)
    }

    /// Aligns the voice, style and range choices with the newly active plugin after the pack changed: a kept
    /// choice the new plugin does not offer becomes the plugin's default (an unset voice plays the pack's). The
    /// explicit 1.02 choice stays.
    pub fn normalize_codriver(&mut self) {
        let Some(mods) = self.codriver_mods.clone() else { return };
        if self
            .codriver_voice
            .as_deref()
            .is_some_and(|v| v != crate::command::ORIGINAL_VOICE && !mods.voices.iter().any(|o| o == v))
        {
            self.codriver_voice = None;
        }
        if !mods.styles.contains(&self.codriver_style) {
            self.codriver_style =
                mods.styles.first().cloned().unwrap_or_else(|| rbr_sound::custom_codriver::DEFAULT_STYLE.into());
        }
        if !mods.ranges.contains(&self.codriver_ranges) {
            self.codriver_ranges =
                mods.ranges.first().cloned().unwrap_or_else(|| rbr_sound::custom_codriver::DEFAULT_RANGES.into());
        }
    }

    /// Starts importing the co-driver download `src` (a folder or archive) on a thread; real progress with a
    /// working Cancel, and nothing published when cancelled.
    pub fn import_codriver_from(&mut self, src: String) {
        if self.codriver_job.is_some() || self.child.is_some() {
            return;
        }
        let home = self.here();
        self.codriver_status = None;
        self.codriver_job = Some(Job::start(move |report, cancel| {
            report(&Progress { step: "Importing co-driver", done: 0, total: 0 });
            import(&home, Path::new(&src), cancel)
        }));
    }

    /// Separate folder and archive imports: cancelling one dialog never opens the other.
    pub fn import_codriver_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let idle = self.codriver_job.is_none() && self.child.is_none();
            if ui
                .add_enabled(idle, egui::Button::new("Import folder…"))
                .on_hover_text(
                    "Choose an extracted co-driver download, a MyPacenotes folder, or a whole RBR/RSF install to \
                     copy from (only read). ReRBR copies its voices, call styles, distance calls and stage notes \
                     into your files' folder; the source is not changed.",
                )
                .clicked()
            {
                if let Some(src) = crate::job::pick_folder("The co-driver download's folder") {
                    self.import_codriver_from(src);
                }
            }
            if ui
                .add_enabled(idle, egui::Button::new("Import archive…"))
                .on_hover_text(
                    "Choose a downloaded .zip/.7z co-driver or note pack. ReRBR copies its voices, call styles, \
                     distance calls and stage notes into your files' folder; the download is not changed.",
                )
                .clicked()
            {
                if let Some(src) = crate::job::pick_file("The co-driver download's .zip/.7z archive") {
                    self.import_codriver_from(src);
                }
            }
            let Some(job) = &mut self.codriver_job else { return };
            if let Some(result) = job.ui(ui) {
                self.codriver_job = None;
                // What a successful import added drives: its voice pack (its default voice) and its notes. No
                // removals run while importing, so the lists before the rescan are the ones it started with.
                let before = result.is_ok().then(|| {
                    let voices: Vec<String> = self.owned_voices.iter().map(|p| p.name.clone()).collect();
                    let notes: Vec<String> = self.owned_notes.iter().map(|p| p.name.clone()).collect();
                    (voices, notes)
                });
                self.codriver_status = Some(match result {
                    Err(e) if e.ends_with("cancelled") => Err("Cancelled; nothing was written.".into()),
                    Ok(lines) => Ok(lines.join(" ")),
                    Err(e) => Err(e),
                });
                self.rescan_codrivers();
                let Some((voices, notes)) = before else { return };
                let voice = self.owned_voices.iter().find(|p| p.problem.is_none() && !voices.contains(&p.name));
                if let Some(name) = voice.map(|p| p.name.clone()) {
                    (self.codriver, self.codriver_pack, self.codriver_voice) = (true, Some(name), None);
                }
                if let Some(name) = self.owned_notes.iter().find(|p| !notes.contains(&p.name)).map(|p| p.name.clone()) {
                    (self.my_pacenotes, self.notes_pack, self.notes_set) = (true, Some(name), None);
                }
            }
        });
        match &self.codriver_status {
            Some(Ok(text)) => _ = ui.colored_label(GOOD, format!("✔ {text}")),
            Some(Err(text)) => _ = ui.colored_label(WARN, format!("⚠ {text}")),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn imported(home: &Path, src: &Path) -> Result<Vec<String>, String> {
        import(home, src, &AtomicBool::new(false))
    }

    /// A minimal plugin source: one style, one range set, one voice take.
    fn plugin_source(dir: &Path) {
        write(&dir.join("PaceNote.ini"), "[SETTINGS]\nsounds=english/steve\n");
        write(&dir.join("config/pacenotes/Rbr.ini"), "[PACKAGE::RBR]\nfile=calls.ini\n");
        write(&dir.join("config/pacenotes/calls.ini"), "[PACENOTE::ONE_LEFT]\nid=0\nSounds=1\nSnd0=one_left.ogg\n");
        write(&dir.join("config/ranges/Rbr.ini"), "[RANGE::50]\nSounds=1\nSnd0=50_1.ogg\n");
        write(&dir.join("sounds/english/steve/one_left.ogg"), "OggS fake");
        write(&dir.join("sounds/english/steve/50_1.ogg"), "OggS fake");
        // Leftovers that must not ride along.
        write(&dir.join("PaceNote.dll"), "MZ fake");
        write(&dir.join("editor.exe"), "MZ fake");
    }

    fn notes_source(dir: &Path, stage: &str) {
        write(
            &dir.join(format!("{stage}/tanner.ini")),
            "[PACENOTES]\ncount = 1\n\n[P0]\ntype = 16\ndistance = 100.0\nflag = 0\n",
        );
    }

    #[test]
    fn an_extracted_plugin_folder_imports_and_loads() {
        let dir = crate::test_dir("codriver-folder");
        let src = dir.join("download");
        plugin_source(&src);
        let home = dir.join("home");
        let lines = imported(&home, &src).unwrap();
        assert_eq!(lines.len(), 1, "{lines:?}");
        let pack = home.join(VOICES_DIR).join(pack_name(&src));
        assert!(pack.join("PaceNote.ini").is_file());
        assert!(pack.join("config/pacenotes/calls.ini").is_file());
        assert!(pack.join("sounds/english/steve/one_left.ogg").is_file());
        assert!(!pack.join("PaceNote.dll").exists());
        assert!(!pack.join("editor.exe").exists());
        // The copied files still load after the source is gone.
        fs::remove_dir_all(&src).unwrap();
        let calls = codriver::load(&pack, "Rbr.ini", "Rbr.ini").unwrap();
        assert!(calls.notes.contains_key(&0));
        assert!(calls.ranges.contains_key(&50));
        assert_eq!(codriver::voices(&pack), ["english/steve"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_wrapper_directory_and_a_direct_pacenote_folder_import() {
        let dir = crate::test_dir("codriver-wrapper");
        let inner = dir.join("download").join("wrapper");
        plugin_source(&inner.join("Pacenote"));
        let home = dir.join("home");
        // A wrapper directory around a direct Pacenote folder.
        let lines = imported(&home, &dir.join("download")).unwrap();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(home.join(VOICES_DIR).join("download").join("PaceNote.ini").is_file());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_whole_install_is_a_copy_source_only() {
        let dir = crate::test_dir("codriver-install");
        let install = dir.join("rsf");
        plugin_source(&install.join("Plugins/Pacenote"));
        notes_source(&install.join("Plugins/NGPCarMenu/MyPacenotes"), "Tanner");
        let home = dir.join("home");
        let lines = imported(&home, &install).unwrap();
        assert_eq!(lines.len(), 2, "{lines:?}");
        // The source is only read.
        assert!(install.join("Plugins/Pacenote/PaceNote.ini").is_file());
        let voice = home.join(VOICES_DIR).join("rsf");
        let notes = home.join(NOTES_DIR).join("rsf");
        assert!(voice.join("PaceNote.ini").is_file());
        assert!(tanner(&notes).is_some());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn audio_without_definitions_fails_actionably_and_leaves_nothing() {
        let dir = crate::test_dir("codriver-audio-only");
        let src = dir.join("voice");
        write(&src.join("sounds/english/steve/one_left.ogg"), "OggS fake");
        let home = dir.join("home");
        let err = imported(&home, &src).unwrap_err();
        assert!(err.contains("no call styles"), "{err}");
        assert!(!home.join(VOICES_DIR).exists() || fs::read_dir(home.join(VOICES_DIR)).unwrap().count() == 0);
        // A notes-only pack imports through the same entry point.
        let notes = dir.join("mynotes");
        notes_source(&notes, "Tanner");
        let lines = imported(&home, &notes).unwrap();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(tanner(&home.join(NOTES_DIR).join("mynotes")).is_some());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn packs_with_the_same_rbr_ini_stay_isolated() {
        let dir = crate::test_dir("codriver-isolated");
        for (pack, id) in [("a", 0), ("b", 7)] {
            let src = dir.join(pack);
            write(&src.join("config/pacenotes/Rbr.ini"), "[PACKAGE::RBR]\nfile=calls.ini\n");
            write(
                &src.join("config/pacenotes/calls.ini"),
                &format!("[PACENOTE::CALL_{pack}]\nid={id}\nSounds=1\nSnd0={pack}.ogg\n"),
            );
            write(&src.join("config/ranges/Rbr.ini"), "[RANGE::50]\nSounds=1\nSnd0=50_1.ogg\n");
            write(&src.join(format!("sounds/v/{pack}.ogg")), "OggS fake");
        }
        let home = dir.join("home");
        imported(&home, &dir.join("a")).unwrap();
        imported(&home, &dir.join("b")).unwrap();
        let (a, b) = (home.join(VOICES_DIR).join("a"), home.join(VOICES_DIR).join("b"));
        assert!(codriver::load(&a, "Rbr.ini", "Rbr.ini").unwrap().notes.contains_key(&0));
        // Without a PaceNote.ini the packs still default to their discovered voice.
        let voice = |dir: &Path| crate::command::CodriverMods::from_dir(dir.to_owned()).unwrap().voice(None);
        assert_eq!(voice(&a).as_deref(), Some("v"));
        assert_eq!(voice(&b).as_deref(), Some("v"));
        assert!(!codriver::load(&a, "Rbr.ini", "Rbr.ini").unwrap().notes.contains_key(&7));
        assert!(codriver::load(&b, "Rbr.ini", "Rbr.ini").unwrap().notes.contains_key(&7));
        assert!(!codriver::load(&b, "Rbr.ini", "Rbr.ini").unwrap().notes.contains_key(&0));
        // An existing pack is never overwritten: the same source again gets a new name.
        imported(&home, &dir.join("a")).unwrap();
        assert!(!home.join(VOICES_DIR).join("a-2").is_file());
        assert!(home.join(VOICES_DIR).join("a-2").is_dir());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn zip_wrapper_and_unsafe_paths() {
        let dir = crate::test_dir("codriver-zip");
        let src = dir.join("download");
        plugin_source(&src);
        // A ZIP with a wrapper directory.
        let archive = dir.join("Voice Pack.zip");
        {
            let file = File::create(&archive).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            // Info-ZIP's explicit directory entries, as real downloads have them.
            zip.add_directory("wrapper/", options).unwrap();
            for (rel, path) in walk(&src) {
                if path.is_dir() {
                    continue;
                }
                zip.start_file(format!("wrapper/{rel}"), options).unwrap();
                let bytes = fs::read(&path).unwrap();
                std::io::Write::write_all(&mut zip, &bytes).unwrap();
            }
            zip.finish().unwrap();
        }
        let home = dir.join("home");
        let lines = imported(&home, &archive).unwrap();
        assert_eq!(lines.len(), 1, "{lines:?}");
        let pack = home.join(VOICES_DIR).join("Voice Pack");
        assert!(pack.join("PaceNote.ini").is_file());
        // An unsafe archive leaves no pack behind.
        let evil = dir.join("evil.zip");
        {
            let file = File::create(&evil).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("wrapper/../../evil.ini", options).unwrap();
            std::io::Write::write_all(&mut zip, b"[PACENOTES]\ncount = 0\n").unwrap();
            zip.start_file("wrapper/config/pacenotes/Rbr.ini", options).unwrap();
            std::io::Write::write_all(&mut zip, b"[PACKAGE::RBR]\nfile=calls.ini\n").unwrap();
            zip.finish().unwrap();
        }
        let before: Vec<String> = fs::read_dir(home.join(VOICES_DIR))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        let _ = imported(&home, &evil);
        let after: Vec<String> = fs::read_dir(home.join(VOICES_DIR))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!after.iter().any(|n| n.starts_with("evil")), "{after:?}");
        assert_eq!(before.len(), after.len(), "{before:?} vs {after:?}");
        assert!(!dir.join("evil.ini").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn seven_z_and_notes_only_archives() {
        let dir = crate::test_dir("codriver-seven");
        let src = dir.join("notes-src");
        notes_source(&src, "Tanner");
        let archive = dir.join("tanner-notes.7z");
        {
            let mut out = sevenz_rust2::ArchiveWriter::create(&archive).unwrap();
            for (rel, path) in walk(&src) {
                if path.is_dir() {
                    continue;
                }
                let name = format!("top/{rel}");
                let bytes = fs::read(&path).unwrap();
                out.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(&name), Some(bytes.as_slice())).unwrap();
            }
            out.finish().unwrap();
        }
        let home = dir.join("home");
        let lines = imported(&home, &archive).unwrap();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(tanner(&home.join(NOTES_DIR).join("tanner-notes")).is_some());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_parts_are_named_not_panics() {
        let dir = crate::test_dir("codriver-parts");
        let home = dir.join("home");
        // Ranges without styles (or voices) fail with the missing part, instead of indexing an empty list.
        let ranges_only = dir.join("ranges-only");
        write(&ranges_only.join("config/ranges/Rbr.ini"), "[RANGE::50]\nSounds=1\nSnd0=50_1.ogg\n");
        write(&ranges_only.join("sounds/english/steve/50_1.ogg"), "OggS fake");
        let err = imported(&home, &ranges_only).unwrap_err();
        assert!(err.contains("no call styles"), "{err}");
        // Styles without voices name the voices.
        let styles_only = dir.join("styles-only");
        write(&styles_only.join("config/pacenotes/Rbr.ini"), "[PACKAGE::RBR]\nfile=calls.ini\n");
        write(
            &styles_only.join("config/pacenotes/calls.ini"),
            "[PACENOTE::ONE_LEFT]\nid=0\nSounds=1\nSnd0=one_left.ogg\n",
        );
        let err = imported(&home, &styles_only).unwrap_err();
        assert!(err.contains("no voices"), "{err}");
        assert!(!home.join(VOICES_DIR).join("ranges-only").exists());
        assert!(!home.join(VOICES_DIR).join("styles-only").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn symlinks_are_neither_followed_nor_copied() {
        use std::os::unix::fs::symlink;
        let dir = crate::test_dir("codriver-symlinks");
        let src = dir.join("download");
        plugin_source(&src);
        // A loop back to the source itself, and a file link to another pack's file: neither is followed.
        symlink(&src, src.join("loop")).unwrap();
        symlink(src.join("PaceNote.ini"), src.join("sounds/english/steve/linked.ogg")).unwrap();
        let home = dir.join("home");
        imported(&home, &src).unwrap();
        let pack = home.join(VOICES_DIR).join("download");
        assert!(pack.join("PaceNote.ini").is_file());
        assert!(!pack.join("loop").exists());
        assert!(!pack.join("sounds/english/steve/linked.ogg").exists());
        // A symlinked pack folder is not listed as a pack either.
        symlink(home.join(VOICES_DIR).join("download"), home.join(VOICES_DIR).join("ghost")).unwrap();
        assert_eq!(voice_packs(&home).iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["download"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn single_stage_sources_keep_their_stage() {
        let dir = crate::test_dir("codriver-single-stage");
        let home = dir.join("home");
        let note = "[PACENOTES]\ncount = 1\n\n[P0]\ntype = 16\ndistance = 100.0\nflag = 0\n";
        // A bare stage folder: its own name stays over its notes.
        let folder = dir.join("Tanner");
        write(&folder.join("tanner.ini"), note);
        imported(&home, &folder).unwrap();
        assert!(tanner(&home.join(NOTES_DIR).join("Tanner")).is_some());
        // A single-stage archive: the stage folder is kept, not stripped as a wrapper.
        for (archive, make) in [("single.zip", true), ("single.7z", false)] {
            let path = dir.join(archive);
            if make {
                let file = File::create(&path).unwrap();
                let mut zip = zip::ZipWriter::new(file);
                let options = zip::write::SimpleFileOptions::default();
                zip.start_file("Tanner/tanner.ini", options).unwrap();
                std::io::Write::write_all(&mut zip, note.as_bytes()).unwrap();
                zip.finish().unwrap();
            } else {
                let mut out = sevenz_rust2::ArchiveWriter::create(&path).unwrap();
                let name = "Tanner/tanner.ini".to_owned();
                out.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(&name), Some(note.as_bytes())).unwrap();
                out.finish().unwrap();
            }
            imported(&home, &path).unwrap();
            let pack = home.join(NOTES_DIR).join("single");
            assert!(tanner(&pack).is_some(), "{archive}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The Tanner notes of any set in `dir`.
    fn tanner(dir: &Path) -> Option<PathBuf> {
        note_sets(dir).iter().find_map(|set| find_in_set(dir, &["Tanner"], &set.name))
    }

    /// A MyPacenotes-like folder (names as in an RSF install): each stage folder's files make the sets named by what
    /// they add to the stage's name, counted by stage; `_default` exports make none; the highest edited copy plays.
    #[test]
    fn note_sets_are_named_after_the_stage() {
        let dir = crate::test_dir("codriver-sets");
        let note = "[PACENOTES]\ncount = 1\n\n[P0]\ntype = 16\ndistance = 1.0\nflag = 0\n";
        for stage in ["Tanner", "Tanner II", "Mlynky"] {
            write(&dir.join(format!("{stage}/{stage} LuppisV3.ini")), note);
            write(&dir.join(format!("{stage}/{stage}_default.ini")), note);
        }
        write(&dir.join("Tanner/tanner.ini"), note);
        write(&dir.join("Mlynky/Mlynky - Tanner.ini"), note);
        write(&dir.join("Mlynky.ini"), note);
        // An edited copy is a later version of its set; a stage spelt otherwise in the file name still matches.
        write(&dir.join("Kormoran I/Kormoran I LuppisV3_1.ini"), note);
        write(&dir.join("Ul. Rajdowa/Ul_ Rajdowa LuppisV3.ini"), note);
        write(&dir.join("Tanner/Tanner LuppisV3_2.ini"), note);
        write(&dir.join("Tanner II/Tanner II LuppisV3_1.1.ini"), note);
        // A stage folder named otherwise than its files: the set the other stages' files share.
        write(&dir.join("Bareyo BTB/Bareyo LuppisV3.ini"), note);
        write(&dir.join("Haguenau 2012/Haugenau 2012 LuppisV3.ini"), note);
        write(&dir.join("Karowa 2009/Karowa 2009 Again LuppisV3_1.ini"), note);
        write(&dir.join("Readme.txt"), "not notes");
        let sets: Vec<(String, usize)> = note_sets(&dir).into_iter().map(|s| (s.name, s.stages)).collect();
        assert_eq!(sets, [("LuppisV3".to_owned(), 8), (String::new(), 2), ("Tanner".to_owned(), 1)]);
        let file =
            |names: &[&str], set| find_in_set(&dir, names, set).map(|p| p.strip_prefix(&dir).unwrap().to_owned());
        assert_eq!(file(&["Tanner II"], "luppisv3"), Some(PathBuf::from("Tanner II/Tanner II LuppisV3_1.1.ini")));
        assert_eq!(file(&["Tanner"], ""), Some(PathBuf::from("Tanner/tanner.ini")));
        assert_eq!(file(&["Tanner"], "LuppisV3"), Some(PathBuf::from("Tanner/Tanner LuppisV3_2.ini")));
        assert_eq!(file(&["Kormoran I"], "LuppisV3"), Some(PathBuf::from("Kormoran I/Kormoran I LuppisV3_1.ini")));
        assert_eq!(file(&["Ul. Rajdowa"], "LuppisV3"), Some(PathBuf::from("Ul. Rajdowa/Ul_ Rajdowa LuppisV3.ini")));
        assert_eq!(file(&["mlynky"], ""), Some(PathBuf::from("Mlynky.ini")));
        assert_eq!(file(&["Tanner II"], ""), None);
        assert_eq!(file(&["Bareyo BTB"], "LuppisV3"), Some(PathBuf::from("Bareyo BTB/Bareyo LuppisV3.ini")));
        assert_eq!(set_label(None, ""), "MyPacenotes");
        assert_eq!(set_label(Some("pack"), ""), "pack");
        assert_eq!(set_label(Some("pack"), "LuppisV3"), "LuppisV3");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_combined_import_publishes_nothing() {
        let dir = crate::test_dir("codriver-atomic");
        let home = dir.join("home");
        let voice = dir.join("voice");
        plugin_source(&voice);
        // Notes that read as a folder but validate as nothing: the voice must not publish alone.
        let notes = dir.join("notes");
        write(&notes.join("Tanner/broken.ini"), "[SETTINGS]\nfoo = bar\n");
        let err = import_both(&home, "combo", &AtomicBool::new(false), &voice, &notes).unwrap_err();
        assert!(err.contains("no stage notes"), "{err}");
        assert!(!home.join(VOICES_DIR).join("combo").exists());
        assert!(!home.join(NOTES_DIR).join("combo").exists());
        // A cancelled import publishes nothing either.
        let cancelled = AtomicBool::new(true);
        let err = import(&home, &voice, &cancelled).unwrap_err();
        assert!(err.ends_with("cancelled"), "{err}");
        assert!(!home.join(VOICES_DIR).join("voice").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A ZIP archive of the folder `base` (relative paths kept whole).
    fn make_zip(archive: &Path, base: &Path) {
        let file = File::create(archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (rel, path) in walk(base) {
            if path.is_dir() {
                continue;
            }
            zip.start_file(rel, options).unwrap();
            let bytes = fs::read(&path).unwrap();
            std::io::Write::write_all(&mut zip, &bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn wrapper_plugin_layouts_import() {
        let dir = crate::test_dir("codriver-wrapper-layouts");
        let home = dir.join("home");
        // Folder sources: the plugin under a wrapper, as `Pacenote` or `Plugins/Pacenote`.
        for (case, plugin) in [("folder-pacenote", "wrapper/Pacenote"), ("folder-install", "wrapper/Plugins/Pacenote")]
        {
            let src = dir.join(case);
            plugin_source(&src.join(plugin));
            let lines = imported(&home, &src).unwrap();
            assert_eq!(lines.len(), 1, "{case}: {lines:?}");
            assert!(home.join(VOICES_DIR).join(case).join("PaceNote.ini").is_file(), "{case}");
        }
        // Archive sources: the same layouts, kept whole (the stage folder is never a wrapper).
        let zipped = dir.join("zipped-pacenote");
        plugin_source(&zipped.join("wrapper/Pacenote"));
        let archive = dir.join("zipped-pacenote.zip");
        make_zip(&archive, &zipped);
        let lines = imported(&home, &archive).unwrap();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(home.join(VOICES_DIR).join("zipped-pacenote").join("PaceNote.ini").is_file());
        let install = dir.join("zipped-install");
        plugin_source(&install.join("wrapper/Plugins/Pacenote"));
        let archive = dir.join("zipped-install.7z");
        {
            let mut out = sevenz_rust2::ArchiveWriter::create(&archive).unwrap();
            for (rel, path) in walk(&install) {
                if path.is_dir() {
                    continue;
                }
                let bytes = fs::read(&path).unwrap();
                out.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(&rel), Some(bytes.as_slice())).unwrap();
            }
            out.finish().unwrap();
        }
        let lines = imported(&home, &archive).unwrap();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(home.join(VOICES_DIR).join("zipped-install").join("PaceNote.ini").is_file());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_voice_archive_with_an_unrelated_ini_keeps_the_voice() {
        let dir = crate::test_dir("codriver-unrelated-ini");
        let home = dir.join("home");
        // A valid voice with an unrelated INI next to it: the voice imports alone, nothing pruned loudly.
        let src = dir.join("download");
        plugin_source(&src);
        write(&src.join("Readme.ini"), "[ABOUT]\ntext = hello\n");
        let archive = dir.join("voice.zip");
        make_zip(&archive, &src);
        let lines = imported(&home, &archive).unwrap();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(home.join(VOICES_DIR).join("voice").join("PaceNote.ini").is_file());
        assert!(!home.join(NOTES_DIR).join("voice").exists());
        // A valid voice with a real note and an unrelated INI: both publish, the note kept.
        notes_source(&src, "Tanner");
        let archive = dir.join("both.zip");
        make_zip(&archive, &src);
        let lines = imported(&home, &archive).unwrap();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(tanner(&home.join(NOTES_DIR).join("both")).is_some());
        // A notes-only archive with no parseable notes still fails loudly and publishes nothing.
        let junk = dir.join("junk");
        write(&junk.join("Readme.ini"), "[ABOUT]\ntext = hello\n");
        let archive = dir.join("junk.zip");
        make_zip(&archive, &junk);
        let err = imported(&home, &archive).unwrap_err();
        assert!(err.contains("no stage notes"), "{err}");
        assert!(!home.join(NOTES_DIR).join("junk").exists());
        assert!(!home.join(VOICES_DIR).join("junk").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_cancel_after_staging_publishes_nothing() {
        let dir = crate::test_dir("codriver-late-cancel");
        let home = dir.join("home");
        let src = dir.join("voice");
        plugin_source(&src);
        let cancel = AtomicBool::new(false);
        // The copy flips the flag after staging, as a Cancel pressed during validation would: the commit,
        // checked immediately before renaming, refuses.
        let err = import_voice_from(&home, "late", &cancel, |d, c| {
            let n = copy_voice(&src, d, c)?;
            c.store(true, Ordering::Relaxed);
            Ok(n)
        })
        .unwrap_err();
        assert!(err.ends_with("cancelled"), "{err}");
        assert!(!home.join(VOICES_DIR).join("late").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_second_publish_failure_rolls_the_voice_back() {
        let dir = crate::test_dir("codriver-rollback");
        let home = dir.join("home");
        let no_cancel = AtomicBool::new(false);
        // A notes publish that cannot rename (its staging is gone): the just-published voice is removed again,
        // so a dual publish never leaves a lone voice behind.
        let voice = stage(&home, VOICES_DIR, "pair").unwrap();
        let notes = stage(&home, NOTES_DIR, "pair").unwrap();
        write(&voice.staged.join("PaceNote.ini"), "[SETTINGS]\nsounds=english/steve\n");
        fs::remove_dir_all(&notes.staged).unwrap();
        let err = publish_pair(&voice, &notes, &no_cancel).unwrap_err();
        assert!(!home.join(VOICES_DIR).join("pair").exists(), "{err}");
        assert!(!home.join(NOTES_DIR).join("pair").exists(), "{err}");
        // A cancel checked immediately before the commit publishes nothing either.
        let staged = stage(&home, VOICES_DIR, "late").unwrap();
        let cancel = AtomicBool::new(true);
        let err = publish(&staged, &cancel).unwrap_err();
        assert!(err.ends_with("cancelled"), "{err}");
        assert!(!home.join(VOICES_DIR).join("late").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn corrupt_packs_list_their_validation_problem() {
        let dir = crate::test_dir("codriver-corrupt-list");
        let home = dir.join("home");
        let broken = home.join(VOICES_DIR).join("broken");
        // A style naming a package file that is not there: the lists see no missing part, but loading dangles.
        write(&broken.join("config/pacenotes/Rbr.ini"), "[PACKAGE::RBR]\nfile=calls.ini\n");
        write(&broken.join("config/ranges/Rbr.ini"), "[RANGE::50]\nSounds=1\nSnd0=50_1.ogg\n");
        write(&broken.join("sounds/english/steve/50_1.ogg"), "OggS fake");
        let packs = voice_packs(&home);
        assert_eq!(packs.len(), 1);
        let problem = packs[0].problem.as_deref().expect("a dangling pack lists its problem");
        assert!(problem.contains("calls.ini"), "{problem}");
        // The same reason activation refuses with: the list and the activation agree.
        let refused = check_voice(&broken).unwrap_err();
        assert!(refused.ends_with(problem), "{refused}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn default_voice_follows_the_installed_spelling() {
        let dir = crate::test_dir("codriver-default-voice");
        let src = dir.join("pack");
        plugin_source(&src);
        let mods = crate::command::CodriverMods::from_dir(src.clone()).unwrap();
        assert_eq!(mods.default_voice.as_deref(), Some("english/steve"));
        // A differently-cased setting with the other separator still resolves to the installed voice.
        write(&src.join("PaceNote.ini"), "[SETTINGS]\nsounds=ENGLISH\\STEVE\n");
        let mods = crate::command::CodriverMods::from_dir(src.clone()).unwrap();
        assert_eq!(mods.default_voice.as_deref(), Some("english/steve"));
        // A setting naming no installed voice falls back to the first installed voice.
        write(&src.join("PaceNote.ini"), "[SETTINGS]\nsounds=ghost/voice\n");
        let mods = crate::command::CodriverMods::from_dir(src.clone()).unwrap();
        assert_eq!(mods.default_voice.as_deref(), Some("english/steve"));
        // Without a setting the first installed voice speaks.
        fs::remove_file(src.join("PaceNote.ini")).unwrap();
        let mods = crate::command::CodriverMods::from_dir(src).unwrap();
        assert_eq!(mods.default_voice.as_deref(), Some("english/steve"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn owned_packs_list_with_their_problems() {
        use crate::job::Progress;
        use std::sync::atomic::AtomicBool;
        let dir = crate::test_dir("codriver-list");
        let src = dir.join("voice");
        plugin_source(&src);
        let home = dir.join("home");
        imported(&home, &src).unwrap();
        let voices = voice_packs(&home);
        assert_eq!(voices.len(), 1);
        assert_eq!(voices[0].problem, None);
        let progress = Progress { step: "Importing", done: 0, total: 0 };
        assert_eq!(progress.line(), "Importing…");
        let _ = AtomicBool::new(false);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// One style per file, one range set per file: identical files do not differ, a changed take does.
    #[test]
    fn differing_style_and_range_sets_are_detected() {
        let dir = crate::test_dir("codriver-differ");
        let write_style = |name: &str, take: &str| {
            write(
                &dir.join(format!("config/pacenotes/{name}")),
                &format!("[PACENOTE::ONE_LEFT]\nid=0\nSounds=1\nSnd0={take}\n"),
            );
        };
        let write_ranges = |name: &str, take: &str| {
            write(&dir.join(format!("config/ranges/{name}")), &format!("[RANGE::50]\nSounds=1\nSnd0={take}\n"));
        };
        write_style("A.ini", "a.ogg");
        write_style("Same.ini", "a.ogg");
        write_style("B.ini", "b.ogg");
        write_ranges("RA.ini", "ra.ogg");
        write_ranges("RB.ini", "rb.ogg");
        let (styles, ranges) = (codriver::styles(&dir), codriver::range_sets(&dir));
        assert!(!call_sets_differ(&dir, &["A.ini".into()], &ranges).0, "a single style cannot differ");
        assert!(!call_sets_differ(&dir, &["A.ini".into(), "Same.ini".into()], &ranges).0);
        assert!(call_sets_differ(&dir, &["A.ini".into(), "B.ini".into()], &ranges).0);
        assert!(!call_sets_differ(&dir, &styles, &["RA.ini".into()]).1, "a single range set cannot differ");
        assert!(call_sets_differ(&dir, &styles, &["RA.ini".into(), "RB.ini".into()]).1);
        // A missing file is skipped, not a difference.
        assert!(!call_sets_differ(&dir, &["A.ini".into(), "Missing.ini".into()], &ranges).0);
        assert!(!styles.is_empty() && !ranges.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }
}
