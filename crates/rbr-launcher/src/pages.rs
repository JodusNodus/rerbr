//! The window's frame: the pages and their groups, the shell around them and the smaller pages.

use std::path::Path;
use std::time::Duration;

use egui::{Id, RichText};
use rbr_sound::menu::MenuPlayer;

use crate::car_browser;
use crate::catalog::{self, car_label};
use crate::get_page::GetMore;
use crate::kit;

use crate::Form;

/// How often a running viewer is checked for its exit.
const POLL: Duration = Duration::from_millis(250);

/// The sidebar's groups of pages: driving, the library of content, and the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Drive,
    Library,
    Settings,
}

impl Section {
    /// The groups, in order, with their names.
    pub const ALL: [(Section, &'static str); 3] =
        [(Section::Drive, "Drive"), (Section::Library, "Library"), (Section::Settings, "Settings")];

    /// The group's pages, in order, with their names (the Settings group's are its tabs).
    pub fn pages(self) -> &'static [(Page, &'static str)] {
        match self {
            Section::Drive => &[
                (Page::Drive, "Rally"),
                (Page::Circuit, "Circuit"),
                (Page::Championship, "Championship"),
                (Page::School, "School"),
                (Page::Replays, "Replays"),
            ],
            Section::Library => &[(Page::Cars, "Cars"), (Page::Stages, "Stages"), (Page::Codrivers, "Co-drivers")],
            Section::Settings => {
                &[(Page::Settings, "General"), (Page::Controls, "Controls"), (Page::Graphics, "Graphics & sound")]
            }
        }
    }

    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|(s, _)| *s == self).unwrap_or(0)
    }
}

/// The launcher's pages, by what the player wants to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// The quick race: car, stage, conditions, the race options and Start.
    Drive,
    /// A circuit's laps: the circuit, the car, the conditions and the session's length, with the best laps.
    Circuit,
    Championship,
    /// The rally school's lessons, each on its map in the Drive page's car.
    School,
    /// The car setup, opened from a page ([`Form::open_setup`]); in no group.
    Setup,
    Replays,
    /// Every car: its details and skins, the cars added and their removal.
    Cars,
    /// Every stage: its details, the stages added and their removal.
    Stages,
    /// The co-driver's voice and the pace notes, and their packs.
    Codrivers,
    /// Adding content: the recent downloads and Add (the Get more sites are modals on Cars, Stages and Co-drivers).
    Import,
    Controls,
    /// The window, the camera, the HUD, the pace notes, the sound and the renderer's additions.
    Graphics,
    /// The install folder, the physics, the telemetry and the advanced paths.
    Settings,
}

impl Page {
    /// The group the page is in; none for Import and Car setup.
    pub fn section(self) -> Option<Section> {
        Section::ALL.into_iter().map(|(s, _)| s).find(|s| s.pages().iter().any(|(p, _)| *p == self))
    }

    /// The page's name in the sidebar and in back links.
    pub fn name(self) -> &'static str {
        match self {
            Page::Import => "Import",
            Page::Setup => "Car setup",
            _ => Section::ALL
                .into_iter()
                .flat_map(|(s, _)| s.pages())
                .find(|(p, _)| *p == self)
                .map_or("", |(_, name)| name),
        }
    }

    /// The page a shortcut opens: Ctrl+1 to Ctrl+3 (Cmd on macOS) a group (`last`: the page each was left on),
    /// Ctrl+Tab or Ctrl+Page Down the group's next page, Ctrl+Shift+Tab or Ctrl+Page Up the one before (none from
    /// a page outside the groups).
    pub(crate) fn shortcut(self, input: &mut egui::InputState, last: &[Page; 3]) -> Option<Page> {
        use egui::{Key, Modifiers};
        const NUMBERS: [Key; 3] = [Key::Num1, Key::Num2, Key::Num3];
        let shift_ctrl = Modifiers::CTRL | Modifiers::SHIFT;
        let within = self.section().map(|s| {
            let pages = s.pages();
            let at = pages.iter().position(|(p, _)| *p == self).unwrap_or(0);
            (pages, at)
        });
        let back = input.consume_key(shift_ctrl, Key::Tab) || input.consume_key(Modifiers::CTRL, Key::PageUp);
        let next = !back
            && (input.consume_key(Modifiers::CTRL, Key::Tab) || input.consume_key(Modifiers::CTRL, Key::PageDown));
        if let Some((pages, at)) = within {
            if back {
                return Some(pages[(at + pages.len() - 1) % pages.len()].0);
            }
            if next {
                return Some(pages[(at + 1) % pages.len()].0);
            }
        }
        NUMBERS.iter().zip(last).find(|(key, _)| input.consume_key(Modifiers::COMMAND, **key)).map(|(_, p)| *p)
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

pub use crate::theme::{ACCENT, GOOD, WARN};

/// The page's main action (Start, Drive lesson, Drive SS1): [`kit::primary_button`].
pub fn start_button(label: &str) -> kit::PrimaryButton<'_> {
    kit::primary_button(label, None)
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
    /// The Cars page with the Get more cars dialog open on gvrc.de's list.
    DownloadCars,
    /// The Circuit page, for a circuit the Drive page has selected.
    Circuit,
}

impl Fix {
    pub fn label(self) -> &'static str {
        match self {
            Fix::Settings => "Open Settings",
            Fix::DownloadNgp => "Download NGP 7.4.778",
            Fix::DownloadCars => "Download cars…",
            Fix::Circuit => "Open Circuit",
        }
    }
}

/// What the launcher says without NGP's archive.
pub const NO_NGP: &str = "This game folder has no NGP; ReRBR needs NGP 7.4.778.";
/// Why ReRBR needs NGP 7.4.778 and what its download changes, under the Settings page's NGP row.
pub const NGP_WHY: &str = "ReRBR's physics is verified against NGP 7.4.778 only. Its download keeps NGP.dll and \
                           physics.rbz in ReRBR's folder; nothing installed changes.";
/// What the launcher says without a car NGP drives.
pub const NO_CARS: &str = "No cars yet. Download cars from gvrc.de's NGP car packs on Library > Cars, or \
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
        if self.page == Page::Drive && self.is_circuit(self.map) {
            return Some(Issue { text: "a circuit plays on the Circuit page".into(), fix: Some(Fix::Circuit) });
        }
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
                self.open_get_more(GetMore::Cars);
            }
            Fix::Circuit => self.page = Page::Circuit,
        }
    }

    /// A game with no cars to drive (just set up from the disc, say) opens the Cars page's gvrc.de list.
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
        crate::theme::apply(ctx);
        self.poll();
        let (bindings, ffb) = (self.bindings.clone(), self.ffb.clone());
        let running = self.child.is_some();
        self.controls.poll(&mut self.bindings, running);
        // The first start with a gamepad and nothing bound to it: the Controls page is shown, once the game
        // is ready to drive (Settings says what it lacks first).
        if !running && self.readiness().is_none() && self.controls.offer(&self.bindings) {
            self.page = Page::Controls;
        }
        // While the viewer runs the window wakes only for its exit (`POLL`): the car turntable stands still and the
        // Controls page's bars are not live.
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
        let old_telemetry = self.telemetry_udp.clone();
        let old_head_gasket = (self.head_gasket, self.physics);
        let old_car_picks = (self.gearboxes.clone(), self.skins.chosen.clone());
        let old_folder = self.install_folder.clone();
        let old_recent = (self.recent_cars.clone(), self.recent_maps.clone(), self.favourite_maps.clone());
        let old_codriver =
            (self.codriver, self.codriver_voice.clone(), self.codriver_style.clone(), self.codriver_pack.clone());
        let old_notes = self.notes_inputs();
        let browsers_open = (self.stage_browser.open, self.car_browser.open, self.circuit_browser.open);
        let old_circuit = (self.circuit.map, self.circuit.laps);
        let readiness = self.readiness();
        if let Some(page) = self.nav_ui(ctx) {
            if page == Page::Replays {
                self.replays.refresh(&self.here());
            }
            self.page = page;
        }
        // Over the page, on every page: the Add's progress and result.
        self.content_notice(ctx);
        // The browsers are a page's body: leaving the page closes them.
        self.stage_browser.open &= match self.stage_browser.target {
            car_browser::Target::Drive => self.page == Page::Drive,
            car_browser::Target::Championship => self.page == Page::Championship,
            car_browser::Target::Circuit => self.page == Page::Circuit,
        };
        self.circuit_browser.open &= self.page == Page::Circuit;
        self.car_browser.open &= match self.car_browser.target {
            car_browser::Target::Drive => self.page == Page::Drive,
            car_browser::Target::Championship => self.page == Page::Championship,
            car_browser::Target::Circuit => self.page == Page::Circuit,
        };
        ctx.root_ui(|root| {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(crate::theme::PAGE))
                .show(root, |ui| self.page_ui(ui))
        });
        self.get_more_modal(ctx);
        self.crash.ui(ctx);
        self.content_drop(ctx);
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
        if old_codriver.3 != self.codriver_pack {
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
            || (browsers_open.2 && !self.circuit_browser.open)
            || old_circuit != (self.circuit.map, self.circuit.laps)
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
            || old_telemetry != self.telemetry_udp
            || old_head_gasket != (self.head_gasket, self.physics)
            || old_car_picks != (self.gearboxes.clone(), self.skins.chosen.clone())
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
                != (self.codriver, self.codriver_voice.clone(), self.codriver_style.clone(), self.codriver_pack.clone())
        {
            self.save();
        }
        if old_paths != self.ngp_physics {
            self.list_cars();
        }
    }

    /// The page shown in the central area: the Settings group's header over its page, Import and the rest as they
    /// are (a restyled page draws its own `kit::page_header` and `kit::body`; the others are still wrapped here).
    fn page_ui(&mut self, ui: &mut egui::Ui) {
        if self.page == Page::Import {
            return self.import_page(ui);
        }
        // The library's pages draw their own header and padding (`browser`).
        match self.page {
            Page::Cars => return self.cars_page(ui),
            Page::Stages => return self.stages_page(ui),
            Page::Replays => return self.replays_page(ui),
            _ => {}
        }
        if self.page.section() == Some(Section::Settings) {
            if let Some(page) = self.settings_header(ui) {
                self.page = page;
            }
        } else {
            ui.add_space(14.0);
        }
        // The pages not yet restyled keep their fixed widths: narrower side padding than the kit's, except under the
        // Settings header, which lines up with it.
        let side =
            if self.page.section() == Some(Section::Settings) { (kit::PAGE_LEFT, kit::PAGE_RIGHT) } else { (14, 4) };
        let margin = egui::Margin { left: side.0, right: side.1, top: 0, bottom: 8 };
        egui::Frame::new().inner_margin(margin).show(ui, |ui| match self.page {
            Page::Drive => self.drive_page(ui),
            Page::Circuit => self.circuit_page(ui),
            Page::Championship => self.championship_page(ui),
            Page::School => self.school_page(ui),
            Page::Replays => {}
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
            Page::Cars | Page::Stages => {}
            Page::Codrivers => self.codrivers_page(ui),
            Page::Import => {}
            Page::Settings => {
                egui::ScrollArea::vertical().id_salt("settings").show(ui, |ui| self.settings_page(ui));
            }
        });
    }

    /// Opens the Car setup page, with a back link to the page it was opened from.
    pub(crate) fn open_setup(&mut self) {
        if self.page != Page::Setup {
            self.setup_from = self.page;
        }
        self.page = Page::Setup;
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
        if kit::link(ui, &format!("‹ {}", self.setup_from.name())).clicked() {
            self.page = self.setup_from;
        }
        ui.add_space(4.0);
        let car = self.car().map_or_else(|| "no car".to_owned(), car_label);
        ui.label(
            RichText::new(format!("{car} · {}", catalog::map_label(&self.catalog.maps[self.map]))).size(16.0).strong(),
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
            let last = [Page::School, Page::Stages, Page::Settings];
            ctx.run_pass(input, |ctx| opened = ctx.input_mut(|i| page.shortcut(i, &last))).textures_delta.clear();
            opened
        };
        let (ctrl, shift_ctrl) =
            (Modifiers::CTRL | Modifiers::COMMAND, Modifiers::CTRL | Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(press(Page::Drive, Key::Num1, ctrl), Some(Page::School));
        assert_eq!(press(Page::Drive, Key::Num2, ctrl), Some(Page::Stages));
        assert_eq!(press(Page::Drive, Key::Num3, ctrl), Some(Page::Settings));
        assert_eq!(press(Page::Drive, Key::Num4, ctrl), None);
        assert_eq!(press(Page::Drive, Key::Tab, ctrl), Some(Page::Circuit));
        assert_eq!(press(Page::Replays, Key::PageDown, ctrl), Some(Page::Drive));
        assert_eq!(press(Page::Drive, Key::Tab, shift_ctrl), Some(Page::Replays));
        assert_eq!(press(Page::Cars, Key::PageUp, ctrl), Some(Page::Codrivers));
        assert_eq!(press(Page::Setup, Key::Tab, ctrl), None);
        assert_eq!(press(Page::Import, Key::Num2, ctrl), Some(Page::Stages));
        assert_eq!(press(Page::Graphics, Key::Tab, ctrl), Some(Page::Settings));
        assert_eq!(press(Page::Drive, Key::Num3, Modifiers::NONE), None);
        assert_eq!(press(Page::Drive, Key::Tab, Modifiers::NONE), None);
    }

    /// Every page but Import and Car setup is in one group.
    #[test]
    fn every_page_is_in_a_section() {
        let pages: Vec<Page> = Section::ALL.iter().flat_map(|(s, _)| s.pages().iter().map(|(p, _)| *p)).collect();
        for &page in &pages {
            assert_eq!(pages.iter().filter(|p| **p == page).count(), 1, "{page:?}");
            assert!(page.section().is_some_and(|s| s.pages().iter().any(|(p, _)| *p == page)), "{page:?}");
            assert!(!page.name().is_empty());
        }
        assert_eq!((Page::Import.section(), Page::Setup.section()), (None, None));
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
        assert_eq!((no_cars.page, no_cars.get_more), (Page::Cars, Some(GetMore::Cars)));
        std::fs::remove_dir_all(&here).unwrap();
    }
}
