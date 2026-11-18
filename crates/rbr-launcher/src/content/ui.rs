//! The Add's window side: starting a run, what a finished run changes in the launcher, its progress line and toast on
//! every page, and the Import page's drop zone and Last import.

use std::path::PathBuf;

use egui::{Align, Color32, Layout, RichText};

use super::kind::{skin, Known};
use super::{Goto, Outcome, Refresh};
use crate::car_skins;
use crate::catalog::Origin;
use crate::job::{self, Job};
use crate::kit;
use crate::pages::Page;
use crate::theme::*;
use crate::Form;

/// A small card over any page but Import: lines (ticked or warned) and, for an import, a link to the Import page.
pub struct Toast {
    pub lines: Vec<(bool, String)>,
    pub import: bool,
}

/// The Add's state: the running job, the last import's outcomes and the toast.
#[derive(Default)]
pub struct Add {
    job: Option<Job<Vec<Outcome>>>,
    pub notice: Vec<Outcome>,
    pub toast: Option<Toast>,
    /// The owned voice packs when the run started: a new one becomes the co-driver in use.
    voices_before: Vec<String>,
}

impl Add {
    /// Whether a run is going.
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
}

/// The most lines a toast shows.
const TOAST_LINES: usize = 4;

impl Form {
    /// Adds `paths` (files and folders) in one background run, unless one runs. While the viewer runs, only says to
    /// close it.
    pub fn add_content(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() || self.content.job.is_some() {
            return;
        }
        if self.child.is_some() {
            self.show_import(vec![Outcome::warning("Close the drive first")]);
            return;
        }
        self.content.notice.clear();
        self.content.toast = None;
        self.content.voices_before = self.owned_voices.iter().map(|p| p.name.clone()).collect();
        let home = self.here();
        let numbers = self.catalog.maps.iter().chain(&self.catalog.unlisted).map(|m| m.number).collect();
        let weather = Some(self.data.install().join("Maps").join("TrackSettings.ini"));
        let known = Known { numbers, weather, cars: self.skin_targets() };
        self.content.job =
            Some(Job::start(move |report, cancel| Ok(super::run(&home, &known, &paths, report, cancel))));
    }

    /// Makes `outcomes` the last import; on any page but Import, a toast says them too.
    fn show_import(&mut self, outcomes: Vec<Outcome>) {
        self.content.toast = (self.page != Page::Import)
            .then(|| Toast { lines: outcomes.iter().map(|o| (o.ok(), o.line())).collect(), import: true });
        self.content.notice = outcomes;
    }

    /// The cars a skin can dress, one per key.
    fn skin_targets(&self) -> Vec<skin::Target> {
        // An archive car is drawn from the game data.
        let game = self.data.install().to_path_buf();
        let mut targets: Vec<skin::Target> = Vec::new();
        for car in &self.catalog.cars {
            let Some(model) = car_skins::model_folder(car) else { continue };
            if targets.iter().any(|t| t.key == car.key) {
                continue;
            }
            let title = self.car_browser.row_of(&car.name).map_or_else(|| car.name.clone(), |r| r.title.clone());
            targets.push(skin::Target {
                key: car.key.clone(),
                title,
                model,
                textures: car_skins::textures_dir(car, &game),
            });
        }
        targets
    }

    /// Reads again what a finished run changed.
    fn content_done(&mut self, outcomes: Vec<Outcome>) {
        let touched = |r: Refresh| outcomes.iter().any(|o| o.refresh == r);
        let added = |r: Refresh| outcomes.iter().any(|o| o.refresh == r && o.ok());
        if touched(Refresh::Codriver) {
            self.rescan_codrivers();
            // A new voice pack drives (its default voice); its notes play by themselves.
            let voices = std::mem::take(&mut self.content.voices_before);
            let new = self.owned_voices.iter().find(|p| p.problem.is_none() && !voices.contains(&p.name));
            if let Some(name) = new.map(|p| p.name.clone()).filter(|_| added(Refresh::Codriver)) {
                (self.codriver, self.codriver_pack, self.codriver_voice) = (true, Some(name), None);
            }
        }
        if touched(Refresh::Cars) {
            self.rescan_cars();
        }
        // A skin added is worn.
        for outcome in outcomes.iter().filter(|o| o.ok()) {
            if let Some(Goto::Skin { car, skin }) = &outcome.goto {
                self.skins.chosen.insert(car.clone(), skin.clone());
            }
        }
        if touched(Refresh::Stages) {
            let tracks = self.here().join(rbr_btb::player::TRACKS);
            self.catalog.add_new_tracks(&tracks);
            self.catalog.add_classic_stages(&self.here().join(rbr_btb::player::TRACKS));
            self.list_maps();
        }
        if added(Refresh::Ngp) {
            let to = self.here().join(crate::ngp_install::KEPT);
            self.ngp_dll.path = to.join("NGP.dll").display().to_string();
            self.ngp_physics = to.join("physics.rbz").display().to_string();
            self.save();
            self.reload = true;
        }
        let disc = outcomes.iter().find_map(|o| match &o.goto {
            Some(Goto::Disc(disc)) if o.ok() => Some(disc.clone()),
            _ => None,
        });
        if let Some(disc) = disc {
            self.disc_setup.set_disc(&disc);
            self.page = Page::Settings;
        }
        self.show_import(outcomes);
    }

    /// Takes an Open link's way.
    pub(crate) fn content_goto(&mut self, goto: &Goto) {
        match goto {
            Goto::Stage(folder) => {
                let named = |dir: &std::path::Path| dir.file_name().is_some_and(|n| n.to_string_lossy() == *folder);
                let at = self.catalog.maps.iter().position(|m| match &m.origin {
                    Origin::Community(stage) => stage.folder.starts_with(self.here()) && named(&stage.folder),
                    _ => m.btb().is_some_and(|b| named(&b.folder)),
                });
                self.page = Page::Stages;
                self.stages_page.show_stage(at.unwrap_or(self.map));
            }
            Goto::Skin { car, .. } => {
                self.page = Page::Cars;
                if let Some(at) = self.catalog.cars.iter().position(|c| c.key == *car) {
                    let browser = &mut self.cars_page.browser;
                    browser.open_on(
                        crate::car_browser::Target::Drive,
                        at,
                        &self.favourite_cars,
                        Some(crate::car_browser::Collection::All),
                    );
                }
            }
            Goto::Disc(disc) => {
                self.disc_setup.set_disc(disc);
                self.page = Page::Settings;
            }
            _ => self.page = goto.page().map_or(self.page, |(page, _)| page),
        }
    }

    /// Runs on every page, every frame: finishes the jobs that ended (the import, the car download), draws the
    /// progress line of a job whose own place is not showing, and the toast.
    ///
    /// A download keeps going when the Get more modal is closed or the page changes: the job is polled here, not
    /// by the UI that shows it.
    pub fn content_notice(&mut self, ctx: &egui::Context) {
        let finished = self.content.job.as_mut().and_then(Job::result);
        if self.content.job.is_some() && finished.is_some() {
            self.content.job = None;
        }
        match finished {
            Some(Ok(outcomes)) => self.content_done(outcomes),
            Some(Err(e)) => self.show_import(vec![Outcome::warning(&format!("Adding stopped: {e}"))]),
            None => {}
        }
        self.poll_adding();
        let import_running = self.content.job.as_ref().filter(|_| self.page != Page::Import);
        let cars_running = self.adding_running().filter(|_| self.get_more != Some(crate::get_page::GetMore::Cars));
        if self.content.job.is_some() || cars_running.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if import_running.is_some() || cars_running.is_some() {
            ctx.root_ui(|root| {
                let margin = egui::Margin { left: kit::PAGE_LEFT, right: kit::PAGE_RIGHT, top: 8, bottom: 8 };
                egui::Panel::bottom("content progress")
                    .frame(egui::Frame::new().fill(FOOTER).inner_margin(margin))
                    .show(root, |ui| {
                        if let Some(job) = import_running {
                            ui.horizontal(|ui| job.progress_ui(ui));
                        }
                        if let Some(job) = cars_running {
                            ui.horizontal(|ui| job.progress_ui(ui));
                        }
                    });
            });
        }
        if self.page == Page::Import {
            self.content.toast = None;
        }
        let Some(toast) = &self.content.toast else { return };
        let mut to_import = false;
        let closed = kit::toast(ctx, "content toast", |ui| {
            for (ok, text) in toast.lines.iter().take(TOAST_LINES) {
                let (color, tick) = if *ok { (GOOD, "✔") } else { (WARN, "⚠") };
                let line = RichText::new(format!("{tick} {text}")).size(12.5).color(color);
                ui.add(egui::Label::new(line).wrap());
            }
            if toast.lines.len() > TOAST_LINES {
                ui.label(RichText::new(format!("and {} more", toast.lines.len() - TOAST_LINES)).color(TEXT_2));
            }
            if toast.import {
                to_import = kit::link(ui, "Open Import").clicked();
            }
        });
        if to_import {
            self.page = Page::Import;
        }
        if closed || to_import {
            self.content.toast = None;
        }
    }

    /// The drop zone: where to drop, Choose files… and Choose folder…, the progress and Cancel while adding.
    pub fn drop_zone_ui(&mut self, ui: &mut egui::Ui) {
        let idle = !self.content.busy() && self.child.is_none();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 190.0), egui::Sense::hover());
        let hovering = ui.input(|i| !i.raw.hovered_files.is_empty());
        let (stroke, fill) = match hovering && super::drop::drop_works() {
            true => (egui::Stroke::new(1.5, ACCENT), ACCENT.gamma_multiply(0.12)),
            false => (egui::Stroke::new(1.0, white(36)), Color32::TRANSPARENT),
        };
        ui.painter().rect_filled(rect, 10, fill);
        let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
        ui.painter().extend(egui::Shape::dashed_line(&corners, stroke, 6.0, 4.0));
        let mut zone =
            ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(16.0)).layout(Layout::top_down(Align::Center)));
        let drop = super::drop::drop_works();
        zone.add_space(44.0);
        let title = if drop { "Drop files or folders here" } else { "Add files or folders" };
        zone.label(RichText::new(title).size(15.0).family(semibold()).color(TEXT));
        zone.add_space(2.0);
        let what = "Stages, cars, skins, co-drivers, pace notes, NGP or the game disc";
        zone.label(RichText::new(what).color(TEXT_2));
        zone.add_space(14.0);
        if let Some(job) = &self.content.job {
            let at = zone.available_width() / 2.0 - 130.0;
            zone.horizontal(|ui| {
                ui.add_space(at.max(0.0));
                job.progress_ui(ui);
            });
            return;
        }
        let at = zone.available_width() / 2.0 - 128.0;
        let (mut files, mut folder) = (false, false);
        zone.horizontal(|ui| {
            ui.add_space(at.max(0.0));
            ui.spacing_mut().item_spacing.x = 8.0;
            let size = egui::vec2(116.0, 30.0);
            let button = |label| egui::Button::new(label);
            files = ui
                .add_enabled(idle, button("Choose files…").min_size(size))
                .on_hover_text(
                    "Choose downloads: BTB stages, NGP car packs, car skins, co-drivers, pace notes or NGP. ReRBR \
                     works out what each is and adds it; the downloads are not changed.",
                )
                .clicked();
            folder = ui
                .add_enabled(idle, button("Choose folder…").min_size(size))
                .on_hover_text(
                    "Choose an extracted download: a stage's folder, a folder of stages, a car skin, a co-driver or a \
                     pace notes folder, or the disc.",
                )
                .clicked();
        });
        if self.child.is_some() {
            zone.add_space(8.0);
            zone.label(RichText::new("Close the drive first").color(WARN));
        }
        if files {
            let picked = job::pick_files("The downloads to add");
            self.add_content(picked.into_iter().map(PathBuf::from).collect());
        }
        if folder {
            if let Some(path) = job::pick_folder("The folder to add") {
                self.add_content(vec![PathBuf::from(path)]);
            }
        }
    }

    /// Last import: every outcome with its own Open link, Clear, and what is refused.
    pub fn last_import_ui(&mut self, ui: &mut egui::Ui) {
        if self.content.notice.is_empty() {
            return;
        }
        if kit::group_label(ui, "Last import", Some("Clear")) {
            self.content.notice.clear();
            return;
        }
        let mut open = None;
        kit::panel(ui, |rows| {
            for outcome in &self.content.notice {
                rows.full(|ui| {
                    let (color, tick) = if outcome.ok() { (GOOD, "✔") } else { (WARN, "⚠") };
                    ui.label(RichText::new(tick).color(color));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some((_, label)) = outcome.goto.as_ref().and_then(|g| g.page()) {
                            if kit::link(ui, label).clicked() {
                                open = outcome.goto.clone();
                            }
                        }
                        if let Some(file) = outcome.open.as_ref().filter(|_| outcome.ok()) {
                            if kit::link(ui, "Open the licence").clicked() {
                                crate::crash::open(file.as_os_str());
                            }
                        }
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            let line = RichText::new(outcome.line()).color(TEXT_LIST);
                            ui.add(egui::Label::new(line).wrap());
                        });
                    });
                });
            }
        });
        if let Some(goto) = open {
            self.content_goto(&goto);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::content::kind::btb::tests::{synthetic_stage, write};
    use crate::settings::{load, Settings};
    use crate::{Start, SETTINGS_FILE};

    /// The Add through a loaded launcher: a stage folder and a co-driver folder dropped together are added, the stage
    /// list and the co-drivers read again; and the Add content section, the notice and the drop hint drawn offscreen
    /// to `RBR_SCREENSHOTS`. Needs `RBR_DATA`, `RBR_PATCH` and `RBR_NGP_PHYSICS`.
    #[test]
    fn the_add_reads_the_lists_again_and_draws() {
        let env = |key: &str| std::env::var_os(key).map(|v| v.to_string_lossy().into_owned());
        let (Some(data), Some(patch), Some(ngp)) = (env("RBR_DATA"), env("RBR_PATCH"), env("RBR_NGP_PHYSICS")) else {
            eprintln!("skipping: RBR_DATA, RBR_PATCH or RBR_NGP_PHYSICS not set");
            return;
        };
        let root = crate::test_dir("content-form");
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        let start = Start {
            viewer: PathBuf::from("/bin/viewer"),
            standalone: PathBuf::from("/bin/rbr-standalone"),
            settings: home.join(SETTINGS_FILE),
            data: Some(PathBuf::from(&data)),
            patch: Some(PathBuf::from(&patch)),
            physics: None,
        };
        let mut form = load(&start, Settings { map: Some(54), ngp_physics: Some(ngp), ..Settings::default() }).unwrap();
        form.read_only = true;
        let before = form.catalog.maps.len();
        let stage = root.join("drop/Autio I Gravel");
        synthetic_stage(&stage, "Autio I Gravel");
        let voice = root.join("drop/Janne Laahanen V3");
        write(&voice.join("PaceNote.ini"), b"[SETTINGS]\nsounds=english/steve\n");
        write(&voice.join("config/pacenotes/Rbr.ini"), b"[PACKAGE::RBR]\nfile=calls.ini\n");
        write(&voice.join("config/pacenotes/calls.ini"), b"[PACENOTE::ONE_LEFT]\nid=0\nSounds=1\nSnd0=a.ogg\n");
        write(&voice.join("config/ranges/Rbr.ini"), b"[RANGE::50]\nSounds=1\nSnd0=50_1.ogg\n");
        write(&voice.join("sounds/english/steve/a.ogg"), b"OggS fake");
        write(&root.join("drop/Holiday.rar"), b"Rar!");
        form.add_content(vec![stage, voice, root.join("drop/Holiday.rar")]);
        assert!(form.content.busy());
        // A second drop while the first runs is ignored.
        form.add_content(vec![root.join("drop/Holiday.rar")]);
        while form.content.busy() {
            crate::capture::draw(|ctx| form.content_notice(ctx)).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let lines: Vec<String> = form.content.notice.iter().map(Outcome::line).collect();
        assert_eq!(lines.len(), 3, "{lines:?}");
        let has = |start: &str| lines.iter().any(|l| l.starts_with(start));
        assert!(has("Added Autio I Gravel (BTB stage)"), "{lines:?}");
        assert!(has("Added Janne Laahanen V3 (co-driver)"), "{lines:?}");
        assert!(has("Couldn't add Holiday.rar"), "{lines:?}");
        assert_eq!(form.catalog.maps.len(), before + 1);
        let added = form.catalog.maps.last().and_then(|m| m.btb()).map(|b| b.name.clone());
        assert_eq!(added.as_deref(), Some("Autio I Gravel"));
        assert!(form.owned_voices.iter().any(|p| p.name == "Janne Laahanen V3"));
        assert_eq!(form.codriver_pack.as_deref(), Some("Janne Laahanen V3"));
        // The stage is Stages': Open goes to it.
        form.content_goto(&Goto::Stage("Autio I Gravel".into()));
        assert_eq!(form.page, Page::Stages);
        // Each kind goes to its page.
        for (goto, page) in [
            (Goto::Cars, Page::Cars),
            (Goto::Codrivers, Page::Codrivers),
            (Goto::Settings, Page::Settings),
            (Goto::Disc("/x.iso".into()), Page::Settings),
        ] {
            form.page = Page::Import;
            form.content_goto(&goto);
            assert_eq!(form.page, page);
        }
        // While the viewer runs, a drop only says to close it: checked by the notice text.
        form.content.notice = vec![Outcome::warning("Close the drive first")];
        assert_eq!(form.content.notice[0].line(), "Close the drive first");

        let Some(shots) = env("RBR_SCREENSHOTS") else {
            eprintln!("skipping the screenshots: RBR_SCREENSHOTS not set");
            return;
        };
        let shots = Path::new(&shots).join("content");
        form.page = Page::Settings;
        form.content.notice.clear();
        form.stage_browser.open = false;
        let section = |ctx: &egui::Context, form: &mut Form| {
            ctx.root_ui(|root| {
                egui::CentralPanel::default().show(root, |ui| {
                    form.import_page(ui);
                })
            });
        };
        crate::capture::draw(|ctx| section(ctx, &mut form)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let rgba = crate::capture::draw(|ctx| section(ctx, &mut form)).unwrap();
        crate::capture::write_png(&shots.join("add-content-section.png"), &rgba).unwrap();
        // The notice: some added, some not.
        let ok = |what: &str, kind, place: &str, goto| Outcome::added("x", kind, what, place, goto);
        form.content.notice = vec![
            ok("Autio I Gravel", "BTB stage", "it's in Stages", Goto::Stage("Autio I Gravel".into())),
            ok("Janne Laahanen V3", "co-driver", "pick it in Co-drivers", Goto::Codrivers),
            Outcome::failed("Setup.exe", "not supported", detect_installer()),
            Outcome::failed(
                "Ford Fiesta - Model.7z",
                "NGP car",
                "this is a car model/sound; add it together with its NGP Physics zip",
            ),
        ];
        let page = |ctx: &egui::Context, form: &mut Form| {
            form.content_notice(ctx);
            ctx.root_ui(|root| {
                egui::CentralPanel::default().show(root, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| form.settings_page(ui));
                })
            });
        };
        crate::capture::shot(&shots.join("notice.png"), |ctx| page(ctx, &mut form)).unwrap();
        crate::capture::shot(&shots.join("drop-overlay.png"), |ctx| {
            page(ctx, &mut form);
            Form::drop_hint(ctx);
        })
        .unwrap();
    }

    /// A skin dropped on a loaded launcher goes to the retail car whose texture names it has, is worn, and Open shows
    /// that car on Cars. Needs `RBR_DATA`, `RBR_PATCH` and `RBR_NGP_PHYSICS`.
    #[test]
    fn a_dropped_skin_is_worn_by_its_car() {
        let env = |key: &str| std::env::var_os(key).map(|v| v.to_string_lossy().into_owned());
        let (Some(data), Some(patch), Some(ngp)) = (env("RBR_DATA"), env("RBR_PATCH"), env("RBR_NGP_PHYSICS")) else {
            eprintln!("skipping: RBR_DATA, RBR_PATCH or RBR_NGP_PHYSICS not set");
            return;
        };
        let root = crate::test_dir("content-skin-form");
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        let start = Start {
            viewer: PathBuf::from("/bin/viewer"),
            standalone: PathBuf::from("/bin/rbr-standalone"),
            settings: home.join(SETTINGS_FILE),
            data: Some(PathBuf::from(&data)),
            patch: Some(PathBuf::from(&patch)),
            physics: None,
        };
        let mut form = load(&start, Settings { ngp_physics: Some(ngp), ..Settings::default() }).unwrap();
        form.read_only = true;
        // A retail car: its key is its model folder, the prefix of its textures.
        let retail = |t: &skin::Target| t.key == t.model && t.textures.is_some();
        let target = form.skin_targets().into_iter().find(retail).expect("a retail car");
        let prefix = format!("{}_external", target.model.to_lowercase());
        let own: Vec<String> = fs::read_dir(target.textures.as_ref().expect("its textures"))
            .unwrap()
            .flatten()
            .map(|f| f.file_name().to_string_lossy().into_owned())
            .filter(|n| n.to_lowercase().starts_with(&prefix))
            .collect();
        assert!(!own.is_empty());
        let skin = root.join("drop/Martini");
        for name in &own {
            write(&skin.join("textures").join(name), b"skin");
        }
        form.add_content(vec![skin]);
        while form.content.busy() {
            crate::capture::draw(|ctx| form.content_notice(ctx)).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let lines: Vec<String> = form.content.notice.iter().map(Outcome::line).collect();
        assert_eq!(lines, [format!("Added Martini (car skin): the {} wears it now", target.title)]);
        assert_eq!(form.skins.chosen.get(&target.key).map(String::as_str), Some("Martini"));
        let at = form.catalog.cars.iter().position(|c| c.key == target.key).unwrap();
        let worn = home.join(car_skins::DIR).join(&target.key).join("Martini");
        assert_eq!(form.skins.choice(&form.catalog.cars[at]), Some(rbr_formats::car_pack::Skin::Dir(worn)));
        let goto = form.content.notice[0].goto.clone().unwrap();
        form.content_goto(&goto);
        assert_eq!((form.page, form.cars_page.browser.highlighted()), (Page::Cars, Some(at)));
        fs::remove_dir_all(&root).unwrap();
    }

    /// Classic stages dropped on a loaded launcher: one added (with its author), one refused for its encryption; the
    /// stage list shows the added one, the notice offers its licence, and Open goes to it. The notice and the stage
    /// picker drawn to `RBR_SCREENSHOTS/classic`. Needs `RBR_DATA`, `RBR_PATCH` and `RBR_NGP_PHYSICS`.
    #[test]
    fn classic_stages_are_added_listed_and_drawn() {
        use crate::content::kind::classic::tests::{rsf_stage, variant};
        let env = |key: &str| std::env::var_os(key).map(|v| v.to_string_lossy().into_owned());
        let (Some(data), Some(patch), Some(ngp)) = (env("RBR_DATA"), env("RBR_PATCH"), env("RBR_NGP_PHYSICS")) else {
            eprintln!("skipping: RBR_DATA, RBR_PATCH or RBR_NGP_PHYSICS not set");
            return;
        };
        let root = crate::test_dir("content-classic");
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        let start = Start {
            viewer: PathBuf::from("/bin/viewer"),
            standalone: PathBuf::from("/bin/rbr-standalone"),
            settings: home.join(SETTINGS_FILE),
            data: Some(PathBuf::from(&data)),
            patch: Some(PathBuf::from(&patch)),
            physics: None,
        };
        let mut form = load(&start, Settings { map: Some(54), ngp_physics: Some(ngp), ..Settings::default() }).unwrap();
        form.read_only = true;
        let before = form.catalog.maps.len();
        // Stage 54 is a retail map's number: the first is added as a new number, the second is encrypted.
        let (one, two) = (root.join("drop/one"), root.join("drop/two"));
        rsf_stage(&one, 54, "OM");
        rsf_stage(&two, 8000, "O");
        write(&two.join("8000-Kamenak/track-8000_O.lbs"), &[0x01, 0xa0, 0, 0, 1]);
        variant(&root.join("drop/loose/Maps"), 8994, 'M');
        write(
            &root.join("drop/loose/Maps/Tracks.ini"),
            b"[Map8994]\r\nStageName=Loose Gravel\r\nAuthor=Test\r\nSurface=1\r\n",
        );
        form.add_content(vec![one.join("54-Kamenak"), two.join("8000-Kamenak"), root.join("drop/loose")]);
        while form.content.busy() {
            crate::capture::draw(|ctx| form.content_notice(ctx)).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let lines: Vec<String> = form.content.notice.iter().map(|o| format!("{} {}", o.ok(), o.line())).collect();
        assert_eq!(
            lines,
            [
                "true Added Kamenak (classic stage) by Miro Kurek",
                "false Couldn't add Kamenak: this stage is encrypted for the RBR Czech plugin / RallySimFans: it can't be used",
                "true Added Loose Gravel (classic stage) by Test",
            ],
            "{lines:?}"
        );
        assert_eq!(form.catalog.maps.len(), before + 2);
        let added = form.catalog.maps.iter().rev().take(2).map(|m| m.number).collect::<Vec<_>>();
        assert_eq!(added, [10_000, 8994]);
        assert!(form.content.notice[0].open.as_ref().is_some_and(|p| p.is_file()));
        let listed = form.catalog.map(10_000).unwrap();
        assert!(matches!(&listed.origin, crate::catalog::Origin::Community(s) if s.problem.is_none()));
        form.content_goto(&Goto::Stage("10000-Kamenak".into()));
        assert_eq!(form.page, Page::Stages);

        let Some(shots) = env("RBR_SCREENSHOTS") else {
            eprintln!("skipping the screenshots: RBR_SCREENSHOTS not set");
            return;
        };
        let shots = Path::new(&shots).join("classic");
        let page = |ctx: &egui::Context, form: &mut Form| {
            form.content_notice(ctx);
            ctx.root_ui(|root| {
                egui::CentralPanel::default().show(root, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| form.settings_page(ui));
                })
            });
        };
        form.page = Page::Settings;
        form.stage_browser.open = false;
        crate::capture::shot(&shots.join("notice.png"), |ctx| page(ctx, &mut form)).unwrap();
        form.content_goto(&Goto::Stage("10000-Kamenak".into()));
        let picker = |ctx: &egui::Context, form: &mut Form| {
            ctx.root_ui(|root| {
                egui::CentralPanel::default().show(root, |ui| form.stage_browser_page(ui));
            });
        };
        crate::capture::draw(|ctx| picker(ctx, &mut form)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        crate::capture::shot(&shots.join("stage-picker.png"), |ctx| picker(ctx, &mut form)).unwrap();
    }

    fn detect_installer() -> &'static str {
        crate::content::source::refused::INSTALLER
    }
}
