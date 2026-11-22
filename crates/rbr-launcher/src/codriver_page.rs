//! The Library > Co-drivers page: the co-driver (a person in a language, `codriver_packs::speakers`) with a preview,
//! their calls when they have several (each a pack's install), what they call on the Drive page's stage (the notes
//! are picked by themselves, `codriver_packs::pick_notes`), the corner numbers of an older plugin pack, the call
//! timing, and the owned packs (`codriver_packs`); and Get more's list of the community's voices
//! (`codriver_packs::catalog`).

use egui::{ComboBox, DragValue, RichText};
use rbr_sound::preview::{self as codriver_preview, Preview, LADDER};

use crate::codriver_packs::{catalog, source_name, Speaker, Style};
use crate::command::ORIGINAL_VOICE;
use crate::get_page::GetMore;
use crate::kit;
use crate::pages::combo;
use crate::settings_page::{note, row_tip};
use crate::theme::{medium, mono, GOOD, TEXT, TEXT_2, TEXT_3, WARN};
use crate::Form;

/// The longest "Call ahead", seconds. Ours: the page stopped at 8. 15 s is about 800 m at 200 km/h, as far as a pace note
/// reads usefully (the ranges between notes are metres to a few hundred).
const CALLOUT_MAX: f32 = 15.0;

/// The widest the page's content gets.
const PAGE_WIDTH: f32 = 640.0;

/// A pack's row: its name, `line` under it in `color`, and Remove (enabled as `remove` says) when it has one; whether
/// Remove was clicked.
fn pack_row(rows: &mut kit::Rows, name: &str, line: &str, color: egui::Color32, remove: Option<bool>) -> bool {
    rows.full(|ui| {
        let mut clicked = false;
        kit::label_over(ui, name, line, color, 90.0);
        if let Some(enabled) = remove {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                clicked = ui.add_enabled(enabled, egui::Button::new("Remove")).clicked();
            });
        }
        clicked
    })
}

/// A call style or range set file's stem: without `.ini`.
fn ini_stem(file: &str) -> &str {
    match file.len().checked_sub(4) {
        Some(n) if file[n..].eq_ignore_ascii_case(".ini") => &file[..n],
        _ => file,
    }
}

/// The stem's base when it is a `-xx` language variant (exactly two ASCII lowercase letters after the last `-`):
/// ReRBR speaks English only, so variants stay out of the dropdowns unless chosen.
fn base_stem(file: &str) -> (&str, Option<&str>) {
    let stem = ini_stem(file);
    match stem.rsplit_once('-') {
        Some((base, lang)) if !base.is_empty() && lang.len() == 2 && lang.bytes().all(|b| b.is_ascii_lowercase()) => {
            (stem, Some(base))
        }
        _ => (stem, None),
    }
}

/// Whether the call style or range set file is offered: `-xx` language variants stay out unless chosen.
pub fn show_call_file(file: &str, current: &str) -> bool {
    base_stem(file).1.is_none() || file.eq_ignore_ascii_case(current)
}

/// A Corner numbers file's label: what the player hears for each corner. Unknown stems show as themselves.
pub fn style_label(file: &str) -> String {
    let (stem, base) = base_stem(file);
    match base.unwrap_or(stem).to_ascii_lowercase().as_str() {
        "numeric" => "Numbers, 1 tightest (\"one left\")".into(),
        "numeric-swapped" => "Numbers, 1 tightest (\"left one\")".into(),
        "numeric-descending" => "Numbers, 6 tightest (\"six left\")".into(),
        "numeric-descending-swapped" => "Numbers, 6 tightest (\"left six\")".into(),
        "descriptive" => "Words (\"easy left\", \"hairpin\")".into(),
        "rbr" => "Original RBR calls".into(),
        "rbr-enhanced" => "Original RBR calls, more of them".into(),
        _ => stem.to_owned(),
    }
}

/// A co-driver as the dropdown names them: their name and language, or an uncatalogued folder with its source.
fn speaker_text(s: &Speaker) -> String {
    match s.language {
        Some(_) => s.label(),
        None => format!("{} ({})", s.name, source_name(s.styles[0].pack.as_deref())),
    }
}

/// The Co-driver dropdown's choice: off, the original 1.02 voice, or a voice by its source pack (none: the
/// RallySimFans install's plugin).
#[derive(Debug, Clone, PartialEq, Eq)]
enum VoiceChoice {
    Off,
    Original,
    Voice { pack: Option<String>, voice: String },
}

impl VoiceChoice {
    /// The dropdown's choice for the saved fields: an unset voice plays its source's default (`default_voice`:
    /// its `PaceNote.ini`'s, else its first voice), so an old settings file without `codriver_voice` still names
    /// the voice heard; with no voice source at all the original voice drives.
    fn of(codriver: bool, pack: Option<&str>, voice: Option<&str>, default_voice: Option<&str>) -> Self {
        match voice.or(default_voice) {
            _ if !codriver => VoiceChoice::Off,
            Some(ORIGINAL_VOICE) | None => VoiceChoice::Original,
            Some(voice) => VoiceChoice::Voice { pack: pack.map(str::to_owned), voice: voice.to_owned() },
        }
    }

    /// The source and voice folder, for a voice.
    fn voice(&self) -> Option<(Option<&str>, &str)> {
        match self {
            VoiceChoice::Voice { pack, voice } => Some((pack.as_deref(), voice)),
            _ => None,
        }
    }

    /// The dropdown's text: the voice and its source.
    fn label(&self) -> String {
        match self {
            VoiceChoice::Off => "Off".into(),
            VoiceChoice::Original => "Original (1.02)".into(),
            VoiceChoice::Voice { pack: None, voice } => format!("{voice} (RallySimFans install)"),
            VoiceChoice::Voice { pack: Some(pack), voice } => format!("{voice} ({pack})"),
        }
    }
}

impl Form {
    /// The Co-driver dropdown's choice from the fields, resolving an unset voice to its source's default.
    fn voice_choice(&self) -> VoiceChoice {
        let default = self.codriver_mods.as_ref().and_then(|mods| mods.voice(None));
        VoiceChoice::of(
            self.codriver,
            self.codriver_pack.as_deref(),
            self.codriver_voice.as_deref(),
            default.as_deref(),
        )
    }

    /// The page: the voice and its preview, the corner numbers (when they change anything), the call timing, what
    /// the selected stage gets, and the packs.
    pub fn codriver_section(&mut self, ui: &mut egui::Ui) {
        kit::page_header(ui, "Co-drivers", Some("Who reads your pace notes, and how early."), |_| {});
        egui::ScrollArea::vertical().id_salt("codrivers").auto_shrink([false, false]).show(ui, |ui| {
            kit::body(ui, |ui| {
                ui.set_max_width(PAGE_WIDTH);
                kit::group_label(ui, "Voice", None);
                let mut warn = None;
                kit::panel(ui, |rows| {
                    warn = self.codriver_voice_row(rows);
                    self.codriver_calls_row(rows);
                    let custom = matches!(self.voice_choice(), VoiceChoice::Voice { .. });
                    if let Some(mods) = self.codriver_mods.as_ref().filter(|_| custom) {
                        if self.codriver_styles_differ {
                            let tip = "How the voice numbers the corners";
                            Self::call_file_row(
                                rows,
                                "Corner numbers",
                                tip,
                                &mods.styles,
                                &mut self.codriver_style,
                                style_label,
                            );
                        }
                    }
                    let tip = "How many seconds before a note the co-driver calls it, at your speed";
                    row_tip(rows, "Call ahead", tip, |ui| {
                        let value = &mut self.marker.callout_distance;
                        let step = |ui: &mut egui::Ui, text| {
                            ui.add(egui::Button::new(text).min_size(egui::vec2(26.0, 0.0))).on_hover_text(tip).clicked()
                        };
                        let plus = step(ui, "+");
                        ui.add(DragValue::new(value).range(0.05..=CALLOUT_MAX).speed(0.05).suffix(" s"))
                            .on_hover_text(tip);
                        let minus = step(ui, "−");
                        *value = (*value + if plus { 0.25 } else { 0.0 } - if minus { 0.25 } else { 0.0 })
                            .clamp(0.05, CALLOUT_MAX);
                    });

                    // The custom co-driver fires on its own lookahead and never reads the threshold: only the
                    // original voice skips notes below it.
                    if matches!(self.voice_choice(), VoiceChoice::Original) {
                        let tip = "The original co-driver skips a note while the speed is below this";
                        row_tip(rows, "Quiet below", tip, |ui| {
                            ui.add(DragValue::new(&mut self.marker.marker_threshold).suffix(" km/h"))
                                .on_hover_text(tip);
                        });
                    }
                    if let Some(warn) = warn.take() {
                        note(rows, WARN, &format!("⚠ {warn}: the original voice"));
                    }
                });
                ui.add_space(4.0);
                self.codriver_status_row(ui);
                ui.add_space(14.0);
                self.codriver_packs_ui(ui);
            })
        });
    }

    /// The Co-driver row: who calls the pace notes, with a preview of the corner ladder and one distance call. An
    /// unset voice plays its source's default, so an old settings file still names the voice heard. A chosen source
    /// that is gone or broke: why the original voice drives instead.
    fn codriver_voice_row(&mut self, rows: &mut kit::Rows) -> Option<String> {
        let tip = "Who calls the pace notes: off (no pace notes at all), the original 1.02 co-driver, or a \
                     co-driver from the RallySimFans install's Pacenote plugin (only read) or from a pack you added. \
                     Calls then picks how they say the corners, when they come in several styles. Whether a pack \
                     covers every note of a stage varies by pack.";
        let current = self.voice_choice();
        row_tip(rows, "Co-driver", tip, |ui| {
            // The preview beside the dropdown: the corner ladder and one distance call with this voice. Built
            // on a thread, so the UI never stalls.
            let idle = self.child.is_none() && !self.content.busy();
            let preview = ui
                .add_enabled(idle && !matches!(current, VoiceChoice::Off), egui::Button::new("▶ Preview"))
                .on_hover_text(
                    "Hear the corners, tightest to fastest, and one distance call with this voice and corner \
                     numbers",
                );
            if preview.clicked() {
                self.preview_codriver();
            }
            let mut picked = current.clone();
            let speaker = self.current_speaker();
            let text = speaker.map_or_else(|| current.label(), speaker_text);
            // A picked co-driver keeps the current calls when they have them.
            let calls = speaker.zip(current.voice()).and_then(|(s, (pack, folder))| s.style(pack, folder));
            let calls = calls.map(|s| s.label.clone());
            ComboBox::from_id_salt("codriver voice").selected_text(text).show_ui(ui, |ui| {
                ui.selectable_value(&mut picked, VoiceChoice::Off, "Off");
                ui.selectable_value(&mut picked, VoiceChoice::Original, "Original (1.02)");
                let mut item = |ui: &mut egui::Ui, s: &Speaker, text: String| {
                    let on = speaker.is_some_and(|c| std::ptr::eq(c, s));
                    if ui.selectable_label(on, text).clicked() && !on {
                        let style = s.style_for(calls.as_deref());
                        picked = VoiceChoice::Voice { pack: style.pack.clone(), voice: style.folder.clone() };
                    }
                };
                for s in self.codriver_speakers.iter().filter(|s| s.complete()) {
                    item(ui, s, speaker_text(s));
                }
                if self.codriver_speakers.iter().any(|s| !s.complete()) {
                    ui.label(RichText::new("Incomplete: the original voice says the rest").size(11.5).color(TEXT_3));
                    for s in self.codriver_speakers.iter().filter(|s| !s.complete()) {
                        let best = s.styles.iter().map(|t| t.corners).max().unwrap_or(0);
                        item(ui, s, format!("{} · {best} of {} corners", speaker_text(s), LADDER.len()));
                    }
                }
            });
            match picked {
                _ if picked == current => {}
                // Off keeps the pack and voice, so turning it back on restores them.
                VoiceChoice::Off => self.codriver = false,
                VoiceChoice::Original => {
                    (self.codriver, self.codriver_voice) = (true, Some(ORIGINAL_VOICE.to_owned()));
                }
                // A changed pack rescans and normalizes in `pages.rs`.
                VoiceChoice::Voice { pack, voice } => {
                    (self.codriver, self.codriver_pack, self.codriver_voice) = (true, pack, Some(voice));
                }
            }
        });

        let VoiceChoice::Voice { pack, .. } = self.voice_choice() else { return None };
        match pack.as_deref() {
            Some(pack) => match self.owned_voices.iter().find(|p| p.name == pack) {
                Some(found) => found.problem.clone().map(|p| format!("{pack}: {p}")),
                None => Some(format!("{pack}: no longer installed")),
            },
            None if !self.rsf_plugin => Some("the RallySimFans install has no Pacenote plugin".to_owned()),
            None => None,
        }
    }

    /// The speaker of the chosen voice folder, if it is listed.
    fn current_speaker(&self) -> Option<&Speaker> {
        let choice = self.voice_choice();
        let (pack, folder) = choice.voice()?;
        self.codriver_speakers.iter().find(|s| s.style(pack, folder).is_some())
    }

    /// The Calls row, when the chosen co-driver has more than one: how they call the corners, each its own install
    /// (a pack, or the RallySimFans install) and voice folder.
    fn codriver_calls_row(&mut self, rows: &mut kit::Rows) {
        let Some(speaker) = self.current_speaker().filter(|s| s.styles.len() > 1).cloned() else { return };
        let mut chosen = (self.codriver_pack.clone(), self.codriver_voice.clone().unwrap_or_default());
        let ladder = speaker.style(chosen.0.as_deref(), &chosen.1).and_then(|s| s.ladder.clone());
        let tip = format!(
            "How {} calls the corners: each a download or a version of its own. Preview to hear it{}",
            speaker.name,
            ladder.map_or_else(String::new, |l| format!("\n\nTightest to fastest: {l}"))
        );
        row_tip(rows, "Calls", &tip, |ui| {
            let styles: Vec<(Option<String>, String)> =
                speaker.styles.iter().map(|s| (s.pack.clone(), s.folder.clone())).collect();
            let text = |(pack, folder): (Option<String>, String)| {
                speaker.style(pack.as_deref(), &folder).map_or(folder, Style::text)
            };
            combo(ui, "codriver calls", None, &mut chosen, &styles, text).on_hover_text(&tip);
        });
        if (chosen.0.as_ref(), Some(&chosen.1)) != (self.codriver_pack.as_ref(), self.codriver_voice.as_ref()) {
            // A changed pack rescans and normalizes in `pages.rs`.
            (self.codriver_pack, self.codriver_voice) = (chosen.0, Some(chosen.1));
        }
    }

    /// A Corner numbers row: the plugin's `files` by `label`, language variants left out unless chosen.
    fn call_file_row(
        rows: &mut kit::Rows,
        name: &str,
        tip: &str,
        files: &[String],
        chosen: &mut String,
        label: fn(&str) -> String,
    ) {
        row_tip(rows, name, tip, |ui| {
            let shown: Vec<String> = files.iter().filter(|f| show_call_file(f, chosen)).cloned().collect();
            combo(ui, name, None, chosen, &shown, |file| label(&file)).on_hover_text(tip);
        });
    }

    /// The summary under the voice: what the co-driver says on the Drive page's stage (the note file on hover),
    /// refreshed when its inputs change, never scanned per draw.
    fn codriver_status_row(&mut self, ui: &mut egui::Ui) {
        self.refresh_notes_cache();
        let summary = &self.notes_summary;
        let text = format!("On {}: {}", summary.stage, summary.text);
        let response = ui.add(egui::Label::new(RichText::new(text).size(12.0).color(TEXT_3)).wrap());
        if let Some(file) = &summary.file {
            response.on_hover_text(file.display().to_string());
        }
    }

    /// The packs: the voices and the pace notes, the ones ReRBR owns with Remove, and the link to Get more.
    fn codriver_packs_ui(&mut self, ui: &mut egui::Ui) {
        kit::group_label(ui, "Installed packs", None);
        // No removals while importing or driving.
        let idle = !self.content.busy() && self.child.is_none();
        let (mut remove_voice, mut remove_notes) = (None, None);
        kit::panel(ui, |rows| {
            if self.rsf_plugin && !self.rsf_voices.is_empty() {
                let n = self.rsf_voices.len();
                let line = format!("{n} voice{} · read only", if n == 1 { "" } else { "s" });
                pack_row(rows, "RallySimFans install", &line, TEXT_2, None);
            }
            for pack in &self.owned_voices {
                let (line, color) = match &pack.problem {
                    Some(problem) => (problem.clone(), WARN),
                    None => {
                        let counts = (pack.voices.len(), pack.styles.len(), pack.ranges.len());
                        (format!("✔ {} voices, {} call styles, {} range sets", counts.0, counts.1, counts.2), GOOD)
                    }
                };
                if pack_row(rows, &pack.name, &line, color, Some(idle)) {
                    remove_voice = Some(pack.name.clone());
                }
            }
            for pack in &self.owned_notes {
                let line = format!("✔ {} note files", pack.files);
                if pack_row(rows, &pack.name, &line, GOOD, Some(idle)) {
                    remove_notes = Some(pack.name.clone());
                }
            }
            if self.owned_voices.is_empty() && self.owned_notes.is_empty() {
                note(rows, TEXT_2, "No packs added yet.");
            }
            if let Some(Err(text)) = &self.codriver_status {
                note(rows, WARN, &format!("⚠ {text}"));
            }
        });
        if let Some(pack) = remove_voice {
            self.codriver_status = crate::codriver_packs::remove_voice(&self.here(), &pack).err().map(Err);
            if self.codriver_pack.as_deref() == Some(&pack) {
                self.codriver_pack = None;
            }
            self.rescan_codrivers();
        }
        if let Some(pack) = remove_notes {
            self.codriver_status = crate::codriver_packs::remove_notes(&self.here(), &pack).err().map(Err);
            self.rescan_codrivers();
        }
        ui.add_space(8.0);
        if kit::link(ui, "Get more co-drivers…")
            .on_hover_text("Voices and pace notes, on Library > Co-drivers")
            .clicked()
        {
            self.open_get_more(GetMore::Codrivers);
        }
    }

    /// Get more's voices: the catalogued packs by language, each with its calls, whether a source has it and its
    /// download page.
    pub fn codriver_catalog_ui(&self, ui: &mut egui::Ui) {
        let has = |voice: &catalog::Voice| {
            self.codriver_speakers.iter().any(|s| s.styles.iter().any(|t| voice.is(&t.folder)))
        };
        kit::panel(ui, |rows| {
            for pack in catalog::PACKS {
                let installed = pack.voices.iter().any(has);
                rows.full(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Open ↗").on_hover_text(pack.page).clicked() {
                            crate::crash::open(pack.page.as_ref());
                        }
                        if installed {
                            ui.label(RichText::new("✔ Added").color(GOOD));
                        }
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.spacing_mut().item_spacing.y = 3.0;
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(pack.title).family(medium()).color(TEXT));
                                ui.label(RichText::new(pack.language).font(mono(11.5)).color(TEXT_3));
                            });
                            let line = format!("by {} · {}", pack.by, pack.calls);
                            ui.add(egui::Label::new(RichText::new(line).size(12.0).color(TEXT_2)).wrap());
                            ui.add_space(8.0);
                        });
                    });
                });
            }
        });
    }

    /// Plays the corner ladder and one distance call in the chosen voice through the menus' sound, built on its own
    /// thread.
    fn preview_codriver(&mut self) {
        let build: Box<dyn FnOnce() -> Result<Preview, String> + Send> = match self.voice_choice() {
            VoiceChoice::Off => return,
            VoiceChoice::Original => {
                let data = self.data.clone();
                Box::new(move || codriver_preview::original_preview(&data))
            }
            VoiceChoice::Voice { .. } => {
                let Some(mods) = &self.codriver_mods else { return };
                let Some(voice) = mods.voice(self.codriver_voice.as_deref()) else { return };
                let (plugin, style, ranges) = (mods.dir.clone(), self.codriver_style.clone(), mods.default_ranges());
                Box::new(move || codriver_preview::custom_preview(&plugin, &voice, &style, &ranges))
            }
        };
        let gain = self.codriver_volume;
        if let Some(player) = self.menu_player() {
            player.preview(gain, build);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::settings::{load, Settings};
    use crate::{Form, SETTINGS_FILE};

    fn write(path: PathBuf, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
    }

    /// A voice download with one call, one range and one sound under `root`.
    fn voice_download(root: &Path) -> PathBuf {
        let download = root.join("voice");
        write(download.join("PaceNote.ini"), "[SETTINGS]\nsounds=english/steve\n");
        write(download.join("config/pacenotes/Rbr.ini"), "[PACKAGE::RBR]\nfile=calls.ini\n");
        write(download.join("config/pacenotes/calls.ini"), "[PACENOTE::ONE_LEFT]\nid=0\nSounds=1\nSnd0=one_left.ogg\n");
        write(download.join("config/ranges/Rbr.ini"), "[RANGE::50]\nSounds=1\nSnd0=50_1.ogg\n");
        write(download.join("sounds/english/steve/one_left.ogg"), "OggS fake");
        download
    }

    /// The co-driver rows drawn offscreen to `RBR_SCREENSHOTS`: a locally owned voice pack and Tanner notes beside
    /// the game folder's plugin. Needs `RBR_DATA`, `RBR_PATCH`, `RBR_NGP_PHYSICS` and `RBR_RSF`.
    #[test]
    fn screenshots_of_the_codriver_rows() {
        let env = |key: &str| std::env::var_os(key).map(|v| v.to_string_lossy().into_owned());
        let (Some(shots), Some(data), Some(patch), Some(ngp), Some(_)) =
            (env("RBR_SCREENSHOTS"), env("RBR_DATA"), env("RBR_PATCH"), env("RBR_NGP_PHYSICS"), env("RBR_RSF"))
        else {
            eprintln!("skipping: RBR_SCREENSHOTS, RBR_DATA, RBR_PATCH, RBR_NGP_PHYSICS or RBR_RSF not set");
            return;
        };
        let root = crate::test_dir("page-codriver-shots");
        // A voice download and a notes-only download, as imported.
        let download = voice_download(&root);
        let notes_src = root.join("notes");
        write(
            notes_src.join("Tanner/tanner.ini"),
            "[PACENOTES]\ncount = 1\n\n[P0]\ntype = 16\ndistance = 100.0\nflag = 0\n",
        );
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        crate::codriver_packs::import(&home, &download, &std::sync::atomic::AtomicBool::new(false)).unwrap();
        crate::codriver_packs::import(&home, &notes_src, &std::sync::atomic::AtomicBool::new(false)).unwrap();
        let start_in = |home: &Path| {
            crate::test_start(home.join(SETTINGS_FILE), Some(data.clone().into()), Some(patch.clone().into()))
        };
        // The owned packs drive when chosen, over the game folder's plugin.
        let settings = Settings {
            map: Some(54),
            ngp_physics: Some(ngp.clone()),
            codriver_pack: Some("voice".into()),
            ..Settings::default()
        };
        let mut local = load(&start_in(&home), settings).unwrap();
        local.read_only = true;
        assert!(local.codriver_mods.is_some());
        assert_eq!(
            local.codriver_mods.as_ref().unwrap().dir,
            home.join(crate::codriver_packs::VOICES_DIR).join("voice")
        );
        // With no chosen pack: the game folder's plugin drives, as before.
        let settings = Settings { map: Some(54), ngp_physics: Some(ngp), ..Settings::default() };
        let elsewhere = root.join("rsf-home");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let mut installed = load(&start_in(&elsewhere), settings).unwrap();
        installed.read_only = true;
        assert!(installed.codriver_mods.is_some());
        assert!(installed.rsf_plugin);
        for (name, form) in [("codriver-local.png", &mut local), ("codriver-rsf.png", &mut installed)] {
            // The summary names the stage, the voice and the set; the file rides the tooltip.
            form.refresh_notes_cache();
            let summary = &form.notes_summary;
            assert!(!summary.stage.is_empty(), "{summary:?}");
            assert!(summary.text.contains(" calls ") && summary.text.ends_with(")"), "{summary:?}");
            assert!(summary.file.as_ref().is_some_and(|f| f.extension().is_some_and(|e| e == "ini")), "{summary:?}");
            // The recent downloads are scanned on a thread: drawn once, given a moment, then drawn for the shot.
            let mut section = |ctx: &egui::Context| {
                ctx.root_ui(|root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        form.codriver_section(ui);
                    })
                });
            };
            crate::capture::draw(&mut section).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(500));
            let rgba = crate::capture::draw(&mut section).unwrap();
            crate::capture::write_png(&Path::new(&shots).join(name), &rgba).unwrap();
            if name == "codriver-local.png" {
                let hint = crate::capture::draw(|ctx| {
                    section(ctx);
                    Form::drop_hint(ctx);
                })
                .unwrap();
                crate::capture::write_png(&Path::new(&shots).join("codriver-drop.png"), &hint).unwrap();
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A V3 install: one call style, its corners recorded in voice folder `folder` and said as `grades` (tightest to
    /// fastest), under `root/<name>`.
    fn v3_install(root: &Path, name: &str, folder: &str, grades: [&str; 8]) -> PathBuf {
        use rbr_sound::preview::LADDER;
        let dir = root.join(name);
        write(dir.join("PaceNote.ini"), &format!("[SETTINGS]\nlanguage=english\nsounds={folder}\n"));
        let calls: String = LADDER.iter().map(|id| format!("[PACENOTE::C{id}]\nid={id}\nSounds=1\nSnd0={id}.ogg\n")).collect();
        write(dir.join("config/pacenotes/Rbr.ini"), &calls);
        write(dir.join("config/ranges/Rbr.ini"), "[RANGE::100]\nSounds=1\nSnd0=100.ogg\n");
        let texts: String = LADDER.iter().zip(grades).map(|(id, g)| format!("C{id}={g} Left\n")).collect();
        write(dir.join("language/english/pacenotes/packages/corners/strings.ini"), &format!("[STRINGS]\n{texts}"));
        for file in LADDER.iter().map(|id| format!("{id}.ogg")).chain(["100.ogg".into()]) {
            write(dir.join("sounds").join(folder).join(file), "OggS fake");
        }
        dir
    }

    /// The Co-drivers page with co-drivers in several styles, and Get more's list, drawn offscreen to
    /// `RBR_SCREENSHOTS`. Needs `RBR_DATA`, `RBR_PATCH` and `RBR_NGP_PHYSICS` (and `RBR_RSF` for its plugin's voices).
    #[test]
    fn screenshots_of_the_codriver_styles() {
        let env = |key: &str| std::env::var_os(key).map(|v| v.to_string_lossy().into_owned());
        let (Some(shots), Some(data), Some(patch), Some(ngp)) =
            (env("RBR_SCREENSHOTS"), env("RBR_DATA"), env("RBR_PATCH"), env("RBR_NGP_PHYSICS"))
        else {
            eprintln!("skipping: RBR_SCREENSHOTS, RBR_DATA, RBR_PATCH or RBR_NGP_PHYSICS not set");
            return;
        };
        let root = crate::test_dir("page-codriver-styles");
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let numeric_a = ["HP", "1", "2", "3", "4", "5", "6", "Flat"];
        let numeric_b = ["HP", "1", "Square", "2", "3", "4", "5", "6"];
        let installs = [
            ("Janne Numeric A Number First", "Janne Laahanen EnNu A NF", numeric_a),
            ("Janne Numeric B Number First", "Janne Laahanen EnNu B NF", numeric_b),
            ("Janne Descriptive", "Janne Laahanen EnDe", ["HP", "Bad", "Square", "K", "Medium", "Fast", "Easy", "Flat"]),
            ("Rita Fullon A", "RitaFullonV3 (benofficial2)", numeric_a),
            ("Rita Fullon B", "RitaFullonV3 (benofficial2)", numeric_b),
            ("Darren", "Darren V3", numeric_a),
        ];
        let no_cancel = std::sync::atomic::AtomicBool::new(false);
        for (name, folder, grades) in installs {
            crate::codriver_packs::import(&home, &v3_install(&root.join("downloads"), name, folder, grades), &no_cancel)
                .unwrap();
        }
        let start = crate::test_start(home.join(SETTINGS_FILE), Some(data.into()), Some(patch.into()));
        let settings = Settings {
            map: Some(54),
            ngp_physics: Some(ngp),
            codriver_pack: Some("Janne Numeric B Number First".into()),
            codriver_voice: Some("Janne Laahanen EnNu B NF".into()),
            ..Settings::default()
        };
        let mut form = load(&start, settings).unwrap();
        form.read_only = true;
        // The three installs, and the RSF install's Janne when it has him; their texts name the corners.
        let janne = form.codriver_speakers.iter().find(|s| s.name == "Janne Laahanen").unwrap();
        let owned: Vec<_> = janne.styles.iter().filter(|s| s.pack.is_some()).collect();
        assert_eq!(owned.len(), 3, "{janne:?}");
        assert!(owned.iter().all(|s| s.ladder.is_some()), "{janne:?}");
        assert_eq!(form.current_speaker().map(|s| s.name.as_str()), Some("Janne Laahanen"));
        let shot = |form: &mut Form, name: &str, modal: bool| {
            let rgba = crate::capture::draw(|ctx| {
                ctx.root_ui(|root| {
                    egui::CentralPanel::default().show(root, |ui| form.codriver_section(ui));
                });
                if modal {
                    form.get_more_modal(ctx);
                }
            })
            .unwrap();
            crate::capture::write_png(&Path::new(&shots).join(name), &rgba).unwrap();
        };
        shot(&mut form, "codriver-styles.png", false);
        form.open_get_more(crate::get_page::GetMore::Codrivers);
        shot(&mut form, "codriver-get-more.png", true);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The Co-driver dropdown reads the saved fields: an unset voice names its source's default, and without a
    /// source the original voice drives. Needs no game data.
    #[test]
    fn the_dropdowns_read_the_fields() {
        use super::VoiceChoice;
        use crate::command::ORIGINAL_VOICE;
        let voice =
            |pack: Option<&str>, voice: &str| VoiceChoice::Voice { pack: pack.map(Into::into), voice: voice.into() };
        assert_eq!(VoiceChoice::of(false, Some("pack"), Some("v"), None), VoiceChoice::Off);
        assert_eq!(VoiceChoice::of(true, Some("pack"), Some(ORIGINAL_VOICE), None), VoiceChoice::Original);
        assert_eq!(VoiceChoice::of(true, None, Some("Janne"), Some("steve")), voice(None, "Janne"));
        assert_eq!(VoiceChoice::of(true, Some("pack"), None, Some("steve")), voice(Some("pack"), "steve"));
        assert_eq!(VoiceChoice::of(true, Some("pack"), None, None), VoiceChoice::Original);
        assert_eq!(voice(None, "Janne").label(), "Janne (RallySimFans install)");
    }

    /// The summary follows the co-driver switch and the voice: switching one dirties the cache (as `pages.rs` does),
    /// and the next refresh says what the launch passes: the picked set's notes with a custom voice, the stage's
    /// own with none, never a stale file. Needs `RBR_DATA`, `RBR_PATCH` and `RBR_NGP_PHYSICS`.
    #[test]
    fn notes_summary_follows_the_voice() {
        use crate::command::ORIGINAL_VOICE;
        let env = |key: &str| std::env::var_os(key).map(|v| v.to_string_lossy().into_owned());
        let (Some(data), Some(patch), Some(ngp)) = (env("RBR_DATA"), env("RBR_PATCH"), env("RBR_NGP_PHYSICS")) else {
            eprintln!("skipping: RBR_DATA, RBR_PATCH or RBR_NGP_PHYSICS not set");
            return;
        };
        let root = crate::test_dir("page-notes-cache");
        let download = voice_download(&root);
        let notes_src = root.join("notes");
        let note = "[PACENOTES]\ncount = 2\n\n[P0]\ntype = 16\ndistance = 100.0\nflag = 0\n\n\
                    [P1]\ntype = 3\ndistance = 200.0\nflag = 0\n";
        write(notes_src.join("Tanner/tanner.ini"), note);
        write(notes_src.join("Somewhere/Somewhere Fast.ini"), note);
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let no_cancel = std::sync::atomic::AtomicBool::new(false);
        crate::codriver_packs::import(&home, &download, &no_cancel).unwrap();
        crate::codriver_packs::import(&home, &notes_src, &no_cancel).unwrap();
        let start = crate::test_start(home.join(SETTINGS_FILE), Some(data.into()), Some(patch.into()));
        let settings = Settings {
            map: Some(54),
            ngp_physics: Some(ngp),
            codriver_pack: Some("voice".into()),
            ..Settings::default()
        };
        let mut form = load(&start, settings).unwrap();
        form.read_only = true;
        // Only the owned packs: an installed MyPacenotes would out-cover them (LuppisV3).
        form.note_sources =
            form.owned_notes.iter().map(|p| crate::codriver_packs::NoteSource::read(&p.dir, Some(&p.name))).collect();
        form.cached_notes_dirty = true;
        let ctx = egui::Context::default();
        let summary = |form: &mut crate::Form| {
            form.refresh_notes_cache();
            form.notes_summary.text.clone()
        };
        // The pack's notes for Tanner play by themselves.
        assert_eq!(summary(&mut form), "english/steve calls notes (2 notes)");
        assert!(form.notes_summary.file.as_ref().is_some_and(|f| f.ends_with("Tanner/tanner.ini")));
        // A note set never claims to play with the co-driver off or the 1.02 voice.
        let before = form.notes_inputs();
        form.codriver = false;
        form.invalidate_notes_cache(&ctx, before);
        assert_eq!(summary(&mut form), "no co-driver, no notes are called");
        let before = form.notes_inputs();
        (form.codriver, form.codriver_voice) = (true, Some(ORIGINAL_VOICE.into()));
        form.invalidate_notes_cache(&ctx, before);
        assert_eq!(summary(&mut form), "the 1.02 co-driver calls the stage's own notes");
        // No pack has the stage: its own notes, said plainly.
        let before = form.notes_inputs();
        form.codriver_voice = None;
        form.note_sources.clear();
        form.invalidate_notes_cache(&ctx, before);
        assert_eq!(summary(&mut form), "english/steve calls the stage's own notes");
        assert_eq!(form.notes_summary.file, None);
        // A missing voice pack (rescans dirty the cache): the original voice drives.
        form.codriver_mods = None;
        form.cached_notes_dirty = true;
        assert_eq!(summary(&mut form), "the 1.02 co-driver calls the stage's own notes");
        // Unchanged inputs dirty nothing.
        form.cached_notes_dirty = false;
        form.invalidate_notes_cache(&ctx, form.notes_inputs());
        assert!(!form.cached_notes_dirty);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Corner numbers show human labels for the known stems, the bare stem otherwise; `-xx` language
    /// variants stay out of the dropdowns unless chosen. Needs no game data.
    #[test]
    fn corner_and_distance_labels_and_language_variants() {
        use super::{show_call_file, style_label};
        assert_eq!(style_label("Numeric.ini"), "Numbers, 1 tightest (\"one left\")");
        assert_eq!(style_label("Numeric-Swapped.ini"), "Numbers, 1 tightest (\"left one\")");
        assert_eq!(style_label("Numeric-Descending.ini"), "Numbers, 6 tightest (\"six left\")");
        assert_eq!(style_label("Numeric-Descending-Swapped.ini"), "Numbers, 6 tightest (\"left six\")");
        assert_eq!(style_label("Descriptive.ini"), "Words (\"easy left\", \"hairpin\")");
        assert_eq!(style_label("Rbr.ini"), "Original RBR calls");
        assert_eq!(style_label("Rbr-Enhanced.ini"), "Original RBR calls, more of them");
        assert_eq!(style_label("Mine.ini"), "Mine");
        // Variants label as their base stem but stay hidden unless chosen.
        assert_eq!(style_label("Numeric-cz.ini"), style_label("Numeric.ini"));
        assert!(!show_call_file("Numeric-cz.ini", "Numeric.ini"));
        assert!(show_call_file("Numeric-cz.ini", "Numeric-cz.ini"));
        assert!(show_call_file("Numeric.ini", "Numeric.ini"));
        // `Rbr-Enhanced` is no variant: more than two letters after the last dash.
        assert!(show_call_file("Rbr-Enhanced.ini", "Numeric.ini"));
    }

    /// The RSF install's styles and range sets all load the same packages, so they are detected as identical, and
    /// its default voice's preview says every corner and the distance. Needs `RBR_RSF`.
    #[test]
    fn the_rsf_installs_styles_are_identical_and_its_preview_speaks() {
        let Some(rsf) = std::env::var_os("RBR_RSF").map(|v| v.to_string_lossy().into_owned()) else {
            eprintln!("skipping: RBR_RSF not set");
            return;
        };
        let Some(mods) = crate::command::CodriverMods::scan(&rsf) else {
            eprintln!("skipping: no Pacenote plugin in RBR_RSF");
            return;
        };
        assert!(!mods.styles.is_empty() && !mods.ranges.is_empty());
        let (styles_differ, ranges_differ) =
            crate::codriver_packs::call_sets_differ(&mods.dir, &mods.styles, &mods.ranges);
        assert!(!styles_differ, "every top-level style points at the same packages");
        assert!(!ranges_differ, "every range set points at the same packages");
        let Some(voice) = mods.voice(None) else {
            eprintln!("skipping: the install names no voice");
            return;
        };
        let preview = rbr_sound::preview::custom_preview(&mods.dir, &voice, &mods.styles[0], &mods.ranges[0]).unwrap();
        assert_eq!(preview.calls.len(), rbr_sound::preview::LADDER.len() + 1, "every corner and the distance");
        assert!(preview.wave.samples.len() > preview.calls.len() * preview.wave.rate as usize / 10);
    }
}
