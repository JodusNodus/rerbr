//! The builder's itinerary: the seed's plan for the settings, shown before Start, and the player's own once edited.
//! Each rally's legs list their stages; a stage can be changed (through the stage browser), moved within its leg or
//! removed, stages and legs added, rallies added, removed and renamed. Start empty clears it to one empty rally to
//! build from scratch; Draw again goes back to the seed's plan.

use egui::{Button, RichText, TextEdit, Ui};
use rbr_season::config::{EventConfig, Length, StagePool, StageRef, Surfaces};
use rbr_season::event::{rally_name, Event, RallyPlan};

use super::common::card;
use super::pool::surface_word;
use super::view::km;
use crate::browser::GREY;
use crate::pages::WARN;

/// The settings a plan was drawn for, and the pool's size.
type Drawn = (Length, StagePool, Surfaces, u32, usize);

/// The most legs a rally takes: a WRC round runs over three or four days.
const MOST_LEGS: usize = 4;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub rallies: Vec<RallyPlan>,
    /// The settings the seed drew it for.
    drawn: Option<Drawn>,
    /// The player edited it: the settings no longer draw it again.
    own: bool,
    /// Why the seed could not draw one.
    error: Option<String>,
}

/// Where the stage browser's pick goes: the leg's stage `at`, or a new stage at the leg's end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub rally: usize,
    pub leg: usize,
    pub at: Option<usize>,
}

impl Plan {
    /// The seed's plan for `config` on `stages`, drawn again when they changed, unless the plan is the player's own.
    pub fn follow(&mut self, config: &EventConfig, stages: &[StageRef]) {
        let drawn = (config.length, config.pool, config.surfaces, config.seed, stages.len());
        if !self.own && self.drawn != Some(drawn) {
            self.draw(config, stages);
        }
    }

    /// Draws the seed's plan again, dropping the player's edits.
    pub fn draw(&mut self, config: &EventConfig, stages: &[StageRef]) {
        let drawn = Event::plan(config, stages);
        (self.rallies, self.error) = match drawn {
            Ok(rallies) => (rallies, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        self.drawn = Some((config.length, config.pool, config.surfaces, config.seed, stages.len()));
        self.own = false;
    }

    pub fn own(&self) -> bool {
        self.own
    }

    /// Every rally has a leg and every leg a stage.
    pub fn ready(&self) -> bool {
        !self.rallies.is_empty()
            && self.rallies.iter().all(|r| !r.legs.is_empty() && r.legs.iter().all(|l| !l.is_empty()))
    }

    /// `stage` put at `slot`; a rally without a name takes its first stage's.
    pub fn put(&mut self, slot: Slot, stage: StageRef) {
        let Some(rally) = self.rallies.get_mut(slot.rally) else { return };
        let Some(leg) = rally.legs.get_mut(slot.leg) else { return };
        match slot.at.filter(|&k| k < leg.len()) {
            Some(k) => leg[k] = stage,
            None => leg.push(stage),
        }
        if rally.name.trim().is_empty() {
            rally.name = rally_name(&rally.legs.concat());
        }
        self.own = true;
    }
}

/// What a click on a row asks.
enum Edit {
    Up(usize, usize, usize),
    Down(usize, usize, usize),
    Remove(usize, usize, usize),
    RemoveLeg(usize, usize),
    AddLeg(usize),
    RemoveRally(usize),
    AddRally,
    Clear,
}

/// The itinerary: each rally's card. The slot to pick a stage for, when the player asked for the stage browser.
pub fn plan_ui(ui: &mut Ui, plan: &mut Plan, config: &EventConfig, stages: &[StageRef]) -> Option<Slot> {
    if let Some(e) = &plan.error {
        ui.colored_label(WARN, format!("The stages cannot be drawn: {e}"));
    }
    let mut browse = None;
    let mut edit = None;
    let single = plan.rallies.len() == 1;
    for (r, rally) in plan.rallies.iter_mut().enumerate() {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                let name = ui.add(TextEdit::singleline(&mut rally.name).desired_width(260.0).hint_text("Rally name"));
                if name.changed() {
                    plan.own = true;
                }
                let all: Vec<&StageRef> = rally.legs.iter().flatten().collect();
                let length: f32 = all.iter().map(|s| s.length).sum();
                let mut surfaces: Vec<&str> = all.iter().map(|s| surface_word(s.surface)).collect();
                surfaces.dedup();
                let surface = if surfaces.len() == 1 { surfaces[0] } else { "mixed surfaces" };
                let count = all.len();
                let line = format!("{count} stage{} · {} · {surface}", if count == 1 { "" } else { "s" }, km(length));
                ui.colored_label(GREY, line);
                if !single && ui.link("Remove rally").clicked() {
                    edit = Some(Edit::RemoveRally(r));
                }
            });
            ui.add_space(4.0);
            let mut number = 0;
            let legs = rally.legs.len();
            egui::Grid::new(("championship-plan", r)).num_columns(5).spacing([12.0, 4.0]).show(ui, |ui| {
                for (l, leg) in rally.legs.iter().enumerate() {
                    if legs > 1 {
                        ui.label(RichText::new(format!("Leg {}", l + 1)).strong());
                        if ui.small_button("Remove leg").clicked() {
                            edit = Some(Edit::RemoveLeg(r, l));
                        }
                        ui.end_row();
                    }
                    for (k, stage) in leg.iter().enumerate() {
                        number += 1;
                        ui.colored_label(GREY, format!("SS{number}"));
                        ui.label(&stage.name);
                        ui.colored_label(GREY, km(stage.length));
                        ui.colored_label(GREY, surface_word(stage.surface));
                        ui.horizontal(|ui| {
                            if ui.add_enabled(k > 0, Button::new("⬆").small()).on_hover_text("Earlier").clicked() {
                                edit = Some(Edit::Up(r, l, k));
                            }
                            let later = ui.add_enabled(k + 1 < leg.len(), Button::new("⬇").small());
                            if later.on_hover_text("Later").clicked() {
                                edit = Some(Edit::Down(r, l, k));
                            }
                            if ui.small_button("Change…").clicked() {
                                browse = Some(Slot { rally: r, leg: l, at: Some(k) });
                            }
                            if ui.small_button("Remove").clicked() {
                                edit = Some(Edit::Remove(r, l, k));
                            }
                        });
                        ui.end_row();
                    }
                    ui.label("");
                    ui.horizontal(|ui| {
                        if ui.link("Add a stage…").clicked() {
                            browse = Some(Slot { rally: r, leg: l, at: None });
                        }
                        if l + 1 == legs && legs < MOST_LEGS && ui.link("Add a leg").clicked() {
                            edit = Some(Edit::AddLeg(r));
                        }
                        if leg.is_empty() {
                            ui.colored_label(WARN, "A leg needs a stage.");
                        }
                    });
                    ui.end_row();
                }
            });
        });
        ui.add_space(8.0);
    }
    ui.horizontal(|ui| {
        if ui.button("Add a rally").on_hover_text("Its stages from the stage browser").clicked() {
            edit = Some(Edit::AddRally);
        }
        if ui.button("Start empty").on_hover_text("One empty rally, to build your own from scratch").clicked() {
            edit = Some(Edit::Clear);
        }
        if plan.own {
            if ui.button("Draw again").clicked() {
                plan.draw(config, stages);
            }
            ui.colored_label(GREY, "Your own itinerary: the length, stages and seed apply when you draw again.");
        }
    });
    if let Some(edit) = edit {
        apply(&mut plan.rallies, edit);
        plan.own = true;
    }
    browse
}

fn apply(rallies: &mut Vec<RallyPlan>, edit: Edit) {
    match edit {
        Edit::Up(r, l, k) => rallies[r].legs[l].swap(k - 1, k),
        Edit::Down(r, l, k) => rallies[r].legs[l].swap(k, k + 1),
        Edit::Remove(r, l, k) => _ = rallies[r].legs[l].remove(k),
        Edit::RemoveLeg(r, l) => _ = rallies[r].legs.remove(l),
        Edit::AddLeg(r) => rallies[r].legs.push(Vec::new()),
        Edit::RemoveRally(r) => _ = rallies.remove(r),
        Edit::AddRally => rallies.push(RallyPlan { name: String::new(), legs: vec![Vec::new()] }),
        Edit::Clear => *rallies = vec![RallyPlan { name: String::new(), legs: vec![Vec::new()] }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbr_season::config::{Preset, Surface};

    fn stage(key: &str) -> StageRef {
        let (name, family) = (key.to_owned(), key.to_owned());
        StageRef { key: key.into(), name, family, surface: Surface::Gravel, community: false, length: 9000.0 }
    }

    #[test]
    fn the_plan_follows_the_settings_until_it_is_the_players_own() {
        let stages: Vec<StageRef> = (0..12).map(|k| stage(&format!("s{k}"))).collect();
        let mut config = Preset::QuickRally.config("Rally2", "car", 1);
        let mut plan = Plan::default();
        plan.follow(&config, &stages);
        assert!(plan.ready() && !plan.own());
        assert_eq!(plan.rallies[0].legs[0].len(), 6);
        config.length.stages = 4;
        plan.follow(&config, &stages);
        assert_eq!(plan.rallies[0].legs[0].len(), 4, "drawn again for the new length");
        // A new rally: not ready until its leg has a stage, named after its first stage.
        apply(&mut plan.rallies, Edit::AddRally);
        plan.own = true;
        assert!(!plan.ready());
        plan.put(Slot { rally: 1, leg: 0, at: None }, stage("Kaatselkä"));
        assert!(plan.ready());
        assert_eq!(plan.rallies[1].name, "Kaatselkä Rally");
        // The player's own stays put as the settings change, until Draw again.
        config.length.stages = 6;
        plan.follow(&config, &stages);
        assert_eq!(plan.rallies.len(), 2);
        plan.put(Slot { rally: 0, leg: 0, at: Some(0) }, stage("own"));
        assert_eq!(plan.rallies[0].legs[0][0].key, "own");
        plan.draw(&config, &stages);
        assert!(!plan.own() && plan.rallies.len() == 1 && plan.rallies[0].legs[0].len() == 6);
        // From scratch: one empty rally, not ready until it has a stage.
        apply(&mut plan.rallies, Edit::Clear);
        plan.own = true;
        assert!(plan.rallies.len() == 1 && !plan.ready());
        plan.put(Slot { rally: 0, leg: 0, at: None }, stage("Falstone"));
        assert!(plan.ready() && plan.rallies[0].name == "Falstone Rally");
    }
}
