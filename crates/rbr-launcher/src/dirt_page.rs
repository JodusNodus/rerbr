//! The DiRT Rally picker: stages of the player's own DiRT Rally install, converted into stages of their own
//! (`rbr_dirt_rally`, `notes/dirt-rally.md`). Two places open it, the card on the Import page and the one in Get more
//! stages. The player chooses the game folder (nothing is searched for), the routes it holds are read, listed by
//! location, and the ones ticked are converted as one Add run ([`crate::content::dirt`]), so the progress line, Cancel,
//! the toast and Last import are the Add's.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use egui::{Align, Layout, RichText};
use rbr_dirt_rally::install::Install;
use rbr_dirt_rally::listing::{self, Kind, Listed};

use crate::content::dirt::{self, Pick, Previews};
use crate::job::{self, Job};
use crate::kit;
use crate::theme::*;
use crate::Form;

/// What a converted stage takes on disk, GB: its textures are 0.4 to 0.75 and the rest 0.1 or so (`notes/dirt-rally.md`).
const GB_PER_STAGE: f64 = 0.6;

/// The routes of a folder the player chose.
struct Scanned {
    /// The install's data folder, the one the conversion reads.
    data: PathBuf,
    routes: Vec<Listed>,
}

/// The picker's state: the folder, what it holds and what is ticked.
#[derive(Default)]
pub struct Picker {
    pub open: bool,
    folder: Option<PathBuf>,
    scan: Option<Job<Scanned>>,
    scanned: Option<Scanned>,
    /// Why the folder gave nothing.
    problem: Option<String>,
    /// Ticked routes, by id.
    picked: BTreeSet<String>,
    /// The stage folders already in the player's `tracks`.
    added: BTreeSet<String>,
}

impl Picker {
    /// Reads `folder` on a thread of its own, forgetting the last one's routes and ticks. The picker is open.
    fn read(&mut self, folder: PathBuf) {
        *self = Picker { open: true, folder: Some(folder.clone()), ..Picker::default() };
        self.scan = Some(Job::start(move |_, _| {
            let install = Install::at(&folder)?;
            let routes = listing::list(&install);
            if routes.is_empty() {
                return Err("it has no rally stages or Pikes Peak sections".into());
            }
            Ok(Scanned { data: install.data, routes })
        }));
    }

    /// A fixed folder and listing for the screenshots: three routes ticked, one added.
    pub fn screenshot(&mut self) {
        let route = |location: &str, area: &str, title: &str, index, kind, km: f64| Listed {
            id: format!("{location}/{area}/route_{index}"),
            folder: format!("DR_{location}_{area}_route_{index}"),
            title: title.to_owned(),
            location: location.to_owned(),
            index,
            kind,
            length: Some(km * 1000.0),
        };
        let rally = Kind::Rally;
        let routes = vec![
            route("finland", "ouninpohja_rally_01", "Finland, Ouninpohja 1, long forward", 0, rally, 14.1),
            route("finland", "ouninpohja_rally_01", "Finland, Ouninpohja 1, long reverse", 1, rally, 14.1),
            route("greece", "ghymno_rally_01", "Greece, Ghymno 1, long forward", 0, rally, 9.7),
            route("greece", "ghymno_rally_01", "Greece, Ghymno 1, long reverse", 1, rally, 9.7),
            route("greece", "ghymno_rally_01", "Greece, Ghymno 1, first half forward", 2, rally, 4.8),
            route("usa", "pikes_peak", "Pikes Peak, full climb", 0, Kind::PikesPeak, 19.5),
            route("usa", "pikes_peak", "Pikes Peak, section 1", 2, Kind::PikesPeak, 6.3),
        ];
        self.picked = [&routes[0], &routes[2], &routes[4]].map(|l| l.id.clone()).into();
        self.added = [routes[1].folder.clone()].into();
        self.folder = Some(PathBuf::from("/home/me/.steam/steam/steamapps/common/DiRT Rally"));
        self.scanned = Some(Scanned { data: PathBuf::new(), routes });
        self.open = true;
    }

    /// Takes the read's result once it is there.
    fn poll(&mut self) {
        match self.scan.as_mut().and_then(Job::result) {
            Some(Ok(scanned)) => self.scanned = Some(scanned),
            Some(Err(why)) => self.problem = Some(why),
            None => return,
        }
        self.scan = None;
    }

    /// Marks the routes whose stage is in `tracks` and unticks them.
    fn mark_added(&mut self, tracks: &std::path::Path) {
        let Some(scanned) = &self.scanned else { return };
        self.added =
            scanned.routes.iter().filter(|l| tracks.join(&l.folder).exists()).map(|l| l.folder.clone()).collect();
        let added = &self.added;
        let routes = &scanned.routes;
        self.picked.retain(|id| routes.iter().any(|l| l.id == *id && !added.contains(&l.folder)));
    }

    /// The folder line, the shortcuts and the routes by location; true when "Change folder…" was pressed.
    fn body_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut change = false;
        if let Some(folder) = &self.folder {
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    change = kit::link(ui, "Change folder…").clicked();
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        let text = RichText::new(folder.display().to_string()).font(mono(11.5)).color(TEXT_3);
                        ui.add(egui::Label::new(text).truncate());
                    });
                });
            });
            ui.add_space(10.0);
        }
        if self.scan.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Reading the folder…");
            });
            ui.ctx().request_repaint_after(Duration::from_millis(100));
            return change;
        }
        if let Some(problem) = &self.problem {
            ui.add(egui::Label::new(RichText::new(problem).color(WARN)).wrap());
            ui.add_space(4.0);
            kit::hint(ui, "Choose the folder DiRT Rally is installed in.");
            return change;
        }
        let Picker { scanned: Some(scanned), picked, added, .. } = self else { return change };
        ui.horizontal(|ui| {
            ui.label(RichText::new("Pick").size(12.0).family(medium()).color(TEXT_2));
            if kit::link(ui, "each stage's long route").clicked() {
                *picked = scanned
                    .routes
                    .iter()
                    .filter(|l| l.kind == Kind::Rally && l.index == 0 && !added.contains(&l.folder))
                    .map(|l| l.id.clone())
                    .collect();
            }
            if kit::link(ui, "none").clicked() {
                picked.clear();
            }
        });
        ui.add_space(8.0);
        for (at, group) in scanned.routes.chunk_by(|a, b| a.location == b.location).enumerate() {
            let ticked = group.iter().filter(|l| picked.contains(&l.id)).count();
            let routes = if group.len() == 1 { "route" } else { "routes" };
            let mut title = format!("{}  ·  {} {routes}", place(&group[0].location), group.len());
            if ticked > 0 {
                title.push_str(&format!("  ·  {ticked} picked"));
            }
            let header = RichText::new(title).family(medium()).color(TEXT);
            // The first location is open, to show what a row is.
            egui::CollapsingHeader::new(header).id_salt(&group[0].location).default_open(at == 0).show(ui, |ui| {
                for route in group {
                    route_row(ui, route, picked, added.contains(&route.folder));
                }
            });
        }
        ui.add_space(10.0);
        kit::hint(ui, "Each stage takes 0.4 to 0.9 GB and up to a minute and a half to convert, picture included.");
        change
    }
}

/// A location folder's name as the list shows it.
fn place(location: &str) -> String {
    if location.len() <= 3 {
        return location.to_uppercase();
    }
    let mut chars = location.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// A route's row: a tick, its name below its location, and its length (or "Added").
fn route_row(ui: &mut egui::Ui, route: &Listed, picked: &mut BTreeSet<String>, added: bool) {
    // `Greece, Ghymno 1, long reverse` is listed under Greece as `Ghymno 1, long reverse`.
    let name = match route.kind {
        Kind::Rally => route.title.split_once(", ").map_or(route.title.as_str(), |(_, rest)| rest),
        Kind::PikesPeak => route.title.as_str(),
    };
    ui.horizontal(|ui| {
        let mut on = added || picked.contains(&route.id);
        if ui.add_enabled(!added, egui::Checkbox::new(&mut on, name)).changed() {
            if on {
                picked.insert(route.id.clone());
            } else {
                picked.remove(&route.id);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if added {
                ui.label(RichText::new("Added").size(12.0).color(GOOD));
            } else if let Some(length) = route.length {
                ui.label(RichText::new(format!("{:.1} km", length / 1000.0)).size(12.0).color(TEXT_3));
            }
        });
    });
}

impl Form {
    /// Opens the picker, asking for the game folder first when none was chosen; false when that was cancelled.
    pub fn open_dirt(&mut self) -> bool {
        if self.dirt.folder.is_none() && !self.choose_dirt_folder() {
            return false;
        }
        self.dirt.open = true;
        let tracks = self.here().join(rbr_btb::player::TRACKS);
        self.dirt.mark_added(&tracks);
        true
    }

    /// Asks for the DiRT Rally folder and reads it; false when the dialog was cancelled.
    fn choose_dirt_folder(&mut self) -> bool {
        let Some(folder) = job::pick_folder("Your DiRT Rally folder") else { return false };
        self.dirt.read(PathBuf::from(folder));
        true
    }

    /// The card that opens the picker, on the Import page and in Get more stages; true when its button was pressed.
    pub fn dirt_card_ui(&self, ui: &mut egui::Ui) -> bool {
        let mut open = false;
        kit::panel(ui, |rows| {
            rows.full(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = if self.dirt.folder.is_some() { "Choose stages…" } else { "Choose folder…" };
                    let hint = "Choose your DiRT Rally folder, then the stages to convert. Nothing in it is changed.";
                    let button = ui.add_enabled(self.child.is_none(), egui::Button::new(label));
                    open = button.on_hover_text(hint).on_disabled_hover_text("Close the drive first").clicked();
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("DiRT Rally").family(medium()).color(TEXT));
                            ui.label(RichText::new("your own install").font(mono(11.5)).color(TEXT_3));
                        });
                        let what = "Rally stages and Pikes Peak, converted from your copy of the game. Choose its \
                                    folder, then the stages.";
                        ui.add(egui::Label::new(RichText::new(what).size(12.0).color(TEXT_2)).wrap());
                        ui.add_space(10.0);
                    });
                });
            });
        });
        open
    }

    /// The picker, when open.
    pub fn dirt_modal(&mut self, ctx: &egui::Context) {
        if !self.dirt.open {
            return;
        }
        self.dirt.poll();
        let busy = self.content.busy() || self.child.is_some();
        let count = self.dirt.picked.len();
        let (mut add, mut cancel, mut change) = (false, false, false);
        let result = kit::modal(
            ctx,
            "dirt rally",
            "Add DiRT Rally stages",
            Some("Converted from your own install, which isn't changed."),
            |ui| change = self.dirt.body_ui(ui),
            |ui| {
                let label = match count {
                    0 => "Add stages".to_owned(),
                    1 => "Add 1 stage".to_owned(),
                    n => format!("Add {n} stages"),
                };
                add = ui.add_enabled(count > 0 && !busy, kit::primary_button(&label, None)).clicked();
                cancel = ui.button("Cancel").clicked();
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| match (busy, count) {
                    (true, _) => _ = ui.label(RichText::new("Wait for the other add to finish").color(WARN)),
                    (_, 0) => {}
                    (_, n) => {
                        _ = ui.label(RichText::new(format!("About {:.1} GB", n as f64 * GB_PER_STAGE)).color(TEXT_2))
                    }
                });
            },
        );
        if change {
            self.choose_dirt_folder();
        }
        if add {
            self.dirt.open = false;
            self.add_dirt();
        } else if cancel || result.closed() {
            self.dirt.open = false;
        }
    }

    /// Converts the ticked routes as the Add's run.
    fn add_dirt(&mut self) {
        let Some(scanned) = &self.dirt.scanned else { return };
        let picks: Vec<Pick> = scanned
            .routes
            .iter()
            .filter(|l| self.dirt.picked.contains(&l.id))
            .map(|l| Pick { id: l.id.clone(), folder: l.folder.clone(), title: l.title.clone() })
            .collect();
        let data = scanned.data.clone();
        self.dirt.picked.clear();
        let home = self.here();
        let previews = self.dirt_previews();
        self.start_run(move |report, cancel| dirt::add(&home, &data, &picks, previews.as_ref(), report, cancel));
    }

    /// What makes each stage's picture: the viewer beside the launcher, with the game folders it drives with. None when
    /// there is no viewer (the stage's card then shows its driveline).
    fn dirt_previews(&self) -> Option<Previews> {
        let viewer = self.paths.viewer.clone();
        viewer.is_file().then(|| Previews { viewer, maps: self.paths.data.join("Maps"), env: self.child_env() })
    }
}
