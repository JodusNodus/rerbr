//! The pause menu's Force feedback page, ours (`crate::hud::pause`): a panel at the bottom of the screen over the
//! paused view, which sets this car's NGP force-feedback sensitivity for the drive's surface (`crate::ffb::Session`).
//! Its rows: the sensitivity, Reset (the car back to the sensitivities every car takes) and Done. Up and Down pick a
//! row, Left and Right change the sensitivity by [`STEP`]; the select acts on Reset and Done, and the back returns to
//! the menu's first page. The change takes effect on the paused drive (`crate::ffb::reshape`) and is kept for the car.

use super::camera_page::Moved;
use super::pause::Nav;
use crate::ffb::{SENSITIVITY_RANGE, SURFACES};
use egui::{Align2, Color32, FontId, Pos2, Rect, Vec2};

const PANEL: Color32 = Color32::from_rgba_premultiplied(10, 10, 12, 220);
const TEXT: Color32 = Color32::from_gray(235);
const HINT: Color32 = Color32::from_gray(160);
const SELECTED: Color32 = Color32::from_rgb(200, 60, 40);
const WIDTH: f32 = 380.0;
const ROW: f32 = 26.0;

/// One step of the sensitivity, percent.
pub const STEP: i32 = 5;

/// A row of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Sensitivity,
    Reset,
    Done,
}

const ROWS: [Row; 3] = [Row::Sensitivity, Row::Reset, Row::Done];

/// The page: the drive's surface (0 gravel, 1 tarmac, 2 snow), the car's sensitivity on it, whether the car has its
/// own, the one every car takes, and the selected row.
#[derive(Debug, Clone, PartialEq)]
pub struct FfbPage {
    pub surface: usize,
    pub value: i32,
    pub own: bool,
    global: i32,
    row: usize,
}

impl FfbPage {
    pub fn new(surface: usize, value: i32, own: bool, global: i32) -> FfbPage {
        FfbPage { surface, value, own, global, row: 0 }
    }

    /// The selected row.
    pub fn row(&self) -> Row {
        ROWS[self.row]
    }

    /// Takes one move.
    pub fn nav(&mut self, nav: Nav) -> Option<Moved> {
        let n = ROWS.len();
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
                let by = if nav == Nav::Right { STEP } else { -STEP };
                let value = (self.value + by).clamp(*SENSITIVITY_RANGE.start(), *SENSITIVITY_RANGE.end());
                if value == self.value {
                    return None;
                }
                (self.value, self.own) = (value, true);
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
            format!("Sensitivity, {surface}")
        }
        Row::Reset => "Reset to every car's".into(),
        Row::Done => "Done".into(),
    }
}

fn value(page: &FfbPage, row: Row) -> String {
    match row {
        Row::Sensitivity => format!("{} %{}", page.value, if page.own { "" } else { " (every car's)" }),
        Row::Reset | Row::Done => String::new(),
    }
}

/// Draws the page at the bottom of the screen, over the undimmed view.
pub fn paint(ctx: &egui::Context, page: &FfbPage) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("ffb page")));
    let screen = ctx.content_rect();
    let size = Vec2::new(WIDTH, 80.0 + ROW * ROWS.len() as f32);
    let panel = Rect::from_min_size(Pos2::new(screen.center().x - WIDTH / 2.0, screen.max.y - size.y - 16.0), size);
    painter.rect_filled(panel, 8.0, PANEL);
    let mut at = Pos2::new(panel.min.x + 20.0, panel.min.y + 10.0);
    let title = "Force feedback, this car";
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
    let hints = ["Above 100 lighter, below 100 stronger", "Up/Down choose    Left/Right change    Esc back"];
    for (k, hint) in hints.into_iter().enumerate() {
        let at = Pos2::new(panel.center().x, panel.max.y - 8.0 - 16.0 * (1 - k) as f32);
        painter.text(at, Align2::CENTER_BOTTOM, hint, FontId::proportional(12.0), HINT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
