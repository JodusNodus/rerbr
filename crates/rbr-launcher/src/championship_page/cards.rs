//! The championship's cards: its header, the itinerary strip, a stage's result, the rally's standings and the points.

use egui::{Align, Color32, Layout, RichText, Sense, Stroke, Ui, Vec2};
use rbr_season::config::Damage;
use rbr_season::event::{Event, Standing, Turn};

use super::common::{card, clock, gap, ordinal, StageMark, BAD};
use super::results::*;
use super::table::{table, Cell, Col, Line};
use super::view::*;
use super::*;
use crate::kit;
use crate::theme::*;

use crate::theme::{GOOD, TEXT_2 as GREY, TEXT_3 as DIM, WARN, WARN as STAR};

/// The page's header: a link back to the presets, the rally, where it stands and the player's place, then the car
/// and the settings. Whether the link was clicked.
pub(super) fn header_ui(ui: &mut Ui, event: &Event, lines: &[Standing], cars: &[Car]) -> bool {
    let rally = rally_of(event);
    let margin = egui::Margin { left: kit::PAGE_LEFT, right: kit::PAGE_RIGHT, top: 14, bottom: 14 };
    egui::Frame::new()
        .inner_margin(margin)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let back = kit::link(ui, "‹ Championship").clicked();
            ui.horizontal_wrapped(|ui| {
                let name = event.rallies.get(rally).map_or("", |r| r.name.as_str());
                ui.label(RichText::new(name).font(title()).color(TEXT));
                ui.add_space(4.0);
                let mut parts = Vec::new();
                if event.rallies.len() > 1 {
                    parts.push(format!("Rally {} of {}", rally + 1, event.rallies.len()));
                }
                parts.push(step(event));
                let season = matches!(event.turn, Turn::EventOver) && event.rallies.len() > 1;
                if let Some(at) = season.then(|| event.points().iter().position(|l| l.entrant.is_none())).flatten() {
                    parts.push(format!("{} in the season", ordinal(at + 1)));
                } else if let Some((at, of)) = place(lines).filter(|_| !driven(event, rally).is_empty()) {
                    parts.push(format!("{} of {of}", ordinal(at)));
                }
                ui.label(RichText::new(parts.join(" · ")).size(13.0).color(STAR));
            });
            let c = &event.config;
            let damage = match c.damage {
                Damage::Off => "no damage",
                Damage::Reduced => "reduced damage",
                Damage::Realistic => "realistic damage",
            };
            let adaptive = if c.adaptive { " · adaptive" } else { "" };
            let line = format!(
                "{} · {} · {}{adaptive} · {damage}",
                car_name(cars, &c.car),
                label_class(&c.class),
                difficulty_word(c.difficulty)
            );
            ui.label(RichText::new(line).color(GREY));
            back
        })
        .inner
}

/// A stage's result in lines: the player's place on it, what it did overall (up or down) and the Super Rally charge
/// or the retirement. `picked`: the stage was picked on the itinerary, with a link back to the latest; whether it
/// was clicked.
pub(super) fn stage_summary(ui: &mut Ui, event: &Event, rally: usize, stage: usize, picked: bool) -> bool {
    let Some(s) = event.rallies.get(rally).and_then(|r| r.stages.get(stage)) else { return false };
    let times = stage_times(event, rally, stage);
    let mine = times.iter().position(|(e, _)| e.is_none());
    let best = times.first().map_or(0.0, |t| t.1);
    let mut back = false;
    ui.label(RichText::new(format!("SS{} · {} result", stage + 1, s.stage.name)).family(medium()).color(TEXT));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let overall = |ui: &mut Ui| {
            let now = event.standings_after(rally, stage + 1);
            let Some((at, _)) = place(&now) else { return };
            ui.label(RichText::new(format!("{} overall", ordinal(at))).color(TEXT_LABEL));
            let moved = (stage > 0).then(|| place_change(&event.standings_after(rally, stage), &now, None)).flatten();
            match moved {
                Some(n) if n > 0 => _ = ui.colored_label(GOOD, format!("up {n}")),
                Some(n) if n < 0 => _ = ui.colored_label(BAD, format!("down {}", -n)),
                _ => {}
            }
        };
        match mine {
            Some(at) => {
                let line = match at {
                    0 => {
                        let by = times.get(1).map_or(String::new(), |t| format!(" by {}", seconds(t.1 - best)));
                        format!("You won SS{}{by}", stage + 1)
                    }
                    at => format!("You were {} on SS{}", ordinal(at + 1), stage + 1),
                };
                ui.label(RichText::new(line).color(STAR));
                ui.colored_label(DIM, "·");
                overall(ui);
            }
            None => match event.charged(rally, stage, None) {
                Some(time) => {
                    let line = format!("Super Rally: you are charged {}, the fastest time plus 10:00.", clock(time));
                    _ = ui.colored_label(BAD, line)
                }
                None => _ = ui.colored_label(BAD, format!("You did not finish SS{}.", stage + 1)),
            },
        }
        if picked {
            back = kit::link(ui, "Latest").clicked();
        }
    });
    back
}

/// A stage's times: every finisher, fastest first, the player's row lit, and the power stage's bonus once the rally
/// is over.
pub(super) fn stage_times_ui(ui: &mut Ui, event: &Event, rally: usize, stage: usize) {
    let times = stage_times(event, rally, stage);
    let best = times.first().map_or(0.0, |t| t.1);
    let bonus = |who: Option<usize>| {
        let last = stage + 1 == event.rallies[rally].stages.len() && rally_over(event, rally);
        let lines = (last && event.config.power_stage).then(|| event.rally_standings(rally));
        lines.and_then(|lines| lines.iter().find(|l| l.entrant == who).map(|l| l.bonus)).filter(|b| *b > 0)
    };
    let cols = [
        Col::new("", 30.0, false),
        Col::new("Driver", 0.0, false),
        Col::new("Time", 76.0, true),
        Col::new("Gap", 70.0, true),
        Col::new("", 52.0, true),
    ];
    let lines = times
        .iter()
        .enumerate()
        .map(|(k, &(who, time))| Line {
            cells: vec![
                Cell::num(format!("{}", k + 1), DIM),
                Cell::text(driver(event, who), if who.is_none() { TEXT } else { TEXT_LIST }),
                Cell::num(clock(time), TEXT),
                Cell::num(gap(time, best), GREY),
                Cell::num(bonus(who).map_or(String::new(), |b| format!("+{b} pts")), STAR),
            ],
            you: who.is_none(),
            tip: None,
        })
        .collect();
    table(ui, "championship-stage-times", &cols, lines, 420.0);
}

/// The itinerary strip: a tile for each stage and service of the rally (SSn and the player's place on it, the name,
/// length and road, the time or the start), scrolling sideways. A driven stage's tile picks it for the standings'
/// times (`shown`); the one clicked, if any. `scroll`: bring the turn's tile into view.
pub(super) fn strip_ui(ui: &mut Ui, event: &Event, rally: usize, shown: Option<usize>, scroll: bool) -> Option<usize> {
    let r = event.rallies.get(rally)?;
    let mut picked = None;
    let results = driven(event, rally);
    let mark = |k: usize| match results.get(k) {
        Some((Some(p), _)) if !p.retired => StageMark::Done(p.time),
        Some(_) => event.charged(rally, k, None).map_or(StageMark::Retired, StageMark::Charged),
        None => match event.turn {
            Turn::Stage { rally: at, stage } if at == rally && stage == k => StageMark::Next,
            _ => StageMark::Later,
        },
    };
    const W: f32 = 150.0;
    egui::ScrollArea::horizontal().id_salt("championship-strip").auto_shrink([false, true]).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (k, s) in r.stages.iter().enumerate() {
                let m = mark(k);
                let power = event.config.power_stage && k + 1 == r.stages.len();
                let on = shown == Some(k);
                let clickable = k < results.len();
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(W, 84.0), if clickable { Sense::click() } else { Sense::hover() });
                let (fill, border) = match (m, on) {
                    (_, true) => (ROW_SELECTED, ACCENT),
                    (StageMark::Next, _) => (ROW_SELECTED, ACCENT),
                    _ => (PANEL, if response.hovered() && clickable { LINE_HOVER } else { LINE }),
                };
                let painter = ui.painter();
                painter.rect(rect, 8, fill, Stroke::new(1.0, border), egui::StrokeKind::Inside);
                let pad = 11.0;
                let (left, right) = (rect.left() + pad, rect.right() - pad);
                let small = egui::FontId::new(11.5, egui::FontFamily::Proportional);
                let tag = if power { format!("SS{} · PS", k + 1) } else { format!("SS{}", k + 1) };
                let galley = painter.layout_no_wrap(tag, small.clone(), TEXT_3);
                painter.galley(egui::pos2(left, rect.top() + 9.0), galley, TEXT_3);
                let (side, side_color) = match m {
                    StageMark::Next => ("Next".to_owned(), LINK),
                    StageMark::Done(_) => (
                        stage_times(event, rally, k)
                            .iter()
                            .position(|(e, _)| e.is_none())
                            .map_or(String::new(), |at| ordinal(at + 1)),
                        TEXT_3,
                    ),
                    StageMark::Charged(_) => ("charged".to_owned(), ERROR),
                    StageMark::Retired => ("out".to_owned(), WARN),
                    StageMark::Later => (String::new(), TEXT_3),
                };
                let galley = painter.layout_no_wrap(side, small.clone(), side_color);
                painter.galley(egui::pos2(right - galley.size().x, rect.top() + 9.0), galley, side_color);
                let clip = painter.with_clip_rect(rect.shrink2(Vec2::new(pad - 2.0, 0.0)));
                let name = clip.layout_no_wrap(s.stage.name.clone(), egui::FontId::new(13.0, medium()), TEXT_LIST);
                clip.galley(egui::pos2(left, rect.top() + 28.0), name, TEXT_LIST);
                let road = format!("{} · {}", km(s.stage.length), road_word(s.conditions));
                let galley = clip.layout_no_wrap(road, mono(11.0), TEXT_3);
                clip.galley(egui::pos2(left, rect.top() + 49.0), galley, TEXT_3);
                let (line, color) = match m {
                    StageMark::Done(time) => (clock(time), TEXT_2),
                    StageMark::Charged(time) => (clock(time), ERROR),
                    StageMark::Retired => ("out".to_owned(), WARN),
                    _ => (format!("start {}", clock_hm(s.clock)), TEXT_3),
                };
                let driven = !matches!(m, StageMark::Next | StageMark::Later);
                let galley = clip.layout_no_wrap(line, mono(11.0), color);
                clip.galley(egui::pos2(left, rect.top() + 64.0), galley, color);
                if driven {
                    // The start clock stays once the stage is driven.
                    let galley = clip.layout_no_wrap(clock_hm(s.clock), mono(11.0), TEXT_3);
                    clip.galley(egui::pos2(right - galley.size().x, rect.top() + 64.0), galley, TEXT_3);
                }
                if scroll && m == StageMark::Next && !matches!(event.turn, Turn::Service { .. }) {
                    response.scroll_to_me(Some(Align::Center));
                }
                let response = if clickable {
                    response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Show the stage's times")
                } else {
                    response
                };
                if matches!(m, StageMark::Charged(_)) {
                    response.clone().on_hover_text("Super Rally: the stage's fastest time plus 10:00");
                }
                if power {
                    response.clone().on_hover_text("Power stage: its five fastest score 5-4-3-2-1");
                }
                if response.clicked() {
                    picked = Some(k);
                }
                if let Some(minutes) = s.service_after.filter(|_| k + 1 < r.stages.len()) {
                    let at = matches!(event.turn, Turn::Service { rally: a, stage, .. } if a == rally && stage == k);
                    let (rect, response) = ui.allocate_exact_size(Vec2::new(110.0, 84.0), Sense::hover());
                    let color = if at { ACCENT } else { LINE_BUTTON };
                    let c =
                        [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
                    let painter = ui.painter();
                    painter.extend(egui::Shape::dashed_line(&c, Stroke::new(1.0, color), 4.0, 3.0));
                    if at {
                        painter.rect_filled(rect.shrink(1.0), 8, ROW_SELECTED);
                    }
                    let text = if at { LINK } else { TEXT_2 };
                    let a = painter.layout_no_wrap("Service".to_owned(), egui::FontId::new(13.0, medium()), text);
                    painter.galley(egui::pos2(rect.left() + 11.0, rect.top() + 28.0), a, text);
                    let b = painter.layout_no_wrap(format!("{minutes} min"), mono(11.0), TEXT_3);
                    painter.galley(egui::pos2(rect.left() + 11.0, rect.top() + 49.0), b, TEXT_3);
                    if at && scroll {
                        response.scroll_to_me(Some(Align::Center));
                    }
                }
            }
        });
        ui.add_space(4.0);
    });
    picked
}

/// The penalties and the total of the rally's stages driven, under the strip; nothing before a stage is driven.
pub(super) fn totals_ui(ui: &mut Ui, event: &Event, rally: usize) {
    let results = driven(event, rally);
    if results.is_empty() {
        return;
    }
    let penalties: f32 = results.iter().filter_map(|(p, _)| p.as_ref()).map(|p| p.penalties).sum();
    let driven = results.iter().filter_map(|(p, _)| p.as_ref()).filter(|p| !p.retired).map(|p| p.time + p.penalties);
    let charges = (0..results.len()).filter_map(|k| event.charged(rally, k, None));
    let total: f32 = driven.chain(charges).sum();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Your total").size(12.0).color(TEXT_3));
        ui.label(RichText::new(clock(total)).font(mono(12.0)).color(TEXT_2));
        if penalties > 0.0 {
            ui.label(RichText::new(format!("· penalties {}", clock(penalties))).font(mono(12.0)).color(BAD));
        }
    });
}

/// The standings panel: its caption with the Overall / stage-times toggle, the stage's result lines, then the table.
/// `stage`: the stage the toggle's times are of (the one picked, or the last driven); `times` the toggle's state;
/// whether "Latest" was clicked.
pub(super) fn standings_ui(
    ui: &mut Ui,
    event: &Event,
    rally: usize,
    lines: &[Standing],
    cars: &[Car],
    stage: Option<(usize, bool)>,
    times: &mut bool,
) -> bool {
    let ran = driven(event, rally).len();
    let count = event.rallies.get(rally).map_or(0, |r| r.stages.len());
    let times_of = stage.filter(|_| *times && ran > 0);
    let mut latest = false;
    ui.horizontal(|ui| {
        let caption = match (times_of, ran) {
            (Some((s, _)), _) => format!("SS{} times", s + 1),
            (None, 0) => "Rally standings".to_owned(),
            (None, n) if n == count => "Final standings".to_owned(),
            (None, n) => format!("Standings after SS{n}"),
        };
        ui.label(RichText::new(caption).size(12.0).family(medium()).color(TEXT_2));
        if let Some((s, _)) = stage.filter(|_| ran > 0) {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let options = [(false, "Overall".to_owned()), (true, format!("SS{} times", s + 1))];
                let options: Vec<(bool, &str)> = options.iter().map(|(v, l)| (*v, l.as_str())).collect();
                kit::segmented(ui, "championship-standings-mode", times, &options);
            });
        }
    });
    ui.add_space(2.0);
    if let Some((s, picked)) = stage.filter(|_| ran > 0) {
        latest = stage_summary(ui, event, rally, s, picked);
        ui.add_space(4.0);
    }
    if let Some((s, _)) = times_of {
        stage_times_ui(ui, event, rally, s);
        return latest;
    }
    if ran == 0 {
        card(ui, |ui| {
            _ = ui.colored_label(GREY, format!("No stage run yet. {} drivers start.", event.field.len() + 1))
        });
        return latest;
    }
    let best = lines.iter().filter_map(|l| l.total).fold(f32::INFINITY, f32::min);
    let points = ran == count;
    let power = points && event.config.power_stage;
    let stage = (!points).then(|| format!("SS{ran}"));
    let before = (ran > 1).then(|| event.standings_after(rally, ran - 1));
    let mut cols = vec![Col::new("", 28.0, false), Col::new("", 38.0, false), Col::new("Driver", 0.0, false)];
    if let Some(stage) = &stage {
        cols.push(Col::new(stage, 72.0, true));
    }
    cols.extend([Col::new("Total", 80.0, true), Col::new("Gap", 66.0, true)]);
    if power {
        cols.push(Col::new("PS", 30.0, true));
    }
    if points {
        cols.push(Col::new("Pts", 34.0, true));
    }
    let rows = lines
        .iter()
        .enumerate()
        .map(|(at, line)| {
            let car =
                line.entrant.and_then(|e| event.field.get(e)).map_or(event.config.car.as_str(), |e| e.car.as_str());
            let mut cells = vec![Cell::num(if line.total.is_some() { format!("{}", at + 1) } else { "–".into() }, DIM)];
            cells.push(match before.as_ref().and_then(|b| place_change(b, lines, line.entrant)) {
                Some(n) if n > 0 => Cell::num(format!("▲ {n}"), GOOD),
                Some(n) if n < 0 => Cell::num(format!("▼ {}", -n), BAD),
                _ => Cell::num("", DIM),
            });
            cells.push(Cell::text(driver(event, line.entrant), if line.entrant.is_none() { TEXT } else { TEXT_LIST }));
            if stage.is_some() {
                let time = stage_time(event, rally, ran - 1, line.entrant);
                cells.push(Cell::num(time.map_or("–".to_owned(), clock), GREY));
            }
            match line.total {
                Some(total) => {
                    cells.push(Cell::num(clock(total), TEXT));
                    cells.push(Cell::num(gap(total, best), GREY));
                }
                None => {
                    cells.push(Cell::text("retired", GREY));
                    cells.push(Cell::num("", GREY));
                }
            }
            if power {
                cells.push(Cell::num(if line.bonus > 0 { format!("+{}", line.bonus) } else { String::new() }, STAR));
            }
            if points {
                cells.push(Cell::num(if line.points > 0 { line.points.to_string() } else { String::new() }, TEXT));
            }
            Line { cells, you: line.entrant.is_none(), tip: Some(car_name(cars, car)) }
        })
        .collect();
    table(ui, "championship-standings", &cols, rows, 460.0);
    latest
}

/// The season's points: each driver's points from each rally (R: retired), and the total.
pub(super) fn points_ui(ui: &mut Ui, event: &Event, lines: &[Standing]) {
    card(ui, |ui| {
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
        egui::Grid::new("championship-calendar").spacing([8.0, 4.0]).show(ui, |ui| {
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
                let (surfaces, length) = surfaces_and_length(&r.stages);
                let stages = format!("{} stages", r.stages.len());
                ui.colored_label(GREY, format!("{surfaces} · {}", km(length))).on_hover_text(stages);
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
