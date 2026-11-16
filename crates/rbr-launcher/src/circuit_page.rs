//! The Circuit page: laps on a circuit (`notes/session.md`), the hotlapping twin of the Drive page. The circuits are the
//! BTB stages marked `circuit = 1` (`btb::Btb::circuit`). It takes the Drive page's car, setup, weather and tyre; what
//! is its own is the circuit's list with the profile's best laps, the session's length (free practice or N laps) and
//! the best lap's sector times. Start runs `rbr-standalone --laps` on the circuit and the best laps are kept in the
//! save folder (`rbr_session::book`).

use std::time::{Duration, Instant};

use egui::{Color32, RichText};
use rbr_formats::saves::SaveDir;
use rbr_render::hud::time_label;
use rbr_session::{BestLap, LapBook};

use crate::car_browser::Target;
use crate::catalog::Map;
use crate::pages::{combo, fields, section, DRIVE_FIELDS, FIELD};
use crate::stage_browser::SURFACES;
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
    /// The best laps in the save folder and when they were read.
    book: LapBook,
    read: Option<Instant>,
    /// The circuit whose conditions were last checked against the weather rows.
    checked: Option<u32>,
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
    pub fn from_settings(map: Option<u32>, laps: Option<u32>) -> Self {
        Self { map, laps: laps.filter(|l| LAPS.contains(l)).unwrap_or(0), ..Self::default() }
    }
}

impl Form {
    /// The catalog's circuits, by their index.
    pub fn circuits(&self) -> Vec<usize> {
        (0..self.catalog.maps.len()).filter(|&i| self.catalog.maps[i].btb().is_some_and(|btb| btb.circuit)).collect()
    }

    /// Whether map `index` is a circuit: it plays on the Circuit page.
    pub fn is_circuit(&self, index: usize) -> bool {
        self.catalog.maps.get(index).and_then(Map::btb).is_some_and(|btb| btb.circuit)
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
        if self.car_browser.open && self.car_browser.target == Target::Circuit {
            return self.car_browser_page(ui);
        }
        let circuits = self.circuits();
        let Some(&first) = circuits.first() else {
            ui.add_space(20.0);
            ui.label(RichText::new("No circuits installed").size(18.0));
            ui.add_space(6.0);
            ui.label(
                "A circuit is a BTB stage whose track.ini has circuit = 1 and whose drive line is a closed ring. \
                 Put one in the RallySimFans install's RX_CONTENT/Tracks and it is listed here.",
            );
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
        self.circuit_body(ui, &circuits);
        self.map = drive_map;
    }

    fn circuit_body(&mut self, ui: &mut egui::Ui, circuits: &[usize]) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(DRIVE_FIELDS);
                egui::ScrollArea::vertical().id_salt("circuit").show(ui, |ui| self.circuit_fields(ui, circuits));
            });
            ui.add_space(20.0);
            ui.vertical(|ui| {
                let root = self.game_root();
                if let Some(car) = self.catalog.cars.get(self.car) {
                    self.previews.ui(ui, &self.catalog.maps[self.map], car, &root);
                }
                ui.add_space(12.0);
                self.best_lap_card(ui);
                ui.add_space(12.0);
                self.start_area(ui);
            });
        });
    }

    fn circuit_fields(&mut self, ui: &mut egui::Ui, circuits: &[usize]) {
        section(ui, "Circuit");
        self.circuit_list(ui, circuits);

        section(ui, "Car");
        fields(ui, "circuit car", |ui| {
            ui.label("Car");
            if self.catalog.cars.is_empty() {
                ui.colored_label(crate::pages::WARN, "No cars yet: add some on the Settings page with Download cars…");
            } else {
                ui.vertical(|ui| self.car_field(ui, Target::Circuit));
            }
            ui.end_row();
            if self.bindings.shifter.advanced {
                self.gearbox_row(ui);
            }
            self.setup_row(ui);
        });

        section(ui, "Session");
        fields(ui, "circuit session", |ui| {
            let tip = "Free practice has no end: drive until you stop. A number of laps ends the session after the \
                       last, with the total time. Your best lap is kept either way";
            ui.label("Length").on_hover_text(tip);
            combo(ui, "circuit laps", FIELD, &mut self.circuit.laps, &LAPS, laps_label);
            ui.end_row();
            self.damage_row(ui);
        });

        self.environment_section(ui);
        self.road_section(ui);
    }

    /// The circuits, one row each: its name, length and surface, and the profile's best lap.
    fn circuit_list(&mut self, ui: &mut egui::Ui, circuits: &[usize]) {
        let profile = self.championship.profile.clone();
        let rows: Vec<(u32, String, String, Option<f32>)> = circuits
            .iter()
            .map(|&i| {
                let map = &self.catalog.maps[i];
                let btb = map.btb();
                let km = btb.and_then(|b| b.km).map(|km| format!("{km:.1} km · ")).unwrap_or_default();
                let surface = SURFACES[map.surface().min(2) as usize];
                let best = self.circuit.book.best(map.number, &profile).map(|b| b.lap);
                (map.number, map.name(), format!("{km}{surface}"), best)
            })
            .collect();
        for (number, name, detail, best) in rows {
            let selected = self.circuit.map == Some(number);
            let mut label = format!("{name}   {detail}");
            if let Some(lap) = best {
                label += &format!("   ·   best {}", time_label(f64::from(lap)));
            }
            if ui.selectable_label(selected, label).clicked() {
                self.circuit.map = Some(number);
            }
        }
    }

    /// The best lap card: the profile's best lap on the circuit, its sectors and the car.
    fn best_lap_card(&self, ui: &mut egui::Ui) {
        let number = self.catalog.maps[self.map].number;
        let best = self.circuit.book.best(number, &self.championship.profile);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(crate::preview::WIDTH - 20.0);
            let Some(best) = best else {
                ui.weak("No lap yet: your best lap on this circuit is kept here");
                return;
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(time_label(f64::from(best.lap))).size(26.0).strong());
                ui.weak("best lap");
            });
            let sectors = sector_times(best);
            if sectors.len() > 1 {
                ui.horizontal_wrapped(|ui| {
                    for (k, time) in sectors.iter().enumerate() {
                        ui.colored_label(Color32::GRAY, format!("S{} {}", k + 1, time_label(f64::from(*time))));
                    }
                });
            }
            if let Some(car) = self.catalog.cars.iter().find(|c| c.slot == best.car) {
                ui.weak(format!("in {}", car.name));
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
        assert_eq!(CircuitPage::from_settings(Some(7), Some(5)).laps, 5);
        assert_eq!(CircuitPage::from_settings(Some(7), Some(4)).laps, 0);
        assert_eq!(CircuitPage::from_settings(None, None).map, None);
    }
}
