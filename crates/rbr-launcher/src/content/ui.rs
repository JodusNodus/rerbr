//! The Add's window side: starting a run, the notice on every page, what a finished run changes in the launcher, and
//! the Get more page's Ready to add and Add from your computer sections.

use std::path::PathBuf;

use super::kind::{skin, Known};
use super::{downloads, Goto, Outcome, Refresh};
use crate::car_browser::{Collection, Target};
use crate::car_skins;
use crate::catalog::Origin;
use crate::job::{self, Job};
use crate::pages::{section, Page, GOOD, WARN};
use crate::Form;

/// The Add's state: the running job, the last notice, and the recent downloads.
#[derive(Default)]
pub struct Add {
    job: Option<Job<Vec<Outcome>>>,
    pub notice: Vec<Outcome>,
    pub recent: downloads::Recent,
    /// The owned voice packs when the run started: a new one becomes the co-driver in use.
    voices_before: Vec<String>,
}

impl Add {
    /// Whether a run is going.
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
}

impl Form {
    /// Adds `paths` (files and folders) in one background run, unless one runs. While the viewer runs, only says to
    /// close it.
    pub fn add_content(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() || self.content.job.is_some() {
            return;
        }
        if self.child.is_some() {
            self.content.notice = vec![Outcome::warning("Close the drive first")];
            return;
        }
        self.content.notice.clear();
        self.content.voices_before = self.owned_voices.iter().map(|p| p.name.clone()).collect();
        let home = self.here();
        let numbers = self.catalog.maps.iter().chain(&self.catalog.unlisted).map(|m| m.number).collect();
        let weather = Some(self.data.install().join("Maps").join("TrackSettings.ini"));
        let known = Known { numbers, weather, cars: self.skin_targets() };
        self.content.job =
            Some(Job::start(move |report, cancel| Ok(super::run(&home, &known, &paths, report, cancel))));
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
        self.content.notice = outcomes;
    }

    /// Takes the Open button's way.
    fn content_goto(&mut self, goto: &Goto) {
        match goto {
            Goto::Stage(folder) => {
                let named = |dir: &std::path::Path| dir.file_name().is_some_and(|n| n.to_string_lossy() == *folder);
                let at = self.catalog.maps.iter().position(|m| match &m.origin {
                    Origin::Community(stage) => stage.folder.starts_with(self.here()) && named(&stage.folder),
                    _ => m.btb().is_some_and(|b| named(&b.folder)),
                });
                self.page = Page::Drive;
                self.stage_browser.open_on(at.unwrap_or(self.map), &self.favourite_maps, None);
            }
            Goto::Skin { car, .. } => {
                self.page = Page::Cars;
                if let Some(at) = self.catalog.cars.iter().position(|c| c.key == *car) {
                    let browser = &mut self.cars_page.browser;
                    browser.open_on(Target::Drive, at, &self.favourite_cars, Some(Collection::All));
                }
            }
            _ => self.page = goto.page().map_or(self.page, |(page, _)| page),
        }
    }

    /// The notice over the status line, on every page: the progress and Cancel while adding, then a line for each
    /// item with Close, and Open where it helps.
    pub fn content_notice(&mut self, ctx: &egui::Context) {
        if self.content.job.is_none() && self.content.notice.is_empty() {
            return;
        }
        let (mut finished, mut close, mut open) = (None, false, None);
        let page = self.page;
        ctx.root_ui(|root| {
            egui::Panel::bottom("content notice").show(root, |ui| {
                ui.add_space(4.0);
                if let Some(job) = &mut self.content.job {
                    ui.horizontal(|ui| finished = job.ui(ui));
                    ui.add_space(2.0);
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt("content notice")
                    .max_height(150.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for outcome in &self.content.notice {
                            let (color, tick) = if outcome.ok() { (GOOD, "✔") } else { (WARN, "⚠") };
                            ui.colored_label(color, format!("{tick} {}", outcome.line()));
                        }
                    });
                ui.horizontal(|ui| {
                    if let Some(file) = self.content.notice.iter().filter(|o| o.ok()).find_map(|o| o.open.clone()) {
                        if ui.button("Open the licence").clicked() {
                            crate::crash::open(file.as_os_str());
                        }
                    }
                    // An added thing's place, else where to do by hand what failed.
                    let notice = &self.content.notice;
                    let goto = notice.iter().filter(|o| o.ok()).chain(notice.iter()).find_map(|o| o.goto.clone());
                    match goto.as_ref().map(|g| (g, g.page())) {
                        Some((Goto::Stage(_), _)) if ui.button("Open the stage picker").clicked() => open = goto,
                        Some((_, Some((target, label)))) if page != target && ui.button(label).clicked() => open = goto,
                        _ => {}
                    }
                    close = ui.button("Close").clicked();
                });
                ui.add_space(2.0);
            });
        });
        if finished.is_some() {
            self.content.job = None;
        }
        match finished {
            Some(Ok(outcomes)) => self.content_done(outcomes),
            Some(Err(e)) => self.content.notice = vec![Outcome::warning(&format!("Adding stopped: {e}"))],
            None => {}
        }
        if let Some(goto) = open {
            self.content_goto(&goto);
        }
        if close {
            self.content.notice.clear();
        }
    }

    /// What the recent downloads list hides: the packs, stages and cars already added.
    pub(crate) fn owned_content(&self) -> downloads::Owned {
        let packs = self.owned_voices.iter().map(|p| p.name.clone());
        let packs = packs.chain(self.owned_notes.iter().map(|p| p.name.clone())).collect();
        let tracks = self.here().join(rbr_btb::player::TRACKS);
        let stages = self.catalog.maps.iter().filter_map(|m| m.btb()).filter(|b| b.folder.starts_with(&tracks));
        let stages = stages.filter_map(|b| Some(b.folder.file_name()?.to_string_lossy().to_lowercase())).collect();
        let cars = self
            .adding
            .cars
            .iter()
            .flat_map(|(name, pack)| [crate::car_packs::words(name), crate::car_packs::words(pack)]);
        let skins = car_skins::imported(&self.here());
        downloads::Owned { packs, stages, cars: cars.collect(), skins, ngp: self.ngp_revision == Some(778) }
    }

    /// Ready to add: the recent downloads ReRBR recognises, each with what it is and Add; or how they appear.
    pub fn ready_to_add_ui(&mut self, ui: &mut egui::Ui) {
        section(ui, "Ready to add");
        let idle = !self.content.busy() && self.child.is_none();
        let owned = self.owned_content();
        self.content.recent.poll(ui.ctx(), owned);
        let mut add = None;
        for found in &self.content.recent.list {
            ui.horizontal(|ui| {
                let name = found.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                ui.label(&name).on_hover_text(found.path.display().to_string());
                ui.weak(&found.label);
                if ui.add_enabled(idle, egui::Button::new("Add")).clicked() {
                    add = Some(found.path.clone());
                }
            });
        }
        if self.content.recent.list.is_empty() {
            ui.weak("Downloads from the last 30 days appear here.");
        }
        if let Some(path) = add {
            self.add_content(vec![path]);
        }
    }

    /// Add from your computer: files, a folder, and the drop line.
    pub fn add_from_computer_ui(&mut self, ui: &mut egui::Ui) {
        section(ui, "Add from your computer");
        let idle = !self.content.busy() && self.child.is_none();
        ui.horizontal_wrapped(|ui| {
            let files = ui.add_enabled(idle, egui::Button::new("Add files…")).on_hover_text(
                "Choose downloads: BTB stages, NGP car packs, car skins, co-drivers, pace notes or NGP. ReRBR works \
                 out what each is and adds it; the downloads are not changed.",
            );
            if files.clicked() {
                let picked = job::pick_files("The downloads to add");
                self.add_content(picked.into_iter().map(PathBuf::from).collect());
            }
            let folder = ui.add_enabled(idle, egui::Button::new("Add folder…")).on_hover_text(
                "Choose an extracted download: a stage's folder, a folder of stages, a car skin, a co-driver or a \
                 pace notes folder, or the disc.",
            );
            if folder.clicked() {
                if let Some(path) = job::pick_folder("The folder to add") {
                    self.add_content(vec![PathBuf::from(path)]);
                }
            }
            if super::drop::drop_works() {
                ui.weak("or drop files anywhere on this window");
            }
        });
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

    fn zip(path: &Path, files: &[(&str, &str)]) {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, text) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

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
        // The stage is the stage picker's: Open goes to it.
        form.content_goto(&Goto::Stage("Autio I Gravel".into()));
        assert!(form.stage_browser.open && form.page == Page::Drive);
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
        // Recent downloads of a fixed folder.
        let downloads = root.join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        let tracks = ["track.ini", "objectlist.ini", "Objects.zip", "Textures.zip"].map(|f| (f, ""));
        let files: Vec<(String, &str)> = tracks.iter().map(|(f, t)| (format!("Lappi Stage/{f}"), *t)).collect();
        let files: Vec<(&str, &str)> = files.iter().map(|(f, t)| (f.as_str(), *t)).collect();
        zip(&downloads.join("Lappi Stage.zip"), &files);
        zip(&downloads.join("Luppis V3 Notes.zip"), &[("Luppis/PaceNote.ini", "[SETTINGS]")]);
        form.content.recent.dir = Some(downloads);
        form.page = Page::Settings;
        form.content.notice.clear();
        form.stage_browser.open = false;
        let section = |ctx: &egui::Context, form: &mut Form| {
            ctx.root_ui(|root| {
                egui::CentralPanel::default().show(root, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| form.get_page(ui));
                })
            });
        };
        crate::capture::draw(|ctx| section(ctx, &mut form)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let rgba = crate::capture::draw(|ctx| section(ctx, &mut form)).unwrap();
        assert_eq!(form.content.recent.list.len(), 2, "{:?}", form.content.recent.list);
        crate::capture::write_png(&shots.join("add-content-section.png"), &rgba).unwrap();
        // The notice: some added, some not.
        let ok = |what: &str, kind, place: &str, goto| Outcome::added("x", kind, what, place, goto);
        form.content.notice = vec![
            ok("Autio I Gravel", "BTB stage", "it's in the stage picker", Goto::Stage("Autio I Gravel".into())),
            ok("Janne Laahanen V3", "co-driver", "pick it in Content > Co-drivers", Goto::Codrivers),
            Outcome::failed("Holiday.rar", "not supported", detect_rar()),
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
    /// that car on Content > Cars. Needs `RBR_DATA`, `RBR_PATCH` and `RBR_NGP_PHYSICS`.
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
        assert!(form.stage_browser.open && form.page == Page::Drive);

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

    fn detect_rar() -> &'static str {
        crate::content::source::refused::RAR
    }
}
