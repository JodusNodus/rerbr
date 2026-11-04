//! The Championship page's drawing: the season's start, the rally under way and its next step, the standings.

use std::path::Path;

use egui::{Color32, ComboBox, Ui};

use super::*;

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
        if let Err(e) = &self.rallies {
            ui.colored_label(WARN, e);
            return None;
        }
        let mut request = None;
        if let Err(e) = &self.ai {
            ui.colored_label(WARN, e);
            return None;
        }
        let startable = self.options_ui(ui, car_row);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Profile");
            ui.add_enabled(!self.driving, egui::TextEdit::singleline(&mut self.profile).desired_width(100.0));
            // The name is part of the save's file name: keep out what could leave the folder.
            self.profile.retain(|c| is_file_name_part(c.encode_utf8(&mut [0; 4])));
            let named = is_file_name_part(&self.profile);
            ui.label("Weather").on_hover_text("The forecast the season drew, or the stage's good or bad weather");
            let choices = [None, Some(Preset::Good), Some(Preset::Bad)];
            combo(ui, "rally-weather", 120.0, &mut self.weather, &choices, |w| match w {
                None => "Season's weather",
                Some(Preset::Bad) => "Bad",
                Some(_) => "Good",
            });
            if ui.add_enabled(!self.driving && startable && named, egui::Button::new("New season")).clicked() {
                let store = self.store(here);
                self.new_season(season, Some(store), season_model::seed_from_unix_ms(now_ms()));
            }
            if ui.add_enabled(!self.driving && named, egui::Button::new("Continue")).clicked() {
                self.continue_season(here, season);
            }
        });
        let Some(c) = self.championship.clone() else {
            if !self.status.is_empty() {
                ui.label(&self.status);
            }
            return None;
        };
        if let Some(run) = &self.run {
            let options = season_model::options(&run.save);
            let team = self.team_label(season_model::player_entry(&run.save).map_or(0, |e| run.save.drivers[e].team));
            let (hour, minute) = events::clock(&run.save);
            ui.label(
                egui::RichText::new(format!(
                    "This season: {}, {team}, {}, car damage {}   Clock {hour:02}:{minute:02}",
                    car.name,
                    options.skill.label(),
                    options.damage.label()
                ))
                .color(Color32::GRAY),
            );
        }
        ui.add_space(8.0);
        egui::Grid::new("stages").num_columns(4).spacing([16.0, 4.0]).show(ui, |ui| {
            for (k, stage) in c.rally().stages.iter().enumerate() {
                ui.label(format!("SS{}", k + 1));
                ui.label(crate::catalog::title(stage.map).map_or_else(|| stage.track(), str::to_owned));
                ui.label(format!("{:02}:{:02}", stage.hour, stage.minute));
                ui.label(c.times().get(k).map_or("–".to_owned(), |t| clock(t.time)));
                ui.end_row();
            }
        });
        ui.label(format!("Penalties {}   Total {}", clock(c.penalty()), clock(c.total_time())));
        ui.separator();
        match c.phase() {
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
            RallyPhase::Over(outcome) => request = self.over_ui(ui, season, outcome, blocker),
        }
        self.season_ui(ui);
        if !self.status.is_empty() {
            ui.label(&self.status);
        }
        request
    }

    /// The options a new season takes, one field each: the game's `TEAM_OFFERS`, `SSKILL` and `CARDAM` pages, and the
    /// car (`car_row`; ours). Whether the car can start.
    fn options_ui(&mut self, ui: &mut Ui, car_row: &mut dyn FnMut(&mut Ui, u32) -> bool) -> bool {
        ui.strong("New season");
        egui::Grid::new("championship-options")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Team");
                let label = |page: &Self, t: u32| {
                    let car = season_model::TEAM_CARS.get(t as usize);
                    let open = car.is_some_and(|c| season_model::OPEN_CARS.contains(c));
                    let lock = if open { "" } else { "  (locked in a new 1.02 profile)" };
                    format!("{}{lock}", page.team_label(t))
                };
                let mut team = self.team;
                combo(ui, "season-team", 200.0, &mut team, &season_model::TEAM_OFFERS, |t| label(self, t));
                self.team = team;
                ui.end_row();
                ui.label("Car");
                let startable = car_row(ui, self.team);
                ui.end_row();
                ui.label("Skill Level");
                let skill = &mut self.options.skill;
                combo(ui, "season-skill", 200.0, skill, &Skill::ALL, Skill::label);
                ui.end_row();
                ui.label("Car Damage");
                let damage = &mut self.options.damage;
                combo(ui, "season-damage", 200.0, damage, &DamageLevel::ALL, DamageLevel::label);
                ui.end_row();
                ui.label("Outside assistance");
                ui.label(egui::RichText::new(assistance_note(self.options)).color(Color32::GRAY));
                ui.end_row();
                startable
            })
            .inner
    }

    /// The rally's standings after the stages done, the season's points and the last stage's bulletins.
    fn season_ui(&self, ui: &mut Ui) {
        let Some(run) = &self.run else { return };
        let done = self.championship.as_ref().map_or(0, |c| c.times().len());
        let (rally, stage) = run.save.position();
        // Stages the AI has run in this rally.
        let ran = match run.turn {
            Turn::Welcome => 0,
            Turn::Stage(s) => s as usize,
            Turn::Service { .. } => stage as usize,
            Turn::Results | Turn::BetweenRallies | Turn::Over => 6,
        };
        let ran = ran.max(done);
        ui.separator();
        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            ui.strong(format!("{} — after SS{}", self.rally_name(), ran));
            egui::Grid::new("rally-standings").striped(true).spacing([12.0, 2.0]).show(ui, |ui| {
                ui.label("");
                ui.label("Driver");
                for s in 0..ran {
                    ui.label(format!("SS{}", s + 1));
                }
                ui.label("Total");
                ui.end_row();
                for (place, line) in season_model::rally_standings(&run.save, rally as usize, ran).iter().enumerate() {
                    ui.label(if line.total.is_some() { format!("{}", place + 1) } else { "–".into() });
                    let label = egui::RichText::new(&line.name);
                    let player = run.save.drivers[line.entry].id == season_model::PLAYER_ID;
                    ui.label(if player { label.strong() } else { label });
                    for t in &line.stages {
                        ui.label(t.map_or("ret".to_owned(), clock));
                    }
                    ui.label(line.total.map_or("retired".to_owned(), clock));
                    ui.end_row();
                }
            });
            let ui = &mut cols[1];
            ui.strong("Season");
            egui::Grid::new("season-standings").striped(true).spacing([12.0, 2.0]).show(ui, |ui| {
                for (place, (_, name, points)) in season_model::season_standings(&run.save).iter().enumerate() {
                    ui.label(format!("{}", place + 1));
                    ui.label(name);
                    ui.label(format!("{points}"));
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            ui.strong("Bulletins since your last turn");
            for line in &run.news {
                ui.label(line);
            }
        });
    }

    /// The stage phase: the start place, the tyre and setup pick after a service (else the car as it is), and
    /// the drive button.
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
        if let Some(ev) = self.run.as_ref().filter(|r| r.turn == Turn::Stage(index as u32)) {
            if let Some(head) = ev.save.events.first() {
                let (hour, minute) = events::time_of(head);
                let place = season_model::player_start_place(&ev.save).map_or(0, |p| p + 1);
                ui.label(format!("SS{}: start place {place}, at {hour:02}:{minute:02}", index + 1));
            }
        }
        let stage = c.rally().stages[index];
        let choosing = choice_before(index);
        ui.horizontal(|ui| {
            if choosing {
                let choices = tyre_choices(crate::catalog::surface_kind(stage.map));
                let tyre = self.next_tyre(index);
                ui.label("Tyres");
                ComboBox::from_id_salt("stage-tyre").selected_text(crate::catalog::tyre_label(tyre)).show_ui(
                    ui,
                    |ui| {
                        for &t in choices {
                            if ui.selectable_label(t == tyre, crate::catalog::tyre_label(t)).clicked() {
                                self.tyre = Some(t);
                            }
                        }
                    },
                );
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
                ui.label("Setup");
                ComboBox::from_id_salt("stage-setup").selected_text(label(&self.setup)).show_ui(ui, |ui| {
                    for row in rows {
                        let selected = row == self.setup;
                        if ui.selectable_label(selected, label(&row)).clicked() {
                            self.setup = row;
                        }
                    }
                });
            } else {
                // No service before the stage: the stage loop starts it on the car as it is.
                ui.label(format!("Tyres {}, setup as driven", crate::catalog::tyre_label(self.last_tyre)));
            }
            let go =
                ui.add_enabled(!self.driving && blocker.is_none(), egui::Button::new(format!("Drive SS{}", index + 1)));
            if go.clicked() {
                match self.stage_start(here, index) {
                    Ok(start) => request = Some(Request::Launch(start)),
                    Err(e) => self.status = e,
                }
            }
            if choosing && ui.button("Setup").clicked() {
                request = Some(Request::Setup);
            }
        });
        if let Some(issue) = blocker.filter(|_| !self.driving) {
            ui.colored_label(WARN, &issue.text);
        }
        request
    }

    /// The rally's start with a fresh car: the welcome page and the recce question.
    fn welcome_ui(&mut self, ui: &mut Ui, next: usize) {
        // The rally's start (the season's welcome page): a fresh car, nothing to repair.
        ui.label(format!("Welcome to {}. Service before SS{}: the car is new.", self.rally_name(), next + 1));
        if self.recce_asked {
            // `CONTINUE_SEASON` (0x5abc20) asks before the setup choice; "Yes" goes on to it (0x5abc00).
            let text = |key| text_of(self.strings.as_ref(), key);
            let (title, info, yes, no) = (text("MSG_CRECCE"), text("INF_FORFEITRECCE"), text("YES"), text("NO"));
            ui.label(title);
            ui.label(info);
            ui.horizontal(|ui| {
                if ui.button(yes).clicked() {
                    self.recce_asked = false;
                    self.start_rally();
                }
                if ui.button(no).clicked() {
                    self.recce_asked = false;
                }
            });
        } else if ui.button("Start the rally").clicked() {
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
        outcome: RallyOutcome,
        blocker: Option<&Issue>,
    ) -> Option<Request> {
        let mut request = None;
        ui.label(match outcome {
            RallyOutcome::Finished => "Rally finished.".to_owned(),
            RallyOutcome::Wrecked { stage } => {
                // Page 0x18 (FAILED, `CFailedSeasonFinishedWithBadCarUI`): the text by the season's queue.
                let key = match self.next_event(stage) {
                    NextEvent::Service => "MSG_UNFINISH_SERVICE",
                    _ => "MSG_UNFINISH_STAGE",
                };
                let text = text_of(self.strings.as_ref(), key);
                format!("SS{} finished, failed: {}", stage + 1, text.split_whitespace().collect::<Vec<_>>().join(" "))
            }
            RallyOutcome::Retired { stage } => format!("Retired on SS{}: rally over.", stage + 1),
        });
        match self.run.as_ref().map(|r| r.turn) {
            Some(Turn::Results) => {
                if ui.button("Continue").clicked() {
                    self.turn_done();
                }
            }
            Some(Turn::BetweenRallies) => {
                // Page 0x19 (0x5acf30): Continue, the two trials (the car tune and the season data are the
                // Setup page and the standings below).
                ui.horizontal(|ui| {
                    if ui.add_enabled(!self.driving, egui::Button::new("Next rally")).clicked() {
                        self.turn_done();
                    }
                    for (label, map) in [("Yard trial", 72), ("Stage trial", 71)] {
                        let trial = ui.add_enabled(!self.driving && blocker.is_none(), egui::Button::new(label));
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
            Some(Turn::Over) => {
                ui.strong("The season is over.");
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
        ui.strong(format!("{} before SS{}", text(park.kind.title_key()), next + 1));
        ui.label(format!(
            "{}{}   {}{}   time left {}",
            text("SP_ALLOW"),
            clock(park.kind.budget()),
            text("SP_ESTIM"),
            clock(park.estimate()),
            clock(park.time_left)
        ));
        let choosing = park.state == ParkState::Choosing && !self.forfeit_asked;
        for i in park.rows() {
            let mut on = park.covering(i).is_some();
            let line = format!(
                "{} — {}, {}",
                text_of(strings, &park.tree[i].name),
                text(service::damage_key(park.level(i))),
                clock(park.time_of(i))
            );
            if ui.add_enabled(choosing, egui::Checkbox::new(&mut on, line)).changed() {
                park.toggle(i);
            }
        }
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
                if ui.button(text("SP_MANADV")).clicked() {
                    park.manager_advice();
                }
                if ui.button(text("SP_REPAIR")).clicked() && !park.start() {
                    self.forfeit_asked = true;
                }
                if ui.button("Setup").clicked() {
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
                if let Some(key) = service::end_message(end, penalty) {
                    ui.label(text(key).replace("%1.f", &format!("{penalty:.0}")));
                }
                ui.label(format!("{}{penalty:.0} s", text("SP_PEN")));
                if ui.button("Continue").clicked() {
                    done = Some((penalty, park.damage.clone()));
                }
            }
        });
        done
    }
}
