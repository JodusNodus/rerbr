//! The championship under way: the header, the last stage's result, the card of the next step with its one primary
//! button, the itinerary under it, the bulletins, and the rally's standings and the championship's points beside.

use std::path::Path;

use egui::{ComboBox, RichText, Ui};
use rbr_gameplay::setup::SeasonSetup;
use rbr_season::event::{Event, Standing, Turn};

use super::cards::*;
use super::common::{card, card_title, clock, gap, ordinal, park_card, text_of, BAD};
use super::results::*;
use super::view::*;
use super::*;
use crate::browser::{GREY, STAR};
use crate::pages::{fields, start_button, WARN};

impl ChampionshipPage {
    /// The championship under way: the header, the next step's card with its button, the itinerary, the standings.
    pub(super) fn event_ui(&mut self, ui: &mut Ui, env: Env) -> Option<Request> {
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
            // The stage picked on the itinerary, else the last one driven while it is news.
            let picked = self.shown.filter(|&(r, s)| r == rally && s < driven(&event, rally).len());
            if let Some((r, s)) = picked.or(last_stage(&event)) {
                if stage_result_ui(ui, &event, r, s, picked.is_some()) {
                    self.shown = None;
                }
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
            if let Some(stage) = itinerary_ui(ui, &event, rally, picked.map(|(_, s)| s)) {
                self.shown = Some((rally, stage));
            }
            let ui = &mut cols[1];
            if !news.is_empty() {
                bulletins_ui(ui, &news, &mut self.news_all);
                ui.add_space(10.0);
            }
            standings_ui(ui, &event, rally, &standings, env.cars);
            if let Some(points) = &points {
                ui.add_space(10.0);
                points_ui(ui, &event, points);
                ui.add_space(10.0);
                calendar_ui(ui, &event);
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
        let rivals = (stage > 0).then(|| rivals_line(event, rally)).flatten();
        fields(ui, "championship-stage", |ui| {
            if let Some(rivals) = rivals {
                ui.label("Rally");
                ui.label(rivals);
                ui.end_row();
            }
            if let Some(road) = road {
                ui.label("Start order");
                ui.label(road);
                ui.end_row();
            }
            ui.label("Tyres");
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
                    ComboBox::from_id_salt("championship-setup")
                        .width(200.0)
                        .selected_text(label(&self.setup))
                        .show_ui(ui, |ui| {
                            for row in rows {
                                if ui.selectable_label(row == self.setup, label(&row)).clicked() {
                                    self.setup = row;
                                }
                            }
                        });
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
        let retired = driven(event, rally).iter().position(|(p, _)| p.is_none_or(|p| p.retired));
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
        let wins = stage_winners(event, rally);
        let mine = wins.iter().filter(|w| **w == Some(None)).count();
        if mine > 0 {
            ui.colored_label(GREY, format!("You won {mine} of the rally's {} stages.", wins.len()));
        }
        ui.add_space(6.0);
        podium_ui(ui, event, lines);
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
                let by = if season { format!(", {} points", winner.points) } else { String::new() };
                ui.colored_label(
                    GREY,
                    format!("{}: {}{by}", if season { "Champion" } else { "Winner" }, driver(event, winner.entrant)),
                );
            }
            if season {
                ui.colored_label(GREY, record_line(&record(event, None)));
            }
        }
        if !season {
            ui.add_space(6.0);
            podium_ui(ui, event, rally);
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
    egui::Grid::new("championship-podium").spacing([12.0, 3.0]).show(ui, |ui| {
        for (at, line) in lines.iter().take(3).enumerate() {
            let Some(total) = line.total else { break };
            ui.colored_label(STAR, format!("{}", at + 1));
            let name = RichText::new(driver(event, line.entrant));
            ui.label(if line.entrant.is_none() { name.strong().color(STAR) } else { name });
            ui.label(clock(total));
            ui.colored_label(GREY, gap(total, best));
            ui.end_row();
        }
    });
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
