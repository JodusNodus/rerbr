//! The championship's cards: its header, a stage's result, the itinerary, the rally's standings and the points.

use egui::{Color32, RichText, Ui};
use rbr_season::config::Damage;
use rbr_season::event::{Event, Standing, Turn};

use super::common::{card, card_title, clock, gap, ordinal, StageMark, BAD};
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
        if let Some((at, of)) = place(lines).filter(|_| !driven(event, rally).is_empty()) {
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
pub(super) fn stage_result_ui(ui: &mut Ui, event: &Event, rally: usize, stage: usize) {
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
pub(super) fn itinerary_ui(ui: &mut Ui, event: &Event, rally: usize) {
    let Some(r) = event.rallies.get(rally) else { return };
    let results = driven(event, rally);
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
        egui::Grid::new("championship-itinerary").num_columns(7).spacing([12.0, 5.0]).show(ui, |ui| {
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
        let points = lines.iter().any(|l| l.points > 0);
        // The last stage's column from SS2 on, until the points take its place.
        let stage = (ran > 1 && !points).then(|| format!("SS{ran}"));
        egui::Grid::new("championship-standings").striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
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
pub(super) fn points_ui(ui: &mut Ui, event: &Event, lines: &[Standing]) {
    card(ui, |ui| {
        card_title(ui, "Season points");
        if lines.iter().all(|l| l.points == 0) {
            ui.colored_label(GREY, "Points come at each rally's end.");
            return;
        }
        egui::Grid::new("championship-points").striped(true).spacing([14.0, 3.0]).show(ui, |ui| {
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
