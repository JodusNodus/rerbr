//! The player's saved drives: browse headers, watch a drive, or export it through the viewer.

use std::fs::{self, File};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use egui::RichText;
use rbr_recording::{realtime, replay, splits::Splits};

use crate::settings::{Clouds, Precipitation};

use crate::browser;
use crate::kit;
use crate::pages::{combo, Choice, WARN};
use crate::theme::{self, TEXT, TEXT_2, TEXT_3, TEXT_LABEL, TEXT_LIST};
use crate::Form;

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    modified: SystemTime,
    title: String,
    conditions: String,
    start_time: Option<String>,
    result: Option<Splits>,
    problem: Option<String>,
}

impl Entry {
    /// Whether the search `query` (lowercase) is in its title, file name or conditions.
    fn matches(&self, query: &str) -> bool {
        [self.title.to_lowercase(), self.path.to_string_lossy().to_lowercase(), self.conditions.to_lowercase()]
            .iter()
            .any(|text| text.contains(query))
    }

    fn read(path: PathBuf) -> Self {
        let modified =
            recorded_time(&path).unwrap_or_else(|| path.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH));
        // The header's title and conditions are kept when the frames are missing: an aborted drive is still named.
        let mut args = None;
        let problem = (|| {
            let mut input = BufReader::new(File::open(&path).map_err(|e| e.to_string())?);
            let args = args.insert(replay::read_args(&mut input)?);
            let mut magic = [0; 4];
            input.read_exact(&mut magic).map_err(|_| "No drive frames saved yet")?;
            if magic != realtime::LOG_MAGIC {
                return Err("Invalid replay frame log".into());
            }
            let mut first = [0; realtime::LOGGED_BYTES];
            input.read_exact(&mut first).map_err(|_| "No complete drive frames saved yet")?;
            if !args.iter().any(|arg| arg == "--live") {
                return Err("Missing the saved drive command".into());
            }
            Ok::<_, String>(())
        })()
        .err();
        let fallback = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let title = args.as_ref().and_then(|args| {
            let viewer = &args[..args.iter().position(|a| a == "--live").unwrap_or(args.len())];
            viewer.windows(2).find(|w| w[0] == "--title").map(|w| w[1].clone())
        });
        let (conditions, start_time) = args.as_deref().map(conditions).unwrap_or_default();
        let result = problem
            .is_none()
            .then(|| replay::read_result(&path).ok())
            .flatten()
            .filter(|r| r.race_time.is_finite() && r.race_time >= 0.0);
        Self { path, modified, title: title.unwrap_or(fallback), conditions, start_time, result, problem }
    }

    /// Whether the drive reached the finish line (not retired, not stopped early).
    fn finished(&self) -> bool {
        self.result.as_ref().is_some_and(|r| r.finished && !r.retired)
    }
}

pub(super) struct Job {
    video: Option<PathBuf>,
    log: PathBuf,
}

/// Cached directory contents and the page's current selection. Re-read on entering, Refresh and child exit.
pub struct Replays {
    entries: Vec<Entry>,
    selected: Option<PathBuf>,
    directory: Option<PathBuf>,
    query: String,
    finished_only: bool,
    /// The Delete button was pressed and waits for Yes or No.
    confirm_delete: bool,
    output: String,
    size: (u32, u32),
    fps: u32,
    view: &'static str,
    /// The list's scroll offset and visible height as last drawn.
    scroll: (f32, f32),
    message: String,
    pub(super) job: Option<Job>,
}

impl Default for Replays {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            selected: None,
            directory: None,
            query: String::new(),
            finished_only: false,
            confirm_delete: false,
            output: String::new(),
            size: (1920, 1080),
            fps: 60,
            view: "cockpit",
            scroll: (0.0, 400.0),
            message: String::new(),
            job: None,
        }
    }
}

impl Replays {
    pub fn refresh(&mut self, home: &Path) {
        let directory = home.join("replays");
        self.directory = Some(directory.clone());
        self.entries = match fs::read_dir(&directory) {
            Ok(files) => files
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "rrp"))
                .map(Entry::read)
                .collect(),
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    self.message = format!("Cannot read saved drives: {e}");
                }
                Vec::new()
            }
        };
        self.entries.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| b.path.cmp(&a.path)));
        if !self.entries.iter().any(|e| Some(&e.path) == self.selected.as_ref()) {
            self.select(self.entries.first().map(|e| e.path.clone()), home);
        }
    }

    fn select(&mut self, path: Option<PathBuf>, home: &Path) {
        let title = path.as_ref().and_then(|p| self.entries.iter().find(|e| &e.path == p)).map(|e| e.title.as_str());
        self.output = title.map(|t| video_path(&home.join("videos"), t).display().to_string()).unwrap_or_default();
        self.selected = path;
        self.confirm_delete = false;
    }

    /// Deletes the replay with its result and setup files, then selects `next`.
    fn delete(&mut self, path: &Path, next: Option<PathBuf>, home: &Path) {
        match delete_files(path) {
            Ok(()) => {
                self.entries.retain(|e| e.path != path);
                self.message.clear();
                self.select(next, home);
            }
            Err(e) => self.message = format!("Cannot delete the drive: {e}"),
        }
        self.confirm_delete = false;
    }

    /// Called by the common child poll instead of advancing a championship for a replay's exit.
    pub fn finished(&mut self, success: bool) {
        if let Some(job) = self.job.take() {
            self.message = match (success, job.video) {
                (true, Some(path)) => format!("Video saved to {}", path.display()),
                (true, None) => "Replay closed".into(),
                (false, _) => format!(
                    "Replay failed. {}\nLog: {}",
                    log_tail(&job.log).lines().last().unwrap_or("See the log for details."),
                    job.log.display()
                ),
            };
        }
    }
}

/// Removes a replay and the files kept beside it (its result and setup); exported videos are not touched.
fn delete_files(path: &Path) -> std::io::Result<()> {
    for file in [path.to_path_buf(), replay::result_path(path), replay::setup_path(path)] {
        match fs::remove_file(file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

/// A new video's file in `dir` named after the drive's `title` (`Torsby 2018 - Lancer Evo II GrpA.mp4`), numbered
/// when that name is taken.
fn video_path(dir: &Path, title: &str) -> PathBuf {
    let name: String = title
        .replace(" · ", " - ")
        .chars()
        .map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
        .collect();
    let name = name.trim().trim_end_matches('.');
    (1..)
        .map(|n| dir.join(if n == 1 { format!("{name}.mp4") } else { format!("{name} ({n}).mp4") }))
        .find(|p| !p.exists())
        .unwrap_or_default()
}

fn log_tail(path: &Path) -> String {
    let Ok(mut file) = File::open(path) else { return String::new() };
    let len = file.metadata().map_or(0, |m| m.len());
    let _ = file.seek(SeekFrom::Start(len.saturating_sub(4096)));
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

fn age(modified: SystemTime) -> String {
    let seconds = modified.elapsed().unwrap_or_default().as_secs();
    match seconds {
        0..60 => "Just now".into(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86400 => format!("{} h ago", seconds / 3600),
        _ => format!("{} days ago", seconds / 86400),
    }
}

/// Launcher replay names use epoch nanoseconds, or seconds for older files. Copying a drive preserves this date.
fn recorded_time(path: &Path) -> Option<SystemTime> {
    let name = path.file_stem()?.to_str()?;
    let stamp = name.split_once('-')?.0.parse::<u64>().ok()?;
    UNIX_EPOCH.checked_add(if stamp >= 1_000_000_000_000 {
        Duration::from_nanos(stamp)
    } else {
        Duration::from_secs(stamp)
    })
}

fn result_text(result: Option<&Splits>) -> String {
    let Some(result) = result else { return "No time".into() };
    let time = rbr_render::hud::clock_label(f64::from(result.race_time));
    if result.retired {
        format!("DNF at {time}")
    } else if result.finished {
        time
    } else {
        format!("Stopped at {time}")
    }
}

/// Conditions come from the saved command, with the viewer's overrides kept apart from the physics options.
fn conditions(args: &[String]) -> (String, Option<String>) {
    let at = args.iter().position(|a| a == "--live").unwrap_or(args.len());
    let viewer = &args[..at];
    let command = args.get(at + 1..).unwrap_or_default();
    let value =
        |args: &[String], flag: &str| args.windows(2).rev().find(|pair| pair[0] == flag).map(|pair| pair[1].clone());
    let mut parts = Vec::new();
    if let Some(road) = value(viewer, "--weather").or_else(|| value(command, "--wetness")) {
        parts.push(match road.as_str() {
            "dry" => "Dry road".into(),
            "damp" => "Damp road".into(),
            "wet" => "Wet road".into(),
            _ => road,
        });
    }
    if let Some(clouds) = value(viewer, "--clouds").and_then(|v| Clouds::parse(&v)) {
        parts.push(clouds.label().into());
    }
    if let Some(rain) = value(viewer, "--precipitation").and_then(|v| Precipitation::parse(&v)) {
        parts.push(if rain == Precipitation::None {
            "No precipitation".into()
        } else {
            format!("{} precipitation", rain.label())
        });
    }
    if let Some(fog) = value(viewer, "--fog").filter(|v| v != "off") {
        parts.push(format!("{fog} fog"));
    }
    let start = value(viewer, "--clock").or_else(|| value(command, "--start-time"));
    (if parts.is_empty() { "Stage defaults".into() } else { parts.join(" · ") }, start)
}

/// Viewer options precede its saved live command, so replay expansion can override presentation settings.
fn viewer_args(path: &Path, view: &str, video: Option<(&Path, (u32, u32), u32)>) -> Vec<String> {
    let mut args = vec!["--replay".into(), path.display().to_string(), "--view".into(), view.into()];
    if let Some((out, (width, height), fps)) = video {
        args.extend([
            "--video".into(),
            out.display().to_string(),
            "--size".into(),
            format!("{width}x{height}"),
            "--video-fps".into(),
            fps.to_string(),
        ]);
    }
    args
}

/// A replay row's height: the stage and its time, the car with the conditions and how long ago.
const ROW: f32 = 56.0;

/// The stage and the car of a drive's title (`Stage · Car`); the whole title when it has no car.
fn split_title(title: &str) -> (&str, &str) {
    title.split_once(" · ").unwrap_or((title, ""))
}

/// A camera's name for the dropdown.
fn camera_label(view: &str) -> String {
    let mut chars = view.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// How a drive's stage time is coloured: dimmed unless the drive finished.
fn time_colour(entry: &Entry) -> egui::Color32 {
    if entry.finished() {
        TEXT
    } else {
        TEXT_3
    }
}

/// The penalties line of a drive.
fn penalties_text(result: Option<&Splits>) -> String {
    match result.filter(|r| r.penalties > 0.0 && r.penalties.is_finite()) {
        Some(result) => format!("{} included", rbr_render::hud::clock_label(f64::from(result.penalties))),
        None => "None".into(),
    }
}

/// `text` with its top left at (`x`, `top`), clipped before `right`.
fn clipped_text(p: &egui::Painter, x: f32, top: f32, right: f32, text: &str, font: egui::FontId, color: egui::Color32) {
    let galley = p.layout_no_wrap(text.to_owned(), font, color);
    let room = egui::Rect::from_min_max(egui::pos2(x, top - 2.0), egui::pos2(right.max(x), top + 20.0));
    p.with_clip_rect(room).galley(egui::pos2(x, top), galley, color);
}

/// A drive's row: its stage and time, its car with the conditions and how long ago; the highlighted one filled.
fn draw_entry(ui: &egui::Ui, rect: egui::Rect, entry: &Entry, selected: bool, hovered: bool) {
    browser::paint_selection(ui, rect, selected, hovered);
    let p = ui.painter();
    let (stage, car) = split_title(&entry.title);
    let x = rect.left() + 14.0;
    let right = rect.right() - 14.0;
    let top = rect.top() + 11.0;
    let time = result_text(entry.result.as_ref());
    let time = p.layout_no_wrap(time, theme::mono(12.0), time_colour(entry));
    let age = p.layout_no_wrap(age(entry.modified), egui::FontId::proportional(12.0), TEXT_3);
    let (time_w, age_w) = (time.size().x, age.size().x);
    clipped_text(p, x, top, right - time_w - 12.0, stage, egui::FontId::proportional(13.0), TEXT_LIST);
    p.galley(egui::pos2(right - time_w, top), time, TEXT);
    let second = match &entry.problem {
        Some(problem) => (problem.as_str(), theme::WARN),
        None => (car, TEXT_2),
    };
    let line = if entry.problem.is_some() || entry.conditions.is_empty() {
        second.0.to_owned()
    } else if car.is_empty() {
        entry.conditions.clone()
    } else {
        format!("{car} · {}", entry.conditions)
    };
    clipped_text(p, x, top + 19.0, right - age_w - 12.0, &line, egui::FontId::proportional(12.0), second.1);
    p.galley(egui::pos2(right - age_w, top + 19.0), age, TEXT_3);
}

impl Form {
    pub fn replays_page(&mut self, ui: &mut egui::Ui) {
        let home = self.here();
        if self.replays.directory.as_ref() != Some(&home.join("replays")) {
            self.replays.refresh(&home);
        }
        let count = self.replays.entries.len();
        let subtitle = if count == 1 { "1 saved drive".to_owned() } else { format!("{count} saved drives") };
        kit::page_header(ui, "Replays", Some(&subtitle), |_| {});
        if self.replays.entries.is_empty() {
            kit::body(ui, |ui| {
                ui.label("Your drives are saved here automatically after you start driving.");
                ui.label(RichText::new(home.join("replays").display().to_string()).color(TEXT_2));
                ui.label(&self.replays.message);
            });
            return;
        }
        browser::bar(ui, |ui| {
            ui.horizontal(|ui| {
                browser::search_box(ui, "Search stage, car or conditions", &mut self.replays.query, &mut false, 280.0);
                ui.add_space(8.0);
                kit::switch(ui, &mut self.replays.finished_only);
                ui.label(RichText::new("Finished drives only").color(TEXT_LABEL));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Refresh").clicked() {
                        self.replays.refresh(&home);
                    }
                });
            });
        });
        // The arrow and page keys move through the list, Enter watches.
        let query = self.replays.query.to_lowercase();
        let entries = &self.replays.entries;
        let shown: Vec<usize> = (0..entries.len())
            .filter(|&k| entries[k].matches(&query) && (!self.replays.finished_only || entries[k].finished()))
            .collect();
        let keys = browser::Keys::take(ui);
        let at = shown.iter().position(|&k| Some(&entries[k].path) == self.replays.selected.as_ref());
        let moved_to = (keys.movement() != 0 && !shown.is_empty()).then(|| {
            let last = shown.len() as isize - 1;
            let to = at.map_or(0, |a| (a as isize + keys.movement()).clamp(0, last) as usize);
            (to, entries[shown[to]].path.clone())
        });
        if let Some((_, path)) = &moved_to {
            self.replays.select(Some(path.clone()), &home);
        }
        // Delete asks first; it is left to the search box while that has the focus.
        let delete_key = ui.memory(|m| m.focused().is_none())
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete));
        let ready = self.child.is_none() && self.readiness().is_none();
        let (mut delete, mut watch, mut export) = (false, false, false);
        let selected = self.replays.entries.iter().find(|e| Some(&e.path) == self.replays.selected.as_ref()).cloned();
        browser::inspector(
            ui,
            |ui| {
                if let Some(entry) = &selected {
                    (watch, export, delete) =
                        self.replay_inspector(ui, entry, ready && entry.problem.is_none(), (&keys, delete_key));
                }
            },
            |_| {},
        );
        let mut clicked = None;
        let shown_text = format!("{} shown", shown.len());
        let hints = [("↑↓", "Move"), ("↵", "Watch"), ("Del", "Delete")];
        browser::hint_bar(ui, &shown_text, &hints, |ui| {
            if shown.is_empty() {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label(RichText::new("No saved drives match this search.").color(TEXT_2));
                });
            }
            let (offset, visible) = self.replays.scroll;
            let mut area = egui::ScrollArea::vertical().id_salt("replays-list").auto_shrink([false, false]);
            if let Some((to, _)) = &moved_to {
                let top = *to as f32 * ROW;
                let want = if top < offset {
                    top
                } else if top + ROW > offset + visible {
                    top + ROW - visible
                } else {
                    offset
                };
                area = area.vertical_scroll_offset(want);
            }
            let output = area.show_rows(ui, ROW, shown.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for &k in &shown[range] {
                    let entry = &self.replays.entries[k];
                    let (line, response) =
                        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), egui::Sense::CLICK);
                    let rect = line.shrink2(egui::vec2(12.0, 1.0));
                    let selected = self.replays.selected.as_ref() == Some(&entry.path);
                    draw_entry(ui, rect, entry, selected, response.hovered());
                    // The file's name on hover; not a Tab stop: the arrows move through the list.
                    let response = response.on_hover_text(entry.path.file_name().unwrap_or_default().to_string_lossy());
                    if response.clicked() {
                        clicked = Some(entry.path.clone());
                    }
                }
            });
            self.replays.scroll = (output.state.clamped_offset().y, output.inner_rect.height());
        });
        if let Some(path) = clicked {
            self.replays.select(Some(path), &home);
            ui.ctx().request_repaint();
        }
        if delete {
            let next = at.and_then(|a| shown.get(a + 1).or(a.checked_sub(1).and_then(|b| shown.get(b))));
            let next = next.map(|&k| self.replays.entries[k].path.clone());
            if let Some(path) = self.replays.selected.clone() {
                self.replays.delete(&path, next, &home);
            }
        }
        if watch || export {
            self.start_replay(export);
        }
    }

    /// The highlighted drive: its stage and car, its facts, Watch and Delete, and the export to video with its progress
    /// and result; whether to watch, export and delete now.
    fn replay_inspector(
        &mut self,
        ui: &mut egui::Ui,
        entry: &Entry,
        ready: bool,
        (keys, delete_key): (&browser::Keys, bool),
    ) -> (bool, bool, bool) {
        let (mut watch, mut export, mut delete) = (false, false, false);
        let (stage, car) = split_title(&entry.title);
        ui.add(egui::Label::new(RichText::new(stage).size(17.0).family(theme::semibold()).color(TEXT)).wrap())
            .on_hover_text(entry.path.file_name().unwrap_or_default().to_string_lossy());
        if !car.is_empty() {
            ui.add_space(2.0);
            ui.add(egui::Label::new(RichText::new(car).color(TEXT_2)).wrap());
        }
        if let Some(problem) = &entry.problem {
            ui.add_space(4.0);
            ui.colored_label(WARN, problem);
        }
        ui.add_space(14.0);
        kit::panel(ui, |rows| {
            rows.row("Stage time", |ui| {
                ui.label(
                    RichText::new(result_text(entry.result.as_ref())).font(theme::mono(12.0)).color(time_colour(entry)),
                );
            });
            rows.row("Conditions", |ui| {
                ui.add(egui::Label::new(&entry.conditions).truncate()).on_hover_text(&entry.conditions);
            });
            rows.row("Recorded", |ui| {
                let start =
                    entry.start_time.as_ref().map_or("stage default start".to_owned(), |t| format!("{t} start"));
                ui.label(format!("{} · {start}", age(entry.modified)));
            });
            rows.row("Penalties", |ui| _ = ui.label(penalties_text(entry.result.as_ref())));
            rows.row("Camera", |ui| {
                let views = ["cockpit", "bonnet", "bumper", "chase"];
                combo(ui, "replay-view", None, &mut self.replays.view, &views, camera_label);
            });
        });
        ui.add_space(12.0);
        let width = ui.available_width();
        ui.allocate_ui_with_layout(egui::vec2(width, 38.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if self.replays.confirm_delete {
                delete = ui.button("Yes, delete").clicked() || delete_key;
                if ui.button("No").clicked() || keys.escape {
                    self.replays.confirm_delete = false;
                }
                ui.label(RichText::new("Delete this drive?").color(TEXT_LABEL));
            } else if ui.button("Delete").on_hover_text("Delete key").clicked() || delete_key {
                self.replays.confirm_delete = true;
            }
            // Watch stays shown, and Enter works, while the deletion waits for its confirmation.
            let tip = "Enter. Replays use the game files, profile and setup from when you drove.";
            watch = browser::drive_button(ui, "Watch replay", ready, tip).clicked() || (keys.enter && ready);
        });
        ui.add_space(4.0);
        kit::hint(ui, "Space pauses · R restarts · Esc closes");
        ui.add_space(18.0);
        kit::group_label(ui, "Export to video", None);
        kit::panel(ui, |rows| {
            rows.row("Quality", |ui| {
                combo(ui, "replay-fps", None, &mut self.replays.fps, &[30, 60], |fps| format!("{fps} fps"));
                let sizes = [(1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)];
                combo(ui, "replay-size", None, &mut self.replays.size, &sizes, |(_, height)| format!("{height}p"));
            });
            rows.row("Save to", |ui| {
                let edit = egui::TextEdit::singleline(&mut self.replays.output).font(theme::mono(11.0));
                ui.add(edit.desired_width(ui.available_width()));
            });
        });
        let output = Path::new(self.replays.output.trim());
        let exists = output.exists();
        let mp4 = output.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let text = (ui.available_width() - 100.0).max(80.0);
            ui.allocate_ui(egui::vec2(text, 0.0), |ui| {
                ui.set_width(text);
                let note = "Includes game audio. Requires ffmpeg on your PATH.";
                ui.add(egui::Label::new(RichText::new(note).size(12.0).color(TEXT_3)).wrap());
            });
            export = ui.add_enabled(ready && !exists && mp4, egui::Button::new("Export MP4")).clicked();
        });
        if exists {
            ui.colored_label(WARN, "That file exists. Choose a new filename.");
        } else if !mp4 {
            kit::hint(ui, "Choose a filename ending in .mp4");
        }
        ui.add_space(12.0);
        if let Some(job) = &self.replays.job {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(if job.video.is_some() { "Exporting video…" } else { "Watching replay…" });
            });
            if job.video.is_some() {
                if let Some(progress) = log_tail(&job.log).lines().rev().find(|line| line.starts_with("video:")) {
                    ui.label(progress);
                }
            }
        }
        ui.label(&self.replays.message);
        (watch, export, delete)
    }

    fn start_replay(&mut self, export: bool) {
        let Some(path) = self.replays.selected.clone() else { return };
        let video = export.then(|| PathBuf::from(self.replays.output.trim()));
        let result = (|| {
            if let Some(video) = &video {
                if video.exists() {
                    return Err("Choose a new filename; that video already exists".into());
                }
                if let Some(parent) = video.parent().filter(|p| !p.as_os_str().is_empty()) {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
            }
            let logs = self.logs_dir();
            fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
            let log = logs.join(format!("replay-{stamp}.log"));
            let stderr = File::create(&log).map_err(|e| e.to_string())?;
            let args = viewer_args(
                &path,
                self.replays.view,
                video.as_deref().map(|p| (p, self.replays.size, self.replays.fps)),
            );
            let mut command = self.viewer_command(&args);
            command.stdout(Stdio::null()).stderr(stderr);
            let child = command.spawn().map_err(|e| format!("Cannot start replay: {e}"))?;
            Ok::<_, String>((child, log))
        })();
        match result {
            Ok((child, log)) => {
                let title = self.replays.entries.iter().find(|e| e.path == path).map_or("", |e| e.title.as_str());
                self.crash.watch(log.clone(), &format!("Replay of {title}"), true);
                self.child = Some(child);
                self.status = if export { "Exporting replay video" } else { "Watching replay" }.into();
                self.replays.message.clear();
                self.replays.job = Some(Job { video, log });
            }
            Err(e) => self.replays.message = e,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_conditions_distinguish_viewer_weather_from_physics_options() {
        let args = [
            "--weather",
            "wet",
            "--clouds",
            "partly",
            "--precipitation",
            "heavy",
            "--clock",
            "16:00",
            "--live",
            "standalone",
            "--weather",
            "0",
            "--start-time",
            "08:00",
        ]
        .map(str::to_owned);
        assert_eq!(conditions(&args), ("Wet road · Partly cloudy · Heavy precipitation".into(), Some("16:00".into())));
        assert_eq!(conditions(&[]), ("Stage defaults".into(), None));
        let args = ["--live", "standalone", "--wetness", "damp", "--start-time", "08:00"].map(str::to_owned);
        assert_eq!(conditions(&args), ("Damp road".into(), Some("08:00".into())));
    }

    #[test]
    fn stage_times_keep_the_finish_retirement_and_partial_drive_distinct() {
        let mut result = Splits { finished: true, race_time: 389.123, penalties: 20.0, ..Default::default() };
        assert_eq!(result_text(Some(&result)), "06:29.123");
        result.retired = true;
        assert_eq!(result_text(Some(&result)), "DNF at 06:29.123");
        result.finished = false;
        result.retired = false;
        assert_eq!(result_text(Some(&result)), "Stopped at 06:29.123");
        assert_eq!(result_text(None), "No time");
        assert_eq!(
            recorded_time(Path::new("1793268612-stage.rrp")),
            recorded_time(Path::new("1793268612000000000-stage.rrp"))
        );
        assert!(recorded_time(Path::new("my-drive.rrp")).is_none());
    }

    #[test]
    fn a_video_is_named_after_its_drive_and_numbered_when_taken() {
        let dir = crate::test_dir("replay-video-names");
        let path = video_path(&dir, "Torsby 2018 · Lancer Evo II GrpA");
        assert_eq!(path, dir.join("Torsby 2018 - Lancer Evo II GrpA.mp4"));
        fs::write(&path, b"").unwrap();
        assert_eq!(
            video_path(&dir, "Torsby 2018 · Lancer Evo II GrpA"),
            dir.join("Torsby 2018 - Lancer Evo II GrpA (2).mp4")
        );
        assert_eq!(video_path(&dir, "A/B: \"C\"?"), dir.join("A_B_ _C__.mp4"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn browsing_keeps_bad_files_visible_and_preserves_the_selection() {
        let home = crate::test_dir("replay-browser");
        let dir = home.join("replays");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("123-stage.rrp");
        let mut bytes =
            replay::header(&["track-54_E", "--title", "Stage · Car", "--live", "standalone"].map(str::to_owned));
        bytes.extend_from_slice(&realtime::LOG_MAGIC);
        bytes.extend_from_slice(&realtime::Logged::unchecked([0; rbr_recording::CONTROL_BYTES], 1).to_bytes());
        fs::write(&path, bytes).unwrap();
        fs::write(dir.join("broken.rrp"), b"broken").unwrap();
        let result = Splits { finished: true, race_time: 389.123, ..Default::default() };
        replay::save_result(&path, &result).unwrap();
        replay::save_result(&dir.join("broken.rrp"), &result).unwrap();
        fs::write(dir.join("unrelated.txt"), b"keep").unwrap();
        let mut browser = Replays::default();
        browser.refresh(&home);
        assert_eq!(browser.entries.len(), 2);
        let entry = browser.entries.iter().find(|e| e.path == path).unwrap();
        assert_eq!(entry.title, "Stage · Car");
        assert_eq!(entry.result, Some(result));
        assert!(browser.entries.iter().find(|e| e.problem.is_some()).unwrap().result.is_none());
        assert!(entry.problem.is_none());
        browser.select(Some(path.clone()), &home);
        browser.output = "my-edit.mp4".into();
        browser.refresh(&home);
        assert_eq!(browser.selected, Some(path));
        assert_eq!(browser.output, "my-edit.mp4");
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn deleting_a_drive_removes_its_files_only_and_selects_the_next() {
        let home = crate::test_dir("replay-delete");
        let dir = home.join("replays");
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(home.join("videos")).unwrap();
        fs::write(home.join("videos/a.mp4"), b"v").unwrap();
        for name in ["1-a.rrp", "1-a.rrp.splits", "1-a.rrp.setup", "2-b.rrp"] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        let mut browser = Replays::default();
        browser.refresh(&home);
        let (a, b) = (dir.join("1-a.rrp"), dir.join("2-b.rrp"));
        browser.delete(&a, Some(b.clone()), &home);
        assert_eq!(browser.selected, Some(b));
        assert_eq!(browser.entries.len(), 1);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        assert!(home.join("videos/a.mp4").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn an_aborted_drive_keeps_its_name_and_has_no_dangling_separator() {
        let home = crate::test_dir("replay-aborted");
        let dir = home.join("replays");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("1-track_1_M.rrp");
        fs::write(&path, replay::header(&["--title", "Lilaia · Car", "--live", "standalone"].map(str::to_owned)))
            .unwrap();
        let entry = Entry::read(path);
        assert_eq!(entry.title, "Lilaia · Car");
        assert!(entry.problem.is_some() && !entry.finished());
        assert_eq!(result_text(entry.result.as_ref()), "No time");
        assert_eq!(split_title(&entry.title), ("Lilaia", "Car"));
        assert_eq!(split_title("Only a stage"), ("Only a stage", ""));
        fs::remove_dir_all(home).unwrap();
    }
}
