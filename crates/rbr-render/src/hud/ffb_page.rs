//! The pause menu's Force feedback page, ours (`crate::hud::pause`): a panel at the bottom of the screen over the
//! paused view (`crate::ffb::Session`). Its rows: this car's sensitivity on the drive's surface (manual mode), or its
//! trim of the automatic one (automatic mode, `crate::ffb::auto`: the automatic percentage shown, or the seconds
//! learned so far), the wheel's strength (every car's), then a line saying how often the wheel maxed out in the last
//! minute, Reset (the car back to every car's sensitivities, or the trim to 100) and Done. Up and Down pick a row,
//! Left and Right change the value by [`STEP`]; the select acts on Reset and Done, and the back returns to the
//! menu's first page. A change takes effect on the paused drive (`crate::ffb::reshape`, `Wheel::set_strength`) and
//! is kept.

use super::camera_page::Moved;
use super::pause::Nav;
use crate::ffb::{auto, Session, SENSITIVITY_RANGE, SURFACES};
use egui::{Align2, Color32, FontId, Pos2, Rect, Vec2};
use std::ops::RangeInclusive;

const PANEL: Color32 = Color32::from_rgba_premultiplied(10, 10, 12, 220);
const TEXT: Color32 = Color32::from_gray(235);
const HINT: Color32 = Color32::from_gray(160);
const SELECTED: Color32 = Color32::from_rgb(200, 60, 40);
const WIDTH: f32 = 420.0;
const ROW: f32 = 26.0;

/// One step of the sensitivity, the trim and the strength, percent.
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

/// What the automatic mode knows of the car on the surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Auto {
    /// Its reference ratio (`auto::Reference::x`).
    Reference(f32),
    /// The kept seconds so far, learning.
    Learning(f32),
}

/// The page: the drive's surface (0 gravel, 1 tarmac, 2 snow), the car's sensitivity on it (manual) or its trim
/// (automatic), whether that is the car's own (a trim not 100), what Reset puts back, the strength, the automatic
/// mode's state, the share of the last minute the wheel maxed out, and the selected row.
#[derive(Debug, Clone, PartialEq)]
pub struct FfbPage {
    pub surface: usize,
    pub value: i32,
    pub own: bool,
    global: i32,
    /// The wheel's strength, percent.
    pub strength: i32,
    pub auto: Option<Auto>,
    clipped: Option<f32>,
    row: usize,
}

impl FfbPage {
    /// A manual page at full strength.
    pub fn new(surface: usize, value: i32, own: bool, global: i32) -> FfbPage {
        FfbPage { surface, value, own, global, strength: 100, auto: None, clipped: None, row: 0 }
    }

    /// The page for the drive's car on `surface` (FFB1's: 0 gravel, 1 tarmac, 2 snow).
    pub fn of(session: &Session, surface: u32) -> FfbPage {
        let surface = surface.min(2) as usize;
        let strength = (session.strength * 100.0).round() as i32;
        let clipped = session.clipped();
        if !session.auto {
            let page = FfbPage::new(surface, session.now()[surface], session.own(), session.global[surface]);
            return FfbPage { strength, clipped, ..page };
        }
        let trim = session.trim(surface);
        let auto = match session.reference(surface) {
            Some(r) => Auto::Reference(r.x),
            None => Auto::Learning(session.learning(surface).unwrap_or(0.0)),
        };
        FfbPage { auto: Some(auto), strength, clipped, ..FfbPage::new(surface, trim, trim != 100, 100) }
    }

    /// The selected row.
    pub fn row(&self) -> Row {
        ROWS[self.row]
    }

    /// The sensitivity's range, or the trim's.
    fn range(&self) -> RangeInclusive<i32> {
        if self.auto.is_some() {
            auto::TRIM_RANGE
        } else {
            SENSITIVITY_RANGE
        }
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
                let range = self.range();
                let value = (self.value + by).clamp(*range.start(), *range.end());
                if value == self.value {
                    return None;
                }
                self.value = value;
                self.own = self.auto.is_none() || value != 100;
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
        Row::Sensitivity => {
            let surface = SURFACES[page.surface.min(2)];
            if page.auto.is_some() {
                format!("Sensitivity, {surface}, auto")
            } else {
                format!("Sensitivity, {surface}")
            }
        }
        Row::Strength => "Strength, every car".into(),
        Row::Reset if page.auto.is_some() => "Reset the trim".into(),
        Row::Reset => "Reset to every car's".into(),
        Row::Done => "Done".into(),
    }
}

fn value(page: &FfbPage, row: Row) -> String {
    let trim = if page.own { format!(" (trim {} %)", page.value) } else { String::new() };
    match (row, page.auto) {
        (Row::Sensitivity, None) => format!("{} %{}", page.value, if page.own { "" } else { " (every car's)" }),
        (Row::Sensitivity, Some(Auto::Reference(x))) => format!("{} %{trim}", auto::effective(x, page.value)),
        (Row::Sensitivity, Some(Auto::Learning(seconds))) => {
            format!("learning {}/{} s{trim}", seconds.floor() as i32, auto::LEARN_SECONDS as i32)
        }
        (Row::Strength, _) => format!("{} %", page.strength),
        (Row::Reset | Row::Done, _) => String::new(),
    }
}

/// The clipping line.
fn clipped(page: &FfbPage) -> String {
    match page.clipped {
        Some(share) => format!("Wheel maxed out: {} % of the last minute", (share * 100.0).round() as i32),
        None => "Wheel maxed out: no driving yet".into(),
    }
}

/// Draws the page at the bottom of the screen, over the undimmed view.
pub fn paint(ctx: &egui::Context, page: &FfbPage) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("ffb page")));
    let screen = ctx.content_rect();
    let size = Vec2::new(WIDTH, 80.0 + ROW * (ROWS.len() + 1) as f32);
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
        if row == Row::Strength {
            let line = Rect::from_min_size(at, Vec2::new(WIDTH - 40.0, ROW - 4.0));
            let left = line.left_center() + Vec2::new(8.0, 0.0);
            painter.text(left, Align2::LEFT_CENTER, clipped(page), FontId::proportional(14.0), HINT);
            at.y += ROW;
        }
    }
    let hints = ["Higher sensitivity is lighter, lower stronger", "Up/Down choose    Left/Right change    Esc back"];
    for (k, hint) in hints.into_iter().enumerate() {
        let at = Pos2::new(panel.center().x, panel.max.y - 8.0 - 16.0 * (1 - k) as f32);
        painter.text(at, Align2::CENTER_BOTTOM, hint, FontId::proportional(12.0), HINT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffb::{auto::Reference, Options};

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
    fn in_automatic_mode_the_row_is_the_trim() {
        let options = Options { sensitivity: [280; 3], ..Options::default() };
        let mut session = Session::load(None, Some("rsf-81".into()), &options, [280; 3]);
        let mut p = FfbPage::of(&session, 1);
        assert_eq!(p.auto, Some(Auto::Learning(0.0)));
        assert_eq!(
            (label(&p, Row::Sensitivity), value(&p, Row::Sensitivity), clipped(&p)),
            ("Sensitivity, tarmac, auto".into(), "learning 0/60 s".into(), "Wheel maxed out: no driving yet".into())
        );
        assert_eq!(p.nav(Nav::Right), Some(Moved::Changed));
        assert_eq!((p.value, p.own), (105, true));
        assert_eq!(value(&p, Row::Sensitivity), "learning 0/60 s (trim 105 %)");
        session.autos.set_reference("rsf-81", 1, Reference { x: 122.0, seconds: 600.0 });
        session.frame(1, 0.97, 1.0, true);
        session.frame(1, 0.2, 3.0, true);
        let mut p = FfbPage::of(&session, 1);
        assert_eq!(
            (value(&p, Row::Sensitivity), clipped(&p)),
            ("446 %".into(), "Wheel maxed out: 0 % of the last minute".into())
        );
        p.nav(Nav::Left);
        assert_eq!(value(&p, Row::Sensitivity), format!("{} % (trim 95 %)", auto::effective(122.0, 95)));
        for _ in 0..20 {
            p.nav(Nav::Left);
        }
        assert_eq!(p.value, *auto::TRIM_RANGE.start());
        // Reset: the trim back to 100.
        p.nav(Nav::Down);
        p.nav(Nav::Down);
        assert_eq!(label(&p, Row::Reset), "Reset the trim");
        assert_eq!(p.nav(Nav::Select), Some(Moved::Changed));
        assert_eq!((p.value, p.own), (100, false));
    }
}
