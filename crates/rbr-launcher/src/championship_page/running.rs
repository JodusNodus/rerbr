//! The championship under way: the header, the itinerary strip, the next step on the left (its one primary button in
//! the pinned footer) and the standings on the right, with the bulletins and the season's cards.

use egui::{Align, ComboBox, Layout, RichText, Ui, Vec2};
use rbr_gameplay::setup::SeasonSetup;
use rbr_season::event::{Event, Standing, Turn, POINTS, POWER_POINTS};

use super::cards::*;
use super::common::{card, card_title, clock, gap, ordinal, park_card, row, text_of};
use super::results::*;
use super::table::{table, Cell, Col, Line};
use super::view::*;
use super::*;
use crate::kit;
use crate::pages::WARN as WARN_COLOR;
use crate::theme::*;

/// The left column's share of the body.
const LEFT: f32 = 0.46;

impl ChampionshipPage {
    /// The championship under way: the header, the itinerary strip, the next step's card, the standings and the
    /// pinned footer with the step's primary button.
    pub(super) fn event_ui(&mut self, ui: &mut Ui, env: Env) -> Option<Request> {
        let mut request = None;
        let run = self.run.as_ref()?;
        let event = run.event.clone();
        let rally = rally_of(&event);
        let standings = event.rally_standings(rally);
        let points = (event.rallies.len() > 1).then(|| event.points());
        let news = if last_stage(&event).is_some() { event.bulletins(rally) } else { Vec::new() };
        if header_ui(ui, &event, &standings, env.cars) {
            self.view = View::Entry;
            return None;
        }
        if let Some(asked) = self.footer_ui(ui, &env, &event, rally) {
            request = Some(asked);
        }
        // The stage picked on the itinerary, else the last one driven while it is news.
        let picked = self.shown.filter(|&(r, s)| r == rally && s < driven(&event, rally).len());
        let key = match event.turn {
            Turn::Welcome { rally } | Turn::RallyOver { rally } => (rally, 0, 0),
            Turn::Stage { rally, stage } => (rally, stage, 1),
            Turn::Service { rally, stage, .. } => (rally, stage, 2),
            Turn::EventOver => (rally, 0, 3),
        };
        let scroll = self.strip_at != Some(key);
        self.strip_at = Some(key);
        kit::body(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("championship").auto_shrink([false, false]).show(ui, |ui| {
                if let Some(stage) = strip_ui(ui, &event, rally, picked.map(|(_, s)| s), scroll) {
                    self.shown = Some((rally, stage));
                    self.stage_times = true;
                }
                totals_ui(ui, &event, rally);
                ui.add_space(14.0);
                let width = ui.available_width();
                let origin = ui.cursor().min;
                let gutter = 20.0;
                let left = (width - gutter) * LEFT;
                let column = |x: f32, w: f32| {
                    egui::UiBuilder::new()
                        .max_rect(egui::Rect::from_min_size(egui::pos2(x, origin.y), Vec2::new(w, 100_000.0)))
                        .layout(Layout::top_down(Align::Min))
                };
                let used_left = ui
                    .scope_builder(column(origin.x, left), |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        if let Some(asked) = self.left_ui(ui, &env, &event, &standings, points.as_deref(), &news) {
                            request = Some(asked);
                        }
                    })
                    .response
                    .rect;
                let used_right = ui
                    .scope_builder(column(origin.x + left + gutter, width - left - gutter), |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        let latest = last_stage(&event);
                        let stage = picked.or(latest).map(|(_, s)| (s, picked.is_some()));
                        let mut times = self.stage_times && stage.is_some();
                        if standings_ui(ui, &event, rally, &standings, env.cars, stage, &mut times) {
                            self.shown = None;
                        }
                        self.stage_times = times;
                        if let Some(points) = &points {
                            ui.add_space(12.0);
                            kit::group_label(ui, "Season points", None);
                            points_ui(ui, &event, points);
                        }
                    })
                    .response
                    .rect;
                ui.allocate_rect(used_left.union(used_right), egui::Sense::hover());
            });
        });
        request
    }

    /// The left column: the step's card (welcome, next stage, service, the rally's result, the championship's),
    /// the bulletins and the season's calendar.
    #[allow(clippy::too_many_arguments)]
    fn left_ui(
        &mut self,
        ui: &mut Ui,
        env: &Env,
        event: &Event,
        standings: &[Standing],
        points: Option<&[Standing]>,
        news: &[String],
    ) -> Option<Request> {
        let mut request = None;
        match event.turn {
            Turn::Welcome { rally } => {
                kit::group_label(ui, "The rally", None);
                self.welcome_ui(ui, event, rally);
            }
            Turn::Stage { rally, stage } => {
                kit::group_label(ui, "Next stage", None);
                request = self.stage_ui(ui, env, event, rally, stage);
            }
            Turn::Service { .. } if rejoin(event).is_some() => {
                kit::group_label(ui, "Service", None);
                self.rejoin_ui(ui, event);
            }
            Turn::Service { stage, minutes, .. } => {
                let title = format!("Service after SS{}", stage + 1);
                let mut setup = false;
                let strings = self.strings.as_ref();
                kit::group_label(ui, "Service park", None);
                let done = card(ui, |ui| {
                    let done = self
                        .park
                        .as_mut()
                        .and_then(|park| park_card(ui, park, strings, &title, minutes as f32 * 60.0, &mut setup));
                    if self.park.is_none() {
                        ui.colored_label(TEXT_2, text_of(strings, "NSERVICE"));
                    }
                    done
                });
                if setup {
                    request = Some(Request::Setup);
                }
                if let Some((penalty, damage)) = done {
                    self.service_done(env.here, env.profile, penalty, damage);
                }
            }
            Turn::RallyOver { rally } => {
                kit::group_label(ui, "Your result", None);
                self.rally_over_ui(ui, event, rally, standings);
            }
            Turn::EventOver => {
                kit::group_label(ui, "Your result", None);
                self.event_over_ui(ui, event, standings, points);
            }
        }
        if !self.status.is_empty() {
            ui.colored_label(WARN_COLOR, &self.status);
        }
        if !news.is_empty() {
            ui.add_space(10.0);
            let title =
                if matches!(event.turn, Turn::RallyOver { .. }) { "During the rally" } else { "Since your last stage" };
            bulletins_ui(ui, title, news, &mut self.news_all);
        }
        if points.is_some() {
            ui.add_space(12.0);
            kit::group_label(ui, "Calendar", None);
            calendar_ui(ui, event);
        }
        request
    }

    fn welcome_ui(&mut self, ui: &mut Ui, event: &Event, rally: usize) {
        let Some(r) = event.rallies.get(rally) else { return };
        let length: f32 = r.stages.iter().map(|s| s.stage.length).sum();
        let services = r.stages.iter().filter(|s| s.service_after.is_some()).count();
        let mut surfaces: Vec<&str> = r.stages.iter().map(|s| pool::surface_word(s.stage.surface)).collect();
        surfaces.dedup();
        let first = r.stages.first().map_or(String::new(), |s| format!(" · SS1 at {}", clock_hm(s.clock)));
        card(ui, |ui| {
            card_title(ui, &format!("Welcome to {}", r.name));
            let line = format!(
                "{} stages · {} · {} · {services} service{}{first}",
                r.stages.len(),
                km(length),
                surfaces.join(", "),
                if services == 1 { "" } else { "s" }
            );
            ui.colored_label(TEXT_2, line);
            ui.colored_label(TEXT_2, "The car starts new.");
            if let Some(line) = title_line(event) {
                ui.add_space(4.0);
                ui.colored_label(WARN, line);
            }
        });
    }

    /// The end-of-leg service a car retired under Super Rally waits out: it is rebuilt, nothing to repair, and
    /// restarts the next leg.
    fn rejoin_ui(&mut self, ui: &mut Ui, event: &Event) {
        let Some(next) = rejoin(event) else { return };
        card(ui, |ui| {
            card_title(ui, "Super Rally");
            ui.label(format!("The car is rebuilt at the end of the leg and restarts the next leg on SS{}.", next + 1));
            ui.colored_label(TEXT_2, "Each stage not completed counts as its fastest time plus 10:00.");
        });
    }

    /// The stage's card: its length, road, start and time of day, the rally's gaps and start order, and the tyres
    /// and setup (kept without a service before).
    fn stage_ui(&mut self, ui: &mut Ui, env: &Env, event: &Event, rally: usize, stage: usize) -> Option<Request> {
        let mut request = None;
        let next = event.rallies.get(rally)?.stages.get(stage)?;
        let kind = surface_kind(next.stage.surface);
        let choosing = self.choice_before(rally, stage);
        let tyre = self.next_tyre();
        let road = self.run.as_ref().and_then(|r| road_line(&r.event, rally, stage));
        let rivals = (stage > 0).then(|| rivals_line(event, rally)).flatten();
        let title = format!("SS{} · {}", stage + 1, next.stage.name);
        let facts = format!(
            "{} · {} · {} road · starts {} ({})",
            km(next.stage.length),
            pool::surface_word(next.stage.surface),
            road_word(next.conditions),
            clock_hm(next.clock),
            time_word(next.clock)
        );
        kit::panel(ui, |rows| {
            rows.full(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.add_space(9.0);
                    ui.label(RichText::new(title).size(14.0).family(semibold()).color(TEXT));
                    ui.label(RichText::new(facts).color(TEXT_2));
                    ui.add_space(9.0);
                });
            });
            if let Some(rivals) = rivals {
                row(rows, "Position", "", |ui| _ = ui.label(RichText::new(rivals).color(TEXT)));
            }
            if let Some(road) = road {
                row(rows, "Start order", "", |ui| _ = ui.label(RichText::new(road).color(TEXT)));
            }
            row(rows, "Tyres", "", |ui| {
                if choosing {
                    ComboBox::from_id_salt("championship-tyre")
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
                    let text = format!("{}, kept: no service before", crate::catalog::tyre_label(tyre));
                    ui.label(RichText::new(text).color(TEXT_2));
                }
            });
            row(rows, "Setup", "", |ui| {
                if !choosing {
                    ui.label(RichText::new(format!("Kept from SS{stage}")).color(TEXT_2));
                    return;
                }
                if ui.button("Edit…").clicked() {
                    request = Some(Request::Setup);
                }
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
                ComboBox::from_id_salt("championship-setup").width(170.0).selected_text(label(&self.setup)).show_ui(
                    ui,
                    |ui| {
                        for row in rows {
                            if ui.selectable_label(row == self.setup, label(&row)).clicked() {
                                self.setup = row;
                            }
                        }
                    },
                );
            });
        });
        if let Some(penalty) = self.run.as_ref().map(|r| r.penalty).filter(|p| *p > 0.0) {
            ui.colored_label(ERROR, format!("Late from service: {penalty:.0} s go on this stage's time."));
        }
        request
    }

    /// The pinned footer: the step's summary on the left and its primary button on the right (the park has its
    /// own in its card).
    fn footer_ui(&mut self, ui: &mut Ui, env: &Env, event: &Event, rally: usize) -> Option<Request> {
        let mut request = None;
        if matches!(event.turn, Turn::Service { .. }) && rejoin(event).is_none() {
            return None;
        }
        kit::footer(ui, "championship-footer", |ui| {
            let width = ui.available_width();
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(Vec2::new(width * 0.5, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    let (title, line) = self.footer_text(event, rally);
                    ui.label(RichText::new(title).family(semibold()).color(TEXT));
                    ui.label(RichText::new(line).size(12.0).color(TEXT_2));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| match event.turn {
                    Turn::Welcome { .. } => {
                        if ui.add(kit::primary_button("Start the rally", None)).clicked() {
                            self.turn_done(env.here, env.profile);
                        }
                    }
                    Turn::Stage { rally, stage } => request = self.drive_footer(ui, env, rally, stage),
                    Turn::Service { .. } => {
                        let next = rejoin(event).unwrap_or(0);
                        if ui.add(kit::primary_button(&format!("Restart on SS{}", next + 1), None)).clicked() {
                            self.turn_done(env.here, env.profile);
                        }
                    }
                    Turn::RallyOver { rally } => request = self.over_footer(ui, env, event, rally),
                    Turn::EventOver => {
                        ui.add_space(0.0);
                        if ui.add(kit::primary_button("New championship", None)).clicked() {
                            self.leave_event(env.here, env.profile);
                        }
                        ui.label(RichText::new("back to the presets").size(12.0).color(TEXT_3));
                    }
                });
            });
        });
        request
    }

    /// The footer's two lines for the turn.
    fn footer_text(&self, event: &Event, rally: usize) -> (String, String) {
        let Some(r) = event.rallies.get(rally) else { return (String::new(), String::new()) };
        match event.turn {
            Turn::Welcome { .. } => (format!("Welcome to {}", r.name), step(event)),
            Turn::Stage { stage, .. } => match r.stages.get(stage) {
                Some(s) => {
                    let road = road_line(event, rally, stage)
                        .map_or(String::new(), |l| format!(" · {}", l.split(" · ").next().unwrap_or("")));
                    let line = format!(
                        "{} · {} · {} road{road}",
                        km(s.stage.length),
                        pool::surface_word(s.stage.surface),
                        road_word(s.conditions)
                    );
                    (format!("SS{} · {}", stage + 1, s.stage.name), line)
                }
                None => (String::new(), String::new()),
            },
            Turn::Service { .. } => ("Super Rally".to_owned(), step(event)),
            Turn::RallyOver { .. } => match event.rallies.get(rally + 1) {
                Some(next) => {
                    let length: f32 = next.stages.iter().map(|s| s.stage.length).sum();
                    let mut surfaces: Vec<&str> =
                        next.stages.iter().map(|s| pool::surface_word(s.stage.surface)).collect();
                    surfaces.dedup();
                    (
                        format!("Next: R{} · {}", rally + 2, next.name),
                        format!("{} · {} · Practice first, not recorded", surfaces.join(", "), km(length)),
                    )
                }
                None => ("The last rally is over".to_owned(), "On to the final result".to_owned()),
            },
            Turn::EventOver => ("The championship is over".to_owned(), String::new()),
        }
    }

    /// The footer of the next stage: Drive, and why it cannot be (added right to left).
    fn drive_footer(&mut self, ui: &mut Ui, env: &Env, _rally: usize, stage: usize) -> Option<Request> {
        let mut request = None;
        let refused = env.no_records;
        let enabled = !self.driving && env.blocker.is_none() && !refused;
        let go = ui.add_enabled(enabled, kit::primary_button(&format!("Drive SS{}", stage + 1), Some("↵")));
        // Enter on a focused control presses it instead.
        let enter = ui.memory(|m| m.focused().is_none()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if go.clicked() || (enabled && enter) {
            match self.stage_start(env.here) {
                Ok(start) => request = Some(Request::Launch(start)),
                Err(e) => self.status = e,
            }
        }
        if self.driving {
            crate::kit::status_dot(ui, TEXT_3, "The stage is being driven.");
        } else if let Some(issue) = env.blocker {
            crate::kit::status_dot(ui, WARN, &issue.text);
        } else if refused {
            crate::kit::status_dot(ui, WARN, super::launch::CHAMPIONSHIP_NGP_ONLY);
        }
        request
    }

    /// The footer of a rally's end: Next rally (or the final result) and the two practice trials.
    fn over_footer(&mut self, ui: &mut Ui, env: &Env, event: &Event, rally: usize) -> Option<Request> {
        let mut request = None;
        let next = event.rallies.get(rally + 1);
        let label = match next {
            Some(next) => format!("Next rally: {}", next.name),
            None => "To the final result".to_owned(),
        };
        if ui.add_enabled(!self.driving, kit::primary_button(&label, None)).clicked() {
            self.turn_done(env.here, env.profile);
        }
        if next.is_some() {
            // The game's two trials (`S_YARD_TRIAL`, `S_STAGE_TRIAL`): practice that is not recorded.
            for (label, yard, tip) in [
                (
                    "Stage trial",
                    false,
                    "A practice drive on the next rally's first stage, on its surface; not recorded",
                ),
                ("Yard trial", true, "A practice drive in the yard; the championship does not record it"),
            ] {
                let trial = ui.add_enabled(!self.driving && env.blocker.is_none(), egui::Button::new(label));
                if trial.on_hover_text(tip).clicked() {
                    match self.trial(yard) {
                        Ok(start) => request = Some(Request::Launch(start)),
                        Err(e) => self.status = e,
                    }
                }
            }
            if let Some(issue) = env.blocker.filter(|_| !self.driving) {
                crate::kit::status_dot(ui, WARN, &issue.text);
            }
        }
        request
    }

    /// The rally's result: where the player finished, the points, the podium and the season's fight.
    fn rally_over_ui(&mut self, ui: &mut Ui, event: &Event, rally: usize, lines: &[Standing]) {
        let name = event.rallies.get(rally).map_or("The rally", |r| r.name.as_str());
        let out = place(lines).is_none();
        let retired = driven(event, rally).iter().rposition(|(p, _)| p.is_some_and(|p| p.retired)).filter(|_| out);
        let wins = stage_winners(event, rally);
        let mine = wins.iter().filter(|w| **w == Some(None)).count();
        card(ui, |ui| {
            match retired {
                Some(stage) => card_title(ui, &format!("Out of {name} on SS{}", stage + 1)),
                None => card_title(ui, &format!("{name} is over")),
            }
            if let Some((at, of)) = place(lines) {
                let points = lines[at - 1].points;
                let text = format!("You finished {} of {of} · {points} points", ordinal(at));
                ui.label(RichText::new(text).size(15.0).family(semibold()).color(WARN));
            }
            if let Some(line) = title_line(event).filter(|_| rally + 1 < event.rallies.len()) {
                ui.colored_label(TEXT_2, line);
            }
            if mine > 0 {
                ui.colored_label(TEXT_2, format!("You won {mine} of the rally's {} stages.", wins.len()));
            }
        });
        podium_ui(ui, event, lines);
    }

    /// The championship's end: the champion or the player's finish, the record and the podium.
    fn event_over_ui(&mut self, ui: &mut Ui, event: &Event, rally: &[Standing], points: Option<&[Standing]>) {
        let season = points.is_some();
        let lines = points.unwrap_or(rally);
        card(ui, |ui| {
            card_title(ui, if season { "The season is over" } else { "The championship is over" });
            if let Some(at) = lines.iter().position(|l| l.entrant.is_none()) {
                let total = lines[at].points;
                let text = match (at, season) {
                    (0, true) => format!("Champion, with {total} points."),
                    (0, false) => "You won.".to_owned(),
                    (_, true) => format!("You finished {} of {} with {total} points.", ordinal(at + 1), lines.len()),
                    (_, false) if lines[at].total.is_none() => "You retired.".to_owned(),
                    (_, false) => format!("You finished {} of {}.", ordinal(at + 1), lines.len()),
                };
                ui.label(RichText::new(text).size(15.0).family(semibold()).color(WARN));
                if let Some(winner) = lines.first().filter(|l| l.entrant.is_some()) {
                    let by = if season { format!(", {} points", winner.points) } else { String::new() };
                    let who = driver(event, winner.entrant);
                    ui.colored_label(TEXT_2, format!("{}: {who}{by}", if season { "Champion" } else { "Winner" }));
                }
                if season {
                    ui.colored_label(TEXT_2, record_line(&record(event, None)));
                }
            }
        });
        if !season {
            podium_ui(ui, event, rally);
        }
    }
}

/// The last stage driven in the turn's rally, while its result is news (not at a welcome or the championship's end).
fn last_stage(event: &Event) -> Option<(usize, usize)> {
    let rally = match event.turn {
        Turn::Stage { rally, stage } if stage > 0 => rally,
        Turn::Service { rally, .. } | Turn::RallyOver { rally } => rally,
        _ => return None,
    };
    let done = driven(event, rally).len();
    Some((rally, done.checked_sub(1)?))
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

/// The stage card's road line: "14th on the road · worn road", none without a road order.
fn road_line(event: &Event, rally: usize, stage: usize) -> Option<String> {
    let at = event.start_order(rally, stage).iter().position(Option::is_none)?;
    let wear = ["fresh", "used", "worn"][event.road_wear(rally, stage) as usize % 3];
    Some(format!("{} on the road · {wear} road", ordinal(at + 1)))
}

/// The bulletins' lines shown without "show all".
const NEWS_SHOWN: usize = 8;

/// The news since the player's last stage: the last few lines, or all of them.
fn bulletins_ui(ui: &mut Ui, title: &str, news: &[String], all: &mut bool) {
    kit::group_label(ui, title, None);
    kit::panel(ui, |rows| {
        let hidden = if *all { 0 } else { news.len().saturating_sub(NEWS_SHOWN) };
        for line in &news[hidden..] {
            rows.full(|ui| _ = ui.add(egui::Label::new(RichText::new(line).color(TEXT_LIST)).wrap()));
        }
        if news.len() > NEWS_SHOWN {
            rows.full(|ui| *all ^= kit::link(ui, if *all { "Show fewer" } else { "Show all" }).clicked());
        }
    });
}

/// The stage card's rally line: the player's place and the gaps to the drivers either side, "3rd · 4.2 s behind
/// T. Moreau · 6.1 s ahead of D. Novak"; none while the player has no total.
fn rivals_line(event: &Event, rally: usize) -> Option<String> {
    let r = rivals(&event.rally_standings(rally))?;
    let mut parts = vec![if r.place == 1 { "Leading".to_owned() } else { ordinal(r.place) }];
    if let Some((who, by)) = r.ahead {
        parts.push(format!("{} behind {}", seconds(by), driver(event, who)));
    }
    if let Some((who, by)) = r.behind {
        parts.push(format!("{} ahead of {}", seconds(by), driver(event, who)));
    }
    Some(parts.join(" · "))
}

/// The rally's top three with their totals and gaps.
fn podium_ui(ui: &mut Ui, event: &Event, lines: &[Standing]) {
    let best = lines.first().and_then(|l| l.total).unwrap_or(0.0);
    let cols =
        [Col::new("", 30.0, false), Col::new("", 0.0, false), Col::new("", 84.0, true), Col::new("", 70.0, true)];
    let rows = lines
        .iter()
        .take(3)
        .enumerate()
        .map_while(|(at, line)| {
            let total = line.total?;
            Some(Line {
                cells: vec![
                    Cell::num(format!("{}", at + 1), WARN),
                    Cell::text(driver(event, line.entrant), if line.entrant.is_none() { TEXT } else { TEXT_LIST }),
                    Cell::num(clock(total), TEXT),
                    Cell::num(gap(total, best), TEXT_2),
                ],
                you: line.entrant.is_none(),
                tip: None,
            })
        })
        .collect();
    table(ui, "championship-podium", &cols, rows, 200.0);
}

/// The player's season in a line: "2 rally wins · 4 podiums · 11 stage wins · 1 retirement".
fn record_line(r: &Record) -> String {
    let count = |n: usize, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
    let mut parts = vec![count(r.wins, "rally win"), count(r.podiums, "podium"), count(r.stage_wins, "stage win")];
    if r.retirements > 0 {
        parts.push(count(r.retirements, "retirement"));
    }
    parts.join(" · ")
}

/// A season's title fight: the player's place in the points, the gap to the leader (or the lead over second), and the
/// points still to win, "3rd in the season · 12 pts behind H. Weber · 60 pts still to win"; none outside a season or
/// before any points.
pub(super) fn title_line(event: &Event) -> Option<String> {
    let lines = event.points();
    if event.rallies.len() < 2 || lines.iter().all(|l| l.points == 0) {
        return None;
    }
    let at = lines.iter().position(|l| l.entrant.is_none())?;
    let left = (0..event.rallies.len()).filter(|&r| !rally_over(event, r)).count() as u32;
    let bonus = if event.config.power_stage { POWER_POINTS[0] } else { 0 };
    let still = left * (POINTS[0] + bonus);
    let (mine, leader) = (lines[at].points, lines[0].points);
    let mut parts = vec![format!("{} in the season", ordinal(at + 1))];
    if at > 0 {
        parts.push(format!("{} pts behind {}", leader - mine, driver(event, lines[0].entrant)));
    } else if let Some(second) = lines.get(1) {
        parts.push(format!("{} pts ahead of {}", mine - second.points, driver(event, second.entrant)));
    }
    if left > 0 {
        parts.push(format!("{still} pts still to win"));
    }
    Some(parts.join(" · "))
}
