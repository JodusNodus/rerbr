//! The window's frame: the look, the tabs, the status line and the smaller pages.

use std::path::Path;
use std::time::Duration;

use egui::{Color32, Id, RichText};
use rbr_sound::menu::MenuPlayer;

use crate::car_browser;
use crate::catalog::{self, car_label};

use crate::Form;

/// How often a running viewer is checked for its exit.
const POLL: Duration = Duration::from_millis(250);

/// The launcher's pages, by what the player wants to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// The quick race: car, stage, conditions, the race options and Start.
    Drive,
    Championship,
    /// The rally school's lessons, each on its map in the Drive page's car.
    School,
    /// The car setup.
    Setup,
    Replays,
    Controls,
    /// The window, the camera, the HUD, the pace notes, the sound and the renderer's additions.
    Graphics,
    /// The install folder, the advanced paths and the programs.
    Settings,
}

impl Page {
    /// The tabs, in order.
    const TABS: [(Page, &'static str); 8] = [
        (Page::Drive, "Drive"),
        (Page::Championship, "Championship"),
        (Page::School, "School"),
        (Page::Setup, "Car setup"),
        (Page::Replays, "Replays"),
        (Page::Controls, "Controls"),
        (Page::Graphics, "Graphics & sound"),
        (Page::Settings, "Settings"),
    ];

    /// The page a shortcut opens: Ctrl+1 to Ctrl+8 (Cmd on macOS) a tab, Ctrl+Tab or Ctrl+Page Down the next,
    /// Ctrl+Shift+Tab or Ctrl+Page Up the one before.
    fn shortcut(self, input: &mut egui::InputState) -> Option<Page> {
        use egui::{Key, Modifiers};
        const NUMBERS: [Key; 8] =
            [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8];
        let at = Self::TABS.iter().position(|(p, _)| *p == self).unwrap_or(0);
        let by = |n: usize| Self::TABS[(at + n) % Self::TABS.len()].0;
        let shift_ctrl = Modifiers::CTRL | Modifiers::SHIFT;
        if input.consume_key(shift_ctrl, Key::Tab) || input.consume_key(Modifiers::CTRL, Key::PageUp) {
            return Some(by(Self::TABS.len() - 1));
        }
        if input.consume_key(Modifiers::CTRL, Key::Tab) || input.consume_key(Modifiers::CTRL, Key::PageDown) {
            return Some(by(1));
        }
        NUMBERS
            .iter()
            .zip(Self::TABS)
            .find(|(key, _)| input.consume_key(Modifiers::COMMAND, **key))
            .map(|(_, (p, _))| p)
    }
}

/// The width of the Drive and Graphics & sound pages' field column, and of a field.
pub const DRIVE_FIELDS: f32 = 520.0;
pub const FIELD: f32 = 340.0;
/// The width of the fields' labels, the same on every section.
const LABEL: f32 = 130.0;
/// A dropdown choice: every value offered, its settings word and its label; parsing searches the offered words.
/// (Switch and Present parse case-insensitively, and Rate's word is not a static table, so those three keep
/// their own parsing and use [`combo`] with an explicit label.)
pub trait Choice: Copy + PartialEq + 'static {
    /// The values offered, in the dropdown's order.
    const ALL: &[Self];
    /// The settings file's word (and the viewer argument's, where the viewer takes it).
    fn name(self) -> &'static str;
    /// The dropdown's word.
    fn label(self) -> &'static str;
    /// The settings word's value; `None` for anything else (unknown text keeps the default).
    fn parse(text: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|choice| choice.name() == text)
    }
}

/// A dropdown of `options` with `label` words, setting `value` ([`Choice::label`] for a [`Choice`]); its button's
/// response, for hover text.
pub fn combo<T: Clone + PartialEq, S: Into<String>>(
    ui: &mut egui::Ui,
    id: &str,
    width: impl Into<Option<f32>>,
    value: &mut T,
    options: &[T],
    label: impl Fn(T) -> S,
) -> egui::Response {
    let selected: String = label(value.clone()).into();
    let mut dropdown = egui::ComboBox::from_id_salt(id).selected_text(selected);
    if let Some(width) = width.into() {
        dropdown = dropdown.width(width);
    }
    dropdown
        .show_ui(ui, |ui| {
            for option in options {
                let text: String = label(option.clone()).into();
                ui.selectable_value(value, option.clone(), text);
            }
        })
        .response
}

/// What needs attention, what is fine, and the Start button.
pub const WARN: Color32 = Color32::from_rgb(230, 180, 60);
pub const GOOD: Color32 = Color32::from_rgb(110, 190, 110);
pub const ACCENT: Color32 = Color32::from_rgb(40, 120, 70);

/// The launcher's look: readable sizes and roomier spacing than egui's defaults.
pub(crate) fn theme(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        use egui::{FontId, TextStyle};
        for (text, size) in [
            (TextStyle::Body, 14.5),
            (TextStyle::Button, 14.5),
            (TextStyle::Monospace, 13.5),
            (TextStyle::Small, 11.5),
            (TextStyle::Heading, 20.0),
        ] {
            let family =
                if text == TextStyle::Monospace { egui::FontFamily::Monospace } else { egui::FontFamily::Proportional };
            style.text_styles.insert(text, FontId::new(size, family));
        }
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.spacing.interact_size.y = 24.0;
    });
}

/// The page's main action (Start, Drive lesson, Drive SS1): large, white on green.
pub fn start_button(label: &str) -> egui::Button<'static> {
    let text = RichText::new(label).size(20.0).strong().color(Color32::WHITE);
    egui::Button::new(text).fill(ACCENT).min_size(egui::vec2(180.0, 44.0))
}

/// A section's heading.
pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).size(16.5).strong());
    ui.add_space(2.0);
}

/// A section's fields: a label and its field a row, the labels lined up across sections.
pub fn fields<R>(ui: &mut egui::Ui, id: &str, rows: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Grid::new(id).num_columns(2).spacing([16.0, 8.0]).min_col_width(LABEL).show(ui, rows).inner
}

/// Why Start cannot run, and what fixes it.
#[derive(Clone, PartialEq)]
pub struct Issue {
    pub text: String,
    pub fix: Option<Fix>,
}

/// A status line's fix button.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fix {
    /// The Settings page, for the game folder, the paths and the cars.
    Settings,
    /// The Settings page with NGP 7.4.778's download asked (`Form::ask_download_ngp`).
    DownloadNgp,
    /// The Settings page with gvrc.de's car list open.
    DownloadCars,
}

impl Fix {
    pub fn label(self) -> &'static str {
        match self {
            Fix::Settings => "Open Settings",
            Fix::DownloadNgp => "Download NGP 7.4.778",
            Fix::DownloadCars => "Download cars…",
        }
    }
}

/// What the launcher says without NGP's archive.
pub const NO_NGP: &str = "This game folder has no NGP; ReRBR needs NGP 7.4.778.";
/// Why ReRBR needs NGP 7.4.778 and what its download changes, under the Settings page's NGP row.
pub const NGP_WHY: &str = "ReRBR's physics is verified against NGP 7.4.778 only. Its download keeps NGP.dll and \
                           physics.rbz in ReRBR's folder; nothing installed changes.";
/// What the launcher says without a car NGP drives.
pub const NO_CARS: &str = "No cars yet. Download cars from gvrc.de's NGP car packs on the Settings page, or \
                          choose a game folder with community cars.";
/// What the player sees when the viewer or `rbr-standalone` is missing; the paths go to the log
/// ([`Form::log_missing_programs`]).
const INCOMPLETE: &str = "ReRBR is incomplete: its driving window or physics program is missing. Reinstall ReRBR.";

impl Form {
    /// What the game lacks to drive: NGP's archive ([`NO_NGP`]), or a car NGP drives ([`NO_CARS`]).
    pub fn no_ngp(&self) -> Option<String> {
        if self.ngp_revision.is_none() {
            return Some(NO_NGP.to_owned());
        }
        self.catalog.cars.is_empty().then(|| NO_CARS.to_owned())
    }

    /// The viewer and `rbr-standalone`, when missing.
    fn missing_programs(&self) -> impl Iterator<Item = &Path> {
        [&self.paths.viewer, &self.paths.standalone].into_iter().map(|p| p.as_path()).filter(|p| !p.is_file())
    }

    /// Says on stderr which programs are missing and how a checkout builds them.
    pub fn log_missing_programs(&self) {
        for path in self.missing_programs() {
            eprintln!(
                "launcher: {} is missing: cargo build --release -p rbr-render -p rbr-standalone (or --viewer, \
                 --standalone)",
                path.display()
            );
        }
    }

    /// What blocks a stage's launch: a running viewer, what Start checks (`readiness`) and a car that cannot start.
    pub(crate) fn launch_blocker(&self) -> Option<Issue> {
        self.child
            .is_some()
            .then(|| Issue { text: "the viewer is running".into(), fix: None })
            .or_else(|| self.readiness())
            .or_else(|| {
                self.car().and_then(|car| {
                    car.unavailable().map(|reason| Issue { text: format!("{}: {reason}", car.name), fix: None })
                })
            })
    }

    /// What the install lacks, on every page's status line: a missing program, NGP's archive or DLL, or a car NGP
    /// drives; with the fix when there is one.
    pub fn readiness(&self) -> Option<Issue> {
        if self.missing_programs().next().is_some() {
            return Some(Issue { text: INCOMPLETE.into(), fix: None });
        }
        // NGP's archive or DLL missing: its download when the game folder takes one.
        let fix = Some(if self.can_download_ngp() { Fix::DownloadNgp } else { Fix::Settings });
        if self.ngp_revision.is_none() {
            return Some(Issue { text: NO_NGP.into(), fix });
        }
        if let Some(text) = self.ngp_dll.problem(&self.data, &self.game_root()) {
            return Some(Issue { text, fix });
        }
        self.catalog.cars.is_empty().then(|| Issue { text: NO_CARS.into(), fix: Some(Fix::DownloadCars) })
    }

    /// The page a fresh start opens on: Settings while something is missing, else Drive.
    pub fn first_page(&self) -> Page {
        if self.readiness().is_some() {
            Page::Settings
        } else {
            Page::Drive
        }
    }

    /// What blocks Start: a running viewer, [`Form::readiness`], a car that cannot start, or the selection.
    pub fn blocker(&self) -> Option<Issue> {
        if self.child.is_some() {
            return Some(Issue { text: "the viewer is running".into(), fix: None });
        }
        if let Some(issue) = self.readiness() {
            return Some(issue);
        }
        let car = self.car()?;
        if let Some(reason) = car.unavailable() {
            return Some(Issue { text: format!("{}: {reason}", car.name), fix: None });
        }
        self.selection().err().map(|text| Issue { text, fix: None })
    }

    pub fn apply(&mut self, fix: Fix) {
        match fix {
            Fix::Settings => self.page = Page::Settings,
            Fix::DownloadNgp => {
                self.page = Page::Settings;
                self.ask_download_ngp();
            }
            Fix::DownloadCars => {
                self.page = Page::Settings;
                self.open_car_list();
            }
        }
    }

    /// A game with no cars to drive (just set up from the disc, say) opens on the Settings page's car list.
    pub fn offer_cars(&mut self) {
        if self.readiness().is_some_and(|issue| issue.fix == Some(Fix::DownloadCars)) {
            self.apply(Fix::DownloadCars);
        }
    }

    /// The menus' sound for the page shown: the service park at a championship's service stop, while the viewer is
    /// not running.
    pub fn menu_sound(&mut self) {
        let service = self.championship.in_service();
        let park = self.child.is_none() && service && matches!(self.page, Page::Championship | Page::Setup);
        if !park && self.menu_sound.is_none() {
            return;
        }
        let volume = self.volume;
        if let Some(player) = self.menu_player() {
            player.set_volume(volume);
            player.set_park(park);
        }
    }

    /// The menus' sound, started on first use; `None` without an audio device or data.
    pub fn menu_player(&mut self) -> Option<&MenuPlayer> {
        self.menu_sound
            .get_or_insert_with(|| {
                MenuPlayer::start(&self.data, self.volume).map_err(|e| eprintln!("launcher: no menu sound: {e}")).ok()
            })
            .as_ref()
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        theme(ctx);
        self.poll();
        let (bindings, ffb) = (self.bindings.clone(), self.ffb.clone());
        let running = self.child.is_some();
        self.controls.poll(&mut self.bindings, running);
        // The first start with a gamepad and nothing bound to it: the Controls page is shown, once the game
        // is ready to drive (Settings says what it lacks first).
        if !running && self.readiness().is_none() && self.controls.offer(&self.bindings) {
            self.page = Page::Controls;
        }
        // While the viewer runs the window wakes only for its exit and the gamepad events taken while it is driven
        // with one (`POLL`): the car turntable stands still and the Controls page's bars are not live.
        self.previews.still = running;
        if self.controls.capturing() {
            ctx.request_repaint();
        } else if running || self.controls.has_pads() {
            let live = self.page == Page::Controls && !running;
            ctx.request_repaint_after(if live { Duration::from_millis(30) } else { POLL });
        }
        let old_selection = (
            self.map,
            self.car,
            self.options,
            self.weather_row.clone(),
            self.wetness,
            self.fog,
            self.clouds,
            self.precipitation,
            self.wear,
            self.tyre,
        );
        let old_setup = self.setup.choices.clone();
        let old_ghost = self.ghost.clone();
        let old_graphics = (
            self.graphics,
            self.render_scale,
            self.upscale_textures,
            self.present,
            self.max_fps,
            self.fullscreen,
            self.hdr,
            self.perf_log,
            self.start_time,
        );
        let old_paths = self.ngp_physics.clone();
        let old_head_gasket = (self.head_gasket, self.physics, self.tyre_tuning);
        let old_car_picks = self.gearboxes.clone();
        let old_folder = self.install_folder.clone();
        let old_recent = (self.recent_cars.clone(), self.recent_maps.clone(), self.favourite_maps.clone());
        let old_codriver = (
            self.codriver,
            self.codriver_voice.clone(),
            self.codriver_style.clone(),
            self.codriver_ranges.clone(),
            self.codriver_pack.clone(),
            self.notes_pack.clone(),
            self.my_pacenotes,
            self.notes_set.clone(),
        );
        let old_notes = self.notes_inputs();
        let browsers_open = (self.stage_browser.open, self.car_browser.open);
        ctx.root_ui(|root| {
            egui::Panel::top("tabs").show(root, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("ReRBR").size(20.0).strong());
                    ui.add_space(20.0);
                    let mut open = ui.input_mut(|i| self.page.shortcut(i));
                    for (n, (page, name)) in Page::TABS.into_iter().enumerate() {
                        let tab = egui::Button::selectable(self.page == page, RichText::new(name).size(15.5));
                        let key = format!("{}+{}", if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" }, n + 1);
                        if ui.add(tab.min_size(egui::vec2(0.0, 28.0))).on_hover_text(key).clicked() {
                            open = Some(page);
                        }
                    }
                    if let Some(page) = open {
                        if page == Page::Replays {
                            self.replays.refresh(&self.here());
                        }
                        self.page = page;
                    }
                });
                ui.add_space(4.0);
            })
        });
        let readiness = self.readiness();
        ctx.root_ui(|root| {
            egui::Panel::bottom("status").show(root, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    match &readiness {
                        Some(issue) => {
                            ui.colored_label(WARN, format!("⚠ {}", issue.text));
                            // Not on the Settings page, which has the fixes in its rows.
                            if let Some(fix) = issue.fix.filter(|_| self.page != Page::Settings) {
                                if ui.button(fix.label()).clicked() {
                                    self.apply(fix);
                                }
                            }
                        }
                        None => {
                            let text = if self.status.is_empty() { "Ready" } else { &self.status };
                            ui.colored_label(Color32::GRAY, text);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), crate::update::link);
                });
                ui.add_space(2.0);
            })
        });
        // The browsers are a page's body: leaving the page closes them.
        self.stage_browser.open &= match self.stage_browser.target {
            car_browser::Target::Drive => self.page == Page::Drive,
            car_browser::Target::Championship => self.page == Page::Championship,
        };
        self.car_browser.open &= match self.car_browser.target {
            car_browser::Target::Drive => self.page == Page::Drive,
            car_browser::Target::Championship => self.page == Page::Championship,
        };
        ctx.root_ui(|root| {
            egui::CentralPanel::default().show(root, |ui| match self.page {
                Page::Drive => self.drive_page(ui),
                Page::Championship => self.championship_page(ui),
                Page::School => self.school_page(ui),
                Page::Replays => self.replays_page(ui),
                Page::Setup => self.setup_page(ui),
                Page::Controls => {
                    let root = self.game_root();
                    let settings = self.paths.settings.clone();
                    let (read_only, car) = (self.read_only, self.car);
                    self.controls.ui(
                        ui,
                        &mut self.bindings,
                        &mut self.ffb,
                        &mut self.options.aids,
                        &mut self.head_gasket,
                        &mut self.ffb_cars,
                        &self.ffb_load,
                        &self.catalog.cars,
                        car,
                        &root,
                        &settings,
                        read_only,
                    )
                }
                Page::Graphics => self.graphics_page(ui),
                Page::Settings => {
                    egui::ScrollArea::vertical().id_salt("settings").show(ui, |ui| self.settings_page(ui));
                }
            })
        });
        self.crash.ui(ctx);
        // A dropdown's item picked with Enter or Space: egui closes dropdowns only on a mouse click. The focus goes
        // back to the dropdown (the last control focused outside one).
        let outside = Id::unique("focused outside dropdowns");
        match ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)) {
            Some(r) if r.layer_id.order == egui::Order::Foreground => {
                if r.clicked() {
                    egui::Popup::close_all(ctx);
                    if let Some(id) = ctx.data(|d| d.get_temp::<Id>(outside)) {
                        ctx.memory_mut(|m| m.request_focus(id));
                    }
                }
            }
            Some(r) => _ = ctx.data_mut(|d| d.insert_temp(outside, r.id)),
            None => {}
        }
        // A page changed what the status line shows: drawn again with it.
        if self.readiness() != readiness {
            ctx.request_repaint();
        }
        if old_codriver.4 != self.codriver_pack {
            // Another voice pack: its voices, styles and range sets, with the choices aligned to what it
            // offers (before the save below, so the aligned choices persist).
            self.rescan_codrivers();
            self.normalize_codriver();
        }
        // The notes summary is cached: mark it dirty and repaint, or a switch keeps the old text until the map
        // changes.
        self.invalidate_notes_cache(ctx, old_notes);
        if (browsers_open.0 && !self.stage_browser.open)
            || (browsers_open.1 && !self.car_browser.open)
            || self.bindings != bindings
            || self.ffb != ffb
            || old_selection
                != (
                    self.map,
                    self.car,
                    self.options,
                    self.weather_row.clone(),
                    self.wetness,
                    self.fog,
                    self.clouds,
                    self.precipitation,
                    self.wear,
                    self.tyre,
                )
            || old_setup != self.setup.choices
            || old_ghost != self.ghost
            || old_paths != self.ngp_physics.clone()
            || old_head_gasket != (self.head_gasket, self.physics, self.tyre_tuning)
            || old_car_picks != self.gearboxes
            || old_graphics
                != (
                    self.graphics,
                    self.render_scale,
                    self.upscale_textures,
                    self.present,
                    self.max_fps,
                    self.fullscreen,
                    self.hdr,
                    self.perf_log,
                    self.start_time,
                )
            || old_folder != self.install_folder
            || old_recent != (self.recent_cars.clone(), self.recent_maps.clone(), self.favourite_maps.clone())
            || old_codriver
                != (
                    self.codriver,
                    self.codriver_voice.clone(),
                    self.codriver_style.clone(),
                    self.codriver_ranges.clone(),
                    self.codriver_pack.clone(),
                    self.notes_pack.clone(),
                    self.my_pacenotes,
                    self.notes_set.clone(),
                )
        {
            self.save();
        }
        if old_paths != self.ngp_physics {
            self.list_cars();
        }
    }

    /// The Drive page with the car browser open on `collection` (`--screenshot`).
    pub fn screenshot_cars(&mut self, ctx: &egui::Context, collection: car_browser::Collection) {
        self.stage_browser.open = false;
        if !self.car_browser.open {
            self.car_browser.open_on(car_browser::Target::Drive, self.car, &self.favourite_cars, Some(collection));
        }
        self.ui(ctx);
    }

    /// The Drive page with the stage browser open on `collection` (`--screenshot`).
    pub fn screenshot_stages(&mut self, ctx: &egui::Context, collection: crate::stage_browser::Collection) {
        self.car_browser.open = false;
        if !self.stage_browser.open {
            self.stage_browser.open_on(self.map, &self.favourite_maps, Some(collection));
        }
        self.ui(ctx);
    }

    /// A page that needs a car, without any: what to do. Whether there are none.
    pub(crate) fn no_cars_yet(&self, ui: &mut egui::Ui) -> bool {
        if !self.catalog.cars.is_empty() {
            return false;
        }
        ui.add_space(8.0);
        ui.colored_label(WARN, NO_CARS);
        true
    }

    /// The Car setup page (`setup_page`), for the selected car on the stage's surface.
    fn setup_page(&mut self, ui: &mut egui::Ui) {
        if self.no_cars_yet(ui) {
            return;
        }
        self.select_setup();
        let car = self.car().map_or_else(|| "no car".to_owned(), car_label);
        ui.label(
            RichText::new(format!("{car} · {}", catalog::map_label(&self.catalog.maps[self.map]))).size(16.0).strong(),
        );
        ui.colored_label(
            Color32::GRAY,
            "Make and save setups for this car (saved for this stage's surface); pick the one to drive with on the Drive page.",
        );
        ui.add_space(4.0);
        self.setup.ui(ui);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::graphics_page::WetRoads;
    use crate::settings::{load, Clouds, Fog, Precipitation, Settings};
    use crate::{Start, SETTINGS_FILE};
    use rbr_gameplay::race_options::{DamageLevel, Skill};
    use rbr_render::renderer::Detail;

    #[test]
    fn shortcuts_open_the_pages() {
        use egui::{Key, Modifiers};
        let press = |page: Page, key: Key, modifiers: Modifiers| {
            let event = egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
            let input =
                egui::RawInput { events: vec![egui::Event::ModifiersChanged(modifiers), event], ..Default::default() };
            let mut opened = None;
            let ctx = egui::Context::default();
            ctx.run_pass(input, |ctx| opened = ctx.input_mut(|i| page.shortcut(i))).textures_delta.clear();
            opened
        };
        let (ctrl, shift_ctrl) =
            (Modifiers::CTRL | Modifiers::COMMAND, Modifiers::CTRL | Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(press(Page::Drive, Key::Num3, ctrl), Some(Page::School));
        assert_eq!(press(Page::Drive, Key::Num4, ctrl), Some(Page::Setup));
        assert_eq!(press(Page::Drive, Key::Num8, ctrl), Some(Page::Settings));
        assert_eq!(press(Page::Drive, Key::Tab, ctrl), Some(Page::Championship));
        assert_eq!(press(Page::Settings, Key::PageDown, ctrl), Some(Page::Drive));
        assert_eq!(press(Page::Drive, Key::Tab, shift_ctrl), Some(Page::Settings));
        assert_eq!(press(Page::Replays, Key::PageUp, ctrl), Some(Page::Setup));
        assert_eq!(press(Page::Drive, Key::Num3, Modifiers::NONE), None);
        assert_eq!(press(Page::Drive, Key::Tab, Modifiers::NONE), None);
    }

    /// Every dropdown choice parses back from its settings word, with a label of its own (`settings_round_trip`
    /// pins the file's text; this pins each value's word and label).
    #[test]
    fn choices_round_trip() {
        fn round_trips<T: Choice + std::fmt::Debug>(_: T) {
            let mut labels: Vec<&str> = T::ALL
                .iter()
                .map(|choice| {
                    assert_eq!(T::parse(choice.name()), Some(*choice), "{choice:?}");
                    assert!(!choice.label().is_empty(), "{choice:?}");
                    choice.label()
                })
                .collect();
            labels.sort_unstable();
            labels.dedup();
            assert_eq!(labels.len(), T::ALL.len());
            assert_eq!(T::parse(""), None);
            assert_eq!(T::parse("something the settings never write"), None);
        }
        round_trips(Fog::Off);
        round_trips(Clouds::Clear);
        round_trips(Precipitation::None);
        round_trips(WetRoads::Off);
        round_trips(DamageLevel::Safe);
        round_trips(Skill::Rookie);
        round_trips(Detail::High);
        round_trips(crate::physics::PhysicsProfile::Ngp);
    }

    /// The first start opens on Settings while NGP, a car or a program is missing, with the fix for it, and on Drive
    /// once everything is there (`RBR_DATA`, `RBR_PATCH`, `RBR_NGP_PHYSICS`: the test archive drives retail slots).
    #[test]
    fn the_first_start_opens_on_what_needs_attention() {
        let env = |key| std::env::var(key).ok().filter(|v| !v.is_empty());
        let (Some(data), Some(patch), Some(ngp)) = (env("RBR_DATA"), env("RBR_PATCH"), env("RBR_NGP_PHYSICS")) else {
            eprintln!("skipping: RBR_DATA, RBR_PATCH or RBR_NGP_PHYSICS not set");
            return;
        };
        let here = crate::test_dir("pages-first-page");
        std::fs::create_dir_all(&here).unwrap();
        let program = std::env::current_exe().unwrap();
        let form = |archive: &str, programs: bool| {
            let start = Start {
                viewer: if programs { program.clone() } else { here.join("viewer") },
                standalone: program.clone(),
                settings: here.join(SETTINGS_FILE),
                data: Some(PathBuf::from(&data)),
                patch: Some(PathBuf::from(&patch)),
                physics: None,
            };
            // The community content comes from the game folder in use.
            let text = format!("[launcher]\nngp_physics = {archive}\n");
            load(&start, Settings::parse(&text)).unwrap()
        };
        let retail = format!("{data}/Root/physics.rbz");
        let page = |form: &Form| (form.first_page(), form.readiness().map(|i| (i.text, i.fix)));
        assert_eq!(page(&form(&ngp, true)), (Page::Drive, None));
        let no_ngp = (Page::Settings, Some((NO_NGP.to_owned(), Some(Fix::Settings))));
        assert_eq!(page(&form(&retail, true)), no_ngp);
        assert_eq!(page(&form(&ngp, false)), (Page::Settings, Some((INCOMPLETE.to_owned(), None))));
        let mut no_cars = form(&ngp, true);
        no_cars.catalog.cars.clear();
        assert_eq!(page(&no_cars), (Page::Settings, Some((NO_CARS.to_owned(), Some(Fix::DownloadCars)))));
        no_cars.page = Page::Drive;
        no_cars.offer_cars();
        assert_eq!(no_cars.page, Page::Settings);
        std::fs::remove_dir_all(&here).unwrap();
    }
}
