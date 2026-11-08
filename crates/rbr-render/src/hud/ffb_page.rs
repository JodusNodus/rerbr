//! The pause menu's Force feedback page, ours (`crate::hud::pause`): a panel at the bottom of the screen over the
//! paused view (`crate::ffb::Session`). Its rows: this car's NGP sensitivity on the drive's surface, the wheel's
//! strength (every car's), Reset (the car back to the sensitivities every car takes) and Done. Under them a graph of
//! NGP's curve at the sensitivity shown over the player's steering load with this car on this surface
//! (`crate::ffb::load`), the region where the wheel maxes out shaded, how often it would max out, and a suggested
//! sensitivity that is only a hint. Up and Down pick a row, Left and Right change the value by [`STEP`]; the select
//! acts on Reset and Done, and the back returns to the menu's first page. A change takes effect on the paused drive
//! (`crate::ffb::reshape`, `Wheel::set_strength`) and is kept.

use super::camera_page::Moved;
use super::pause::Nav;
use crate::ffb::load::{self, Load};
use crate::ffb::{Session, SENSITIVITY_RANGE, SURFACES};
use egui::{Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, Vec2};
use std::ops::RangeInclusive;

const PANEL: Color32 = Color32::from_rgba_premultiplied(10, 10, 12, 220);
const TEXT: Color32 = Color32::from_gray(235);
const HINT: Color32 = Color32::from_gray(160);
const SELECTED: Color32 = Color32::from_rgb(200, 60, 40);
const BARS: Color32 = Color32::from_rgba_premultiplied(90, 110, 140, 200);
const MAXED: Color32 = Color32::from_rgba_premultiplied(70, 16, 12, 110);
const WIDTH: f32 = 420.0;
const ROW: f32 = 26.0;
/// The graph's height, pixels.
const GRAPH: f32 = 120.0;

/// One step of the sensitivity and the strength, percent.
pub const STEP: i32 = 5;
/// The strength's range, percent.
const STRENGTH: RangeInclusive<i32> = 0..=100;

/// A row of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Sensitivity,
    Strength,
    Reset,
    Done,
}

const ROWS: [Row; 4] = [Row::Sensitivity, Row::Strength, Row::Reset, Row::Done];

/// The page: the drive's surface (0 gravel, 1 tarmac, 2 snow), the car's sensitivity on it, whether the car has its
/// own, the one every car takes, the strength, the player's load with the car on the surface, and the selected row.
#[derive(Debug, Clone, PartialEq)]
pub struct FfbPage {
    pub surface: usize,
    pub value: i32,
    pub own: bool,
    global: i32,
    /// The wheel's strength, percent.
    pub strength: i32,
    load: Load,
    row: usize,
}

impl FfbPage {
    /// A page at full strength with no load tracked.
    pub fn new(surface: usize, value: i32, own: bool, global: i32) -> FfbPage {
        FfbPage { surface, value, own, global, strength: 100, load: Load::default(), row: 0 }
    }

    /// The page for the drive's car on `surface` (FFB1's: 0 gravel, 1 tarmac, 2 snow).
    pub fn of(session: &Session, surface: u32) -> FfbPage {
        let surface = surface.min(2) as usize;
        let page = FfbPage::new(surface, session.now()[surface], session.own(), session.global[surface]);
        FfbPage { strength: (session.strength * 100.0).round() as i32, load: session.load_on(surface), ..page }
    }

    /// The selected row.
    pub fn row(&self) -> Row {
        ROWS[self.row]
    }

    /// Takes one move.
    pub fn nav(&mut self, nav: Nav) -> Option<Moved> {
        let n = ROWS.len();
        let by = if nav == Nav::Right { STEP } else { -STEP };
        match (nav, self.row()) {
            (Nav::Up, _) => self.row = (self.row + n - 1) % n,
            (Nav::Down, _) => self.row = (self.row + 1) % n,
            (Nav::Back, _) | (Nav::Select, Row::Done) => return Some(Moved::Left),
            (Nav::Select, Row::Reset) => {
                let changed = self.own;
                (self.value, self.own) = (self.global, false);
                return changed.then_some(Moved::Changed);
            }
            (Nav::Left | Nav::Right, Row::Sensitivity) => {
                let value = (self.value + by).clamp(*SENSITIVITY_RANGE.start(), *SENSITIVITY_RANGE.end());
                if value == self.value {
                    return None;
                }
                (self.value, self.own) = (value, true);
                return Some(Moved::Changed);
            }
            (Nav::Left | Nav::Right, Row::Strength) => {
                let value = (self.strength + by).clamp(*STRENGTH.start(), *STRENGTH.end());
                if value == self.strength {
                    return None;
                }
                self.strength = value;
                return Some(Moved::Changed);
            }
            _ => {}
        }
        None
    }
}

fn label(page: &FfbPage, row: Row) -> String {
    match row {
        Row::Sensitivity => format!("Sensitivity, {}", SURFACES[page.surface.min(2)]),
        Row::Strength => "Strength, every car".into(),
        Row::Reset => "Reset to every car's".into(),
        Row::Done => "Done".into(),
    }
}

fn value(page: &FfbPage, row: Row) -> String {
    match row {
        Row::Sensitivity => format!("{} %{}", page.value, if page.own { "" } else { " (every car's)" }),
        Row::Strength => format!("{} %", page.strength),
        Row::Reset | Row::Done => String::new(),
    }
}

/// The lines under the graph: how often the wheel maxes out at the sensitivity shown, and the suggestion.
fn notes(page: &FfbPage) -> [String; 2] {
    let surface = SURFACES[page.surface.min(2)];
    match page.load.maxed_out(page.value) {
        Some(share) => [
            format!("Maxed out {} % of your driving (this car, {surface})", (share * 100.0).round() as i32),
            load::suggested(&page.load, SENSITIVITY_RANGE)
                .map_or(String::new(), |p| format!("Suggested {p} % (your usual cornering at half force)")),
        ],
        None => [format!("No driving tracked yet (this car, {surface})"), String::new()],
    }
}

/// The graph's ratio range: past where the wheel maxes out at `p`, and past the load's last bin, within the bins.
fn x_range(page: &FfbPage) -> f32 {
    let maxed = load::ratio_at(load::MAXED, page.value);
    let last = page.load.tenths.iter().rposition(|t| *t > 0.0).map_or(0.0, |k| (k + 1) as f32 * load::BIN);
    (maxed * 1.4).max(last * 1.05).clamp(20.0, load::BINS as f32 * load::BIN)
}

/// Draws the graph into `rect`: the load's bars, the maxed-out region and NGP's curve at the sensitivity shown.
fn graph(painter: &egui::Painter, rect: Rect, page: &FfbPage) {
    let x_max = x_range(page);
    let at =
        |x: f32, f: f32| Pos2::new(rect.min.x + rect.width() * (x / x_max).min(1.0), rect.max.y - rect.height() * f);
    let maxed = load::ratio_at(load::MAXED, page.value);
    if maxed < x_max {
        painter.rect_filled(Rect::from_min_max(at(maxed, 1.0), rect.max), 0.0, MAXED);
        let small = FontId::proportional(11.0);
        painter.text(at(maxed, 1.0) + Vec2::new(4.0, 2.0), Align2::LEFT_TOP, "maxed out", small, HINT);
    }
    let top = page.load.tenths.iter().copied().fold(0.0, f32::max);
    for (k, t) in page.load.tenths.iter().enumerate().filter(|(_, t)| **t > 0.0) {
        let x0 = k as f32 * load::BIN;
        if x0 >= x_max {
            break;
        }
        let bar = Rect::from_min_max(at(x0, 0.85 * t / top), at(x0 + load::BIN, 0.0));
        painter.rect_filled(bar.shrink2(Vec2::new(0.5, 0.0)), 0.0, BARS);
    }
    let curve: Vec<Pos2> =
        (0..=80).map(|k| k as f32 * x_max / 80.0).map(|x| at(x, load::force(x, page.value))).collect();
    painter.add(Shape::line(curve, Stroke::new(2.0, TEXT)));
    painter.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, HINT));
    painter.line_segment([rect.left_bottom(), rect.left_top()], Stroke::new(1.0, HINT));
    let small = FontId::proportional(11.0);
    painter.text(rect.left_top() + Vec2::new(4.0, 0.0), Align2::LEFT_TOP, "wheel force 100 %", small.clone(), HINT);
    painter.text(rect.right_bottom() + Vec2::new(0.0, 2.0), Align2::RIGHT_TOP, "steering load", small.clone(), HINT);
    painter.text(rect.left_bottom() + Vec2::new(0.0, 2.0), Align2::LEFT_TOP, "bars: your driving", small, HINT);
}

/// Draws the page at the bottom of the screen, over the undimmed view.
pub fn paint(ctx: &egui::Context, page: &FfbPage) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("ffb page")));
    let screen = ctx.content_rect();
    let size = Vec2::new(WIDTH, 80.0 + ROW * ROWS.len() as f32 + GRAPH + 22.0 + 2.0 * 18.0);
    let panel = Rect::from_min_size(Pos2::new(screen.center().x - WIDTH / 2.0, screen.max.y - size.y - 16.0), size);
    painter.rect_filled(panel, 8.0, PANEL);
    let mut at = Pos2::new(panel.min.x + 20.0, panel.min.y + 10.0);
    let title = "Force feedback";
    painter.text(Pos2::new(panel.center().x, at.y), Align2::CENTER_TOP, title, FontId::proportional(18.0), TEXT);
    at.y += 28.0;
    let font = FontId::proportional(16.0);
    for row in ROWS {
        let line = Rect::from_min_size(at, Vec2::new(WIDTH - 40.0, ROW - 4.0));
        let selected = row == page.row();
        if selected {
            painter.rect_filled(line, 4.0, SELECTED);
        }
        let left = line.left_center() + Vec2::new(8.0, 0.0);
        painter.text(left, Align2::LEFT_CENTER, label(page, row), font.clone(), TEXT);
        let value = value(page, row);
        if !value.is_empty() {
            let value = if selected { format!("<  {value}  >") } else { value };
            painter.text(line.right_center() - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, value, font.clone(), TEXT);
        }
        at.y += ROW;
    }
    at.y += 6.0;
    graph(&painter, Rect::from_min_size(at, Vec2::new(WIDTH - 40.0, GRAPH)), page);
    at.y += GRAPH + 18.0;
    for note in notes(page) {
        painter.text(at, Align2::LEFT_TOP, note, FontId::proportional(14.0), TEXT);
        at.y += 18.0;
    }
    let hints = ["Above 100 lighter, below 100 stronger", "Up/Down choose    Left/Right change    Esc back"];
    for (k, hint) in hints.into_iter().enumerate() {
        let at = Pos2::new(panel.center().x, panel.max.y - 8.0 - 16.0 * (1 - k) as f32);
        painter.text(at, Align2::CENTER_BOTTOM, hint, FontId::proportional(12.0), HINT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffb::Options;

    #[test]
    fn the_sensitivity_steps_within_its_range_and_resets() {
        let mut p = FfbPage::new(0, 280, false, 280);
        assert_eq!(
            (label(&p, Row::Sensitivity), value(&p, Row::Sensitivity)),
            ("Sensitivity, gravel".into(), "280 % (every car's)".into())
        );
        assert_eq!(p.nav(Nav::Right), Some(Moved::Changed));
        assert_eq!((p.value, p.own), (285, true));
        for _ in 0..100 {
            p.nav(Nav::Right);
        }
        assert_eq!(p.value, *SENSITIVITY_RANGE.end());
        assert_eq!(p.nav(Nav::Right), None);
        // Reset: every car's again, once.
        p.nav(Nav::Down);
        p.nav(Nav::Down);
        assert_eq!(p.nav(Nav::Select), Some(Moved::Changed));
        assert_eq!((p.value, p.own), (280, false));
        assert_eq!(p.nav(Nav::Select), None);
        // Done and the back leave; Up from the top wraps to Done.
        assert_eq!(p.nav(Nav::Down), None);
        assert_eq!(p.nav(Nav::Select), Some(Moved::Left));
        let mut p = FfbPage::new(2, 15, true, 100);
        assert_eq!(p.nav(Nav::Left), Some(Moved::Changed));
        assert_eq!(p.nav(Nav::Left), None, "at the range's start");
        assert_eq!(p.nav(Nav::Up), None);
        assert_eq!(p.row(), Row::Done);
        assert_eq!(p.nav(Nav::Back), Some(Moved::Left));
    }

    #[test]
    fn the_strength_steps_from_none_to_full() {
        let mut p = FfbPage::new(0, 280, false, 280);
        p.nav(Nav::Down);
        assert_eq!((p.row(), value(&p, Row::Strength)), (Row::Strength, "100 %".into()));
        assert_eq!(p.nav(Nav::Right), None);
        assert_eq!(p.nav(Nav::Left), Some(Moved::Changed));
        assert_eq!((p.strength, p.value, p.own), (95, 280, false));
        for _ in 0..30 {
            p.nav(Nav::Left);
        }
        assert_eq!(p.strength, 0);
    }

    #[test]
    fn the_notes_follow_the_sensitivity_shown() {
        let mut session = Session::load(None, Some("rsf-81".into()), &Options::default(), [100; 3]);
        let page = FfbPage::of(&session, 1);
        assert_eq!(notes(&page), ["No driving tracked yet (this car, tarmac)".to_owned(), String::new()]);
        let mut stored = Load::default();
        stored.add(40.0, 80.0);
        session.loads.cars.insert(("rsf-81".into(), 1), stored);
        // This drive's frames count too: x 142.5 maxes out at 100 %, not at 200 %.
        for _ in 0..20 {
            session.frame(1, load::force(141.0, 100), 1.0, true);
        }
        let mut page = FfbPage::of(&session, 1);
        assert_eq!(
            notes(&page),
            [
                "Maxed out 20 % of your driving (this car, tarmac)".to_owned(),
                "Suggested 500 % (your usual cornering at half force)".to_owned()
            ]
        );
        page.value = 200;
        assert_eq!(notes(&page)[0], "Maxed out 0 % of your driving (this car, tarmac)");
        assert!(x_range(&page) > load::ratio_at(load::MAXED, 200));
    }
}
