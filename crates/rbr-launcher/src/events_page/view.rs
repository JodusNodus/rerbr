//! The Championship tab's drawing: the presets (Continue first when there is a saved championship), the builder (its settings beside the
//! live summary and Start event), and the event under way: the header (rally n of m, the step, the player's place),
//! the last stage's result, the card of the next step with its one primary button, the itinerary under it, and the
//! rally's standings and the event's points beside.

use std::path::Path;

use egui::{Color32, ComboBox, RichText, Sense, Slider, Stroke, Ui};
use rbr_formats::saves::is_file_name_part;
use rbr_gameplay::setup::SeasonSetup;
use rbr_season::config::{Damage, Daylight, Length, Preset, Skill, StagePool, Surface, Surfaces, Weather};
use rbr_season::event::{Conditions, Event, Standing, Turn};
use rbr_season::field::AiResult;
use rbr_season::team::team_offers;

use super::common::{card, card_title, clock, gap, ordinal, park_card, text_of, StageMark, BAD, CARD};
use super::*;
use crate::browser::{DIM, GREY, STAR};
use crate::pages::{combo, fields, section, start_button, Issue, ACCENT, GOOD, WARN};

/// What the page draws with.
pub struct Env<'a> {
    pub here: &'a Path,
    pub profile: &'a mut String,
    /// The catalog's cars that NGP drives.
    pub cars: &'a [Car],
    /// The car a new builder starts on (the Drive page's, the last driven).
    pub default_car: usize,
    /// Draws the builder's car field for the car; whether Browse was clicked.
    pub car_field: &'a mut dyn FnMut(&mut Ui, usize) -> bool,
    /// The car's saved setups on a surface (`SetupPage::saved_names`).
    pub saved: &'a dyn Fn(u32) -> Vec<(String, String)>,
    /// What blocks a stage's launch.
    pub blocker: Option<&'a Issue>,
    /// A new builder's seed.
    pub seed: u32,
}

/// The step of the event's turn: "Before the start", "Stage 3 of 6", "Service before stage 3 of 6", ...
pub fn step(event: &Event) -> String {
    let count = |rally: usize| event.rallies.get(rally).map_or(0, |r| r.stages.len());
    match event.turn {
        Turn::Welcome { .. } => "Before the start".to_owned(),
        Turn::Stage { rally, stage } => format!("Stage {} of {}", stage + 1, count(rally)),
        Turn::Service { rally, stage, .. } => format!("Service before stage {} of {}", stage + 2, count(rally)),
        Turn::RallyOver { .. } => "Rally over".to_owned(),
        Turn::EventOver => "Over".to_owned(),
    }
}

/// The rally the turn is in (the last one at the event's end).
fn rally_of(event: &Event) -> usize {
    match event.turn {
        Turn::Welcome { rally }
        | Turn::Stage { rally, .. }
        | Turn::Service { rally, .. }
        | Turn::RallyOver { rally } => rally,
        Turn::EventOver => event.rallies.len().saturating_sub(1),
    }
}

/// The last stage driven in the turn's rally, while its result is news (not at a welcome or the event's end).
fn last_stage(event: &Event) -> Option<(usize, usize)> {
    let rally = match event.turn {
        Turn::Stage { rally, stage } if stage > 0 => rally,
        Turn::Service { rally, .. } | Turn::RallyOver { rally } => rally,
        _ => return None,
    };
    let done = event.results.get(rally)?.len();
    Some((rally, done.checked_sub(1)?))
}

/// The player's place in the standings and their size, while the player has a total.
fn place(lines: &[Standing]) -> Option<(usize, usize)> {
    let at = lines.iter().position(|l| l.entrant.is_none())?;
    lines[at].total.map(|_| (at + 1, lines.len()))
}

/// A driver's name: the field's, "You" for the player.
fn driver(event: &Event, entrant: Option<usize>) -> String {
    entrant.and_then(|e| event.field.get(e)).map_or("You".to_owned(), |e| e.name.clone())
}

/// A catalog car key's name.
fn car_name(cars: &[Car], key: &str) -> String {
    cars.iter().find(|c| c.key == key).map_or(key.to_owned(), |c| c.name.clone())
}

/// An entrant's (or the player's) time on stage `stage` of `rally`: none once retired or not run.
fn stage_time(event: &Event, rally: usize, stage: usize, entrant: Option<usize>) -> Option<f32> {
    let (player, ai) = event.results.get(rally)?.get(stage)?;
    match entrant {
        None => player.filter(|p| !p.retired).map(|p| p.time),
        Some(e) => match ai.get(e)? {
            AiResult::Time(t) => Some(*t),
            AiResult::Retired => None,
        },
    }
}

/// The time of day a clock reads as.
fn time_word(hours: f32) -> &'static str {
    match hours {
        h if h < 5.5 => "night",
        h if h < 10.0 => "morning",
        h if h < 14.0 => "midday",
        h if h < 17.5 => "afternoon",
        h if h < 20.0 => "evening",
        h if h < 21.5 => "dusk",
        _ => "night",
    }
}

fn road_word(conditions: Conditions) -> &'static str {
    match conditions {
        Conditions::Dry => "dry",
        Conditions::Damp => "damp",
        Conditions::Wet => "wet",
    }
}

fn km(metres: f32) -> String {
    format!("{:.1} km", metres / 1000.0)
}

/// A preset's card: its name, what it is and its shape.
fn preset_text(preset: Preset) -> (&'static str, &'static str, &'static str) {
    match preset {
        Preset::QuickRally => ("Quick rally", "One rally on gravel, service halfway.", "6 stages · about an hour"),
        Preset::Weekend => (
            "Weekend",
            "Two legs and an overnight service; the second leg runs into the night.",
            "10 stages · two legs",
        ),
        Preset::Season => ("Season", "Six rallies on mixed surfaces, points after each.", "6 rallies · 36 stages"),
        Preset::Random => ("Random", "A weekend drawn from a random seed: stages, weather, field.", "a surprise"),
    }
}

/// A card that is one button: its frame lit on hover and focus. Whether it was clicked.
fn button_card(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) -> bool {
    let inner = egui::Frame::new().fill(CARD).corner_radius(6.0).inner_margin(12.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(92.0);
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), add);
    });
    let response = ui.interact(inner.response.rect, ui.scope_id().with(id), Sense::click());
    if response.hovered() || response.has_focus() {
        let stroke = Stroke::new(1.5, ACCENT);
        ui.painter().rect_stroke(response.rect, 6.0, stroke, egui::StrokeKind::Inside);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

impl EventsPage {
    /// The page: the presets, the builder or the event under way.
    pub fn ui(&mut self, ui: &mut Ui, env: Env) -> Option<Request> {
        match self.view {
            View::Builder if self.config.is_some() => self.builder_ui(ui, env),
            View::Event if self.run.is_some() => self.event_ui(ui, env),
            _ => {
                self.view = View::Entry;
                self.entry_ui(ui, env);
                None
            }
        }
    }

    /// The presets, with the profile and Continue event first when it has one.
    fn entry_ui(&mut self, ui: &mut Ui, env: Env) {
        ui.label(RichText::new("Championship").size(24.0).strong());
        ui.colored_label(
            GREY,
            "Build a rally or a season on any installed stage, against a field in your car's class.",
        );
        ui.add_space(8.0);
        fields(ui, "event-profile", |ui| {
            ui.label("Profile").on_hover_text("The championship is saved under this name, one per profile");
            ui.add_enabled(!self.driving, egui::TextEdit::singleline(env.profile).desired_width(160.0));
            env.profile.retain(|c| is_file_name_part(c.encode_utf8(&mut [0; 4])));
            ui.end_row();
        });
        let saved = self.saved_event(env.here, env.profile);
        if let Some(saved) = &saved {
            ui.add_space(8.0);
            card(ui, |ui| {
                card_title(ui, "Your championship");
                match saved {
                    Ok(line) => {
                        ui.label(RichText::new(line).size(16.0));
                        ui.add_space(8.0);
                        if ui.horizontal(|ui| ui.add(start_button("Continue"))).inner.clicked() {
                            self.continue_event(env.here, env.profile);
                        }
                    }
                    Err(e) => _ = ui.colored_label(WARN, format!("The saved championship does not read: {e}")),
                }
            });
        }
        if !self.status.is_empty() {
            ui.colored_label(WARN, &self.status);
        }
        section(ui, if saved.is_some() { "Or start a new one" } else { "Start a championship" });
        for row in Preset::ALL.chunks(2) {
            ui.columns(2, |cols| {
                for (ui, &preset) in cols.iter_mut().zip(row) {
                    let (title, text, detail) = preset_text(preset);
                    let clicked = button_card(ui, title, |ui| {
                        ui.label(RichText::new(title).size(18.0).strong());
                        ui.add_space(2.0);
                        ui.colored_label(GREY, text);
                        ui.add_space(4.0);
                        ui.colored_label(DIM, detail);
                    });
                    if clicked {
                        self.open_builder(preset, env.cars, env.default_car, env.seed);
                    }
                }
            });
            ui.add_space(8.0);
        }
    }

    /// The builder: the settings beside the summary and Start event.
    fn builder_ui(&mut self, ui: &mut Ui, env: Env) -> Option<Request> {
        let mut request = None;
        let preset = self.builder.map_or(Preset::QuickRally, |b| b.preset);
        if ui.link("‹ Championship").clicked() {
            self.view = View::Entry;
            return None;
        }
        ui.label(RichText::new(preset_text(preset).0).size(24.0).strong());
        ui.colored_label(GREY, preset_text(preset).1);
        ui.add_space(8.0);
        let car = self.car.unwrap_or(env.default_car);
        let stages = self.pool.as_mut().and_then(|p| p.stages().map(<[_]>::to_vec));
        let progress = self.pool.as_ref().and_then(Pool::progress);
        let saved = self.saved_event(env.here, env.profile).is_some();
        let mut edited = self.config.clone()?;
        let config = &mut edited;
        let classes = classes(env.cars);
        let (driving, status) = (self.driving, self.status.clone());
        let mut start = false;
        let teams = self.ai.as_ref().map_or_else(|_| Vec::new(), |(_, roster)| team_offers(roster));
        let (team, mut picked_team) = (config.team as usize, config.team as usize);
        let ai_error = self.ai.as_ref().err().cloned();
        ui.columns(2, |cols| {
            fields(&mut cols[0], "event-builder", |ui| {
                ui.label("Car");
                if (env.car_field)(ui, car) {
                    request = Some(Request::BrowseCar);
                }
                ui.end_row();
                ui.label("Class").on_hover_text("Only cars of this class fill the field");
                let label = |c: String| if c.is_empty() { "No class".to_owned() } else { c };
                combo(ui, "event-class", 200.0, &mut config.class, &classes, label);
                ui.end_row();
                ui.label("Length");
                length_row(ui, &mut config.length);
                ui.end_row();
                ui.label("Stages from");
                combo(
                    ui,
                    "event-pool",
                    200.0,
                    &mut config.pool,
                    &[StagePool::All, StagePool::Retail, StagePool::Community],
                    |p| match p {
                        StagePool::All => "Every installed stage",
                        StagePool::Retail => "The game's stages",
                        StagePool::Community => "Community stages (RSF, BTB)",
                    },
                );
                ui.end_row();
                ui.label("Surface");
                let surfaces = [Surface::Gravel, Surface::Tarmac, Surface::Snow].map(Surfaces::Only);
                combo(
                    ui,
                    "event-surface",
                    200.0,
                    &mut config.surfaces,
                    &[surfaces[0], surfaces[1], surfaces[2], Surfaces::Mixed],
                    |s| match s {
                        Surfaces::Only(Surface::Gravel) => "Gravel",
                        Surfaces::Only(Surface::Tarmac) => "Tarmac",
                        Surfaces::Only(Surface::Snow) => "Snow",
                        Surfaces::Mixed => "Mixed, by rally",
                    },
                );
                ui.end_row();
                ui.label("Time of day");
                combo(
                    ui,
                    "event-daylight",
                    200.0,
                    &mut config.daylight,
                    &[Daylight::Day, Daylight::Realistic, Daylight::Night],
                    |d| match d {
                        Daylight::Day => "Day",
                        Daylight::Realistic => "Day, the last stages at dusk",
                        Daylight::Night => "Night",
                    },
                );
                ui.end_row();
                ui.label("Weather");
                combo(
                    ui,
                    "event-weather",
                    200.0,
                    &mut config.weather,
                    &[Weather::Dry, Weather::Changeable, Weather::Wet],
                    |w| match w {
                        Weather::Dry => "Dry",
                        Weather::Changeable => "Changeable",
                        Weather::Wet => "Wet",
                    },
                )
                .on_hover_text(
                    "Changeable: each leg's road follows the stage's own weathers, so the tyre choice matters",
                );
                ui.end_row();
                ui.label("Damage");
                combo(
                    ui,
                    "event-damage",
                    200.0,
                    &mut config.damage,
                    &[Damage::Off, Damage::Reduced, Damage::Realistic],
                    |d| match d {
                        Damage::Off => "Off",
                        Damage::Reduced => "Reduced",
                        Damage::Realistic => "Realistic",
                    },
                );
                ui.end_row();
                ui.label("Skill level");
                ui.horizontal(|ui| {
                    combo(
                        ui,
                        "event-skill",
                        160.0,
                        &mut config.skill,
                        &[Skill::Rookie, Skill::Professional, Skill::Champion],
                        skill_word,
                    );
                    ui.colored_label(GREY, format!("help when stuck: {}", assistance_note(config.skill)));
                });
                ui.end_row();
                if !teams.is_empty() {
                    ui.label("Team").on_hover_text("The service crew that repairs your car");
                    let (mut team, offers) = (team, &teams);
                    ui.horizontal(|ui| {
                        let name = |i: usize| {
                            offers.iter().find(|t| t.index as usize == i).map_or(String::new(), |t| t.name.clone())
                        };
                        let indexes: Vec<usize> = offers.iter().map(|t| t.index as usize).collect();
                        combo(ui, "event-team", 200.0, &mut team, &indexes, name);
                        if let Some(offer) = offers.iter().find(|t| t.index as usize == team) {
                            ui.colored_label(GREY, &offer.crew);
                        }
                    });
                    picked_team = team;
                    ui.end_row();
                }
                ui.label("Difficulty");
                ui.horizontal(|ui| {
                    ui.add(Slider::new(&mut config.difficulty, 0.0..=1.0).show_value(false));
                    ui.label(RichText::new(difficulty_word(config.difficulty)).strong());
                });
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut config.adaptive, "Adaptive: the field follows your results")
                    .on_hover_text("The difficulty moves with your stage results so the fight stays close");
                ui.end_row();
                ui.label("Field");
                ui.add(Slider::new(&mut config.field_size, 1..=30).suffix(" drivers"));
                ui.end_row();
                ui.label("Seed").on_hover_text("The same seed and stages build the same championship");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut config.seed));
                    if ui.button("New seed").clicked() {
                        config.seed = config.seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    }
                });
                ui.end_row();
            });
            let ui = &mut cols[1];
            card(ui, |ui| {
                card_title(ui, "Summary");
                ui.label(RichText::new(summary(config, stages.as_deref())).size(16.0).color(Color32::WHITE));
                ui.add_space(4.0);
                let field = class_cars(env.cars, &config.class).len();
                ui.colored_label(
                    GREY,
                    match field {
                        0 | 1 => format!(
                            "The field drives your car: no other {} car is installed.",
                            label_class(&config.class)
                        ),
                        n => format!("{n} {} cars fill the field.", label_class(&config.class)),
                    },
                );
                let fit = stages.as_deref().map(|s| eligible(config, s).len());
                match (progress, fit) {
                    (Some((done, total)), _) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.colored_label(GREY, format!("Reading the stages' lengths… {done} of {total}"));
                        });
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                    }
                    (None, Some(0)) => {
                        _ = ui.colored_label(WARN, "No installed stage fits: choose other stages or another surface.")
                    }
                    (None, Some(n)) => _ = ui.colored_label(GREY, format!("{n} installed stages fit.")),
                    (None, None) => {}
                }
                ui.add_space(10.0);
                let ready =
                    fit.is_some_and(|n| n > 0) && is_file_name_part(env.profile) && !driving && ai_error.is_none();
                let go = ui.horizontal(|ui| ui.add_enabled(ready, start_button("Start"))).inner;
                if saved {
                    ui.colored_label(GREY, format!("Replaces {}'s saved championship.", env.profile));
                }
                start = go.clicked();
                if let Some(e) = &ai_error {
                    ui.colored_label(WARN, format!("The AI tables do not read: {e}"));
                }
                if !status.is_empty() {
                    ui.colored_label(WARN, &status);
                }
            });
        });
        edited.team = picked_team as u8;
        self.config = Some(edited);
        if let Some(stages) = stages.filter(|_| start) {
            self.start_event(env.here, env.profile, env.cars, &stages);
        }
        request
    }

    /// The event under way: the header, the next step's card with its button, the itinerary, the standings.
    fn event_ui(&mut self, ui: &mut Ui, env: Env) -> Option<Request> {
        let mut request = None;
        let run = self.run.as_ref()?;
        let event = run.event.clone();
        let rally = rally_of(&event);
        let standings = event.rally_standings(rally);
        let points = (event.rallies.len() > 1).then(|| event.points());
        let news = if last_stage(&event).is_some() { event.bulletins(rally) } else { Vec::new() };
        if ui.link("‹ Championship").clicked() {
            self.view = View::Entry;
            return None;
        }
        header_ui(ui, &event, &standings, env.cars);
        ui.add_space(8.0);
        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            if let Some((r, s)) = last_stage(&event) {
                stage_result_ui(ui, &event, r, s);
                ui.add_space(10.0);
            }
            card(ui, |ui| match event.turn {
                Turn::Welcome { rally } => self.welcome_ui(ui, env.here, env.profile, &event, rally),
                Turn::Stage { rally, stage } => request = self.stage_ui(ui, &env, &event, rally, stage),
                Turn::Service { stage, minutes, .. } => {
                    let title = format!("Service after SS{}", stage + 1);
                    let mut setup = false;
                    let strings = self.strings.as_ref();
                    let done = self
                        .park
                        .as_mut()
                        .and_then(|park| park_card(ui, park, strings, &title, minutes as f32 * 60.0, &mut setup));
                    if self.park.is_none() {
                        ui.colored_label(GREY, text_of(strings, "NSERVICE"));
                    }
                    if setup {
                        request = Some(Request::Setup);
                    }
                    if let Some((penalty, damage)) = done {
                        self.service_done(env.here, env.profile, penalty, damage);
                    }
                }
                Turn::RallyOver { rally } => {
                    request = self.rally_over_ui(ui, &env, &event, rally, &standings);
                }
                Turn::EventOver => self.event_over_ui(ui, env.here, env.profile, &event, &standings, points.as_deref()),
            });
            if !self.status.is_empty() {
                ui.add_space(4.0);
                ui.colored_label(WARN, &self.status);
            }
            ui.add_space(10.0);
            itinerary_ui(ui, &event, rally);
            let ui = &mut cols[1];
            if !news.is_empty() {
                bulletins_ui(ui, &news, &mut self.news_all);
                ui.add_space(10.0);
            }
            standings_ui(ui, &event, rally, &standings, env.cars);
            if let Some(points) = &points {
                ui.add_space(10.0);
                points_ui(ui, &event, points);
            }
        });
        request
    }

    fn welcome_ui(&mut self, ui: &mut Ui, here: &Path, profile: &str, event: &Event, rally: usize) {
        let Some(r) = event.rallies.get(rally) else { return };
        card_title(ui, &format!("Welcome to {}", r.name));
        let length: f32 = r.stages.iter().map(|s| s.stage.length).sum();
        let services = r.stages.iter().filter(|s| s.service_after.is_some()).count();
        let mut surfaces: Vec<&str> = r.stages.iter().map(|s| pool::surface_word(s.stage.surface)).collect();
        surfaces.dedup();
        let first = r.stages.first().map_or(String::new(), |s| format!(" · SS1 at {}", clock_hm(s.clock)));
        ui.colored_label(
            GREY,
            format!(
                "{} stages · {} · {} · {services} service{}{first}",
                r.stages.len(),
                km(length),
                surfaces.join(", "),
                if services == 1 { "" } else { "s" }
            ),
        );
        ui.colored_label(GREY, "The car starts new.");
        ui.add_space(10.0);
        if ui.horizontal(|ui| ui.add(start_button("Start the rally"))).inner.clicked() {
            self.turn_done(here, profile);
        }
    }

    /// The stage's card: its length, road, start and time of day, the tyres and setup (kept without a service
    /// before), and Drive.
    fn stage_ui(&mut self, ui: &mut Ui, env: &Env, event: &Event, rally: usize, stage: usize) -> Option<Request> {
        let mut request = None;
        let next = event.rallies.get(rally)?.stages.get(stage)?;
        card_title(ui, &format!("Next: SS{} · {}", stage + 1, next.stage.name));
        let kind = surface_kind(next.stage.surface);
        ui.colored_label(
            GREY,
            format!(
                "{} · {} · {} road · start {} ({})",
                km(next.stage.length),
                pool::surface_word(next.stage.surface),
                road_word(next.conditions),
                clock_hm(next.clock),
                time_word(next.clock)
            ),
        );
        ui.add_space(6.0);
        let choosing = self.choice_before(rally, stage);
        let tyre = self.next_tyre();
        let road = self.run.as_ref().and_then(|r| road_line(&r.event, rally, stage));
        fields(ui, "event-stage", |ui| {
            if let Some(road) = road {
                ui.label("Start order");
                ui.label(road);
                ui.end_row();
            }
            ui.label("Tyres");
            if choosing {
                ComboBox::from_id_salt("event-tyre")
                    .width(200.0)
                    .selected_text(crate::catalog::tyre_label(tyre))
                    .show_ui(ui, |ui| {
                        for &t in rbr_season::rally::tyre_choices(kind) {
                            if ui.selectable_label(t == tyre, crate::catalog::tyre_label(t)).clicked() {
                                self.tyre = Some(t);
                            }
                        }
                    });
            } else {
                ui.colored_label(GREY, format!("{}, kept: no service before", crate::catalog::tyre_label(tyre)));
            }
            ui.end_row();
            ui.label("Setup");
            if choosing {
                let saved = (env.saved)(kind);
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
                    ComboBox::from_id_salt("event-setup").width(200.0).selected_text(label(&self.setup)).show_ui(
                        ui,
                        |ui| {
                            for row in rows {
                                if ui.selectable_label(row == self.setup, label(&row)).clicked() {
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
        });
        if let Some(penalty) = self.run.as_ref().map(|r| r.penalty).filter(|p| *p > 0.0) {
            ui.colored_label(BAD, format!("Late from service: {penalty:.0} s go on this stage's time."));
        }
        ui.add_space(10.0);
        let go = ui.horizontal(|ui| {
            ui.add_enabled(!self.driving && env.blocker.is_none(), start_button(&format!("Drive SS{}", stage + 1)))
        });
        if go.inner.clicked() {
            match self.stage_start(env.here) {
                Ok(start) => request = Some(Request::Launch(start)),
                Err(e) => self.status = e,
            }
        }
        if self.driving {
            ui.colored_label(GREY, "The stage is being driven.");
        } else if let Some(issue) = env.blocker {
            ui.colored_label(WARN, &issue.text);
        }
        request
    }

    fn rally_over_ui(
        &mut self,
        ui: &mut Ui,
        env: &Env,
        event: &Event,
        rally: usize,
        lines: &[Standing],
    ) -> Option<Request> {
        let mut request = None;
        let name = event.rallies.get(rally).map_or("The rally", |r| r.name.as_str());
        let retired = event.results.get(rally).and_then(|r| r.iter().position(|(p, _)| p.is_none_or(|p| p.retired)));
        match retired {
            Some(stage) => card_title(ui, &format!("Out of {name} on SS{}", stage + 1)),
            None => card_title(ui, &format!("{name} is over")),
        }
        if let Some((at, of)) = place(lines) {
            let points = lines[at - 1].points;
            ui.label(
                RichText::new(format!("You finished {} of {of} · {points} points", ordinal(at))).size(16.0).color(STAR),
            );
        }
        ui.add_space(10.0);
        let next = event.rallies.get(rally + 1);
        let label = match next {
            Some(next) => format!("Next rally: {}", next.name),
            None => "To the final result".to_owned(),
        };
        if ui.horizontal(|ui| ui.add_enabled(!self.driving, start_button(&label))).inner.clicked() {
            self.turn_done(env.here, env.profile);
        }
        if next.is_some() {
            // The game's two trials (`S_YARD_TRIAL`, `S_STAGE_TRIAL`): practice that is not recorded.
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.colored_label(GREY, "Practice first (not recorded):");
                for (label, yard, tip) in [
                    ("Yard trial", true, "A practice drive in the yard; the championship does not record it"),
                    (
                        "Stage trial",
                        false,
                        "A practice drive on the next rally's first stage, on its surface; not recorded",
                    ),
                ] {
                    let trial = ui.add_enabled(!self.driving && env.blocker.is_none(), egui::Button::new(label));
                    if trial.on_hover_text(tip).clicked() {
                        match self.trial(yard) {
                            Ok(start) => request = Some(Request::Launch(start)),
                            Err(e) => self.status = e,
                        }
                    }
                }
            });
            if let Some(issue) = env.blocker.filter(|_| !self.driving) {
                ui.colored_label(WARN, &issue.text);
            }
        }
        request
    }

    fn event_over_ui(
        &mut self,
        ui: &mut Ui,
        here: &Path,
        profile: &str,
        event: &Event,
        rally: &[Standing],
        points: Option<&[Standing]>,
    ) {
        let season = points.is_some();
        card_title(ui, if season { "The season is over" } else { "The championship is over" });
        let lines = points.unwrap_or(rally);
        if let Some(at) = lines.iter().position(|l| l.entrant.is_none()) {
            let total = lines[at].points;
            let text = match (at, season) {
                (0, true) => format!("Champion, with {total} points."),
                (0, false) => "You won.".to_owned(),
                (_, true) => format!("You finished {} of {} with {total} points.", ordinal(at + 1), lines.len()),
                (_, false) if lines[at].total.is_none() => "You retired.".to_owned(),
                (_, false) => format!("You finished {} of {}.", ordinal(at + 1), lines.len()),
            };
            ui.label(RichText::new(text).size(17.0).strong().color(STAR));
            if let Some(winner) = lines.first().filter(|l| l.entrant.is_some()) {
                ui.colored_label(GREY, format!("Winner: {}", driver(event, winner.entrant)));
            }
        }
        ui.add_space(10.0);
        let new = ui.horizontal(|ui| {
            let new = ui.add(start_button("New championship"));
            ui.colored_label(GREY, "back to the presets");
            new
        });
        if new.inner.clicked() {
            self.leave_event(here, profile);
        }
    }
}

/// The stage card's road line: "14th on the road · worn road", none without a road order.
fn road_line(event: &Event, rally: usize, stage: usize) -> Option<String> {
    let at = event.start_order(rally, stage).iter().position(Option::is_none)?;
    let wear = ["fresh", "used", "worn"][event.road_wear(rally, stage) as usize % 3];
    Some(format!("{} on the road · {wear} road", ordinal(at + 1)))
}

/// The bulletins' lines shown without "show all".
const NEWS_SHOWN: usize = 8;

/// The news since the player's last stage: the last few lines, or all of them.
fn bulletins_ui(ui: &mut Ui, news: &[String], all: &mut bool) {
    card(ui, |ui| {
        card_title(ui, "Since your last stage");
        let hidden = if *all { 0 } else { news.len().saturating_sub(NEWS_SHOWN) };
        for line in &news[hidden..] {
            ui.label(line);
        }
        if news.len() > NEWS_SHOWN && ui.link(if *all { "Show fewer" } else { "Show all" }).clicked() {
            *all = !*all;
        }
    });
}

fn label_class(class: &str) -> &str {
    if class.is_empty() {
        "unclassed"
    } else {
        class
    }
}

/// The length's kind, its stages and, for a season, its rallies.
fn length_row(ui: &mut Ui, length: &mut Length) {
    let (mut kind, mut rallies, mut stages) = match *length {
        Length::Rally { stages } => (0, 6, stages),
        Length::Weekend { stages } => (1, 6, stages),
        Length::Season { rallies, stages } => (2, rallies, stages),
    };
    ui.horizontal(|ui| {
        combo(ui, "event-length", 120.0, &mut kind, &[0, 1, 2], |k| ["One rally", "Weekend", "Season"][k]);
        if kind == 2 {
            ui.add(Slider::new(&mut rallies, 2..=12).suffix(" rallies"));
        }
        ui.add(Slider::new(&mut stages, 2..=16).suffix(if kind == 2 { " stages each" } else { " stages" }));
    });
    *length = match kind {
        0 => Length::Rally { stages },
        1 => Length::Weekend { stages },
        _ => Length::Season { rallies, stages },
    };
}

/// The event's header: the rally, where it stands and the player's place, then the car and the settings.
fn header_ui(ui: &mut Ui, event: &Event, lines: &[Standing], cars: &[Car]) {
    let rally = rally_of(event);
    ui.horizontal(|ui| {
        let name = event.rallies.get(rally).map_or("", |r| r.name.as_str());
        ui.label(RichText::new(name).size(24.0).strong());
        ui.add_space(6.0);
        let big = |text: String| RichText::new(text).size(17.0);
        if event.rallies.len() > 1 {
            ui.label(big(format!("Rally {} of {}", rally + 1, event.rallies.len())));
            ui.colored_label(DIM, big("·".into()));
        }
        ui.label(big(step(event)).color(STAR));
        if let Some((at, of)) = place(lines).filter(|_| event.results.get(rally).is_some_and(|r| !r.is_empty())) {
            ui.colored_label(DIM, big("·".into()));
            ui.label(big(format!("{} of {of}", ordinal(at))));
        }
    });
    let c = &event.config;
    let damage = match c.damage {
        Damage::Off => "no damage",
        Damage::Reduced => "reduced damage",
        Damage::Realistic => "realistic damage",
    };
    let adaptive = if c.adaptive { " · adaptive" } else { "" };
    ui.colored_label(
        GREY,
        format!(
            "{} · {} · {}{adaptive} · {damage}",
            car_name(cars, &c.car),
            label_class(&c.class),
            difficulty_word(c.difficulty)
        ),
    );
}

/// The last stage's result: the stage winner and the player's time, place and gap on it.
fn stage_result_ui(ui: &mut Ui, event: &Event, rally: usize, stage: usize) {
    let Some(s) = event.rallies.get(rally).and_then(|r| r.stages.get(stage)) else { return };
    let mut times: Vec<(Option<usize>, f32)> = (std::iter::once(None).chain((0..event.field.len()).map(Some)))
        .filter_map(|e| Some((e, stage_time(event, rally, stage, e)?)))
        .collect();
    times.sort_by(|a, b| a.1.total_cmp(&b.1));
    card(ui, |ui| {
        card_title(ui, &format!("SS{} · {} result", stage + 1, s.stage.name));
        if let Some(&(winner, best)) = times.first() {
            ui.horizontal(|ui| {
                ui.colored_label(GREY, "Stage winner");
                ui.label(RichText::new(driver(event, winner)).strong());
                ui.label(clock(best));
            });
        }
        match times.iter().position(|(e, _)| e.is_none()) {
            Some(0) => {
                if let Some(&(_, second)) = times.get(1) {
                    ui.colored_label(STAR, format!("You won the stage by {}", clock(second - times[0].1)));
                }
            }
            Some(at) => {
                let (_, time) = times[at];
                ui.horizontal(|ui| {
                    ui.colored_label(GREY, "You");
                    ui.label(RichText::new(clock(time)).strong().color(STAR));
                    ui.label(format!("{} on the stage", ordinal(at + 1)));
                    if at > 0 {
                        ui.colored_label(GREY, gap(time, times[0].1));
                    }
                });
            }
            None => _ = ui.colored_label(BAD, "You retired on the stage."),
        }
    });
}

/// The rally's stages and services: each stage's start, length, road and the player's time once driven.
fn itinerary_ui(ui: &mut Ui, event: &Event, rally: usize) {
    let Some(r) = event.rallies.get(rally) else { return };
    let results = event.results.get(rally).map_or(&[][..], Vec::as_slice);
    let mark = |k: usize| match results.get(k) {
        Some((Some(p), _)) if !p.retired => StageMark::Done(p.time),
        Some(_) => StageMark::Retired,
        None => match event.turn {
            Turn::Stage { rally: at, stage } if at == rally && stage == k => StageMark::Next,
            Turn::Service { rally: at, stage, .. } if at == rally && stage + 1 == k => StageMark::Next,
            _ => StageMark::Later,
        },
    };
    card(ui, |ui| {
        card_title(ui, "Itinerary");
        egui::Grid::new("event-itinerary").num_columns(7).spacing([12.0, 5.0]).show(ui, |ui| {
            for heading in ["", "", "Stage", "Length", "Start", "Road", "Time"] {
                ui.colored_label(GREY, heading);
            }
            ui.end_row();
            for (k, s) in r.stages.iter().enumerate() {
                let m = mark(k);
                let (icon, colour) = match m {
                    StageMark::Done(_) => ("✔", GOOD),
                    StageMark::Retired => ("✖", WARN),
                    StageMark::Next => ("▶", STAR),
                    StageMark::Later => ("", GREY),
                };
                let at_service = matches!(event.turn, Turn::Service { .. });
                ui.colored_label(colour, if at_service && m == StageMark::Next { "" } else { icon });
                let strong = |text: String| {
                    let text = RichText::new(text);
                    if m == StageMark::Next {
                        text.strong().color(Color32::WHITE)
                    } else {
                        text
                    }
                };
                ui.label(strong(format!("SS{}", k + 1)));
                ui.label(strong(s.stage.name.clone()));
                ui.colored_label(GREY, km(s.stage.length));
                ui.colored_label(GREY, clock_hm(s.clock));
                ui.colored_label(GREY, road_word(s.conditions));
                ui.label(match m {
                    StageMark::Done(time) => clock(time),
                    StageMark::Retired => "out".to_owned(),
                    _ => String::new(),
                });
                ui.end_row();
                if let Some(minutes) = s.service_after.filter(|_| k + 1 < r.stages.len()) {
                    let at = matches!(event.turn, Turn::Service { rally: a, stage, .. } if a == rally && stage == k);
                    let colour = if at { STAR } else { DIM };
                    ui.colored_label(colour, if at { "▶" } else { "" });
                    ui.label("");
                    ui.colored_label(colour, format!("🔧 Service, {minutes} min"));
                    ui.end_row();
                }
            }
        });
        let penalties: f32 = results.iter().filter_map(|(p, _)| p.as_ref()).map(|p| p.penalties).sum();
        let total: f32 =
            results.iter().filter_map(|(p, _)| p.as_ref()).filter(|p| !p.retired).map(|p| p.time + p.penalties).sum();
        if !results.is_empty() {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if penalties > 0.0 {
                    ui.colored_label(BAD, format!("Penalties {}", clock(penalties)));
                    ui.add_space(12.0);
                }
                ui.label(RichText::new(format!("Total {}", clock(total))).strong());
            });
        }
    });
}

/// The rally's standings after the stages run: place, driver, car, the last stage's time, total, gap and points.
fn standings_ui(ui: &mut Ui, event: &Event, rally: usize, lines: &[Standing], cars: &[Car]) {
    let ran = event.results.get(rally).map_or(0, Vec::len);
    let count = event.rallies.get(rally).map_or(0, |r| r.stages.len());
    card(ui, |ui| {
        ui.horizontal(|ui| {
            card_title(ui, "Rally standings");
            if ran > 0 {
                ui.colored_label(GREY, if ran == count { "final".to_owned() } else { format!("after SS{ran}") });
            }
        });
        if ran == 0 {
            ui.colored_label(GREY, format!("No stage run yet. {} drivers start.", event.field.len() + 1));
            return;
        }
        let best = lines.iter().filter_map(|l| l.total).fold(f32::INFINITY, f32::min);
        let points = lines.iter().any(|l| l.points > 0);
        // The last stage's column from SS2 on, until the points take its place.
        let stage = (ran > 1 && !points).then(|| format!("SS{ran}"));
        egui::Grid::new("event-standings").striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
            let headings = ["", "Driver", "Car"].into_iter().chain(stage.as_deref()).chain(["Total", "Gap"]);
            for heading in headings.chain(points.then_some("Pts")) {
                ui.colored_label(GREY, heading);
            }
            ui.end_row();
            for (at, line) in lines.iter().enumerate() {
                let name = RichText::new(driver(event, line.entrant));
                let car =
                    line.entrant.and_then(|e| event.field.get(e)).map_or(event.config.car.as_str(), |e| e.car.as_str());
                ui.label(if line.total.is_some() { format!("{}", at + 1) } else { "–".into() });
                ui.label(if line.entrant.is_none() { name.strong().color(STAR) } else { name });
                ui.colored_label(GREY, car_name(cars, car));
                if stage.is_some() {
                    ui.label(stage_time(event, rally, ran - 1, line.entrant).map_or("–".to_owned(), clock));
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
    });
}

/// The season's points.
fn points_ui(ui: &mut Ui, event: &Event, lines: &[Standing]) {
    card(ui, |ui| {
        card_title(ui, "Season points");
        if lines.iter().all(|l| l.points == 0) {
            ui.colored_label(GREY, "Points come at each rally's end.");
            return;
        }
        egui::Grid::new("event-points").striped(true).spacing([14.0, 3.0]).show(ui, |ui| {
            for (at, line) in lines.iter().enumerate() {
                let name = RichText::new(driver(event, line.entrant));
                ui.label(format!("{}", at + 1));
                ui.label(if line.entrant.is_none() { name.strong().color(STAR) } else { name });
                ui.label(line.points.to_string());
                ui.end_row();
            }
        });
    });
}
