//! Content > Stages: every stage, its details, the stages added and their removal. The stage browser as a page
//! (`StageBrowser::start_page`), over every map, circuits too.

use std::path::Path;

use rbr_formats::saves::SaveDir;
use rbr_gameplay::weather::{Draws, Preset};
use rbr_session::LapBook;

use crate::pages::Page;
use crate::stage_browser::{self, Action, StageBrowser, StageRow};
use crate::Form;

/// The Stages page's state.
#[derive(Default)]
pub struct StagesPage {
    browser: StageBrowser,
}

/// Deletes the added stage's `folder`, which must be a folder directly in `tracks` (`<files>/tracks/`): never
/// anything else.
fn remove_added(tracks: &Path, folder: &Path) -> Result<(), String> {
    let inside = match (tracks.canonicalize(), folder.canonicalize()) {
        (Ok(tracks), Ok(folder)) => folder.parent() == Some(tracks.as_path()),
        _ => false,
    };
    if !inside {
        return Err(format!("{} is not a stage added to your files", folder.display()));
    }
    std::fs::remove_dir_all(folder).map_err(|e| format!("couldn't remove {}: {e}", folder.display()))
}

impl Form {
    /// Whether map `index` is a stage added to `<files>/tracks/`.
    fn is_added(&self, index: usize) -> bool {
        let tracks = self.here().join(rbr_btb::player::TRACKS);
        let folder =
            self.catalog.maps.get(index).and_then(|m| m.community().map(|s| &s.folder).or(m.btb().map(|b| &b.folder)));
        folder.is_some_and(|f| f.starts_with(tracks))
    }

    /// Lists every map for the Stages page: the circuits too, each with its reason not to start.
    pub fn list_stages_page(&mut self) {
        let rows = (self.catalog.maps.iter().enumerate())
            .map(|(i, map)| {
                let mut row = StageRow::new(i, map, self.catalog.availability(&self.season, map).err());
                row.added = self.is_added(i);
                row.circuit = self.is_circuit(i);
                row
            })
            .collect();
        self.stages_page.browser.set_rows(rows);
    }

    /// Every stage: its details, the stages added and their removal.
    pub fn stages_page(&mut self, ui: &mut egui::Ui) {
        self.stages_page.browser.start_page(self.map);
        let added = (0..self.catalog.maps.len()).filter(|&i| self.is_added(i)).count();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{} stages · {added} added by you", self.catalog.maps.len()))
                    .size(16.5)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Get more stages…").clicked() {
                    self.page = Page::Get;
                }
            });
        });
        ui.add_space(6.0);
        let records = self.records();
        let profile = self.championship.profile.clone();
        let (catalog, season) = (&self.catalog, &self.season);
        let facts = |row: &StageRow| {
            let weather = |preset| {
                let found = catalog.stage_weather(season, row.number, preset, Draws::default());
                found.map_or("none".to_owned(), |(_, w)| w.mat_words().1.to_owned())
            };
            let (good, bad) = (weather(Preset::Good), weather(Preset::Bad));
            let mut facts = vec![("Weather", if good == bad { good } else { format!("good {good} · bad {bad}") })];
            if row.circuit {
                let book = LapBook::load(&SaveDir::new(&records).path).unwrap_or_default();
                let best = book.best(row.number, &profile);
                facts.push((
                    "Best lap",
                    best.map_or("none yet".to_owned(), |b| rbr_render::hud::time_label(f64::from(b.lap))),
                ));
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
        match self.stages_page.browser.ui(ui, env) {
            Some(Action::Choose(index)) => {
                if self.is_circuit(index) {
                    self.choose_circuit(self.catalog.maps[index].number);
                    self.page = Page::Circuit;
                } else {
                    self.choose_stage(index);
                    self.page = Page::Drive;
                }
            }
            Some(Action::Favourite(number)) => match self.favourite_maps.iter().position(|&n| n == number) {
                Some(at) => drop(self.favourite_maps.remove(at)),
                None => self.favourite_maps.push(number),
            },
            Some(Action::Back) | None => {}
        }
        if let Some(index) = self.stages_page.browser.removal.take() {
            self.remove_stage(index);
        }
    }

    /// Removes the added stage at `index` of the catalog: its folder in `<files>/tracks/`, then the catalog and the
    /// lists without it. Its records and ghosts stay.
    fn remove_stage(&mut self, index: usize) {
        let Some(folder) = (self.is_added(index))
            .then(|| self.catalog.maps[index].clone())
            .and_then(|m| m.community().map(|s| s.folder.clone()).or(m.btb().map(|b| b.folder.clone())))
        else {
            return;
        };
        if let Err(e) = remove_added(&self.here().join(rbr_btb::player::TRACKS), &folder) {
            self.status = e;
            return;
        }
        let selected = self.catalog.maps[self.map].number;
        self.catalog
            .maps
            .retain(|m| m.community().is_none_or(|s| s.folder != folder) && m.btb().is_none_or(|b| b.folder != folder));
        self.map = self.catalog.maps.iter().position(|m| m.number == selected).unwrap_or(0);
        self.list_maps();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_folder_directly_in_tracks_is_removed() {
        let dir = crate::test_dir("stages-remove");
        let tracks = dir.join("tracks");
        let stage = tracks.join("9001-Mine");
        std::fs::create_dir_all(stage.join("sub")).unwrap();
        std::fs::write(stage.join("sub/file"), "x").unwrap();
        let outside = dir.join("Maps").join("300-Theirs");
        std::fs::create_dir_all(&outside).unwrap();
        assert!(remove_added(&tracks, &outside).is_err());
        assert!(remove_added(&tracks, &tracks).is_err());
        assert!(remove_added(&tracks, &tracks.join("sub").join("..").join("Maps")).is_err());
        assert!(outside.exists() && tracks.exists());
        remove_added(&tracks, &stage).unwrap();
        assert!(!stage.exists() && tracks.exists());
        assert!(remove_added(&tracks, &stage).is_err(), "gone already");
    }
}
