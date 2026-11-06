//! The championship's cards: its header, a stage's result, the itinerary, the rally's standings and the points.

use egui::{Color32, RichText, Ui};
use rbr_season::config::Damage;
use rbr_season::event::{Event, Standing, Turn};

use super::common::{card, card_title, clock, gap, ordinal, StageMark, BAD};
use super::results::*;
use super::view::*;
use super::*;
use crate::browser::{DIM, GREY, STAR};
use crate::pages::{GOOD, WARN};

/// The championship's header: the rally, where it stands and the player's place, then the car and the settings.
pub(super) fn header_ui(ui: &mut Ui, event: &Event, lines: &[Standing], cars: &[Car]) {
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
        let season = matches!(event.turn, Turn::EventOver) && event.rallies.len() > 1;
        if let Some(at) = season.then(|| event.points().iter().position(|l| l.entrant.is_none())).flatten() {
            ui.colored_label(DIM, big("·".into()));
            ui.label(big(format!("{} in the season", ordinal(at + 1))));
        } else if let Some((at, of)) = place(lines).filter(|_| !driven(event, rally).is_empty()) {
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

/// The rows a stage's result lists: its top five, then the player's row when further down.
const STAGE_TOP: usize = 5;

/// A stage's result: its fastest drivers and the player's row, the player's place on it and the places it moved them
/// overall. `picked`: the stage was picked on the itinerary, with a link back to the latest; whether it was clicked.
pub(super) fn stage_result_ui(ui: &mut Ui, event: &Event, rally: usize, stage: usize, picked: bool) -> bool {
    let Some(s) = event.rallies.get(rally).and_then(|r| r.stages.get(stage)) else { return false };
    let times = stage_times(event, rally, stage);
    let mine = times.iter().position(|(e, _)| e.is_none());
    let mut back = false;
    card(ui, |ui| {
        ui.horizontal(|ui| {
            card_title(ui, &format!("SS{} · {} result", stage + 1, s.stage.name));
            if picked {
                back = ui.link("Latest").clicked();
            }
        });
        let best = times.first().map_or(0.0, |t| t.1);
        let rows = (0..times.len().min(STAGE_TOP)).chain(mine.filter(|&m| m >= STAGE_TOP));
        // The power stage's bonus, once the rally is over.
        let bonus = |who: Option<usize>| {
            let last = stage + 1 == event.rallies[rally].stages.len() && rally_over(event, rally);
            let lines = (last && event.config.power_stage).then(|| event.rally_standings(rally));
            lines.and_then(|lines| lines.iter().find(|l| l.entrant == who).map(|l| l.bonus)).filter(|b| *b > 0)
        };
        egui::Grid::new(("championship-stage-result", rally, stage)).spacing([12.0, 3.0]).show(ui, |ui| {
            for k in rows {
                let (who, time) = times[k];
                ui.colored_label(GREY, format!("{}", k + 1));
                let name = RichText::new(driver(event, who));
                ui.label(if who.is_none() { name.strong().color(STAR) } else { name });
                ui.label(clock(time));
                ui.colored_label(GREY, gap(time, best));
                ui.colored_label(STAR, bonus(who).map_or(String::new(), |b| format!("+{b} pts")));
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        let overall = |ui: &mut Ui| {
            let now = event.standings_after(rally, stage + 1);
            let Some((at, _)) = place(&now) else { return };
            ui.label(format!("{} overall", ordinal(at)));
            let moved = (stage > 0).then(|| place_change(&event.standings_after(rally, stage), &now, None)).flatten();
            match moved {
                Some(n) if n > 0 => _ = ui.colored_label(GOOD, format!("up {n}")),
                Some(n) if n < 0 => _ = ui.colored_label(BAD, format!("down {}", -n)),
                _ => {}
            }
        };
        match mine {
            Some(0) => {
                ui.horizontal(|ui| {
                    let by = times.get(1).map_or(String::new(), |t| format!(" by {}", seconds(t.1 - best)));
                    ui.colored_label(STAR, format!("You won the stage{by}"));
                    ui.colored_label(DIM, "·");
                    overall(ui);
                });
            }
            Some(at) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("You were {} on the stage", ordinal(at + 1))).color(STAR));
                    ui.colored_label(DIM, "·");
                    overall(ui);
                });
            }
            None => match event.charged(rally, stage) {
                Some(time) => {
                    _ = ui.colored_label(
                        BAD,
                        format!("Super Rally: you are charged {}, the best time plus 5:00.", clock(time)),
                    )
                }
                None => _ = ui.colored_label(BAD, "You did not finish the stage."),
            },
        }
    });
    back
}

/// The rally's stages and services: each stage's start, length, road and the player's time once driven. A driven
/// stage's name picks it for the result card (`shown`); the one clicked, if any.
pub(super) fn itinerary_ui(ui: &mut Ui, event: &Event, rally: usize, shown: Option<usize>) -> Option<usize> {
    let r = event.rallies.get(rally)?;
    let mut picked = None;
    let results = driven(event, rally);
    let mark = |k: usize| match results.get(k) {
        Some((Some(p), _)) if !p.retired => StageMark::Done(p.time),
        Some(_) => event.charged(rally, k).map_or(StageMark::Retired, StageMark::Charged),
        None => match event.turn {
            Turn::Stage { rally: at, stage } if at == rally && stage == k => StageMark::Next,
            Turn::Service { rally: at, stage, .. } if at == rally && stage + 1 == k => StageMark::Next,
            _ => StageMark::Later,
        },
    };
    card(ui, |ui| {
        card_title(ui, "Itinerary");
        egui::Grid::new("championship-itinerary").num_columns(8).spacing([12.0, 5.0]).show(ui, |ui| {
            for heading in ["", "", "Stage", "", "Length", "Start", "Road", "Time"] {
                ui.colored_label(GREY, heading);
            }
            ui.end_row();
            for (k, s) in r.stages.iter().enumerate() {
                let m = mark(k);
                let (icon, colour) = match m {
                    StageMark::Done(_) => ("✔", GOOD),
                    StageMark::Retired | StageMark::Charged(_) => ("✖", WARN),
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
                let power = event.config.power_stage && k + 1 == r.stages.len();
                if k < results.len() {
                    let name = RichText::new(&s.stage.name);
                    let name = if shown == Some(k) { name.strong().color(STAR) } else { name };
                    let name = ui.add(egui::Label::new(name).sense(egui::Sense::click()));
                    let name = name.on_hover_cursor(egui::CursorIcon::PointingHand);
                    if name.on_hover_text("Show the stage's times").clicked() {
                        picked = Some(k);
                    }
                } else {
                    ui.label(strong(s.stage.name.clone()));
                }
                if power {
                    ui.colored_label(STAR, "PS").on_hover_text("Power stage: its five fastest score 5-4-3-2-1");
                } else {
                    ui.label("");
                }
                ui.colored_label(GREY, km(s.stage.length));
                ui.colored_label(GREY, clock_hm(s.clock));
                ui.colored_label(GREY, road_word(s.conditions));
                match m {
                    StageMark::Done(time) => _ = ui.label(clock(time)),
                    StageMark::Charged(time) => {
                        let text = "Super Rally: the stage's best time plus 5:00";
                        ui.colored_label(BAD, clock(time)).on_hover_text(text);
                    }
                    StageMark::Retired => _ = ui.label("out"),
                    _ => _ = ui.label(""),
                }
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
        let driven =
            results.iter().filter_map(|(p, _)| p.as_ref()).filter(|p| !p.retired).map(|p| p.time + p.penalties);
        let charges = (0..results.len()).filter_map(|k| event.charged(rally, k));
        let total: f32 = driven.chain(charges).sum();
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
    picked
}

/// The rally's standings after the stages run: place, the places the last stage moved, driver (the car on hover), the
/// last stage's time until the rally is over, total, gap, and the points once it is.
pub(super) fn standings_ui(ui: &mut Ui, event: &Event, rally: usize, lines: &[Standing], cars: &[Car]) {
    let ran = driven(event, rally).len();
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
        let points = ran == count;
        let power = points && event.config.power_stage;
        let stage = (!points).then(|| format!("SS{ran}"));
        let before = (ran > 1).then(|| event.standings_after(rally, ran - 1));
        egui::Grid::new("championship-standings").striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
            let headings = ["", "", "Driver"].into_iter().chain(stage.as_deref()).chain(["Total", "Gap"]);
            for heading in headings.chain(power.then_some("PS")).chain(points.then_some("Pts")) {
                ui.colored_label(GREY, heading);
            }
            ui.end_row();
            for (at, line) in lines.iter().enumerate() {
                let name = RichText::new(driver(event, line.entrant));
                let car =
                    line.entrant.and_then(|e| event.field.get(e)).map_or(event.config.car.as_str(), |e| e.car.as_str());
                ui.label(if line.total.is_some() { format!("{}", at + 1) } else { "–".into() });
                match before.as_ref().and_then(|before| place_change(before, lines, line.entrant)) {
                    Some(n) if n > 0 => _ = ui.colored_label(GOOD, format!("⬆{n}")),
                    Some(n) if n < 0 => _ = ui.colored_label(BAD, format!("⬇{}", -n)),
                    _ => _ = ui.label(""),
                }
                let name = if line.entrant.is_none() { name.strong().color(STAR) } else { name };
                ui.label(name).on_hover_text(car_name(cars, car));
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
                if power {
                    ui.colored_label(STAR, if line.bonus > 0 { format!("+{}", line.bonus) } else { String::new() });
                }
                if points {
                    ui.label(if line.points > 0 { line.points.to_string() } else { String::new() });
                }
                ui.end_row();
            }
        });
    });
}

/// The season's points: each driver's points from each rally (R: retired), and the total.
pub(super) fn points_ui(ui: &mut Ui, event: &Event, lines: &[Standing]) {
    card(ui, |ui| {
        card_title(ui, "Season points");
        if lines.iter().all(|l| l.points == 0) {
            ui.colored_label(GREY, "Points come at each rally's end.");
            return;
        }
        let finishes: Vec<_> = (0..event.rallies.len()).map(|r| rally_finishes(event, r)).collect();
        egui::Grid::new("championship-points").striped(true).spacing([10.0, 3.0]).show(ui, |ui| {
            ui.label("");
            ui.label("");
            for r in 0..finishes.len() {
                ui.colored_label(GREY, format!("R{}", r + 1)).on_hover_text(&event.rallies[r].name);
            }
            ui.colored_label(GREY, "Pts");
            ui.end_row();
            for (at, line) in lines.iter().enumerate() {
                let name = RichText::new(driver(event, line.entrant));
                ui.label(format!("{}", at + 1));
                ui.label(if line.entrant.is_none() { name.strong().color(STAR) } else { name });
                for rally in &finishes {
                    let finish = rally.as_ref().and_then(|f| f.iter().find(|(e, _)| *e == line.entrant));
                    match finish.map(|(_, f)| *f) {
                        Some(Finish { place: None, .. }) => _ = ui.colored_label(DIM, "R"),
                        Some(Finish { place: Some(p), points }) => {
                            let text = if points > 0 { points.to_string() } else { String::new() };
                            let colour = if p <= 3 { STAR } else { Color32::WHITE };
                            _ = ui.colored_label(colour, text).on_hover_text(ordinal(p));
                        }
                        None => _ = ui.label(""),
                    }
                }
                ui.label(RichText::new(line.points.to_string()).strong());
                ui.end_row();
            }
        });
    });
}

/// A season's calendar: each rally, its surface and length, the player's finish and its winner.
pub(super) fn calendar_ui(ui: &mut Ui, event: &Event) {
    let now = rally_of(event);
    let over = matches!(event.turn, Turn::EventOver);
    card(ui, |ui| {
        card_title(ui, "Calendar");
        egui::Grid::new("championship-calendar").spacing([12.0, 4.0]).show(ui, |ui| {
            for (k, r) in event.rallies.iter().enumerate() {
                let current = k == now && !over;
                let text = |t: String| {
                    if current {
                        RichText::new(t).strong().color(Color32::WHITE)
                    } else {
                        RichText::new(t)
                    }
                };
                ui.colored_label(STAR, if current { "▶" } else { "" });
                ui.label(text(format!("R{}", k + 1)));
                ui.label(text(r.name.clone()));
                let mut surfaces: Vec<&str> = r.stages.iter().map(|s| pool::surface_word(s.stage.surface)).collect();
                surfaces.dedup();
                let length: f32 = r.stages.iter().map(|s| s.stage.length).sum();
                let stages = format!("{} stages", r.stages.len());
                ui.colored_label(GREY, format!("{} · {}", surfaces.join(", "), km(length))).on_hover_text(stages);
                match rally_finishes(event, k) {
                    Some(finishes) => {
                        match finishes.iter().find(|(e, _)| e.is_none()).map(|(_, f)| *f) {
                            Some(Finish { place: Some(p), points }) => {
                                let colour = if p <= 3 { STAR } else { Color32::WHITE };
                                let points = if points > 0 { format!(" · {points} pts") } else { String::new() };
                                ui.colored_label(colour, format!("{}{points}", ordinal(p)));
                            }
                            _ => _ = ui.colored_label(BAD, "retired"),
                        }
                        let winner = finishes.first().filter(|(_, f)| f.place.is_some()).map(|(e, _)| *e);
                        let winner = ui.colored_label(GREY, winner.map_or(String::new(), |w| driver(event, w)));
                        winner.on_hover_text("The rally's winner");
                    }
                    None => {
                        let started = !driven(event, k).is_empty();
                        ui.colored_label(
                            GREY,
                            if !current {
                                ""
                            } else if started {
                                "under way"
                            } else {
                                "next"
                            },
                        );
                        ui.label("");
                    }
                }
                ui.end_row();
            }
        });
    });
}
