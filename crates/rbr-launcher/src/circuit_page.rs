//! The Circuit page: laps on a circuit (`notes/session.md`), the hotlapping twin of the Drive page. The circuits are the
//! BTB stages marked `circuit = 1` (`btb::Btb::circuit`) and the stages that look like one (`rbr_btb::circuit`, found by a
//! scan), which stay stages on the Drive page too. It takes the Drive page's car, setup, weather and tyre; what
//! is its own is the circuit picker (the stage picker's, over the circuits), the profile's best laps, the session's length (free practice or N laps) and
//! the best lap's sector times. Start runs `rbr-standalone --laps` on the circuit and the best laps are kept in the
//! save folder (`rbr_session::book`).

use std::time::{Duration, Instant};

use egui::RichText;
use rbr_formats::saves::SaveDir;
use rbr_render::hud::time_label;
use rbr_session::{BestLap, LapBook};

use crate::browser;
use crate::car_browser::Target;
use crate::catalog::Map;
use crate::drive_page::{card, column, labelled, split_columns, CardText, Summary};
use crate::kit;
use crate::pages::combo;
use crate::stage_browser::{self, Action, StageRow};
use crate::theme::{self, ERROR, TEXT, TEXT_2, TEXT_3};
use crate::Form;

/// The session lengths on offer, 0 for free practice.
pub const LAPS: [u32; 6] = [0, 1, 3, 5, 10, 20];
/// How long the best laps read from the save folder stay current: a drive ends in another process.
const REFRESH: Duration = Duration::from_secs(2);

/// What the page keeps.
#[derive(Debug, Default)]
pub struct CircuitPage {
    /// The circuit's map number, once chosen.
    pub map: Option<u32>,
    /// The session's laps, 0 for free practice.
    pub laps: u32,
    /// The pace notes (the co-driver's voice, the note symbols and the stack) in the session; off unless asked for: a
    /// lap is not a stage, and its notes follow the road of a stage.
    pub pace_notes: bool,
    /// The best laps in the save folder and when they were read.
    book: LapBook,
    read: Option<Instant>,
    /// The circuit whose conditions were last checked against the weather rows.
    checked: Option<u32>,
    /// The BTB stages that look like circuits and are not marked ([`rbr_btb::circuit::is_candidate`]), by map number,
    /// once the scan is done; the scan runs on a thread, started when the page first draws.
    candidates: Vec<u32>,
    scan: Scan,
    /// Road and tyres are open in the Conditions.
    pub road_open: bool,
}

/// The scan for candidates.
#[derive(Debug, Default)]
enum Scan {
    #[default]
    Idle,
    Running(std::sync::mpsc::Receiver<Vec<u32>>),
    Done,
}

/// The session length in words.
pub fn laps_label(laps: u32) -> String {
    match laps {
        0 => "Free practice".to_owned(),
        1 => "1 lap".to_owned(),
        n => format!("{n} laps"),
    }
}

/// The sectors' own times of a lap given by its time at each gate (since the lap began): each gate less the one before,
/// and the rest of the lap.
pub fn sector_times(best: &BestLap) -> Vec<f32> {
    let mut from = 0.0;
    best.sectors
        .iter()
        .copied()
        .chain([best.lap])
        .map(|gate| {
            let time = gate - from;
            from = gate;
            time
        })
        .collect()
}

impl CircuitPage {
    /// The page as the settings left it.
    pub fn from_settings(map: Option<u32>, laps: Option<u32>, pace_notes: Option<bool>) -> Self {
        Self {
            map,
            laps: laps.filter(|l| LAPS.contains(l)).unwrap_or(0),
            pace_notes: pace_notes.unwrap_or(false),
            ..Self::default()
        }
    }
}

impl Form {
    /// Whether map `index` plays as laps: a circuit by its files, or a stage the scan found to end where it starts.
    pub fn plays_as_circuit(&self, index: usize) -> bool {
        self.catalog.maps.get(index).is_some_and(|m| {
            m.btb().is_some_and(|btb| btb.circuit) || (m.btb().is_some() && self.circuit.candidates.contains(&m.number))
        })
    }

    /// Whether map `index` is a circuit by its files: it plays only on the Circuit page.
    pub fn is_circuit(&self, index: usize) -> bool {
        self.catalog.maps.get(index).and_then(Map::btb).is_some_and(|btb| btb.circuit)
    }

    /// The maps that play as laps, by their index.
    pub fn circuits(&self) -> Vec<usize> {
        (0..self.catalog.maps.len()).filter(|&i| self.plays_as_circuit(i)).collect()
    }

    /// Fills the circuit browser with the maps that play as laps.
    pub fn list_circuits(&mut self) {
        let rows = self
            .circuits()
            .into_iter()
            .map(|i| {
                let map = &self.catalog.maps[i];
                let mut row = StageRow::new(i, map, self.catalog.availability(&self.season, map).err());
                row.candidate = !self.is_circuit(i);
                row
            })
            .collect();
        self.circuit_browser.set_rows(rows);
    }

    /// Starts the scan for stages that look like circuits, and takes its result when it is done.
    fn poll_scan(&mut self, ctx: &egui::Context) {
        if matches!(self.circuit.scan, Scan::Idle) {
            let stages: Vec<(u32, std::path::PathBuf)> = (self.catalog.maps.iter())
                .filter_map(|m| m.btb().filter(|b| !b.circuit).map(|b| (m.number, b.folder.clone())))
                .collect();
            let (send, receive) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let found = stages
                    .into_iter()
                    .filter(|(_, folder)| {
                        rbr_btb::track::Track::open(folder)
                            .and_then(|t| t.driveline())
                            .is_ok_and(|line| rbr_btb::circuit::is_candidate(&line.points))
                    })
                    .map(|(number, _)| number)
                    .collect();
                let _ = send.send(found);
            });
            self.circuit.scan = Scan::Running(receive);
        }
        if let Scan::Running(receive) = &self.circuit.scan {
            match receive.try_recv() {
                Ok(found) => self.scanned(found),
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(200)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.scanned(Vec::new()),
            }
        }
    }

    /// The scan is done: the stages it found play as laps too.
    fn scanned(&mut self, found: Vec<u32>) {
        self.circuit.candidates = found;
        self.circuit.scan = Scan::Done;
        self.list_circuits();
    }

    /// For the screenshots: waits for the scan.
    pub(crate) fn wait_for_scan(&mut self, ctx: &egui::Context) {
        self.poll_scan(ctx);
        if let Scan::Running(receive) = &self.circuit.scan {
            let found = receive.recv().unwrap_or_default();
            self.scanned(found);
        }
    }

    /// The best laps, read again when they are stale.
    fn best_laps(&mut self) -> &LapBook {
        let dir = SaveDir::new(&self.records()).path;
        let page = &mut self.circuit;
        if page.read.is_none_or(|at| at.elapsed() > REFRESH) {
            page.book = LapBook::load(&dir).unwrap_or_default();
            page.read = Some(Instant::now());
        }
        &self.circuit.book
    }

    /// The page: the circuit, the car, the session and the conditions on the left; the previews, the best lap and Start
    /// on the right.
    pub fn circuit_page(&mut self, ui: &mut egui::Ui) {
        if self.circuit_browser.open {
            return self.browser_frame(ui, Form::circuit_browser_page);
        }
        if self.car_browser.open && self.car_browser.target == Target::Circuit {
            return self.browser_frame(ui, Form::car_browser_page);
        }
        self.poll_scan(ui.ctx());
        kit::page_header(ui, "Circuit", Some("Lap a closed course. Your best lap is kept per circuit."), |_| {});
        let circuits = self.circuits();
        let Some(&first) = circuits.first() else {
            kit::body(ui, |ui| {
                ui.label(RichText::new("No circuits installed").size(16.0).color(TEXT));
                ui.add_space(6.0);
                ui.label(
                    "A circuit is a BTB stage whose track.ini has circuit = 1 and whose drive line is a closed ring, \
                     or a stage that ends where it starts. Put one in ReRBR's tracks folder (or the RallySimFans \
                     install's RX_CONTENT/Tracks) and it is listed here.",
                );
            });
            return;
        };
        let index =
            self.circuit.map.and_then(|n| self.catalog.map_index(n)).filter(|i| circuits.contains(i)).unwrap_or(first);
        let number = self.catalog.maps[index].number;
        self.circuit.map = Some(number);
        self.best_laps();
        // The Drive page's conditions, tyre and Start work on the selected map: it is the circuit's for this page.
        let drive_map = std::mem::replace(&mut self.map, index);
        if self.circuit.checked != Some(number) {
            self.validate_weather_row();
            self.circuit.checked = Some(number);
        }
        self.circuit_body(ui);
        self.map = drive_map;
    }

    fn circuit_body(&mut self, ui: &mut egui::Ui) {
        let number = self.catalog.maps[self.map].number;
        let car = self.car_browser.row(self.car).map(|r| r.title.clone());
        let name = self
            .circuit_browser
            .row_of(number)
            .map_or_else(|| self.catalog.maps[self.map].name().to_owned(), |r| r.name.clone());
        let title = format!("{name} · {}", laps_label(self.circuit.laps));
        let summary = match self.selection() {
            Ok((_, weather)) => {
                let conditions = self.conditions_summary(&weather);
                let sub = car.map_or(conditions.clone(), |car| format!("{car} · {conditions}"));
                Summary { title, sub, problem: None, hover: None }
            }
            Err(reason) => Summary { title, sub: String::new(), problem: Some(reason), hover: None },
        };
        let blocker = self.blocker();
        if self.launch_footer(ui, "circuit footer", summary, blocker, "Start", Some("↵")) {
            self.start();
        }
        kit::body(ui, |ui| {
            let rects = split_columns(ui.available_rect_before_wrap(), 3);
            column(ui, rects[0], "circuit car", |ui| {
                self.car_column(ui, Target::Circuit);
                self.car_rows(ui);
            });
            column(ui, rects[1], "circuit circuit", |ui| self.circuit_column(ui));
            column(ui, rects[2], "circuit session", |ui| {
                kit::group_label(ui, "Session", None);
                kit::panel(ui, |rows| {
                    let tip =
                        "Free practice has no end: drive until you stop. A number of laps ends the session after the \
                               last, with the total time. Your best lap is kept either way";
                    labelled(rows, "Length", tip, |ui| {
                        combo(ui, "circuit laps", 140.0, &mut self.circuit.laps, &LAPS, laps_label).on_hover_text(tip);
                    });
                    let tip =
                        "The co-driver's calls and the note symbols. Off by default on a circuit: the notes follow a \
                               stage's road, and a lap is driven from memory";
                    labelled(rows, "Pace notes", tip, |ui| {
                        _ = kit::switch(ui, &mut self.circuit.pace_notes).on_hover_text(tip)
                    });
                    self.damage_rows(rows);
                });
                self.conditions_column(ui, "Circuit's own", true);
            });
        });
    }

    /// The Circuit column: its card, the favourites and the best lap.
    fn circuit_column(&mut self, ui: &mut egui::Ui) {
        let Some(number) = self.circuit.map else { return };
        let position = self.circuit_browser.position(number).unwrap_or(0);
        kit::group_label(ui, "Circuit", None);
        let Some(selected) = self.circuit_browser.row_of(number) else { return };
        let (name, favourite) = (selected.name.clone(), self.favourite_maps.contains(&number));
        let open = card(ui, favourite, &name, |ui, width, radius| {
            let map = &self.catalog.maps[self.map];
            match self.previews.stage_picture(ui, map, width, radius) {
                Some((meta, problem)) => CardText {
                    meta,
                    notes: problem.map(|p| (format!("Cannot load: {p}"), ERROR)).into_iter().collect(),
                },
                None => CardText::default(),
            }
        });
        if open {
            self.circuit_browser.open_for(Target::Circuit, position, &self.favourite_maps);
        }
        self.best_lap_panel(ui);
    }

    /// Makes the map with `number` the circuit, and the most recent pick.
    pub(crate) fn choose_circuit(&mut self, number: u32) {
        self.circuit.map = Some(number);
        browser::remember(&mut self.recent_maps, number);
    }

    /// The circuit browser in place of the page's body.
    fn circuit_browser_page(&mut self, ui: &mut egui::Ui) {
        let records = self.records();
        let profile = self.championship.profile.clone();
        let book = LapBook::load(&SaveDir::new(&records).path).unwrap_or_default();
        let (catalog, season) = (&self.catalog, &self.season);
        let facts = |row: &StageRow| {
            let weather = |preset| {
                let found = catalog.stage_weather(season, row.number, preset, rbr_gameplay::weather::Draws::default());
                found.map_or("none".to_owned(), |(_, w)| w.mat_words().1.to_owned())
            };
            let best = book.best(row.number, &profile).map_or("none yet".to_owned(), |b| time_label(f64::from(b.lap)));
            let mut facts = vec![("Best lap", best), ("Weather", weather(rbr_gameplay::weather::Preset::Good))];
            if row.candidate {
                facts.push(("Also", "a stage on the Drive page: it ends where it starts".to_owned()));
            }
            facts
        };
        let env = stage_browser::Env {
            maps: &self.catalog.maps,
            favourites: &self.favourite_maps,
            recent: &self.recent_maps,
            previews: &mut self.previews,
            facts: &facts,
        };
        match self.circuit_browser.ui(ui, env) {
            Some(Action::Choose(index)) => {
                let number = self.catalog.maps[index].number;
                self.choose_circuit(number);
                self.circuit_browser.open = false;
            }
            Some(Action::Favourite(number)) => match self.favourite_maps.iter().position(|&n| n == number) {
                Some(at) => drop(self.favourite_maps.remove(at)),
                None => self.favourite_maps.push(number),
            },
            Some(Action::Back) => self.circuit_browser.open = false,
            None => {}
        }
    }

    /// The best lap panel: the profile's best lap on the circuit, its sectors and the car.
    fn best_lap_panel(&self, ui: &mut egui::Ui) {
        let number = self.catalog.maps[self.map].number;
        let best = self.circuit.book.best(number, &self.championship.profile);
        kit::panel(ui, |rows| {
            let Some(best) = best else {
                rows.row("Best lap", |ui| _ = ui.label(RichText::new("No lap yet").color(TEXT_3)));
                rows.hint("Your best lap on this circuit is kept here.");
                return;
            };
            rows.row("Best lap", |ui| {
                ui.label(RichText::new(time_label(f64::from(best.lap))).font(theme::mono(14.0)).color(TEXT));
            });
            let sectors = sector_times(best);
            if sectors.len() > 1 {
                let text: Vec<String> = sectors
                    .iter()
                    .enumerate()
                    .map(|(k, t)| format!("S{} {}", k + 1, time_label(f64::from(*t))))
                    .collect();
                rows.row("Sectors", |ui| {
                    _ = ui.label(RichText::new(text.join(" · ")).font(theme::mono(11.5)).color(TEXT_2))
                });
            }
            if let Some(car) = self.catalog.cars.iter().find(|c| c.slot == best.car) {
                rows.row("Car", |ui| _ = ui.add(egui::Label::new(RichText::new(&car.name).color(TEXT_2)).truncate()));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sectors_are_the_gaps_between_the_gates_and_the_rest_of_the_lap() {
        let best = BestLap { car: 2, lap: 440.0, sectors: vec![150.0, 300.5] };
        assert_eq!(sector_times(&best), [150.0, 150.5, 139.5]);
        let one = BestLap { car: 2, lap: 61.5, sectors: Vec::new() };
        assert_eq!(sector_times(&one), [61.5]);
    }

    #[test]
    fn the_session_lengths_read_as_words() {
        let words: Vec<String> = LAPS.map(laps_label).to_vec();
        assert_eq!(words, ["Free practice", "1 lap", "3 laps", "5 laps", "10 laps", "20 laps"]);
    }

    #[test]
    fn the_page_keeps_a_listed_length_and_starts_on_free_practice_otherwise() {
        assert_eq!(CircuitPage::from_settings(Some(7), Some(5), None).laps, 5);
        assert!(!CircuitPage::from_settings(Some(7), Some(5), None).pace_notes, "off by default");
        assert_eq!(CircuitPage::from_settings(Some(7), Some(4), Some(true)).laps, 0);
        assert_eq!(CircuitPage::from_settings(None, None, None).map, None);
    }
}
