//! What the championship pages draw with: cards, places and times, and the service park (`CSelectPartsUI` and the timer) on
//! the car's repair tree. The park's rules and state are `rbr_season::service::ServicePark`'s.

use std::path::Path;

use egui::{Color32, RichText, Ui};
use rbr_formats::data::Data;
use rbr_formats::strings::Strings;
use rbr_gameplay::car_damage::{CarriedDamage, Rules};
use rbr_season::service::{self, ParkState, ServicePark};

use crate::browser::{DIM, GREY, STAR};
use crate::catalog::Car;
use crate::pages::start_button;

/// A severe or lost part.
pub(crate) const BAD: Color32 = Color32::from_rgb(225, 95, 80);
/// A card's fill: a shade above the page's.
pub(super) const CARD: Color32 = Color32::from_gray(32);

/// Content in a card: the fill, rounded corners and a margin, the full width.
pub(crate) fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
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
pub(crate) fn card_title(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).size(18.0).strong());
    ui.add_space(2.0);
}

/// Where a stage stands in a rally: its time once driven, retired, the next, or to come.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum StageMark {
    Done(f32),
    Retired,
    Next,
    Later,
}

/// `n` as a place: 1st, 2nd, 3rd, 4th, 11th.
pub(crate) fn ordinal(n: usize) -> String {
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
pub(crate) fn gap(time: f32, best: f32) -> String {
    if time <= best {
        String::new()
    } else {
        format!("+{}", clock(time - best))
    }
}

/// `seconds` as `M:SS.ss`.
pub(crate) fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0);
    format!("{}:{:05.2}", (s / 60.0) as u32, s % 60.0)
}

/// A string-table text by key, the key itself when the table or the key is missing.
pub(crate) fn text_of(strings: Option<&Strings>, key: &str) -> String {
    strings.and_then(|s| s.get(key)).unwrap_or(key).to_owned()
}

/// The car's repair tree and rules ((`ChampionshipPage::open_park`)): under NGP (`ngp`: its `physics.rbz`) an RSF car's own
/// `repair.ini`, else the archive's slot tree, with NGP's rules; without NGP the retail one with 1.02's.
pub(crate) fn repair_tree(
    data: &Data,
    car: &Car,
    ngp: Option<&Path>,
) -> (Result<Vec<rbr_formats::repair::Part>, String>, Rules) {
    match ngp {
        Some(archive) => {
            let own =
                car.pack().and_then(|path| service::repair_parts_of(&rbr_formats::car_pack::Pack::open(path).ok()?));
            (own.unwrap_or_else(|| service::repair_parts_in(archive, car.slot as usize)), Rules::Ngp)
        }
        None => (service::repair_parts(data, car.slot as usize), Rules::V102),
    }
}

/// The park's primary button: Repair! with orders starts the repairs; without, Leave service closes the park
/// unrepaired (`forfeit`, as the game's `MSG_NOREP` Yes; ours: not asked) and returns its penalty and the car.
fn park_go(park: &mut ServicePark) -> Option<(f32, CarriedDamage)> {
    if park.start() {
        return None;
    }
    park.forfeit();
    match park.state {
        ParkState::Closed { penalty, .. } => Some((penalty, park.damage.clone())),
        _ => None,
    }
}

/// The park, as `CSelectPartsUI` and the timer show it: the allowed and estimated times, the damaged parts to order,
/// the manager's advice, Repair! (with no orders Leave service, without the game's `MSG_NOREP` question), selecting
/// during the repairs to finish them now, and the closing message; `setup` set by its Setup… button. Returns the
/// penalty and the car when closed and confirmed, or at once on Leave service.
pub(crate) fn park_card(
    ui: &mut Ui,
    park: &mut ServicePark,
    strings: Option<&Strings>,
    title: &str,
    budget: f32,
    setup: &mut bool,
) -> Option<(f32, CarriedDamage)> {
    let text = |key: &str| text_of(strings, key);
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    park.tick(dt);
    if !matches!(park.state, ParkState::Closed { .. }) {
        ui.ctx().request_repaint();
    }
    card_title(ui, title);
    // The clock that matters first, big; what the orders take beside it, red past the time allowed.
    let over = park.estimate() > park.time_left;
    ui.horizontal(|ui| {
        ui.colored_label(GREY, "Time left");
        ui.label(RichText::new(clock(park.time_left)).size(20.0).strong());
        ui.colored_label(DIM, format!("of {}", clock(budget)));
        ui.add_space(14.0);
        ui.colored_label(GREY, text("SP_ESTIM").trim_end_matches([':', ' ']));
        ui.label(RichText::new(clock(park.estimate())).strong().color(if over { BAD } else { Color32::WHITE }));
    });
    if over && park.state == ParkState::Choosing {
        ui.colored_label(BAD, "The orders take longer than the time left: expect a late penalty.");
    }
    ui.add_space(6.0);
    let choosing = park.state == ParkState::Choosing;
    let rows = park.rows();
    if rows.is_empty() {
        ui.colored_label(GREY, "Nothing is damaged.");
    } else if choosing {
        ui.colored_label(GREY, "Tick the parts to repair. With none ticked, the car leaves as it is.");
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
    let (mut done, mut go) = (None, false);
    ui.horizontal(|ui| match park.state {
        ParkState::Choosing => {
            let label = if park.orders.is_empty() { "Leave service".to_owned() } else { text("SP_REPAIR") };
            go = ui.add(start_button(&label)).clicked();
            if ui.button(text("SP_MANADV")).on_hover_text("Order what the team manager would").clicked() {
                park.manager_advice();
            }
            if ui.button("Setup…").clicked() {
                *setup = true;
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
    if go {
        done = park_go(park);
    }
    done
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
