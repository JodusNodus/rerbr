//! The Championship page's drawing. With no season loaded: the profile, Continue first when it has a saved season,
//! then the new season's options and New season. With one under way: the header (the rally, rally n of 6, the step:
//! stage n of 6 or the service before it, the player's place), the card of the next step with its one primary button
//! (the stage's tyres, setup and Drive; the welcome; the service park; the rally's end), the itinerary under it (stages
//! and service stops, done, next, to come), and beside them the rally's standings, the season's points and the
//! bulletins. Starting another season or loading the saved one is folded away under the itinerary.

use std::path::Path;

use egui::{Color32, ComboBox, RichText, Ui};

use super::*;
use crate::browser::{DIM, GREY, STAR};
use crate::pages::{fields, section, start_button, GOOD};

/// A severe or lost part.
const BAD: Color32 = Color32::from_rgb(225, 95, 80);
/// A card's fill: a shade above the page's.
const CARD: Color32 = Color32::from_gray(32);
/// The tip the page opens with.
const TIP: &str = "The Controls page's driver aids and the Graphics & sound options apply to the season too.";

/// Content in a card: the fill, rounded corners and a margin, the full width.
fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .corner_radius(6.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Left-aligned, not the columns' justified layout: buttons keep their size and text its spacing.
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), add).inner
        })
        .inner
}

/// A card's title.
fn card_title(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).size(18.0).strong());
    ui.add_space(2.0);
}

/// A stage's name: the catalogue's title, else its track file.
fn stage_name(stage: &RallyStage) -> String {
    crate::catalog::title(stage.map).map_or_else(|| stage.track(), str::to_owned)
}

/// Where stage `index` stands in a rally at `phase`: its time once driven, retired, the next, or to come.
#[derive(Debug, Clone, Copy, PartialEq)]
enum StageMark {
    Done(f32),
    Retired,
    Next,
    Later,
}

fn stage_mark(c: &Championship, index: usize) -> StageMark {
    if let Some(t) = c.times().get(index) {
        return StageMark::Done(t.time);
    }
    match c.phase() {
        RallyPhase::Stage(next) if next == index => StageMark::Next,
        RallyPhase::Service(stop) if stop.next == index => StageMark::Next,
        // Only the first stage not driven: the rest were never started.
        RallyPhase::Over(RallyOutcome::Finished) => StageMark::Later,
        RallyPhase::Over(_) if index == c.times().len() => StageMark::Retired,
        _ => StageMark::Later,
    }
}

/// `n` as a place: 1st, 2nd, 3rd, 4th, 11th.
fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// `time`'s gap to the leader's `best`: blank for the leader.
fn gap(time: f32, best: f32) -> String {
    if time <= best {
        String::new()
    } else {
        format!("+{}", clock(time - best))
    }
}

impl RallyPage {
    /// The page for the season's `car` (its saved setups on a map surface: `saved`) under NGP (its `physics.rbz`,
    /// `ngp`); `car_row` draws the new season's car field for a team, and says whether that car can start.
    #[allow(clippy::too_many_arguments)]
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        data: &Data,
        season: &Season,
        here: &Path,
        car: &Car,
        saved: &dyn Fn(u32) -> Vec<(String, String)>,
        ngp: &Path,
        car_row: &mut dyn FnMut(&mut Ui, u32) -> bool,
        blocker: Option<&Issue>,
    ) -> Option<Request> {
        if let Some(e) = self.rallies.as_ref().err().or(self.ai.as_ref().err()) {
            ui.colored_label(WARN, e);
            return None;
        }
        let Some(c) = self.championship.clone() else {
            self.start_ui(ui, season, here, car_row);
            return None;
        };
        let mut request = None;
        self.header_ui(ui, car, &c);
        ui.add_space(8.0);
        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            // The next step first, so its button is always in view; the itinerary under it.
            card(ui, |ui| match c.phase() {
                RallyPhase::Stage(index) => request = self.stage_ui(ui, here, saved, &c, index, blocker),
                RallyPhase::Service(stop) if c.damage().is_none() => self.welcome_ui(ui, stop.next),
                RallyPhase::Service(stop) => {
                    if self.park.is_none() {
                        self.open_park(data, car, Some(ngp));
                    }
                    if let Some((penalty, damage)) = self.park_ui(ui, stop.next, &mut request) {
                        self.service_done(penalty, damage);
                    }
                }
                RallyPhase::Over(outcome) => request = self.over_ui(ui, season, here, outcome, blocker),
            });
            if !self.status.is_empty() {
                ui.add_space(4.0);
                ui.colored_label(WARN, &self.status);
            }
            ui.add_space(10.0);
            self.itinerary_ui(ui, &c);
            ui.add_space(10.0);
            self.seasons_ui(ui, season, here);
            self.standings_ui(&mut cols[1]);
        });
        request
    }

    /// No season loaded: Continue first when the profile has a saved season, then the new season's options and New
    /// season (which replaces it).
    fn start_ui(&mut self, ui: &mut Ui, season: &Season, here: &Path, car_row: &mut dyn FnMut(&mut Ui, u32) -> bool) {
        let named = is_file_name_part(&self.profile);
        let saved = named && self.has_saved_season(here);
        section(ui, "Championship");
        ui.colored_label(GREY, "Six rallies of six stages against the AI field, with service between stages.");
        ui.add_space(6.0);
        fields(ui, "season-profile", |ui| {
            ui.label("Profile").on_hover_text("The season is saved under this name");
            ui.horizontal(|ui| {
                self.profile_edit(ui);
                if !saved {
                    ui.colored_label(GREY, "no saved season");
                }
            });
            ui.end_row();
        });
        if saved {
            ui.add_space(6.0);
            let go = ui.horizontal(|ui| ui.add_enabled(!self.driving, start_button("Continue saved season")));
            if go.inner.clicked() {
                self.continue_season(here, season);
            }
        }
        if !self.status.is_empty() {
            ui.colored_label(WARN, &self.status);
        }
        section(ui, "New season");
        let startable = fields(ui, "season-new", |ui| self.options_rows(ui, car_row));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let enabled = !self.driving && startable && named;
            let button = if saved { egui::Button::new("New season") } else { start_button("New season") };
            if ui.add_enabled(enabled, button).clicked() {
                self.season_now(season, here);
            }
            if saved {
                ui.colored_label(GREY, format!("replaces {}'s saved season", self.profile));
            }
        });
        ui.add_space(10.0);
        ui.colored_label(DIM, TIP);
    }

    /// New season: a season on the clock's seed, saved under the profile (replacing its saved one).
    fn season_now(&mut self, season: &Season, here: &Path) {
        let store = self.store(here);
        self.new_season(season, Some(store), season_model::seed_from_unix_ms(now_ms()));
    }

    /// The profile's field: the save's name.
    fn profile_edit(&mut self, ui: &mut Ui) {
        ui.add_enabled(!self.driving, egui::TextEdit::singleline(&mut self.profile).desired_width(160.0));
        // The name is part of the save's file name: keep out what could leave the folder.
        self.profile.retain(|c| is_file_name_part(c.encode_utf8(&mut [0; 4])));
    }

    /// The options a new season takes, a row each: the game's `TEAM_OFFERS`, `SSKILL` and `CARDAM` pages, and the car
    /// (`car_row`; ours). Whether the car can start.
    fn options_rows(&mut self, ui: &mut Ui, car_row: &mut dyn FnMut(&mut Ui, u32) -> bool) -> bool {
        ui.label("Team");
        let label = |page: &Self, t: u32| {
            let car = season_model::TEAM_CARS.get(t as usize);
            let open = car.is_some_and(|c| season_model::OPEN_CARS.contains(c));
            let lock = if open { "" } else { "  (locked in a new 1.02 profile)" };
            format!("{}{lock}", page.team_label(t))
        };
        let mut team = self.team;
        combo(ui, "season-team", 300.0, &mut team, &season_model::TEAM_OFFERS, |t| label(self, t));
        self.team = team;
        ui.end_row();
        ui.label("Car");
        let startable = car_row(ui, self.team);
        ui.end_row();
        ui.label("Skill level");
        ui.horizontal(|ui| {
            combo(ui, "season-skill", 160.0, &mut self.options.skill, &Skill::ALL, Skill::label);
            ui.colored_label(GREY, format!("help when stuck: {}", assistance_note(self.options)));
        });
        ui.end_row();
        ui.label("Car damage");
        combo(ui, "season-damage", 160.0, &mut self.options.damage, &DamageLevel::ALL, DamageLevel::label);
        ui.end_row();
        startable
    }

    /// The season's header: the rally, where the rally stands and the player's place in it, then the car, team,
    /// options and the season's clock.
    fn header_ui(&self, ui: &mut Ui, car: &Car, c: &Championship) {
        let Some(run) = &self.run else { return };
        let (rally, _) = run.save.position();
        let stages = c.rally().stages.len();
        let step = match (run.turn, c.phase()) {
            (Turn::Welcome, _) => "Before the start".to_owned(),
            (_, RallyPhase::Stage(i)) => format!("Stage {} of {stages}", i + 1),
            (_, RallyPhase::Service(stop)) => format!("Service before stage {} of {stages}", stop.next + 1),
            (Turn::Over, _) => "Season over".to_owned(),
            (_, RallyPhase::Over(_)) => "Rally over".to_owned(),
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.rally_name()).size(24.0).strong());
            ui.add_space(6.0);
            let big = |text: String| RichText::new(text).size(17.0);
            ui.label(big(format!("Rally {} of {}", rally + 1, run.blocks.len())));
            ui.colored_label(DIM, big("·".into()));
            ui.label(big(step).color(STAR));
            if let Some((place, field)) = self.player_place() {
                ui.colored_label(DIM, big("·".into()));
                ui.label(big(format!("{} of {field}", ordinal(place))));
            }
        });
        let options = season_model::options(&run.save);
        let team = season_model::player_entry(&run.save).map_or(0, |e| run.save.drivers[e].team);
        let (hour, minute) = events::clock(&run.save);
        ui.colored_label(
            GREY,
            format!(
                "{} · {} · {} · {} damage · season clock {hour:02}:{minute:02}",
                car.name,
                self.team_label(team),
                options.skill.label(),
                options.damage.label(),
            ),
        );
    }

    /// The stages the AI has run in this rally: how far the rally's standings go.
    fn stages_run(&self) -> usize {
        let Some(run) = &self.run else { return 0 };
        let done = self.championship.as_ref().map_or(0, |c| c.times().len());
        let ran = match run.turn {
            Turn::Welcome => 0,
            Turn::Stage(s) => s as usize,
            Turn::Service { .. } => run.save.position().1 as usize,
            Turn::Results | Turn::BetweenRallies | Turn::Over => 6,
        };
        ran.max(done)
    }

    /// The player's place in the rally's standings and the field's size, once a stage has run and while the player
    /// is in it.
    fn player_place(&self) -> Option<(usize, usize)> {
        let run = self.run.as_ref()?;
        let ran = Some(self.stages_run()).filter(|&r| r > 0)?;
        let lines = season_model::rally_standings(&run.save, run.save.position().0 as usize, ran);
        let place = lines.iter().position(|l| run.save.drivers[l.entry].id == season_model::PLAYER_ID)?;
        lines[place].total.map(|_| (place + 1, lines.len()))
    }

    /// The rally's stages and service stops, each stage's time once driven, then the penalties and the total.
    fn itinerary_ui(&self, ui: &mut Ui, c: &Championship) {
        card(ui, |ui| {
            card_title(ui, "Itinerary");
            let at_service = matches!(c.phase(), RallyPhase::Service(_));
            egui::Grid::new("itinerary").num_columns(5).spacing([14.0, 5.0]).show(ui, |ui| {
                for heading in ["", "", "Stage", "Start", "Time"] {
                    ui.colored_label(GREY, heading);
                }
                ui.end_row();
                for (k, stage) in c.rally().stages.iter().enumerate() {
                    let mark = stage_mark(c, k);
                    if let Some(kind) = service::service_before(k).filter(|_| k > 0) {
                        let at = matches!(c.phase(), RallyPhase::Service(stop) if stop.next == k);
                        let colour = if at { STAR } else { DIM };
                        ui.colored_label(colour, if at { "▶" } else { "" });
                        ui.label("");
                        let title = text_of(self.strings.as_ref(), kind.title_key());
                        ui.colored_label(colour, format!("🔧 {title}, {} min", kind.budget() as u32 / 60));
                        ui.end_row();
                    }
                    let (icon, colour) = match mark {
                        StageMark::Done(_) => ("✔", GOOD),
                        StageMark::Retired => ("✖", WARN),
                        // At a service the arrow is on the service's row.
                        StageMark::Next if at_service => ("", STAR),
                        StageMark::Next => ("▶", STAR),
                        StageMark::Later => ("", GREY),
                    };
                    ui.colored_label(colour, icon);
                    let strong = |text: String| {
                        let text = RichText::new(text);
                        if mark == StageMark::Next {
                            text.strong().color(Color32::WHITE)
                        } else {
                            text
                        }
                    };
                    ui.label(strong(format!("SS{}", k + 1)));
                    ui.label(strong(stage_name(stage)));
                    ui.colored_label(GREY, format!("{:02}:{:02}", stage.hour, stage.minute));
                    ui.label(match mark {
                        StageMark::Done(time) => clock(time),
                        StageMark::Retired => "out".to_owned(),
                        StageMark::Next | StageMark::Later => String::new(),
                    });
                    ui.end_row();
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if c.penalty() > 0.0 {
                    ui.colored_label(BAD, format!("Penalties {}", clock(c.penalty())));
                    ui.add_space(12.0);
                }
                ui.label(RichText::new(format!("Total {}", clock(c.total_time()))).strong());
            });
        });
    }

    /// Another season, folded away: the profile, New season (which replaces the saved one) and Continue (which reads
    /// it back).
    fn seasons_ui(&mut self, ui: &mut Ui, season: &Season, here: &Path) {
        egui::CollapsingHeader::new("Another season or profile").id_salt("season-other").show(ui, |ui| {
            fields(ui, "season-other-fields", |ui| {
                ui.label("Profile").on_hover_text("The season is saved under this name");
                self.profile_edit(ui);
                ui.end_row();
            });
            ui.colored_label(GREY, "New season takes the team and options the page last had; the car is the season's.");
            ui.horizontal(|ui| {
                let named = is_file_name_part(&self.profile);
                let new = ui.add_enabled(!self.driving && named, egui::Button::new("New season"));
                if new.on_hover_text(format!("Replaces {}'s saved season", self.profile)).clicked() {
                    self.season_now(season, here);
                }
                if ui.add_enabled(!self.driving && named, egui::Button::new("Continue saved season")).clicked() {
                    self.continue_season(here, season);
                }
            });
        });
    }

    /// The rally's standings after the stages run, the season's points and the bulletins since the player's turn.
    fn standings_ui(&self, ui: &mut Ui) {
        let Some(run) = &self.run else { return };
        let rally = run.save.position().0;
        let ran = self.stages_run();
        let is_player = |entry: usize| run.save.drivers[entry].id == season_model::PLAYER_ID;
        let name = |entry: usize, text: &str| {
            let text = RichText::new(text);
            if is_player(entry) {
                text.strong().color(STAR)
            } else {
                text
            }
        };
        card(ui, |ui| {
            ui.horizontal(|ui| {
                card_title(ui, "Rally standings");
                if ran > 0 {
                    ui.colored_label(GREY, if ran == 6 { "final".to_owned() } else { format!("after SS{ran}") });
                }
            });
            let lines = season_model::rally_standings(&run.save, rally as usize, ran);
            if ran == 0 {
                ui.colored_label(GREY, "No stage run yet.");
            } else {
                let best = lines.iter().filter_map(|l| l.total).fold(f32::INFINITY, f32::min);
                // After one stage its time is the total: one column.
                let stage = (ran > 1).then(|| format!("SS{ran}"));
                let points = lines.iter().any(|l| l.points > 0);
                egui::Grid::new("rally-standings").striped(true).spacing([14.0, 3.0]).show(ui, |ui| {
                    let headings = ["", "Driver"].into_iter().chain(stage.as_deref()).chain(["Total", "Gap"]);
                    for heading in headings.chain(points.then_some("Pts")) {
                        ui.colored_label(GREY, heading);
                    }
                    ui.end_row();
                    for (place, line) in lines.iter().enumerate() {
                        ui.label(if line.total.is_some() { format!("{}", place + 1) } else { "–".into() });
                        ui.label(name(line.entry, &line.name));
                        if stage.is_some() {
                            ui.label(line.stages.last().copied().flatten().map_or("–".to_owned(), clock));
                        }
                        match line.total {
                            Some(total) => {
                                ui.label(clock(total));
                                ui.colored_label(GREY, gap(total, best));
                            }
                            None => {
                                ui.colored_label(GREY, "retired");
                                ui.label("");
                            }
                        }
                        if points {
                            ui.label(if line.points > 0 { line.points.to_string() } else { String::new() });
                        }
                        ui.end_row();
                    }
                });
            }
        });
        ui.add_space(10.0);
        let points = season_model::season_standings(&run.save);
        card(ui, |ui| {
            card_title(ui, "Season points");
            if points.iter().all(|(_, _, p)| *p == 0) {
                let scale = season_model::POINTS.map(|p| p.to_string()).join(", ");
                ui.colored_label(GREY, format!("Points come at each rally's finish: {scale}."));
                return;
            }
            egui::Grid::new("season-standings").striped(true).spacing([14.0, 3.0]).show(ui, |ui| {
                for (place, (entry, driver, points)) in points.iter().enumerate() {
                    ui.label(format!("{}", place + 1));
                    ui.label(name(*entry, driver));
                    ui.label(format!("{points}"));
                    ui.end_row();
                }
            });
        });
        if !run.news.is_empty() {
            ui.add_space(10.0);
            card(ui, |ui| {
                card_title(ui, "Bulletins");
                for line in &run.news {
                    ui.label(line);
                }
            });
        }
    }

    /// The stage's card: its start, the tyre and setup pick after a service (else the car as it is), the weather and
    /// Drive.
    fn stage_ui(
        &mut self,
        ui: &mut Ui,
        here: &Path,
        saved: &dyn Fn(u32) -> Vec<(String, String)>,
        c: &Championship,
        index: usize,
        blocker: Option<&Issue>,
    ) -> Option<Request> {
        let mut request = None;
        let stage = c.rally().stages[index];
        card_title(ui, &format!("Next: SS{} · {}", index + 1, stage_name(&stage)));
        if let Some(ev) = self.run.as_ref().filter(|r| r.turn == Turn::Stage(index as u32)) {
            if let Some(head) = ev.save.events.first() {
                let (hour, minute) = events::time_of(head);
                let start = season_model::player_start_place(&ev.save);
                let place = start.map_or(0, |p| p + 1);
                let road = ["new", "normal", "worn"][season_model::road_wear(start) as usize];
                ui.colored_label(
                    GREY,
                    format!("Start {hour:02}:{minute:02} · {} on the road · {road} surface", ordinal(place)),
                )
                .on_hover_text("The start order: the first three find a new road, from the twelfth it is worn");
            }
        }
        ui.add_space(6.0);
        let choosing = choice_before(index);
        fields(ui, "stage-fields", |ui| {
            ui.label("Tyres");
            if choosing {
                let choices = tyre_choices(crate::catalog::surface_kind(stage.map));
                let tyre = self.next_tyre(index);
                ComboBox::from_id_salt("stage-tyre")
                    .width(200.0)
                    .selected_text(crate::catalog::tyre_label(tyre))
                    .show_ui(ui, |ui| {
                        for &t in choices {
                            if ui.selectable_label(t == tyre, crate::catalog::tyre_label(t)).clicked() {
                                self.tyre = Some(t);
                            }
                        }
                    });
            } else {
                // No service before the stage: the stage loop starts it on the car as it is.
                ui.colored_label(
                    GREY,
                    format!("{}, kept: no service before", crate::catalog::tyre_label(self.last_tyre)),
                );
            }
            ui.end_row();
            ui.label("Setup");
            if choosing {
                // The car's saved setups, the stage surface's first; a pick that is gone falls back to the current one.
                let saved = saved(crate::catalog::surface_kind(stage.map));
                let rows = SeasonSetup::rows(saved.iter().map(|(id, _)| id.clone()));
                if !rows.contains(&self.setup) {
                    self.setup = SeasonSetup::Current;
                }
                let label = |row: &SeasonSetup| match row {
                    SeasonSetup::Saved(id) => {
                        saved.iter().find(|(i, _)| i == id).map_or(id.clone(), |(_, l)| l.clone())
                    }
                    _ => row.label(),
                };
                ui.horizontal(|ui| {
                    ComboBox::from_id_salt("stage-setup").width(200.0).selected_text(label(&self.setup)).show_ui(
                        ui,
                        |ui| {
                            for row in rows {
                                let selected = row == self.setup;
                                if ui.selectable_label(selected, label(&row)).clicked() {
                                    self.setup = row;
                                }
                            }
                        },
                    );
                    if ui.button("Edit…").clicked() {
                        request = Some(Request::Setup);
                    }
                });
            } else {
                ui.colored_label(GREY, "kept");
            }
            ui.end_row();
            ui.label("Weather").on_hover_text("The forecast the season drew, or the stage's good or bad weather");
            let choices = [None, Some(Preset::Good), Some(Preset::Bad)];
            combo(ui, "rally-weather", 200.0, &mut self.weather, &choices, |w| match w {
                None => "Season's forecast",
                Some(Preset::Bad) => "Bad",
                Some(_) => "Good",
            });
            ui.end_row();
        });
        ui.add_space(10.0);
        let go = ui.horizontal(|ui| {
            ui.add_enabled(!self.driving && blocker.is_none(), start_button(&format!("Drive SS{}", index + 1)))
        });
        if go.inner.clicked() {
            match self.stage_start(here, index) {
                Ok(start) => request = Some(Request::Launch(start)),
                Err(e) => self.status = e,
            }
        }
        if self.driving {
            ui.colored_label(GREY, "The stage is being driven.");
        } else if let Some(issue) = blocker {
            ui.colored_label(WARN, &issue.text);
        }
        request
    }

    /// The rally's start with a fresh car: the welcome and the recce question.
    fn welcome_ui(&mut self, ui: &mut Ui, next: usize) {
        card_title(ui, &format!("Welcome to {}", self.rally_name()));
        if let Some(c) = &self.championship {
            let stages = &c.rally().stages;
            let services = (1..stages.len()).filter(|&k| service::service_before(k).is_some()).count();
            let first = stages
                .get(next)
                .map_or(String::new(), |s| format!(", SS{} at {:02}:{:02}", next + 1, s.hour, s.minute));
            ui.colored_label(
                GREY,
                format!("{} stages, {services} service stops{first}. The car starts new.", stages.len()),
            );
        }
        ui.add_space(10.0);
        if self.recce_asked {
            // `CONTINUE_SEASON` (0x5abc20) asks before the setup choice; "Yes" goes on to it (0x5abc00).
            let text = |key| text_of(self.strings.as_ref(), key);
            let (title, info, yes, no) = (text("MSG_CRECCE"), text("INF_FORFEITRECCE"), text("YES"), text("NO"));
            ui.label(RichText::new(title).strong());
            ui.colored_label(GREY, info);
            ui.horizontal(|ui| {
                if ui.button(yes).clicked() {
                    self.recce_asked = false;
                    self.start_rally();
                }
                if ui.button(no).clicked() {
                    self.recce_asked = false;
                }
            });
        } else if ui.horizontal(|ui| ui.add(start_button("Start the rally"))).inner.clicked() {
            let welcome = self.run.as_ref().filter(|r| r.turn == Turn::Welcome);
            if welcome.is_some_and(|r| events::recce_next(&r.save.events)) {
                self.recce_asked = true;
            } else {
                self.start_rally();
            }
        }
    }

    /// The rally's end: the result, then the season's queue (the results, the trials between rallies, the season
    /// over).
    fn over_ui(
        &mut self,
        ui: &mut Ui,
        season: &Season,
        here: &Path,
        outcome: RallyOutcome,
        blocker: Option<&Issue>,
    ) -> Option<Request> {
        let mut request = None;
        let (title, detail) = match outcome {
            RallyOutcome::Finished => ("Rally finished".to_owned(), None),
            RallyOutcome::Wrecked { stage } => {
                // Page 0x18 (FAILED, `CFailedSeasonFinishedWithBadCarUI`): the text by the season's queue.
                let key = match self.next_event(stage) {
                    NextEvent::Service => "MSG_UNFINISH_SERVICE",
                    _ => "MSG_UNFINISH_STAGE",
                };
                let text = text_of(self.strings.as_ref(), key);
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                (format!("Out of the rally after SS{}", stage + 1), Some(text))
            }
            RallyOutcome::Retired { stage } => (format!("Retired on SS{}", stage + 1), None),
        };
        card_title(ui, &title);
        if let Some(detail) = detail {
            ui.colored_label(GREY, detail);
        }
        if let Some((place, field)) = self.player_place() {
            ui.label(RichText::new(format!("You finished {} of {field}", ordinal(place))).size(16.0).color(STAR));
        }
        let turn = self.run.as_ref().map(|r| r.turn);
        if turn == Some(Turn::Over) {
            ui.add_space(6.0);
            ui.label(RichText::new("The season is over.").size(16.0).strong());
            if let Some((place, points)) = self.run.as_ref().and_then(|run| {
                let lines = season_model::season_standings(&run.save);
                let place = lines.iter().position(|l| run.save.drivers[l.0].id == season_model::PLAYER_ID)?;
                Some((place + 1, lines[place].2))
            }) {
                ui.label(format!("You are {} in the season with {points} points.", ordinal(place)));
            }
        }
        ui.add_space(10.0);
        match turn {
            Some(Turn::Results) => {
                if ui.horizontal(|ui| ui.add(start_button("Continue"))).inner.clicked() {
                    self.turn_done();
                }
            }
            Some(Turn::Over) => {
                // The fold's New season, brought up: the only way on.
                let new = ui.horizontal(|ui| {
                    let new =
                        ui.add_enabled(!self.driving && is_file_name_part(&self.profile), start_button("New season"));
                    ui.colored_label(GREY, format!("replaces {}'s saved season", self.profile));
                    new
                });
                if new.inner.clicked() {
                    self.season_now(season, here);
                }
            }
            Some(Turn::BetweenRallies) => {
                // Page 0x19 (0x5acf30): Continue, the two trials (the car tune and the season data are the
                // Setup page and the standings beside).
                let next = self.run.as_ref().map_or(0, |r| r.save.position().0 as usize + 1);
                let label = format!("Next rally: {}", self.rally_name_at(next));
                if ui.horizontal(|ui| ui.add_enabled(!self.driving, start_button(&label))).inner.clicked() {
                    self.turn_done();
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.colored_label(GREY, "Practice first (not recorded):");
                    for (label, map, tip) in [
                        ("Yard trial", 72, "A practice drive in the yard; the season does not record it"),
                        ("Stage trial", 71, "A practice stage; the season does not record it"),
                    ] {
                        let trial = ui.add_enabled(!self.driving && blocker.is_none(), egui::Button::new(label));
                        let trial = trial.on_hover_text(tip);
                        if trial.clicked() {
                            match self.trial(season, map) {
                                Ok(start) => {
                                    (self.driving, self.practice) = (true, true);
                                    request = Some(Request::Launch(start));
                                }
                                Err(e) => self.status = e,
                            }
                        }
                    }
                });
                if let Some(issue) = blocker.filter(|_| !self.driving) {
                    ui.colored_label(WARN, &issue.text);
                }
            }
            _ => {}
        }
        request
    }

    /// The park, as `CSelectPartsUI` and the timer show it: the allowed and estimated times, the damaged parts to
    /// order, the manager's advice, Repair! (with no orders, `MSG_NOREP` asks to leave), selecting during the repairs
    /// to finish them now, and the closing message. Returns the penalty and the car when closed and confirmed.
    fn park_ui(&mut self, ui: &mut Ui, next: usize, request: &mut Option<Request>) -> Option<(f32, CarriedDamage)> {
        let park = self.park.as_mut()?;
        let strings = self.strings.as_ref();
        let text = |key: &str| text_of(strings, key);
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        if !self.forfeit_asked {
            park.tick(dt);
        }
        if !matches!(park.state, ParkState::Closed { .. }) {
            ui.ctx().request_repaint();
        }
        card_title(ui, &format!("{} before SS{}", text(park.kind.title_key()), next + 1));
        // The clock that matters first, big; what the orders take beside it, red past the time allowed.
        let over = park.estimate() > park.time_left;
        ui.horizontal(|ui| {
            ui.colored_label(GREY, "Time left");
            ui.label(RichText::new(clock(park.time_left)).size(20.0).strong());
            ui.colored_label(DIM, format!("of {}", clock(park.kind.budget())));
            ui.add_space(14.0);
            ui.colored_label(GREY, text("SP_ESTIM").trim_end_matches([':', ' ']));
            ui.label(RichText::new(clock(park.estimate())).strong().color(if over { BAD } else { Color32::WHITE }));
        });
        if over && park.state == ParkState::Choosing {
            ui.colored_label(BAD, "The orders take longer than the time left: expect a late penalty.");
        }
        ui.add_space(6.0);
        let choosing = park.state == ParkState::Choosing && !self.forfeit_asked;
        let rows = park.rows();
        if rows.is_empty() {
            ui.colored_label(GREY, "Nothing is damaged.");
        } else if choosing {
            ui.colored_label(GREY, "Tick the parts to repair, then Repair!");
        }
        egui::Grid::new("park-parts").striped(true).spacing([14.0, 3.0]).show(ui, |ui| {
            if !rows.is_empty() {
                for heading in ["Part", "Damage", "Repair time"] {
                    ui.colored_label(GREY, heading);
                }
                ui.end_row();
            }
            for i in rows {
                let mut on = park.covering(i).is_some();
                let level = park.level(i);
                let part = ui.add_enabled(choosing, egui::Checkbox::new(&mut on, text_of(strings, &park.tree[i].name)));
                if part.changed() {
                    park.toggle(i);
                }
                let colour = match level {
                    0 => GREY,
                    1 | 2 => STAR,
                    _ => BAD,
                };
                ui.colored_label(colour, text(service::damage_key(level)));
                ui.colored_label(GREY, clock(park.time_of(i)));
                ui.end_row();
            }
        });
        ui.add_space(10.0);
        let mut done = None;
        if self.forfeit_asked {
            ui.label(text("MSG_NOREP"));
            ui.horizontal(|ui| {
                if ui.button(text("YES")).clicked() {
                    park.forfeit();
                    self.forfeit_asked = false;
                }
                if ui.button(text("NO")).clicked() {
                    self.forfeit_asked = false;
                }
            });
            return None;
        }
        ui.horizontal(|ui| match park.state {
            ParkState::Choosing => {
                if ui.add(start_button(&text("SP_REPAIR"))).clicked() && !park.start() {
                    self.forfeit_asked = true;
                }
                if ui.button(text("SP_MANADV")).on_hover_text("Order what the team manager would").clicked() {
                    park.manager_advice();
                }
                if ui.button("Setup…").clicked() {
                    *request = Some(Request::Setup);
                }
            }
            ParkState::Repairing => {
                ui.label(format!("{}…", text("SP_STATUS")));
                if ui.button("Finish now").clicked() {
                    park.finish_now();
                }
            }
            ParkState::Closed { penalty, end } => {
                ui.vertical(|ui| {
                    if let Some(key) = service::end_message(end, penalty) {
                        ui.label(text(key).replace("%1.f", &format!("{penalty:.0}")));
                    }
                    ui.colored_label(GREY, format!("{}{penalty:.0} s", text("SP_PEN")));
                    if ui.horizontal(|ui| ui.add(start_button("Continue"))).inner.clicked() {
                        done = Some((penalty, park.damage.clone()));
                    }
                });
            }
        });
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gap_is_blank_for_the_leader() {
        assert_eq!(gap(300.0, 300.0), "");
        assert_eq!(gap(312.5, 300.0), "+0:12.50");
    }

    #[test]
    fn places_read_as_ordinals() {
        let places: Vec<_> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 103].map(ordinal).into();
        assert_eq!(places, ["1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "103rd"]);
    }
}
